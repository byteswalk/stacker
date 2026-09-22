//! What decides which `python` a terminal (or an AI agent in it) runs, and removing Python
//! runtimes found on the machine that Stacker does not manage.
//!
//! Windows builds a process's PATH as the system PATH followed by the user PATH, so a Python
//! on the system PATH wins over the pyenv default Stacker writes to the user PATH. The report
//! lists every `python` in that real order, so the summary given to an AI can say which one
//! runs and whether it is the default.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PathPython {
    /// The PATH entry, expanded.
    pub dir: String,
    /// The file found there (`python.exe`, or pyenv's `python.bat` shim).
    pub program: String,
    /// `system` or `user`: which PATH it comes from.
    pub scope: String,
    /// `default` (the pyenv default's own folder), `pyenv-shim`, `store-alias` (the Microsoft
    /// Store stub that opens the Store), `venv`, or `other`.
    pub kind: String,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PythonEnvReport {
    pub pyenv_root: Option<String>,
    pub pyenv_bin: Option<String>,
    pub pyenv_shims: Option<String>,
    pub default_version: Option<String>,
    pub default_dir: Option<String>,
    pub default_python: Option<String>,
    pub default_scripts: Option<String>,
    /// Every `python` on PATH in the order Windows searches: system PATH, then user PATH.
    pub path_pythons: Vec<PathPython>,
    /// Whether the first `python` found is the pyenv default (its folder or pyenv's shim).
    pub first_is_default: bool,
    /// The Windows Python Launcher (`py`), which picks versions its own way.
    pub py_launcher: Option<String>,
    /// PYTHONHOME / PYTHONPATH set in the user or system environment: they redirect any Python.
    pub overrides: Vec<(String, String)>,
    /// What `start python` and the Run dialog open: the App Paths registration, which
    /// ignores PATH.
    pub app_paths_python: Option<String>,
}

/// `%NAME%` expanded from this process's environment; unknown names are left as written.
pub(crate) fn expand_env(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn same_dir(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// What a PATH folder holding a `python` is.
pub(crate) fn classify(
    dir: &str,
    pyenv_root: Option<&str>,
    default_dir: Option<&str>,
) -> &'static str {
    let lower = dir.to_lowercase().replace('/', "\\");
    if default_dir.is_some_and(|d| same_dir(dir, d)) {
        return "default";
    }
    if let Some(root) = pyenv_root {
        if same_dir(dir, &format!("{}shims", with_slash(root))) {
            return "pyenv-shim";
        }
    }
    if lower.contains("\\microsoft\\windowsapps") {
        return "store-alias";
    }
    if lower.ends_with("\\scripts")
        && Path::new(dir)
            .parent()
            .is_some_and(|p| p.join("pyvenv.cfg").is_file())
    {
        return "venv";
    }
    "other"
}

/// Which programs named `python` a folder holds.
fn python_in(dir: &Path) -> Option<String> {
    ["python.exe", "python.bat", "python.cmd"]
        .iter()
        .find(|name| dir.join(name).is_file())
        .map(|name| name.to_string())
}

#[cfg(windows)]
pub(crate) fn report() -> PythonEnvReport {
    use crate::winenv::{get_path_in, get_raw_in, Hive};
    let status = crate::pyenv::pyenv_status_snapshot();
    let root = crate::pyenv::pyenv_root_path();
    let default_version = status
        .versions
        .iter()
        .find(|v| v.is_default)
        .map(|v| v.version.clone())
        .or(status.default.clone());
    let default_dir = match (&root, &default_version) {
        (Some(root), Some(version)) => {
            let dir = PathBuf::from(root).join("versions").join(version);
            dir.is_dir().then(|| dir.to_string_lossy().into_owned())
        }
        _ => None,
    };
    let mut path_pythons = Vec::new();
    for (hive, scope) in [(Hive::System, "system"), (Hive::User, "user")] {
        for entry in get_path_in(hive) {
            let dir = expand_env(&entry);
            if let Some(program) = python_in(Path::new(&dir)) {
                let kind = classify(&dir, root.as_deref(), default_dir.as_deref());
                path_pythons.push(PathPython {
                    dir,
                    program,
                    scope: scope.into(),
                    kind: kind.into(),
                });
            }
        }
    }
    let first_is_default = path_pythons
        .first()
        .is_some_and(|p| p.kind == "default" || p.kind == "pyenv-shim");
    let py_launcher = [
        std::env::var("SystemRoot")
            .map(|r| PathBuf::from(r).join("py.exe"))
            .ok(),
        std::env::var("LOCALAPPDATA")
            .map(|l| PathBuf::from(l).join(r"Programs\Python\Launcher\py.exe"))
            .ok(),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
    .map(|p| p.to_string_lossy().into_owned());
    let mut overrides = Vec::new();
    for (hive, scope) in [(Hive::System, "系统"), (Hive::User, "用户")] {
        for name in ["PYTHONHOME", "PYTHONPATH"] {
            if let Some(value) = get_raw_in(hive, name) {
                overrides.push((format!("{scope} {name}"), value));
            }
        }
    }
    let app_paths_python = [
        winreg::enums::HKEY_CURRENT_USER,
        winreg::enums::HKEY_LOCAL_MACHINE,
    ]
    .into_iter()
    .find_map(|hive| {
        let key = winreg::RegKey::predef(hive)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\App Paths\python.exe")
            .ok()?;
        let target: String = key.get_value("").ok()?;
        (!target.trim().is_empty()).then(|| expand_env(target.trim().trim_matches('"')))
    });
    PythonEnvReport {
        pyenv_bin: root.as_ref().map(|r| format!("{}bin", with_slash(r))),
        pyenv_shims: root.as_ref().map(|r| format!("{}shims", with_slash(r))),
        pyenv_root: root,
        default_python: default_dir.as_ref().map(|d| {
            PathBuf::from(d)
                .join("python.exe")
                .to_string_lossy()
                .into_owned()
        }),
        default_scripts: default_dir.as_ref().map(|d| {
            PathBuf::from(d)
                .join("Scripts")
                .to_string_lossy()
                .into_owned()
        }),
        default_dir,
        default_version,
        path_pythons,
        first_is_default,
        py_launcher,
        overrides,
        app_paths_python,
    }
}

#[cfg(not(windows))]
pub(crate) fn report() -> PythonEnvReport {
    PythonEnvReport::default()
}

fn with_slash(path: &str) -> String {
    if path.ends_with('\\') || path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}\\")
    }
}

