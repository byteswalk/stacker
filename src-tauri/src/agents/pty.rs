//! Runs a command inside a Windows pseudo console (ConPTY). WinGet draws its download bar
//! ("████ 9.35 MB / 62.3 MB") only when it believes it writes to a console; through a plain
//! pipe it prints no progress at all. The console output is turned back into lines, and the
//! bar into the same "正在下载 45% · 9.4/62.3 MB" line Stacker's own downloader writes.

use std::time::Duration;

/// Output with terminal control sequences removed, and the taskbar progress a program set
/// through `ESC ] 9 ; 4 ; state ; percent` (WinGet and the Store both use it), if any.
pub(crate) fn strip_vt(raw: &str) -> (String, Option<u8>) {
    let mut out = String::with_capacity(raw.len());
    let mut percent = None;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            // CSI: parameters, then one final byte in @..~. WinGet redraws its spinner and
            // bars by moving the cursor (ESC[H, ESC[K) instead of a carriage return: those end
            // a line too, or everything after them would pile onto one line never shown.
            Some('[') => {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        if matches!(next, 'H' | 'f' | 'K' | 'G' | 'J') {
                            out.push('\r');
                        }
                        break;
                    }
                }
            }
            // OSC: up to BEL or ESC \.
            Some(']') => {
                let mut body = String::new();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                    body.push(next);
                }
                if let Some(rest) = body.strip_prefix("9;4;") {
                    let mut parts = rest.split(';');
                    let state = parts.next().unwrap_or("");
                    let value = parts.next().and_then(|v| v.trim().parse::<u8>().ok());
                    // 1 = normal progress; 0 clears it.
                    if state == "1" {
                        percent = value.map(|v| v.min(100)).or(percent);
                    }
                }
            }
            // Two-character sequences (ESC =, ESC >, …).
            Some(_) | None => {}
        }
    }
    (out, percent)
}

/// `(done MB, total MB)` from a progress bar line such as `███  9.35 MB / 62.3 MB`.
pub(crate) fn download_sizes(line: &str) -> Option<(f64, f64)> {
    let unit = |u: &str| match u {
        "KB" => Some(1.0 / 1024.0),
        "MB" => Some(1.0),
        "GB" => Some(1024.0),
        _ => None,
    };
    let (left, right) = line.split_once('/')?;
    let number_unit = |text: &str, from_end: bool| -> Option<f64> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let (num, u) = if from_end {
            let n = words.len();
            (words.get(n.checked_sub(2)?)?, words.get(n - 1)?)
        } else {
            (words.first()?, words.get(1)?)
        };
        Some(num.parse::<f64>().ok()? * unit(u.trim_end_matches(']'))?)
    };
    let done = number_unit(left, true)?;
    let total = number_unit(right, false)?;
    (total > 0.0 && done <= total * 1.01).then_some((done, total))
}

/// The percent a bar with no sizes shows (`████▒▒▒▒  45%`), as the Store install draws.
pub(crate) fn bar_percent(line: &str) -> Option<u32> {
    if !line
        .chars()
        .any(|c| matches!(c, '█' | '▒' | '▓' | '░' | '■'))
    {
        return None;
    }
    let number = line.trim_end().strip_suffix('%')?;
    let start = number
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |i| i + 1);
    number[start..].parse::<u32>().ok().filter(|p| *p <= 100)
}

/// The line the task log and the progress bar understand.
pub(crate) fn download_line(done: f64, total: f64, elapsed: Duration) -> String {
    let percent = ((done / total) * 100.0).clamp(0.0, 100.0).round() as u32;
    format!(
        "正在下载 {percent}% · {done:.1}/{total:.1} MB · 已 {}s",
        elapsed.as_secs()
    )
}

