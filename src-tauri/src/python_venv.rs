//! Per-project Python environments: a `.venv` made from a pyenv version, and the embeddable
//! runtimes some projects ship next to their code.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Folder names checked for a project's virtual environment, in order.
const VENV_NAMES: [&str; 3] = [".venv", "venv", "env"];

static SCAN_CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Debug, Default, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VenvInfo {
    /// The folder the user added: a project, or the environment itself.
    pub project: String,
    /// `venv` (a virtual environment), `embedded` (a Python shipped with the project), or
    /// `none` (nothing found).
    pub kind: String,
    /// The environment folder, when there is one.
    pub dir: Option<String>,
    pub python: Option<String>,
    /// `version` from pyvenv.cfg, or what an embedded python.exe reports.
    pub version: Option<String>,
    /// `home` from pyvenv.cfg: the Python it was made from.
    pub base: Option<String>,
    /// Whether that Python still exists; a venv breaks when its base is removed.
    pub base_exists: bool,
    /// A pip.ini inside the environment, which overrides the user's pip.ini for it.
    pub pip_ini: Option<String>,
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

/// An embeddable Python: `python.exe` beside a `pythonXY._pth` file, which is how the
/// embeddable package pins its own search path. Projects ship these inside their releases.
pub(crate) fn is_embedded(dir: &Path) -> bool {
    if !dir.join("python.exe").is_file() {
        return false;
    }
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with("._pth")
            })
        })
        .unwrap_or(false)
}

fn venv_dir_of(path: &Path) -> Option<PathBuf> {
    if path.join("pyvenv.cfg").is_file() {
        return Some(path.to_path_buf());
    }
    VENV_NAMES
        .iter()
        .map(|name| path.join(name))
        .find(|dir| dir.join("pyvenv.cfg").is_file())
}

fn with_pip_ini(dir: &Path, mut info: VenvInfo) -> VenvInfo {
    let pip_ini = dir.join("pip.ini");
    info.pip_ini = pip_ini
        .is_file()
        .then(|| pip_ini.to_string_lossy().into_owned());
    info
}

/// What the given folder holds: a virtual environment (its own, or one below it), an
/// embeddable runtime, or nothing.
pub(crate) fn inspect(path: &Path) -> VenvInfo {
    let mut info = VenvInfo {
        project: path.to_string_lossy().into_owned(),
        kind: "none".into(),
        ..Default::default()
    };
    if let Some(dir) = venv_dir_of(path) {
        let cfg = std::fs::read_to_string(dir.join("pyvenv.cfg")).unwrap_or_default();
        info.kind = "venv".into();
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
        return with_pip_ini(&dir, info);
    }
    if is_embedded(path) {
        let python = path.join("python.exe");
        info.kind = "embedded".into();
        info.version = crate::pyenv::python_exe_version(&python);
        // An embeddable runtime carries its own files, so nothing else has to exist.
        info.base_exists = true;
        info.python = Some(python.to_string_lossy().into_owned());
        info.dir = Some(path.to_string_lossy().into_owned());
        return with_pip_ini(path, info);
    }
    info
}

#[tauri::command]
pub fn python_venv_inspect(project: String) -> VenvInfo {
    inspect(Path::new(project.trim()))
}

fn create_venv(python: &Path, target: &Path) -> Result<(), String> {
    if !python.is_file() {
        return Err(format!("找不到解释器：{}", python.display()));
    }
    crate::agents::process::run_command_text(
        python,
        &["-m", "venv", &target.to_string_lossy()],
        "python -m venv",
        std::time::Duration::from_secs(600),
    )?;
    Ok(())
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
            return Err(format!("这个目录里已有 Python 环境：{dir}"));
        }
        let target = project.join(".venv");
        create_venv(Path::new(python.trim()), &target)?;
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

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RebuildResult {
    pub info: VenvInfo,
    /// What happened, in the order it happened.
    pub notes: Vec<String>,
}

