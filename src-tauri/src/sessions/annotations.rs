//! Stacker's own notes about sessions: favorites and saved summaries.
use super::model::{Roots, Session};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Stacker")
        .join(if cfg!(debug_assertions) {
            "dev"
        } else {
            "stable"
        })
        .join("conversations")
}

pub fn connect() -> Result<Connection, String> {
    connect_at(&root())
}

pub fn connect_at(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir).map_err(|_| "E_STORAGE".to_string())?;
    let conn =
        Connection::open(dir.join("sessions.sqlite3")).map_err(|_| "E_STORAGE".to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "E_STORAGE".to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS session_notes (id TEXT PRIMARY KEY, favorite INTEGER NOT NULL DEFAULT 0,
             summary TEXT NOT NULL DEFAULT '', summary_fingerprint TEXT NOT NULL DEFAULT '');
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(|_| "E_STORAGE".to_string())?;
    add_summary_columns(&conn)?;
    migrate(&conn, dir)?;
    Ok(conn)
}

pub(crate) fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
        r.get(0)
    })
    .optional()
    .ok()
    .flatten()
}

pub(crate) fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|_| "E_STORAGE".to_string())
}

fn add_summary_columns(conn: &Connection) -> Result<(), String> {
    let have: Vec<String> = conn
        .prepare("PRAGMA table_info(session_notes)")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(1))
                .map(|m| m.filter_map(Result::ok).collect())
        })
        .map_err(|_| "E_STORAGE".to_string())?;
    for (name, sql) in [
        (
            "summary_by",
            "ALTER TABLE session_notes ADD COLUMN summary_by TEXT NOT NULL DEFAULT ''",
        ),
        (
            "summary_at",
            "ALTER TABLE session_notes ADD COLUMN summary_at INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if !have.iter().any(|c| c == name) {
            conn.execute(sql, []).map_err(|_| "E_STORAGE".to_string())?;
        }
    }
    Ok(())
}

pub fn save_summary(
    conn: &Connection,
    id: &str,
    text: &str,
    fingerprint: &str,
    by: &str,
    at: u64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO session_notes(id,summary,summary_fingerprint,summary_by,summary_at) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(id) DO UPDATE SET summary=excluded.summary,summary_fingerprint=excluded.summary_fingerprint,
         summary_by=excluded.summary_by,summary_at=excluded.summary_at",
        params![id, text, fingerprint, by, at as i64],
    )
    .map(|_| ())
    .map_err(|_| "E_STORAGE".to_string())
}

/// Old conversation-index ids that still point at a session.
fn migrated_id(old: &str) -> Option<String> {
    let (source, native) = old.split_once(':')?;
    match source {
        "codex" | "codex-local" => Some(format!("codex:{native}")),
        "claude-cli" => Some(format!("claude:{native}")),
        _ => None,
    }
}

/// Copies favorites and summaries from the old conversation index once.
fn migrate(conn: &Connection, dir: &Path) -> Result<(), String> {
    if setting(conn, "migrated_v1").is_some() {
        return Ok(());
    }
    let old = dir.join("index.sqlite3");
    if old.is_file() {
        if let Ok(old) = Connection::open_with_flags(&old, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            let rows: Vec<(String, i64, String, String)> = old
                .prepare(
                    "SELECT id, favorite, summary, summary_fingerprint FROM annotations WHERE favorite=1 OR summary!=''",
                )
                .and_then(|mut s| {
                    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                        .map(|m| m.filter_map(Result::ok).collect())
                })
                .unwrap_or_default();
            for (id, favorite, summary, fingerprint) in rows {
                let Some(new_id) = migrated_id(&id) else {
                    continue;
                };
                conn.execute(
                    "INSERT OR IGNORE INTO session_notes(id,favorite,summary,summary_fingerprint) VALUES(?1,?2,?3,?4)",
                    params![new_id, favorite, summary, fingerprint],
                )
                .map_err(|_| "E_STORAGE".to_string())?;
            }
        }
    }
    set_setting(conn, "migrated_v1", "1")
}

/// Same format as the old index, so migrated summaries stay current.
pub fn quick_fingerprint(path: &Path) -> String {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| {
            let nanos = m
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos();
            Some(format!("{}:{nanos}", m.len()))
        })
        .unwrap_or_default()
}

