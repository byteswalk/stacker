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

/// Runs a program with administrator rights through the shell (one UAC prompt, raised by
/// Stacker's own process) and waits for its exit code.
#[cfg(windows)]
pub(crate) fn run_elevated_wait(
    program: &str,
    parameters: &str,
    name: &str,
    action: &str,
    window: &Option<tauri::Window>,
) -> Result<u32, String> {
    use std::time::Instant;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetExitCodeProcess, TerminateProcess};
    use winapi::um::shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winuser::SW_HIDE;

    let wide = |text: &str| {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (verb, file, params) = (wide("runas"), wide(program), wide(parameters));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            let error = std::io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(1223) {
                format!("已取消：{name} 需要管理员授权")
            } else {
                format!("无法启动 {name} {action}程序：{error}")
            });
        }
        let started = Instant::now();
        let mut last_reported = u64::MAX;
        loop {
            if crate::installer::op_cancelled() {
                let _ = TerminateProcess(info.hProcess, 1);
                let _ = WaitForSingleObject(info.hProcess, 5_000);
                CloseHandle(info.hProcess);
                return Err(format!("已取消{action} {name}"));
            }
            match WaitForSingleObject(info.hProcess, 250) {
                0 => {
                    let mut code: u32 = 1;
                    let read = GetExitCodeProcess(info.hProcess, &mut code);
                    CloseHandle(info.hProcess);
                    return if read == 0 {
                        Err(format!("无法读取 {name} {action}程序的退出状态"))
                    } else {
                        Ok(code)
                    };
                }
                258 => {
                    let elapsed = started.elapsed().as_secs();
                    if elapsed >= 1200 {
                        let _ = TerminateProcess(info.hProcess, 1);
                        let _ = WaitForSingleObject(info.hProcess, 5_000);
                        CloseHandle(info.hProcess);
                        return Err(format!("{name} {action}超过 20 分钟，已停止等待"));
                    }
                    if elapsed != last_reported {
                        last_reported = elapsed;
                        emit_progress(window, format!("正在{action} {name} · 已 {elapsed} 秒"));
                    }
                }
                _ => {
                    CloseHandle(info.hProcess);
                    return Err(format!("等待 {name} {action}程序时发生系统错误"));
                }
            }
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn run_elevated_wait(
    _: &str,
    _: &str,
    _: &str,
    _: &str,
    _: &Option<tauri::Window>,
) -> Result<u32, String> {
    Err("仅支持 Windows".into())
}

/// The lines of a verbose MSI log that say what failed: Windows Installer's own error
/// messages (`Error 1920. …`, `MSI (s) … Note: 1: 1708`), the last few of them.
pub(crate) fn msi_log_errors(log: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(log).ok()?;
    // msiexec writes its log in UTF-16 LE.
    let text = if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("Error ")
                || line.contains("Return value 3")
                || line.starts_with("CustomAction")
        })
        .collect();
    (!lines.is_empty()).then(|| lines[lines.len().saturating_sub(3)..].join(" | "))
}

/// The Windows MSI Kiro's release manifest lists: its URL and SHA-256.
pub(crate) fn kiro_msi(manifest: &serde_json::Value) -> Option<(String, String)> {
    let package = manifest["packages"].as_array()?.iter().find(|package| {
        package["os"] == "windows"
            && package["architecture"] == "x86_64"
            && package["kind"] == "msi"
    })?;
    let download = package["download"].as_str()?;
    let sha256 = package["sha256"].as_str()?.to_ascii_lowercase();
    Some((
        format!("https://prod.download.cli.kiro.dev/stable/{download}"),
        sha256,
    ))
}

/// Kiro CLI is a per-machine MSI, which only installs with administrator rights. What the
/// official script does is done here instead: the MSI from Kiro's release manifest, checked
/// against the SHA-256 listed there, then msiexec started by Stacker itself through the
/// shell's `runas`. PowerShell's `Start-Process -Verb RunAs` from a child process is refused
/// ("Access is denied") when Stacker runs it; the shell call from Stacker raises UAC.
pub(crate) fn install_or_update_kiro_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    if !is_windows_11() {
        return Err("Kiro CLI 只支持 Windows 11，这台电脑的系统版本装不了。".into());
    }
    emit_progress(window, "正在读取 Kiro CLI 的官方发布清单…");
    let manifest: serde_json::Value = serde_json::from_str(&crate::agents::detect::fetch_text(
        crate::agents::detect::KIRO_CLI_MANIFEST,
    )?)
    .map_err(|e| format!("Kiro 的发布清单解析失败：{e}"))?;
    let (url, sha256) = kiro_msi(&manifest).ok_or("Kiro 的发布清单里没有 Windows 安装包")?;
    // Kiro's MSI refuses to reinstall its own version (RegisterProduct, 1603); an install
    // already at the listed version has nothing to update.
    let latest = manifest["version"].as_str().unwrap_or_default();
    let installed = resolve_command(&["kiro-cli.exe"])
        .and_then(|program| {
            run_command_text(
                &program,
                &["--version"],
                "kiro-cli --version",
                Duration::from_secs(15),
            )
            .ok()
        })
        .and_then(|text| crate::agents::detect::first_semver(&text));
    if !latest.is_empty() && installed.as_deref() == Some(latest) {
        return Ok(format!("Kiro CLI 已是最新版本 {latest}"));
    }
    let msi = std::env::temp_dir().join(format!(
        "stacker-kiro-cli-{}.msi",
        chrono::Local::now().timestamp_millis()
    ));
    let log = msi.with_extension("log");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(60))
        .build();
    let result = (|| {
        crate::installer::download_file_candidates_with_agent(
            &agent,
            std::slice::from_ref(&url),
            &msi,
            1_048_576,
            |message| emit_progress(window, message),
        )?;
        emit_progress(window, "正在校验 Kiro CLI 安装包…");
        if crate::update::sha256_of_file(&msi)? != sha256 {
            return Err("Kiro CLI 安装包的 SHA-256 与官方清单不符，已停止安装。".to_string());
        }
        emit_progress(
            window,
            format!("正在{action} Kiro CLI：请在弹出的 UAC 窗口中点「是」…"),
        );
        let code = run_elevated_wait(
            "msiexec.exe",
            &format!(
                "/i \"{}\" /quiet /norestart /l*v \"{}\"",
                msi.display(),
                log.display()
            ),
            "Kiro CLI",
            action,
            window,
        )?;
        match code {
            // 3010: done, a restart finishes it.
            0 | 3010 => Ok(()),
            1602 => Err("已取消 Kiro CLI 安装".to_string()),
            code => Err(format!(
                "Kiro CLI 安装程序退出代码 {code}{}",
                msi_log_errors(&log)
                    .map(|detail| format!("：{detail}"))
                    .unwrap_or_default()
            )),
        }
    })();
    let _ = std::fs::remove_file(&msi);
    let _ = std::fs::remove_file(&log);
    result.map_err(|error| format!("Kiro CLI {action}未完成：{error}"))?;
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
