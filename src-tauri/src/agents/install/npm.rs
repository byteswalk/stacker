use crate::agents::{process::*, registry::*, *};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) fn update_with_npm_source(
    spec: &ToolSpec,
    program: &Path,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let pkg = spec
        .cli
        .npm_package
        .ok_or_else(|| format!("{} 不是 npm 包。", spec.cli.name))?;
    emit_progress(window, format!("正在通过 npm 更新 {}…", spec.cli.name));
    npm_install_latest(pkg, Some(program), window)
}

pub(crate) fn npm_install_latest(
    package: &str,
    installed_program: Option<&Path>,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let npm = npm_for_program(installed_program)
        .or_else(|| resolve_command(&["npm.cmd", "npm.exe", "npm.bat"]))
        .ok_or_else(|| "未检测到 npm。请先在 Node 页面安装并设置默认 Node。".to_string())?;
    validate_npm_proxy(&npm)?;
    let spec = format!("{package}@latest");
    run_command_streamed(
        &npm,
        &[
            "install",
            "-g",
            &spec,
            "--fetch-retries=2",
            "--fetch-retry-mintimeout=1000",
            "--fetch-retry-maxtimeout=5000",
            "--fetch-timeout=30000",
        ],
        "npm install",
        Duration::from_secs(900),
        Duration::ZERO,
        window,
    )?;
    Ok(())
}

pub(crate) fn npm_uninstall(
    package: &str,
    installed_program: Option<&Path>,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let npm = npm_for_program(installed_program)
        .or_else(|| resolve_command(&["npm.cmd", "npm.exe", "npm.bat"]))
        .ok_or_else(|| "未检测到 npm。".to_string())?;
    run_command_streamed(
        &npm,
        &["uninstall", "-g", package],
        "npm uninstall",
        Duration::from_secs(900),
        Duration::ZERO,
        window,
    )?;
    Ok(())
}

pub(crate) fn npm_for_program(program: Option<&Path>) -> Option<PathBuf> {
    let program = program?;
    let dir = program.parent()?;
    for name in ["npm.cmd", "npm.exe", "npm.bat"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub(crate) fn validate_npm_proxy(npm: &Path) -> Result<(), String> {
    use std::net::{SocketAddr, TcpStream};

    let proxy_mode = crate::settings::proxy_mode();
    if proxy_mode == "system" {
        // Windows proxy settings can change while Stacker remains open. Refresh the current
        // explicit endpoint immediately before npm operations; routing mode is not inferred.
        crate::settings::settings_set_proxy_mode("system".into())?;
    } else if proxy_mode == "off" {
        // Upgrade old installations by removing proxy entries written by previous versions.
        crate::proxy::sync_existing_explicit_proxies(None, 0)?;
    }

    for key in ["proxy", "https-proxy"] {
        let value = run_command_text(
            npm,
            &["config", "get", key],
            "读取 npm 代理配置",
            Duration::from_secs(5),
        )?;
        let proxy = value.trim().trim_matches('"');
        if proxy.is_empty() || proxy.eq_ignore_ascii_case("null") {
            continue;
        }

        let address = proxy
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(proxy)
            .split('/')
            .next()
            .unwrap_or_default();
        let Some((host, port)) = address.rsplit_once(':') else {
            continue;
        };
        if host != "127.0.0.1" && !host.eq_ignore_ascii_case("localhost") {
            continue;
        }
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        let socket = SocketAddr::from(([127, 0, 0, 1], port));
        if TcpStream::connect_timeout(&socket, Duration::from_millis(1200)).is_err() {
            return Err(format!(
                "npm 的 {key} 配置指向本机代理 {proxy}，但该端口当前不可连接。请在「设置」中同步当前网络设置，或清除 npm 的 proxy/https-proxy 后重试"
            ));
        }
    }
    Ok(())
}
