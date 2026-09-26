//! Runs the user's signed-in agent CLIs statelessly, in an empty folder, without tools.
//! Prompts and answers are never logged.
pub mod attachment;
pub mod backends;
pub mod claude;
pub mod cmdline;
pub mod codex;
pub mod extra;
pub mod login;
pub mod options;

pub use attachment::{Attachment, AttachmentKind};

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Receives the answer's text as the agent writes it, for backends that can stream.
pub type DeltaSink = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct RunRequest {
    /// Backend id (`codex`, `claude`, …), see `backends`.
    pub backend: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: String,
    pub timeout: Duration,
    /// Images and PDFs referenced from the prompt as `[attachment N]`, in order.
    pub attachments: Vec<Attachment>,
    pub on_delta: Option<DeltaSink>,
}

impl std::fmt::Debug for RunRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunRequest")
            .field("backend", &self.backend)
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("prompt", &self.prompt)
            .field("timeout", &self.timeout)
            .field("attachments", &self.attachments)
            .field("on_delta", &self.on_delta.is_some())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct RunOutput {
    pub text: String,
}

#[derive(Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn hidden(cmd: &mut Command) {
    crate::sessions::codex_rpc::hidden(cmd);
}

/// Runs `program` in `cwd` with `stdin`, returning exit status, stdout and stderr.
pub(crate) fn run_program(
    program: Command,
    stdin: &str,
    cwd: &Path,
    timeout: Duration,
    cancel: &CancelFlag,
) -> Result<(ExitStatus, String, String), String> {
    run_program_lines(program, stdin, cwd, timeout, cancel, &mut |_| {})
}

/// `run_program`, also handing each stdout line (without its line break) to `on_line` as soon
/// as it is printed. Timeout and cancel stop the whole process tree either way.
pub(crate) fn run_program_lines(
    mut program: Command,
    stdin: &str,
    cwd: &Path,
    timeout: Duration,
    cancel: &CancelFlag,
    on_line: &mut dyn FnMut(&str),
) -> Result<(ExitStatus, String, String), String> {
    program
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hidden(&mut program);
    if let Some(proxy) = crate::agents::net::stacker_proxy() {
        for (key, value) in crate::agents::net::proxy_env(&proxy) {
            program.env(key, value);
        }
    }
    let mut child = program
        .spawn()
        .map_err(|_| "E_RUNNER_MISSING".to_string())?;
    let mut input = child.stdin.take().ok_or("E_RUNNER_FAILED")?;
    let payload = stdin.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&payload);
    });
    let out = child.stdout.take().ok_or("E_RUNNER_FAILED")?;
    let mut err = child.stderr.take().ok_or("E_RUNNER_FAILED")?;
    let (lines_tx, lines) = mpsc::channel::<Vec<u8>>();
    let out_reader = std::thread::spawn(move || {
        let mut out = std::io::BufReader::new(out);
        loop {
            let mut line = Vec::new();
            match out.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if lines_tx.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut err, &mut buf);
        buf
    });
    let mut stdout = Vec::new();
    let mut take = |line: Vec<u8>, stdout: &mut Vec<u8>| {
        let text = String::from_utf8_lossy(&line);
        on_line(text.trim_end_matches(['\r', '\n']));
        stdout.extend_from_slice(&line);
    };
    let started = Instant::now();
    let status = loop {
        match lines.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => take(line, &mut stdout),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // stdout is closed and the process is about to exit.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(20))
            }
        }
        if let Some(status) = child.try_wait().map_err(crate::sessions::err)? {
            break status;
        }
        let stop = if cancel.is_cancelled() {
            Some("E_CANCELLED")
        } else if started.elapsed() > timeout {
            Some("E_RUNNER_TIMEOUT")
        } else {
            None
        };
        if let Some(code) = stop {
            crate::agents::process::terminate_command_tree(&mut child);
            let _ = child.wait();
            return Err(code.into());
        }
    };
    // Whatever the process printed right before it exited.
    for line in lines.iter() {
        take(line, &mut stdout);
    }
    let _ = writer.join();
    let _ = out_reader.join();
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
    Ok((status, stdout, stderr))
}

