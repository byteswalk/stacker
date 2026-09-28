//! Whether an agent CLI is signed in, asked from the CLI itself; credentials are never read.
use super::{run_program, CancelFlag};
use crate::sessions::model::Agent;
use serde::Serialize;
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    /// logged_in | logged_out | unknown
    pub state: String,
    /// How it is signed in ("ChatGPT", "claude.ai"…); never the account itself.
    pub method: String,
}

fn status(state: &str, method: &str) -> LoginStatus {
    LoginStatus {
        state: state.into(),
        method: method.into(),
    }
}

/// `codex login status` prints e.g. "Logged in using ChatGPT" / "Not logged in".
pub fn parse_codex(text: &str) -> LoginStatus {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let lower = line.to_lowercase();
    if lower.starts_with("logged in") {
        let method = line
            .split_once(" using ")
            .map(|(_, m)| m.trim())
            .unwrap_or("");
        status("logged_in", method)
    } else if lower.contains("not logged in") {
        status("logged_out", "")
    } else {
        status("unknown", "")
    }
}

/// `claude auth status` prints JSON with `loggedIn` and `authMethod`.
pub fn parse_claude(text: &str) -> LoginStatus {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return status("unknown", "");
    };
    let method = v.get("authMethod").and_then(|m| m.as_str()).unwrap_or("");
    match v.get("loggedIn").and_then(|b| b.as_bool()) {
        Some(true) => status("logged_in", method),
        Some(false) => status("logged_out", ""),
        None => status("unknown", ""),
    }
}

pub fn login_status(agent: Agent) -> LoginStatus {
    let (cmd, parse): (Option<Command>, fn(&str) -> LoginStatus) = match agent {
        Agent::CodeBuddy
        | Agent::WorkBuddy
        | Agent::WorkBuddyAi
        | Agent::Qoder
        | Agent::QoderCn
        | Agent::MiMo
        | Agent::Kimi => return status("unknown", ""),
        Agent::Codex => (
            crate::sessions::codex_rpc::command().ok().map(|mut c| {
                c.args(["login", "status"]);
                c
            }),
            parse_codex,
        ),
        Agent::Claude => (
            crate::agents::process::resolve_command(&["claude.exe", "claude.cmd"]).map(|p| {
                let mut c = Command::new(p);
                c.args(["auth", "status"]);
                c
            }),
            parse_claude,
        ),
    };
    let Some(cmd) = cmd else {
        return status("unknown", "");
    };
    let tmp = std::env::temp_dir();
    match run_program(
        cmd,
        "",
        &tmp,
        Duration::from_secs(20),
        &CancelFlag::default(),
    ) {
        // Some versions report on stderr.
        Ok((_, out, err)) => {
            let first = parse(&out);
            if first.state == "unknown" {
                parse(&err)
            } else {
                first
            }
        }
        Err(_) => status("unknown", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_outputs_parse() {
        assert_eq!(
            parse_codex("Logged in using ChatGPT\n"),
            status("logged_in", "ChatGPT")
        );
        assert_eq!(parse_codex("Not logged in").state, "logged_out");
        assert_eq!(parse_codex("").state, "unknown");
        let json = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"someone@example.com"}"#;
        let s = parse_claude(json);
        assert_eq!(s, status("logged_in", "claude.ai"));
        assert!(
            !format!("{s:?}").contains("example.com"),
            "the account is never kept"
        );
        assert_eq!(parse_claude(r#"{"loggedIn":false}"#).state, "logged_out");
    }
}
