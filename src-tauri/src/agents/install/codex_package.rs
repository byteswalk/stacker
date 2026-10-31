//! Updates Codex's standalone install from its official npm package, for when the official
//! installer cannot reach its update CDN. The package holds the same files as a standalone
//! release, so it goes where a release goes and `current` is switched to it; the previous
//! release stays, and a new one that does not start leaves the old one in place.

use crate::agents::install::direct::{desktop_download_agent, read_release_info, sha512_base64};
use crate::agents::install::emit_progress;
use crate::agents::process::run_command_text;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

const REGISTRIES: [&str; 2] = [
    "https://registry.npmjs.org",
    "https://registry.npmmirror.com",
];
const TARGET: &str = "x86_64-pc-windows-msvc";
const PACKAGE_TAG: &str = "win32-x64";
/// The package is hundreds of megabytes: anything small is an error page.
const MINIMUM_PACKAGE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Package {
    tarball: String,
    sha512: String,
}

fn latest_version(manifest: &str) -> Option<String> {
    let version = serde_json::from_str::<Value>(manifest).ok()?;
    let version = version.get("version")?.as_str()?;
    let plain = !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    plain.then(|| version.to_string())
}

/// The tarball's address and SHA-512 from a registry's reply; npm writes the digest as
/// `sha512-<base64>`, and a package without one is not used.
fn package_of(manifest: &str) -> Option<Package> {
    let dist = serde_json::from_str::<Value>(manifest).ok()?;
    let dist = dist.get("dist")?;
    let tarball = dist.get("tarball")?.as_str()?;
    let sha512 = dist.get("integrity")?.as_str()?.strip_prefix("sha512-")?;
    tarball.starts_with("https://").then(|| Package {
        tarball: tarball.to_string(),
        sha512: sha512.to_string(),
    })
}

fn release_name(version: &str) -> String {
    format!("{version}-{TARGET}")
}

fn standalone_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .ok_or("无法定位 Codex 目录")?;
    let dir = home.join("packages").join("standalone");
    if dir.join("releases").is_dir() {
        Ok(dir)
    } else {
        Err("没有找到 Codex 的独立安装目录".into())
    }
}

/// The newest version and its package, from the first registry that answers.
fn find_package() -> Result<(String, Vec<Package>), String> {
    let mut last = String::new();
    let mut found: Option<String> = None;
    let mut packages = Vec::new();
    for registry in REGISTRIES {
        let version = match &found {
            Some(version) => version.clone(),
            None => {
                let url = format!("{registry}/@openai/codex/latest");
                match read_release_info(&url).map(|text| latest_version(&text)) {
                    Ok(Some(version)) => version,
                    Ok(None) => {
                        last = "版本信息格式无法识别".into();
                        continue;
                    }
                    Err(error) => {
                        last = error;
                        continue;
                    }
                }
            }
        };
        let url = format!("{registry}/@openai/codex/{version}-{PACKAGE_TAG}");
        match read_release_info(&url).map(|text| package_of(&text)) {
            Ok(Some(package)) => {
                found = Some(version);
                packages.push(package);
            }
            Ok(None) => last = "安装包信息格式无法识别".into(),
            Err(error) => last = error,
        }
    }
    match found {
        Some(version) => Ok((version, packages)),
        None => Err(format!("无法读取 Codex 的 npm 官方包信息：{last}")),
    }
}

fn runs_as(program: &Path, version: &str) -> bool {
    let reply = run_command_text(
        program,
        &["--version"],
        "Codex CLI",
        Duration::from_secs(60),
    );
    reply.is_ok_and(|text| text.split_whitespace().any(|word| word == version))
}

fn download(
    version: &str,
    packages: &[Package],
    window: &Option<tauri::Window>,
) -> Result<PathBuf, String> {
    let target = std::env::temp_dir().join(format!("stacker-codex-{version}.tgz"));
    let proxy = crate::agents::net::stacker_proxy();
    let agent = desktop_download_agent(proxy.as_deref())?;
    let urls: Vec<String> = packages.iter().map(|p| p.tarball.clone()).collect();
    crate::installer::download_file_candidates_with_agent(
        &agent,
        &urls,
        &target,
        MINIMUM_PACKAGE_BYTES,
        |message| emit_progress(window, message),
    )?;
    emit_progress(window, "正在校验安装包…");
    let actual = sha512_base64(&target)?;
    if !packages.iter().any(|package| package.sha512 == actual) {
        let _ = std::fs::remove_file(&target);
        return Err("安装包与 npm 官方公布的 SHA-512 校验值不一致，已停止更新".into());
    }
    Ok(target)
}