/// Rebuilds a virtual environment on another Python version. A venv cannot change its
/// interpreter in place: the old one is recycled and a new one is created in the same folder,
/// with the packages reinstalled from `pip freeze` unless the caller declines.
#[tauri::command]
pub async fn python_venv_rebuild(
    dir: String,
    python: String,
    keep_packages: bool,
) -> Result<RebuildResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = PathBuf::from(dir.trim());
        if !dir.join("pyvenv.cfg").is_file() {
            return Err("这不是虚拟环境目录（没有 pyvenv.cfg）".to_string());
        }
        let python = PathBuf::from(python.trim());
        if !python.is_file() {
            return Err(format!("找不到解释器：{}", python.display()));
        }
        let mut notes = Vec::new();
        let old_python = dir.join("Scripts").join("python.exe");
        let mut requirements = None;
        if keep_packages && old_python.is_file() {
            match crate::agents::process::run_command_text(
                &old_python,
                &["-m", "pip", "freeze", "--local"],
                "pip freeze",
                std::time::Duration::from_secs(300),
            ) {
                Ok(text) => {
                    let list: Vec<&str> = text
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .collect();
                    if list.is_empty() {
                        notes.push("原环境没有装任何第三方包".into());
                    } else {
                        let file = std::env::temp_dir()
                            .join(format!("stacker-requirements-{}.txt", std::process::id()));
                        std::fs::write(&file, list.join("\r\n"))
                            .map_err(|e| format!("无法记录已装的包：{e}"))?;
                        notes.push(format!("已记录 {} 个已装的包", list.len()));
                        requirements = Some(file);
                    }
                }
                Err(e) => notes.push(format!("读取已装的包失败（{e}），重建后需要自己重装依赖")),
            }
        }
        let pip_ini = dir.join("pip.ini");
        let kept_pip_ini = pip_ini
            .is_file()
            .then(|| std::fs::read(&pip_ini).ok())
            .flatten();
        #[cfg(windows)]
        crate::python_env::recycle(&dir)?;
        notes.push("原虚拟环境已移到回收站".into());
        create_venv(&python, &dir)?;
        if let Some(content) = kept_pip_ini {
            if std::fs::write(&pip_ini, content).is_ok() {
                notes.push("环境内的 pip.ini 已还原".into());
            }
        }
        let new_python = dir.join("Scripts").join("python.exe");
        if !new_python.is_file() {
            return Err("新的虚拟环境没有创建成功".into());
        }
        notes.push("已用新版本创建虚拟环境".into());
        if let Some(file) = requirements {
            match crate::agents::process::run_command_text(
                &new_python,
                &["-m", "pip", "install", "-r", &file.to_string_lossy()],
                "pip install -r",
                std::time::Duration::from_secs(3600),
            ) {
                Ok(_) => notes.push("依赖已重新安装".into()),
                Err(e) => notes.push(format!(
                    "依赖重装失败（{e}）。已装包清单还在 {}，可以自己重试",
                    file.display()
                )),
            }
        }
        let project = PathBuf::from(inspect(&dir).project);
        Ok(RebuildResult {
            info: inspect(&project),
            notes,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- scanning the disks for project environments --------------------------------------------

#[tauri::command]
pub fn python_env_scan_cancel() {
    SCAN_CANCEL.store(true, Ordering::SeqCst);
}

/// Walks the given roots for virtual environments and embeddable runtimes. It reports what it
/// finds; nothing is changed.
#[tauri::command]
pub async fn python_env_scan(window: tauri::Window, roots: Vec<String>) -> Vec<VenvInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        SCAN_CANCEL.store(false, Ordering::SeqCst);
        let mut found: Vec<VenvInfo> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut walked = 0_usize;
        'outer: for root in roots {
            if root.trim().is_empty() {
                continue;
            }
            let walker = jwalk::WalkDir::new(root.trim())
                .skip_hidden(false)
                .process_read_dir(|_, _, _, children| {
                    children.retain(|child| {
                        child
                            .as_ref()
                            .ok()
                            .and_then(|entry| {
                                entry
                                    .file_type
                                    .is_dir()
                                    .then(|| entry.file_name.to_str())
                                    .flatten()
                            })
                            .map_or(true, |name| !crate::env::is_noise(name))
                    });
                });
            for entry in walker {
                if SCAN_CANCEL.load(Ordering::SeqCst) {
                    break 'outer;
                }
                let Ok(entry) = entry else { continue };
                if entry.file_type.is_dir() {
                    walked += 1;
                    if walked % 400 == 0 {
                        let _ = window.emit(
                            "python-env-scan-progress",
                            entry.path().to_string_lossy().to_string(),
                        );
                    }
                    continue;
                }
                let name = entry.file_name.to_string_lossy().to_ascii_lowercase();
                let is_marker = name == "pyvenv.cfg" || name.ends_with("._pth");
                if !is_marker {
                    continue;
                }
                let Some(dir) = entry.path().parent().map(Path::to_path_buf) else {
                    continue;
                };
                if !seen.insert(dir.to_string_lossy().to_lowercase()) {
                    continue;
                }
                let info = inspect(&dir);
                if info.kind != "none" {
                    found.push(info);
                }
            }
        }
        let _ = window.emit("python-env-scan-progress", "__done__".to_string());
        found.sort_by_key(|info| info.project.to_lowercase());
        found
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "stacker-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

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
        let root = temp("venv");
        let venv = root.join("venv");
        std::fs::create_dir_all(venv.join("Scripts")).unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            "home = C:\\nowhere\nversion = 3.11.2\n",
        )
        .unwrap();
        std::fs::write(venv.join("pip.ini"), "[global]\n").unwrap();
        let info = inspect(&root);
        assert_eq!(info.kind, "venv");
        assert_eq!(info.dir.as_deref(), Some(venv.to_string_lossy().as_ref()));
        assert_eq!(info.version.as_deref(), Some("3.11.2"));
        assert!(!info.base_exists);
        assert_eq!(info.python, None);
        assert!(info.pip_ini.is_some());
        // The environment folder itself reports the same environment.
        assert_eq!(inspect(&venv).dir, info.dir);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(inspect(&root).kind, "none");
    }

    #[test]
    fn recognises_an_embedded_runtime() {
        let root = temp("embed");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("python.exe"), b"not really").unwrap();
        assert!(
            !is_embedded(&root),
            "a python.exe alone is a normal install"
        );
        std::fs::write(root.join("python313._pth"), b"python313.zip\n.\n").unwrap();
        assert!(is_embedded(&root));
        let info = inspect(&root);
        assert_eq!(info.kind, "embedded");
        assert!(info.base_exists);
        assert_eq!(info.dir.as_deref(), Some(root.to_string_lossy().as_ref()));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
