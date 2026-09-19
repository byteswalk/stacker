use crate::agents::*;
use encoding_rs::GBK;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn run_powershell(
    args: &[&str],
    name: &str,
    timeout: Duration,
) -> Result<String, String> {
    let program = resolve_command_including_windowsapps(&["powershell.exe", "powershell.cmd"])
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    run_command_text(&program, args, name, timeout)
}

pub(crate) fn run_powershell_streamed(
    args: &[&str],
    name: &str,
    timeout: Duration,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let program = resolve_command_including_windowsapps(&["powershell.exe", "powershell.cmd"])
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    run_command_streamed(&program, args, name, timeout, Duration::ZERO, window)
}

pub(crate) fn ps_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn run_command_text(
    program: &Path,
    args: &[&str],
    display_name: &str,
    timeout: Duration,
) -> Result<String, String> {
    run_command_streamed(program, args, display_name, timeout, Duration::ZERO, &None)
}

pub(crate) fn run_command_streamed(
    program: &Path,
    args: &[&str],
    display_name: &str,
    timeout: Duration,
    stall_timeout: Duration,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    use std::io::Read;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    let mut cmd = command_for_path(program, args);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    apply_fresh_path(&mut cmd);
    if let Some(proxy) = crate::agents::net::stacker_proxy() {
        for (key, value) in crate::agents::net::proxy_env(&proxy) {
            cmd.env(key, value);
        }
    }
    log::info!(
        "external command started: name={display_name} program={} args={args:?}",
        program.display()
    );
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 {display_name} 失败：{e}"))?;

    let started = Instant::now();
    let last_activity = Arc::new(AtomicU64::new(0));
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));

    // Reader threads report into the same task as the caller.
    let task = crate::installer::current_task_context();
    let spawn_reader = |mut reader: Box<dyn Read + Send>, win: Option<tauri::Window>| {
        let activity = last_activity.clone();
        let captured = output.clone();
        let task = task.clone();
        std::thread::spawn(move || {
            let mut read = move || {
                let mut chunk = [0u8; 1024];
                let mut line = Vec::new();
                while let Ok(n) = reader.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    activity.store(
                        started.elapsed().as_millis().max(1) as u64,
                        Ordering::Relaxed,
                    );
                    if let Ok(mut all) = captured.lock() {
                        all.extend_from_slice(&chunk[..n]);
                    }
                    for &byte in &chunk[..n] {
                        if byte == b'\r' || byte == b'\n' {
                            emit_command_progress(&win, &line);
                            line.clear();
                        } else {
                            line.push(byte);
                        }
                    }
                }
                emit_command_progress(&win, &line);
            };
            match task {
                Some(context) => crate::installer::with_task_context(context, read),
                None => read(),
            }
        })
    };

    let stdout = child.stdout.take().ok_or("无法读取 WinGet 输出")?;
    let stderr = child.stderr.take().ok_or("无法读取 WinGet 错误输出")?;
    let stdout_reader = spawn_reader(Box::new(stdout), window.clone());
    let stderr_reader = spawn_reader(Box::new(stderr), window.clone());
    let mut last_heartbeat = Instant::now();

    let status = loop {
        if crate::installer::op_cancelled() {
            terminate_command_tree(&mut child);
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("已取消操作".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    terminate_command_tree(&mut child);
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    let partial = captured_command_output(&output);
                    log::error!(
                        "external command timed out: name={display_name} elapsed_ms={} output={}",
                        elapsed.as_millis(),
                        log_output_excerpt(&partial)
                    );
                    return Err(format!("{display_name} 执行超时，请检查网络后重试"));
                }
                let activity_ms = last_activity.load(Ordering::Relaxed);
                let inactive = if activity_ms == 0 {
                    elapsed
                } else {
                    elapsed.saturating_sub(Duration::from_millis(activity_ms))
                };
                if !stall_timeout.is_zero() && inactive >= stall_timeout {
                    terminate_command_tree(&mut child);
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    let partial = captured_command_output(&output);
                    log::error!(
                        "external command stalled: name={display_name} inactive_ms={} output={}",
                        inactive.as_millis(),
                        log_output_excerpt(&partial)
                    );
                    return Err(format!(
                        "{display_name} 连续 {} 秒没有响应，已停止操作。请检查网络或 WinGet 软件源后重试",
                        stall_timeout.as_secs()
                    ));
                }
                if last_heartbeat.elapsed() >= Duration::from_secs(1)
                    && inactive >= Duration::from_secs(1)
                {
                    emit_progress(
                        window,
                        format!("{display_name} 正在处理 · 已 {} 秒", elapsed.as_secs()),
                    );
                    last_heartbeat = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                terminate_command_tree(&mut child);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("读取 {display_name} 状态失败：{e}"));
            }
        }
    };

    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    let bytes = output.lock().map(|data| data.clone()).unwrap_or_default();
    let text = decode_command_bytes(&bytes).trim().to_string();
    if status.success() {
        log::info!(
            "external command completed: name={display_name} elapsed_ms={} output={}",
            started.elapsed().as_millis(),
            log_output_excerpt(&text)
        );
        Ok(text)
    } else {
        log::error!(
            "external command failed: name={display_name} exit_code={:?} elapsed_ms={} output={}",
            status.code(),
            started.elapsed().as_millis(),
            log_output_excerpt(&text)
        );
        Err(first_output_line(&text).unwrap_or_else(|| format!("{display_name} 执行失败")))
    }
}

