use super::model::*;
use super::project::{project_key, project_ref};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

struct Row {
    id: String,
    rollout: String,
    created: u64,
    updated: u64,
    source: String,
    cwd: String,
    title: String,
    archived: bool,
    name: Option<String>,
    originator: Option<String>,
    thread_source: String,
    project_id: Option<String>,
    pinned: bool,
}

fn columns(conn: &Connection) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(threads)")
        .map_err(super::err)?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(super::err)?
        .filter_map(Result::ok)
        .collect();
    Ok(names)
}

fn rows(conn: &Connection) -> Result<Vec<Row>, String> {
    let have = columns(conn)?;
    if have.is_empty() {
        return Err("E_SOURCE_MISSING".into());
    }
    // Optional columns differ between Codex versions.
    let col = |name: &str| {
        if have.contains(name) {
            name.to_string()
        } else {
            "NULL".to_string()
        }
    };
    let sql = format!(
        "SELECT id, rollout_path, created_at, updated_at, source, cwd, title, archived, {}, {}, {}, {}, {} FROM threads",
        col("name"),
        col("originator"),
        col("thread_source"),
        col("project_id"),
        col("is_pinned")
    );
    let mut stmt = conn.prepare(&sql).map_err(super::err)?;
    let mapped = stmt
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                rollout: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                created: r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
                updated: r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64,
                source: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                cwd: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                title: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                archived: r.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
                name: r.get(8)?,
                originator: r.get(9)?,
                thread_source: r
                    .get::<_, Option<String>>(10)?
                    .unwrap_or_else(|| "user".into()),
                project_id: r.get(11)?,
                pinned: r.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
            })
        })
        .map_err(super::err)?;
    Ok(mapped.filter_map(Result::ok).collect())
}

fn pairs(conn: &Connection, sql: &str) -> Vec<(String, String)> {
    conn.prepare(sql)
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|m| m.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
}

fn is_child(row: &Row) -> bool {
    matches!(row.thread_source.as_str(), "subagent" | "guardian_review")
        || row.source.starts_with('{')
}

fn parent_of(row: &Row, edges: &HashMap<String, String>) -> Option<String> {
    edges.get(&row.id).cloned().or_else(|| {
        serde_json::from_str::<Value>(&row.source)
            .ok()?
            .pointer("/subagent/thread_spawn/parent_thread_id")?
            .as_str()
            .map(str::to_string)
    })
}

fn client(row: &Row) -> ClientTag {
    if row.originator.as_deref() == Some("Codex Desktop") {
        return ClientTag::Desktop;
    }
    match row.source.as_str() {
        "exec" => ClientTag::Automation,
        "vscode" => ClientTag::Ide,
        "cli" => ClientTag::Terminal,
        _ => ClientTag::Unknown,
    }
}

fn title(row: &Row) -> (String, TitleSource) {
    if let Some(name) = row.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        return (name.to_string(), TitleSource::Client);
    }
    let first = row
        .title
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    (first.chars().take(80).collect(), TitleSource::FirstMessage)
}

