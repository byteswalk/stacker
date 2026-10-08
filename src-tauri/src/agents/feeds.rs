//! Vendors' own update services, read the way their desktop apps read them. Each one was
//! confirmed against the app's own update log before being added here; the parsers are pure
//! so the reply formats stay under test without touching the network.

use serde_json::Value;
use std::time::Duration;

/// An update service that answers for a given installed version: the update meant for it, or
/// nothing (HTTP 204) when it is current. WorkBuddy and Qoder both work this way, and roll out
/// in steps, so asking with any other version would name the wrong release.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Assignment {
    Update(String),
    Current,
}

fn agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(10));
    if let Some(proxy) = super::net::stacker_proxy() {
        if let Ok(proxy) = ureq::Proxy::new(&proxy) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

fn get(url: &str) -> Result<(u16, String), String> {
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(600 * attempt));
        }
        match agent().get(url).set("User-Agent", "Stacker").call() {
            Ok(response) => {
                let status = response.status();
                return match response.into_string() {
                    Ok(body) => Ok((status, body)),
                    Err(e) => Err(format!("读取最新版本失败：{e}")),
                };
            }
            // A status reply is an answer, not a connection problem: do not retry it.
            Err(ureq::Error::Status(status, _)) => {
                return Err(format!("查询最新版本失败：服务返回 {status}"))
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!(
        "查询最新版本失败：连接中断（已重试 3 次）。这类中断多半是网络问题，挂上代理或稍后点「重试」通常就好。原因：{last}"
    ))
}

fn arm64() -> bool {
    cfg!(target_arch = "aarch64")
}

/// Keep the release's `major.minor.patch`; WorkBuddy appends a build number
/// (`5.5.6.38337834`) that the installed version never carries.
fn release_version(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches(['v', 'V']);
    // A rebuild of one version (`2026.9.8-1`, as OpenClaw tags them) is that version.
    let value = match value.split_once(['-', '+']) {
        Some((head, tail)) if tail.chars().all(|ch| ch.is_ascii_digit() || ch == '.') => head,
        _ => value,
    };
    let parts: Vec<&str> = value.split('.').take(3).collect();
    let numeric = !parts.is_empty()
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()));
    numeric.then(|| parts.join("."))
}

/// `version: 3.14.1` at the top of ZCode's electron-builder manifest.
pub(crate) fn parse_zcode_manifest(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("version:"))
        .and_then(|value| release_version(value.trim().trim_matches(['"', '\''])))
}

/// ZCode's release manifest, the one its updater reads (stable channel).
pub(crate) fn zcode_latest() -> Result<String, String> {
    let (_, body) = get(super::registry::zcode_manifest_url())?;
    parse_zcode_manifest(&body).ok_or_else(|| "ZCode 发布清单里没有版本号".into())
}

/// WorkBuddy and Qoder both reply `{"productVersion": "…", …}` with 200, or 204 when current.
pub(crate) fn parse_assignment(status: u16, body: &str) -> Result<Assignment, String> {
    if status == 204 {
        return Ok(Assignment::Current);
    }
    let reply: Value = serde_json::from_str(body)
        .map_err(|_| format!("更新服务返回了无法识别的内容（{status}）"))?;
    reply
        .get("productVersion")
        .or_else(|| reply.get("version"))
        .and_then(Value::as_str)
        .and_then(release_version)
        .map(Assignment::Update)
        .ok_or_else(|| {
            let message = reply
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("没有版本号");
            format!("更新服务没有给出版本号：{message}")
        })
}

fn assigned(current: Option<&str>, url: impl FnOnce(&str) -> String) -> Result<String, String> {
    let current = current.ok_or("未读到已安装版本，无法向更新服务查询")?;
    let (status, body) = get(&url(current))?;
    Ok(match parse_assignment(status, &body)? {
        Assignment::Update(version) => version,
        Assignment::Current => current.to_string(),
    })
}

/// WorkBuddy's update service: `copilot.tencent.com` for the China edition,
/// `www.workbuddy.ai` for the global one.
pub(crate) fn workbuddy_latest(base: &str, current: Option<&str>) -> Result<String, String> {
    let platform = if arm64() {
        "win32-arm64-user"
    } else {
        "win32-x64-user"
    };
    assigned(current, |version| {
        format!("{base}/v2/update?platform=workbuddy-{platform}&version={version}")
    })
}