/// Unpacks into the standalone folder itself, so moving the result into place is one rename.
fn unpack(tarball: &Path, standalone: &Path, version: &str) -> Result<PathBuf, String> {
    let work = standalone.join(format!(".unpack-{version}"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|error| format!("创建临时目录失败：{error}"))?;
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let tar = system.join("System32").join("tar.exe");
    run_command_text(
        &tar,
        &[
            "-xzf",
            &tarball.to_string_lossy(),
            "-C",
            &work.to_string_lossy(),
        ],
        "解压 Codex 安装包",
        Duration::from_secs(600),
    )?;
    let release = standalone.join("releases").join(release_name(version));
    let unpacked = work.join("package").join("vendor").join(TARGET);
    if !unpacked.join("bin").join("codex.exe").is_file() {
        let _ = std::fs::remove_dir_all(&work);
        return Err("安装包里没有找到 codex.exe".into());
    }
    let moved = std::fs::rename(&unpacked, &release);
    let _ = std::fs::remove_dir_all(&work);
    moved.map_err(|error| format!("放置新版本失败：{error}"))?;
    Ok(release)
}

fn junction(link: &Path, target: &Path) -> Result<(), String> {
    // mklink reads a forward slash as a switch, and CODEX_HOME may be written with them.
    let native = |path: &Path| path.to_string_lossy().replace('/', "\\");
    run_command_text(
        Path::new("cmd.exe"),
        &["/c", "mklink", "/J", &native(link), &native(target)],
        "切换 Codex 版本",
        Duration::from_secs(30),
    )
    .map(|_| ())
}

/// Points `current` at `release`, keeping the old link as `current.previous` for the way back.
fn switch_current(standalone: &Path, release: &Path) -> Result<(), String> {
    let current = standalone.join("current");
    let incoming = standalone.join("current.new");
    let previous = standalone.join("current.previous");
    let _ = std::fs::remove_dir(&incoming);
    junction(&incoming, release)?;
    let _ = std::fs::remove_dir(&previous);
    std::fs::rename(&current, &previous).map_err(|error| format!("切换版本失败：{error}"))?;
    std::fs::rename(&incoming, &current).map_err(|error| {
        let _ = std::fs::rename(&previous, &current);
        format!("切换版本失败：{error}")
    })
}

fn switch_back(standalone: &Path) {
    let current = standalone.join("current");
    let previous = standalone.join("current.previous");
    if previous.exists() {
        let _ = std::fs::remove_dir(&current);
        let _ = std::fs::rename(&previous, &current);
    }
}

pub(crate) fn update(program: &Path, window: &Option<tauri::Window>) -> Result<String, String> {
    let standalone = standalone_dir()?;
    emit_progress(window, "正在读取 Codex 的 npm 官方包信息…");
    let (version, packages) = find_package()?;
    let release = standalone.join("releases").join(release_name(&version));
    if !release.join("bin").join("codex.exe").is_file() {
        let tarball = download(&version, &packages, window)?;
        emit_progress(window, "正在解压安装包…");
        let unpacked = unpack(&tarball, &standalone, &version);
        let _ = std::fs::remove_file(&tarball);
        unpacked?;
    }
    if !runs_as(&release.join("bin").join("codex.exe"), &version) {
        return Err("新版本无法启动，已保留原版本".into());
    }
    emit_progress(window, format!("正在切换到 Codex {version}…"));
    switch_current(&standalone, &release)?;
    if !runs_as(program, &version) {
        switch_back(&standalone);
        return Err("切换后无法启动新版本，已退回原版本".into());
    }
    // What Codex's own updater records, so `codex doctor` and it agree with what is installed.
    let _ = std::fs::write(
        standalone.join("auto-update-version"),
        release_name(&version),
    );
    Ok(format!("Codex CLI 已通过 npm 官方包更新到 {version}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_version_is_read_and_anything_odd_refused() {
        assert_eq!(
            latest_version(r#"{"version":"0.166.0"}"#).as_deref(),
            Some("0.166.0")
        );
        assert_eq!(latest_version(r#"{"version":"../../x"}"#), None);
        assert_eq!(latest_version(r#"{"version":""}"#), None);
        assert_eq!(latest_version("not json"), None);
    }

    #[test]
    fn a_package_needs_a_secure_address_and_a_sha512() {
        let good = r#"{"dist":{"tarball":"https://r.example/c.tgz","integrity":"sha512-QUJD"}}"#;
        assert_eq!(
            package_of(good),
            Some(Package {
                tarball: "https://r.example/c.tgz".into(),
                sha512: "QUJD".into()
            })
        );
        assert_eq!(
            package_of(
                r#"{"dist":{"tarball":"http://r.example/c.tgz","integrity":"sha512-QUJD"}}"#
            ),
            None
        );
        assert_eq!(
            package_of(r#"{"dist":{"tarball":"https://r.example/c.tgz","integrity":"sha1-QUJD"}}"#),
            None
        );
        assert_eq!(package_of(r#"{"dist":{}}"#), None);
    }

    #[test]
    fn a_release_is_named_the_way_the_official_installer_names_it() {
        assert_eq!(release_name("0.166.0"), "0.166.0-x86_64-pc-windows-msvc");
    }

    #[test]
    fn switching_keeps_the_old_release_reachable_and_back_restores_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (old, new) = (root.join("releases/1"), root.join("releases/2"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("v"), "1").unwrap();
        std::fs::write(new.join("v"), "2").unwrap();
        junction(&root.join("current"), &old).unwrap();

        switch_current(root, &new).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("current/v")).unwrap(),
            "2"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("current.previous/v")).unwrap(),
            "1"
        );

        switch_back(root);
        assert_eq!(
            std::fs::read_to_string(root.join("current/v")).unwrap(),
            "1"
        );
        assert!(new.join("v").is_file(), "the release itself is left alone");
    }
}
