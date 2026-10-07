//! 命名方案：一套网络与下载环境的快照（各工具的下载源、各处代理是否跟随系统代理、
//! 大文件镜像、WinGet 下载器，以及各生态页的下载偏好），一键套用 / 删除。
//! 存储 %APPDATA%\stacker\profiles.json（明文，仅记录镜像 id 与开关，无密码）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::sources;

#[derive(Serialize, Deserialize, Clone)]
pub struct SourceSel {
    pub tool: String,   // 工具 id（pip/npm/go/...）
    pub mirror: String, // 镜像 id（official/tsinghua/...）
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub name: String,
    pub sources: Vec<SourceSel>,
    /// The old terminal-proxy switch. Read from older files and never acted on: the proxy now
    /// follows the Windows proxy, which `proxy_mode` records.
    #[serde(default)]
    pub proxy: bool,
    /// `follow`: every place Stacker manages follows the system proxy; `release`: none has a
    /// proxy. Empty in older profiles and when the places disagreed: the proxy is left alone.
    #[serde(default)]
    pub proxy_mode: Option<String>,
    /// The large-download mirrors that are on; others are turned off. `None`: left alone.
    #[serde(default)]
    pub binary_mirrors: Option<Vec<String>>,
    /// WinGet's `network.downloader`; `None`: left alone.
    #[serde(default)]
    pub winget_downloader: Option<String>,
    pub created: String,
    /// 前端持久化偏好，例如各生态下载源、版本筛选条件和主题。
    #[serde(default)]
    pub frontend_settings: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct ApplyResult {
    pub changed: usize,
    pub frontend_settings: BTreeMap<String, String>,
    /// What could not be done as the profile says, and why.
    pub notes: Vec<String>,
}

pub const FOLLOW: &str = "follow";
pub const RELEASE: &str = "release";

/// What a profile records about the proxy: follow the system proxy when it is on, nothing
/// anywhere when it is off and nothing is left set; when places still disagree with an
/// off system proxy, nothing (the proxy is left alone, as no single state was chosen).
pub fn proxy_mode_of(system_on: bool, differ: usize) -> Option<&'static str> {
    match (system_on, differ) {
        (true, _) => Some(FOLLOW),
        (false, 0) => Some(RELEASE),
        (false, _) => None,
    }
}

fn store_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("profiles.json")
}

fn load() -> Vec<Profile> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_all(list: &[Profile]) -> Result<(), String> {
    let p = store_path();
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())
}

/// 抓取当前各工具的源选择 + 代理开关，存成命名方案（同名覆盖）。
#[tauri::command]
pub fn profile_save(
    name: String,
    frontend_settings: BTreeMap<String, String>,
) -> Result<Profile, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("方案名不能为空".into());
    }
    let sources: Vec<SourceSel> = sources::tools()
        .iter()
        .filter_map(|t| {
            sources::detect(t).map(|mirror| SourceSel {
                tool: t.id.clone(),
                mirror,
            })
        })
        .collect();
    let report = crate::proxy_ledger::sync_report();
    let system_on = report.system.state == crate::proxy_system::SystemState::On;
    let winget = crate::winget_settings::status();
    let prof = Profile {
        name: name.clone(),
        sources,
        proxy: false,
        proxy_mode: proxy_mode_of(system_on, report.rows.len()).map(str::to_string),
        binary_mirrors: Some(
            crate::binary::binary_mirror_status()
                .into_iter()
                .filter(|mirror| mirror.user_configured && mirror.enabled)
                .map(|mirror| mirror.id)
                .collect(),
        ),
        winget_downloader: winget.available.then_some(winget.value),
        created: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        frontend_settings,
    };
    let mut list = load();
    list.retain(|p| p.name != name);
    list.push(prof.clone());
    save_all(&list)?;
    Ok(prof)
}

#[tauri::command]
pub fn profile_list() -> Vec<Profile> {
    load()
}

/// 导出全部方案（供配置打包）。
pub fn export_all() -> Vec<Profile> {
    load()
}

/// 导入方案（按名覆盖同名）。返回写入数。
pub fn import_merge(incoming: Vec<Profile>) -> Result<usize, String> {
    let mut list = load();
    let mut n = 0;
    for p in incoming {
        list.retain(|x| x.name != p.name);
        list.push(p);
        n += 1;
    }
    save_all(&list)?;
    Ok(n)
}

