//! 内置镜像清单的远程更新：从用户配置的 GitHub URL 拉取 mirrors.json，覆盖兜底清单。
//! raw.githubusercontent.com 在部分网络环境下不可达，自动追加 jsDelivr 兜底。
//! 本地缓存 %APPDATA%\stacker\mirrors.json；地址存 config.json（运行时可配，不写死仓库）。

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde::{de, Deserialize, Deserializer, Serialize};
use tauri::{Emitter, Manager};

use crate::sources::{self, Mirror};

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteTool {
    pub id: String,
    pub mirrors: Vec<Mirror>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteList {
    #[serde(
        default = "default_catalog_version",
        deserialize_with = "de_version_string"
    )]
    pub version: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub tools: Vec<RemoteTool>,
}

#[derive(Serialize, Deserialize, Default)]
struct Cfg {
    #[serde(default)]
    mirror_list_url: String,
}

const DEFAULT_MIRROR_LIST_URL: &str =
    "https://raw.githubusercontent.com/byteswalk/stacker/main/resources/mirrors.json";
const GITEE_MIRROR_LIST_URL: &str =
    "https://gitee.com/shaxiong/stacker/raw/main/resources/mirrors.json";
const DEFAULT_LATEST_URL: &str =
    "https://raw.githubusercontent.com/byteswalk/stacker/main/resources/latest.json";
const GITEE_LATEST_URL: &str = "https://gitee.com/shaxiong/stacker/raw/main/resources/latest.json";

fn official_mirror_urls_for_locale(locale: &str) -> Vec<&'static str> {
    if locale.eq_ignore_ascii_case("en-US") {
        vec![DEFAULT_MIRROR_LIST_URL, GITEE_MIRROR_LIST_URL]
    } else {
        vec![GITEE_MIRROR_LIST_URL, DEFAULT_MIRROR_LIST_URL]
    }
}

fn github_first_for_locale(locale: &str) -> bool {
    locale.eq_ignore_ascii_case("en-US")
}

fn official_mirror_urls() -> Vec<String> {
    official_mirror_urls_for_locale(&crate::settings::load().locale)
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn is_official_mirror_url(url: &str) -> bool {
    matches!(url.trim(), DEFAULT_MIRROR_LIST_URL | GITEE_MIRROR_LIST_URL)
}

fn default_catalog_version() -> String {
    "197001010000".into()
}

fn de_version_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        _ => Err(de::Error::custom("version 必须是字符串或数字")),
    }
}

fn configured_mirror_url() -> String {
    let cfg = load_cfg();
    if cfg.mirror_list_url.trim().is_empty() || is_official_mirror_url(&cfg.mirror_list_url) {
        official_mirror_urls()
            .into_iter()
            .next()
            .unwrap_or_else(|| DEFAULT_MIRROR_LIST_URL.into())
    } else {
        cfg.mirror_list_url
    }
}

fn configured_mirror_urls() -> Vec<String> {
    let cfg = load_cfg();
    if cfg.mirror_list_url.trim().is_empty() || is_official_mirror_url(&cfg.mirror_list_url) {
        official_mirror_urls()
    } else {
        vec![cfg.mirror_list_url]
    }
}

fn requested_mirror_urls(url: Option<String>) -> Vec<String> {
    match url.filter(|value| !value.trim().is_empty()) {
        Some(value) if is_official_mirror_url(&value) => {
            let mut urls = vec![value.trim().to_string()];
            urls.extend(official_mirror_urls());
            urls.dedup();
            urls
        }
        Some(value) => vec![value],
        None => configured_mirror_urls(),
    }
}

fn dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
}
fn list_path() -> PathBuf {
    dir().join("mirrors.json")
}
fn cfg_path() -> PathBuf {
    dir().join("config.json")
}