#[tauri::command]
pub async fn python_env_report() -> PythonEnvReport {
    tauri::async_runtime::spawn_blocking(report)
        .await
        .unwrap_or_default()
}

// ---- removing Python runtimes Stacker found but does not manage ---------------------------

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RemovalResult {
    pub path: String,
    pub ok: bool,
    /// `uninstaller` (its own uninstaller ran) or `recycle` (moved to the Recycle Bin).
    pub method: String,
    pub message: String,
}

/// The install folder a scanned path names (the folder, or `python.exe` in it).
pub(crate) fn runtime_dir(path: &str) -> PathBuf {
    let p = PathBuf::from(path.trim());
    if p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("python.exe"))
    {
        p.parent().map(Path::to_path_buf).unwrap_or(p)
    } else {
        p
    }
}

/// Why a folder must not be removed from here, if it must not.
pub(crate) fn removal_refusal(
    dir: &Path,
    pyenv_root: Option<&str>,
    default_dir: Option<&str>,
) -> Option<&'static str> {
    let text = dir.to_string_lossy().replace('/', "\\");
    let lower = text.to_lowercase();
    if !dir.is_absolute() || dir.components().count() < 3 {
        return Some("为保护磁盘数据，拒绝删除过短的目录路径");
    }
    if lower.starts_with(r"c:\windows\") || lower.contains(r"\microsoft\windowsapps") {
        return Some("这是 Windows 或 Microsoft Store 的占位程序，请在 Windows 设置 → 应用 → 应用执行别名 中关闭");
    }
    if let Some(root) = pyenv_root {
        let root = root
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
            .replace('/', "\\");
        if lower == root || lower.starts_with(&format!("{root}\\")) {
            return Some("这是 pyenv 管理的版本，请在上方的版本列表里卸载");
        }
    }
    if default_dir.is_some_and(|d| same_dir(&text, d)) {
        return Some("这是当前默认的 Python，请先把其他版本设为默认");
    }
    if dir.join("pyvenv.cfg").is_file() {
        return Some("这是虚拟环境（venv），请在所属项目里删除");
    }
    None
}

