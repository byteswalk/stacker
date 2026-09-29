//! Conversations the desktop apps keep, which are not the same stores their CLIs write.
//!
//! Qoder is a VS Code fork and keeps its chat list in the editor's own key-value database,
//! `state.vscdb`: one row holding a JSON array of `{id, title, timestamp}`. The message
//! bodies are not in there, so these conversations are listed and searched by title, never
//! opened — and never deleted either, because rewriting an editor's database behind its back
//! is not something Stacker should do.
//!
//! TRAE's CLI keeps a folder per run holding logs and traces, not a transcript. The trace
//! does carry the prompt the run started with, so those runs are listed by what was asked.
//!
//! Antigravity keeps one SQLite file per conversation, with the turns stored as protobuf.
//! Without the schema the only honest reading is of the text inside: the workspace it ran in
//! and the first thing the user typed, which is enough to list it.

use super::model::*;
use super::project::project_ref;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn open(db: &Path) -> Result<Connection, String> {
    if !db.is_file() {
        return Err("E_SOURCE_MISSING".into());
    }
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(super::err)
}

fn secs(ms: i64) -> u64 {
    if ms <= 0 {
        0
    } else {
        (ms / 1000) as u64
    }
}

fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn session(agent: Agent, id: String, title: String, at: u64, cwd: &str, path: &Path) -> Session {
    let project = project_ref(cwd, None);
    let status = if project.path.is_empty() || project.exists {
        SessionStatus::Active
    } else {
        SessionStatus::Orphaned
    };
    Session {
        id: format!("{}:{id}", agent.as_str()),
        agent,
        children: Vec::new(),
        title,
        title_source: TitleSource::Client,
        project,
        client: ClientTag::Desktop,
        created_at: at,
        updated_at: at,
        archived: false,
        pinned: false,
        status,
        bytes: 0,
        path: path.to_string_lossy().into_owned(),
        in_desktop_index: true,
        parent_missing: false,
        favorite: false,
        summary: None,
        summary_stale: false,
        summary_by: String::new(),
        summary_at: 0,
        copies: Vec::new(),
        imported_from: None,
        imported_by: Vec::new(),
        native_id: id,
    }
}

/// One title out of what the app stored: its first line, without the banner some tools paste
/// in front of the conversation.
pub fn first_line(title: &str) -> String {
    title
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.chars().all(|c| c == '=' || c == '-' || c == '*'))
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

/// Qoder's desktop chat list, from the editor database it shares with VS Code.
pub fn qoder_app(app_data: &Path, agent: Agent) -> Result<Vec<Session>, String> {
    let db = app_data
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    let conn = open(&db)?;
    let mut stmt = conn
        .prepare("SELECT value FROM ItemTable WHERE key LIKE 'lingma.chat.localHistory%'")
        .map_err(super::err)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(super::err)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    let mut sessions = Vec::new();
    for raw in rows {
        let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        for item in items {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            let title = first_line(item.get("title").and_then(Value::as_str).unwrap_or(""));
            let at = secs(item.get("timestamp").and_then(Value::as_i64).unwrap_or(0));
            sessions.push(session(
                agent,
                id.to_string(),
                if title.is_empty() {
                    id.to_string()
                } else {
                    title
                },
                if at > 0 { at } else { mtime(&db) },
                "",
                &db,
            ));
        }
    }
    Ok(sessions)
}

/// TRAE's CLI runs, listed by the prompt each one started with.
pub fn trae_cli(dir: &Path, agent: Agent) -> Result<Vec<Session>, String> {
    if !dir.is_dir() {
        return Err("E_SOURCE_MISSING".into());
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(dir)
        .map_err(|_| "E_READ".to_string())?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        // A run that never asked anything left only a log, and is not a conversation.
        let Some((title, at)) = trae_prompt(&path.join("traces.jsonl")) else {
            continue;
        };
        let mut found = session(
            agent,
            id,
            title,
            if at > 0 { at } else { mtime(&path) },
            "",
            &path,
        );
        found.client = ClientTag::Terminal;
        found.bytes = folder_bytes(&path);
        sessions.push(found);
    }
    Ok(sessions)
}

/// The prompt a run started with, out of the tags on its first span that carries one.
fn trae_prompt(traces: &Path) -> Option<(String, u64)> {
    let text = fs::read_to_string(traces).ok()?;
    for line in text.lines() {
        let Ok(span) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let summary = span
            .get("tags")
            .and_then(Value::as_array)
            .and_then(|tags| {
                tags.iter()
                    .find(|tag| tag.get("key").and_then(Value::as_str) == Some("prompt.summary"))
                    .and_then(|tag| tag.get("value").and_then(Value::as_str))
            })
            .map(first_line)
            .filter(|summary| !summary.is_empty() && summary != "-");
        if let Some(summary) = summary {
            // Span times are microseconds since the epoch.
            let at = span.get("startTime").and_then(Value::as_u64).unwrap_or(0) / 1_000_000;
            return Some((summary, at));
        }
    }
    None
}

fn folder_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.metadata().ok())
                .filter(|meta| meta.is_file())
                .map(|meta| meta.len())
                .sum()
        })
        .unwrap_or(0)
}

/// The readable text inside a protobuf payload: every length-delimited field that turns out
/// to be printable. Without the schema this is what can be said honestly.
pub fn readable_strings(bytes: &[u8], want: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() && out.len() < want {
        // A field header, then a length, then that many bytes: the only shape worth reading.
        let wire = bytes[i] & 0x7;
        i += 1;
        if wire != 2 {
            continue;
        }
        let mut length = 0usize;
        let mut shift = 0;
        while i < bytes.len() {
            let byte = bytes[i];
            i += 1;
            length |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return out;
            }
        }
        if length == 0 || length > bytes.len().saturating_sub(i) {
            continue;
        }
        let slice = &bytes[i..i + length];
        if let Ok(text) = std::str::from_utf8(slice) {
            let trimmed = text.trim();
            if trimmed.chars().count() >= 2
                && trimmed
                    .chars()
                    .all(|c| !c.is_control() || c == '\n' || c == '\t')
            {
                out.push(trimmed.chars().take(200).collect());
            }
        }
        i += length;
    }
    out
}