pub fn apply(conn: &Connection, sessions: &mut [Session]) {
    let Ok(mut stmt) =
        conn.prepare("SELECT favorite, summary, summary_fingerprint, summary_by, summary_at FROM session_notes WHERE id=?")
    else {
        return;
    };
    for s in sessions.iter_mut() {
        let row = stmt
            .query_row([&s.id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .optional();
        if let Ok(Some((favorite, summary, fingerprint, by, at))) = row {
            s.favorite = favorite != 0;
            if !summary.is_empty() {
                s.summary_by = by;
                s.summary_at = at.max(0) as u64;
                s.summary_stale = fingerprint != quick_fingerprint(Path::new(&s.path));
                s.summary = Some(summary);
            }
        }
    }
}

pub fn set_favorite(conn: &Connection, ids: &[String], favorite: bool) -> Result<(), String> {
    for id in ids {
        conn.execute(
            "INSERT INTO session_notes(id,favorite) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET favorite=excluded.favorite",
            params![id, i64::from(favorite)],
        )
        .map_err(|_| "E_STORAGE".to_string())?;
    }
    Ok(())
}

pub fn forget(conn: &Connection, ids: &[String]) -> Result<(), String> {
    for id in ids {
        conn.execute("DELETE FROM session_notes WHERE id=?", [id])
            .map_err(|_| "E_STORAGE".to_string())?;
    }
    Ok(())
}

pub fn roots(conn: &Connection) -> Roots {
    setting(conn, "roots")
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

pub fn set_roots(conn: &Connection, roots: &Roots) -> Result<(), String> {
    set_setting(
        conn,
        "roots",
        &serde_json::to_string(roots).map_err(super::err)?,
    )
}

#[cfg(test)]
mod tests {
    use super::super::catalog::tests_support::session;
    use super::*;

    #[test]
    fn favorites_and_roots_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let conn = connect_at(dir.path()).unwrap();
        set_favorite(&conn, &["codex:a".into()], true).unwrap();
        let saved = Roots {
            codex: "C:/x".into(),
            ..Default::default()
        };
        set_roots(&conn, &saved).unwrap();
        assert_eq!(roots(&conn), saved);
        let mut sessions = vec![session("codex:a")];
        apply(&conn, &mut sessions);
        assert!(sessions[0].favorite);
        forget(&conn, &["codex:a".into()]).unwrap();
        let mut again = vec![session("codex:a")];
        apply(&conn, &mut again);
        assert!(!again[0].favorite);
    }

    #[test]
    fn summaries_round_trip_with_their_runner() {
        let dir = tempfile::tempdir().unwrap();
        let conn = connect_at(dir.path()).unwrap();
        drop(conn);
        let conn = connect_at(dir.path()).unwrap(); // column migration is idempotent
        save_summary(&conn, "codex:a", "notes", "", "codex / default / low", 7).unwrap();
        let mut sessions = vec![session("codex:a")];
        apply(&conn, &mut sessions);
        assert_eq!(sessions[0].summary.as_deref(), Some("notes"));
        assert_eq!(sessions[0].summary_by, "codex / default / low");
        assert_eq!(sessions[0].summary_at, 7);
    }

    #[test]
    fn old_index_annotations_are_migrated_once() {
        let dir = tempfile::tempdir().unwrap();
        let old = Connection::open(dir.path().join("index.sqlite3")).unwrap();
        old.execute_batch(
            "CREATE TABLE annotations (id TEXT PRIMARY KEY, favorite INTEGER, hidden INTEGER, group_name TEXT, summary TEXT, summary_fingerprint TEXT);
             INSERT INTO annotations VALUES ('claude-cli:s1',1,0,'','要点','1:2');
             INSERT INTO annotations VALUES ('codex-local:t2',1,0,'','','');
             INSERT INTO annotations VALUES ('claude-desktop-0:z',1,0,'','','');
             INSERT INTO annotations VALUES ('codex-local:t1',0,1,'g','','');",
        )
        .unwrap();
        drop(old);
        let conn = connect_at(dir.path()).unwrap();
        let ids: Vec<String> = conn
            .prepare("SELECT id FROM session_notes ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(ids, vec!["claude:s1", "codex:t2"]);
        conn.execute("DELETE FROM session_notes", []).unwrap();
        drop(conn);
        let conn = connect_at(dir.path()).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM session_notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "migration runs only once");
    }
}