/// The python.org uninstaller for the install in `dir`: an entry published by the Python
/// Software Foundation whose PythonCore registration points at this folder.
#[cfg(windows)]
fn python_org_uninstaller(dir: &Path) -> Option<(String, String)> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    // PythonCore\<tag>\InstallPath (default value) names the folder of a python.org install.
    let mut registered = false;
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Ok(core) = RegKey::predef(hive).open_subkey(r"Software\Python\PythonCore") {
            for tag in core.enum_keys().flatten() {
                let path: String = core
                    .open_subkey(format!(r"{tag}\InstallPath"))
                    .and_then(|k| k.get_value(""))
                    .unwrap_or_default();
                if !path.is_empty() && same_dir(&path, &dir.to_string_lossy()) {
                    registered = true;
                }
            }
        }
    }
    if !registered {
        return None;
    }
    for (hive, key) in [
        (
            HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ] {
        let Ok(list) = RegKey::predef(hive).open_subkey(key) else {
            continue;
        };
        for name in list.enum_keys().flatten() {
            let Ok(entry) = list.open_subkey(&name) else {
                continue;
            };
            let publisher: String = entry.get_value("Publisher").unwrap_or_default();
            let display: String = entry.get_value("DisplayName").unwrap_or_default();
            let quiet: String = entry.get_value("QuietUninstallString").unwrap_or_default();
            let plain: String = entry.get_value("UninstallString").unwrap_or_default();
            let bundle = plain.to_lowercase().contains("/uninstall");
            if !publisher.contains("Python Software Foundation") || !bundle {
                continue;
            }
            // The bundle ("Python 3.12.4 (64-bit)") of the version installed in this folder.
            let installed = crate::pyenv::python_exe_version(&dir.join("python.exe"));
            if installed
                .as_deref()
                .is_some_and(|v| display.starts_with(&format!("Python {v} ")))
            {
                let command = if quiet.is_empty() {
                    format!("{plain} /quiet")
                } else {
                    quiet
                };
                return Some((display, command));
            }
        }
    }
    None
}

/// `"C:\x\setup.exe" /uninstall /quiet` → program and arguments.
pub(crate) fn split_command(command: &str) -> Option<(String, Vec<String>)> {
    let command = command.trim();
    let (program, rest) = if let Some(stripped) = command.strip_prefix('"') {
        let end = stripped.find('"')?;
        (stripped[..end].to_string(), &stripped[end + 1..])
    } else {
        let end = command.find(' ').unwrap_or(command.len());
        (command[..end].to_string(), &command[end..])
    };
    let args = rest.split_whitespace().map(str::to_string).collect();
    Some((program, args))
}

/// Moves a folder to the Recycle Bin, so a mistaken removal can be undone.
#[cfg(windows)]
pub(crate) fn recycle(dir: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::shellapi::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };
    let from: Vec<u16> = dir.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE as u32,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `from` is a double-NUL-terminated path that outlives the call.
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 || op.fAnyOperationsAborted != 0 {
        return Err(format!(
            "移到回收站失败（代码 {code}），可能有程序正在使用其中的文件"
        ));
    }
    Ok(())
}

/// Drops the folder and its Scripts from the user PATH; says which system PATH entries remain.
#[cfg(windows)]
fn unlink_path(dir: &Path) -> Option<String> {
    use crate::winenv::{get_path_in, remove_path_in, Hive};
    let scripts = dir.join("Scripts");
    let targets = [
        dir.to_string_lossy().into_owned(),
        scripts.to_string_lossy().into_owned(),
    ];
    for entry in get_path_in(Hive::User) {
        if targets.iter().any(|t| same_dir(&expand_env(&entry), t)) {
            let _ = remove_path_in(Hive::User, &entry);
        }
    }
    let left: Vec<String> = get_path_in(Hive::System)
        .into_iter()
        .filter(|entry| targets.iter().any(|t| same_dir(&expand_env(entry), t)))
        .collect();
    (!left.is_empty()).then(|| {
        format!(
            "系统 PATH 里仍有 {}，需要管理员在系统环境变量中删除",
            left.join("、")
        )
    })
}

