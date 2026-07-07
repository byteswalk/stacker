//! 内置镜像清单的远程更新：从用户配置的 GitHub URL 拉取 mirrors.json，覆盖兜底清单。
//! raw.githubusercontent.com 在部分网络环境下不可达，自动追加 jsDelivr 兜底。
//! 本地缓存 %APPDATA%\stacker\mirrors.json；地址存 config.json（运行时可配，不写死仓库）。

use std::path::PathBuf;
use std::time::Duration;

use serde::{de, Deserialize, Deserializer, Serialize};

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
const DEFAULT_LATEST_URL: &str =
    "https://raw.githubusercontent.com/byteswalk/stacker/main/resources/latest.json";

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
    if cfg.mirror_list_url.trim().is_empty() {
        DEFAULT_MIRROR_LIST_URL.into()
    } else {
        cfg.mirror_list_url
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
    let mut by_tool = std::collections::HashMap::new();
    for rt in list.tools {
        by_tool.insert(rt.id, rt.mirrors);
    }
    for t in tools {
        if let Some(mirrors) = by_tool.remove(&t.id) {
            t.mirrors = mirrors;
        } else {
            t.mirrors.clear();
        }
    }
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

/// 检查 Stacker 自身是否有新版（比对当前版本号与 GitHub 最新 release）。
#[tauri::command]
pub async fn app_check_update() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let current = env!("CARGO_PKG_VERSION").to_string();
        if APP_REPO.is_empty() {
            return Err(
                "尚未配置更新源：项目仓库未发布。发布到 GitHub Releases 后即可在此检查更新。"
                    .to_string(),
            );
        }
        match github_latest_release(APP_REPO, &current) {
            Ok(info) => Ok(info),
            Err(primary) => latest_json_update(&current)
                .map_err(|fallback| format!("检查更新失败：{primary}；兜底清单失败：{fallback}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
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

fn latest_json_update(current: &str) -> Result<UpdateInfo, String> {
    let body = fetch(DEFAULT_LATEST_URL)?;
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

fn fetch_remote_list(url: &str) -> Result<RemoteList, String> {
    let body = fetch(url.trim())?;
    let list: RemoteList = serde_json::from_str(&body).map_err(|e| format!("清单格式错误：{e}"))?;
    if list.tools.is_empty() {
        return Err("服务器清单为空".into());
    }
    Ok(list)
}

/// 只检查远程清单版本，不写本地缓存。
#[tauri::command]
pub async fn mirrors_check_update(url: Option<String>) -> Result<MirrorsUpdateCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = url
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(configured_mirror_url);
        let remote = fetch_remote_list(&url)?;
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
        let url = url
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(configured_mirror_url);
        let body = fetch(url.trim())?;
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

/// 导出当前兜底清单为可上传 GitHub 的 mirrors.json 种子，返回文件路径。
#[tauri::command]
pub fn mirrors_seed() -> Result<String, String> {
    let list = RemoteList {
        version: chrono::Local::now().format("%Y%m%d%H%M").to_string(),
        updated_at: chrono::Local::now().to_rfc3339(),
        tools: sources::hardcoded()
            .iter()
            .map(|t| RemoteTool {
                id: t.id.clone(),
                mirrors: t.mirrors.clone(),
            })
            .collect(),
    };
    let p = dir().join("mirrors.seed.json");
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().to_string())
}