/// Qoder's desktop update service: `gateway.qoder.com.cn` (China) or `center.qoder.sh`.
pub(crate) fn qoder_latest(base: &str, current: Option<&str>) -> Result<String, String> {
    let platform = if arm64() { "win32-arm64" } else { "win32-x64" };
    assigned(current, |version| {
        format!("{base}/algo/api/update/desktop/{platform}/stable/{version}")
    })
}

/// TRAE's `check_update` reply: `data.appVersion` is the latest TRAE Work version.
pub(crate) fn parse_trae_check_update(body: &str) -> Result<String, String> {
    let reply: Value =
        serde_json::from_str(body).map_err(|_| "TRAE 更新服务返回了无法识别的内容".to_string())?;
    if reply.get("err_code").and_then(Value::as_i64) != Some(0) {
        let message = reply
            .get("err_message")
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(format!("TRAE 更新服务报错：{message}"));
    }
    reply
        .pointer("/data/appVersion")
        .and_then(Value::as_str)
        .and_then(release_version)
        .ok_or_else(|| "TRAE 更新服务没有给出版本号".into())
}

/// One TRAE Work edition's `check_update`, with the query its app sends.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TraeFeed {
    host: &'static str,
    package_type: &'static str,
    branch: &'static str,
    /// The China service answers with an older release unless asked for the `marscode` tenant.
    extra: &'static str,
}

pub(crate) const TRAE_CN: TraeFeed = TraeFeed {
    host: "api.trae.com.cn",
    package_type: "stable_cn",
    branch: "release_solo_win32_cn",
    extra: "&userRegion=CN&tenant=marscode",
};

pub(crate) const TRAE_GLOBAL: TraeFeed = TraeFeed {
    host: "icube-normal.traeapi.us",
    package_type: "stable_i18n",
    branch: "release_solo_win32_i18n",
    extra: "",
};

/// The update check TRAE Work runs itself. Asked as version 0.0.0 it always names the latest
/// release. The service requires a machine id; an all-zero placeholder stands in for one so
/// nothing identifying is sent.
pub(crate) fn trae_work_latest(feed: &TraeFeed) -> Result<String, String> {
    let arch = if arm64() { "arm64" } else { "x64" };
    let mid = "0".repeat(64);
    let TraeFeed {
        host,
        package_type,
        branch,
        extra,
    } = feed;
    let (_, body) = get(&format!(
        "https://{host}/icube/api/v1/package/check_update?mid={mid}&packageType={package_type}\
         &productCode=SOLO_Lite&platform=Win&branch={branch}&arch={arch}\
         &appVersion=0.0.0&buildVersion=0.0.0{extra}"
    ))?;
    parse_trae_check_update(&body)
}

use super::install::direct::ResolvedInstaller;

fn installer_named(
    url: String,
    sha512: Option<String>,
    args: Option<Vec<String>>,
) -> Result<ResolvedInstaller, String> {
    if !url.starts_with("https://") {
        return Err("官方服务给出的安装包地址不是 HTTPS 链接".into());
    }
    let file_name = url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| name.to_ascii_lowercase().ends_with(".exe"))
        .ok_or("官方服务给出的安装包地址不是 exe 文件")?
        .to_string();
    Ok(ResolvedInstaller {
        url,
        file_name,
        sha512,
        silent_args: args,
    })
}

/// The installer URL WorkBuddy's update service gives when asked without a version, which is
/// what its website's download button does: the latest full installer.
pub(crate) fn parse_workbuddy_installer(body: &str) -> Option<String> {
    let reply: Value = serde_json::from_str(body).ok()?;
    reply.get("url").and_then(Value::as_str).map(str::to_string)
}

fn workbuddy_installer(base: &str) -> Result<ResolvedInstaller, String> {
    let platform = if arm64() {
        "win32-arm64-user"
    } else {
        "win32-x64-user"
    };
    let (_, body) = get(&format!("{base}/v2/update?platform=workbuddy-{platform}"))?;
    let url = parse_workbuddy_installer(&body).ok_or("WorkBuddy 更新服务没有给出安装包")?;
    installer_named(url, None, None)
}

pub(crate) fn workbuddy_cn_installer() -> Result<ResolvedInstaller, String> {
    workbuddy_installer("https://copilot.tencent.com")
}

