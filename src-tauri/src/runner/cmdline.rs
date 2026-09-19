//! Antigravity (agy), DeepSeek Harness (dsh) and Hermes: CLIs that take the prompt as an
//! argument instead of on stdin, each made tool-less and session-free its own way.
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

// ---- DeepSeek Harness ------------------------------------------------------------------

const DSH_MODELS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];
const DSH_EFFORTS: &[&str] = &["off", "low", "high", "max"];
/// Plugins that give the headless profile tools, a titling call, telemetry or project rules.
const DSH_DISABLED: &[&str] = &[
    "tool-pwsh",
    "tool-bash",
    "tool-jobs",
    "tool-fs",
    "tool-fs-search",
    "tool-skill",
    "tool-subagent-control",
    "tool-subagent-list-agents",
    "tool-subagent",
    "tool-subagent-fork",
    "tool-workflow",
    "tool-todo",
    "tool-goal",
    "tool-ralph",
    "tool-web",
    "session-title-llm",
    "session-telemetry-otel",
    "agent-instructions",
    "plan-mode",
    "user-questions",
];

fn yaml_path(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('\\', "/"))
}

/// The `--patch` overlay: tool plugins off, sessions and settings inside the scratch folder.
pub fn dsh_patch(scratch: &Path) -> String {
    let mut out = String::new();
    for id in DSH_DISABLED {
        out.push_str(&format!("- id: {id}\n  disabled: true\n"));
    }
    for (id, key, path) in [
        (
            "session-persistence-jsonl",
            "root",
            scratch.join("sessions"),
        ),
        ("storage-json", "root", scratch.join("storages")),
        ("settings", "path", scratch.join("settings.yaml")),
    ] {
        out.push_str(&format!(
            "- id: {id}\n  config:\n    {key}: {}\n",
            yaml_path(&path)
        ));
    }
    out
}

pub fn dsh_settings(model: Option<&str>, effort: Option<&str>) -> String {
    let model = model.unwrap_or(DSH_MODELS[0]);
    let mut out = format!(
        "agent-default-model:\n  provider: deepseek-official\n  model: \"{}\"\n",
        model.replace('"', "")
    );
    if let Some(effort) = effort {
        out.push_str(&format!(
            "  reasoningEffort: \"{}\"\n",
            effort.replace('"', "")
        ));
    }
    out
}

/// npm installs dsh as a `.cmd` shim, and cmd.exe cannot carry a multi-line prompt safely,
/// so its script is started with node directly.
fn dsh_command() -> Result<Command, String> {
    let shim = crate::agents::process::resolve_command(&["dsh.cmd"]).ok_or("E_RUNNER_MISSING")?;
    let dir = shim.parent().ok_or("E_RUNNER_MISSING")?;
    let script = dir
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js");
    if !script.is_file() {
        return Err("E_RUNNER_MISSING".into());
    }
    let node = Some(dir.join("node.exe"))
        .filter(|p| p.is_file())
        .or_else(|| crate::agents::process::resolve_command(&["node.exe"]))
        .ok_or("E_RUNNER_MISSING")?;
    let mut cmd = Command::new(node);
    cmd.arg(script).env("DSH_TELEMETRY_DISABLED", "1");
    Ok(cmd)
}

pub fn run_dsh(c: &Ctx) -> Result<String, String> {
    check_prompt(c.prompt)?;
    let work = scratch()?;
    let patch = work.path().join("patch.yml");
    std::fs::write(&patch, dsh_patch(work.path())).map_err(|_| "E_STORAGE".to_string())?;
    std::fs::write(
        work.path().join("settings.yaml"),
        dsh_settings(c.model, c.effort),
    )
    .map_err(|_| "E_STORAGE".to_string())?;
    let mut cmd = dsh_command()?;
    cmd.args(["--profile", "headless", "--patch"]).arg(&patch);
    // A prompt starting with `-` would be read as an option.
    cmd.arg(if c.prompt.starts_with('-') {
        format!(" {}", c.prompt)
    } else {
        c.prompt.to_string()
    });
    let (status, stdout, stderr) = run_program(cmd, "", c.tmp, c.timeout, c.cancel)?;
    if !status.success() || stdout.trim().is_empty() {
        return Err(failure(&stdout, &stderr));
    }
    Ok(stdout)
}

/// dsh keeps its key with its credentials, which Stacker does not read; a test run tells.
pub fn dsh_login() -> LoginStatus {
    status("unknown", "")
}

pub fn dsh_models() -> Vec<ModelOption> {
    DSH_MODELS
        .iter()
        .map(|m| ModelOption {
            id: m.to_string(),
            label: m.to_string(),
            efforts: strings(DSH_EFFORTS),
            default_effort: None,
        })
        .collect()
}

pub fn dsh_efforts() -> Vec<String> {
    strings(DSH_EFFORTS)
}

// ---- Hermes ----------------------------------------------------------------------------

fn hermes_home() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("hermes"))
}

fn hermes_command() -> Result<Command, String> {
    crate::agents::process::resolve_command(&["hermes.exe"])
        .map(Command::new)
        .ok_or_else(|| "E_RUNNER_MISSING".into())
}

