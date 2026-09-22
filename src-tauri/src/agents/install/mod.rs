pub(crate) mod direct;
pub(crate) mod npm;
pub(crate) mod vendor;
pub(crate) mod winget;

use crate::agents::{
    detect::*, install::direct::*, install::npm::*, install::vendor::*, install::winget::*,
    process::*, registry::*, *,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub(crate) fn install_cli_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    emit_progress(window, format!("正在安装 {}…", spec.cli.name));
    match spec.vendor {
        Vendor::Claude => install_or_update_claude(None, None, window),
        Vendor::Codex => run_codex_installer(window).or_else(|installer_error| {
            if let Some(pkg) = spec.cli.npm_package {
                emit_progress(
                    window,
                    format!("Codex 官方安装器未完成（{installer_error}），正在改用 npm…"),
                );
                npm_install_latest(pkg, None, window)?;
                Ok("Codex CLI 已通过 npm 安装".into())
            } else {
                Err("Codex CLI 安装失败".into())
            }
        }),
        Vendor::Kimi => install_or_update_kimi_cli(None, None, window, "安装"),
        Vendor::Antigravity => {
            if let Some(id) = spec.cli.winget_id {
                if winget_command().is_some() {
                    emit_progress(window, "正在通过 WinGet 安装 Antigravity CLI…");
                    let result = run_winget_owned(
                        winget_args("install", id, None, true),
                        Duration::from_secs(900),
                        window,
                    );
                    match result {
                        Ok(_) => {}
                        Err(err) => {
                            if cli_installed_after_action(spec) {
                                return Ok("Antigravity CLI 已安装".into());
                            }
                            return Err(err);
                        }
                    }
                    return Ok("Antigravity CLI 已通过 WinGet 安装".into());
                }
            }
            install_or_update_antigravity_cli(window, "安装")
        }
        Vendor::OpenCode => install_opencode(window),
        Vendor::Trae => install_or_update_trae_cli(window, "安装"),
        Vendor::OpenClaw => install_openclaw(window),
        Vendor::Hermes => install_hermes(window),
        Vendor::Pi => install_or_update_pi(None, window),
        _ => {
            if let Some(pkg) = spec.cli.npm_package {
                npm_install_latest(pkg, None, window)?;
                Ok(format!("{} 已通过 npm 安装", spec.cli.name))
            } else {
                Err("该 CLI 暂无自动安装方案，请查看官方文档。".into())
            }
        }
    }
}

