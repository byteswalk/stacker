//! Project virtual environments: a `.venv` made from a pyenv version.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Folder names checked for a project's virtual environment, in order.
const VENV_NAMES: [&str; 3] = [".venv", "venv", "env"];

#[derive(Serialize, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VenvInfo {
    pub project: String,
    /// The virtual environment folder, when the project has one.
    pub dir: Option<String>,
    pub python: Option<String>,
    /// `version` from pyvenv.cfg.
    pub version: Option<String>,
    /// `home` from pyvenv.cfg: the Python it was made from.
    pub base: Option<String>,
    /// Whether that Python still exists; a venv breaks when its base is removed.
    pub base_exists: bool,
}

/// `key = value` lines of pyvenv.cfg.
pub(crate) fn cfg_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().to_string())
        })
        .filter(|v| !v.is_empty())
}

pub(crate) fn inspect(project: &Path) -> VenvInfo {
    let mut info = VenvInfo {
        project: project.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let Some(dir) = VENV_NAMES
        .iter()
        .map(|name| project.join(name))
        .find(|dir| dir.join("pyvenv.cfg").is_file())
    else {
        return info;
    };
    let cfg = std::fs::read_to_string(dir.join("pyvenv.cfg")).unwrap_or_default();
    info.version = cfg_value(&cfg, "version").or_else(|| cfg_value(&cfg, "version_info"));
    info.base = cfg_value(&cfg, "home");
    info.base_exists = info
        .base
        .as_ref()
        .is_some_and(|home| Path::new(home).is_dir());
    let python = dir.join("Scripts").join("python.exe");
    info.python = python
        .is_file()
        .then(|| python.to_string_lossy().into_owned());
    info.dir = Some(dir.to_string_lossy().into_owned());
    info
}

#[tauri::command]
pub fn python_venv_inspect(project: String) -> VenvInfo {
    inspect(Path::new(project.trim()))
}

/// `python -m venv <project>\.venv` with the chosen interpreter.
#[tauri::command]
pub async fn python_venv_create(project: String, python: String) -> Result<VenvInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let project = PathBuf::from(project.trim());
        if !project.is_dir() {
            return Err("项目目录不存在".to_string());
        }
        let existing = inspect(&project);
        if let Some(dir) = existing.dir {
            return Err(format!("项目里已有虚拟环境：{dir}"));
        }
        let python = PathBuf::from(python.trim());
        if !python.is_file() {
            return Err(format!("找不到解释器：{}", python.display()));
        }
        let target = project.join(".venv");
        let target_text = target.to_string_lossy().into_owned();
        crate::agents::process::run_command_text(
            &python,
            &["-m", "venv", &target_text],
            "python -m venv",
            std::time::Duration::from_secs(300),
        )?;
        let info = inspect(&project);
        if info.python.is_none() {
            return Err("虚拟环境没有创建成功：.venv\\Scripts\\python.exe 不存在".into());
        }
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Moves a project's virtual environment to the Recycle Bin.
#[tauri::command]
pub async fn python_venv_remove(dir: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = PathBuf::from(dir.trim());
        if !dir.join("pyvenv.cfg").is_file() {
            return Err("这不是虚拟环境目录（没有 pyvenv.cfg），不删除".to_string());
        }
        #[cfg(windows)]
        {
            crate::python_env::recycle(&dir)
        }
        #[cfg(not(windows))]
        {
            Err("仅支持 Windows".to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pyvenv_cfg() {
        let cfg =
            "home = C:\\py\\3.12\r\ninclude-system-site-packages = false\r\nversion = 3.12.9\r\n";
        assert_eq!(cfg_value(cfg, "home").as_deref(), Some("C:\\py\\3.12"));
        assert_eq!(cfg_value(cfg, "version").as_deref(), Some("3.12.9"));
        assert_eq!(cfg_value(cfg, "executable"), None);
    }

    #[test]
    fn finds_a_project_venv() {
        let root = std::env::temp_dir().join(format!("stacker-venv-{}", std::process::id()));
        let venv = root.join("venv");
        std::fs::create_dir_all(venv.join("Scripts")).unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            "home = C:\\nowhere\nversion = 3.11.2\n",
        )
        .unwrap();
        let info = inspect(&root);
        assert_eq!(info.dir.as_deref(), Some(venv.to_string_lossy().as_ref()));
        assert_eq!(info.version.as_deref(), Some("3.11.2"));
        assert!(!info.base_exists);
        assert_eq!(info.python, None);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(inspect(&root).dir, None);
    }
}
