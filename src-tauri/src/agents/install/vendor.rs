use crate::agents::{install::npm::*, install::winget::*, install::*, process::*, registry::*, *};
use std::path::{Path, PathBuf};
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

pub(crate) fn install_or_update_trae_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    emit_progress(window, format!("正在通过官方脚本{action} TRAE CLI…"));
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://trae.cn/trae-cli/install.ps1 | iex",
        ],
        "TRAE CLI Installer",
        Duration::from_secs(1200),
        window,
    )?;
    let spec = spec_by_id("trae-work").ok_or_else(|| "缺少 TRAE CLI 目录信息。".to_string())?;
    if !cli_installed_after_action(&spec) {
        return Err("TRAE CLI 安装程序已结束，但尚未检测到命令入口。请重新打开终端后重试。".into());
    }
    Ok(format!("TRAE CLI 已{action}"))
}

pub(crate) fn uninstall_trae_cli(window: &Option<tauri::Window>) -> Result<String, String> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "无法读取当前用户的本地应用目录。".to_string())?;
    let install_dir = local.join("trae-cli");
    if !install_dir.is_dir() {
        return Err("未找到 TRAE CLI 官方安装目录。".into());
    }
    emit_progress(window, "正在卸载 TRAE CLI 并清理用户 PATH…");
    let install = ps_single_quoted(&install_dir.to_string_lossy());
    let bin = ps_single_quoted(&install_dir.join("bin").to_string_lossy());
    let script = format!(
        "$install={install}; $bin={bin}; \
         if (Test-Path -LiteralPath $install) {{ Remove-Item -LiteralPath $install -Recurse -Force -ErrorAction Stop }}; \
         $parts=([Environment]::GetEnvironmentVariable('Path','User') -split ';' | Where-Object {{ $_ -and $_.TrimEnd('\\') -ine $bin.TrimEnd('\\') }}); \
         [Environment]::SetEnvironmentVariable('Path',($parts -join ';'),'User')"
    );
    run_powershell_streamed(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "TRAE CLI Uninstall",
        Duration::from_secs(120),
        window,
    )?;
    Ok("TRAE CLI 已卸载，账号配置和项目文件已保留".into())
}

/// pi moved from `@mariozechner/pi-coding-agent` to `@earendil-works/pi-coding-agent`.
/// Both packages provide the `pi` command, so the old one is removed before installing.
pub(crate) const PI_LEGACY_PACKAGE: &str = "@mariozechner/pi-coding-agent";
pub(crate) const PI_PACKAGE: &str = "@earendil-works/pi-coding-agent";

pub(crate) fn has_legacy_pi(program: Option<&Path>) -> bool {
    program
        .and_then(Path::parent)
        .is_some_and(|prefix| prefix.join("node_modules").join(PI_LEGACY_PACKAGE).is_dir())
}

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
