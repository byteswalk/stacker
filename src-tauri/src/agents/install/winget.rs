use crate::agents::process::*;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn winget_args(
    action: &str,
    id: &str,
    source: Option<&str>,
    exact: bool,
) -> Vec<String> {
    // WinGet rejects --proxy unless an administrator enabled ProxyCommandLineOptions,
    // so it uses its own system proxy handling; child env still carries Stacker's proxy.
    winget_args_with_proxy(action, id, source, exact, None)
}

pub(crate) fn winget_args_with_proxy(
    action: &str,
    id: &str,
    source: Option<&str>,
    exact: bool,
    proxy: Option<&str>,
) -> Vec<String> {
    let mut args = vec![action.to_string(), "--id".into(), id.into()];
    if exact {
        args.push("--exact".into());
    }
    if let Some(source) = source {
        args.push("--source".into());
        args.push(source.into());
    }
    args.push("--accept-source-agreements".into());
    args.push("--disable-interactivity".into());
    if let Some(proxy) = proxy.filter(|value| !value.trim().is_empty()) {
        args.push("--proxy".into());
        args.push(proxy.to_string());
    }
    if action == "install" || action == "upgrade" {
        args.push("--accept-package-agreements".into());
        args.push("--silent".into());
    }
    args
}

pub(crate) fn winget_command() -> Option<PathBuf> {
    resolve_command_including_windowsapps(&["winget.exe", "winget.cmd", "winget.bat"])
        .or_else(|| known_winget_paths().into_iter().find(|p| p.exists()))
}

pub(crate) fn known_winget_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("Microsoft\\WindowsApps\\winget.exe"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        out.push(PathBuf::from(profile).join("AppData\\Local\\Microsoft\\WindowsApps\\winget.exe"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let windows_apps = PathBuf::from(program_files).join("WindowsApps");
        if let Ok(entries) = std::fs::read_dir(windows_apps) {
            let mut app_installer_paths = entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    name.starts_with("Microsoft.DesktopAppInstaller_")
                        .then(|| entry.path().join("winget.exe"))
                })
                .filter(|path| path.exists())
                .collect::<Vec<_>>();
            app_installer_paths.sort();
            app_installer_paths.reverse();
            out.extend(app_installer_paths);
        }
    }
    out
}

pub(crate) fn scoop_command() -> Option<PathBuf> {
    resolve_command_including_windowsapps(&["scoop.cmd", "scoop.exe", "scoop.bat"])
}

pub(crate) fn choco_command() -> Option<PathBuf> {
    resolve_command_including_windowsapps(&["choco.exe", "choco.cmd", "choco.bat"])
}

pub(crate) fn run_winget(args: &[&str], timeout: Duration) -> Result<String, String> {
    let winget = winget_command().ok_or_else(|| "未检测到 WinGet。".to_string())?;
    run_command_text(&winget, args, "winget", timeout)
}

pub(crate) fn run_winget_owned(
    args: Vec<String>,
    timeout: Duration,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let winget = winget_command().ok_or_else(|| "未检测到 WinGet。".to_string())?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_command_streamed(&winget, &refs, "WinGet", timeout, Duration::ZERO, window)
}

pub(crate) fn run_scoop(args: &[&str], timeout: Duration) -> Result<String, String> {
    let scoop = scoop_command().ok_or_else(|| "未检测到 Scoop。".to_string())?;
    run_command_text(&scoop, args, "scoop", timeout)
}

pub(crate) fn run_choco(args: &[&str], timeout: Duration) -> Result<String, String> {
    let choco = choco_command().ok_or_else(|| "未检测到 Chocolatey。".to_string())?;
    run_command_text(&choco, args, "choco", timeout)
}

pub(crate) fn winget_query_is_read_only(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("show") | Some("list")
    ) && !args.iter().any(|arg| {
        matches!(
            arg.to_ascii_lowercase().as_str(),
            "install" | "upgrade" | "uninstall"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winget_uses_configured_proxy_without_separate_verbose_log() {
        let args = winget_args_with_proxy(
            "install",
            "9PLM9XGG6VKS",
            Some("msstore"),
            true,
            Some("http://127.0.0.1:7897"),
        );
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--proxy", "http://127.0.0.1:7897"]));
        assert!(!args.iter().any(|arg| arg == "--verbose-logs"));
        assert!(args.windows(2).any(|pair| pair == ["--source", "msstore"]));
    }

    #[test]
    fn winget_background_version_query_is_read_only() {
        let args = winget_args_with_proxy(
            "show",
            "Anthropic.ClaudeCode",
            None,
            true,
            Some("http://127.0.0.1:7897"),
        );
        assert!(winget_query_is_read_only(&args));
        assert_eq!(args.first().map(String::as_str), Some("show"));
        assert!(!args.iter().any(|arg| {
            matches!(
                arg.to_ascii_lowercase().as_str(),
                "install" | "upgrade" | "uninstall"
            )
        }));
    }
}