pub(crate) fn workbuddy_global_installer() -> Result<ResolvedInstaller, String> {
    workbuddy_installer("https://www.workbuddy.ai")
}

/// The installer for `region` (then any) in TRAE's `check_update` reply.
pub(crate) fn parse_trae_installer(body: &str, region: &str, arch: &str) -> Option<String> {
    let reply: Value = serde_json::from_str(body).ok()?;
    let downloads = reply.pointer("/data/manifest/win32/download")?.as_array()?;
    let pick = |wanted: Option<&str>| {
        downloads.iter().find_map(|entry| {
            let matches = wanted.map_or(true, |r| {
                entry.get("region").and_then(Value::as_str) == Some(r)
            });
            matches
                .then(|| entry.get(arch).and_then(Value::as_str))
                .flatten()
        })
    };
    pick(Some(region))
        .or_else(|| pick(None))
        .map(str::to_string)
}

fn trae_installer(feed: &TraeFeed, region: &str) -> Result<ResolvedInstaller, String> {
    let arch = if arm64() { "arm64" } else { "x64" };
    let mid = "0".repeat(64);
    let TraeFeed {
        host,
        package_type,
        branch,
        extra,
    } = feed;
    let (_, body) = get(&format!(
        "https://{host}/icube/api/v1/package/check_update?mid={mid}&packageType={package_type}\
         &productCode=SOLO_Lite&platform=Win&branch={branch}&arch={arch}\
         &appVersion=0.0.0&buildVersion=0.0.0{extra}"
    ))?;
    let url = parse_trae_installer(&body, region, arch).ok_or("TRAE 更新服务没有给出安装包")?;
    installer_named(url, None, None)
}

pub(crate) fn trae_cn_installer() -> Result<ResolvedInstaller, String> {
    trae_installer(&TRAE_CN, "cn")
}

pub(crate) fn trae_global_installer() -> Result<ResolvedInstaller, String> {
    trae_installer(&TRAE_GLOBAL, "sg")
}

/// Whether an app whose uninstall entry is named `<prefix><version>` was installed for all
/// users (listed under HKLM with `/allusers`). Read straight from the registry: the full
/// desktop detection pulls in APIs a test binary cannot load.
#[cfg(windows)]
fn installed_for_all_users(prefix: &str) -> bool {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let Ok(uninstall) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key) else {
        return false;
    };
    uninstall.enum_keys().flatten().any(|name| {
        let Ok(entry) = uninstall.open_subkey(&name) else {
            return false;
        };
        let display: String = entry.get_value("DisplayName").unwrap_or_default();
        let command: String = entry.get_value("UninstallString").unwrap_or_default();
        display.starts_with(prefix)
            && display[prefix.len()..].starts_with(|c: char| c.is_ascii_digit())
            && command.to_ascii_lowercase().contains("/allusers")
    })
}

#[cfg(not(windows))]
fn installed_for_all_users(_: &str) -> bool {
    false
}

/// Qoder publishes `releases/latest.yml` (per-user installer) and `releases/latest-system.yml`
/// (for all users), each naming a file under `releases/<version>/` with its SHA-512.
fn qoder_installer(base: &str, display_name: &str) -> Result<ResolvedInstaller, String> {
    // An install made for all users is updated in place by the per-machine package; a new
    // install is per user and needs no administrator.
    let machine = installed_for_all_users(display_name);
    let manifest = if machine {
        "latest-system.yml"
    } else {
        "latest.yml"
    };
    let (_, body) = get(&format!("{base}/qoder-app/releases/{manifest}"))?;
    let release =
        super::install::direct::parse_latest_yml(&body).ok_or("Qoder 发布信息格式无法识别")?;
    // Only `/S`: each manifest already names a package built for its scope, and an
    // electron-builder installer refuses a silent run that also carries `/allusers`
    // (Qoder's quit with exit code 21). The per-machine package asks for elevation itself.
    let args = vec!["/S".to_string()];
    installer_named(
        format!(
            "{base}/qoder-app/releases/{}/{}",
            release.version, release.file_name
        ),
        Some(release.sha512),
        Some(args),
    )
}

pub(crate) fn qoder_cn_installer() -> Result<ResolvedInstaller, String> {
    qoder_installer("https://static.qoder.com.cn", "Qoder CN ")
}