pub(crate) fn update_cli_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    emit_progress(window, "正在检测当前安装来源…");
    let program = resolve_command(spec.cli.candidates);
    let method = detect_install_method(spec, program.as_deref());
    match spec.vendor {
        Vendor::Claude => install_or_update_claude(program.as_deref(), method.as_deref(), window),
        Vendor::Codex => match (program.as_deref(), method.as_deref()) {
            (Some(program), Some("npm")) => {
                update_with_npm_source(spec, program, window)?;
                Ok("Codex CLI 已通过 npm 更新".into())
            }
            (Some(_), Some("winget")) => {
                let id = spec.cli.winget_id.ok_or("Codex CLI 缺少 WinGet 包 ID")?;
                emit_progress(window, "正在通过 WinGet 更新 Codex CLI…");
                run_winget_owned(
                    winget_args("upgrade", id, None, true),
                    Duration::from_secs(900),
                    window,
                )?;
                Ok("Codex CLI 已通过 WinGet 更新".into())
            }
            (Some(_), _) => {
                emit_progress(window, "正在运行 Codex 官方 Windows 安装器…");
                run_codex_installer(window)?;
                Ok("Codex CLI 已通过官方 Windows 安装器更新".into())
            }
            (None, _) => install_cli_tool(spec, window),
        },
        Vendor::Kimi => {
            install_or_update_kimi_cli(program.as_deref(), method.as_deref(), window, "更新")
        }
        Vendor::Antigravity => {
            if method.as_deref() == Some("winget") {
                let id = spec
                    .cli
                    .winget_id
                    .ok_or("Antigravity CLI 缺少 WinGet 包 ID")?;
                emit_progress(window, "正在通过 WinGet 更新 Antigravity CLI…");
                run_winget_owned(
                    winget_args("upgrade", id, None, true),
                    Duration::from_secs(900),
                    window,
                )?;
                return Ok("Antigravity CLI 已通过 WinGet 更新".into());
            }
            if let Some(program) = program {
                emit_progress(window, "正在执行 agy update…");
                match run_command_text(
                    &program,
                    &["update"],
                    "agy update",
                    Duration::from_secs(900),
                ) {
                    Ok(_) => return Ok("Antigravity CLI 已更新".into()),
                    Err(err) => emit_progress(
                        window,
                        format!("agy update 未完成，改用官方安装脚本：{err}"),
                    ),
                }
            }
            install_or_update_antigravity_cli(window, "更新")
        }
        Vendor::OpenCode => match (program.as_deref(), method.as_deref()) {
            (Some(program), Some("npm")) => {
                update_with_npm_source(spec, program, window)?;
                Ok("OpenCode CLI 已通过 npm 更新".into())
            }
            (Some(_), Some("winget")) => {
                let id = spec.cli.winget_id.ok_or("OpenCode CLI 缺少 WinGet 包 ID")?;
                emit_progress(window, "正在通过 WinGet 更新 OpenCode CLI…");
                run_winget_owned(
                    winget_args("upgrade", id, None, true),
                    Duration::from_secs(900),
                    window,
                )?;
                Ok("OpenCode CLI 已通过 WinGet 更新".into())
            }
            (Some(_), Some("scoop")) => {
                emit_progress(window, "正在通过 Scoop 更新 OpenCode CLI…");
                run_scoop(&["update", "opencode"], Duration::from_secs(900))?;
                Ok("OpenCode CLI 已通过 Scoop 更新".into())
            }
            (Some(_), Some("chocolatey")) => {
                emit_progress(window, "正在通过 Chocolatey 更新 OpenCode CLI…");
                run_choco(&["upgrade", "opencode", "-y"], Duration::from_secs(900))?;
                Ok("OpenCode CLI 已通过 Chocolatey 更新".into())
            }
            (Some(_), _) => Err(
                "已检测到 OpenCode CLI，但无法判断安装来源。请按官方文档使用原安装方式更新。"
                    .into(),
            ),
            (None, _) => install_cli_tool(spec, window),
        },
        Vendor::Trae => {
            if let Some(program) = program {
                emit_progress(window, "正在执行 traecli update…");
                match run_command_text(
                    &program,
                    &["update"],
                    "traecli update",
                    Duration::from_secs(1200),
                ) {
                    Ok(_) => return Ok("TRAE CLI 已更新".into()),
                    Err(err) => emit_progress(
                        window,
                        format!("traecli update 未完成，改用官方安装脚本：{err}"),
                    ),
                }
            }
            install_or_update_trae_cli(window, "更新")
        }
        Vendor::Hermes => {
            let program = program.ok_or_else(|| "未检测到 Hermes CLI。".to_string())?;
            emit_progress(window, "正在执行 hermes update…");
            run_command_text(
                &program,
                &["update"],
                "hermes update",
                Duration::from_secs(1200),
            )?;
            Ok("Hermes CLI 已更新".into())
        }
        Vendor::Pi => install_or_update_pi(program.as_deref(), window),
        Vendor::MiMo if method.as_deref() == Some("native") => {
            install_or_update_mimo_native(window)
        }
        _ => {
            let program = program.ok_or_else(|| format!("未检测到 {}。", spec.cli.name))?;
            // Update through the source it was installed with, never a second copy via npm.
            if let (Some("winget"), Some(id)) = (method.as_deref(), spec.cli.winget_id) {
                emit_progress(window, format!("正在通过 WinGet 更新 {}…", spec.cli.name));
                run_winget_owned(
                    winget_args("upgrade", id, None, true),
                    Duration::from_secs(900),
                    window,
                )?;
                return Ok(format!("{} 已通过 WinGet 更新", spec.cli.name));
            }
            update_with_npm_source(spec, &program, window)?;
            Ok(format!("{} 已更新", spec.cli.name))
        }
    }
}

