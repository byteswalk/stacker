//! Updates the Antigravity CLI where its official installer put it (`%LOCALAPPDATA%\agy\bin`),
//! from the same release manifest that installer and `agy update` read. The installer declines
//! to touch a copy that is already there and exits 0, and `agy update` can sit for minutes on
//! "Checking for updates", so neither tells whether anything changed.

use crate::agents::install::direct::desktop_download_agent;
use crate::agents::install::emit_progress;
use crate::agents::process::run_command_text;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MANIFEST_BASE: &str =
    "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests";
/// The CLI is tens of megabytes: anything small is an error page.
const MINIMUM_BINARY_BYTES: u64 = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Release {
    version: String,
    url: String,
    sha512: String,
}

fn platform() -> &'static str {
    let arch = std::env::var("PROCESSOR_ARCHITEW6432")
        .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
        .unwrap_or_default();
    if arch.eq_ignore_ascii_case("ARM64") {
        "windows_arm64"
    } else {
        "windows_amd64"
    }
}

/// A release the manifest names, taken only with a secure address and a full SHA-512.
fn release_of(manifest: &str) -> Option<Release> {
    let value = serde_json::from_str::<Value>(manifest).ok()?;
    let version = value.get("version")?.as_str()?.trim();
    let url = value.get("url")?.as_str()?.trim();
    let sha512 = value.get("sha512")?.as_str()?.trim().to_ascii_lowercase();
    let plain = !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    let digest = sha512.len() == 128 && sha512.chars().all(|c| c.is_ascii_hexdigit());
    (plain && digest && url.starts_with("https://")).then(|| Release {
        version: version.to_string(),
        url: url.to_string(),
        sha512,
    })
}

fn sha512_hex(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha512};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha512::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Whether `agy --version` names this version.
fn runs_as(program: &Path, version: &str) -> bool {
    run_command_text(
        program,
        &["--version"],
        "Antigravity CLI",
        Duration::from_secs(60),
    )
    .is_ok_and(|text| {
        text.split_whitespace()
            .any(|word| word.trim_start_matches('v') == version)
    })
}

fn fetch_release() -> Result<Release, String> {
    let proxy = crate::agents::net::stacker_proxy();
    let agent = desktop_download_agent(proxy.as_deref())?;
    let url = format!("{MANIFEST_BASE}/{}.json", platform());
    let text = agent
        .get(&url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("读取 Antigravity CLI 官方发布清单失败：{e}"))?
        .into_string()
        .map_err(|e| format!("读取 Antigravity CLI 官方发布清单失败：{e}"))?;
    release_of(&text).ok_or_else(|| "Antigravity CLI 官方发布清单格式不对".to_string())
}

fn download(release: &Release, window: &Option<tauri::Window>) -> Result<PathBuf, String> {
    let target = std::env::temp_dir().join(format!("stacker-agy-{}.exe", release.version));
    // The binary is hundreds of megabytes: one already here from a try that failed later on
    // is used again once it checks out.
    if target.is_file() && sha512_hex(&target).is_ok_and(|digest| digest == release.sha512) {
        emit_progress(window, "已下载过这个版本，校验通过，直接使用");
        return Ok(target);
    }
    emit_progress(
        window,
        format!("正在下载 Antigravity CLI {}…", release.version),
    );
    let proxy = crate::agents::net::stacker_proxy();
    let agent = desktop_download_agent(proxy.as_deref())?;
    crate::installer::download_file_candidates_with_agent(
        &agent,
        std::slice::from_ref(&release.url),
        &target,
        MINIMUM_BINARY_BYTES,
        |message| emit_progress(window, message),
    )?;
    crate::installer::wait_until_free(&target, Duration::from_secs(90), || {
        emit_progress(window, "安全软件正在检查刚下载的文件，稍等…");
    })?;
    emit_progress(window, "正在校验 SHA-512…");
    if sha512_hex(&target)? != release.sha512 {
        let _ = std::fs::remove_file(&target);
        return Err("下载的 Antigravity CLI 与官方公布的 SHA-512 不一致，已停止更新".into());
    }
    Ok(target)
}

/// Puts the new binary in the old one's place. A running `agy.exe` can be renamed though not
/// overwritten, so the old one is moved aside first and comes back if anything fails.
fn replace(program: &Path, fresh: &Path, version: &str) -> Result<(), String> {
    let aside = program.with_extension("exe.old");
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(program, &aside).map_err(|e| format!("无法移开旧版 agy.exe：{e}"))?;
    let put_back = || {
        let _ = std::fs::remove_file(program);
        let _ = std::fs::rename(&aside, program);
    };
    if let Err(error) = std::fs::copy(fresh, program) {
        put_back();
        return Err(format!("无法写入新版 agy.exe：{error}"));
    }
    if !runs_as(program, version) {
        put_back();
        return Err(format!(
            "新版 agy.exe 无法运行或版本不是 {version}，已换回原来的版本"
        ));
    }
    // Still in use when the old one was running; it goes next time.
    let _ = std::fs::remove_file(&aside);
    Ok(())
}

/// One update at a time: two would download into the same file and pull it from under each
/// other. A second request waits, then finds the first one's work done.
static UPDATING: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn update(program: &Path, window: &Option<tauri::Window>) -> Result<String, String> {
    let _one = UPDATING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    emit_progress(window, "正在读取 Antigravity CLI 官方发布清单…");
    let release = fetch_release()?;
    if runs_as(program, &release.version) {
        return Ok(format!("Antigravity CLI 已是最新版本 {}", release.version));
    }
    let fresh = download(&release, window)?;
    replace(program, &fresh, &release.version)?;
    // Kept on failure, so a retry does not download it again.
    let _ = std::fs::remove_file(&fresh);
    Ok(format!("Antigravity CLI 已更新至 {}", release.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "d0b00af82e3fd882d0b00af82e3fd882d0b00af82e3fd882d0b00af82e3fd882d0b00af82e3fd882d0b00af82e3fd882d0b00af82e3fd882d0b00af82e3fd882";

    #[test]
    fn a_release_needs_a_secure_address_and_a_full_sha512() {
        let good = format!(
            r#"{{"version":"1.5.2","url":"https://storage.googleapis.com/a/cli.exe","sha512":"{}"}}"#,
            SHA.to_uppercase()
        );
        assert_eq!(
            release_of(&good),
            Some(Release {
                version: "1.5.2".into(),
                url: "https://storage.googleapis.com/a/cli.exe".into(),
                sha512: SHA.into(),
            })
        );
        let plain_http =
            format!(r#"{{"version":"1.5.2","url":"http://x/cli.exe","sha512":"{SHA}"}}"#);
        assert_eq!(release_of(&plain_http), None);
        let short = r#"{"version":"1.5.2","url":"https://x/cli.exe","sha512":"abc"}"#;
        assert_eq!(release_of(short), None);
        let odd =
            format!(r#"{{"version":"1.5.2; rm","url":"https://x/cli.exe","sha512":"{SHA}"}}"#);
        assert_eq!(release_of(&odd), None);
    }

    #[test]
    fn sha512_is_written_in_hex_like_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.bin");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha512_hex(&file).unwrap(),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }
}