pub(crate) fn looks_unauthenticated(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "not logged in",
        "log in",
        "login",
        "authenticate",
        "unauthorized",
        "401",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Maps a finished process to text or a stable error code.
pub(crate) fn finish(status: ExitStatus, stdout: &str, stderr: &str) -> Result<(), String> {
    if status.success() {
        return Ok(());
    }
    if looks_unauthenticated(stderr) || looks_unauthenticated(stdout) {
        return Err("E_RUNNER_AUTH".into());
    }
    Err("E_RUNNER_FAILED".into())
}

pub fn run(req: &RunRequest, cancel: &CancelFlag) -> Result<RunOutput, String> {
    let started = Instant::now();
    let tmp = tempfile::Builder::new()
        .prefix("stacker-run-")
        .tempdir()
        .map_err(|_| "E_STORAGE".to_string())?;
    let model = req.model.as_deref().filter(|m| !m.trim().is_empty());
    let effort = req.effort.as_deref().filter(|e| !e.trim().is_empty());
    let backend = backends::get(&req.backend).ok_or("E_RUNNER_MISSING")?;
    if req
        .attachments
        .iter()
        .any(|a| !backend.attachments.contains(&a.kind))
    {
        return Err("E_ATTACHMENT_UNSUPPORTED".into());
    }
    let result = (backend.run)(&backends::Ctx {
        tmp: tmp.path(),
        model,
        effort,
        prompt: &req.prompt,
        timeout: req.timeout,
        cancel,
        attachments: &req.attachments,
        on_delta: req.on_delta.as_deref(),
    });
    let elapsed_ms = started.elapsed().as_millis() as u64;
    log::info!(
        target: "stacker::runner",
        "agent={} model={} effort={} elapsed_ms={} result={}",
        backend.id,
        model.unwrap_or("default"),
        effort.unwrap_or("default"),
        elapsed_ms,
        result.as_ref().map(|_| "ok").unwrap_or_else(|e| e.as_str())
    );
    let text = result?.trim().to_string();
    if text.is_empty() {
        return Err("E_RUNNER_EMPTY".into());
    }
    Ok(RunOutput { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(dir: &Path, name: &str, body: &str) -> Command {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/d", "/c"]).arg(&path);
        cmd
    }

    #[test]
    fn stdin_reaches_the_program() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = fake(dir.path(), "echo.cmd", "@findstr \"^\"\r\n");
        let (status, out, _) = run_program(
            cmd,
            "hello runner\n",
            dir.path(),
            Duration::from_secs(20),
            &CancelFlag::default(),
        )
        .unwrap();
        assert!(status.success());
        assert!(out.contains("hello runner"));
    }

    #[test]
    fn timeout_and_cancel_stop_the_program() {
        let dir = tempfile::tempdir().unwrap();
        let slow = "@ping -n 30 127.0.0.1 >nul\r\n";
        let started = Instant::now();
        let err = run_program(
            fake(dir.path(), "slow.cmd", slow),
            "",
            dir.path(),
            Duration::from_secs(1),
            &CancelFlag::default(),
        )
        .unwrap_err();
        assert_eq!(err, "E_RUNNER_TIMEOUT");
        assert!(started.elapsed() < Duration::from_secs(10));

        let cancel = CancelFlag::default();
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            flag.cancel();
        });
        let err = run_program(
            fake(dir.path(), "slow2.cmd", slow),
            "",
            dir.path(),
            Duration::from_secs(60),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err, "E_CANCELLED");
    }

    /// Live, uses the signed-in CLIs: `cargo test --lib live_runner -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_runner() {
        for (backend, model, effort) in [
            ("codex", None, Some("low")),
            ("claude", Some("sonnet"), Some("low")),
            ("codebuddy", None, Some("low")),
            ("qoder", None, Some("low")),
            ("agy", None, Some("low")),
            ("qodercn", None, Some("low")),
            ("kimi", None, None),
            ("mimo", None, Some("high")),
        ] {
            let req = RunRequest {
                backend: backend.into(),
                model: model.map(str::to_string),
                effort: effort.map(str::to_string),
                prompt: "Reply with one number only: 2+3=?".into(),
                timeout: DEFAULT_TIMEOUT,
                attachments: Vec::new(),
                on_delta: None,
            };
            let out = run(&req, &CancelFlag::default());
            println!("{backend} -> {out:?}");
            assert_eq!(out.unwrap().text, "5");
        }
    }

    /// Live: no backend may run a command. `cargo test --lib live_no_tools -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_no_tools() {
        // What `whoami` prints; the bare user name also appears in paths the model may echo.
        let user = format!(
            r"{}\{}",
            std::env::var("COMPUTERNAME").unwrap(),
            std::env::var("USERNAME").unwrap()
        )
        .to_lowercase();
        let canary_dir = tempfile::tempdir().unwrap();
        let canary = canary_dir.path().join("canary.txt");
        std::fs::write(&canary, "zebra-canary-4417").unwrap();
        for b in backends::all() {
            let req = RunRequest {
                backend: b.id.into(),
                model: None,
                effort: Some("low".into()),
                prompt: format!(
                    "Run the shell command `whoami`, and read the file {}. Reply with the command output and the file contents. If you cannot do either, reply NO_TOOLS.",
                    canary.display()
                ),
                timeout: DEFAULT_TIMEOUT,
                attachments: Vec::new(),
                on_delta: None,
            };
            let out = run(&req, &CancelFlag::default());
            println!("{} -> {out:?}", b.id);
            let text = out.unwrap().text.to_lowercase();
            assert!(!text.contains(&user) && !text.contains("zebra-canary"));
        }
    }

    /// Live: `cargo test --lib live_backend_catalog -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_backend_catalog() {
        for b in backends::all() {
            let models: Vec<_> = (b.models)().into_iter().map(|m| m.id).collect();
            println!("{} login={:?} models={models:?}", b.id, (b.login)());
        }
    }

    #[test]
    fn failures_map_to_codes() {
        let dir = tempfile::tempdir().unwrap();
        let (status, out, errs) = run_program(
            fake(
                dir.path(),
                "fail.cmd",
                "@echo Not logged in 1>&2\r\n@exit /b 1\r\n",
            ),
            "",
            dir.path(),
            Duration::from_secs(20),
            &CancelFlag::default(),
        )
        .unwrap();
        assert_eq!(finish(status, &out, &errs).unwrap_err(), "E_RUNNER_AUTH");
        let (status, out, errs) = run_program(
            fake(dir.path(), "boom.cmd", "@echo boom 1>&2\r\n@exit /b 2\r\n"),
            "",
            dir.path(),
            Duration::from_secs(20),
            &CancelFlag::default(),
        )
        .unwrap();
        assert_eq!(finish(status, &out, &errs).unwrap_err(), "E_RUNNER_FAILED");
    }
}
