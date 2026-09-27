//! MiMo Code keeps everything in one SQLite file: `session` rows name the folder and title,
//! `message` rows the turns, and `part` rows the text, reasoning and tool calls inside each
//! turn. The database is only ever opened read-only.

use super::model::*;
use super::project::project_ref;
use super::transcript::Message;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::Path;

pub const DB_NAME: &str = "mimocode.db";

fn open(db: &Path) -> Result<Connection, String> {
    if !db.is_file() {
        return Err("E_SOURCE_MISSING".into());
    }
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(super::err)
}

/// Milliseconds in the database, seconds everywhere in Stacker.
fn secs(ms: i64) -> u64 {
    if ms <= 0 {
        0
    } else {
        (ms / 1000) as u64
    }
}

/// A session's own share of the database: the bytes its messages and parts take.
fn size_of(conn: &Connection, id: &str) -> u64 {
    let sql = "SELECT COALESCE((SELECT SUM(LENGTH(data)) FROM message WHERE session_id=?1),0)
             + COALESCE((SELECT SUM(LENGTH(data)) FROM part WHERE session_id=?1),0)";
    conn.query_row(sql, [id], |r| r.get::<_, i64>(0))
        .map(|n| n.max(0) as u64)
        .unwrap_or(0)
}

pub fn load(root: &Path) -> Result<Vec<Session>, String> {
    let db = root.join(DB_NAME);
    let conn = open(&db)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, directory, title, title_source, time_created, time_updated, time_archived, parent_id
             FROM session ORDER BY time_updated DESC",
        )
        .map_err(super::err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(super::err)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    let path = db.to_string_lossy().into_owned();
    Ok(rows
        .into_iter()
        .filter(|row| row.7.is_none())
        .map(
            |(id, directory, title, title_source, created, updated, archived, _parent)| {
                let project = project_ref(&directory, None);
                let archived = archived.is_some_and(|t| t > 0);
                let status = if archived {
                    SessionStatus::Archived
                } else if !project.exists {
                    SessionStatus::Orphaned
                } else {
                    SessionStatus::Active
                };
                Session {
                    id: format!("mimo:{id}"),
                    agent: Agent::MiMo,
                    children: Vec::new(),
                    title: if title.trim().is_empty() {
                        id.clone()
                    } else {
                        title
                    },
                    // MiMo says where a title came from: anything but its own fallback is
                    // a title the session earned.
                    title_source: if title_source == "fallback" {
                        TitleSource::FirstMessage
                    } else {
                        TitleSource::Summary
                    },
                    project,
                    client: ClientTag::Terminal,
                    created_at: secs(created),
                    updated_at: secs(updated),
                    archived,
                    pinned: false,
                    status,
                    bytes: size_of(&conn, &id),
                    path: path.clone(),
                    in_desktop_index: false,
                    parent_missing: false,
                    favorite: false,
                    summary: None,
                    summary_stale: false,
                    summary_by: String::new(),
                    summary_at: 0,
                    copies: Vec::new(),
                    native_id: id,
                }
            },
        )
        .collect())
}

/// The text of one part, for the parts that carry any.
fn part_text(data: &str) -> Option<String> {
    let value: Value = serde_json::from_str(data).ok()?;
    match value.get("type").and_then(Value::as_str)? {
        "text" | "reasoning" => value
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        "tool" => {
            let tool = value.get("tool").and_then(Value::as_str).unwrap_or("tool");
            let input = value
                .get("state")
                .and_then(|s| s.get("input"))
                .map(|v| v.to_string())
                .unwrap_or_default();
            Some(format!("{tool}\n{input}"))
        }
        _ => None,
    }
}