pub(crate) fn uninstall_cli_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    emit_progress(window, "正在检测当前安装来源…");
    let program = resolve_command(spec.cli.candidates);
    let method = detect_install_method(spec, program.as_deref());
    if spec.vendor == Vendor::Hermes {
        let program = program.ok_or_else(|| "未检测到 Hermes CLI。".to_string())?;
        emit_progress(window, "正在运行 Hermes 官方卸载程序…");
        run_command_text(
            &program,
            &["uninstall", "--yes"],
            "hermes uninstall",
            Duration::from_secs(900),
        )?;
        return Ok("Hermes CLI 已卸载，用户配置和会话数据已保留".into());
    }
    if spec.vendor == Vendor::OpenClaw {
        let program = program.ok_or_else(|| "未检测到 OpenClaw CLI。".to_string())?;
        emit_progress(window, "正在移除 OpenClaw 网关服务…");
        let _ = run_command_text(
            &program,
            &["gateway", "uninstall"],
            "openclaw gateway uninstall",
            Duration::from_secs(180),
        );
        let pkg = spec.cli.npm_package.ok_or("OpenClaw 缺少 npm 包信息。")?;
        emit_progress(window, "正在卸载 OpenClaw CLI…");
        npm_uninstall(pkg, Some(&program), window)?;
        return Ok("OpenClaw CLI 与网关服务已卸载，配置和工作区已保留".into());
    }
    if spec.vendor == Vendor::Trae {
        return uninstall_trae_cli(window);
    }
    match method.as_deref() {
        Some("winget") => {
            let id = spec.cli.winget_id.ok_or("该 CLI 缺少 WinGet 包 ID")?;
            emit_progress(window, format!("正在通过 WinGet 卸载 {}…", spec.cli.name));
            run_winget_owned(
                winget_args("uninstall", id, None, true),
                Duration::from_secs(900),
                window,
            )?;
            Ok(format!("{} 已通过 WinGet 卸载", spec.cli.name))
        }
        Some("npm") | Some("conda-npm") => {
            let pkg = spec
                .cli
                .npm_package
                .ok_or("该 CLI 不是 npm 包，无法通过 npm 卸载。")?;
            emit_progress(window, format!("正在通过 npm 卸载 {}…", spec.cli.name));
            // An install from before pi's package move lives under the old package name.
            let pkg = if spec.vendor == Vendor::Pi && has_legacy_pi(program.as_deref()) {
                PI_LEGACY_PACKAGE
            } else {
                pkg
            };
            npm_uninstall(pkg, program.as_deref(), window)?;
            Ok(format!("{} 已通过 npm 卸载", spec.cli.name))
        }
        Some("scoop") => {
            emit_progress(window, format!("正在通过 Scoop 卸载 {}…", spec.cli.name));
            run_scoop(&["uninstall", spec.cli.command], Duration::from_secs(900))?;
            Ok(format!("{} 已通过 Scoop 卸载", spec.cli.name))
        }
        Some("chocolatey") => {
            emit_progress(
                window,
                format!("正在通过 Chocolatey 卸载 {}…", spec.cli.name),
            );
            run_choco(
                &["uninstall", spec.cli.command, "-y"],
                Duration::from_secs(900),
            )?;
            Ok(format!("{} 已通过 Chocolatey 卸载", spec.cli.name))
        }
        Some("native") => {
            let program = program.ok_or_else(|| "未找到可卸载的命令入口。".to_string())?;
            remove_cli_binary(&program, spec.cli.command)?;
            Ok(format!(
                "{} 命令文件已移除，登录配置和历史数据已保留。",
                spec.cli.name
            ))
        }
        _ => Err(format!(
            "无法判断 {} 的安装来源。为避免误删文件，请按官方文档卸载。",
            spec.cli.name
        )),
    }
}