fn load_cfg() -> Cfg {
    std::fs::read_to_string(cfg_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
fn save_cfg(c: &Cfg) -> Result<(), String> {
    let p = cfg_path();
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(c).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn load_list() -> Option<RemoteList> {
    std::fs::read_to_string(list_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn remote_snapshot() -> Option<RemoteList> {
    load_list()
}

pub fn save_remote_snapshot(list: &RemoteList) -> Result<(), String> {
    let p = list_path();
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 用远程/缓存清单全量替换兜底工具的镜像列表。
/// 远程清单是服务器维护的内置源真相：未出现在清单里的内置工具会变成空镜像列表。
pub fn overlay(tools: &mut [sources::Tool]) {
    let Some(list) = load_list() else { return };
    let list_version = list.version.clone();
    let mut by_tool = std::collections::HashMap::new();
    for rt in list.tools {
        by_tool.insert(rt.id, rt.mirrors);
    }
    for t in tools {
        if let Some(mirrors) = by_tool.remove(&t.id) {
            t.mirrors = mirrors;
            patch_legacy_catalog_mirrors(&mut t.mirrors, &t.id, &list_version);
        } else if t.id == sources::GIT_RUNTIME_TOOL_ID && list_version.as_str() < "202607111317" {
            // 兼容新增 Git 下载源之前保存的旧清单；新版本清单仍保持全量替换语义。
            continue;
        } else if matches!(
            t.id.as_str(),
            sources::MAVEN_RUNTIME_TOOL_ID
                | sources::GRADLE_RUNTIME_TOOL_ID
                | sources::GO_RUNTIME_TOOL_ID
                | sources::RUST_RUNTIME_TOOL_ID
        ) && list_version.as_str() < "202607131700"
        {
            // 兼容新增三类运行时下载源之前保存的旧清单。
            continue;
        } else {
            t.mirrors.clear();
        }
    }
}

fn patch_legacy_catalog_mirrors(mirrors: &mut Vec<Mirror>, tool_id: &str, list_version: &str) {
    if list_version >= "202607141100" {
        return;
    }
    match tool_id {
        sources::MAVEN_RUNTIME_TOOL_ID => push_missing_mirror(
            mirrors,
            "apache-cdn",
            "Apache CDN",
            "https://dlcdn.apache.org/maven",
            "dlcdn.apache.org",
        ),
        "go" => push_missing_mirror(
            mirrors,
            "goproxyio",
            "goproxy.io",
            "https://goproxy.io,direct",
            "goproxy.io",
        ),
        "maven" | "gradle" => push_missing_mirror(
            mirrors,
            "maven-central-repo1",
            "Maven Central (repo1)",
            "https://repo1.maven.org/maven2/",
            "repo1.maven.org",
        ),
        _ => {}
    }
}

fn push_missing_mirror(mirrors: &mut Vec<Mirror>, id: &str, name: &str, url: &str, host: &str) {
    if mirrors.iter().any(|mirror| mirror.id == id) {
        return;
    }
    mirrors.push(Mirror {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        host: host.into(),
    });
}

// ── 拉取（带 CDN 兜底）──
fn candidates(url: &str) -> Vec<String> {
    let mut v = vec![url.to_string()];
    if let Some(rest) = url.strip_prefix("https://raw.githubusercontent.com/") {
        let p: Vec<&str> = rest.splitn(4, '/').collect();
        if p.len() == 4 {
            // OWNER/REPO/BRANCH/PATH → jsDelivr CDN
            v.push(format!(
                "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
                p[0], p[1], p[2], p[3]
            ));
        }
    }
    v
}

fn fetch_first(urls: &[String]) -> Result<(String, String), String> {
    let mut errors = Vec::new();
    for url in urls {
        match fetch(url) {
            Ok(body) => return Ok((url.clone(), body)),
            Err(error) => errors.push(format!("{url}: {error}")),
        }
    }
    Err(errors.join("; "))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(30))
        .build()
}

fn fetch(url: &str) -> Result<String, String> {
    let agent = agent();
    let mut last = String::new();
    for u in candidates(url) {
        match agent.get(&u).call() {
            Ok(resp) => match resp.into_string() {
                Ok(body) => return Ok(body),
                Err(e) => last = e.to_string(),
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("拉取失败（已试直连/jsDelivr）：{last}"))
}

/// 工具自身更新信息（fnm/pyenv 用）。
#[derive(Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    pub release_url: Option<String>,
    pub installer_url: Option<String>,
    pub portable_url: Option<String>,
    pub published_at: Option<String>,
    pub notes: Vec<String>,
}

// Stacker 自身的发布仓库（owner/repo）。发布到 GitHub Releases 后填上即可启用「检查更新」。
const APP_REPO: &str = "byteswalk/stacker";

/// 按界面语言选择 GitHub/Gitee 的检查顺序。
#[tauri::command]
pub async fn app_check_update() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let current = env!("CARGO_PKG_VERSION").to_string();
        let english = github_first_for_locale(&crate::settings::load().locale);
        let mut errors = Vec::new();

        if !english {
            match latest_json_update_from(GITEE_LATEST_URL, &current) {
                Ok(info) => return Ok(info),
                Err(error) => errors.push(format!("Gitee: {error}")),
            }
        }
        match github_latest_release(APP_REPO, &current) {
            Ok(info) => return Ok(info),
            Err(error) => errors.push(format!("GitHub Releases: {error}")),
        }
        match latest_json_update_from(DEFAULT_LATEST_URL, &current) {
            Ok(info) => return Ok(info),
            Err(error) => errors.push(format!("GitHub manifest: {error}")),
        }
        if english {
            match latest_json_update_from(GITEE_LATEST_URL, &current) {
                Ok(info) => return Ok(info),
                Err(error) => errors.push(format!("Gitee: {error}")),
            }
        }

        Err(format!("检查更新失败：{}", errors.join("；")))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn app_download_update(
    window: tauri::Window,
    url: String,
    version: String,
) -> Result<String, String> {
    if !url.trim().starts_with("https://") {
        return Err("更新包必须使用 HTTPS 地址".into());
    }
    let target = std::env::temp_dir().join(format!(
        "stacker-update-{}.exe",
        version
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '.' || *ch == '-')
            .collect::<String>()
    ));
    let download_window = window.clone();
    let download_url = url.clone();
    let download_target = target.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .build();
        let response = agent
            .get(download_url.trim())
            .set("User-Agent", "Stacker")
            .call()
            .map_err(|error| format!("更新包下载失败：{error}"))?;
        let total = response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok());
        let mut reader = response.into_reader();
        let mut file = std::fs::File::create(&download_target)
            .map_err(|error| format!("无法创建更新文件：{error}"))?;
        let mut downloaded = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|error| format!("读取更新包失败：{error}"))?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])
                .map_err(|error| format!("写入更新包失败：{error}"))?;
            downloaded += count as u64;
            let progress = match total.filter(|value| *value > 0) {
                Some(total) => format!(
                    "正在下载更新 · {:.1}% · {:.1}/{:.1} MB",
                    downloaded as f64 * 100.0 / total as f64,
                    downloaded as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                ),
                None => format!("正在下载更新 · {:.1} MB", downloaded as f64 / 1_048_576.0),
            };
            let _ = download_window.emit("app-update-progress", progress);
        }
        file.flush()
            .map_err(|error| format!("保存更新包失败：{error}"))?;
        if downloaded < 512 * 1024 {
            let _ = std::fs::remove_file(&download_target);
            return Err("下载的更新包大小异常，已取消安装".into());
        }
        let mut signature = [0u8; 2];
        std::fs::File::open(&download_target)
            .and_then(|mut input| input.read_exact(&mut signature))
            .map_err(|error| format!("无法校验更新包：{error}"))?;
        if signature != *b"MZ" {
            let _ = std::fs::remove_file(&download_target);
            return Err("下载内容不是有效的 Windows 安装程序".into());
        }
        let _ = download_window.emit("app-update-progress", "更新包已下载，正在启动安装程序…");
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| error.to_string())??;

    let current_pid = std::process::id();
    let launch_script = format!(
        "$p=Get-Process -Id {current_pid} -ErrorAction SilentlyContinue; if($p){{Wait-Process -Id {current_pid} -Timeout 30 -ErrorAction SilentlyContinue}}; Start-Process -FilePath '{}' -ArgumentList '/S'",
        target.to_string_lossy().replace('\'', "''")
    );
    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &launch_script,
        ])
        .spawn()
        .map_err(|error| format!("启动安装程序失败：{error}"))?;
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        app.exit(0);
    });
    Ok("更新包已下载，安装程序即将接管升级".into())
}

