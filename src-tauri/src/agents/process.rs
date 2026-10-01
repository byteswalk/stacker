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
    let job = ProcessJob::attach(&child);

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

    // The command may hand its work to a process that outlives it and keeps writing to the
    // same pipes (Hermes does): it is done when its output closes, not when it exits. Cancel,
    // the timeout and the stall check apply until then, and end everything it started.
    let mut status = None;
    let failure = loop {
        if status.is_some() && stdout_reader.is_finished() && stderr_reader.is_finished() {
            break None;
        }
        if crate::installer::op_cancelled() {
            break Some("已取消操作".to_string());
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exited)) => {
                    status = Some(exited);
                    continue;
                }
                Ok(None) => {}
                Err(e) => break Some(format!("读取 {display_name} 状态失败：{e}")),
            }
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            log::error!(
                "external command timed out: name={display_name} elapsed_ms={} output={}",
                elapsed.as_millis(),
                log_output_excerpt(&captured_command_output(&output))
            );
            break Some(format!("{display_name} 执行超时，请检查网络后重试"));
        }
        let activity_ms = last_activity.load(Ordering::Relaxed);
        let inactive = if activity_ms == 0 {
            elapsed
        } else {
            elapsed.saturating_sub(Duration::from_millis(activity_ms))
        };
        if !stall_timeout.is_zero() && inactive >= stall_timeout {
            log::error!(
                "external command stalled: name={display_name} inactive_ms={} output={}",
                inactive.as_millis(),
                log_output_excerpt(&captured_command_output(&output))
            );
            break Some(format!(
                "{display_name} 连续 {} 秒没有响应，已停止操作。请检查网络或 WinGet 软件源后重试",
                stall_timeout.as_secs()
            ));
        }
        if last_heartbeat.elapsed() >= Duration::from_secs(1) && inactive >= Duration::from_secs(1)
        {
            emit_progress(
                window,
                format!("{display_name} 正在处理 · 已 {} 秒", elapsed.as_secs()),
            );
            last_heartbeat = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if let Some(message) = failure {
        stop_command(&mut child, job.as_ref());
        let _ = child.wait();
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        return Err(message);
    }
    let status = status.expect("the loop ends normally only after the command exited");

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
        // "Nothing matched" is the answer to a query, not a broken command: logging it as an
        // error made a clean run look like it had failures in it.
        if is_query_miss(&text) {
            log::info!(
                "external command found nothing: name={display_name} elapsed_ms={} output={}",
                started.elapsed().as_millis(),
                log_output_excerpt(&text)
            );
        } else {
            log::error!(
                "external command failed: name={display_name} exit_code={:?} elapsed_ms={} output={}",
                status.code(),
                started.elapsed().as_millis(),
                log_output_excerpt(&text)
            );
        }
        Err(failure_summary(&text).unwrap_or_else(|| format!("{display_name} 执行失败")))
    }
}

/// WinGet says so in these words when a package is simply not installed or not in a source.
pub(crate) fn is_query_miss(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "no installed package found",
        "no package found matching",
        "未找到与输入条件匹配的已安装程序包",
        "未找到与输入条件匹配的程序包",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
}

/// Why a command failed. Tools that end with a `✗`/`⚠` summary (Hermes does) are best
/// described by the last such line; the first line is often an unrelated warning.
pub(crate) fn failure_summary(text: &str) -> Option<String> {
    let lines = || text.lines().map(str::trim).rev();
    lines()
        .find(|line| line.starts_with('✗') || line.starts_with('⚠'))
        .map(|line| line.trim_start_matches(['✗', '⚠']).trim().to_string())
        .filter(|line| !line.is_empty())
        // Otherwise the last line that reports an error: WinGet ends a Store refusal with
        // "无法安装或更新 Microsoft Store 程序包。错误代码: 0x803fb015" after a page of details.
        .or_else(|| {
            lines()
                .find(|line| {
                    let lower = line.to_lowercase();
                    // npm signs off with "npm error A complete log of this run can be found
                    // in: …debug-0.log", which says nothing about what went wrong.
                    let pointer = lower.contains("complete log of this run")
                        || lower.ends_with("-debug-0.log");
                    !pointer
                        && ["错误", "无法", "失败", "error", "failed", "unable"]
                            .iter()
                            .any(|word| lower.contains(word))
                })
                .map(str::to_string)
        })
        .or_else(|| first_output_line(text))
}