#[cfg(windows)]
fn remove_one(path: &str, pyenv_root: Option<&str>, default_dir: Option<&str>) -> RemovalResult {
    let dir = runtime_dir(path);
    let shown = dir.to_string_lossy().into_owned();
    let result = |ok: bool, method: &str, message: String| RemovalResult {
        path: shown.clone(),
        ok,
        method: method.into(),
        message,
    };
    if let Some(reason) = removal_refusal(&dir, pyenv_root, default_dir) {
        return result(false, "", reason.into());
    }
    if !dir.is_dir() {
        return result(false, "", "目录已不存在".into());
    }
    let mut method = "recycle";
    let mut notes = Vec::new();
    if let Some((name, command)) = python_org_uninstaller(&dir) {
        if let Some((program, args)) = split_command(&command) {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            match crate::agents::process::run_command_text(
                Path::new(&program),
                &refs,
                &name,
                std::time::Duration::from_secs(600),
            ) {
                Ok(_) => {
                    method = "uninstaller";
                    notes.push(format!("已运行 {name} 的卸载程序"));
                }
                Err(err) => notes.push(format!("{name} 的卸载程序未完成（{err}），改为移到回收站")),
            }
        }
    }
    if dir.is_dir() {
        if let Err(err) = recycle(&dir) {
            return result(false, method, [notes, vec![err]].concat().join("；"));
        }
        if method == "uninstaller" {
            notes.push("剩余文件已移到回收站".into());
        } else {
            notes.push("已移到回收站，可从回收站还原".into());
        }
    }
    if let Some(note) = unlink_path(&dir) {
        notes.push(note);
    }
    result(true, method, notes.join("；"))
}

#[tauri::command]
pub async fn python_remove_runtimes(paths: Vec<String>) -> Vec<RemovalResult> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        {
            let report = report();
            let out = paths
                .iter()
                .map(|p| {
                    remove_one(
                        p,
                        report.pyenv_root.as_deref(),
                        report.default_dir.as_deref(),
                    )
                })
                .collect();
            crate::winenv::broadcast_change();
            out
        }
        #[cfg(not(windows))]
        {
            paths
                .into_iter()
                .map(|path| RemovalResult {
                    path,
                    ok: false,
                    method: String::new(),
                    message: "仅支持 Windows".into(),
                })
                .collect::<Vec<_>>()
        }
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_entries_expand_their_variables() {
        std::env::set_var("STACKER_TEST_HOME", r"C:\Users\me");
        assert_eq!(
            expand_env(r"%STACKER_TEST_HOME%\.pyenv\shims"),
            r"C:\Users\me\.pyenv\shims"
        );
        assert_eq!(expand_env(r"%NO_SUCH_VAR_4417%\x"), r"%NO_SUCH_VAR_4417%\x");
        assert_eq!(expand_env("100% sure"), "100% sure");
    }

    #[test]
    fn python_folders_on_path_are_told_apart() {
        let root = Some(r"C:\Users\me\.pyenv\pyenv-win\");
        let default = Some(r"C:\Users\me\.pyenv\pyenv-win\versions\3.13.14");
        assert_eq!(
            classify(
                r"C:\Users\me\.pyenv\pyenv-win\versions\3.13.14",
                root,
                default
            ),
            "default"
        );
        assert_eq!(
            classify(r"C:\Users\me\.pyenv\pyenv-win\shims", root, default),
            "pyenv-shim"
        );
        assert_eq!(
            classify(
                r"C:\Users\me\AppData\Local\Microsoft\WindowsApps",
                root,
                default
            ),
            "store-alias"
        );
        assert_eq!(
            classify(r"C:\Program Files\Python312", root, default),
            "other"
        );
    }

    #[test]
    fn removal_refuses_what_is_not_a_stray_runtime() {
        let root = Some(r"C:\Users\me\.pyenv\pyenv-win\");
        let default = Some(r"C:\Users\me\.pyenv\pyenv-win\versions\3.13.14");
        let refuse = |p: &str| removal_refusal(Path::new(p), root, default);
        assert!(refuse(r"C:\Users\me\.pyenv\pyenv-win\versions\3.12.0")
            .unwrap()
            .contains("pyenv"));
        assert!(refuse(r"C:\Users\me\AppData\Local\Microsoft\WindowsApps")
            .unwrap()
            .contains("Store"));
        assert!(refuse(r"C:\Windows\py").is_some());
        assert!(refuse(r"C:\").is_some());
        assert!(refuse(r"D:\Python312").is_none());
        assert!(refuse(r"C:\Users\me\AppData\Local\Programs\Python\Python312").is_none());
        assert_eq!(
            runtime_dir(r"D:\Python312\python.exe"),
            PathBuf::from(r"D:\Python312")
        );
    }

    #[test]
    fn uninstall_commands_split_into_program_and_arguments() {
        assert_eq!(
            split_command(
                r#""C:\Users\me\AppData\Local\Package Cache\{x}\python-3.12.4-amd64.exe"  /uninstall /quiet"#
            ),
            Some((
                r"C:\Users\me\AppData\Local\Package Cache\{x}\python-3.12.4-amd64.exe".to_string(),
                vec!["/uninstall".to_string(), "/quiet".to_string()]
            ))
        );
        assert_eq!(split_command("setup.exe /x").unwrap().0, "setup.exe");
    }
}