fn child_title(row: &Row) -> String {
    let nickname = serde_json::from_str::<Value>(&row.source)
        .ok()
        .and_then(|v| {
            v.pointer("/subagent/thread_spawn/agent_nickname")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let text = title(row).0;
    match nickname {
        Some(nick) if !text.is_empty() => format!("{nick}：{text}"),
        Some(nick) => nick,
        None => text,
    }
}

fn file_size(path: &str) -> u64 {
    std::fs::metadata(path.trim_start_matches(r"\\?\"))
        .map(|m| m.len())
        .unwrap_or(0)
}

pub fn sessions_from_db(conn: &Connection) -> Result<Vec<Session>, String> {
    let rows = rows(conn)?;
    let edges: HashMap<String, String> = pairs(
        conn,
        "SELECT child_thread_id, parent_thread_id FROM thread_spawn_edges",
    )
    .into_iter()
    .collect();
    let project_names: HashMap<String, String> = pairs(conn, "SELECT id, name FROM projects")
        .into_iter()
        .collect();
    let roots = pairs(
        conn,
        "SELECT r.path, p.name FROM project_roots r JOIN projects p ON p.id = r.project_id",
    );
    let name_for = |row: &Row| -> Option<String> {
        if let Some(name) = row.project_id.as_ref().and_then(|id| project_names.get(id)) {
            return Some(name.clone());
        }
        let cwd = project_key(&row.cwd);
        roots
            .iter()
            .filter(|(path, _)| {
                let root = project_key(path);
                !root.is_empty() && (cwd == root || cwd.starts_with(&format!("{root}\\")))
            })
            .max_by_key(|(path, _)| path.len())
            .map(|(_, name)| name.clone())
    };

    let by_id: HashMap<&str, &Row> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
    // Follow parents up to the top-level thread that is shown as a session.
    let top_parent = |row: &Row| -> Option<String> {
        let mut current = row;
        let mut seen = HashSet::new();
        loop {
            if !is_child(current) {
                return (current.id != row.id).then(|| current.id.clone());
            }
            if !seen.insert(current.id.clone()) {
                return None;
            }
            let parent = parent_of(current, &edges)?;
            current = by_id.get(parent.as_str())?;
        }
    };

    let mut children: HashMap<String, Vec<ChildSummary>> = HashMap::new();
    let mut sessions = Vec::new();
    for row in &rows {
        let child = is_child(row);
        if child {
            if let Some(parent) = top_parent(row) {
                children.entry(parent).or_default().push(ChildSummary {
                    id: format!("codex:{}", row.id),
                    kind: row.thread_source.clone(),
                    title: child_title(row),
                    bytes: file_size(&row.rollout),
                    path: row.rollout.trim_start_matches(r"\\?\").to_string(),
                });
                continue;
            }
        }
        let (title, title_source) = title(row);
        let project = project_ref(&row.cwd, name_for(row).as_deref());
        let status = if row.archived {
            SessionStatus::Archived
        } else if !project.exists {
            SessionStatus::Orphaned
        } else {
            SessionStatus::Active
        };
        sessions.push(Session {
            id: format!("codex:{}", row.id),
            agent: Agent::Codex,
            native_id: row.id.clone(),
            title,
            title_source,
            project,
            // Sub-agent and review threads without a parent are background noise.
            client: if child {
                ClientTag::Automation
            } else {
                client(row)
            },
            created_at: row.created,
            updated_at: row.updated,
            archived: row.archived,
            pinned: row.pinned,
            status,
            children: Vec::new(),
            bytes: file_size(&row.rollout),
            path: row.rollout.trim_start_matches(r"\\?\").to_string(),
            in_desktop_index: false,
            parent_missing: child,
            favorite: false,
            summary: None,
            summary_stale: false,
            summary_by: String::new(),
            summary_at: 0,
            copies: Vec::new(),
        });
    }
    for session in &mut sessions {
        if let Some(kids) = children.remove(&session.native_id) {
            session.bytes += kids.iter().map(|k| k.bytes).sum::<u64>();
            session.children = kids;
        }
    }
    Ok(sessions)
}

/// Newest `state_<n>.sqlite` under the Codex root, opened read-only.
pub fn load(root: &Path) -> Result<Vec<Session>, String> {
    let mut databases: Vec<(u64, std::path::PathBuf)> = std::fs::read_dir(root)
        .map_err(|_| "E_SOURCE_MISSING".to_string())?
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let number = name.strip_prefix("state_")?.strip_suffix(".sqlite")?;
            Some((number.parse().ok()?, p))
        })
        .collect();
    databases.sort();
    let (_, path) = databases.pop().ok_or("E_SOURCE_MISSING")?;
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(super::err)?;
    conn.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(super::err)?;
    sessions_from_db(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn insert(
        conn: &Connection,
        id: &str,
        path: String,
        source: &str,
        name: Option<&str>,
        title: &str,
        archived: i64,
        originator: Option<&str>,
        thread_source: &str,
        pinned: i64,
        cwd: &str,
    ) {
        conn.execute(
            "INSERT INTO threads VALUES (?1,?2,100,200,?3,?4,?5,?6,?7,?8,?9,NULL,?10)",
            rusqlite::params![
                id,
                path,
                source,
                cwd,
                title,
                archived,
                name,
                originator,
                thread_source,
                pinned
            ],
        )
        .unwrap();
    }

    fn fixture(dir: &Path) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, created_at INTEGER, updated_at INTEGER,
               source TEXT, cwd TEXT, title TEXT, archived INTEGER, name TEXT, originator TEXT,
               thread_source TEXT, project_id TEXT, is_pinned INTEGER);
             CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT PRIMARY KEY, status TEXT);
             CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT);
             CREATE TABLE project_roots (project_id TEXT, position INTEGER, path TEXT);",
        )
        .unwrap();
        let project = dir.join("app");
        std::fs::create_dir(&project).unwrap();
        let rollout = |name: &str, size: usize| {
            let path = dir.join(name);
            std::fs::write(&path, vec![b'x'; size]).unwrap();
            path.to_string_lossy().into_owned()
        };
        let cwd = project.to_string_lossy().into_owned();
        conn.execute("INSERT INTO projects VALUES ('p1','My app')", [])
            .unwrap();
        conn.execute("INSERT INTO project_roots VALUES ('p1',0,?1)", [&cwd])
            .unwrap();
        insert(
            &conn,
            "main",
            rollout("main.jsonl", 100),
            "vscode",
            Some("Short title"),
            "A long first message\nsecond line",
            0,
            Some("Codex Desktop"),
            "user",
            1,
            &cwd,
        );
        insert(
            &conn,
            "child",
            rollout("child.jsonl", 10),
            r#"{"subagent":{"thread_spawn":{"parent_thread_id":"main","agent_nickname":"Ohm"}}}"#,
            None,
            "sub",
            0,
            None,
            "subagent",
            0,
            &cwd,
        );
        insert(
            &conn,
            "grandchild",
            rollout("grand.jsonl", 1),
            "{\"subagent\":{\"other\":\"x\"}}",
            None,
            "deep",
            0,
            None,
            "subagent",
            0,
            &cwd,
        );
        conn.execute(
            "INSERT INTO thread_spawn_edges VALUES ('child','grandchild','done')",
            [],
        )
        .unwrap();
        insert(
            &conn,
            "exec",
            rollout("exec.jsonl", 5),
            "exec",
            None,
            "run tests",
            0,
            None,
            "user",
            0,
            &cwd,
        );
        insert(
            &conn,
            "orphan-child",
            rollout("oc.jsonl", 5),
            r#"{"subagent":{"other":"guardian"}}"#,
            None,
            "review",
            0,
            None,
            "guardian_review",
            0,
            &cwd,
        );
        insert(
            &conn,
            "gone",
            rollout("gone.jsonl", 5),
            "cli",
            None,
            "old",
            1,
            None,
            "user",
            0,
            r"Z:\missing\project",
        );
        conn
    }

    #[test]
    fn codex_threads_become_user_facing_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let conn = fixture(dir.path());
        let sessions = sessions_from_db(&conn).unwrap();
        let ids: Vec<_> = sessions.iter().map(|s| s.native_id.as_str()).collect();
        assert_eq!(ids.len(), 4, "children fold into main: {ids:?}");

        let main = sessions.iter().find(|s| s.native_id == "main").unwrap();
        assert_eq!(main.title, "Short title");
        assert_eq!(main.title_source, TitleSource::Client);
        assert_eq!(main.client, ClientTag::Desktop);
        assert!(main.pinned);
        assert_eq!(main.project.name, "My app");
        assert_eq!(main.children.len(), 2);
        assert!(main.children.iter().any(|c| c.title == "Ohm：sub"));
        assert_eq!(main.bytes, 111);
        assert_eq!(main.status, SessionStatus::Active);

        let exec = sessions.iter().find(|s| s.native_id == "exec").unwrap();
        assert_eq!(exec.client, ClientTag::Automation);

        let review = sessions
            .iter()
            .find(|s| s.native_id == "orphan-child")
            .unwrap();
        assert!(review.parent_missing);
        assert_eq!(review.client, ClientTag::Automation);

        let gone = sessions.iter().find(|s| s.native_id == "gone").unwrap();
        assert_eq!(gone.client, ClientTag::Terminal);
        assert_eq!(
            gone.status,
            SessionStatus::Archived,
            "archived wins over orphaned"
        );
        assert_eq!(gone.title, "old");
    }

    #[test]
    fn missing_optional_columns_are_tolerated() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT, rollout_path TEXT, created_at INTEGER, updated_at INTEGER,
               source TEXT, cwd TEXT, title TEXT, archived INTEGER);
             INSERT INTO threads VALUES ('a','',1,2,'cli','','hello',0);",
        )
        .unwrap();
        let sessions = sessions_from_db(&conn).unwrap();
        assert_eq!(sessions[0].title, "hello");
        assert_eq!(sessions[0].client, ClientTag::Terminal);
    }
}