pub(crate) fn log_output_excerpt(text: &str) -> String {
    const LIMIT: usize = 8_000;
    let mut output = text.chars().take(LIMIT).collect::<String>();
    if text.chars().count() > LIMIT {
        output.push_str(" …[truncated]");
    }
    output
}

pub(crate) fn captured_command_output(
    output: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
) -> String {
    output
        .lock()
        .map(|data| decode_command_bytes(&data))
        .unwrap_or_default()
}

pub(crate) fn emit_command_progress(window: &Option<tauri::Window>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let decoded = decode_command_bytes(bytes);
    let cleaned = decoded
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\t')
        .collect::<String>();
    let line = cleaned.trim();
    if !line.is_empty() {
        emit_progress(window, line);
    }
}

pub(crate) fn terminate_command_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut taskkill = Command::new("taskkill.exe");
        taskkill.args(["/PID", &child.id().to_string(), "/T", "/F"]);
        taskkill.creation_flags(0x08000000);
        let _ = taskkill.output();
    }
    let _ = child.kill();
}

pub(crate) fn command_dirs() -> Vec<PathBuf> {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    dirs
}

pub(crate) fn resolve_command(candidates: &[&str]) -> Option<PathBuf> {
    for dir in command_dirs() {
        let lower = dir.to_string_lossy().to_lowercase();
        if lower.contains("\\windowsapps") {
            continue;
        }
        for name in candidates {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub(crate) fn resolve_command_including_windowsapps(candidates: &[&str]) -> Option<PathBuf> {
    for dir in command_dirs() {
        for name in candidates {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub(crate) fn command_for_path(program: &Path, args: &[&str]) -> Command {
    let ext = program
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if matches!(ext.as_str(), "bat" | "cmd") {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/d", "/c", "call"]).arg(program);
        cmd.args(args);
        cmd
    } else if ext == "ps1" {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(program);
        cmd.args(args);
        cmd
    } else {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    }
}

pub(crate) fn apply_fresh_path(c: &mut Command) {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        c.env("PATH", path);
    }
}

pub(crate) fn run_program_probe(
    display_name: &str,
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let mut cmd = command_for_path(program, args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    apply_fresh_path(&mut cmd);
    let out = command_output_timeout_named(cmd, display_name, timeout)?;
    let version_text = output_text(&out);
    if !out.status.success() {
        return Err(first_output_line(&version_text).unwrap_or_else(|| "命令返回失败状态".into()));
    }
    Ok(first_output_line(&version_text).unwrap_or_else(|| "可用".into()))
}

pub(crate) fn command_output_timeout_named(
    mut c: Command,
    name: &str,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 {name} 命令失败：{e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{name} 命令响应超时"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub(crate) fn decode_command_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (text, _, _) = GBK.decode(bytes);
            text.into_owned()
        }
    }
}

pub(crate) fn output_text(out: &Output) -> String {
    let bytes = if out.stdout.is_empty() {
        &out.stderr
    } else {
        &out.stdout
    };
    decode_command_bytes(bytes).trim().to_string()
}

pub(crate) fn output_all_text(out: &Output) -> String {
    let mut text = String::new();
    let stdout = decode_command_bytes(&out.stdout);
    let stderr = decode_command_bytes(&out.stderr);
    if !stdout.trim().is_empty() {
        text.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim());
    }
    text
}

pub(crate) fn first_output_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| is_meaningful_output_line(line))
        .map(|line| {
            let mut out = line.to_string();
            if out.chars().count() > 180 {
                out = out.chars().take(180).collect::<String>() + "...";
            }
            out
        })
}

pub(crate) fn is_meaningful_output_line(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let stripped =
        line.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '=' | '_' | '*'));
    !stripped.is_empty()
}