#[tauri::command]
pub fn profile_delete(name: String) -> Result<(), String> {
    let mut list = load();
    let before = list.len();
    list.retain(|p| p.name != name);
    if list.len() == before {
        return Err(format!("方案不存在：{name}"));
    }
    save_all(&list)
}

/// 套用命名方案：逐工具切源（仅已安装、与当前不同的才动），再按记录让各处代理跟随系统
/// 或全部撤销、开关大文件镜像、设定 WinGet 下载器。返回实际改动的项数与没能照做的说明。
#[tauri::command]
pub async fn profile_apply(name: String) -> Result<ApplyResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let prof = load()
            .into_iter()
            .find(|p| p.name == name)
            .ok_or("方案不存在")?;
        let tools = sources::tools();
        let mut changed = 0usize;
        for sel in &prof.sources {
            let Some(tool) = tools.iter().find(|t| t.id == sel.tool) else {
                continue;
            };
            // 当前已是目标源就跳过
            if sources::detect(tool).as_deref() == Some(sel.mirror.as_str()) {
                continue;
            }
            let Some(mirror) = tool.mirrors.iter().find(|m| m.id == sel.mirror) else {
                continue;
            };
            sources::apply(tool, mirror)?;
            if mirror.id.starts_with("custom:") {
                crate::custom::apply_auth(&tool.handler, mirror)?;
            }
            changed += 1;
        }
        let mut notes = Vec::new();
        // The proxy, as the proxy page does it: every place follows the system, or none has one.
        if let Some(mode) = prof.proxy_mode.as_deref() {
            let report = crate::proxy_ledger::sync_report();
            let system_on = report.system.state == crate::proxy_system::SystemState::On;
            let differ = report.rows.len();
            if mode == FOLLOW && !system_on {
                notes.push(
                    "方案要求各处代理跟随系统，但 Windows 现在没有开启系统代理，代理没有改动。"
                        .to_string(),
                );
            } else if differ > 0 {
                match crate::proxy_ledger::follow_system(mode == RELEASE) {
                    Ok(_) => changed += differ,
                    Err(error) => notes.push(format!("代理没能全部设好：{error}")),
                }
            }
        }
        if let Some(wanted) = &prof.binary_mirrors {
            for mirror in crate::binary::binary_mirror_status() {
                let on = wanted.contains(&mirror.id);
                let done = if on && !(mirror.enabled && mirror.user_configured) {
                    Some(crate::binary::binary_mirror_apply(mirror.id.clone()))
                } else if !on && mirror.user_configured {
                    Some(crate::binary::binary_mirror_clear(mirror.id.clone()))
                } else {
                    None
                };
                match done {
                    Some(Ok(_)) => changed += 1,
                    Some(Err(error)) => notes.push(format!("{}：{error}", mirror.name)),
                    None => {}
                }
            }
        }
        if let Some(value) = prof.winget_downloader.as_deref() {
            let current = crate::winget_settings::status();
            if current.available && current.value != value {
                match crate::winget_settings::set(value) {
                    Ok(_) => changed += 1,
                    Err(error) => notes.push(format!("WinGet 下载器：{error}")),
                }
            }
        }
        Ok(ApplyResult {
            changed,
            frontend_settings: prof.frontend_settings,
            notes,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::Profile;

    #[test]
    fn an_older_profile_leaves_the_proxy_mirrors_and_winget_alone() {
        let profile: Profile = serde_json::from_str(
            r#"{"name":"旧方案","sources":[],"proxy":true,"created":"2026-01-01 00:00"}"#,
        )
        .unwrap();
        assert_eq!(profile.proxy_mode, None);
        assert_eq!(profile.binary_mirrors, None);
        assert_eq!(profile.winget_downloader, None);
    }

    #[test]
    fn the_proxy_is_recorded_as_one_state_or_not_at_all() {
        assert_eq!(super::proxy_mode_of(true, 0), Some(super::FOLLOW));
        assert_eq!(super::proxy_mode_of(true, 3), Some(super::FOLLOW));
        assert_eq!(super::proxy_mode_of(false, 0), Some(super::RELEASE));
        assert_eq!(
            super::proxy_mode_of(false, 2),
            None,
            "leftovers: no single state was chosen"
        );
    }

    #[test]
    fn old_profile_defaults_frontend_settings() {
        let profile: Profile = serde_json::from_str(
            r#"{"name":"旧方案","sources":[],"proxy":false,"created":"2026-01-01 00:00"}"#,
        )
        .expect("旧版方案应继续可读");
        assert!(profile.frontend_settings.is_empty());
    }
}
