//! CodeBuddy's transcripts: `<root>/projects/<project-slug>/<session-id>.jsonl`, one JSON
//! object per line. It is a Claude Code descendant, so the shape is familiar: `message`
//! lines carry the conversation, a `summary` line carries the title the CLI wrote, and every
//! line names the session and the working directory it ran in.

use super::claude_catalog::{is_link, tree_size};
use super::model::*;
use super::project::project_ref;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// What one transcript says about itself.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Head {
    pub session_id: String,
    pub cwd: String,
    pub summary: Option<String>,
    pub first_user_message: Option<String>,
    /// The first slash command, for sessions that only ran commands.
    pub first_command: Option<String>,
    pub agent: String,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// What the CLI itself puts in the user's turn: injected reminders, the slash command the
/// user ran, and that command's output. None of it is something the user typed at us, so
/// none of it becomes a title.
fn is_context(text: &str) -> bool {
    let text = text.trim_start();
    (text.starts_with('<') && text.contains("</")) || text.starts_with("Caveat:")
}

/// `<command-name>/model</command-name>` — the slash command a session ran, which is the
/// only thing some sessions contain.
fn command_name(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix("<command-name>")?;
    let name = rest.split("</command-name>").next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn absorb(head: &mut Head, value: &Value) {
    let at = value
        .get("timestamp")
        .and_then(Value::as_u64)
        .map(|ms| ms / 1000)
        .unwrap_or(0);
    if at > 0 {
        if head.created_at == 0 {
            head.created_at = at;
        }
        head.updated_at = head.updated_at.max(at);
    }
    for (key, target) in [("sessionId", &mut head.session_id), ("cwd", &mut head.cwd)] {
        if target.is_empty() {
            if let Some(found) = value.get(key).and_then(Value::as_str) {
                *target = found.to_string();
            }
        }
    }
    if let Some(provider) = value.get("providerData") {
        for (key, target) in [("agent", &mut head.agent), ("model", &mut head.model)] {
            if target.is_empty() {
                if let Some(found) = provider.get(key).and_then(Value::as_str) {
                    *target = found.to_string();
                }
            }
        }
    }
    match value.get("type").and_then(Value::as_str) {
        Some("summary") => {
            if let Some(summary) = value
                .get("summary")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                head.summary = Some(summary.to_string());
            }
        }
        Some("message")
            if head.first_user_message.is_none()
                && value.get("role").and_then(Value::as_str) == Some("user") =>
        {
            let text = value.get("content").map(text_of).unwrap_or_default();
            let text = text.trim();
            if head.first_command.is_none() {
                head.first_command = command_name(text);
            }
            if !text.is_empty() && !is_context(text) {
                head.first_user_message = Some(text.chars().take(120).collect());
            }
        }
        _ => {}
    }
}

pub fn read_head(path: &Path) -> Result<Head, String> {
    let text = fs::read_to_string(path).map_err(|_| "E_READ".to_string())?;
    let mut head = Head::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            absorb(&mut head, &value);
        }
    }
    Ok(head)
}

fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && !is_link(p))
                .collect()
        })
        .unwrap_or_default()
}

fn session_from(path: &Path) -> Option<Session> {
    let head = read_head(path).ok()?;
    let session_id = if head.session_id.is_empty() {
        path.file_stem()?.to_string_lossy().into_owned()
    } else {
        head.session_id.clone()
    };
    let (title, title_source) = match (&head.summary, &head.first_user_message) {
        (Some(summary), _) => (summary.clone(), TitleSource::Summary),
        (None, Some(first)) => (first.clone(), TitleSource::FirstMessage),
        // Sessions that only ran slash commands are named after the first one.
        _ => match &head.first_command {
            Some(command) => (command.clone(), TitleSource::FirstMessage),
            None => (session_id.clone(), TitleSource::FirstMessage),
        },
    };
    let client = match head.agent.as_str() {
        "cli" => ClientTag::Terminal,
        "ide" | "vscode" | "jetbrains" => ClientTag::Ide,
        "" => ClientTag::Unknown,
        _ => ClientTag::Automation,
    };
    let project = project_ref(&head.cwd, None);
    let file_time = mtime(path);
    let status = if project.exists {
        SessionStatus::Active
    } else {
        SessionStatus::Orphaned
    };
    Some(Session {
        id: format!("codebuddy:{session_id}"),
        agent: Agent::CodeBuddy,
        children: Vec::new(),
        native_id: session_id,
        title,
        title_source,
        project,
        client,
        created_at: if head.created_at > 0 {
            head.created_at
        } else {
            file_time
        },
        updated_at: if head.updated_at > 0 {
            head.updated_at
        } else {
            file_time
        },
        archived: false,
        pinned: false,
        status,
        bytes: tree_size(path),
        path: path.to_string_lossy().into_owned(),
        in_desktop_index: false,
        parent_missing: false,
        favorite: false,
        summary: None,
        summary_stale: false,
        summary_by: String::new(),
        summary_at: 0,
        copies: Vec::new(),
    })
}