/// The workspace a conversation ran in, out of the `file:///…` URI its metadata carries.
pub fn workspace_of(texts: &[String]) -> String {
    let uri = texts.iter().find_map(|text| {
        text.split_whitespace()
            .find(|word| word.starts_with("file:///"))
    });
    uri.map(decode_uri).unwrap_or_default()
}

fn decode_uri(uri: &str) -> String {
    let path = uri.trim_start_matches("file:///");
    let bytes = path.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&path[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'/' { b'\\' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Antigravity's desktop conversations: one database each, read for what it can say.
pub fn antigravity(dir: &Path, agent: Agent) -> Result<Vec<Session>, String> {
    let conversations = dir.join("conversations");
    if !conversations.is_dir() {
        return Err("E_SOURCE_MISSING".into());
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&conversations)
        .map_err(|_| "E_READ".to_string())?
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "db") {
            if let Some(found) = antigravity_one(&path, agent) {
                sessions.push(found);
            }
        }
    }
    Ok(sessions)
}

fn antigravity_one(db: &Path, agent: Agent) -> Option<Session> {
    let id = db.file_stem()?.to_string_lossy().into_owned();
    let conn = open(db).ok()?;
    let meta: Vec<u8> = conn
        .query_row(
            "SELECT data FROM trajectory_metadata_blob LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();
    let cwd = workspace_of(&readable_strings(&meta, 12));
    let first = conn
        .query_row(
            "SELECT step_payload FROM steps ORDER BY idx LIMIT 1",
            [],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .ok()
        .map(|payload| readable_strings(&payload, 24))
        .unwrap_or_default();
    // Identifiers and model names travel in the same payload; a title is the longest thing
    // that reads like something a person typed.
    let title = first
        .into_iter()
        .filter(|text| !text.contains("file:///") && !looks_like_id(text))
        .max_by_key(|text| text.chars().count())
        .unwrap_or_else(|| id.clone());
    let bytes = fs::metadata(db).map(|m| m.len()).unwrap_or(0);
    let mut session = session(agent, id, first_line(&title), mtime(db), &cwd, db);
    session.bytes = bytes;
    Some(session)
}

fn looks_like_id(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.len() >= 20
        && trimmed
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_banner_is_not_a_title() {
        assert_eq!(first_line("=====\nCreate a user\n====="), "Create a user");
        assert_eq!(first_line("  \n\n提交"), "提交");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn readable_text_comes_out_of_a_protobuf_payload() {
        // Field 1, length-delimited, "hello"; then field 2 with a non-UTF-8 blob.
        let mut payload = vec![0x0a, 5];
        payload.extend_from_slice(b"hello");
        payload.extend_from_slice(&[0x12, 3, 0xff, 0xfe, 0xfd]);
        let texts = readable_strings(&payload, 10);
        assert_eq!(texts, vec!["hello".to_string()]);
    }

    #[test]
    fn a_workspace_uri_becomes_a_windows_path() {
        let texts = vec!["file:///c%3A/Users/me/Desktop/%E6%8A%95%E6%A0%87".to_string()];
        assert_eq!(workspace_of(&texts), "c:\\Users\\me\\Desktop\\投标");
        assert_eq!(workspace_of(&["no uri here".to_string()]), "");
    }

    #[test]
    fn an_identifier_is_not_a_title() {
        assert!(looks_like_id("38ae068e-bc18-4aee-bb47-0ddd872bb5ee"));
        assert!(!looks_like_id("帮我看看这个工程"));
        assert!(!looks_like_id("fix the build"));
    }

    #[test]
    fn a_trae_run_is_listed_by_the_prompt_in_its_trace() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join("a1b2");
        std::fs::create_dir_all(&run).unwrap();
        let span = |name: &str, summary: &str| {
            format!(
                concat!(
                    r#"{{"operationName":"{}","startTime":1789796760589829,"#,
                    r#""tags":[{{"key":"prompt.summary","value":"{}"}}]}}"#
                ),
                name, summary
            )
        };
        std::fs::write(
            run.join("traces.jsonl"),
            format!(
                "{}\n{}\n",
                span("cmd.root", "-"),
                span("query.do", "fix the build")
            ),
        )
        .unwrap();
        // A run with nothing asked in it is not a conversation.
        let empty = dir.path().join("c3d4");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("traces.jsonl"), "{}\n").unwrap();

        let sessions = trae_cli(dir.path(), Agent::Trae).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "fix the build");
        assert_eq!(sessions[0].native_id, "a1b2");
        assert_eq!(sessions[0].client, ClientTag::Terminal);
        assert_eq!(sessions[0].created_at, 1789796760);
        assert!(sessions[0].bytes > 0);
        assert_eq!(
            trae_cli(&dir.path().join("nope"), Agent::Trae).unwrap_err(),
            "E_SOURCE_MISSING"
        );
    }

    #[test]
    fn a_missing_store_is_not_an_error_to_show() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            qoder_app(dir.path(), Agent::Qoder).unwrap_err(),
            "E_SOURCE_MISSING"
        );
        assert_eq!(
            antigravity(dir.path(), Agent::Antigravity).unwrap_err(),
            "E_SOURCE_MISSING"
        );
    }
}
