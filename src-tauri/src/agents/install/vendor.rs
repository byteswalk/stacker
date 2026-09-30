use crate::agents::{install::npm::*, install::winget::*, install::*, process::*, registry::*, *};
use std::path::Path;
use std::time::Duration;

pub(crate) fn install_or_update_claude(
    program: Option<&Path>,
    method: Option<&str>,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    match (program, method) {
        (Some(_), Some("winget")) => {
            emit_progress(window, "正在通过 WinGet 更新 Claude Code CLI…");
            run_winget_owned(
                winget_args("upgrade", "Anthropic.ClaudeCode", None, true),
                Duration::from_secs(900),
                window,
            )?;
            Ok("Claude Code CLI 已通过 WinGet 更新".into())
        }
        (Some(program), Some("npm")) => {
            emit_progress(window, "正在通过 npm 更新 Claude Code CLI…");
            npm_install_latest("@anthropic-ai/claude-code", Some(program), window)?;
            Ok("Claude Code CLI 已通过 npm 更新".into())
        }
        (Some(program), Some("native")) => {
            emit_progress(window, "正在执行 claude update…");
            run_command_text(
                program,
                &["update"],
                "claude update",
                Duration::from_secs(900),
            )?;
            Ok("Claude Code CLI 已执行 claude update".into())
        }
        (Some(_), _) => Err(
            "已检测到 Claude Code CLI，但无法判断安装来源。请按官方文档使用原安装方式更新。".into(),
        ),
        (None, _) => {
            if winget_command().is_some() {
                emit_progress(window, "正在通过 WinGet 安装 Claude Code CLI…");
                let result = run_winget_owned(
                    winget_args("install", "Anthropic.ClaudeCode", Some("winget"), true),
                    Duration::from_secs(900),
                    window,
                );
                match result {
                    Ok(_) => {}
                    Err(err) => {
                        if let Some(spec) = spec_by_id("claude") {
                            if cli_installed_after_action(&spec) {
                                return Ok("Claude Code CLI 已安装".into());
                            }
                        }
                        return Err(err);
                    }
                }
                Ok("Claude Code CLI 已通过 WinGet 安装".into())
            } else {
                emit_progress(window, "正在运行 Claude Code 官方 Native Installer…");
                run_powershell_streamed(
                    &[
                        "-NoProfile",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-Command",
                        "irm https://claude.ai/install.ps1 | iex",
                    ],
                    "Claude Code Native Install",
                    Duration::from_secs(900),
                    window,
                )?;
                Ok("Claude Code CLI 已通过官方 Native Installer 安装".into())
            }
        }
    }
}

pub(crate) fn install_opencode(window: &Option<tauri::Window>) -> Result<String, String> {
    if resolve_command(&["npm.cmd", "npm.exe", "npm.bat"]).is_some() {
        emit_progress(window, "正在通过 npm 安装 OpenCode CLI…");
        npm_install_latest("opencode-ai", None, window)?;
        return Ok("OpenCode CLI 已通过 npm 安装".into());
    }
    if winget_command().is_some() {
        emit_progress(window, "正在通过 WinGet 安装 OpenCode CLI…");
        let result = run_winget_owned(
            winget_args("install", "SST.opencode", None, true),
            Duration::from_secs(900),
            window,
        );
        match result {
            Ok(_) => {}
            Err(err) => {
                if let Some(spec) = spec_by_id("opencode") {
                    if cli_installed_after_action(&spec) {
                        return Ok("OpenCode CLI 已安装".into());
                    }
                }
                return Err(err);
            }
        }
        return Ok("OpenCode CLI 已通过 WinGet 安装".into());
    }
    if scoop_command().is_some() {
        emit_progress(window, "正在通过 Scoop 安装 OpenCode CLI…");
        run_scoop(&["install", "opencode"], Duration::from_secs(900))?;
        return Ok("OpenCode CLI 已通过 Scoop 安装".into());
    }
    if choco_command().is_some() {
        emit_progress(window, "正在通过 Chocolatey 安装 OpenCode CLI…");
        run_choco(&["install", "opencode", "-y"], Duration::from_secs(900))?;
        return Ok("OpenCode CLI 已通过 Chocolatey 安装".into());
    }
    Err("未检测到 npm、WinGet、Scoop 或 Chocolatey。请先安装 Node.js，或按 OpenCode 官方文档选择安装方式。".into())
}