pub fn load(root: &Path) -> Result<Vec<Session>, String> {
    let projects = root.join("projects");
    if !projects.is_dir() {
        return Err("E_SOURCE_MISSING".into());
    }
    let mut sessions = Vec::new();
    for project_dir in subdirs(&projects) {
        let Ok(files) = fs::read_dir(&project_dir) else {
            continue;
        };
        for path in files.flatten().map(|e| e.path()) {
            if !path.extension().is_some_and(|e| e == "jsonl") || is_link(&path) {
                continue;
            }
            if let Some(session) = session_from(&path) {
                sessions.push(session);
            }
        }
    }
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &str = concat!(
        r#"{"id":"1","timestamp":1790126889032,"type":"message","role":"user","content":[{"type":"input_text","text":"<system-reminder>ignore me</system-reminder>"}],"sessionId":"s-1","cwd":"D:\\work"}"#,
        "\n",
        r#"{"id":"2","timestamp":1790126889100,"type":"message","role":"user","content":[{"type":"input_text","text":"list the dependencies"}],"providerData":{"agent":"cli"},"sessionId":"s-1","cwd":"D:\\work"}"#,
        "\n",
        r#"{"id":"3","timestamp":1790126893728,"type":"message","role":"assistant","content":[{"type":"output_text","text":"sure"}],"providerData":{"agent":"cli","model":"hy4-preview"}}"#,
        "\n",
        r#"{"id":"4","timestamp":1790126899000,"type":"summary","summary":"Dependency list"}"#,
        "\n",
    );

    #[test]
    fn a_transcript_names_its_session_project_and_title() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s-1.jsonl");
        std::fs::write(&path, LINES).unwrap();
        let head = read_head(&path).unwrap();
        assert_eq!(head.session_id, "s-1");
        assert_eq!(head.cwd, "D:\\work");
        assert_eq!(head.agent, "cli");
        assert_eq!(head.model, "hy4-preview");
        // The injected reminder is skipped; the first thing the user typed is the fallback.
        assert_eq!(
            head.first_user_message.as_deref(),
            Some("list the dependencies")
        );
        assert!(head.first_command.is_none());
        assert_eq!(head.summary.as_deref(), Some("Dependency list"));
        assert_eq!(head.created_at, 1790126889);
        assert_eq!(head.updated_at, 1790126899);

        let session = session_from(&path).unwrap();
        assert_eq!(session.id, "codebuddy:s-1");
        assert_eq!(session.agent, Agent::CodeBuddy);
        assert_eq!(session.title, "Dependency list");
        assert_eq!(session.title_source, TitleSource::Summary);
        assert_eq!(session.client, ClientTag::Terminal);
        assert_eq!(session.project.path, "D:\\work");
    }

    /// Some sessions contain nothing but the CLI echoing slash commands back.
    #[test]
    fn command_echoes_never_become_the_title() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s-2.jsonl");
        let lines = concat!(
            r#"{"timestamp":1790437594085,"type":"message","role":"user","content":[{"text":"<system-reminder data-role=\"command-caveat\">Caveat: …</system-reminder>"}],"sessionId":"s-2","cwd":"D:\work"}"#,
            "
",
            r#"{"timestamp":1790437594086,"type":"message","role":"user","content":[{"text":"<command-name>/model</command-name>"}],"sessionId":"s-2"}"#,
            "
",
            r#"{"timestamp":1790437594087,"type":"message","role":"user","content":[{"text":"<local-command-stdout>Switch model to deepseek</local-command-stdout>"}],"sessionId":"s-2"}"#,
            "
",
        );
        std::fs::write(&path, lines).unwrap();
        let head = read_head(&path).unwrap();
        assert_eq!(head.first_user_message, None);
        assert_eq!(head.first_command.as_deref(), Some("/model"));
        assert_eq!(session_from(&path).unwrap().title, "/model");
    }

    #[test]
    fn a_missing_store_is_not_an_error_to_show() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap_err(), "E_SOURCE_MISSING");
        let projects = dir.path().join("projects").join("d-work");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::write(projects.join("s-1.jsonl"), LINES).unwrap();
        let sessions = load(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].native_id, "s-1");
    }
}
