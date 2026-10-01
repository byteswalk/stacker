//! CodeBuddy and Qoder: Claude-style print mode with their own output, sign-in and model list.
use super::backends::Ctx;
use super::login::LoginStatus;
use super::options::ModelOption;
use super::{run_program, CancelFlag};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CODEBUDDY: &[&str] = &["codebuddy.exe", "codebuddy.cmd", "codebuddy.bat"];
const QODER: &[&str] = &["qodercli.exe", "qodercli.cmd", "qodercli.bat"];
/// Qoder's China edition is a separate npm package (`@qodercn-ai/qoderclicn`) with its own
/// command and its own account; the arguments and output are the same.
const QODER_CN: &[&str] = &["qoderclicn.exe", "qoderclicn.cmd", "qoderclicn.bat"];
const CODEBUDDY_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];
/// `auto`, `none` and `ultracode` are left out: they are not plain reasoning levels.
const QODER_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
/// Asks CodeBuddy for a model that cannot exist; it refuses before any tokens are spent
/// and lists the account's models.
const NO_SUCH_MODEL: &str = "stacker-list-models";
const MODEL_CACHE: Duration = Duration::from_secs(30 * 60);

fn command(candidates: &[&str]) -> Result<Command, String> {
    crate::agents::process::resolve_command(candidates)
        .map(|path| crate::agents::process::program_command(&path))
        .ok_or_else(|| "E_RUNNER_MISSING".into())
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The `type: "result"` object, whether printed alone or as the last item of a transcript array.
fn result_object(stdout: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(stdout.trim()).ok().or_else(|| {
        stdout
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str(l.trim()).ok())
    })?;
    match value {
        Value::Array(items) => items
            .into_iter()
            .rev()
            .find(|v| v.get("type").and_then(Value::as_str) == Some("result")),
        v => Some(v),
    }
}

/// Reads the answer; both CLIs may exit oddly, so a clean result wins over the exit code.
fn answer(stdout: &str, stderr: &str) -> Result<String, String> {
    match result_object(stdout) {
        Some(v) if v.get("is_error").and_then(Value::as_bool) != Some(true) => v
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "E_RUNNER_EMPTY".into()),
        Some(v) => Err(failure(
            v.get("result").and_then(Value::as_str).unwrap_or(""),
            stderr,
        )),
        None => Err(failure(stdout, stderr)),
    }
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

fn cached(
    cell: &Mutex<Option<(Instant, Vec<ModelOption>)>>,
    load: impl FnOnce() -> Vec<ModelOption>,
) -> Vec<ModelOption> {
    if let Ok(guard) = cell.lock() {
        if let Some((at, models)) = guard.as_ref() {
            if at.elapsed() < MODEL_CACHE && !models.is_empty() {
                return models.clone();
            }
        }
    }
    let models = load();
    if let Ok(mut guard) = cell.lock() {
        *guard = Some((Instant::now(), models.clone()));
    }
    models
}

fn options(ids: Vec<String>, efforts: &[&str]) -> Vec<ModelOption> {
    ids.into_iter()
        .map(|id| ModelOption {
            label: id.clone(),
            id,
            efforts: strings(efforts),
            default_effort: None,
        })
        .collect()
}

fn scratch() -> Option<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix("stacker-run-")
        .tempdir()
        .ok()
}

// ---- CodeBuddy -------------------------------------------------------------------------

/// CodeBuddy takes Claude's print-mode flags as they are.
pub fn codebuddy_args(model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    super::claude::claude_args(model, effort)
}

pub fn run_codebuddy(c: &Ctx) -> Result<String, String> {
    let mut cmd = command(CODEBUDDY)?;
    cmd.args(codebuddy_args(c.model, c.effort));
    let run = run_program(cmd, c.prompt, c.tmp, c.timeout, c.cancel);
    remove_codebuddy_logs(c.tmp);
    let (_, stdout, stderr) = run?;
    answer(&stdout, &stderr)
}

/// Its per-run log is named after the working folder: `logs\<date>\<folder>__<hash>.log`.
fn remove_codebuddy_logs(cwd: &Path) {
    let (Some(home), Some(name)) = (dirs::home_dir(), cwd.file_name()) else {
        return;
    };
    let prefix = format!("{}__", name.to_string_lossy());
    let Ok(days) = std::fs::read_dir(home.join(".codebuddy").join("logs")) else {
        return;
    };
    for day in days.flatten() {
        for file in std::fs::read_dir(day.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if file.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(file.path());
            }
        }
    }
}

/// CodeBuddy has no sign-in command and its token lives in its settings, which are not read.
pub fn codebuddy_login() -> LoginStatus {
    LoginStatus {
        state: "unknown".into(),
        method: String::new(),
        account: String::new(),
    }
}

/// The `  - id` lines under "Currently supported models for your account:".
pub fn parse_codebuddy_models(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .skip_while(|l| !l.to_lowercase().contains("supported models"))
        .skip(1)
        .map_while(|l| l.trim().strip_prefix("- ").map(|m| m.trim().to_string()))
        .filter(|m| !m.is_empty())
        .collect()
}