pub(crate) fn install_openclaw(window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, "正在运行 OpenClaw 官方 Windows 安装器…");
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& ([scriptblock]::Create((iwr -useb https://openclaw.ai/install.ps1))) -NoOnboard",
        ],
        "OpenClaw Windows Installer",
        Duration::from_secs(1200),
        window,
    )?;
    Ok("OpenClaw CLI 已通过官方安装器安装".into())
}

pub(crate) fn install_hermes(window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, "正在运行 Hermes 官方 Windows 安装器…");
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "iex (irm https://hermes-agent.nousresearch.com/install.ps1)",
        ],
        "Hermes Windows Installer",
        Duration::from_secs(1200),
        window,
    )?;
    Ok("Hermes CLI 已通过官方安装器安装".into())
}

pub(crate) fn run_codex_installer(window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, "正在运行 Codex 官方 Windows 安装器…");
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://chatgpt.com/codex/install.ps1 | iex",
        ],
        "Codex Windows Installer",
        Duration::from_secs(900),
        window,
    )
    .map(|_| "Codex CLI 已安装".into())
}

pub(crate) fn install_or_update_kimi_cli(
    program: Option<&Path>,
    method: Option<&str>,
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    if method == Some("npm") || method == Some("conda-npm") {
        emit_progress(window, format!("正在通过 npm {action} Kimi Code CLI…"));
        npm_install_latest("@moonshot-ai/kimi-code", program, window)?;
        return Ok(format!("Kimi Code CLI 已通过 npm {action}"));
    }

    emit_progress(window, format!("正在通过 Kimi 官方安装器{action} CLI…"));
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://code.kimi.com/kimi-code/install.ps1 | iex",
        ],
        "Kimi Code CLI Installer",
        Duration::from_secs(900),
        window,
    )?;
    Ok(format!("Kimi Code CLI 已通过官方安装器{action}"))
}

pub(crate) fn install_or_update_antigravity_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    emit_progress(window, format!("正在通过官方脚本{action} Antigravity CLI…"));
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://antigravity.google/cli/install.ps1 | iex",
        ],
        "Antigravity CLI Install",
        Duration::from_secs(900),
        window,
    )?;
    Ok(format!("Antigravity CLI 已{action}"))
}

fn run_official_script(
    window: &Option<tauri::Window>,
    command: &str,
    label: &str,
) -> Result<(), String> {
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command,
        ],
        label,
        Duration::from_secs(900),
        window,
    )
    .map(|_| ())
}

/// Cursor's installer replaces `%LOCALAPPDATA%\cursor-agent` whole, so it also updates.
pub(crate) fn install_or_update_cursor_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    emit_progress(
        window,
        format!("正在通过 Cursor 官方脚本{action} Cursor CLI…"),
    );
    run_official_script(
        window,
        "irm 'https://cursor.com/install?win32=true' | iex",
        "Cursor CLI Install",
    )?;
    Ok(format!("Cursor CLI 已{action}"))
}

/// Factory's installer checks the binary's SHA-256; it also stops running `droid` sessions
/// before replacing the file.
pub(crate) fn install_or_update_droid_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    emit_progress(
        window,
        format!(
            "正在通过 Factory 官方脚本{action} Droid CLI（正在运行的 droid 会被官方脚本结束）…"
        ),
    );
    run_official_script(
        window,
        "irm https://app.factory.ai/cli/windows | iex",
        "Droid CLI Install",
    )?;
    Ok(format!("Droid CLI 已{action}"))
}

/// Windows 11 starts at build 22000; Kiro CLI refuses anything older.
pub(crate) fn is_windows_11() -> bool {
    windows_build() >= 22000
}

#[cfg(windows)]
fn windows_build() -> u32 {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|key| key.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|build| build.parse().ok())
        .unwrap_or(0)
}

#[cfg(not(windows))]
fn windows_build() -> u32 {
    0
}

/// Kiro's installer runs a per-machine MSI, which a quiet install can only do with
/// administrator rights, so the official script runs elevated: one UAC prompt.
pub(crate) fn install_or_update_kiro_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    if !is_windows_11() {
        return Err("Kiro CLI 只支持 Windows 11，这台电脑的系统版本装不了。".into());
    }
    emit_progress(
        window,
        format!("正在通过 Kiro 官方脚本{action} Kiro CLI；Windows 会请求一次管理员授权…"),
    );
    run_official_script(
        window,
        "try { $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-Command','irm https://cli.kiro.dev/install.ps1 | iex' } catch { Write-Output $_.Exception.Message; exit 1223 }; if (-not $p) { exit 1 }; exit $p.ExitCode",
        "Kiro CLI Install",
    )
    .map_err(|error| format!("Kiro CLI {action}未完成（管理员授权被取消或安装失败）：{error}"))?;
    // The MSI installs to Program Files; a run that ends without it there did not install.
    let installed = std::env::var_os("ProgramFiles")
        .map(|dir| std::path::PathBuf::from(dir).join("Kiro-Cli"))
        .is_some_and(|dir| dir.is_dir());
    if !installed {
        return Err(format!(
            "Kiro CLI {action}未完成：安装程序结束了，但没有找到安装目录。"
        ));
    }
    Ok(format!("Kiro CLI 已{action}"))
}

