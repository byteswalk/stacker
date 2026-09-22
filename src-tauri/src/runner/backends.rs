//! Every agent CLI the runner can drive, and how: invocation, answer parsing, sign-in check,
//! and model list. Each entry is verified to run without saving a session and without tools.
use super::login::LoginStatus;
use super::options::ModelOption;
use super::{finish, run_program, run_program_lines, Attachment, AttachmentKind, CancelFlag};
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
    /// Only kinds the backend lists in `attachments` ever reach it.
    pub attachments: &'a [Attachment],
    /// Called with each piece of the answer as it arrives, where the CLI streams.
    pub on_delta: Option<&'a (dyn Fn(&str) + Send + Sync)>,
}

pub struct Backend {
    /// Same id as the agent registry's CLI id.
    pub id: &'static str,
    pub run: fn(&Ctx) -> Result<String, String>,
    pub login: fn() -> LoginStatus,
    pub models: fn() -> Vec<ModelOption>,
    /// Reasoning levels for a request that names no model.
    pub efforts: fn() -> Vec<String>,
    /// Attachment kinds the CLI can read; any other attachment fails the run up front.
    pub attachments: &'static [AttachmentKind],
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
        // `codex exec -i`; Codex has no document input.
        attachments: &[AttachmentKind::Image],
    },
    Backend {
        id: "claude",
        run: run_claude,
        login: || super::login::login_status(crate::sessions::model::Agent::Claude),
        models: claude_models,
        efforts: claude_efforts,
        attachments: &[AttachmentKind::Image, AttachmentKind::Pdf],
    },
    Backend {
        id: "codebuddy",
        run: super::extra::run_codebuddy,
        login: super::extra::codebuddy_login,
        models: super::extra::codebuddy_models,
        efforts: super::extra::codebuddy_efforts,
        attachments: &[],
    },
    Backend {
        id: "qoder",
        run: super::extra::run_qoder,
        login: super::extra::qoder_login,
        models: super::extra::qoder_models,
        efforts: super::extra::qoder_efforts,
        attachments: &[],
    },
    Backend {
        id: "agy",
        run: super::cmdline::run_agy,
        login: super::cmdline::agy_login,
        models: super::cmdline::agy_models,
        efforts: super::cmdline::agy_efforts,
        attachments: &[],
    },
    Backend {
        id: "dsh",
        run: super::cmdline::run_dsh,
        login: super::cmdline::dsh_login,
        models: super::cmdline::dsh_models,
        efforts: super::cmdline::dsh_efforts,
        attachments: &[],
    },
    Backend {
        id: "hermes",
        run: super::cmdline::run_hermes,
        login: super::cmdline::hermes_login,
        models: super::cmdline::hermes_models,
        efforts: super::cmdline::hermes_efforts,
        attachments: &[],
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

/// Codex reads images only from files: each one is written into the run's folder.
fn image_files(c: &Ctx) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    for (i, a) in c.attachments.iter().enumerate() {
        if a.kind != AttachmentKind::Image {
            return Err("E_ATTACHMENT_UNSUPPORTED".into());
        }
        let path = c
            .tmp
            .join(format!("attachment-{}.{}", i + 1, a.extension()));
        std::fs::write(&path, a.bytes()?).map_err(|_| "E_STORAGE".to_string())?;
        files.push(path);
    }
    Ok(files)
}

fn run_codex(c: &Ctx) -> Result<String, String> {
    let out = c.tmp.join("out.md");
    let images = image_files(c)?;
    let mut cmd =
        crate::sessions::codex_rpc::command().map_err(|_| "E_RUNNER_MISSING".to_string())?;
    cmd.args(super::codex::codex_args(
        c.tmp,
        &out,
        c.model,
        c.effort,
        &images,
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
    cmd.args(super::claude::claude_stream_args(c.model, c.effort));
    let input = super::claude::stream_input(c.prompt, c.attachments);
    let mut stream = super::claude::StreamState::default();
    let run = run_program_lines(cmd, &input, c.tmp, c.timeout, c.cancel, &mut |line| {
        if let Some(delta) = stream.line(line) {
            if let Some(sink) = c.on_delta {
                sink(&delta);
            }
        }
    });
    super::claude::remove_project_leftover(c.tmp);
    let (status, _, stderr) = run?;
    // A result line decides; without one, the exit status and stderr do.
    if stream.result.is_none() {
        finish(status, "", &stderr)?;
    }
    stream.answer()
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
