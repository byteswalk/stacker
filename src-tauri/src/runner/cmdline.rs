//! Antigravity (agy), Kimi Code and MiMo Code: CLIs that take the prompt as an argument
//! instead of on stdin, each made tool-less and session-free its own way.
use super::backends::Ctx;
use super::login::LoginStatus;
use super::options::ModelOption;
use super::{run_program, CancelFlag};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Windows limits a command line to 32 767 characters; leave room for the other arguments.
const MAX_PROMPT_CHARS: usize = 30_000;

fn check_prompt(prompt: &str) -> Result<(), String> {
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        Err("E_PROMPT_TOO_LONG".into())
    } else {
        Ok(())
    }
}

fn status(state: &str, method: &str) -> LoginStatus {
    LoginStatus {
        state: state.into(),
        method: method.into(),
    }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn scratch() -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("stacker-run-")
        .tempdir()
        .map_err(|_| "E_STORAGE".to_string())
}

fn quick(cmd: Command, cwd: &Path) -> Option<(bool, String, String)> {
    run_program(
        cmd,
        "",
        cwd,
        Duration::from_secs(60),
        &CancelFlag::default(),
    )
    .ok()
    .map(|(status, out, err)| (status.success(), out, err))
}

fn failure(stdout: &str, stderr: &str) -> String {
    #[cfg(windows)]
    let status = {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(1)
    };
    super::finish(status, stdout, stderr)
        .err()
        .unwrap_or_else(|| "E_RUNNER_FAILED".into())
}

// ---- Antigravity -----------------------------------------------------------------------

const AGY_EFFORTS: &[&str] = &["low", "medium", "high"];
/// Headless agy still reads files anywhere and searches the web, and the user's own config
/// pre-allows commands. So each run gets a throwaway home with everything denied and a hook
/// that refuses every tool call; sign-in lives outside the home folder and keeps working.
const AGY_SETTINGS: &str = r#"{"permissions":{"deny":["command(*)","mcp(*)","read_file(*)","read_url(*)","write_file(*)"]}}"#;
const AGY_HOOKS: &str = r#"{"deny-all-tools":{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"echo {\"decision\":\"deny\",\"reason\":\"tools disabled\"}","timeout":5}]}]}}"#;

fn agy_command(home: &Path) -> Result<Command, String> {
    let program =
        crate::agents::process::resolve_command(&["agy.exe"]).ok_or("E_RUNNER_MISSING")?;
    let gemini = home.join(".gemini");
    let write = |path: PathBuf, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap_or(home))
            .and_then(|_| std::fs::write(&path, text))
            .map_err(|_| "E_STORAGE".to_string())
    };
    write(
        gemini.join("antigravity-cli").join("settings.json"),
        AGY_SETTINGS,
    )?;
    write(gemini.join("config").join("hooks.json"), AGY_HOOKS)?;
    let mut cmd = Command::new(program);
    cmd.env("USERPROFILE", home)
        .env("HOME", home)
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "1");
    Ok(cmd)
}

pub fn agy_args(model: Option<&str>, effort: Option<&str>, prompt: &str) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(model) = model {
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(effort) = effort {
        args.push("--effort".into());
        args.push(effort.into());
    }
    args.push("--output-format".into());
    args.push("json".into());
    // `-p <prompt>` would read a following flag as the prompt; the `=` form cannot.
    args.push(format!("--print={prompt}"));
    args
}

/// One JSON object: `status` SUCCESS with `response`, or ERROR with `error`. A success with
/// an empty response means a tool was refused and nothing was answered.
pub fn parse_agy(stdout: &str, stderr: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(stdout.trim())
        .ok()
        .or_else(|| {
            stdout
                .lines()
                .rev()
                .find_map(|l| serde_json::from_str(l.trim()).ok())
        })
        .ok_or_else(|| failure(stdout, stderr))?;
    let text = value
        .get("response")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if value.get("status").and_then(Value::as_str) == Some("SUCCESS") && !text.is_empty() {
        return Ok(text.to_string());
    }
    let error = value.get("error").and_then(Value::as_str).unwrap_or("");
    Err(failure(error, stderr))
}