/// MiniMax's installer keeps its own Node.js runtime in `%USERPROFILE%\.minimax-code` and
/// updates in place.
pub(crate) fn install_or_update_mcode(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    emit_progress(
        window,
        format!("正在通过 MiniMax 官方脚本{action} MiniMax Code CLI…"),
    );
    run_official_script(
        window,
        "irm https://filecdn.minimax.chat/public/install.ps1 | iex",
        "MiniMax Code CLI Install",
    )?;
    Ok(format!("MiniMax Code CLI 已{action}"))
}

/// Removes a folder an official installer owns whole (Cursor's, MiniMax's), and its PATH
/// entry. Only a folder of that exact name under the user's own profile qualifies.
pub(crate) fn remove_installer_dir(program: &Path, folder: &str) -> Result<(), String> {
    let dir = program
        .ancestors()
        .find(|dir| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(folder))
        })
        .ok_or("命令不在官方安装目录内，已取消卸载。")?;
    let user = std::env::var_os("USERPROFILE")
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if user.is_empty() || !dir.to_string_lossy().to_lowercase().starts_with(&user) {
        return Err("安装目录不在当前用户目录内，为避免误删已取消卸载。".into());
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("删除安装目录失败：{e}"))?;
    let _ = crate::winenv::remove_path_in(crate::winenv::Hive::User, &dir.to_string_lossy());
    crate::winenv::broadcast_change();
    Ok(())
}

/// xAI's own installer; it also updates an existing install in place.
pub(crate) fn install_or_update_grok_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    emit_progress(
        window,
        format!("正在通过 xAI 官方脚本{action} Grok Build CLI…"),
    );
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://x.ai/cli/install.ps1 | iex",
        ],
        "Grok Build CLI Install",
        Duration::from_secs(900),
        window,
    )?;
    Ok(format!("Grok Build CLI 已{action}"))
}

pub(crate) fn has_legacy_pi(program: Option<&Path>) -> bool {
    program
        .and_then(Path::parent)
        .is_some_and(|prefix| prefix.join("node_modules").join(PI_LEGACY_PACKAGE).is_dir())
}

pub(crate) const PI_LEGACY_PACKAGE: &str = "@mariozechner/pi-coding-agent";
pub(crate) const PI_PACKAGE: &str = "@earendil-works/pi-coding-agent";

pub(crate) fn install_or_update_pi(
    program: Option<&Path>,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    if has_legacy_pi(program) {
        emit_progress(
            window,
            "pi 已迁移到 @earendil-works/pi-coding-agent，正在移除旧包 @mariozechner/pi-coding-agent…",
        );
        npm_uninstall(PI_LEGACY_PACKAGE, program, window)?;
    }
    npm_install_latest(PI_PACKAGE, program, window)?;
    Ok("pi 已通过 npm 安装最新版本".into())
}

/// MiMo Code's official installer, served from Xiaomi's CDN into `~/.mimocode/bin`.
pub(crate) fn install_or_update_mimo_native(
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    emit_progress(window, "正在通过小米官方安装脚本更新 MiMo Code CLI…");
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://mimo.xiaomi.com/install.ps1 | iex",
        ],
        "MiMo Code Installer",
        Duration::from_secs(900),
        window,
    )?;
    Ok("MiMo Code CLI 已通过官方安装脚本更新".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_pi_is_detected_next_to_the_npm_shim() {
        let prefix = tempfile::tempdir().unwrap();
        let shim = prefix.path().join("pi.cmd");
        std::fs::write(&shim, b"").unwrap();
        assert!(!has_legacy_pi(Some(&shim)));
        std::fs::create_dir_all(prefix.path().join("node_modules").join(PI_LEGACY_PACKAGE))
            .unwrap();
        assert!(has_legacy_pi(Some(&shim)));
        assert!(!has_legacy_pi(None));
    }
}