/// 取 GitHub 仓库最新 release 的 tag（去掉前导 v）。仅走官方 GitHub API。
pub fn github_latest_tag(repo: &str) -> Result<String, String> {
    github_latest_release(repo, env!("CARGO_PKG_VERSION")).map(|info| info.latest)
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct LatestFile {
    version: String,
    #[serde(default)]
    release_url: Option<String>,
    #[serde(default)]
    installer_url: Option<String>,
    #[serde(default)]
    portable_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    released_at: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
}

fn notes_from_body(body: &str) -> Vec<String> {
    body.lines()
        .map(|line| {
            line.trim()
                .trim_start_matches(['-', '*', '•', ' '])
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(8)
        .collect()
}

fn pick_asset(assets: &[GitHubAsset], portable: bool) -> Option<String> {
    assets
        .iter()
        .find(|a| {
            let name = a.name.to_ascii_lowercase();
            if portable {
                name.ends_with(".zip") && (name.contains("portable") || name.contains("免安装"))
            } else {
                name.ends_with(".exe") && (name.contains("setup") || name.contains("install"))
            }
        })
        .or_else(|| {
            assets.iter().find(|a| {
                let name = a.name.to_ascii_lowercase();
                if portable {
                    name.ends_with(".zip")
                } else {
                    name.ends_with(".exe")
                }
            })
        })
        .map(|a| a.browser_download_url.clone())
}

fn github_latest_release(repo: &str, current: &str) -> Result<UpdateInfo, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let a = agent();
    let mut last = String::new();
    for u in [url.clone()] {
        match a.get(&u).set("User-Agent", "Stacker").call() {
            Ok(r) => match r.into_string() {
                Ok(b) => {
                    let release: GitHubRelease = serde_json::from_str(&b)
                        .map_err(|e| format!("GitHub Release 格式错误：{e}"))?;
                    let latest = release.tag_name.trim_start_matches('v').trim().to_string();
                    return Ok(UpdateInfo {
                        has_update: ver_lt(current, &latest),
                        current: current.into(),
                        latest,
                        release_url: release.html_url,
                        installer_url: pick_asset(&release.assets, false),
                        portable_url: pick_asset(&release.assets, true),
                        published_at: release.published_at,
                        notes: release
                            .body
                            .as_deref()
                            .map(notes_from_body)
                            .unwrap_or_default(),
                    });
                }
                Err(e) => last = e.to_string(),
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("获取最新版本失败：{last}"))
}

fn latest_json_update_from(url: &str, current: &str) -> Result<UpdateInfo, String> {
    let body = fetch(url)?;
    let file: LatestFile =
        serde_json::from_str(&body).map_err(|e| format!("latest.json 格式错误：{e}"))?;
    let latest = file.version.trim_start_matches('v').trim().to_string();
    Ok(UpdateInfo {
        has_update: ver_lt(current, &latest),
        current: current.into(),
        latest,
        release_url: file.release_url,
        installer_url: file.installer_url,
        portable_url: file.portable_url,
        published_at: file.published_at.or(file.released_at),
        notes: file.notes,
    })
}

/// 简单版本比较：a < b 返回 true（按数字组）。
pub fn ver_lt(a: &str, b: &str) -> bool {
    fn key(s: &str) -> Vec<u64> {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let st = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                out.push(s[st..i].parse().unwrap_or(0));
            } else {
                i += 1;
            }
        }
        out
    }
    key(a) < key(b)
}

#[tauri::command]
pub fn app_open_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("只能打开 http(s) 链接".into());
    }
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = OsStr::new(trimmed).encode_wide().chain(Some(0)).collect();
        let rc = unsafe {
            winapi::um::shellapi::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                winapi::um::winuser::SW_SHOWNORMAL,
            )
        };
        if (rc as isize) <= 32 {
            Err(format!("打开链接失败：ShellExecuteW 返回 {}", rc as isize))
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(trimmed)
            .spawn()
            .map_err(|e| format!("打开链接失败：{e}"))?;
        Ok(())
    }
}