pub(crate) fn qoder_global_installer() -> Result<ResolvedInstaller, String> {
    qoder_installer("https://download.qoder.com.cn", "Qoder ")
}

/// `tag_name` of a GitHub release, without the leading `v`.
pub(crate) fn parse_github_release(body: &str) -> Result<String, String> {
    let reply: Value =
        serde_json::from_str(body).map_err(|_| "GitHub 返回了无法识别的内容".to_string())?;
    reply
        .get("tag_name")
        .and_then(Value::as_str)
        .and_then(release_version)
        .ok_or_else(|| {
            let message = reply
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("没有版本号");
            format!("GitHub 没有给出版本号：{message}")
        })
}

/// The newest stable release of a GitHub repository, read from the API (the web
/// `releases/latest` redirect is often unreachable from China).
pub(crate) fn github_latest(repo: &str) -> Result<String, String> {
    let (_, body) = get(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))?;
    parse_github_release(&body)
}

/// `ProductVersion` from a PE file's version resource (UTF-16 key, NUL padding, UTF-16 value).
pub(crate) fn pe_product_version(bytes: &[u8]) -> Option<String> {
    let key: Vec<u8> = "ProductVersion"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let start = bytes.windows(key.len()).position(|window| window == key)? + key.len();
    let mut units = bytes[start..]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .skip_while(|&unit| unit == 0)
        .take_while(|&unit| unit != 0);
    let value = String::from_utf16(&units.by_ref().take(64).collect::<Vec<_>>()).ok()?;
    let value = value.trim();
    (!value.is_empty() && value.chars().any(|ch| ch.is_ascii_digit())).then(|| value.to_string())
}

/// The version of an official installer, read from its first 256 KB. MiMo publishes only a
/// `…-latest-…` installer, and its version resource sits near the start of the file, so a
/// range request reads it without downloading the whole 250 MB.
pub(crate) fn installer_version(url: &str) -> Result<String, String> {
    let response = agent()
        .get(url)
        .set("User-Agent", "Stacker")
        .set("Range", "bytes=0-262143")
        .call()
        .map_err(|e| format!("查询最新版本失败：{e}"))?;
    let mut head = Vec::with_capacity(262_144);
    std::io::Read::read_to_end(
        &mut std::io::Read::take(response.into_reader(), 262_144),
        &mut head,
    )
    .map_err(|e| format!("读取最新版本失败：{e}"))?;
    pe_product_version(&head).ok_or_else(|| "官方安装包里没有版本号".into())
}

/// A version written into an installer's name: `ZCode-3.14.5-win-x64.exe`,
/// `MiniMax Code Setup 3.1.1.exe`, `XiaomiMiMo-26.929.292248-x64-setup.exe`.
pub(crate) fn version_in_name(name: &str) -> Option<String> {
    let re = regex::Regex::new(r"(\d+(?:\.\d+){2,3})").ok()?;
    re.captures(name).map(|c| c[1].to_string())
}

/// `updaterCacheDirName` from an Electron app's `resources/app-update.yml`.
pub(crate) fn updater_cache_name(app_update_yml: &str) -> Option<String> {
    app_update_yml.lines().find_map(|line| {
        let value = line.trim().strip_prefix("updaterCacheDirName:")?;
        let value = value.trim().trim_matches(['"', '\'']).trim();
        (!value.is_empty() && !value.contains(['/', '\\', '.'])).then(|| value.to_string())
    })
}