/// Like `run_command_streamed`, but inside a pseudo console, so a program that only draws
/// progress on a console (WinGet) shows it: a download bar becomes Stacker's "正在下载 45% ·
/// 9.4/62.3 MB" line, and a taskbar percent becomes "正在处理 45%".
#[cfg(windows)]
pub(crate) fn run_command_console(
    program: &Path,
    args: &[&str],
    display_name: &str,
    timeout: Duration,
    stall_timeout: Duration,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    use super::pty::{bar_percent, download_line, download_sizes, run_in_pty, strip_vt, Stop};
    use std::sync::mpsc;

    // The whole environment, with the same PATH and proxy a piped command gets.
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(key, _)| !key.eq_ignore_ascii_case("PATH"))
        .collect();
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        env.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    if let Some(proxy) = crate::agents::net::stacker_proxy() {
        for (key, value) in crate::agents::net::proxy_env(&proxy) {
            env.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
            env.push((key.to_string(), value));
        }
    }
    log::info!(
        "external command started in a console: name={display_name} program={} args={args:?}",
        program.display()
    );

    // Console output arrives on the reader thread; lines are logged here, on the task's thread.
    let (sender, receiver) = mpsc::channel::<String>();
    let started = Instant::now();
    let mut text = String::new();
    let mut line = String::new();
    let mut last_activity = Duration::ZERO;
    let mut last_heartbeat = Instant::now();
    let mut shown_percent: Option<u32> = None;
    let mut saw_bar = false;
    let mut last_line = String::new();
    // True when the chunk said something new: a line, or a percentage that moved. Spinner
    // frames and redraws of the same text are not progress.
    let mut handle = |chunk: &str, elapsed: Duration, text: &mut String, line: &mut String| {
        let mut progressed = false;
        let (clean, taskbar) = strip_vt(chunk);
        for ch in clean.chars() {
            if ch != '\r' && ch != '\n' {
                line.push(ch);
                continue;
            }
            let current = line.trim().to_string();
            line.clear();
            if let Some((done, total)) = download_sizes(&current) {
                saw_bar = true;
                let percent = ((done / total) * 100.0).round() as u32;
                if shown_percent != Some(percent) {
                    shown_percent = Some(percent);
                    progressed = true;
                    emit_progress(window, download_line(done, total, elapsed));
                }
            } else if let Some(percent) = bar_percent(&current) {
                saw_bar = true;
                if shown_percent != Some(percent) {
                    shown_percent = Some(percent);
                    progressed = true;
                    emit_progress(
                        window,
                        format!("正在处理 {percent}% · 已 {}s", elapsed.as_secs()),
                    );
                }
            } else if current.chars().count() > 2 && current != last_line {
                // Spinner frames (- \ | /) are one character; real output is longer.
                progressed = true;
                emit_progress(window, current.as_str());
                text.push_str(&current);
                text.push('\n');
                last_line = current;
            }
        }
        if let (Some(percent), false) = (taskbar, saw_bar) {
            let percent = u32::from(percent);
            if shown_percent != Some(percent) {
                shown_percent = Some(percent);
                progressed = true;
                emit_progress(
                    window,
                    format!("正在处理 {percent}% · 已 {}s", elapsed.as_secs()),
                );
            }
        }
        progressed
    };
    let mut stall_noted = false;

    let result = run_in_pty(
        program,
        args,
        &env,
        move |chunk| {
            let _ = sender.send(chunk.to_string());
        },
        |elapsed| {
            while let Ok(chunk) = receiver.try_recv() {
                if handle(&chunk, elapsed, &mut text, &mut line) {
                    last_activity = elapsed;
                    stall_noted = false;
                }
            }
            if crate::installer::op_cancelled() {
                return Some(Stop::Cancelled);
            }
            if elapsed >= timeout {
                return Some(Stop::TimedOut);
            }
            let inactive = elapsed.saturating_sub(last_activity);
            if !stall_timeout.is_zero() && inactive >= stall_timeout {
                return Some(Stop::TimedOut);
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
            // Say once, plainly, that nothing is moving: a download server it cannot reach
            // looks exactly like this (Store apps download through the Store service).
            if !stall_noted && inactive >= Duration::from_secs(60) {
                stall_noted = true;
                emit_progress(
                    window,
                    format!(
                        "已 {} 秒没有新的进度，可能是网络连不上下载服务器。可以继续等待，或取消后检查网络和代理再试",
                        inactive.as_secs()
                    ),
                );
            }
            None
        },
        ProcessJob::attach_handle,
    );
    while let Ok(chunk) = receiver.try_recv() {
        handle(&chunk, started.elapsed(), &mut text, &mut line);
    }
    handle("\n", started.elapsed(), &mut text, &mut line);
    let text = text.trim().to_string();
    match result {
        Ok(0) => {
            log::info!(
                "external command completed: name={display_name} elapsed_ms={} output={}",
                started.elapsed().as_millis(),
                log_output_excerpt(&text)
            );
            Ok(text)
        }
        Ok(code) => {
            log::error!(
                "external command failed: name={display_name} exit_code={code} output={}",
                log_output_excerpt(&text)
            );
            Err(failure_summary(&text).unwrap_or_else(|| format!("{display_name} 执行失败")))
        }
        Err(Stop::Cancelled) => Err("已取消操作".into()),
        Err(Stop::TimedOut) => Err(format!("{display_name} 执行超时，请检查网络后重试")),
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

/// Every process a command starts, including ones that outlive it: `hermes update` hands its
/// work to a second Python process and exits. `taskkill /T` cannot reach a process whose
/// parent already exited; ending the job does. Descendants are not killed when the job is
/// merely dropped, so services a command restarts on purpose keep running.
pub(crate) struct ProcessJob {
    #[cfg(windows)]
    handle: winapi::um::winnt::HANDLE,
}

// The handle is only used to terminate or close the job; both are thread-safe calls.
unsafe impl Send for ProcessJob {}
unsafe impl Sync for ProcessJob {}

impl ProcessJob {
    /// For a process started without `std::process` (the pseudo console runner).
    #[cfg(windows)]
    pub(crate) fn attach_handle(process: *mut std::ffi::c_void) -> Option<Self> {
        use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW};
        // SAFETY: a fresh unnamed job and a live process handle owned by the caller.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let job = ProcessJob { handle };
            (AssignProcessToJobObject(handle, process as _) != 0).then_some(job)
        }
    }

    pub(crate) fn attach(child: &std::process::Child) -> Option<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW};
            // SAFETY: a fresh unnamed job; the child's handle stays valid while `child` lives.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let job = ProcessJob { handle };
                if AssignProcessToJobObject(handle, child.as_raw_handle() as _) == 0 {
                    return None;
                }
                Some(job)
            }
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            None
        }
    }

    pub(crate) fn terminate(&self) {
        #[cfg(windows)]
        // SAFETY: the handle is a job this value owns.
        unsafe {
            winapi::um::jobapi2::TerminateJobObject(self.handle, 1);
        }
    }
}

