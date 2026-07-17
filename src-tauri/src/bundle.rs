//! 配置导入/导出：把命名方案 + 自定义源定义打成一个 JSON 配置包，便于换机迁移。
//! 出于安全，自定义源的密码不导出（DPAPI 密文跨机/换用户无法解，明文又不安全），
//! 导入后需在新机重新填密码。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{custom, profile};

#[derive(Serialize, Deserialize)]
pub struct Bundle {
    pub version: u32,
    pub app: String,
    #[serde(default)]
    pub profiles: Vec<profile::Profile>,
    #[serde(default)]
    pub customs: Vec<custom::CustomDTO>,
    #[serde(default)]
    pub frontend_settings: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub profiles: usize,
    pub customs: usize,
    pub frontend_settings: BTreeMap<String, String>,
}

#[tauri::command]
pub fn bundle_export(
    path: String,
    frontend_settings: BTreeMap<String, String>,
) -> Result<(), String> {
    let b = Bundle {
        version: 2,
        app: "stacker".into(),
        profiles: profile::export_all(),
        customs: custom::export_all(),
        frontend_settings,
    };
    let s = serde_json::to_string_pretty(&b).map_err(|e| e.to_string())?;
    std::fs::write(&path, s).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn bundle_import(path: String) -> Result<ImportResult, String> {
    let s = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let b: Bundle = serde_json::from_str(&s)
        .map_err(|_| "文件格式无法识别（需 Stacker 导出的配置 JSON）".to_string())?;
    if b.app != "stacker" {
        return Err("这不是 Stacker 配置文件".into());
    }
    let profiles = profile::import_merge(b.profiles)?;
    let customs = custom::import_merge(b.customs)?;
    Ok(ImportResult {
        profiles,
        customs,
        frontend_settings: b.frontend_settings,
    })
}

#[cfg(test)]
mod tests {
    use super::Bundle;

    #[test]
    fn version_one_bundle_defaults_frontend_settings() {
        let bundle: Bundle =
            serde_json::from_str(r#"{"version":1,"app":"stacker","profiles":[],"customs":[]}"#)
                .expect("旧版配置包应继续可读");
        assert!(bundle.frontend_settings.is_empty());
    }
}