static CODEBUDDY_MODELS: Mutex<Option<(Instant, Vec<ModelOption>)>> = Mutex::new(None);

pub fn codebuddy_models() -> Vec<ModelOption> {
    cached(&CODEBUDDY_MODELS, || {
        let (Ok(mut cmd), Some(tmp)) = (command(CODEBUDDY), scratch()) else {
            return Vec::new();
        };
        cmd.args(codebuddy_args(Some(NO_SUCH_MODEL), None));
        let out = run_program(
            cmd,
            "OK",
            tmp.path(),
            Duration::from_secs(60),
            &CancelFlag::default(),
        );
        remove_codebuddy_logs(tmp.path());
        let ids = out
            .map(|(_, _, err)| parse_codebuddy_models(&err))
            .unwrap_or_default();
        options(ids, CODEBUDDY_EFFORTS)
    })
}

pub fn codebuddy_efforts() -> Vec<String> {
    strings(CODEBUDDY_EFFORTS)
}

// ---- Qoder -----------------------------------------------------------------------------

/// Plugins and hooks would start shell commands on every prompt; they are all switched off.
const QODER_SETTINGS: &str = r#"{"disableAllHooks":true,"enabledPlugins":{}}"#;

pub fn qoder_args(settings: &Path, model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut args = strings(&[
        "-p",
        "--no-session-persistence",
        "--tools",
        "",
        "--strict-mcp-config",
        "-o",
        "json",
        "--settings",
    ]);
    args.push(settings.to_string_lossy().into_owned());
    if let Some(model) = model {
        args.push("-m".into());
        args.push(model.into());
    }
    if let Some(effort) = effort {
        args.push("--reasoning-effort".into());
        args.push(effort.into());
    }
    args
}

/// The settings file lives outside the working folder so the run still starts in an empty one.
fn qoder_settings() -> Result<tempfile::NamedTempFile, String> {
    let mut file = tempfile::Builder::new()
        .prefix("stacker-qoder-")
        .suffix(".json")
        .tempfile()
        .map_err(|_| "E_STORAGE".to_string())?;
    std::io::Write::write_all(&mut file, QODER_SETTINGS.as_bytes())
        .map_err(|_| "E_STORAGE".to_string())?;
    Ok(file)
}

pub fn run_qoder(c: &Ctx) -> Result<String, String> {
    run_qoder_with(QODER, c)
}

pub fn run_qoder_cn(c: &Ctx) -> Result<String, String> {
    run_qoder_with(QODER_CN, c)
}

fn run_qoder_with(candidates: &[&str], c: &Ctx) -> Result<String, String> {
    let settings = qoder_settings()?;
    let mut cmd = command(candidates)?;
    cmd.args(qoder_args(settings.path(), c.model, c.effort));
    let run = run_program(cmd, c.prompt, c.tmp, c.timeout, c.cancel);
    remove_qoder_log(c.tmp);
    let (_, stdout, stderr) = run?;
    answer(&stdout, &stderr)
}

/// Qoder names a folder's log by replacing `:`, `\`, `/` and `.` with `-`.
pub fn qoder_slug(dir: &Path) -> String {
    dir.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .chars()
        .map(|c| {
            if matches!(c, ':' | '\\' | '/' | '.') {
                '-'
            } else {
                c
            }
        })
        .collect()
}