/// Where an Electron app's own updater keeps what it downloaded: the folder its
/// `app-update.yml` names, and any `…-updater` folder the catalog lists for it.
pub(crate) fn updater_cache_dirs(
    data_dirs: &[super::registry::DataDir],
    executable: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = data_dirs
        .iter()
        .filter(|dir| dir.relative.ends_with("-updater"))
        .filter_map(|dir| dir.path())
        .collect();
    let named = executable
        .and_then(|exe| exe.parent())
        .map(|dir| dir.join("resources").join("app-update.yml"))
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| updater_cache_name(&text));
    if let (Some(name), Some(local)) = (named, dirs::data_local_dir()) {
        let dir = local.join(name);
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The version an app's own updater has already downloaded and will install on the next
/// restart. electron-updater writes `pending/update-info.json` only once the download is
/// complete and checked, so this is a version that exists for this machine, including one a
/// vendor is still rolling out and its public feed does not show yet.
pub(crate) fn downloaded_update(dirs: &[std::path::PathBuf]) -> Option<String> {
    let mut best: Option<String> = None;
    for dir in dirs {
        let pending = dir.join("pending");
        let Ok(text) = std::fs::read_to_string(pending.join("update-info.json")) else {
            continue;
        };
        let Ok(info) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let file = info.get("fileName").and_then(Value::as_str).unwrap_or("");
        // A name without a version (`Antigravity-x64.exe`): the installer's own says it.
        let version = version_in_name(file).or_else(|| {
            let installer = pending.join(file);
            let mut head = Vec::new();
            std::io::Read::read_to_end(
                &mut std::io::Read::take(std::fs::File::open(installer).ok()?, 4 * 1024 * 1024),
                &mut head,
            )
            .ok()?;
            pe_product_version(&head)
        });
        if let Some(version) = version {
            if best
                .as_deref()
                .map_or(true, |b| crate::update::ver_lt(b, &version))
            {
                best = Some(version);
            }
        }
    }
    best
}

/// The `version` field of a package.json.
pub(crate) fn package_json_version(text: &str) -> Option<String> {
    let manifest: Value = serde_json::from_str(text).ok()?;
    manifest
        .get("version")
        .and_then(Value::as_str)
        .and_then(release_version)
}

/// Where an asar archive's JSON header is: `(json_offset, json_len, data_offset)`, read from its
/// 16-byte prefix (a Chromium pickle holding the header size, then the JSON string's length).
pub(crate) fn asar_layout(prefix: &[u8; 16]) -> Option<(u64, usize, u64)> {
    let word = |at: usize| {
        u32::from_le_bytes([prefix[at], prefix[at + 1], prefix[at + 2], prefix[at + 3]])
    };
    if word(0) != 4 {
        return None;
    }
    let header_size = u64::from(word(4));
    let json_len = word(12) as usize;
    (json_len > 0 && json_len as u64 <= header_size).then_some((16, json_len, 8 + header_size))
}

/// The `version` of the `package.json` at the root of an Electron `app.asar`.
pub(crate) fn asar_package_version(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let mut prefix = [0u8; 16];
    file.read_exact(&mut prefix).ok()?;
    let (json_at, json_len, data_at) = asar_layout(&prefix)?;
    if json_len > 16 * 1024 * 1024 {
        return None;
    }
    let mut json = vec![0u8; json_len];
    file.seek(SeekFrom::Start(json_at)).ok()?;
    file.read_exact(&mut json).ok()?;
    let header: Value = serde_json::from_slice(&json).ok()?;
    let entry = header.pointer("/files/package.json")?;
    let offset: u64 = entry.get("offset")?.as_str()?.parse().ok()?;
    let size = entry.get("size")?.as_u64()?;
    if size > 1024 * 1024 {
        return None;
    }
    let mut manifest = vec![0u8; size as usize];
    file.seek(SeekFrom::Start(data_at + offset)).ok()?;
    file.read_exact(&mut manifest).ok()?;
    package_json_version(std::str::from_utf8(&manifest).ok()?)
}

/// `version = "0.21.4"` under `[project]` in a pyproject.toml.
pub(crate) fn pyproject_version(text: &str) -> Option<String> {
    let mut in_project = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_project = line == "[project]";
        } else if in_project {
            if let Some(value) = line.strip_prefix("version").map(str::trim_start) {
                if let Some(value) = value.strip_prefix('=') {
                    return release_version(value.trim().trim_matches(['"', '\'']));
                }
            }
        }
    }
    None
}

/// `hermes update` pulls `main` of the Hermes Agent repository; its pyproject names the
/// version that update brings.
pub(crate) fn hermes_cli_latest() -> Result<String, String> {
    let (_, body) =
        get("https://raw.githubusercontent.com/NousResearch/hermes-agent/main/pyproject.toml")?;
    pyproject_version(&body).ok_or_else(|| "Hermes 的 pyproject.toml 里没有版本号".into())
}

/// Hermes Desktop is built from the Hermes Agent checkout, and its updater pulls `main`: the
/// version there is the one an update brings.
pub(crate) fn hermes_desktop_latest() -> Result<String, String> {
    let (_, body) = get(
        "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/apps/desktop/package.json",
    )?;
    package_json_version(&body).ok_or_else(|| "Hermes 桌面端的 package.json 里没有版本号".into())
}