/// A session's turns, in order: each message's role with the text of its parts.
pub fn read(db: &Path, session_id: &str, limit: usize) -> Result<(Vec<Message>, bool), String> {
    let conn = open(db)?;
    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.data, p.data FROM message m
             LEFT JOIN part p ON p.message_id = m.id
             WHERE m.session_id = ?1
             ORDER BY m.time_created, p.time_created",
        )
        .map_err(super::err)?;
    let rows = stmt
        .query_map([session_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(super::err)?
        .filter_map(Result::ok);

    let mut messages: Vec<Message> = Vec::new();
    let mut complete = true;
    let mut line = 0usize;
    for (message_id, message_data, part) in rows {
        let role = serde_json::from_str::<Value>(&message_data)
            .ok()
            .and_then(|v| v.get("role").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| "assistant".into());
        let Some(text) = part.as_deref().and_then(part_text) else {
            continue;
        };
        line += 1;
        // Parts of one turn read as one message, the way the other agents' transcripts do.
        match messages.last_mut() {
            Some(last) if last.role == role && last.line == line - 1 => {
                last.text.push('\n');
                last.text.push_str(&text);
                last.line = line;
            }
            _ => messages.push(Message { line, role, text }),
        }
        let _ = message_id;
        if messages.len() > limit {
            complete = false;
            break;
        }
    }
    Ok((messages, complete))
}

/// Case-insensitive search of a session's text, without loading all of it at once.
pub fn contains(db: &Path, session_id: &str, needle: &str) -> bool {
    let Ok(conn) = open(db) else {
        return false;
    };
    let sql = "SELECT 1 FROM part WHERE session_id = ?1 AND LOWER(data) LIKE ?2 LIMIT 1";
    let pattern = format!("%{}%", needle.to_lowercase());
    conn.query_row(sql, rusqlite::params![session_id, pattern], |_| Ok(()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_db(dir: &Path) -> std::path::PathBuf {
        let db = dir.join(DB_NAME);
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT, project_id TEXT, parent_id TEXT, slug TEXT, directory TEXT,
                title TEXT, version TEXT, time_created INTEGER, time_updated INTEGER, time_archived INTEGER,
                title_source TEXT);
             CREATE TABLE message (id TEXT, session_id TEXT, agent_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
             INSERT INTO session VALUES ('ses_1','global',NULL,'brave-moon','D:\\work','Plan the release',
                '0.1.15',1790482355492,1790482359000,NULL,'model');
             INSERT INTO message VALUES ('m1','ses_1','main',1,'{\"role\":\"user\"}');
             INSERT INTO message VALUES ('m2','ses_1','main',2,'{\"role\":\"assistant\"}');
             INSERT INTO part VALUES ('p1','m1','ses_1',1,'{\"type\":\"text\",\"text\":\"ship it\"}');
             INSERT INTO part VALUES ('p2','m2','ses_1',2,'{\"type\":\"text\",\"text\":\"on it\"}');
             INSERT INTO part VALUES ('p3','m2','ses_1',3,'{\"type\":\"tool\",\"tool\":\"Bash\",\"state\":{\"input\":{\"command\":\"ls\"}}}');",
        )
        .unwrap();
        db
    }

    #[test]
    fn sessions_carry_their_folder_title_and_size() {
        let dir = tempfile::tempdir().unwrap();
        write_db(dir.path());
        let sessions = load(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.id, "mimo:ses_1");
        assert_eq!(session.agent, Agent::MiMo);
        assert_eq!(session.native_id, "ses_1");
        assert_eq!(session.title, "Plan the release");
        assert_eq!(session.title_source, TitleSource::Summary);
        assert_eq!(session.project.path, "D:\\work");
        assert_eq!(session.created_at, 1790482355);
        assert!(session.bytes > 0, "a session counts its own rows");
    }

    #[test]
    fn a_transcript_reads_turn_by_turn() {
        let dir = tempfile::tempdir().unwrap();
        let db = write_db(dir.path());
        let (messages, complete) = read(&db, "ses_1", 100).unwrap();
        assert!(complete);
        assert_eq!(messages.len(), 2);
        assert_eq!(
            (messages[0].role.as_str(), messages[0].text.as_str()),
            ("user", "ship it")
        );
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].text.contains("on it"));
        assert!(
            messages[1].text.contains("Bash"),
            "tool calls are part of the turn"
        );
        assert!(contains(&db, "ses_1", "SHIP"));
        assert!(!contains(&db, "ses_1", "absent"));
    }

    #[test]
    fn a_missing_database_is_not_an_error_to_show() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap_err(), "E_SOURCE_MISSING");
    }
}