pub fn run_agy(c: &Ctx) -> Result<String, String> {
    check_prompt(c.prompt)?;
    let home = scratch()?;
    let mut cmd = agy_command(home.path())?;
    cmd.args(agy_args(c.model, c.effort, c.prompt));
    let (_, stdout, stderr) = run_program(cmd, "", c.tmp, c.timeout, c.cancel)?;
    parse_agy(&stdout, &stderr)
}

/// `agy models`: "Fetching available models..." then `id<TAB>name` lines.
pub fn parse_agy_models(stdout: &str) -> Vec<ModelOption> {
    stdout
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, name)| ModelOption {
            id: id.trim().to_string(),
            label: name.trim().to_string(),
            efforts: strings(AGY_EFFORTS),
            default_effort: None,
        })
        .filter(|m| !m.id.is_empty())
        .collect()
}

type Listing = Option<(bool, String, String)>;
static AGY_MODELS: Mutex<Option<(Instant, Listing)>> = Mutex::new(None);
const AGY_CACHE: Duration = Duration::from_secs(30 * 60);

/// `agy models` takes several seconds; sign-in and the model list share one cached call.
fn agy_models_output() -> Listing {
    if let Ok(guard) = AGY_MODELS.lock() {
        if let Some((at, Some(listing))) = guard.as_ref() {
            if at.elapsed() < AGY_CACHE && listing.0 {
                return Some(listing.clone());
            }
        }
    }
    let listing = (|| {
        let home = scratch().ok()?;
        let cwd = scratch().ok()?;
        let mut cmd = agy_command(home.path()).ok()?;
        cmd.arg("models");
        quick(cmd, cwd.path())
    })();
    if let Ok(mut guard) = AGY_MODELS.lock() {
        *guard = Some((Instant::now(), listing.clone()));
    }
    listing
}

/// agy has no sign-in command; listing models works only when signed in.
pub fn agy_login() -> LoginStatus {
    match agy_models_output() {
        Some((true, out, _)) if !parse_agy_models(&out).is_empty() => status("logged_in", ""),
        Some((_, out, err)) if super::looks_unauthenticated(&format!("{out}{err}")) => {
            status("logged_out", "")
        }
        _ => status("unknown", ""),
    }
}

pub fn agy_models() -> Vec<ModelOption> {
    agy_models_output()
        .filter(|(ok, _, _)| *ok)
        .map(|(_, out, _)| parse_agy_models(&out))
        .unwrap_or_default()
}

pub fn agy_efforts() -> Vec<String> {
    strings(AGY_EFFORTS)
}

// ---- Kimi Code ---------------------------------------------------------------------------

/// An agent definition with no tools: the gateway answers questions, it does not touch the
/// user's machine.
const KIMI_AGENT: &str = "---\nname: stacker-answer\ndescription: Answer without tools.\ntools: []\n---\nAnswer the question directly. Never use tools.\n";

fn kimi_command() -> Result<Command, String> {
    crate::agents::process::resolve_command(&["kimi.exe", "kimi.cmd", "kimi.bat"])
        .map(Command::new)
        .ok_or_else(|| "E_RUNNER_MISSING".into())
}

pub fn kimi_args(agent_file: &Path, model: Option<&str>, prompt: &str) -> Vec<String> {
    let mut args = vec![
        "--agent-file".to_string(),
        agent_file.to_string_lossy().into_owned(),
        "--output-format".to_string(),
        "text".to_string(),
    ];
    if let Some(model) = model {
        args.push("-m".into());
        args.push(model.into());
    }
    // `--prompt=` keeps a prompt that starts with `-` from being read as an option.
    args.push(format!("--prompt={prompt}"));
    args
}

