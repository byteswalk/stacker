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
        "authentication failed",
        "please sign in",
        "sign in to",
        "unauthorized",
        "401",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// An account with no plan or credit left for the CLI: Kimi answers with its pricing page,
/// MiMo with "Insufficient account balance", Qoder with its credit limit. A desktop app's free
/// chat allowance does not carry over to these CLIs.
fn looks_unpaid(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "insufficient account balance",
        "insufficient balance",
        "insufficient credit",
        "credit usage limit",
        "out of credits",
        "quota exceeded",
        "exceeded your current quota",
        "upgrade your subscription",
        "/pricing",
        "#pricing",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// An account the vendor will not serve: signed in, and refused all the same.
fn looks_ineligible(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "not eligible",
        "eligibility check failed",
        "not available in your country",
        "not available in your region",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// The CLI's own words for the most recent failed run. Errors travel as stable codes, which
/// the page can translate; this is the sentence behind the code, for the page to show under
/// it. Nothing decides anything by reading it.
static LAST_FAILURE: std::sync::Mutex<Option<(Instant, String)>> = std::sync::Mutex::new(None);

/// The one sentence worth keeping out of what a CLI printed before it gave up.
fn failure_sentence(stdout: &str, stderr: &str) -> String {
    let lines: Vec<&str> = [stderr, stdout]
        .iter()
        .flat_map(|text| text.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('{') && !line.starts_with('['))
        .collect();
    let mut sentence: String = lines.first().unwrap_or(&"").chars().take(300).collect();
    // A CLI that refuses often says where to go and sort it out; that link is the useful part.
    let link = lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .find(|word| word.starts_with("https://"));
    if let Some(link) = link {
        if !sentence.contains(link) {
            sentence.push(' ');
            sentence.push_str(link);
        }
    }
    sentence
}

fn note_failure(stdout: &str, stderr: &str) {
    let sentence = failure_sentence(stdout, stderr);
    if let Ok(mut slot) = LAST_FAILURE.lock() {
        *slot = (!sentence.is_empty()).then(|| (Instant::now(), sentence));
    }
}

/// What the CLI said about the failure that just happened, when it is recent enough to be
/// about the run the caller is asking about.
pub fn last_failure(within: Duration) -> Option<String> {
    let slot = LAST_FAILURE.lock().ok()?;
    let (at, sentence) = slot.as_ref()?;
    (at.elapsed() <= within).then(|| sentence.clone())
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    fn failed(stdout: &str, stderr: &str) -> String {
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(1)
        };
        finish(status, stdout, stderr).unwrap_err()
    }

    #[test]
    fn a_refusal_is_told_apart_from_a_crash() {
        // What Antigravity answers an account without access, word for word.
        let refused =
            "error: Eligibility check failed: Your current account is not eligible for Antigravity.";
        assert_eq!(failed(refused, ""), "E_RUNNER_INELIGIBLE");
        // Signed in, with no plan or credit for the CLI.
        assert_eq!(
            failed("", "kimi version 2.1.1 https://www.kimi.com/code/#pricing"),
            "E_RUNNER_NO_PLAN"
        );
        assert_eq!(
            failed("Insufficient account balance", ""),
            "E_RUNNER_NO_PLAN"
        );
        assert_eq!(
            failed(
                "You've reached your credit usage limit. Please upgrade your subscription plan.",
                ""
            ),
            "E_RUNNER_NO_PLAN"
        );
        assert_eq!(
            failed("", "Please sign in to view available models."),
            "E_RUNNER_AUTH"
        );
        assert_eq!(failed("", "panicked at src/main.rs"), "E_RUNNER_FAILED");
    }

    #[test]
    fn the_kept_sentence_is_the_clis_own_words_and_its_link() {
        // The JSON envelope is the machine's; the line beside it is what a person can read.
        assert_eq!(
            failure_sentence("{\"status\":\"ERROR\"}\nrate limit reached", ""),
            "rate limit reached"
        );
        // A link to sort it out travels with the sentence, however far down it was printed.
        assert_eq!(
            failure_sentence(
                "error: verify your account\nnoise\nhttps://accounts.google.com/verify?x=1",
                "",
            ),
            "error: verify your account https://accounts.google.com/verify?x=1"
        );
        assert_eq!(failure_sentence("", ""), "");
    }
}

/// Maps a finished process to text or a stable error code.
pub(crate) fn finish(status: ExitStatus, stdout: &str, stderr: &str) -> Result<(), String> {
    if status.success() {
        return Ok(());
    }
    note_failure(stdout, stderr);
    if looks_unpaid(stderr) || looks_unpaid(stdout) {
        return Err("E_RUNNER_NO_PLAN".into());
    }
    if looks_ineligible(stderr) || looks_ineligible(stdout) {
        return Err("E_RUNNER_INELIGIBLE".into());
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

    /// Live, one backend (`STACKER_LIVE_BACKEND`, agy when unset), to watch for console
    /// windows: `cargo test --lib live_one_backend -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_one_backend() {
        let backend = std::env::var("STACKER_LIVE_BACKEND").unwrap_or_else(|_| "agy".into());
        let req = RunRequest {
            backend: backend.clone(),
            model: None,
            effort: Some("low".into()),
            prompt: "Reply with one number only: 2+3=?".into(),
            timeout: DEFAULT_TIMEOUT,
            attachments: Vec::new(),
            on_delta: None,
        };
        let out = run(&req, &CancelFlag::default());
        println!("{backend} -> {out:?}");
        assert_eq!(out.unwrap().text, "5");
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