/// Removes the event log Qoder keeps for the run's working folder (it holds the prompt).
fn remove_qoder_log(cwd: &Path) {
    let Some(home) = dirs::home_dir() else { return };
    let root: PathBuf = home.join(".qoder").join("logs").join("sessions");
    let mut dirs = vec![cwd.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(cwd) {
        dirs.push(real);
    }
    for dir in dirs {
        let slug = qoder_slug(&dir);
        if !slug.is_empty() && slug.contains("stacker-run-") {
            let _ = std::fs::remove_dir_all(root.join(slug));
        }
    }
}

/// `qodercli status -o json`; only `logged_in` and `login_method` are kept, never the account.
pub fn parse_qoder_login(text: &str) -> LoginStatus {
    let value = serde_json::from_str::<Value>(text.trim())
        .ok()
        .or_else(|| {
            text.lines()
                .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        })
        .unwrap_or(Value::Null);
    let method = value
        .get("login_method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let state = match value.get("logged_in").and_then(Value::as_bool) {
        Some(true) => "logged_in",
        Some(false) => "logged_out",
        None => "unknown",
    };
    // The e-mail says which account it is; the name alone often does not.
    let account = ["email", "username"]
        .iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or("")
        .to_string();
    let signed_in = state == "logged_in";
    LoginStatus {
        state: state.into(),
        method: if signed_in { method } else { String::new() },
        account: if signed_in { account } else { String::new() },
    }
}

pub fn qoder_login() -> LoginStatus {
    qoder_login_with(QODER)
}

pub fn qoder_cn_login() -> LoginStatus {
    qoder_login_with(QODER_CN)
}

fn qoder_login_with(candidates: &[&str]) -> LoginStatus {
    let (Ok(mut cmd), Some(tmp)) = (command(candidates), scratch()) else {
        return parse_qoder_login("");
    };
    cmd.args(["status", "-o", "json"]);
    let out = run_program(
        cmd,
        "",
        tmp.path(),
        Duration::from_secs(20),
        &CancelFlag::default(),
    );
    remove_qoder_log(tmp.path());
    out.map(|(_, stdout, _)| parse_qoder_login(&stdout))
        .unwrap_or_else(|_| parse_qoder_login(""))
}

/// `qodercli --list-models` prints a `MODEL` header, then one name per line.
pub fn parse_qoder_models(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .skip_while(|l| !l.eq_ignore_ascii_case("model") && !l.to_lowercase().starts_with("model "))
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

static QODER_MODELS: Mutex<Option<(Instant, Vec<ModelOption>)>> = Mutex::new(None);
static QODER_CN_MODELS: Mutex<Option<(Instant, Vec<ModelOption>)>> = Mutex::new(None);

pub fn qoder_models() -> Vec<ModelOption> {
    qoder_models_with(&QODER_MODELS, QODER)
}

/// The two editions list different models, so each keeps its own cache.
pub fn qoder_cn_models() -> Vec<ModelOption> {
    qoder_models_with(&QODER_CN_MODELS, QODER_CN)
}

fn qoder_models_with(
    cache: &'static Mutex<Option<(Instant, Vec<ModelOption>)>>,
    candidates: &'static [&'static str],
) -> Vec<ModelOption> {
    cached(cache, || {
        let (Ok(mut cmd), Some(tmp)) = (command(candidates), scratch()) else {
            return Vec::new();
        };
        cmd.arg("--list-models");
        let out = run_program(
            cmd,
            "",
            tmp.path(),
            Duration::from_secs(60),
            &CancelFlag::default(),
        );
        remove_qoder_log(tmp.path());
        let ids = out
            .map(|(_, stdout, _)| parse_qoder_models(&stdout))
            .unwrap_or_default();
        options(ids, QODER_EFFORTS)
    })
}

pub fn qoder_efforts() -> Vec<String> {
    strings(QODER_EFFORTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codebuddy_transcript_array_yields_the_result() {
        let out = r#"[{"type":"user"},{"type":"assistant"},{"type":"result","is_error":false,"result":"OK"}]"#;
        assert_eq!(answer(out, "").unwrap(), "OK");
        assert_eq!(
            answer("", "400 model [x] service info not found").unwrap_err(),
            "E_RUNNER_FAILED"
        );
        let bad = r#"[{"type":"result","is_error":true,"result":"Please login first"}]"#;
        assert_eq!(answer(bad, "").unwrap_err(), "E_RUNNER_AUTH");
    }

    #[test]
    fn codebuddy_models_come_from_the_refusal() {
        let err = "400 model [x] service info not found (a/b)\nCurrently supported models for your account:\n  - hy3\n  - glm-5.3\nPlease use --model <model_id> to specify a valid model.\n";
        assert_eq!(parse_codebuddy_models(err), vec!["hy3", "glm-5.3"]);
        assert!(parse_codebuddy_models("").is_empty());
    }

    #[test]
    fn qoder_args_status_and_models() {
        let args = qoder_args(Path::new("s.json"), Some("Auto"), Some("low"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--tools" && w[1].is_empty()));
        assert!(args.contains(&"--no-session-persistence".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--reasoning-effort" && w[1] == "low"));

        let status = r#"{"logged_in":true,"login_method":"browser","username":"someone","email":"someone@example.com"}"#;
        let s = parse_qoder_login(status);
        assert_eq!(
            (s.state.as_str(), s.method.as_str()),
            ("logged_in", "browser")
        );
        // The account is named, by e-mail: two editions on two accounts look alike otherwise.
        assert_eq!(s.account, "someone@example.com");
        assert_eq!(
            parse_qoder_login(r#"{"logged_in":false,"email":"someone@example.com"}"#).account,
            ""
        );
        assert_eq!(
            parse_qoder_login(r#"{"logged_in":false}"#).state,
            "logged_out"
        );
        assert_eq!(parse_qoder_login("").state, "unknown");
        let pretty = "{
  \"logged_in\": true,
  \"login_method\": \"browser\"
}
";
        assert_eq!(parse_qoder_login(pretty).state, "logged_in");

        assert_eq!(
            parse_qoder_models("MODEL\nAuto\nQwen3.8-Max\nDeepSeek-Flash\n"),
            vec!["Auto", "Qwen3.8-Max", "DeepSeek-Flash"]
        );
        assert_eq!(
            qoder_slug(Path::new(r"C:\Users\a\Temp\stacker-run-x.y")),
            "C--Users-a-Temp-stacker-run-x-y"
        );
    }
}