#[cfg(windows)]
pub(crate) use imp::{run_in_pty, Stop};

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::Console::{
        ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
        PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESHOWWINDOW,
        STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    /// Why a run ended early.
    pub(crate) enum Stop {
        Cancelled,
        TimedOut,
    }

    struct Handles(Vec<HANDLE>);
    impl Drop for Handles {
        fn drop(&mut self) {
            for &h in &self.0 {
                if !h.is_null() {
                    // SAFETY: each handle was opened here and is closed once.
                    unsafe { CloseHandle(h) };
                }
            }
        }
    }

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Windows command-line quoting for one argument.
    fn quote(arg: &str) -> String {
        if !arg.is_empty() && !arg.chars().any(|c| c == ' ' || c == '\t' || c == '"') {
            return arg.to_string();
        }
        let mut out = String::from("\"");
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                _ => {
                    out.push_str(&"\\".repeat(backslashes));
                    out.push(c);
                    backslashes = 0;
                }
            }
        }
        out.push_str(&"\\".repeat(backslashes * 2));
        out.push('"');
        out
    }

    /// Runs `program args…` in a pseudo console with `env` (the whole environment), handing
    /// raw console output to `on_output` as it arrives. `stop` is polled; when it returns a
    /// reason the process tree is ended. Returns the exit code.
    pub(crate) fn run_in_pty(
        program: &Path,
        args: &[&str],
        env: &[(String, String)],
        mut on_output: impl FnMut(&str) + Send + 'static,
        mut stop: impl FnMut(Duration) -> Option<Stop>,
        job_for: impl Fn(HANDLE) -> Option<crate::agents::process::ProcessJob>,
    ) -> Result<u32, Stop> {
        let spawn_error = |_: &str| Stop::Cancelled;
        // SAFETY: plain Win32 calls; every handle is owned by `handles` or closed below, the
        // attribute list outlives CreateProcessW, and the reader thread owns its pipe end.
        unsafe {
            let (mut in_read, mut in_write, mut out_read, mut out_write): (
                HANDLE,
                HANDLE,
                HANDLE,
                HANDLE,
            ) = (null_mut(), null_mut(), null_mut(), null_mut());
            if CreatePipe(&mut in_read, &mut in_write, null(), 0) == 0
                || CreatePipe(&mut out_read, &mut out_write, null(), 0) == 0
            {
                return Err(spawn_error("pipe"));
            }
            let mut console: HPCON = 0;
            let size = COORD { X: 120, Y: 30 };
            if CreatePseudoConsole(size, in_read, out_write, 0, &mut console) < 0 {
                let _ = Handles(vec![in_read, in_write, out_read, out_write]);
                return Err(spawn_error("console"));
            }
            // The console holds its own references to these two ends.
            let pipe_ends = Handles(vec![in_read, out_write]);

            let mut bytes = 0usize;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
            let mut attrs = vec![0u8; bytes];
            let list = attrs.as_mut_ptr().cast();
            if InitializeProcThreadAttributeList(list, 1, 0, &mut bytes) == 0
                || UpdateProcThreadAttribute(
                    list,
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                    console as *const _,
                    std::mem::size_of::<HPCON>(),
                    null_mut(),
                    null(),
                ) == 0
            {
                ClosePseudoConsole(console);
                let _ = Handles(vec![in_write, out_read]);
                return Err(spawn_error("attributes"));
            }
            let mut startup: STARTUPINFOEXW = std::mem::zeroed();
            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = list;
            // With no standard handles of its own the child would inherit this process's
            // (redirected) ones and bypass the console; empty ones make it use the console.
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
            // Should Windows still give the child a console window, it starts hidden. Not
            // CREATE_NO_WINDOW: that detaches the child from the pseudo console, and not a
            // word of its output arrives.
            startup.StartupInfo.wShowWindow = SW_HIDE as u16;

            let mut command_line = std::iter::once(quote(&program.to_string_lossy()))
                .chain(args.iter().map(|a| quote(a)))
                .collect::<Vec<_>>()
                .join(" ");
            command_line.push('\0');
            let mut command_line: Vec<u16> = command_line.encode_utf16().collect();
            let mut block: Vec<u16> = Vec::new();
            for (key, value) in env {
                block.extend(OsStr::new(&format!("{key}={value}")).encode_wide());
                block.push(0);
            }
            block.push(0);
            let application = wide(program.as_os_str());

            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let created = CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                block.as_ptr().cast(),
                null(),
                &startup.StartupInfo,
                &mut info,
            );
            DeleteProcThreadAttributeList(list);
            if created == 0 {
                ClosePseudoConsole(console);
                let _ = Handles(vec![in_write, out_read]);
                return Err(spawn_error("process"));
            }
            let process = Handles(vec![info.hProcess, info.hThread]);
            // Ours go now: while this process still holds the console's output end, closing the
            // console never ends the output, and the reader waits out its five seconds.
            drop(pipe_ends);
            let job = job_for(info.hProcess);

            // Drain the console until it closes; a full pipe would stall the program.
            let out_read = out_read as usize;
            let done = Arc::new(AtomicBool::new(false));
            let reader_done = done.clone();
            let reader = std::thread::spawn(move || {
                let handle = out_read as HANDLE;
                let mut buf = vec![0u8; 8192];
                let mut pending = Vec::new();
                loop {
                    let mut read = 0u32;
                    if ReadFile(
                        handle,
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        &mut read,
                        null_mut(),
                    ) == 0
                        || read == 0
                    {
                        break;
                    }
                    pending.extend_from_slice(&buf[..read as usize]);
                    // Hand over whole UTF-8 sequences only.
                    let valid = match std::str::from_utf8(&pending) {
                        Ok(_) => pending.len(),
                        Err(e) => e.valid_up_to(),
                    };
                    if valid > 0 {
                        let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
                        pending.drain(..valid);
                        on_output(&text);
                    }
                }
                CloseHandle(handle);
                reader_done.store(true, Ordering::SeqCst);
            });

            let started = Instant::now();
            let outcome;
            loop {
                if WaitForSingleObject(info.hProcess, 100) == WAIT_OBJECT_0 {
                    let mut code = 0u32;
                    GetExitCodeProcess(info.hProcess, &mut code);
                    outcome = Ok(code);
                    break;
                }
                if let Some(reason) = stop(started.elapsed()) {
                    if let Some(job) = &job {
                        job.terminate();
                    }
                    TerminateProcess(info.hProcess, 1);
                    outcome = Err(reason);
                    break;
                }
            }
            // Closing the console ends its output, which lets the reader finish.
            ClosePseudoConsole(console);
            CloseHandle(in_write);
            let waited = Instant::now();
            while !done.load(Ordering::SeqCst) && waited.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(20));
            }
            if done.load(Ordering::SeqCst) {
                let _ = reader.join();
            }
            drop(process);
            outcome
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_output_loses_its_control_sequences_and_keeps_the_taskbar_percent() {
        let raw = "\u{1b}[?25l\u{1b}[2K  ██████  9.35 MB / 62.3 MB\u{1b}]9;4;1;15\u{7}\r\u{1b}[0mdone\u{1b}]9;4;0;0\u{1b}\\";
        let (text, percent) = strip_vt(raw);
        assert_eq!(text, "\r  ██████  9.35 MB / 62.3 MB\rdone");
        assert_eq!(percent, Some(15));
        assert_eq!(strip_vt("plain").1, None);
    }

    #[test]
    fn cursor_moves_end_lines_the_way_winget_redraws_need() {
        // Recorded from `winget install` in a console: two spinner frames, then text.
        let raw = "\u{1b}[38;2;50;116;207m   - \u{1b}[m\u{1b}[H   | \u{1b}[m\u{1b}[H\u{1b}[K\u{1b}[120CFound it";
        let (text, _) = strip_vt(raw);
        let lines: Vec<&str> = text
            .split('\r')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(lines, ["-", "|", "Found it"]);
        assert_eq!(bar_percent("  ██████████▒▒▒▒▒▒▒▒▒  45%"), Some(45));
        assert_eq!(bar_percent("Improved startup by 30%"), None);
    }

    #[test]
    fn a_download_bar_becomes_stackers_download_line() {
        assert_eq!(
            download_sizes("  ██████  9.35 MB / 62.3 MB"),
            Some((9.35, 62.3))
        );
        assert_eq!(download_sizes("1024 KB / 62.3 MB"), Some((1.0, 62.3)));
        assert_eq!(download_sizes("Found Git [Git.Git] Version 2.55.0"), None);
        assert_eq!(download_sizes("ratio 3 / 4"), None);
        assert_eq!(
            download_line(9.35, 62.3, Duration::from_secs(12)),
            "正在下载 15% · 9.3/62.3 MB · 已 12s"
        );
    }
}

#[cfg(all(test, windows))]
mod console_tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// What runs in the console reaches us, and the run ends when the program does. Both broke
    /// once: a flag meant to hide a window cut the child off from the console, and a pipe end
    /// left open made every run wait five more seconds.
    #[test]
    fn a_program_in_the_console_is_heard_and_done_when_it_exits() {
        let out = Arc::new(Mutex::new(String::new()));
        let sink = out.clone();
        let env: Vec<(String, String)> = std::env::vars().collect();
        let started = Instant::now();
        let code = super::run_in_pty(
            std::path::Path::new(r"C:\Windows\System32\cmd.exe"),
            &["/c", "echo", "stacker-console-check"],
            &env,
            move |chunk| sink.lock().unwrap().push_str(chunk),
            |elapsed| (elapsed > Duration::from_secs(20)).then_some(super::Stop::TimedOut),
            |_| None,
        );
        assert!(matches!(code, Ok(0)));
        let (text, _) = super::strip_vt(&out.lock().unwrap());
        assert!(text.contains("stacker-console-check"), "{text:?}");
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
    }
}
