//! 内置镜像清单的远程更新：从用户配置的 GitHub URL 拉取 mirrors.json，覆盖兜底清单。
//! 因 raw.githubusercontent.com 在国内常被墙，自动追加 jsDelivr 兜底。
//! 本地缓存 %APPDATA%\stacker\mirrors.json；地址存 config.json（运行时可配，不写死仓库）。

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::sources::{self, Mirror};

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteTool {
    pub id: String,
    pub mirrors: Vec<Mirror>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteList {
    pub version: u32,
    #[serde(default)]
    pub tools: Vec<RemoteTool>,
}

#[derive(Serialize, Deserialize, Default)]
struct Cfg {
    #[serde(default)]
    mirror_list_url: String,
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

// ── 拉取（带国内兜底）──
fn candidates(url: &str) -> Vec<String> {
    let mut v = vec![url.to_string()];
    if let Some(rest) = url.strip_prefix("https://raw.githubusercontent.com/") {
        let p: Vec<&str> = rest.splitn(4, '/').collect();
        if p.len() == 4 {
            // OWNER/REPO/BRANCH/PATH → jsDelivr CDN（国内多可达）
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
        let latest = github_latest_tag(APP_REPO)?;
        let has_update = ver_lt(&current, &latest);
        Ok(UpdateInfo {
            current,
            latest,
            has_update,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 取 GitHub 仓库最新 release 的 tag（去掉前导 v）。仅走官方 GitHub API。
pub fn github_latest_tag(repo: &str) -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let a = agent();
    let mut last = String::new();
    for u in [url.clone()] {
        match a.get(&u).set("User-Agent", "Stacker").call() {
            Ok(r) => match r.into_string() {
                Ok(b) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&b) {
                        if let Some(t) = v["tag_name"].as_str() {
                            return Ok(t.trim_start_matches('v').trim().to_string());
                        }
                    }
                    last = "返回无 tag_name".into();
                }
                Err(e) => last = e.to_string(),
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("获取最新版本失败：{last}"))
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

#[derive(Serialize)]
pub struct MirrorsStatus {
    pub url: String,
    pub local_version: Option<u32>,
    pub tools: usize,
}

#[tauri::command]
pub fn mirrors_status() -> MirrorsStatus {
    let url = load_cfg().mirror_list_url;
    let list = load_list();
    MirrorsStatus {
        url,
        local_version: list.as_ref().map(|l| l.version),
        tools: list.map(|l| l.tools.len()).unwrap_or(0),
    }
}

/// 拉取并应用远程清单。url 为空则用已存配置；成功后记住该地址。
#[tauri::command]
pub async fn mirrors_update(url: Option<String>) -> Result<MirrorsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = url
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| load_cfg().mirror_list_url);
        if url.trim().is_empty() {
            return Err("未配置镜像清单地址".into());
        }
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
            local_version: Some(list.version),
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
        version: 1,
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
