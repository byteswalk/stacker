//! What the AI jobs (summaries, distilling, handoffs) pass around, and a process runner with a
//! timeout and cancel for the few agent CLI checks that remain (sign-in status for repairs).
pub mod login;

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct RunRequest {
    /// Empty: the configured external API answers. Agent CLIs are no longer driven.
    pub backend: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: String,
    pub timeout: Duration,
}

impl std::fmt::Debug for RunRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunRequest")
            .field("backend", &self.backend)
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("prompt", &self.prompt)
            .field("timeout", &self.timeout)
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

pub(crate) fn hidden(cmd: &mut Command) {
    crate::sessions::codex_rpc::hidden(cmd);
}

/// An agent's CLI while it runs: started in a hidden console where Windows allows it, so
/// what it starts in turn shares that console instead of flashing up one of its own.
enum Running {
    Plain(std::process::Child),
    #[cfg(windows)]
    Hidden(crate::agents::hidden_console::HiddenChild),
}

type Pipes = (
    Box<dyn Write + Send>,
    Box<dyn std::io::Read + Send>,
    Box<dyn std::io::Read + Send>,
);

impl Running {
    fn start(program: &mut Command) -> std::io::Result<Self> {
        #[cfg(windows)]
        match crate::agents::hidden_console::spawn(program) {
            Ok(child) => return Ok(Running::Hidden(child)),
            Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {}
            Err(error) => return Err(error),
        }
        program.spawn().map(Running::Plain)
    }

    fn pipes(&mut self) -> Option<Pipes> {
        match self {
            Running::Plain(child) => Some((
                Box::new(child.stdin.take()?),
                Box::new(child.stdout.take()?),
                Box::new(child.stderr.take()?),
            )),
            #[cfg(windows)]
            Running::Hidden(child) => Some((
                Box::new(child.stdin.take()?),
                Box::new(child.stdout.take()?),
                Box::new(child.stderr.take()?),
            )),
        }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match self {
            Running::Plain(child) => child.try_wait(),
            #[cfg(windows)]
            Running::Hidden(child) => child.try_wait(),
        }
    }

    /// Ends the CLI and everything it started.
    fn stop(&mut self) {
        match self {
            Running::Plain(child) => {
                crate::agents::process::terminate_command_tree(child);
                let _ = child.wait();
            }
            #[cfg(windows)]
            Running::Hidden(child) => {
                child.kill_tree();
                let _ = child.wait();
            }
        }
    }
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
    let mut child = Running::start(&mut program).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "E_RUNNER_MISSING".to_string()
        } else {
            log::warn!(target: "stacker::runner", "could not start the agent CLI: {error}");
            "E_RUNNER_START".to_string()
        }
    })?;
    let (mut input, out, mut err) = child.pipes().ok_or("E_RUNNER_FAILED")?;
    let payload = stdin.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&payload);
    });
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
            child.stop();
            return Err(code.into());
        }
    };
    // Whatever the process printed right before it exited — for a few seconds at most: a
    // process it left behind (a background updater) can hold the pipes open for good.
    let drained_by = Instant::now() + Duration::from_secs(3);
    loop {
        let left = drained_by.saturating_duration_since(Instant::now());
        match lines.recv_timeout(left) {
            Ok(line) => take(line, &mut stdout),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                child.stop();
                break;
            }
        }
    }
    while !err_reader.is_finished() && Instant::now() < drained_by {
        std::thread::sleep(Duration::from_millis(20));
    }
    let stderr = if err_reader.is_finished() {
        err_reader.join().unwrap_or_default()
    } else {
        Vec::new()
    };
    if writer.is_finished() {
        let _ = writer.join();
    }
    if out_reader.is_finished() {
        let _ = out_reader.join();
    }
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr = String::from_utf8_lossy(&stderr).into_owned();
    Ok((status, stdout, stderr))
}