/// Kimi files each run under a folder named after the working directory; ours is a fresh
/// scratch folder, so that folder is the run's own and goes away with it.
fn remove_kimi_workspace(work: &Path) {
    let Some(name) = work.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let Some(sessions) = dirs::home_dir().map(|h| h.join(".kimi-code").join("sessions")) else {
        return;
    };
    let prefix = format!("wd_{name}_");
    if let Ok(entries) = std::fs::read_dir(&sessions) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(prefix.as_str())
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

pub fn run_kimi(c: &Ctx) -> Result<String, String> {
    check_prompt(c.prompt)?;
    let work = scratch()?;
    let agent_file = work.path().join("stacker-answer.md");
    std::fs::write(&agent_file, KIMI_AGENT).map_err(|_| "E_STORAGE".to_string())?;
    let mut cmd = kimi_command()?;
    cmd.args(kimi_args(&agent_file, c.model, c.prompt));
    let run = run_program(cmd, "", c.tmp, c.timeout, c.cancel);
    remove_kimi_workspace(c.tmp);
    let (status, stdout, stderr) = run?;
    if !status.success() || stdout.trim().is_empty() {
        return Err(failure(&stdout, &stderr));
    }
    Ok(stdout.trim().to_string())
}

/// `kimi provider list --json`: providers carry the sign-in, models carry their display names.
pub fn parse_kimi_providers(json: &str) -> (bool, Vec<ModelOption>) {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return (false, Vec::new());
    };
    let signed_in = value
        .get("providers")
        .and_then(Value::as_object)
        .is_some_and(|providers| {
            providers.values().any(|p| {
                p.get("oauth").is_some()
                    || p.get("apiKey")
                        .and_then(Value::as_str)
                        .is_some_and(|k| !k.is_empty())
            })
        });
    let models = value
        .get("models")
        .and_then(Value::as_object)
        .map(|models| {
            models
                .iter()
                .map(|(id, m)| ModelOption {
                    id: id.clone(),
                    label: m
                        .get("displayName")
                        .and_then(Value::as_str)
                        .map(|name| format!("{name} ({id})"))
                        .unwrap_or_else(|| id.clone()),
                    efforts: Vec::new(),
                    default_effort: None,
                })
                .collect()
        })
        .unwrap_or_default();
    (signed_in, models)
}

fn kimi_providers() -> Option<(bool, Vec<ModelOption>)> {
    let mut cmd = kimi_command().ok()?;
    cmd.args(["provider", "list", "--json"]);
    let cwd = scratch().ok()?;
    let (ok, out, _) = quick(cmd, cwd.path())?;
    ok.then(|| parse_kimi_providers(&out))
}

pub fn kimi_login() -> LoginStatus {
    match kimi_providers() {
        Some((true, _)) => status("logged_in", ""),
        Some((false, _)) => status("logged_out", ""),
        None => status("unknown", ""),
    }
}

pub fn kimi_models() -> Vec<ModelOption> {
    kimi_providers()
        .map(|(_, models)| models)
        .unwrap_or_default()
}

/// Kimi picks the reasoning depth itself; there is no level to set per request.
pub fn kimi_efforts() -> Vec<String> {
    Vec::new()
}

// ---- MiMo Code ---------------------------------------------------------------------------

/// `--variant` names the reasoning depth; MiMo documents these three.
const MIMO_EFFORTS: &[&str] = &["minimal", "high", "max"];

fn mimo_command() -> Result<Command, String> {
    crate::agents::process::resolve_command(&["mimo.exe", "mimo.cmd", "mimo.bat"])
        .map(Command::new)
        .ok_or_else(|| "E_RUNNER_MISSING".into())
}

pub fn mimo_args(
    work: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--format".to_string(),
        "json".to_string(),
        // No external plugins, so the run cannot reach tools the user installed.
        "--pure".to_string(),
        "--dir".to_string(),
        work.to_string_lossy().into_owned(),
    ];
    if let Some(model) = model {
        args.push("-m".into());
        args.push(model.into());
    }
    if let Some(effort) = effort {
        args.push("--variant".into());
        args.push(effort.into());
    }
    args.push("--".to_string());
    args.push(prompt.to_string());
    args
}

