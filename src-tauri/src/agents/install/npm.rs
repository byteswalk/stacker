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
    let install = |registry: Option<&str>| {
        let mut args = vec![
            "install",
            "-g",
            &spec,
            "--fetch-retries=2",
            "--fetch-retry-mintimeout=1000",
            "--fetch-retry-maxtimeout=5000",
            "--fetch-timeout=30000",
            // npm draws no progress bar into a pipe; at this level it names every package as
            // it arrives (address, time taken, cache hit or miss), so a long update is seen
            // moving. Byte progress it does not report at all.
            "--loglevel=http",
        ];
        if let Some(registry) = registry {
            args.push(registry);
        }
        run_command_streamed(
            &npm,
            &args,
            "npm install",
            Duration::from_secs(900),
            Duration::ZERO,
            window,
        )
    };
    match install(None) {
        Ok(_) => Ok(()),
        Err(error) if mirror_is_behind(&error) => {
            emit_progress(
                window,
                "当前 npm 镜像里还没有这个版本（镜像同步延迟），正在改用 npm 官方源重试…",
            );
            install(Some(OFFICIAL_REGISTRY)).map(|_| ()).map_err(|second| {
                format!(
                    "{error}；改用官方源后仍失败：{second}。可稍后重试，或在 Node 页面把 npm 源切到官方源再更新。"
                )
            })
        }
        Err(error) => Err(error),
    }
}

const OFFICIAL_REGISTRY: &str = "--registry=https://registry.npmjs.org/";

/// npm's ETARGET: the registry advertises a version it cannot serve, which is what a Chinese
/// npm mirror looks like in the minutes after upstream publishes.
pub(crate) fn mirror_is_behind(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("etarget") || error.contains("notarget") || error.contains("no matching version")
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
        &["uninstall", "-g", package, "--loglevel=http"],
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

    // Read-only: Stacker never rewrites npm config. An injected Stacker proxy overrides it.
    if crate::agents::net::stacker_proxy().is_some() {
        return Ok(());
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
                "npm 的 {key} 配置指向本机代理 {proxy}，但该端口当前不可连接。Stacker 不会修改 npm 配置，请在终端执行 npm config delete {key} 或恢复该代理后重试。"
            ));
        }
    }
    Ok(())
}
