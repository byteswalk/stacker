//! Kimi Code files each session under the working directory it ran in:
//! `<root>/sessions/wd_<workspace>/session_<id>/`. `state.json` says what the session is and
//! `agents/<agent>/wire.jsonl` records the conversation as a stream of events.

use super::claude_catalog::{is_link, tree_size};
use super::model::*;
use super::project::project_ref;
use super::transcript::Message;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// `state.json`, across the two shapes Kimi has written: milliseconds in the current one,
/// ISO timestamps in the older one.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct State {
    pub id: String,
    pub cwd: String,
    pub title: String,
    pub custom_title: bool,
    pub archived: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

fn iso_secs(text: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|t| t.timestamp().max(0) as u64)
        .unwrap_or(0)
}

fn time_at(value: &Value, key: &str) -> u64 {
    match value.get(key) {
        Some(Value::Number(n)) => n.as_u64().map(|ms| ms / 1000).unwrap_or(0),
        Some(Value::String(s)) => iso_secs(s),
        _ => 0,
    }
}

pub fn parse_state(text: &str) -> Option<State> {
    let value: Value = serde_json::from_str(text).ok()?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != "New Session")
        .unwrap_or_default()
        .to_string();
    Some(State {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cwd: value
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        custom_title: value
            .get("isCustomTitle")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && !title.is_empty(),
        title,
        archived: value
            .get("archived")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        created_at: time_at(&value, "createdAt"),
        updated_at: time_at(&value, "updatedAt"),
    })
}

/// The message inside one wire event, whichever way that event wraps it.
fn message_of(value: &Value) -> Option<(String, String)> {
    let message = match value.get("type").and_then(Value::as_str)? {
        "context.append_message" => value.get("message")?,
        "agent.message.appended" => value.get("message")?.get("message")?,
        _ => return None,
    };
    let role = message.get("role").and_then(Value::as_str)?.to_string();
    let text = match message.get("content")? {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some((role, text))
}

fn wire_files(session_dir: &Path) -> Vec<PathBuf> {
    let agents = session_dir.join("agents");
    fs::read_dir(&agents)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path().join("wire.jsonl"))
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default()
}

/// The conversation, in the order the events were written.
pub fn read(session_dir: &Path, limit: usize) -> Result<(Vec<Message>, bool), String> {
    let mut messages = Vec::new();
    let mut complete = true;
    for wire in wire_files(session_dir) {
        let text = fs::read_to_string(&wire).map_err(|_| "E_READ".to_string())?;
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                complete = false;
                continue;
            };
            if let Some((role, text)) = message_of(&value) {
                messages.push(Message {
                    line: messages.len() + 1,
                    role,
                    text,
                });
                if messages.len() > limit {
                    return Ok((messages, false));
                }
            }
        }
    }
    Ok((messages, complete))
}

pub fn contains(session_dir: &Path, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    read(session_dir, 4000)
        .map(|(messages, _)| {
            messages
                .iter()
                .any(|m| m.text.to_lowercase().contains(&needle))
        })
        .unwrap_or(false)
}

fn first_user_message(session_dir: &Path) -> Option<String> {
    read(session_dir, 40).ok()?.0.into_iter().find_map(|m| {
        (m.role == "user").then(|| {
            m.text
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect()
        })
    })
}

fn session_from(session_dir: &Path, index_cwd: Option<&str>) -> Option<Session> {
    let state = parse_state(&fs::read_to_string(session_dir.join("state.json")).ok()?)?;
    let name = session_dir.file_name()?.to_string_lossy().into_owned();
    let id = if state.id.is_empty() { name } else { state.id };
    let cwd = if state.cwd.is_empty() {
        index_cwd.unwrap_or_default().to_string()
    } else {
        state.cwd
    };
    let (title, title_source) = if !state.title.is_empty() {
        (
            state.title,
            if state.custom_title {
                TitleSource::Custom
            } else {
                TitleSource::Client
            },
        )
    } else {
        match first_user_message(session_dir) {
            Some(first) => (first, TitleSource::FirstMessage),
            None => (id.clone(), TitleSource::FirstMessage),
        }
    };
    let project = project_ref(&cwd, None);
    let status = if state.archived {
        SessionStatus::Archived
    } else if !project.exists {
        SessionStatus::Orphaned
    } else {
        SessionStatus::Active
    };
    Some(Session {
        id: format!("kimi:{id}"),
        agent: Agent::Kimi,
        children: Vec::new(),
        title,
        title_source,
        project,
        client: ClientTag::Terminal,
        created_at: state.created_at,
        updated_at: state.updated_at.max(state.created_at),
        archived: state.archived,
        pinned: false,
        status,
        bytes: tree_size(session_dir),
        path: session_dir.to_string_lossy().into_owned(),
        in_desktop_index: false,
        parent_missing: false,
        favorite: false,
        summary: None,
        summary_stale: false,
        summary_by: String::new(),
        summary_at: 0,
        copies: Vec::new(),
        native_id: id,
    })
}

