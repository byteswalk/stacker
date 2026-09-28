//! WorkBuddy writes its conversations the way CodeBuddy does —
//! `<root>/projects/<project-slug>/<session-id>.jsonl` — and keeps an index of them in
//! `workbuddy.db`: the title the app shows, the working directory, whether the conversation
//! was an automation run, and whether the user deleted it in the app. Deleting there does
//! not remove the transcript, so those rows are what tells a discarded conversation from a
//! live one. The database is only ever opened read-only.

use super::model::*;
use super::project::project_ref;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

pub const DB_NAME: &str = "workbuddy.db";

/// What the app's own index says about one conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexRow {
    pub title: String,
    pub custom_title: bool,
    pub cwd: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub discarded: bool,
    pub automation: bool,
}

fn secs(ms: i64) -> u64 {
    if ms <= 0 {
        0
    } else {
        (ms / 1000) as u64
    }
}

/// The index, or an empty one: an app that has never been opened has no database, and a
/// transcript still describes itself well enough to list.
pub fn index(root: &Path) -> HashMap<String, IndexRow> {
    let db = root.join(DB_NAME);
    if !db.is_file() {
        return HashMap::new();
    }
    let Ok(conn) = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return HashMap::new();
    };
    let sql = "SELECT id, title, custom_title, cwd, created_at, updated_at, deleted_at,
                      is_background_automation
               FROM sessions";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return HashMap::new();
    };
    let rows = stmt.query_map([], |r| {
        let custom: Option<String> = r.get(2)?;
        Ok((
            r.get::<_, String>(0)?,
            IndexRow {
                title: custom
                    .clone()
                    .or(r.get::<_, Option<String>>(1)?)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                custom_title: custom.is_some_and(|t| !t.trim().is_empty()),
                cwd: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                created_at: secs(r.get::<_, Option<i64>>(4)?.unwrap_or(0)),
                updated_at: secs(r.get::<_, Option<i64>>(5)?.unwrap_or(0)),
                discarded: r.get::<_, Option<i64>>(6)?.is_some(),
                automation: r.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
            },
        ))
    });
    match rows {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => HashMap::new(),
    }
}

/// What the index knows that the transcript does not.
fn apply(session: &mut Session, row: &IndexRow) {
    if !row.title.is_empty() {
        session.title = row.title.clone();
        session.title_source = if row.custom_title {
            TitleSource::Custom
        } else {
            TitleSource::Client
        };
    }
    if !row.cwd.is_empty() && session.project.path != row.cwd {
        session.project = project_ref(&row.cwd, None);
    }
    if row.created_at > 0 {
        session.created_at = row.created_at;
    }
    if row.updated_at > 0 {
        session.updated_at = session.updated_at.max(row.updated_at);
    }
    if row.automation {
        session.client = ClientTag::Automation;
    } else if session.client == ClientTag::Terminal {
        // The desktop app writes the transcript through the same engine the CLI uses, so
        // the transcript calls every conversation a terminal one.
        session.client = ClientTag::Desktop;
    }
    // A conversation the user deleted in the app keeps its transcript; it is not gone, and
    // it is not a live conversation either.
    if row.discarded {
        session.status = SessionStatus::Discarded;
    } else if !session.project.exists {
        session.status = SessionStatus::Orphaned;
    }
}

pub fn load(root: &Path, agent: Agent) -> Result<Vec<Session>, String> {
    let mut sessions = super::codebuddy_catalog::load_as(root, agent)?;
    let index = index(root);
    for session in &mut sessions {
        if let Some(row) = index.get(&session.native_id) {
            apply(session, row);
        }
    }
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const LINES: &str = concat!(
        r#"{"timestamp":1785661850059,"type":"message","role":"user","content":[{"type":"input_text","text":"find me a knowledge base"}],"providerData":{"agent":"cli"},"sessionId":"s-1","cwd":"C:\\Users\\simpl\\WorkBuddy\\2026-08-02"}"#,
        "\n",
        r#"{"timestamp":1785661852291,"type":"ai-title","aiTitle":"Open-source knowledge bases","sessionId":"s-1"}"#,
        "\n",
    );

    fn store(dir: &Path, rows: &[(&str, Option<&str>, bool, bool)]) {
        let project = dir.join("projects").join("c-Users-simpl-WorkBuddy");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("s-1.jsonl"), LINES).unwrap();
        let conn = Connection::open(dir.join(DB_NAME)).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, cwd TEXT, user_id TEXT, title TEXT,
                custom_title TEXT, status TEXT, created_at INTEGER, updated_at INTEGER,
                deleted_at INTEGER, is_playground INTEGER, source_mode TEXT,
                is_background_automation INTEGER);",
        )
        .unwrap();
        for (id, custom, discarded, automation) in rows {
            conn.execute(
                "INSERT INTO sessions (id, cwd, title, custom_title, created_at, updated_at,
                    deleted_at, is_background_automation)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    id,
                    "D:\\work",
                    "Knowledge bases, indexed",
                    custom,
                    1_785_661_000_000i64,
                    1_785_662_000_000i64,
                    discarded.then_some(1_785_663_000_000i64),
                    i64::from(*automation),
                ],
            )
            .unwrap();
        }
    }

    #[test]
    fn the_index_names_the_conversation_the_way_the_app_does() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), &[("s-1", None, false, false)]);
        let sessions = load(dir.path(), Agent::WorkBuddy).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.id, "workbuddy:s-1");
        assert_eq!(session.agent, Agent::WorkBuddy);
        assert_eq!(session.title, "Knowledge bases, indexed");
        assert_eq!(session.title_source, TitleSource::Client);
        // The transcript calls it a terminal session; the index says it came from the app.
        assert_eq!(session.client, ClientTag::Desktop);
        assert_eq!(session.project.path, "D:\\work");
        assert_eq!(session.created_at, 1_785_661_000);
    }

    #[test]
    fn a_conversation_deleted_in_the_app_is_discarded_not_gone() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), &[("s-1", Some("My own title"), true, false)]);
        let session = &load(dir.path(), Agent::WorkBuddyAi).unwrap()[0];
        assert_eq!(session.id, "workbuddy-ai:s-1");
        assert_eq!(session.status, SessionStatus::Discarded);
        assert_eq!(session.title, "My own title");
        assert_eq!(session.title_source, TitleSource::Custom);
        assert!(session.bytes > 0, "the transcript is still on disk");
    }

    #[test]
    fn an_automation_run_is_tagged_as_one() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), &[("s-1", None, false, true)]);
        let session = &load(dir.path(), Agent::WorkBuddy).unwrap()[0];
        assert_eq!(session.client, ClientTag::Automation);
    }

    /// A transcript the index does not mention is still a conversation.
    #[test]
    fn a_transcript_without_an_index_row_still_lists() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects").join("c-Users-simpl-WorkBuddy");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("s-9.jsonl"), LINES).unwrap();
        let sessions = load(dir.path(), Agent::WorkBuddy).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Open-source knowledge bases");
        assert_eq!(sessions[0].status, SessionStatus::Orphaned);
    }

    #[test]
    fn a_missing_store_is_not_an_error_to_show() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(dir.path(), Agent::WorkBuddy).unwrap_err(),
            "E_SOURCE_MISSING"
        );
    }
}