pub(crate) fn install_desktop_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    if let Some(id) = spec.desktop.winget_id {
        let mut last_error = None;
        for attempt in 1..=2 {
            emit_progress(
                window,
                if attempt == 1 {
                    format!("正在通过 WinGet 安装 {}…", spec.desktop.name)
                } else {
                    format!("正在利用已下载的安装缓存重试 {}…", spec.desktop.name)
                },
            );
            match run_winget_owned(
                winget_args("install", id, spec.desktop.winget_source, true),
                Duration::from_secs(1200),
                window,
            ) {
                Ok(_) => {
                    last_error = None;
                    break;
                }
                Err(err) => {
                    if wait_for_desktop_install(spec, window, Duration::from_secs(10)) {
                        return Ok(format!("{} 已安装", spec.desktop.name));
                    }
                    if crate::installer::op_cancelled() || err.contains("已取消") {
                        return Err(err);
                    }
                    last_error = Some(err);
                    if attempt == 1 {
                        emit_progress(window, "首次安装未完成，正在复核下载缓存后重试…");
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        }
        if let Some(err) = last_error {
            return Err(format!("{}；自动重试后仍未完成", err));
        }
        if wait_for_desktop_install(spec, window, Duration::from_secs(120)) {
            return Ok(format!("{} 已通过 WinGet 安装", spec.desktop.name));
        }
        return Err(format!(
            "WinGet 已完成，但尚未检测到 {}。请打开 Microsoft Store 检查下载状态，或在设置中打开 Stacker 日志目录查看详细记录",
            spec.desktop.name
        ));
    }
    if let Some(installer) = direct_desktop_installer(spec.vendor, spec.edition) {
        return match install_desktop_from_official_package(spec, installer, window) {
            Ok(message) => Ok(message),
            Err(err) if err.contains("已取消") => Err(err),
            Err(err) => match open_external_target(spec.desktop.install_url) {
                Ok(()) => Err(format!(
                    "{err}。已打开 {} 官方安装页，可在浏览器中继续下载。",
                    spec.desktop.name
                )),
                Err(open_err) => Err(format!("{err}；同时无法打开官方安装页：{open_err}")),
            },
        };
    }
    Err(format!(
        "{} 无法自动安装：{}",
        spec.desktop.name,
        spec.desktop
            .install_unavailable_reason
            .unwrap_or(DEFAULT_DESKTOP_UNAVAILABLE_REASON)
    ))
}

pub(crate) fn wait_for_desktop_install(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
    timeout: Duration,
) -> bool {
    let started = Instant::now();
    let mut last_reported = u64::MAX;
    while started.elapsed() < timeout {
        if desktop_installed_after_action(spec) {
            return true;
        }
        if crate::installer::op_cancelled() {
            return false;
        }
        let elapsed = started.elapsed().as_secs();
        if elapsed != last_reported {
            last_reported = elapsed;
            emit_progress(
                window,
                format!(
                    "正在确认 {} 安装状态 · 已 {} 秒",
                    spec.desktop.name, elapsed
                ),
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

pub(crate) fn update_desktop_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let found = detect_desktop_app(&spec.desktop);
    let current = found.as_ref().and_then(|f| f.version.as_deref());
    if let Some(next) = desktop_staged_update(spec, current) {
        emit_progress(
            window,
            format!("{} 已下载 {next}，需要重启应用完成更新…", spec.desktop.name),
        );
        open_desktop_tool(spec.id)?;
        return Ok(format!(
            "{} 已下载 {next}，请在应用内点击 Relaunch to update 完成更新。",
            spec.desktop.name
        ));
    }
    if let Some(id) = spec.desktop.winget_id {
        emit_progress(
            window,
            format!("正在通过 WinGet 更新 {}…", spec.desktop.name),
        );
        run_winget_owned(
            winget_args("upgrade", id, spec.desktop.winget_source, true),
            Duration::from_secs(1200),
            window,
        )?;
        return Ok(format!("{} 已通过 WinGet 更新", spec.desktop.name));
    }
    if spec.vendor == Vendor::Hermes {
        return update_hermes_desktop(spec, window);
    }
    if let Some(installer) = direct_desktop_installer(spec.vendor, spec.edition) {
        // A silent installer replaces the program files; never close the user's app for them.
        if let Some(image) = found
            .as_ref()
            .and_then(|f| f.path.as_deref())
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
        {
            if image_is_running(image) {
                return Err(format!(
                    "{} 正在运行。静默更新需要替换程序文件，请先退出应用后重试。",
                    spec.desktop.name
                ));
            }
        }
        emit_progress(window, format!("正在静默更新 {}…", spec.desktop.name));
        return install_desktop_from_official_package(spec, installer, window)
            .map(|_| format!("{} 已静默更新", spec.desktop.name));
    }
    Err(format!(
        "{} 暂无可自动执行的 Windows 更新源，已取消操作。",
        spec.desktop.name
    ))
}

/// Hermes Desktop is built from the Hermes Agent checkout. `hermes update` pulls it and then
/// rebuilds the desktop app (`hermes desktop --build-only`) when one is installed. Hermes-Setup
/// is no updater: it ignores `/S` and waits for its Install button.
fn update_hermes_desktop(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let program = resolve_command(spec.cli.candidates)
        .ok_or_else(|| "未检测到 Hermes CLI，无法更新 Hermes 桌面端。".to_string())?;
    // The rebuild replaces Hermes.exe; never close the user's app for it.
    if image_is_running("Hermes.exe") {
        return Err(format!(
            "{} 正在运行。更新需要重新构建并替换程序文件，请先退出应用后重试。",
            spec.desktop.name
        ));
    }
    emit_progress(
        window,
        "正在执行 hermes update（拉取最新代码并重新构建桌面端）…",
    );
    run_command_text(
        &program,
        &["update"],
        "hermes update",
        Duration::from_secs(1800),
    )
    .map_err(|e| {
        // On this kind of machine npm inside Hermes' update is refused (EPERM) while the same
        // npm run by hand works; security software blocking it is the likely cause.
        let hint = if e.contains("Node.js") || e.contains("npm") {
            "。npm 被拒绝访问文件时，多半是 360 等安全软件拦截了 Hermes 安装目录里的操作，可将其加入信任后重试"
        } else {
            ""
        };
        format!("hermes update 未完成：{e}{hint}。可在终端运行 hermes update 查看完整输出")
    })?;
    Ok(format!("{} 已更新", spec.desktop.name))
}

pub(crate) fn uninstall_desktop_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let found = detect_desktop_app(&spec.desktop);
    if let Some(uninstall) = found
        .as_ref()
        .and_then(|f| f.uninstall.as_deref())
        .filter(|s| s.starts_with("appx:"))
    {
        let package = uninstall.trim_start_matches("appx:");
        emit_progress(window, format!("正在卸载 {}…", spec.desktop.name));
        uninstall_appx_package(package)?;
        return Ok(format!("{} 已卸载", spec.desktop.name));
    }
    if let Some(id) = spec.desktop.winget_id {
        emit_progress(
            window,
            format!("正在通过 WinGet 卸载 {}…", spec.desktop.name),
        );
        run_winget_owned(
            winget_args("uninstall", id, spec.desktop.winget_source, true),
            Duration::from_secs(1200),
            window,
        )?;
        return Ok(format!("{} 已通过 WinGet 卸载", spec.desktop.name));
    }
    if let Some(uninstall) = found.and_then(|f| f.uninstall) {
        emit_progress(window, format!("正在运行 {} 卸载程序…", spec.desktop.name));
        run_uninstall_string(&uninstall)?;
        return Ok(format!("{} 卸载程序已启动", spec.desktop.name));
    }
    Err(format!(
        "未找到 {} 的自动卸载入口。请在 Windows“已安装的应用”中卸载。",
        spec.desktop.name
    ))
}

pub(crate) fn cli_installed_after_action(spec: &ToolSpec) -> bool {
    resolve_command(spec.cli.candidates).is_some()
        || spec.cli.winget_id.is_some_and(winget_package_installed)
}

pub(crate) fn desktop_installed_after_action(spec: &ToolSpec) -> bool {
    detect_desktop_app(&spec.desktop).is_some()
}

pub(crate) fn uninstall_appx_package(package_full_name: &str) -> Result<(), String> {
    let package = ps_single_quoted(package_full_name);
    let script = format!("Remove-AppxPackage -Package {package} -ErrorAction Stop");
    run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "Remove-AppxPackage",
        Duration::from_secs(120),
    )
    .map(|_| ())
}

pub(crate) fn remove_cli_binary(program: &Path, command_name: &str) -> Result<(), String> {
    let path = program
        .canonicalize()
        .unwrap_or_else(|_| program.to_path_buf());
    let file_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if file_name != command_name.to_lowercase() {
        return Err("命令文件名不匹配，已取消卸载。".into());
    }
    let p = path.to_string_lossy().to_lowercase();
    let user = std::env::var_os("USERPROFILE")
        .map(|s| PathBuf::from(s).to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let local = std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if (!user.is_empty() && p.starts_with(&user)) || (!local.is_empty() && p.starts_with(&local)) {
        std::fs::remove_file(&path).map_err(|e| format!("删除命令文件失败：{e}"))?;
        Ok(())
    } else {
        Err("命令不在当前用户目录内，为避免误删已取消卸载。".into())
    }
}

pub(crate) fn run_uninstall_string(uninstall: &str) -> Result<(), String> {
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/d", "/c", uninstall]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.spawn().map_err(|e| format!("启动卸载程序失败：{e}"))?;
    Ok(())
}

/// Removes a broken effective CLI entry so the next healthy install on PATH takes over.
pub(crate) fn repair_cli_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let tool = fresh_tool(spec.id, false).ok_or("无法检测该智能体")?;
    if !tool.cli.can_repair {
        return Err("当前没有可自动修复的损坏入口".into());
    }
    emit_progress(
        window,
        format!(
            "正在移除损坏的入口：{}",
            tool.cli.path.clone().unwrap_or_default()
        ),
    );
    uninstall_cli_tool(spec, window)?;
    Ok(format!("{} 已修复，当前使用健康的安装", spec.cli.name))
}

/// An update the app's own updater already downloaded and applies on relaunch. Only
/// Claude reports this (in its log); other vendors only publish an online version.
pub(crate) fn desktop_staged_update(spec: &ToolSpec, current: Option<&str>) -> Option<String> {
    match spec.vendor {
        Vendor::Claude => claude_desktop_ready_update(current),
        _ => None,
    }
}

pub(crate) fn tasklist_has_image(csv: &str, image: &str) -> bool {
    csv.lines().any(|line| {
        line.split(',')
            .next()
            .map(|column| column.trim().trim_matches('"'))
            .is_some_and(|name| name.eq_ignore_ascii_case(image))
    })
}

pub(crate) fn image_is_running(image: &str) -> bool {
    let mut command = Command::new("tasklist.exe");
    command.args(["/FI", &format!("IMAGENAME eq {image}"), "/FO", "CSV", "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .output()
        .map(|output| tasklist_has_image(&String::from_utf8_lossy(&output.stdout), image))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_apps_with_a_downloaded_update_wait_for_a_relaunch() {
        // Kimi Work only publishes its latest version online; nothing is downloaded, so
        // it must update with its signed silent installer instead of opening the app.
        let kimi = spec_by_id("kimi").unwrap();
        assert!(desktop_staged_update(&kimi, Some("3.2.9")).is_none());
        assert!(direct_desktop_installer(kimi.vendor, kimi.edition)
            .is_some_and(|installer| installer.signed));
    }

    #[test]
    fn running_image_names_come_from_tasklist_csv() {
        let csv = "\"Kimi.exe\",\"1234\",\"Console\",\"1\",\"120,000 K\"
";
        assert!(tasklist_has_image(csv, "Kimi.exe"));
        assert!(!tasklist_has_image(
            "INFO: No tasks are running which match the specified criteria.",
            "Kimi.exe"
        ));
    }
}
