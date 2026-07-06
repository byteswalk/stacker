//! 命名方案：把"当前各工具的源选择 + 代理开关"存成命名快照，一键套用 / 删除。
//! 存储 %APPDATA%\stacker\profiles.json（明文，仅记录内置镜像 id，无密码）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{proxy, sources};

#[derive(Serialize, Deserialize, Clone)]
pub struct SourceSel {
    pub tool: String,   // 工具 id（pip/npm/go/...）
    pub mirror: String, // 镜像 id（official/tsinghua/...）
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub name: String,
    pub sources: Vec<SourceSel>,
    pub proxy: bool, // 期望代理是否开启
    pub created: String,
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
pub fn profile_save(name: String) -> Result<Profile, String> {
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
    let prof = Profile {
        name: name.clone(),
        sources,
        proxy: proxy::status().enabled,
        created: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
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

/// 套用命名方案：逐工具切源（仅已安装、与当前不同的才动），再按记录开/关代理。
/// 返回实际改动的工具数。
#[tauri::command]
pub fn profile_apply(name: String) -> Result<usize, String> {
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
    // 代理对齐
    let ps = proxy::status();
    if prof.proxy && !ps.enabled {
        let port = if ps.port > 0 {
            ps.port
        } else {
            ps.detected_port.unwrap_or(7890)
        };
        proxy::enable(&ps.host, port, false, vec![])?;
    } else if !prof.proxy && ps.enabled {
        proxy::disable(false)?;
    }
    Ok(changed)
}