/// `session_index.jsonl`: the working directory of each session, which older states omit.
fn index_of(root: &Path) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Ok(text) = fs::read_to_string(root.join("session_index.jsonl")) else {
        return out;
    };
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let (Some(id), Some(dir)) = (
            value.get("sessionId").and_then(Value::as_str),
            value.get("workDir").and_then(Value::as_str),
        ) {
            out.insert(id.to_string(), dir.to_string());
        }
    }
    out
}

fn dirs_in(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && !is_link(p))
                .collect()
        })
        .unwrap_or_default()
}

pub fn load(root: &Path) -> Result<Vec<Session>, String> {
    let sessions_dir = root.join("sessions");
    if !sessions_dir.is_dir() {
        return Err("E_SOURCE_MISSING".into());
    }
    let index = index_of(root);
    let mut sessions = Vec::new();
    for workspace in dirs_in(&sessions_dir) {
        if workspace
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        for session_dir in dirs_in(&workspace) {
            let name = session_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if !name.starts_with("session_") {
                continue;
            }
            if let Some(session) =
                session_from(&session_dir, index.get(name.as_ref()).map(String::as_str))
            {
                sessions.push(session);
            }
        }
    }
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_session(root: &Path, workspace: &str, id: &str, state: &str, wire: &str) -> PathBuf {
        let dir = root.join("sessions").join(workspace).join(id);
        fs::create_dir_all(dir.join("agents").join("main")).unwrap();
        fs::write(dir.join("state.json"), state).unwrap();
        fs::write(dir.join("agents").join("main").join("wire.jsonl"), wire).unwrap();
        dir
    }

    const WIRE: &str = concat!(
        r#"{"type":"metadata","protocol_version":"1.5","created_at":1789796169448}"#,
        "\n",
        r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"Reply with exactly: OK"}]}}"#,
        "\n",
        r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"type":"text","text":"OK"}]}}"#,
        "\n",
    );

    #[test]
    fn both_state_shapes_are_read() {
        let now = parse_state(r#"{"id":"session_1","cwd":"D:\\work","createdAt":1789796169419,"updatedAt":1789796170000,"archived":false}"#).unwrap();
        assert_eq!(now.id, "session_1");
        assert_eq!(now.created_at, 1789796169);
        assert_eq!(now.updated_at, 1789796170);
        assert!(now.title.is_empty());

        let older = parse_state(
            r#"{"createdAt":"2026-07-15T07:53:05.937Z","updatedAt":"2026-07-15T07:53:06.000Z","title":"New Session","isCustomTitle":false}"#,
        )
        .unwrap();
        // "New Session" is Kimi's placeholder, not a title worth showing.
        assert!(older.title.is_empty());
        assert_eq!(older.created_at, 1784101985);
    }

    #[test]
    fn a_session_reads_its_folder_transcript_and_title() {
        let root = tempfile::tempdir().unwrap();
        let dir = write_session(
            root.path(),
            "wd_work_1",
            "session_1",
            r#"{"id":"session_1","cwd":"D:\\work","createdAt":1789796169419,"updatedAt":1789796170000}"#,
            WIRE,
        );
        let (messages, complete) = read(&dir, 100).unwrap();
        assert!(complete);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].text, "OK");
        assert!(contains(&dir, "reply with"));
        assert!(!contains(&dir, "absent"));

        let sessions = load(root.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.id, "kimi:session_1");
        assert_eq!(session.agent, Agent::Kimi);
        // No title in the state, so the first thing the user said stands in.
        assert_eq!(session.title, "Reply with exactly: OK");
        assert_eq!(session.project.path, "D:\\work");
        assert!(session.bytes > 0);
    }

    #[test]
    fn a_missing_store_is_not_an_error_to_show() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(load(root.path()).unwrap_err(), "E_SOURCE_MISSING");
    }
}