#[derive(Serialize)]
pub struct MirrorsStatus {
    pub url: String,
    pub local_version: Option<String>,
    pub tools: usize,
}

#[tauri::command]
pub fn mirrors_status() -> MirrorsStatus {
    let url = configured_mirror_url();
    let list = load_list();
    MirrorsStatus {
        url,
        local_version: list.as_ref().map(|l| l.version.clone()),
        tools: list.map(|l| l.tools.len()).unwrap_or(0),
    }
}

#[derive(Serialize)]
pub struct MirrorsUpdateCheck {
    pub url: String,
    pub local_version: Option<String>,
    pub remote_version: String,
    pub has_update: bool,
    pub tools: usize,
}

fn version_gt(remote: &str, local: Option<&str>) -> bool {
    let remote = remote.trim();
    if remote.is_empty() {
        return false;
    }
    match local.map(str::trim).filter(|s| !s.is_empty()) {
        Some(local) => remote > local,
        None => true,
    }
}

fn parse_remote_list_body(body: &str) -> Result<RemoteList, String> {
    let list: RemoteList = serde_json::from_str(body).map_err(|error| error.to_string())?;
    if list.tools.is_empty() {
        return Err("The remote source manifest is empty.".into());
    }
    Ok(list)
}

/// 只检查远程清单版本，不写本地缓存。
#[tauri::command]
pub async fn mirrors_check_update(url: Option<String>) -> Result<MirrorsUpdateCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let urls = requested_mirror_urls(url);
        let (url, body) = fetch_first(&urls)?;
        let remote = parse_remote_list_body(&body)?;
        let local = load_list();
        let local_version = local.as_ref().map(|l| l.version.clone());
        Ok(MirrorsUpdateCheck {
            url,
            has_update: version_gt(&remote.version, local_version.as_deref()),
            remote_version: remote.version,
            local_version,
            tools: remote.tools.len(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 拉取并应用远程清单。url 为空则用已存配置；成功后记住该地址。
#[tauri::command]
pub async fn mirrors_update(url: Option<String>) -> Result<MirrorsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let urls = requested_mirror_urls(url);
        let (url, body) = fetch_first(&urls)?;
        let list: RemoteList =
            serde_json::from_str(&body).map_err(|e| format!("清单格式错误：{e}"))?;
        if list.tools.is_empty() {
            return Err("服务器清单为空，已拒绝应用".into());
        }
        let p = list_path();
        if let Some(par) = p.parent() {
            std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, &body).map_err(|e| e.to_string())?;
        save_cfg(&Cfg {
            mirror_list_url: url.trim().to_string(),
        })?;
        Ok(MirrorsStatus {
            url: url.trim().to_string(),
            local_version: Some(list.version.clone()),
            tools: list.tools.len(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_prefers_github() {
        assert!(github_first_for_locale("en-US"));
        assert_eq!(
            official_mirror_urls_for_locale("en-US"),
            vec![DEFAULT_MIRROR_LIST_URL, GITEE_MIRROR_LIST_URL]
        );
    }

    #[test]
    fn chinese_prefers_gitee() {
        assert!(!github_first_for_locale("zh-CN"));
        assert_eq!(
            official_mirror_urls_for_locale("zh-CN"),
            vec![GITEE_MIRROR_LIST_URL, DEFAULT_MIRROR_LIST_URL]
        );
    }
}