/// `-t context_engine` is a toolset with no tools in it; `--source tool` keeps the run out of
/// the user's session list, and the session is deleted afterwards anyway.
pub fn hermes_args(model: Option<&str>, prompt: &str) -> Vec<String> {
    let mut args = strings(&[
        "chat",
        "-Q",
        "--source",
        "tool",
        "-t",
        "context_engine",
        "--ignore-rules",
    ]);
    if let Some(model) = model {
        match model.split_once('/') {
            Some((provider, name)) => args.extend([
                "--provider".into(),
                provider.into(),
                "-m".into(),
                name.into(),
            ]),
            None => args.extend(["-m".into(), model.into()]),
        }
    }
    args.push(format!("--query={prompt}"));
    args
}

pub fn hermes_session_id(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("session_id:"))
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn remove_hermes_session(id: &str) {
    if let Ok(mut cmd) = hermes_command() {
        cmd.args(["sessions", "delete", "-y", id]);
        if let Ok(cwd) = scratch() {
            let _ = quick(cmd, cwd.path());
        }
    }
    let Some(dir) = hermes_home().map(|h| h.join("sessions")) else {
        return;
    };
    let prefix = format!("request_dump_{id}_");
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn run_hermes(c: &Ctx) -> Result<String, String> {
    check_prompt(c.prompt)?;
    let mut cmd = hermes_command()?;
    cmd.args(hermes_args(c.model, c.prompt));
    let (status, stdout, stderr) = run_program(cmd, "", c.tmp, c.timeout, c.cancel)?;
    if let Some(id) = hermes_session_id(&stderr) {
        remove_hermes_session(&id);
    }
    // Errors are printed on stdout too, so only a clean exit counts.
    if !status.success() || stdout.trim().is_empty() {
        return Err(failure(&stdout, &stderr));
    }
    Ok(stdout)
}

/// `hermes auth list`: `<provider> (N credentials):` headers; only provider names are kept.
pub fn parse_hermes_providers(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with(' '))
        .filter_map(|l| {
            let (name, rest) = l.trim().split_once(" (")?;
            let count: u32 = rest.split_whitespace().next()?.parse().ok()?;
            (count > 0 && rest.contains("credential")).then(|| name.to_string())
        })
        .collect()
}

fn hermes_providers() -> Option<Vec<String>> {
    let mut cmd = hermes_command().ok()?;
    cmd.args(["auth", "list"]);
    let cwd = scratch().ok()?;
    quick(cmd, cwd.path()).map(|(_, out, _)| parse_hermes_providers(&out))
}

pub fn hermes_login() -> LoginStatus {
    match hermes_providers() {
        Some(p) if !p.is_empty() => status("logged_in", &p.join(", ")),
        Some(_) => status("logged_out", ""),
        None => status("unknown", ""),
    }
}

/// Models from Hermes's own cache, for signed-in providers, as `provider/model`.
pub fn parse_hermes_models(cache: &str, providers: &[String]) -> Vec<ModelOption> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(cache) else {
        return Vec::new();
    };
    providers
        .iter()
        .filter_map(|p| Some((p, map.get(p)?.get("models")?.as_array()?)))
        .flat_map(|(p, models)| {
            models
                .iter()
                .filter_map(Value::as_str)
                .map(move |m| ModelOption {
                    id: format!("{p}/{m}"),
                    label: format!("{m} ({p})"),
                    efforts: Vec::new(),
                    default_effort: None,
                })
        })
        .collect()
}

pub fn hermes_models() -> Vec<ModelOption> {
    let providers = hermes_providers().unwrap_or_default();
    hermes_home()
        .and_then(|h| std::fs::read_to_string(h.join("provider_models_cache.json")).ok())
        .map(|text| parse_hermes_models(&text, &providers))
        .unwrap_or_default()
}

/// Hermes sets reasoning effort only in its config file, not per run.
pub fn hermes_efforts() -> Vec<String> {
    Vec::new()
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
    fn dsh_patch_turns_tools_off_and_keeps_state_in_scratch() {
        let patch = dsh_patch(Path::new(r"C:\t\s"));
        assert!(patch.contains("- id: tool-bash\n  disabled: true\n"));
        assert!(patch.contains("- id: user-questions\n  disabled: true\n"));
        assert!(patch.contains("root: \"C:/t/s/sessions\""));
        let settings = dsh_settings(None, Some("low"));
        assert!(settings.contains("model: \"deepseek-v4-flash\""));
        assert!(settings.contains("reasoningEffort: \"low\""));
        assert!(!dsh_settings(Some("deepseek-v4-pro"), None).contains("reasoningEffort"));
    }

    #[test]
    fn hermes_args_session_and_auth() {
        let args = hermes_args(Some("openai-codex/gpt-5.5"), "hi");
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--provider" && w[1] == "openai-codex"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-t" && w[1] == "context_engine"));
        assert_eq!(args.last().unwrap(), "--query=hi");
        assert_eq!(
            hermes_session_id("warn\nsession_id: 20260918_230047_826800\n").as_deref(),
            Some("20260918_230047_826800")
        );
        assert_eq!(hermes_session_id("session_id: ../x"), None);
        let auth = "copilot (1 credentials):\n  #1  token  api_key gh_cli\n\nopenai-codex (0 credentials):\n";
        assert_eq!(parse_hermes_providers(auth), vec!["copilot"]);
        let cache = r#"{"copilot":{"models":["gpt-5.4"]},"other":{"models":["x"]}}"#;
        let models = parse_hermes_models(cache, &["copilot".to_string()]);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "copilot/gpt-5.4");
    }

    #[test]
    fn long_prompts_are_refused_before_running() {
        assert_eq!(
            check_prompt(&"x".repeat(MAX_PROMPT_CHARS + 1)).unwrap_err(),
            "E_PROMPT_TOO_LONG"
        );
    }
}