/// `0.0.0` is what a package.json says when nobody numbers the releases: no version at all.
pub(crate) fn real_version(version: String) -> Option<String> {
    (version.trim_start_matches(['v', 'V']) != "0.0.0").then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_version_comes_from_the_version_resource() {
        let utf16 =
            |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
        // The layout MiMo's installer has: key, NUL padding, value, NUL.
        let mut bytes = b"MZ...".to_vec();
        bytes.extend(utf16("ProductName\0Xiaomi MiMo\0"));
        bytes.extend(utf16("ProductVersion"));
        bytes.extend([0, 0, 0, 0]);
        bytes.extend(utf16("26.922.220226"));
        bytes.extend([0, 0, 0, 0]);
        assert_eq!(pe_product_version(&bytes), Some("26.922.220226".into()));
        assert_eq!(pe_product_version(b"MZ no resources"), None);
    }

    #[test]
    fn pyproject_version_is_the_project_one() {
        let text = "[build-system]
requires = [\"setuptools\"]
version = \"9.9.9\"

[project]
name = \"hermes-agent\"
version = \"0.21.4\"
";
        assert_eq!(pyproject_version(text), Some("0.21.4".into()));
        assert_eq!(
            pyproject_version(
                "[project]
name = \"x\"
"
            ),
            None
        );
    }

    #[test]
    fn asar_layout_comes_from_its_pickle_prefix() {
        // Hermes Desktop's app.asar starts with 4, header size 34896, 34892, JSON length 34887.
        let mut prefix = [0u8; 16];
        for (i, word) in [4u32, 34896, 34892, 34887].iter().enumerate() {
            prefix[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(asar_layout(&prefix), Some((16, 34887, 34904)));
        assert_eq!(asar_layout(&[0u8; 16]), None);
    }

    #[test]
    fn an_update_the_app_downloaded_itself_is_found_by_name_or_not_at_all() {
        assert_eq!(
            version_in_name("ZCode-3.14.5-win-x64.exe").as_deref(),
            Some("3.14.5")
        );
        assert_eq!(
            version_in_name("MiniMax Code Setup 3.1.1.exe").as_deref(),
            Some("3.1.1")
        );
        assert_eq!(
            version_in_name("XiaomiMiMo-26.929.292248-x64-setup.exe").as_deref(),
            Some("26.929.292248")
        );
        assert_eq!(version_in_name("Antigravity-x64.exe"), None);
        assert_eq!(
            updater_cache_name(
                "provider: generic\nupdaterCacheDirName: '@mmx-agentelectron-updater'\n"
            )
            .as_deref(),
            Some("@mmx-agentelectron-updater")
        );
        assert_eq!(updater_cache_name("updaterCacheDirName: ../evil"), None);

        let root = tempfile::tempdir().unwrap();
        let (old, new, empty) = (
            root.path().join("a"),
            root.path().join("b"),
            root.path().join("c"),
        );
        for (dir, name) in [(&old, "App-1.2.0.exe"), (&new, "App Setup 1.10.0.exe")] {
            std::fs::create_dir_all(dir.join("pending")).unwrap();
            std::fs::write(
                dir.join("pending").join("update-info.json"),
                format!(r#"{{"fileName":"{name}","sha512":"x"}}"#),
            )
            .unwrap();
        }
        assert_eq!(
            downloaded_update(&[old, new, empty]).as_deref(),
            Some("1.10.0")
        );
        assert_eq!(downloaded_update(&[]), None);
    }

    #[test]
    fn a_rebuild_tag_is_its_version_and_a_placeholder_is_none() {
        assert_eq!(release_version("v2026.9.8-1").as_deref(), Some("2026.9.8"));
        assert_eq!(release_version("v2026.9.8-alpha.2"), None);
        assert_eq!(real_version("0.0.0".into()), None);
        assert_eq!(real_version("0.17.1".into()).as_deref(), Some("0.17.1"));
    }

    #[test]
    fn package_json_version_is_read() {
        assert_eq!(
            package_json_version(r#"{"name":"hermes-desktop","version":"0.17.6"}"#),
            Some("0.17.6".into())
        );
        assert_eq!(package_json_version("not json"), None);
    }

    #[test]
    fn zcode_manifest_version_is_the_first_version_line() {
        let body = "version: 3.14.1\nfiles:\n    - url: https://cdn-zcode.z.ai/x.exe\n      size: 1\nreleaseName: Release v3.14.1\n";
        assert_eq!(parse_zcode_manifest(body), Some("3.14.1".into()));
        assert_eq!(parse_zcode_manifest("<Error>NoSuchKey</Error>"), None);
    }

    #[test]
    fn an_assignment_names_the_update_or_says_the_install_is_current() {
        // WorkBuddy's reply, trimmed; the build number is dropped to match the installed version.
        let workbuddy = r#"{"version":"5.5.6.38337834","url":"https://download.codebuddy.cn/x.exe","productVersion":"5.5.6.38337834","sha256hash":""}"#;
        assert_eq!(
            parse_assignment(200, workbuddy),
            Ok(Assignment::Update("5.5.6".into()))
        );
        // Qoder's: `version` is a commit hash, `productVersion` the release.
        let qoder = r#"{"url":"https://download.qoder.com.cn/qoder-app/releases/0.3.4/latest-system.yml","name":"0.3.4","version":"081b9000518bbd81daf8108840085007425d9bb9","productVersion":"0.3.4"}"#;
        assert_eq!(
            parse_assignment(200, qoder),
            Ok(Assignment::Update("0.3.4".into()))
        );
        assert_eq!(parse_assignment(204, ""), Ok(Assignment::Current));
        let refused = r#"{"code":10001,"msg":"10001:failed to get package version, invalid platform: workbuddy-win32-x64"}"#;
        assert!(parse_assignment(200, refused)
            .unwrap_err()
            .contains("invalid platform"));
    }

    #[test]
    fn trae_check_update_reads_the_app_version_not_the_build() {
        let body = r#"{"err_code":0,"err_message":"success","data":{"needUpdate":true,"appVersion":"0.1.69","manifest":{"win32":{"version":"2.3.87413"}}}}"#;
        assert_eq!(parse_trae_check_update(body), Ok("0.1.69".into()));
        let refused = r#"{"err_code":1000,"err_message":"missing mid","data":{}}"#;
        assert!(parse_trae_check_update(refused)
            .unwrap_err()
            .contains("missing mid"));
    }

    #[test]
    fn installer_urls_come_from_the_vendors_services() {
        let workbuddy = r#"{"version":"5.5.6.38337834","url":"https://download.codebuddy.cn/workbuddy/saas/win32-x64-user/WorkBuddy-win32-x64-user-5.5.6.38337834-5f969292.exe","sha256hash":""}"#;
        let url = parse_workbuddy_installer(workbuddy).unwrap();
        assert_eq!(
            installer_named(url, None, None).unwrap().file_name,
            "WorkBuddy-win32-x64-user-5.5.6.38337834-5f969292.exe"
        );
        let trae = r#"{"err_code":0,"data":{"manifest":{"win32":{"download":[
            {"region":"cn","x64":"https://lf-cdn.trae.com.cn/a/TraeWork_CN-Setup-x64.exe"},
            {"region":"sg","x64":"https://lf-cdn.trae.ai/sg/TraeWork_CN-Setup-x64.exe"}]}}}}"#;
        assert_eq!(
            parse_trae_installer(trae, "sg", "x64").unwrap(),
            "https://lf-cdn.trae.ai/sg/TraeWork_CN-Setup-x64.exe"
        );
        assert_eq!(
            parse_trae_installer(trae, "va", "x64").unwrap(),
            "https://lf-cdn.trae.com.cn/a/TraeWork_CN-Setup-x64.exe"
        );
        assert!(parse_trae_installer(trae, "cn", "arm64").is_none());
        assert!(installer_named("http://example.com/a.exe".into(), None, None).is_err());
        assert!(installer_named("https://example.com/a.zip".into(), None, None).is_err());
    }

    #[test]
    fn github_release_tag_loses_its_v() {
        assert_eq!(
            parse_github_release(r#"{"tag_name":"v2026.9.4","prerelease":false}"#),
            Ok("2026.9.4".into())
        );
        assert!(parse_github_release(r#"{"message":"Not Found"}"#).is_err());
    }
}