impl Drop for ProcessJob {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: closed exactly once, here.
        unsafe {
            winapi::um::handleapi::CloseHandle(self.handle);
        }
    }
}

/// Ends a command and everything it started.
pub(crate) fn stop_command(child: &mut std::process::Child, job: Option<&ProcessJob>) {
    if let Some(job) = job {
        job.terminate();
    }
    terminate_command_tree(child);
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

/// The script an npm `.cmd` shim runs: the quoted `"%dp0%\node_modules\…"` path, resolved
/// against the shim's folder. The shim names `"%dp0%\node.exe"` first, so every quoted
/// `%dp0%` path is looked at.
pub(crate) fn npm_shim_script(shim: &Path, text: &str) -> Option<PathBuf> {
    let relative = text.split("\"%dp0%\\").skip(1).find_map(|rest| {
        let relative = &rest[..rest.find('"')?];
        relative
            .to_ascii_lowercase()
            .starts_with("node_modules\\")
            .then_some(relative)
    })?;
    Some(shim.parent()?.join(relative))
}

/// How to start a command. An npm `.cmd` shim is skipped for `node <script>`: Rust refuses a
/// batch file any argument it cannot quote safely for cmd.exe (a line break, for one), and
/// prompts are full of those. Anything else starts as it is.
pub(crate) fn program_command(path: &Path) -> std::process::Command {
    let is_batch = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
    if is_batch {
        let script = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| npm_shim_script(path, &text))
            .filter(|script| script.is_file());
        if let Some(script) = script {
            // The shim prefers a node.exe beside it, then the one on PATH.
            let node = path
                .parent()
                .map(|dir| dir.join("node.exe"))
                .filter(|node| node.is_file())
                .or_else(|| resolve_command(&["node.exe"]));
            if let Some(node) = node {
                let mut cmd = std::process::Command::new(node);
                hide_console(&mut cmd);
                cmd.arg(script);
                return cmd;
            }
        }
    }
    let mut cmd = std::process::Command::new(path);
    hide_console(&mut cmd);
    cmd
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

/// Windows hands a console program its own window when a GUI app starts it. Everything built
/// here runs on Stacker's behalf, so the window is refused; the places that mean to show one
/// (a terminal for the user to watch an install in) build their command themselves.
fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
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
        hide_console(&mut cmd);
        cmd
    } else if ext == "ps1" {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(program);
        cmd.args(args);
        hide_console(&mut cmd);
        cmd
    } else {
        let mut cmd = Command::new(program);
        cmd.args(args);
        hide_console(&mut cmd);
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
    // Windows gives a console program its own window when a GUI app starts it, unless it is
    // told not to; nothing run through here is meant to be watched.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_query_that_matched_nothing_is_not_a_failure() {
        assert!(super::is_query_miss(
            "No installed package found matching input criteria."
        ));
        assert!(super::is_query_miss("未找到与输入条件匹配的已安装程序包。"));
        assert!(!super::is_query_miss("0x8a15000f : package install failed"));
    }

    /// npm's last line only points at its log file; the useful line is the one above it.
    #[test]
    fn npm_target_failure_names_the_missing_version() {
        let output = "npm install 正在处理 · 已 6 秒\n\
             npm error code ETARGET\n\
             npm error notarget No matching version found for @openai/codex@0.156.1.\n\
             npm error A complete log of this run can be found in: /logs/2026-debug-0.log\n";
        let summary = super::failure_summary(output).unwrap();
        assert!(
            summary.contains("No matching version found for @openai/codex@0.156.1"),
            "{summary}"
        );
        assert!(crate::agents::install::npm::mirror_is_behind(&summary));
    }

    use super::*;

    #[test]
    fn a_failure_is_described_by_its_last_summary_line() {
        // What `hermes update` printed when its npm step failed: the first line is noise.
        let hermes = "npm warn Unknown project config \"min-release-age-exclude\".\n\
                      → Updating Node.js dependencies...\n  ⚠ npm install failed\n\
                      ⚠ Checkout is current, but Node.js dependencies could not be repaired.";
        assert_eq!(
            failure_summary(hermes).as_deref(),
            Some("Checkout is current, but Node.js dependencies could not be repaired.")
        );
        assert_eq!(
            failure_summary("fatal: not a git repository").as_deref(),
            Some("fatal: not a git repository")
        );
        // WinGet's Store refusal comes last, after the package details.
        let store = "已找到 ChatGPT [9PLM9XGG6VKS] 版本 Unknown\n发布者: OpenAI\n正在启动程序包安装...\n无法安装或更新 Microsoft Store 程序包。错误代码: 0x803fb015";
        assert_eq!(
            failure_summary(store).as_deref(),
            Some("无法安装或更新 Microsoft Store 程序包。错误代码: 0x803fb015")
        );
    }
}

#[cfg(test)]
mod shim_tests {
    use super::*;

    #[test]
    fn an_npm_shim_names_the_script_it_runs() {
        let shim = Path::new(r"D:\node_global\kimi.cmd");
        let text = r#"IF EXIST "%dp0%\node.exe" ( SET "_prog=%dp0%\node.exe" ) ELSE ( SET "_prog=node" )
endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\node_modules\@moonshot-ai\kimi-code\dist\main.mjs" %*"#;
        assert_eq!(
            npm_shim_script(shim, text),
            Some(PathBuf::from(
                r"D:\node_global\node_modules\@moonshot-ai\kimi-code\dist\main.mjs"
            ))
        );
        assert_eq!(npm_shim_script(shim, "@echo off\r\nrun.exe %*"), None);
    }
}
