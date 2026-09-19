//! Every agent CLI the runner can drive, and how: invocation, answer parsing, sign-in check,
//! and model list. Each entry is verified to run without saving a session and without tools.
use super::login::LoginStatus;
use super::options::ModelOption;
use super::{finish, run_program, CancelFlag};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// What one run needs; `tmp` is a fresh empty folder used as the working directory.
pub struct Ctx<'a> {
    pub tmp: &'a Path,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub prompt: &'a str,
    pub timeout: Duration,
    pub cancel: &'a CancelFlag,
}

pub struct Backend {
    /// Same id as the agent registry's CLI id.
    pub id: &'static str,
    pub run: fn(&Ctx) -> Result<String, String>,
    pub login: fn() -> LoginStatus,
    pub models: fn() -> Vec<ModelOption>,
    /// Reasoning levels for a request that names no model.
    pub efforts: fn() -> Vec<String>,
}

pub fn all() -> &'static [Backend] {
    BACKENDS
}

pub fn get(id: &str) -> Option<&'static Backend> {
    BACKENDS.iter().find(|b| b.id.eq_ignore_ascii_case(id))
}

static BACKENDS: &[Backend] = &[
    Backend {
        id: "codex",
        run: run_codex,
        login: || super::login::login_status(crate::sessions::model::Agent::Codex),
        models: codex_models,
        efforts: codex_efforts,
    },
    Backend {
        id: "claude",
        run: run_claude,
        login: || super::login::login_status(crate::sessions::model::Agent::Claude),
        models: claude_models,
        efforts: claude_efforts,
    },
    Backend {
        id: "codebuddy",
        run: super::extra::run_codebuddy,
        login: super::extra::codebuddy_login,
        models: super::extra::codebuddy_models,
        efforts: super::extra::codebuddy_efforts,
    },
    Backend {
        id: "qoder",
        run: super::extra::run_qoder,
        login: super::extra::qoder_login,
        models: super::extra::qoder_models,
        efforts: super::extra::qoder_efforts,
    },
];

fn codex_home() -> std::path::PathBuf {
    std::path::PathBuf::from(crate::sessions::roots::resolve(&Default::default()).codex)
}

fn codex_models() -> Vec<ModelOption> {
    std::fs::read_to_string(codex_home().join("models_cache.json"))
        .map(|text| super::options::codex_models(&text))
        .unwrap_or_default()
}

fn codex_efforts() -> Vec<String> {
    let models = codex_models();
    if models.is_empty() {
        vec!["low".into(), "medium".into(), "high".into()]
    } else {
        super::options::union_efforts(&models)
    }
}

fn claude_efforts() -> Vec<String> {
    super::options::CLAUDE_EFFORTS
        .iter()
        .map(|e| e.to_string())
        .collect()
}

fn claude_models() -> Vec<ModelOption> {
    super::options::CLAUDE_MODELS
        .iter()
        .map(|m| ModelOption {
            id: m.to_string(),
            label: m.to_string(),
            efforts: claude_efforts(),
            default_effort: None,
        })
        .collect()
}

fn run_codex(c: &Ctx) -> Result<String, String> {
    let out = c.tmp.join("out.md");
    let mut cmd =
        crate::sessions::codex_rpc::command().map_err(|_| "E_RUNNER_MISSING".to_string())?;
    cmd.args(super::codex::codex_args(
        c.tmp,
        &out,
        c.model,
        c.effort,
        &super::codex::disabled_features(),
    ));
    let (status, stdout, stderr) = run_program(cmd, c.prompt, c.tmp, c.timeout, c.cancel)?;
    finish(status, &stdout, &stderr)?;
    std::fs::read_to_string(&out).map_err(|_| "E_RUNNER_EMPTY".to_string())
}

fn run_claude(c: &Ctx) -> Result<String, String> {
    let program = crate::agents::process::resolve_command(&["claude.exe", "claude.cmd"])
        .ok_or("E_RUNNER_MISSING")?;
    let mut cmd = Command::new(program);
    cmd.args(super::claude::claude_args(c.model, c.effort));
    let run = run_program(cmd, c.prompt, c.tmp, c.timeout, c.cancel);
    super::claude::remove_project_leftover(c.tmp);
    let (status, stdout, stderr) = run?;
    finish(status, &stdout, &stderr)?;
    super::claude::parse_claude(&stdout)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_unique_and_found_case_insensitively() {
        let ids: Vec<_> = super::all().iter().map(|b| b.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert_eq!(super::get("Claude").map(|b| b.id), Some("claude"));
        assert!(super::get("nope").is_none());
    }
}