/// The JSON event stream: `text` parts make up the answer, and every event carries the
/// session id, which is how the run cleans up after itself.
pub fn parse_mimo_events(stdout: &str) -> (String, Option<String>, Option<String>) {
    let mut answer = String::new();
    let mut session = None;
    let mut error = None;
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if session.is_none() {
            session = value
                .get("sessionID")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        match value.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = value
                    .get("part")
                    .and_then(|p| p.get("text"))
                    .and_then(Value::as_str)
                {
                    answer.push_str(text);
                }
            }
            Some("error") => {
                error = value
                    .get("error")
                    .and_then(|e| e.get("data"))
                    .and_then(|d| d.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        value
                            .get("error")
                            .and_then(|e| e.get("name"))
                            .and_then(Value::as_str)
                    })
                    .map(str::to_string)
                    .or(Some("E_RUNNER_FAILED".into()));
            }
            _ => {}
        }
    }
    (answer.trim().to_string(), session, error)
}

/// Removes the session this run created, so the gateway leaves nothing in the user's list.
fn remove_mimo_session(id: &str) {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return;
    }
    let Ok(mut cmd) = mimo_command() else { return };
    cmd.args(["session", "delete", id]);
    if let Ok(cwd) = scratch() {
        let _ = quick(cmd, cwd.path());
    }
}

pub fn run_mimo(c: &Ctx) -> Result<String, String> {
    check_prompt(c.prompt)?;
    let work = scratch()?;
    let mut cmd = mimo_command()?;
    cmd.args(mimo_args(work.path(), c.model, c.effort, c.prompt));
    let run = run_program(cmd, "", c.tmp, c.timeout, c.cancel);
    let (_, stdout, stderr) = run?;
    let (answer, session, error) = parse_mimo_events(&stdout);
    if let Some(id) = session {
        remove_mimo_session(&id);
    }
    match (answer.is_empty(), error) {
        (true, Some(message)) => Err(failure(&message, &stderr)),
        (true, None) => Err(failure(&stdout, &stderr)),
        _ => Ok(answer),
    }
}

/// `mimo auth whoami` says so in as many words when nobody is signed in.
pub fn parse_mimo_whoami(output: &str) -> &'static str {
    let text = output.to_lowercase();
    if text.contains("not logged in") {
        "logged_out"
    } else if text.contains("mimo auth login") || text.trim().is_empty() {
        "unknown"
    } else {
        "logged_in"
    }
}

pub fn mimo_login() -> LoginStatus {
    let Ok(mut cmd) = mimo_command() else {
        return status("unknown", "");
    };
    cmd.args(["auth", "whoami"]);
    let Ok(cwd) = scratch() else {
        return status("unknown", "");
    };
    match quick(cmd, cwd.path()) {
        Some((_, out, err)) => status(parse_mimo_whoami(&format!("{out}{err}")), ""),
        None => status("unknown", ""),
    }
}

/// `mimo models` prints `provider/model — window 1M, compacts at 900K` per line.
pub fn parse_mimo_models(stdout: &str) -> Vec<ModelOption> {
    stdout
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let id = line.split('—').next()?.trim();
            (id.contains('/') && !id.contains(' ')).then(|| ModelOption {
                id: id.to_string(),
                label: id.to_string(),
                efforts: strings(MIMO_EFFORTS),
                default_effort: None,
            })
        })
        .collect()
}

static MIMO_MODELS: Mutex<Option<(Instant, Vec<ModelOption>)>> = Mutex::new(None);

pub fn mimo_models() -> Vec<ModelOption> {
    let mut cache = MIMO_MODELS.lock().expect("mimo model cache");
    if let Some((at, models)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(30 * 60) {
            return models.clone();
        }
    }
    let models = (|| {
        let mut cmd = mimo_command().ok()?;
        cmd.arg("models");
        let cwd = scratch().ok()?;
        let (_, out, _) = quick(cmd, cwd.path())?;
        Some(parse_mimo_models(&out))
    })()
    .unwrap_or_default();
    *cache = Some((Instant::now(), models.clone()));
    models
}

