//! Runs the user's signed-in agent CLIs statelessly, in an empty folder, without tools.
//! Prompts and answers are never logged.
pub mod claude;
pub mod codex;
pub mod options;

use crate::sessions::model::Agent;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
pub struct RunRequest {
    pub agent: Agent,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: String,
    pub timeout: Duration,
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
    mut program: Command,
    stdin: &str,
    cwd: &Path,
    timeout: Duration,
    cancel: &CancelFlag,
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
    let mut out = child.stdout.take().ok_or("E_RUNNER_FAILED")?;
    let mut err = child.stderr.take().ok_or("E_RUNNER_FAILED")?;
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
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
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = writer.join();
    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
    Ok((status, stdout, stderr))
}

fn looks_unauthenticated(text: &str) -> bool {
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
fn finish(status: ExitStatus, stdout: &str, stderr: &str) -> Result<(), String> {
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
    let result = match req.agent {
        Agent::Codex => {
            let out = tmp.path().join("out.md");
            let mut cmd = crate::sessions::codex_rpc::command()
                .map_err(|_| "E_RUNNER_MISSING".to_string())?;
            cmd.args(codex::codex_args(
                tmp.path(),
                &out,
                model,
                effort,
                &codex::disabled_features(),
            ));
            let (status, stdout, stderr) =
                run_program(cmd, &req.prompt, tmp.path(), req.timeout, cancel)?;
            finish(status, &stdout, &stderr)?;
            std::fs::read_to_string(&out).map_err(|_| "E_RUNNER_EMPTY".to_string())
        }
        Agent::Claude => {
            let program = crate::agents::process::resolve_command(&["claude.exe", "claude.cmd"])
                .ok_or("E_RUNNER_MISSING")?;
            let mut cmd = Command::new(program);
            cmd.args(claude::claude_args(model, effort));
            let run = run_program(cmd, &req.prompt, tmp.path(), req.timeout, cancel);
            claude::remove_project_leftover(tmp.path());
            let (status, stdout, stderr) = run?;
            finish(status, &stdout, &stderr)?;
            claude::parse_claude(&stdout)
        }
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;
    log::info!(
        target: "stacker::runner",
        "agent={} model={} effort={} elapsed_ms={} result={}",
        req.agent.as_str(),
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
        for (agent, model, effort) in [
            (Agent::Codex, None, Some("low")),
            (Agent::Claude, Some("sonnet"), Some("low")),
        ] {
            let req = RunRequest {
                agent,
                model: model.map(str::to_string),
                effort: effort.map(str::to_string),
                prompt: "Reply with one number only: 2+3=?".into(),
                timeout: DEFAULT_TIMEOUT,
            };
            let out = run(&req, &CancelFlag::default());
            println!("{} -> {:?}", agent.as_str(), out);
            assert_eq!(out.unwrap().text, "5");
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