pub fn mimo_efforts() -> Vec<String> {
    strings(MIMO_EFFORTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_prompt_is_one_argument_and_empty_answers_fail() {
        let args = agy_args(Some("m"), Some("low"), "-p looks like a flag");
        assert_eq!(args.last().unwrap(), "--print=-p looks like a flag");
        assert_eq!(
            parse_agy(r#"{"status":"SUCCESS","response":"OK\n"}"#, "").unwrap(),
            "OK"
        );
        assert!(parse_agy(
            r#"{"status":"SUCCESS","response":"","denied_actions":[1]}"#,
            ""
        )
        .is_err());
        assert!(parse_agy(r#"{"status":"ERROR","error":"invalid model"}"#, "").is_err());
        let models = parse_agy_models("Fetching available models...\ng-flash\tGemini Flash\n");
        assert_eq!(
            (models[0].id.as_str(), models[0].label.as_str()),
            ("g-flash", "Gemini Flash")
        );
    }

    #[test]
    fn kimi_runs_one_prompt_without_tools() {
        let args = kimi_args(
            Path::new(r"C:\t\a.md"),
            Some("kimi-code/k"),
            "-p looks like a flag",
        );
        assert_eq!(args.last().unwrap(), "--prompt=-p looks like a flag");
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--output-format" && w[1] == "text"));
        assert!(KIMI_AGENT.contains("tools: []"));
        let json = r#"{"providers":{"managed:kimi-code":{"oauth":{"key":"k"}}},"models":{"kimi-code/kimi-for-coding":{"displayName":"K2.8"}}}"#;
        let (signed_in, models) = parse_kimi_providers(json);
        assert!(signed_in);
        assert_eq!(models[0].id, "kimi-code/kimi-for-coding");
        assert_eq!(models[0].label, "K2.8 (kimi-code/kimi-for-coding)");
        let (signed_out, _) =
            parse_kimi_providers(r#"{"providers":{"p":{"apiKey":""}},"models":{}}"#);
        assert!(!signed_out);
    }

    #[test]
    fn mimo_answers_come_from_the_event_stream() {
        let args = mimo_args(
            Path::new(r"C:\t"),
            Some("xiaomi/mimo-v2.6-pro"),
            Some("high"),
            "hi",
        );
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--format" && w[1] == "json"));
        assert!(args.contains(&"--pure".to_string()));
        assert_eq!(args.last().unwrap(), "hi");
        let events = concat!(
            r#"{"type":"message.updated","sessionID":"ses_1"}"#,
            "\n",
            r#"{"type":"text","sessionID":"ses_1","part":{"text":"4"}}"#,
            "\n",
        );
        let (answer, session, error) = parse_mimo_events(events);
        assert_eq!(answer, "4");
        assert_eq!(session.as_deref(), Some("ses_1"));
        assert!(error.is_none());
        let failed = r#"{"type":"error","sessionID":"ses_2","error":{"name":"APIError","data":{"message":"Invalid API Key"}}}"#;
        let (answer, _, error) = parse_mimo_events(failed);
        assert!(answer.is_empty());
        assert_eq!(error.as_deref(), Some("Invalid API Key"));
        assert_eq!(
            parse_mimo_whoami("Not logged in. Run `mimo auth login`"),
            "logged_out"
        );
        assert_eq!(parse_mimo_whoami("user@example.com"), "logged_in");
        let models =
            parse_mimo_models("mimo/mimo-auto — window 1M\nxiaomi/mimo-v2.6-pro — window 1M\n");
        assert_eq!(models.len(), 2);
        assert_eq!(models[1].id, "xiaomi/mimo-v2.6-pro");
    }

    #[test]
    fn long_prompts_are_refused_before_running() {
        assert_eq!(
            check_prompt(&"x".repeat(MAX_PROMPT_CHARS + 1)).unwrap_err(),
            "E_PROMPT_TOO_LONG"
        );
    }
}
