# Session Catalog (C1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the conversation index with a session catalog that reads Codex and Claude's own metadata, shows the sessions users actually see, groups them by project, and deletes them in three modes.

**Architecture:** A new `src-tauri/src/sessions/` module builds `Session` values on demand from Codex's `state_<n>.sqlite` and from Claude's desktop index plus JSONL heads, cached in memory by file fingerprint. Stacker keeps only favorites and summaries in a small SQLite table. Deletion plans are previewed, re-verified, and executed as a background job (Codex through its App Server, Claude by removing the session's files). The old `conversations/` module and its frontend are removed.

**Tech Stack:** Rust 2021 + Tauri 2, rusqlite, serde_json, sha2; React 19 + TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-09-18-session-catalog-design.md`

## Global Constraints

- Default list shows ~31 Claude and ~70 Codex sessions on the maintainer machine; automation runs hidden by default.
- Title priority: client title (Claude desktop index `title`; Codex `name`, then `title`) → `custom-title` → `summary` → first user message (80 chars).
- Default delete mode: 精简导出后删除. Other modes: 直接删除, 完整备份后删除.
- Claude deletion only for sessions not present in the Claude desktop index; sessions written within the last 120 seconds are blocked as in use.
- Codex deletion only through the Codex App Server with Codex fully quit.
- Never follow symlinks or junctions; every touched path must be inside its agent root.
- Codex state database is opened read-only.
- Every new Chinese UI string needs an English entry in `src/en.generated.ts`; `npm run check:i18n` passes.
- Checks before each commit: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (in `src-tauri`); `npm run typecheck`, `npm run lint`, `npm run test`, `npm run check:i18n`.
- Commit messages end with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- Branch: continue on `feat/agents-rebuild`.

## Spec adjustments (applied to the spec with this plan)

- Detecting "a running `claude` process whose working directory is the session's project" needs another process's PEB, which Windows does not expose simply. Replaced by: a session whose transcript was written within the last 120 seconds is in use and blocked.
- On the maintainer machine 29 Codex sub-agent threads have no spawn edge and guardian review threads never have a parent. Showing them as ordinary sessions would bring back the noise this rebuild removes, so parentless sub-agent / review threads are tagged `Automation` (hidden by default) and flagged 父会话已不存在.

## File Map

Backend (`src-tauri/src/sessions/`):

| File | Responsibility |
| --- | --- |
| `mod.rs` | module declarations, shared helpers (`err`, `now`) |
| `model.rs` | `Agent`, `ClientTag`, `SessionStatus`, `TitleSource`, `ProjectRef`, `ChildSummary`, `Session`, `SessionQuery`, `SessionPage`, `ProjectRow`, `Roots` |
| `project.rs` | project key normalization, worktree folding, project naming |
| `roots.rs` | resolve Codex / Claude / Claude desktop index roots |
| `codex_catalog.rs` | sessions from Codex `state_<n>.sqlite` |
| `claude_catalog.rs` | sessions from Claude desktop index + JSONL heads |
| `catalog.rs` | combine agents, fingerprint cache, query filtering, project rows |
| `annotations.rs` | favorites + summaries SQLite, migration from the old index |
| `transcript.rs` | message parsing for detail view, export, full-text search (moved from `conversations/reader.rs`) |
| `export.rs` | slim Markdown export |
| `codex_rpc.rs` | Codex App Server client (moved from `conversations/codex.rs`) |
| `delete.rs` | deletion preview, blocking rules, execution job |
| `commands.rs` | Tauri commands |

Frontend (`src/features/sessions/`): `types.ts`, `api.ts`, `SessionCatalog.tsx`, `SessionList.tsx`, `ProjectList.tsx`, `SourcesPanel.tsx`, `DeleteDialog.tsx`, `SessionDetail.tsx`, `sessions.css`, `sessionsView.ts` (+ tests).

Removed: `src-tauri/src/conversations/`, `src/features/conversations/`, `docs/conversations.md` (rewritten as `docs/sessions.md`).

---

### Task 1: Session model and project keys

**Files:**
- Create: `src-tauri/src/sessions/mod.rs`, `src-tauri/src/sessions/model.rs`, `src-tauri/src/sessions/project.rs`
- Modify: `src-tauri/src/lib.rs` (`mod sessions;`), spec (adjustment above)

**Interfaces:**
- Produces: types in `model.rs` exactly as below; `project::project_key(path: &str) -> String`, `project::repository_root(path: &str) -> String`, `project::display_name(path: &str) -> String`.

- [ ] **Step 1: Write the failing tests** (`project.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_ignore_case_separators_and_prefix() {
        assert_eq!(project_key(r"\\?\D:\Projects\App\"), project_key("d:/projects/app"));
        assert_eq!(project_key(""), "");
    }

    #[test]
    fn claude_worktrees_fold_into_their_repository() {
        assert_eq!(
            repository_root(r"E:\VibeCoding\repo\.claude\worktrees\nexa-android-planb3"),
            r"E:\VibeCoding\repo"
        );
        assert_eq!(repository_root(r"E:\VibeCoding\repo\src"), r"E:\VibeCoding\repo\src");
    }

    #[test]
    fn display_names_use_the_last_segment() {
        assert_eq!(display_name(r"D:\Projects\rust\envswitch"), "envswitch");
        assert_eq!(display_name(""), "未知项目");
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --lib sessions::project` → compile errors.

- [ ] **Step 3: Implement**

`model.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent { Codex, Claude }

impl Agent {
    pub fn as_str(self) -> &'static str { match self { Agent::Codex => "codex", Agent::Claude => "claude" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientTag { Desktop, Terminal, Ide, Automation, Sdk, Unknown }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus { Active, Archived, Orphaned }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource { Client, Custom, Summary, FirstMessage }

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef { pub key: String, pub name: String, pub path: String, pub exists: bool }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildSummary { pub id: String, pub kind: String, pub title: String, pub bytes: u64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub native_id: String,
    pub title: String,
    pub title_source: TitleSource,
    pub project: ProjectRef,
    pub client: ClientTag,
    pub created_at: u64,
    pub updated_at: u64,
    pub archived: bool,
    pub pinned: bool,
    pub status: SessionStatus,
    /// Sub-agent and review runs attached to this session.
    pub children: Vec<ChildSummary>,
    /// Transcript + children + related directories.
    pub bytes: u64,
    /// Main transcript file.
    pub path: String,
    /// Present in the Claude desktop sidebar (Claude only).
    pub in_desktop_index: bool,
    /// A sub-agent whose parent session no longer exists.
    pub parent_missing: bool,
    pub favorite: bool,
    pub summary: Option<String>,
    pub summary_stale: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionQuery {
    pub agent: String,        // "" | "codex" | "claude"
    pub project: String,      // project key
    pub status: String,       // "" | "active" | "archived" | "orphaned"
    pub client: String,       // "" | ClientTag name
    pub search: String,
    pub full_text: bool,
    pub include_automation: bool,
    pub favorites_only: bool,
    pub updated_after: u64,
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    pub items: Vec<Session>,
    pub total: usize,
    pub ids: Vec<String>,
    pub total_bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub project: ProjectRef,
    pub agents: Vec<Agent>,
    pub sessions: usize,
    pub orphans: usize,
    pub bytes: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Roots { pub codex: String, pub claude: String, pub claude_desktop_index: String }
```

`project.rs`:

```rust
/// Stable key for grouping: lowercase, `\` separators, no `\\?\` prefix or trailing slash.
pub fn project_key(path: &str) -> String {
    path.trim()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Claude worktree sessions (`<repo>\.claude\worktrees\<name>`) belong to the repository.
pub fn repository_root(path: &str) -> String {
    let normalized = path.trim_start_matches(r"\\?\").replace('/', "\\");
    match normalized.to_lowercase().find(r"\.claude\worktrees\") {
        Some(index) => normalized[..index].to_string(),
        None => normalized.trim_end_matches('\\').to_string(),
    }
}

pub fn display_name(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("未知项目")
        .to_string()
}

pub fn project_ref(path: &str, name: Option<&str>) -> super::model::ProjectRef {
    let root = repository_root(path);
    super::model::ProjectRef {
        key: project_key(&root),
        name: name.map(str::to_string).unwrap_or_else(|| display_name(&root)),
        exists: !root.is_empty() && std::path::Path::new(&root).is_dir(),
        path: root,
    }
}
```

`mod.rs`:

```rust
pub mod model;
pub mod project;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
```

Add `mod sessions;` in `lib.rs`. Until later tasks use them, add `#![allow(dead_code)]` at the top of `sessions/mod.rs`; Task 9 removes it.

- [ ] **Step 4: Run tests** → pass. Full Rust checks.
- [ ] **Step 5: Commit** — `feat: add the session catalog model and project keys`.

---

### Task 2: Codex catalog from the state database

**Files:**
- Create: `src-tauri/src/sessions/codex_catalog.rs`
- Test: same file

**Interfaces:**
- Consumes: `model::*`, `project::project_ref`, `project::project_key`.
- Produces: `codex_catalog::sessions_from_db(conn: &rusqlite::Connection) -> Result<Vec<Session>, String>` and `codex_catalog::load(root: &Path) -> Result<Vec<Session>, String>` (opens the newest `state_<n>.sqlite` read-only).

Rules (from the spec): `thread_source` `subagent` / `guardian_review` → child of the parent from `thread_spawn_edges` or the `parent_thread_id` inside `source` JSON; a child without a present parent becomes its own session with `client = Automation` and `parent_missing = true`. `source = exec` → `Automation`. `originator = "Codex Desktop"` → `Desktop`; `source = vscode` → `Ide`; `source = cli` → `Terminal`. Title: `name`, else first line of `title` (80 chars). Project name: `projects.name` via `project_id`, else the longest `project_roots.path` that prefixes `cwd`. Bytes: rollout size + children's rollout sizes. Status: `Archived` when `archived = 1`; `Orphaned` when the project path does not exist; else `Active`. Timestamps are seconds.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &std::path::Path) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, created_at INTEGER, updated_at INTEGER,
               source TEXT, cwd TEXT, title TEXT, archived INTEGER, name TEXT, originator TEXT,
               thread_source TEXT, project_id TEXT, is_pinned INTEGER);
             CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT PRIMARY KEY, status TEXT);
             CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT);
             CREATE TABLE project_roots (project_id TEXT, position INTEGER, path TEXT);",
        ).unwrap();
        let project = dir.join("app");
        std::fs::create_dir(&project).unwrap();
        let rollout = |name: &str, size: usize| {
            let path = dir.join(name);
            std::fs::write(&path, vec![b'x'; size]).unwrap();
            path.to_string_lossy().into_owned()
        };
        let cwd = project.to_string_lossy().into_owned();
        conn.execute("INSERT INTO projects VALUES ('p1','我的应用')", []).unwrap();
        conn.execute("INSERT INTO project_roots VALUES ('p1',0,?1)", [&cwd]).unwrap();
        let insert = |id: &str, path: String, source: &str, name: Option<&str>, title: &str, archived: i64, originator: Option<&str>, thread_source: &str, pinned: i64, cwd: &str| {
            conn.execute(
                "INSERT INTO threads VALUES (?1,?2,100,200,?3,?4,?5,?6,?7,?8,?9,NULL,?10)",
                rusqlite::params![id, path, source, cwd, title, archived, name, originator, thread_source, pinned],
            ).unwrap();
        };
        insert("main", rollout("main.jsonl", 100), "vscode", Some("短标题"), "很长的第一条消息\n第二行", 0, Some("Codex Desktop"), "user", 1, &cwd);
        insert("child", rollout("child.jsonl", 10), r#"{"subagent":{"thread_spawn":{"parent_thread_id":"main"}}}"#, None, "sub", 0, None, "subagent", 0, &cwd);
        insert("exec", rollout("exec.jsonl", 5), "exec", None, "run tests", 0, None, "user", 0, &cwd);
        insert("orphan-child", rollout("oc.jsonl", 5), r#"{"subagent":{"other":"guardian"}}"#, None, "review", 0, None, "guardian_review", 0, &cwd);
        insert("gone", rollout("gone.jsonl", 5), "cli", None, "old", 1, None, "user", 0, r"Z:\missing\project");
        conn
    }

    #[test]
    fn codex_threads_become_user_facing_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let conn = fixture(dir.path());
        let sessions = sessions_from_db(&conn).unwrap();
        let ids: Vec<_> = sessions.iter().map(|s| s.native_id.as_str()).collect();
        assert_eq!(ids.len(), 4, "child is folded into main: {ids:?}");

        let main = sessions.iter().find(|s| s.native_id == "main").unwrap();
        assert_eq!(main.title, "短标题");
        assert_eq!(main.title_source, TitleSource::Client);
        assert_eq!(main.client, ClientTag::Desktop);
        assert!(main.pinned);
        assert_eq!(main.project.name, "我的应用");
        assert_eq!(main.children.len(), 1);
        assert_eq!(main.bytes, 110);
        assert_eq!(main.status, SessionStatus::Active);

        let exec = sessions.iter().find(|s| s.native_id == "exec").unwrap();
        assert_eq!(exec.client, ClientTag::Automation);

        let review = sessions.iter().find(|s| s.native_id == "orphan-child").unwrap();
        assert!(review.parent_missing);
        assert_eq!(review.client, ClientTag::Automation);

        let gone = sessions.iter().find(|s| s.native_id == "gone").unwrap();
        assert_eq!(gone.client, ClientTag::Terminal);
        assert_eq!(gone.status, SessionStatus::Archived, "archived wins over orphaned");
        assert_eq!(gone.title, "old");
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --lib sessions::codex_catalog`.

- [ ] **Step 3: Implement**

```rust
use super::model::*;
use super::project::{project_key, project_ref};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::HashMap;
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

fn columns(conn: &Connection) -> Result<std::collections::HashSet<String>, String> {
    let mut stmt = conn.prepare("PRAGMA table_info(threads)").map_err(super::err)?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(super::err)?
        .filter_map(Result::ok)
        .collect();
    Ok(names)
}

fn rows(conn: &Connection) -> Result<Vec<Row>, String> {
    let have = columns(conn)?;
    let col = |name: &str| if have.contains(name) { name.to_string() } else { "NULL".to_string() };
    let sql = format!(
        "SELECT id, rollout_path, created_at, updated_at, source, cwd, title, archived, {}, {}, {}, {}, {} FROM threads",
        col("name"), col("originator"), col("thread_source"), col("project_id"), col("is_pinned")
    );
    let mut stmt = conn.prepare(&sql).map_err(super::err)?;
    let mapped = stmt
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                rollout: r.get::<_, String>(1)?,
                created: r.get::<_, i64>(2)?.max(0) as u64,
                updated: r.get::<_, i64>(3)?.max(0) as u64,
                source: r.get(4)?,
                cwd: r.get(5)?,
                title: r.get(6)?,
                archived: r.get::<_, i64>(7)? != 0,
                name: r.get(8)?,
                originator: r.get(9)?,
                thread_source: r.get::<_, Option<String>>(10)?.unwrap_or_else(|| "user".into()),
                project_id: r.get(11)?,
                pinned: r.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
            })
        })
        .map_err(super::err)?;
    Ok(mapped.filter_map(Result::ok).collect())
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

fn is_child(row: &Row) -> bool {
    matches!(row.thread_source.as_str(), "subagent" | "guardian_review") || row.source.starts_with('{')
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
    let first = row.title.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    (first.chars().take(80).collect(), TitleSource::FirstMessage)
}

fn file_size(path: &str) -> u64 {
    std::fs::metadata(path.trim_start_matches(r"\\?\")).map(|m| m.len()).unwrap_or(0)
}

pub fn sessions_from_db(conn: &Connection) -> Result<Vec<Session>, String> {
    let rows = rows(conn)?;
    let edges: HashMap<String, String> = conn
        .prepare("SELECT child_thread_id, parent_thread_id FROM thread_spawn_edges")
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|m| m.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let project_names: HashMap<String, String> = conn
        .prepare("SELECT id, name FROM projects")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|m| m.filter_map(Result::ok).collect()))
        .unwrap_or_default();
    let roots: Vec<(String, String)> = conn
        .prepare("SELECT r.path, p.name FROM project_roots r JOIN projects p ON p.id = r.project_id")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|m| m.filter_map(Result::ok).collect()))
        .unwrap_or_default();
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

    let ids: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let mut children: HashMap<String, Vec<ChildSummary>> = HashMap::new();
    let mut sessions = Vec::new();
    for row in &rows {
        let parent = if is_child(row) { parent_of(row, &edges) } else { None };
        if let Some(parent) = parent.filter(|p| ids.contains(p.as_str())) {
            children.entry(parent).or_default().push(ChildSummary {
                id: format!("codex:{}", row.id),
                kind: row.thread_source.clone(),
                title: title(row).0,
                bytes: file_size(&row.rollout),
            });
            continue;
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
        let orphan_child = is_child(row);
        sessions.push(Session {
            id: format!("codex:{}", row.id),
            agent: Agent::Codex,
            native_id: row.id.clone(),
            title,
            title_source,
            project,
            client: if orphan_child { ClientTag::Automation } else { client(row) },
            created_at: row.created,
            updated_at: row.updated,
            archived: row.archived,
            pinned: row.pinned,
            status,
            children: Vec::new(),
            bytes: file_size(&row.rollout),
            path: row.rollout.trim_start_matches(r"\\?\").to_string(),
            in_desktop_index: false,
            parent_missing: orphan_child,
            favorite: false,
            summary: None,
            summary_stale: false,
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
    let mut databases: Vec<_> = std::fs::read_dir(root)
        .map_err(|_| "E_SOURCE_MISSING".to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("state_"))
                && p.extension().is_some_and(|e| e == "sqlite")
        })
        .collect();
    databases.sort();
    let path = databases.pop().ok_or("E_SOURCE_MISSING")?;
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(super::err)?;
    conn.busy_timeout(std::time::Duration::from_secs(3)).map_err(super::err)?;
    sessions_from_db(&conn)
}
```

Register `pub mod codex_catalog;` in `sessions/mod.rs`.

- [ ] **Step 4: Run tests and checks** → pass.
- [ ] **Step 5: Commit** — `feat: read Codex sessions from its state database`.

---

### Task 3: Claude catalog from the desktop index and transcript heads

**Files:**
- Create: `src-tauri/src/sessions/claude_catalog.rs`

**Interfaces:**
- Produces:
  - `claude_catalog::DesktopEntry { cli_session_id: String, title: String, archived: bool, created_ms: u64, last_activity_ms: u64 }`
  - `claude_catalog::read_desktop_index(dir: &Path) -> HashMap<String, DesktopEntry>` (keys: `cliSessionId`; scans `<dir>/*/*/local_*.json`)
  - `claude_catalog::Head { session_id, cwd, entrypoint, custom_title, summary, first_user_message, worktree_cwd: Option<String> }`
  - `claude_catalog::read_head(path: &Path) -> Result<Head, String>` (≤ 256 lines / 1 MB)
  - `claude_catalog::load(root: &Path, desktop_index: &Path) -> Result<Vec<Session>, String>`
  - `claude_catalog::related_paths(root: &Path, transcript: &Path, session_id: &str) -> Vec<PathBuf>` (transcript, same-named dir, `file-history/<id>`, `session-env/<id>` — only those that exist)

Rules: title = desktop index `title` (Client) → `custom-title.json` / `custom-title` record (Custom) → `summary` record (Summary) → first user message (FirstMessage). Client: `claude-desktop` → Desktop; `cli` → Terminal; `sdk*` → Sdk; missing → Unknown. `Orphaned` when `entrypoint = claude-desktop` and not in the desktop index, or the project path is missing; `Archived` when the index says `isArchived`. Children: `<sessionId>/subagents/*.jsonl`. Bytes: sum of `related_paths` sizes (recursive, links skipped). Project path: the `worktree-state` / `relocated` original cwd if present, else `cwd`, folded by `project_ref`. Times: index ms → seconds, else file mtime.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn line(v: serde_json::Value) -> String {
        format!("{v}\n")
    }

    #[test]
    fn claude_sessions_follow_the_desktop_app() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".claude");
        let project = home.path().join("repo");
        fs::create_dir_all(&project).unwrap();
        let cwd = project.to_string_lossy().into_owned();
        let slug = root.join("projects").join("repo");
        fs::create_dir_all(slug.join("s-desktop").join("subagents")).unwrap();
        fs::create_dir_all(&slug).unwrap();
        let user = |sid: &str, entry: &str, text: &str| line(serde_json::json!({"type":"user","sessionId":sid,"cwd":cwd,"entrypoint":entry,"message":{"content":text}}));
        fs::write(slug.join("s-desktop.jsonl"), user("s-desktop", "claude-desktop", "hello")).unwrap();
        fs::write(slug.join("s-desktop").join("subagents").join("agent-1.jsonl"), user("s-desktop", "claude-desktop", "sub")).unwrap();
        fs::write(slug.join("s-desktop").join("custom-title.json"), r#"{"customTitle":"自定义"}"#).unwrap();
        fs::write(slug.join("s-orphan.jsonl"), user("s-orphan", "claude-desktop", "gone from sidebar")).unwrap();
        fs::write(slug.join("s-cli.jsonl"), format!("{}{}", user("s-cli", "cli", "first question"), line(serde_json::json!({"type":"custom-title","customTitle":"CLI 标题","sessionId":"s-cli"})))).unwrap();
        fs::create_dir_all(root.join("file-history").join("s-cli")).unwrap();
        fs::write(root.join("file-history").join("s-cli").join("a"), b"12345").unwrap();

        let index = home.path().join("claude-code-sessions").join("acct").join("org");
        fs::create_dir_all(&index).unwrap();
        fs::write(index.join("local_1.json"), serde_json::json!({"cliSessionId":"s-desktop","title":"桌面标题","isArchived":true,"createdAt":1_000_000u64,"lastActivityAt":2_000_000u64}).to_string()).unwrap();

        let sessions = load(&root, &home.path().join("claude-code-sessions")).unwrap();
        assert_eq!(sessions.len(), 3, "sub-agents are children, not sessions");

        let desktop = sessions.iter().find(|s| s.native_id == "s-desktop").unwrap();
        assert_eq!(desktop.title, "桌面标题");
        assert_eq!(desktop.title_source, TitleSource::Client);
        assert_eq!(desktop.client, ClientTag::Desktop);
        assert!(desktop.in_desktop_index);
        assert_eq!(desktop.status, SessionStatus::Archived);
        assert_eq!(desktop.children.len(), 1);
        assert_eq!(desktop.updated_at, 2_000);

        let orphan = sessions.iter().find(|s| s.native_id == "s-orphan").unwrap();
        assert_eq!(orphan.status, SessionStatus::Orphaned);
        assert!(!orphan.in_desktop_index);

        let cli = sessions.iter().find(|s| s.native_id == "s-cli").unwrap();
        assert_eq!(cli.title, "CLI 标题");
        assert_eq!(cli.title_source, TitleSource::Custom);
        assert_eq!(cli.client, ClientTag::Terminal);
        assert_eq!(cli.status, SessionStatus::Active);
        let related = related_paths(&root, std::path::Path::new(&cli.path), "s-cli");
        assert!(related.iter().any(|p| p.ends_with(std::path::Path::new("file-history").join("s-cli"))));
        assert!(cli.bytes >= 5);
    }

    #[test]
    fn first_message_fallback_skips_system_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        fs::write(&path, format!(
            "{}{}",
            line(serde_json::json!({"type":"user","sessionId":"s","message":{"content":"<environment_context>x</environment_context>"}})),
            line(serde_json::json!({"type":"user","sessionId":"s","message":{"content":[{"type":"text","text":"真正的问题\n第二行"}]}}))
        )).unwrap();
        let head = read_head(&path).unwrap();
        assert_eq!(head.first_user_message.as_deref(), Some("真正的问题"));
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --lib sessions::claude_catalog`.

- [ ] **Step 3: Implement**

```rust
use super::model::*;
use super::project::project_ref;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

const HEAD_LINES: usize = 256;
const HEAD_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub struct DesktopEntry {
    pub cli_session_id: String,
    pub title: String,
    pub archived: bool,
    pub created_ms: u64,
    pub last_activity_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Head {
    pub session_id: String,
    pub cwd: String,
    pub entrypoint: String,
    pub custom_title: Option<String>,
    pub summary: Option<String>,
    pub first_user_message: Option<String>,
    pub worktree_cwd: Option<String>,
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return true;
            }
        }
        m.file_type().is_symlink()
    }).unwrap_or(true)
}

pub fn read_desktop_index(dir: &Path) -> HashMap<String, DesktopEntry> {
    let mut entries = HashMap::new();
    let Ok(accounts) = fs::read_dir(dir) else { return entries };
    for account in accounts.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !is_link(p)) {
        let Ok(orgs) = fs::read_dir(&account) else { continue };
        for org in orgs.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !is_link(p)) {
            let Ok(files) = fs::read_dir(&org) else { continue };
            for file in files.flatten().map(|e| e.path()) {
                let is_local = file.file_name().is_some_and(|n| n.to_string_lossy().starts_with("local_"))
                    && file.extension().is_some_and(|e| e == "json");
                if !is_local || is_link(&file) {
                    continue;
                }
                let Ok(v) = fs::read(&file).map_err(|_| ()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|_| ())) else { continue };
                let Some(id) = v.get("cliSessionId").and_then(Value::as_str) else { continue };
                entries.insert(id.to_string(), DesktopEntry {
                    cli_session_id: id.to_string(),
                    title: v.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                    archived: v.get("isArchived").and_then(Value::as_bool).unwrap_or(false),
                    created_ms: v.get("createdAt").and_then(Value::as_u64).unwrap_or(0),
                    last_activity_ms: v.get("lastActivityAt").and_then(Value::as_u64).unwrap_or(0),
                });
            }
        }
    }
    entries
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn is_context(text: &str) -> bool {
    let t = text.trim_start();
    t.is_empty() || t.starts_with('<') || t.starts_with("# AGENTS.md") || t.starts_with("Caveat:")
}

pub fn read_head(path: &Path) -> Result<Head, String> {
    let file = fs::File::open(path).map_err(super::err)?;
    let reader = BufReader::new(file.take(HEAD_BYTES));
    let mut head = Head::default();
    for line in reader.lines().take(HEAD_LINES) {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        let field = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        if head.session_id.is_empty() { head.session_id = field("sessionId"); }
        if head.cwd.is_empty() { head.cwd = field("cwd"); }
        if head.entrypoint.is_empty() { head.entrypoint = field("entrypoint"); }
        match kind {
            "custom-title" => head.custom_title = Some(field("customTitle")).filter(|t| !t.is_empty()),
            "summary" => head.summary = head.summary.take().or(Some(field("summary")).filter(|t| !t.is_empty())),
            "worktree-state" | "relocated" => {
                if head.worktree_cwd.is_none() {
                    let original = v.pointer("/worktreeState/originalCwd").or_else(|| v.get("originalCwd")).and_then(Value::as_str);
                    head.worktree_cwd = original.map(str::to_string);
                }
            }
            "user" if head.first_user_message.is_none() => {
                let text = text_of(&v["message"]["content"]);
                if !is_context(&text) {
                    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
                    head.first_user_message = Some(first.chars().take(80).collect());
                }
            }
            _ => {}
        }
    }
    Ok(head)
}

fn tree_size(path: &Path) -> u64 {
    if is_link(path) { return 0; }
    match fs::metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        Ok(m) if m.is_dir() => fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| tree_size(&e.path())).sum())
            .unwrap_or(0),
        _ => 0,
    }
}

pub fn related_paths(root: &Path, transcript: &Path, session_id: &str) -> Vec<PathBuf> {
    let mut paths = vec![transcript.to_path_buf()];
    if let Some(parent) = transcript.parent() {
        paths.push(parent.join(session_id));
    }
    paths.push(root.join("file-history").join(session_id));
    paths.push(root.join("session-env").join(session_id));
    paths.into_iter().filter(|p| p.exists() && !is_link(p)).collect()
}

fn secs(ms: u64) -> u64 { ms / 1000 }

fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load(root: &Path, desktop_index: &Path) -> Result<Vec<Session>, String> {
    let projects = root.join("projects");
    if !projects.is_dir() {
        return Err("E_SOURCE_MISSING".into());
    }
    let index = read_desktop_index(desktop_index);
    let mut sessions = Vec::new();
    for project_dir in fs::read_dir(&projects).map_err(super::err)?.flatten().map(|e| e.path()) {
        if !project_dir.is_dir() || is_link(&project_dir) { continue; }
        let Ok(files) = fs::read_dir(&project_dir) else { continue };
        for path in files.flatten().map(|e| e.path()) {
            if !path.extension().is_some_and(|e| e == "jsonl") || is_link(&path) { continue; }
            let Ok(head) = read_head(&path) else { continue };
            let session_id = if head.session_id.is_empty() {
                path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
            } else {
                head.session_id.clone()
            };
            let side_dir = project_dir.join(&session_id);
            let custom_file = fs::read(side_dir.join("custom-title.json")).ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| v.get("customTitle").and_then(Value::as_str).map(str::to_string))
                .filter(|t| !t.is_empty());
            let desktop = index.get(&session_id);
            let (title, title_source) = if let Some(t) = desktop.map(|d| d.title.clone()).filter(|t| !t.is_empty()) {
                (t, TitleSource::Client)
            } else if let Some(t) = custom_file.or(head.custom_title.clone()) {
                (t, TitleSource::Custom)
            } else if let Some(t) = head.summary.clone() {
                (t, TitleSource::Summary)
            } else {
                (head.first_user_message.clone().unwrap_or_else(|| session_id.clone()), TitleSource::FirstMessage)
            };
            let client = match head.entrypoint.as_str() {
                "claude-desktop" => ClientTag::Desktop,
                "cli" => ClientTag::Terminal,
                e if e.starts_with("sdk") => ClientTag::Sdk,
                _ => ClientTag::Unknown,
            };
            let project = project_ref(head.worktree_cwd.as_deref().unwrap_or(&head.cwd), None);
            let archived = desktop.is_some_and(|d| d.archived);
            let status = if archived {
                SessionStatus::Archived
            } else if (client == ClientTag::Desktop && desktop.is_none()) || !project.exists {
                SessionStatus::Orphaned
            } else {
                SessionStatus::Active
            };
            let children: Vec<ChildSummary> = fs::read_dir(side_dir.join("subagents"))
                .map(|entries| entries.flatten().map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
                    .map(|p| ChildSummary {
                        id: format!("claude:{session_id}:{}", p.file_stem().unwrap_or_default().to_string_lossy()),
                        kind: "subagent".into(),
                        title: read_head(&p).ok().and_then(|h| h.first_user_message).unwrap_or_default(),
                        bytes: tree_size(&p),
                    })
                    .collect())
                .unwrap_or_default();
            let bytes = related_paths(root, &path, &session_id).iter().map(|p| tree_size(p)).sum();
            sessions.push(Session {
                id: format!("claude:{session_id}"),
                agent: Agent::Claude,
                native_id: session_id,
                title,
                title_source,
                project,
                client,
                created_at: desktop.map(|d| secs(d.created_ms)).filter(|t| *t > 0).unwrap_or_else(|| mtime(&path)),
                updated_at: desktop.map(|d| secs(d.last_activity_ms)).filter(|t| *t > 0).unwrap_or_else(|| mtime(&path)),
                archived,
                pinned: false,
                status,
                children,
                bytes,
                path: path.to_string_lossy().into_owned(),
                in_desktop_index: desktop.is_some(),
                parent_missing: false,
                favorite: false,
                summary: None,
                summary_stale: false,
            });
        }
    }
    Ok(sessions)
}
```

MSRV is 1.77.2: do not use `Option::is_none_or`.

Register `pub mod claude_catalog;`.

- [ ] **Step 4: Run tests and checks** → pass.
- [ ] **Step 5: Commit** — `feat: read Claude sessions from the desktop index and transcript heads`.

---

### Task 4: Roots, catalog assembly, cache and queries

**Files:**
- Create: `src-tauri/src/sessions/roots.rs`, `src-tauri/src/sessions/catalog.rs`

**Interfaces:**
- Produces:
  - `roots::resolve(overrides: &Roots) -> Roots` — Codex: override → user env `CODEX_HOME` (`crate::winenv::get_user_raw`) → process env → `~/.codex`; Claude: override → `CLAUDE_CONFIG_DIR` → `~/.claude`; desktop index: override → `%APPDATA%\Claude\claude-code-sessions`.
  - `catalog::Catalog { sessions: Vec<Session>, warnings: Vec<String> }`
  - `catalog::load(roots: &Roots) -> Catalog` (errors per agent become warnings; cached by a fingerprint of the Codex state db mtime+size and the Claude projects dir listing mtime, reused for 10 s)
  - `catalog::invalidate()`
  - `catalog::filter(sessions: &[Session], query: &SessionQuery) -> Vec<Session>` (sorted by `updated_at` desc; pinned first within equal days not required)
  - `catalog::page(filtered: Vec<Session>, offset: usize) -> SessionPage` (40 per page)
  - `catalog::projects(sessions: &[Session]) -> Vec<ProjectRow>`

- [ ] **Step 1: Write the failing tests** (`catalog.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, agent: Agent, client: ClientTag, status: SessionStatus, project: &str, updated: u64) -> Session {
        Session {
            id: id.into(), agent, native_id: id.into(), title: format!("title {id}"), title_source: TitleSource::Client,
            project: ProjectRef { key: project.into(), name: project.into(), path: project.into(), exists: status != SessionStatus::Orphaned },
            client, created_at: 0, updated_at: updated, archived: status == SessionStatus::Archived, pinned: false, status,
            children: vec![], bytes: 10, path: String::new(), in_desktop_index: false, parent_missing: false,
            favorite: false, summary: None, summary_stale: false,
        }
    }

    fn sample() -> Vec<Session> {
        vec![
            session("a", Agent::Codex, ClientTag::Desktop, SessionStatus::Active, "p1", 3),
            session("b", Agent::Codex, ClientTag::Automation, SessionStatus::Active, "p1", 5),
            session("c", Agent::Claude, ClientTag::Desktop, SessionStatus::Orphaned, "p2", 4),
            session("d", Agent::Claude, ClientTag::Terminal, SessionStatus::Archived, "p2", 1),
        ]
    }

    #[test]
    fn automation_is_hidden_unless_requested() {
        let ids = |q: &SessionQuery| filter(&sample(), q).iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&SessionQuery::default()), vec!["c", "a", "d"]);
        assert_eq!(ids(&SessionQuery { include_automation: true, ..Default::default() }), vec!["b", "c", "a", "d"]);
        assert_eq!(ids(&SessionQuery { status: "orphaned".into(), ..Default::default() }), vec!["c"]);
        assert_eq!(ids(&SessionQuery { agent: "claude".into(), project: "p2".into(), ..Default::default() }), vec!["c", "d"]);
        assert_eq!(ids(&SessionQuery { search: "TITLE A".into(), ..Default::default() }), vec!["a"]);
    }

    #[test]
    fn project_rows_count_sessions_orphans_and_bytes() {
        let rows = projects(&sample());
        let p2 = rows.iter().find(|r| r.project.key == "p2").unwrap();
        assert_eq!(p2.sessions, 2);
        assert_eq!(p2.orphans, 1);
        assert_eq!(p2.bytes, 20);
        assert_eq!(p2.updated_at, 4);
        let p1 = rows.iter().find(|r| r.project.key == "p1").unwrap();
        assert_eq!(p1.sessions, 1, "automation runs do not count as sessions");
    }
}
```

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement**

`roots.rs`:

```rust
use super::model::Roots;
use std::path::PathBuf;

fn env(name: &str) -> Option<String> {
    crate::winenv::get_user_raw(name)
        .or_else(|| std::env::var(name).ok())
        .filter(|v| !v.trim().is_empty())
}

fn pick(override_value: &str, env_name: Option<&str>, fallback: PathBuf) -> String {
    if !override_value.trim().is_empty() {
        return override_value.trim().to_string();
    }
    env_name
        .and_then(env)
        .unwrap_or_else(|| fallback.to_string_lossy().into_owned())
}

pub fn resolve(overrides: &Roots) -> Roots {
    let home = dirs::home_dir().unwrap_or_default();
    let roaming = dirs::data_dir().unwrap_or_default();
    Roots {
        codex: pick(&overrides.codex, Some("CODEX_HOME"), home.join(".codex")),
        claude: pick(&overrides.claude, Some("CLAUDE_CONFIG_DIR"), home.join(".claude")),
        claude_desktop_index: pick(&overrides.claude_desktop_index, None, roaming.join("Claude").join("claude-code-sessions")),
    }
}
```

`catalog.rs`:

```rust
use super::model::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Catalog { pub sessions: Vec<Session>, pub warnings: Vec<String> }

static CACHE: Mutex<Option<(Instant, Roots, Vec<Session>, Vec<String>)>> = Mutex::new(None);
const CACHE_TTL: Duration = Duration::from_secs(10);
pub const PAGE_SIZE: usize = 40;

pub fn invalidate() {
    if let Ok(mut cache) = CACHE.lock() { *cache = None; }
}

pub fn load(roots: &Roots) -> Catalog {
    if let Ok(cache) = CACHE.lock() {
        if let Some((at, cached_roots, sessions, warnings)) = cache.as_ref() {
            if at.elapsed() < CACHE_TTL && cached_roots == roots {
                return Catalog { sessions: sessions.clone(), warnings: warnings.clone() };
            }
        }
    }
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();
    match super::codex_catalog::load(Path::new(&roots.codex)) {
        Ok(found) => sessions.extend(found),
        Err(code) => warnings.push(format!("Codex: {code}")),
    }
    match super::claude_catalog::load(Path::new(&roots.claude), Path::new(&roots.claude_desktop_index)) {
        Ok(found) => sessions.extend(found),
        Err(code) => warnings.push(format!("Claude: {code}")),
    }
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((Instant::now(), roots.clone(), sessions.clone(), warnings.clone()));
    }
    Catalog { sessions, warnings }
}

fn client_name(client: ClientTag) -> &'static str {
    match client {
        ClientTag::Desktop => "desktop",
        ClientTag::Terminal => "terminal",
        ClientTag::Ide => "ide",
        ClientTag::Automation => "automation",
        ClientTag::Sdk => "sdk",
        ClientTag::Unknown => "unknown",
    }
}

fn is_automation(s: &Session) -> bool {
    matches!(s.client, ClientTag::Automation | ClientTag::Sdk)
}

pub fn filter(sessions: &[Session], q: &SessionQuery) -> Vec<Session> {
    let search = q.search.trim().to_lowercase();
    let mut out: Vec<Session> = sessions
        .iter()
        .filter(|s| q.include_automation || !q.client.is_empty() || !is_automation(s))
        .filter(|s| q.agent.is_empty() || s.agent.as_str() == q.agent)
        .filter(|s| q.project.is_empty() || s.project.key == q.project)
        .filter(|s| q.client.is_empty() || client_name(s.client) == q.client)
        .filter(|s| match q.status.as_str() {
            "active" => s.status == SessionStatus::Active,
            "archived" => s.status == SessionStatus::Archived,
            "orphaned" => s.status == SessionStatus::Orphaned,
            _ => true,
        })
        .filter(|s| !q.favorites_only || s.favorite)
        .filter(|s| q.updated_after == 0 || s.updated_at >= q.updated_after)
        .filter(|s| {
            search.is_empty()
                || s.title.to_lowercase().contains(&search)
                || s.project.name.to_lowercase().contains(&search)
                || s.project.path.to_lowercase().contains(&search)
                || s.summary.as_deref().is_some_and(|t| t.to_lowercase().contains(&search))
        })
        .cloned()
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

pub fn page(filtered: Vec<Session>, offset: usize, warnings: Vec<String>) -> SessionPage {
    SessionPage {
        total: filtered.len(),
        ids: filtered.iter().map(|s| s.id.clone()).collect(),
        total_bytes: filtered.iter().map(|s| s.bytes).sum(),
        items: filtered.into_iter().skip(offset).take(PAGE_SIZE).collect(),
        warnings,
    }
}

pub fn projects(sessions: &[Session]) -> Vec<ProjectRow> {
    let mut rows: BTreeMap<String, ProjectRow> = BTreeMap::new();
    for s in sessions.iter().filter(|s| !is_automation(s)) {
        let row = rows.entry(s.project.key.clone()).or_insert_with(|| ProjectRow {
            project: s.project.clone(), agents: vec![], sessions: 0, orphans: 0, bytes: 0, updated_at: 0,
        });
        if !row.agents.contains(&s.agent) { row.agents.push(s.agent); }
        row.sessions += 1;
        row.orphans += usize::from(s.status == SessionStatus::Orphaned);
        row.bytes += s.bytes;
        row.updated_at = row.updated_at.max(s.updated_at);
    }
    let mut out: Vec<_> = rows.into_values().collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}
```

Full-text search (`q.full_text`) is applied in the command (Task 6) after `filter`, because it needs transcripts.

- [ ] **Step 4: Run tests and checks.**
- [ ] **Step 5: Commit** — `feat: combine agent sessions, filter them and group by project`.

---

### Task 5: Annotations store with migration

**Files:**
- Create: `src-tauri/src/sessions/annotations.rs`

**Interfaces:**
- Produces:
  - `annotations::root() -> PathBuf` (same data dir as the old index: `%LOCALAPPDATA%\Stacker\{dev|stable}\conversations`)
  - `annotations::connect() -> Result<Connection, String>` / `connect_at(dir: &Path)`
  - `annotations::apply(conn, sessions: &mut [Session])` — sets `favorite`, `summary`, `summary_stale` (stale when the stored fingerprint differs from `quick_fingerprint(path)`)
  - `annotations::set_favorite(conn, ids: &[String], favorite: bool) -> Result<(), String>`
  - `annotations::roots(conn) -> Roots` / `set_roots(conn, &Roots)`
  - `annotations::forget(conn, ids: &[String])` — drop rows of deleted sessions
  - `annotations::quick_fingerprint(path: &Path) -> String` (`"<len>:<mtime_nanos>"`, same format as the old index so old summaries stay current)

Schema: `CREATE TABLE IF NOT EXISTS session_notes (id TEXT PRIMARY KEY, favorite INTEGER NOT NULL DEFAULT 0, summary TEXT NOT NULL DEFAULT '', summary_fingerprint TEXT NOT NULL DEFAULT '')`, `CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)`, in `sessions.sqlite3`. Migration runs once (settings key `migrated_v1`): if `index.sqlite3` exists, copy rows from its `annotations` table where `favorite = 1` or `summary != ''`; map id `codex:<x>` → `codex:<x>`, `claude-cli:<x>` → `claude:<x>`, drop others (`claude-desktop-*`, `import-*`); the summary fingerprint is taken from the old `conversations.fingerprint` column when present.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_and_roots_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let conn = connect_at(dir.path()).unwrap();
        set_favorite(&conn, &["codex:a".into()], true).unwrap();
        let roots = Roots { codex: "C:/x".into(), ..Default::default() };
        set_roots(&conn, &roots).unwrap();
        assert_eq!(super::roots(&conn), roots);
        let mut sessions = vec![super::super::catalog::tests_support::session("codex:a")];
        apply(&conn, &mut sessions);
        assert!(sessions[0].favorite);
        forget(&conn, &["codex:a".into()]).unwrap();
        let mut again = vec![super::super::catalog::tests_support::session("codex:a")];
        apply(&conn, &mut again);
        assert!(!again[0].favorite);
    }

    #[test]
    fn old_index_annotations_are_migrated_once() {
        let dir = tempfile::tempdir().unwrap();
        let old = rusqlite::Connection::open(dir.path().join("index.sqlite3")).unwrap();
        old.execute_batch(
            "CREATE TABLE annotations (id TEXT PRIMARY KEY, favorite INTEGER, hidden INTEGER, group_name TEXT, summary TEXT, summary_fingerprint TEXT);
             INSERT INTO annotations VALUES ('claude-cli:s1',1,0,'','要点','1:2');
             INSERT INTO annotations VALUES ('claude-desktop-0:z',1,0,'','','');
             INSERT INTO annotations VALUES ('codex:t1',0,1,'g','','');",
        ).unwrap();
        drop(old);
        let conn = connect_at(dir.path()).unwrap();
        let ids: Vec<String> = conn.prepare("SELECT id FROM session_notes ORDER BY id").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(ids, vec!["claude:s1"]);
        drop(conn);
        let conn = connect_at(dir.path()).unwrap();
        let count: i64 = conn.query_row("SELECT count(*) FROM session_notes", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }
}
```

Add to `catalog.rs` a test helper module used by other tests:

```rust
#[cfg(test)]
pub(crate) mod tests_support {
    use super::super::model::*;
    pub fn session(id: &str) -> Session {
        Session {
            id: id.into(), agent: Agent::Codex, native_id: id.into(), title: id.into(), title_source: TitleSource::Client,
            project: ProjectRef::default(), client: ClientTag::Desktop, created_at: 0, updated_at: 0, archived: false,
            pinned: false, status: SessionStatus::Active, children: vec![], bytes: 0, path: String::new(),
            in_desktop_index: false, parent_missing: false, favorite: false, summary: None, summary_stale: false,
        }
    }
}
```

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement**

```rust
use super::model::{Roots, Session};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Stacker")
        .join(if cfg!(debug_assertions) { "dev" } else { "stable" })
        .join("conversations")
}

pub fn connect() -> Result<Connection, String> { connect_at(&root()) }

pub fn connect_at(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir).map_err(|_| "E_STORAGE".to_string())?;
    let conn = Connection::open(dir.join("sessions.sqlite3")).map_err(|_| "E_STORAGE".to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(super::err)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS session_notes (id TEXT PRIMARY KEY, favorite INTEGER NOT NULL DEFAULT 0,
             summary TEXT NOT NULL DEFAULT '', summary_fingerprint TEXT NOT NULL DEFAULT '');
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    ).map_err(|_| "E_STORAGE".to_string())?;
    migrate(&conn, dir)?;
    Ok(conn)
}

fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key=?", [key], |r| r.get(0)).optional().ok().flatten()
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])
        .map(|_| ()).map_err(super::err)
}

fn migrate(conn: &Connection, dir: &Path) -> Result<(), String> {
    if setting(conn, "migrated_v1").is_some() { return Ok(()); }
    let old = dir.join("index.sqlite3");
    if old.is_file() {
        if let Ok(old) = Connection::open_with_flags(&old, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            let rows: Vec<(String, i64, String, String)> = old
                .prepare("SELECT id, favorite, summary, summary_fingerprint FROM annotations WHERE favorite=1 OR summary!=''")
                .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map(|m| m.filter_map(Result::ok).collect()))
                .unwrap_or_default();
            for (id, favorite, summary, fingerprint) in rows {
                let new_id = if let Some(rest) = id.strip_prefix("codex:") {
                    format!("codex:{rest}")
                } else if let Some(rest) = id.strip_prefix("claude-cli:") {
                    format!("claude:{rest}")
                } else {
                    continue;
                };
                conn.execute(
                    "INSERT OR IGNORE INTO session_notes(id,favorite,summary,summary_fingerprint) VALUES(?1,?2,?3,?4)",
                    params![new_id, favorite, summary, fingerprint],
                ).map_err(super::err)?;
            }
        }
    }
    set_setting(conn, "migrated_v1", "1")
}

pub fn quick_fingerprint(path: &Path) -> String {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| {
            let nanos = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
            Some(format!("{}:{nanos}", m.len()))
        })
        .unwrap_or_default()
}

pub fn apply(conn: &Connection, sessions: &mut [Session]) {
    let Ok(mut stmt) = conn.prepare("SELECT favorite, summary, summary_fingerprint FROM session_notes WHERE id=?") else { return };
    for s in sessions.iter_mut() {
        if let Ok(Some((favorite, summary, fingerprint))) = stmt
            .query_row([&s.id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .optional()
        {
            s.favorite = favorite != 0;
            if !summary.is_empty() {
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
        ).map_err(super::err)?;
    }
    Ok(())
}

pub fn forget(conn: &Connection, ids: &[String]) -> Result<(), String> {
    for id in ids {
        conn.execute("DELETE FROM session_notes WHERE id=?", [id]).map_err(super::err)?;
    }
    Ok(())
}

pub fn roots(conn: &Connection) -> Roots {
    setting(conn, "roots").and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

pub fn set_roots(conn: &Connection, roots: &Roots) -> Result<(), String> {
    set_setting(conn, "roots", &serde_json::to_string(roots).map_err(super::err)?)
}
```

- [ ] **Step 4: Run tests and checks.**
- [ ] **Step 5: Commit** — `feat: keep session favorites and summaries in a small annotations store`.

---

### Task 6: Transcript reading, full-text search and read commands

**Files:**
- Create: `src-tauri/src/sessions/transcript.rs` (message parsing moved from `conversations/reader.rs::read`, minus import handling and conversation fields), `src-tauri/src/sessions/commands.rs`
- Modify: `src-tauri/src/lib.rs` (register commands)

**Interfaces:**
- Produces:
  - `transcript::Message { line: usize, role: String, text: String }` (Serialize camelCase)
  - `transcript::read(agent: Agent, path: &Path) -> Result<(Vec<Message>, bool /*complete*/), String>` — Codex: `response_item` messages / function calls, fallback to `event_msg` user/agent messages; Claude: `user` / `assistant` records. Limits kept from the old reader: 64 MB file, 1 MB line, 8 MB text.
  - `transcript::contains(agent, path, needle: &str) -> bool` (streaming, lowercase, stops at first hit)
  - Commands:
    - `sessions_list(query: SessionQuery) -> SessionPage` (async)
    - `sessions_projects() -> Vec<ProjectRow>` (async)
    - `sessions_read(id: String, offset: usize) -> SessionDetail` where `SessionDetail { session: Session, messages: Vec<Message>, total: usize, complete: bool }` (200 messages per page)
    - `sessions_roots() -> RootsView` where `RootsView { effective: Roots, overrides: Roots, export_dir: String }`
    - `sessions_set_roots(overrides: Roots) -> RootsView`
    - `sessions_favorite(ids: Vec<String>, favorite: bool)`
    - `sessions_open(id: String, target: String)` — `folder` (transcript folder), `project`, `native` (Codex only: `codex://threads/<id>`)

- [ ] **Step 1: Write the failing tests** (`transcript.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_messages_do_not_duplicate_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        std::fs::write(&path, concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"abc\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"hello\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello\"}]}}\n"
        )).unwrap();
        let (messages, complete) = read(Agent::Codex, &path).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(complete);
        assert!(contains(Agent::Codex, &path, "HELLO"));
        assert!(!contains(Agent::Codex, &path, "absent"));
    }

    #[test]
    fn claude_partial_tail_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, "{\"type\":\"user\",\"sessionId\":\"a\",\"message\":{\"content\":\"question\"}}\n{bad").unwrap();
        let (messages, complete) = read(Agent::Claude, &path).unwrap();
        assert_eq!(messages[0].text, "question");
        assert!(!complete);
    }
}
```

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement**

`transcript.rs`: copy `content()`, `str_at()` and the line loop of `conversations/reader.rs::read` (lines 263–471 today), keeping only message extraction and limits; replace `source.kind == Kind::Codex` with `agent == Agent::Codex`; return `(messages, complete)`. `contains` reads line by line with the same limits and returns true when a lowercase line contains the lowercase needle inside a user / assistant / tool text field (parse the JSON line and test the extracted text).

`commands.rs`:

```rust
use super::catalog;
use super::model::*;
use super::transcript::{self, Message};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail { pub session: Session, pub messages: Vec<Message>, pub total: usize, pub complete: bool }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsView { pub effective: Roots, pub overrides: Roots, pub export_dir: String }

fn current() -> Result<(rusqlite::Connection, Roots), String> {
    let conn = super::annotations::connect()?;
    let roots = super::roots::resolve(&super::annotations::roots(&conn));
    Ok((conn, roots))
}

fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> impl std::future::Future<Output = Result<T, String>> {
    async move { tauri::async_runtime::spawn_blocking(work).await.map_err(super::err)? }
}

pub(crate) fn annotated_catalog() -> Result<(Vec<Session>, Vec<String>, Roots), String> {
    let (conn, roots) = current()?;
    let catalog = catalog::load(&roots);
    let mut sessions = catalog.sessions;
    super::annotations::apply(&conn, &mut sessions);
    Ok((sessions, catalog.warnings, roots))
}

#[tauri::command]
pub async fn sessions_list(query: SessionQuery) -> Result<SessionPage, String> {
    blocking(move || {
        let (sessions, warnings, _) = annotated_catalog()?;
        let mut filtered = catalog::filter(&sessions, &SessionQuery { full_text: false, ..query.clone() });
        if query.full_text && !query.search.trim().is_empty() {
            let text_query = SessionQuery { search: String::new(), full_text: false, ..query.clone() };
            let already: std::collections::HashSet<String> = filtered.iter().map(|s| s.id.clone()).collect();
            let extra = catalog::filter(&sessions, &text_query)
                .into_iter()
                .filter(|s| !already.contains(&s.id) && transcript::contains(s.agent, Path::new(&s.path), query.search.trim()));
            filtered.extend(extra);
            filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        }
        Ok(catalog::page(filtered, query.offset, warnings))
    }).await
}

#[tauri::command]
pub async fn sessions_projects() -> Result<Vec<ProjectRow>, String> {
    blocking(|| Ok(catalog::projects(&annotated_catalog()?.0))).await
}

#[tauri::command]
pub async fn sessions_read(id: String, offset: usize) -> Result<SessionDetail, String> {
    blocking(move || {
        let (sessions, _, _) = annotated_catalog()?;
        let session = sessions.into_iter().find(|s| s.id == id).ok_or("E_NOT_FOUND")?;
        let (messages, complete) = transcript::read(session.agent, Path::new(&session.path))?;
        let total = messages.len();
        let messages = messages.into_iter().skip(offset).take(200).collect();
        Ok(SessionDetail { session, messages, total, complete })
    }).await
}

fn roots_view(conn: &rusqlite::Connection) -> RootsView {
    let overrides = super::annotations::roots(conn);
    RootsView {
        effective: super::roots::resolve(&overrides),
        overrides,
        export_dir: super::annotations::root().join("exports").to_string_lossy().into_owned(),
    }
}

#[tauri::command]
pub fn sessions_roots() -> Result<RootsView, String> {
    Ok(roots_view(&super::annotations::connect()?))
}

#[tauri::command]
pub fn sessions_set_roots(overrides: Roots) -> Result<RootsView, String> {
    for value in [&overrides.codex, &overrides.claude, &overrides.claude_desktop_index] {
        if !value.trim().is_empty() && !Path::new(value.trim()).is_dir() {
            return Err("E_PATH".into());
        }
    }
    let conn = super::annotations::connect()?;
    super::annotations::set_roots(&conn, &overrides)?;
    catalog::invalidate();
    Ok(roots_view(&conn))
}

#[tauri::command]
pub fn sessions_favorite(ids: Vec<String>, favorite: bool) -> Result<(), String> {
    super::annotations::set_favorite(&super::annotations::connect()?, &ids, favorite)
}

#[tauri::command]
pub async fn sessions_open(id: String, target: String) -> Result<(), String> {
    blocking(move || {
        let (sessions, _, _) = annotated_catalog()?;
        let session = sessions.into_iter().find(|s| s.id == id).ok_or("E_NOT_FOUND")?;
        let mut cmd = std::process::Command::new("explorer.exe");
        match target.as_str() {
            "native" if session.agent == Agent::Codex && session.native_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                cmd.arg(format!("codex://threads/{}", session.native_id));
            }
            "project" if Path::new(&session.project.path).is_dir() => { cmd.arg(&session.project.path); }
            "folder" => { cmd.arg(Path::new(&session.path).parent().ok_or("E_PATH")?); }
            _ => return Err("E_REQUEST".into()),
        }
        cmd.spawn().map(|_| ()).map_err(super::err)
    }).await
}
```

Register the seven commands in `lib.rs` (`sessions::commands::sessions_list`, …) next to the old `conversations::` ones (removed in Task 9).

- [ ] **Step 4: Run tests and checks.**
- [ ] **Step 5: Commit** — `feat: session list, projects, detail and source commands`.

---

### Task 7: Slim Markdown export

**Files:**
- Create: `src-tauri/src/sessions/export.rs`

**Interfaces:**
- Consumes: `transcript::read`.
- Produces: `export::slim_markdown(session: &Session, messages: &[Message]) -> String`; `export::write_slim(session: &Session, dir: &Path) -> Result<PathBuf, String>` (target `<dir>/<yyyy-mm-dd>/<agent>/<project name>/<title>-<native id>.md`, file-name characters `\/:*?"<>|` replaced by `_`, title cut to 60 chars).

Content: a header (title, agent, project path, client, created/updated, native id, original path), then each message as `### 用户` / `### 助手` / `### 工具` followed by its text. Tool messages are cut to 2,000 characters with a `…（已截断）` marker; `[image attachment]` lines stay as placeholders; nothing else from the JSONL (compaction snapshots, base64) is written.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slim_export_keeps_readable_text_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let image = "A".repeat(50_000);
        std::fs::write(&path, format!(
            "{}\n{}\n",
            serde_json::json!({"type":"user","sessionId":"s","message":{"content":[{"type":"text","text":"问题"},{"type":"image","source":{"data":image}}]}}),
            serde_json::json!({"type":"assistant","sessionId":"s","message":{"content":[{"type":"text","text":"回答"}]}})
        )).unwrap();
        let mut session = super::super::catalog::tests_support::session("claude:s");
        session.agent = Agent::Claude;
        session.native_id = "s".into();
        session.title = "a/b:c".into();
        session.path = path.to_string_lossy().into_owned();
        let out = write_slim(&session, &dir.path().join("exports")).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("问题") && text.contains("回答"));
        assert!(text.len() < 2_000, "image data must not be exported");
        assert!(out.file_name().unwrap().to_string_lossy().starts_with("a_b_c-s"));
    }
}
```

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement**

```rust
use super::model::*;
use super::transcript::{self, Message};
use std::path::{Path, PathBuf};

fn safe(name: &str, limit: usize) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .take(limit)
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() { "session".into() } else { trimmed }
}

fn date(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

pub fn slim_markdown(session: &Session, messages: &[Message]) -> String {
    let mut out = format!(
        "# {}\n\n- 智能体：{}\n- 项目：{}\n- 创建：{}\n- 最后活动：{}\n- 会话 ID：{}\n- 原始记录：{}\n\n",
        session.title, session.agent.as_str(), session.project.path,
        date(session.created_at), date(session.updated_at), session.native_id, session.path
    );
    for m in messages {
        let heading = match m.role.as_str() { "user" => "用户", "assistant" => "助手", _ => "工具" };
        let mut text = m.text.clone();
        if heading == "工具" && text.chars().count() > 2_000 {
            text = text.chars().take(2_000).collect::<String>() + "\n…（已截断）";
        }
        out.push_str(&format!("### {heading}\n\n{text}\n\n"));
    }
    out
}

pub fn write_slim(session: &Session, dir: &Path) -> Result<PathBuf, String> {
    let (messages, _) = transcript::read(session.agent, Path::new(&session.path))?;
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    let folder = dir.join(day).join(session.agent.as_str()).join(safe(&session.project.name, 60));
    std::fs::create_dir_all(&folder).map_err(|_| "E_STORAGE".to_string())?;
    let file = folder.join(format!("{}-{}.md", safe(&session.title, 60), safe(&session.native_id, 80)));
    std::fs::write(&file, slim_markdown(session, &messages)).map_err(|_| "E_STORAGE".to_string())?;
    Ok(file)
}
```

The transcript reader must turn image parts into `[image attachment]` (it does today via `content()`).

- [ ] **Step 4: Run tests and checks.**
- [ ] **Step 5: Commit** — `feat: export sessions as slim Markdown`.

---

### Task 8: Deletion preview and execution

**Files:**
- Create: `src-tauri/src/sessions/codex_rpc.rs` (moved from `conversations/codex.rs`: `command`, `hidden`, `capabilities`, `require_closed`, `Rpc` with `start(root: &Path)` taking the Codex root instead of a `Source`, `call`, `descendants`)
- Create: `src-tauri/src/sessions/delete.rs`
- Modify: `commands.rs`, `lib.rs`

**Interfaces:**
- Produces:
  - `delete::Mode { SlimExport, Direct, FullBackup }` (serde `snake_case`: `slim_export` / `direct` / `full_backup`)
  - `delete::Blocked { id: String, title: String, reason: String }`
  - `delete::Preview { token, mode, sessions: Vec<Session>, children: usize, files: usize, bytes: u64, blocked: Vec<Blocked>, created: u64 }`
  - `delete::plan(sessions: &[Session], ids: &[String], roots: &Roots, now: u64) -> (Vec<(Session, Vec<PathBuf>)>, Vec<Blocked>)` — pure except for filesystem reads; used by preview and re-used at execution.
  - `delete::JobState { id, state /*running|completed|failed|cancelled*/, done, total, items: Vec<JobItem>, export_dir, error }`, `JobItem { id, title, status, detail }`
  - Commands: `sessions_delete_preview(ids: Vec<String>, mode: Mode) -> Preview`, `sessions_delete_execute(token: String) -> JobState`, `sessions_job() -> Option<JobState>`, `sessions_cancel()`

Blocking rules:
- Claude: `in_desktop_index` → `E_IN_DESKTOP` ("请先在 Claude 桌面端删除该会话"); transcript modified within 120 s of `now` → `E_IN_USE`; any related path that is a link or outside `roots.claude` → `E_LINK`.
- Codex: `codex_rpc::require_closed()` fails → every Codex item `E_CLOSE_CODEX`; `codex_rpc::capabilities()` fails → `E_CODEX_VERSION`.
- More than 500 selected → `E_REQUEST`.

Execution (background thread, one job at a time, `E_BUSY` otherwise):
1. Reload the catalog; re-run `plan` for the preview's ids; if the allowed set or any file's `annotations::quick_fingerprint` differs from the preview → fail with `E_CHANGED`.
2. For each allowed session, in order: mode `SlimExport` → `export::write_slim(session, export_dir)`; `FullBackup` → copy every related path into `backups/<job id>/<agent>-<native id>/`; `Direct` → nothing. A failed export/backup marks the item failed and skips its deletion.
3. Delete: Claude → `fs::remove_file` / `fs::remove_dir_all` for each related path after re-checking it is not a link and is inside `roots.claude`; Codex → one `codex_rpc::Rpc` per job, `thread/delete` for the session (App Server deletes descendants), verify the rollout file no longer exists.
4. `annotations::forget` for completed ids; `catalog::invalidate()`.
5. Honor `sessions_cancel` between items.

Previews live in memory (`Mutex<HashMap<String, (Preview, Vec<String> /*fingerprints*/)>>`), expire after 600 s and are removed on execution.

- [ ] **Step 1: Write the failing tests** (`delete.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn claude_session(root: &Path, id: &str, in_index: bool) -> Session {
        let project = root.join("projects").join("p");
        fs::create_dir_all(project.join(id).join("subagents")).unwrap();
        let transcript = project.join(format!("{id}.jsonl"));
        fs::write(&transcript, b"{}").unwrap();
        fs::create_dir_all(root.join("file-history").join(id)).unwrap();
        let mut s = super::super::catalog::tests_support::session(&format!("claude:{id}"));
        s.agent = Agent::Claude;
        s.native_id = id.into();
        s.path = transcript.to_string_lossy().into_owned();
        s.in_desktop_index = in_index;
        s
    }

    fn roots(root: &Path) -> Roots {
        Roots { claude: root.to_string_lossy().into_owned(), ..Default::default() }
    }

    #[test]
    fn desktop_sessions_and_fresh_writes_are_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".claude");
        let shown = claude_session(&root, "shown", true);
        let fresh = claude_session(&root, "fresh", false);
        let old = claude_session(&root, "old", false);
        let later = super::super::now() + 10;
        let (allowed, blocked) = plan(&[shown.clone(), fresh.clone(), old.clone()], &["claude:shown".into(), "claude:old".into()], &roots(&root), later + 1_000);
        assert_eq!(blocked.iter().map(|b| b.reason.as_str()).collect::<Vec<_>>(), vec!["E_IN_DESKTOP"]);
        assert_eq!(allowed.len(), 1);
        let files: Vec<_> = allowed[0].1.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert!(files.contains(&"old.jsonl".to_string()) && files.contains(&"old".to_string()));
        let (_, blocked) = plan(&[fresh], &["claude:fresh".into()], &roots(&root), later);
        assert_eq!(blocked[0].reason, "E_IN_USE");
    }

    #[test]
    fn claude_deletion_removes_every_related_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".claude");
        let s = claude_session(&root, "gone", false);
        let (allowed, _) = plan(&[s], &["claude:gone".into()], &roots(&root), super::super::now() + 1_000);
        delete_claude(&allowed[0].1, &roots(&root)).unwrap();
        assert!(!root.join("projects").join("p").join("gone.jsonl").exists());
        assert!(!root.join("projects").join("p").join("gone").exists());
        assert!(!root.join("file-history").join("gone").exists());
    }

    #[test]
    fn paths_outside_the_root_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, b"keep").unwrap();
        let root = dir.path().join(".claude");
        fs::create_dir_all(&root).unwrap();
        assert!(delete_claude(&[outside.clone()], &roots(&root)).is_err());
        assert!(outside.exists());
    }
}
```

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement** `delete.rs`:

```rust
use super::model::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const IN_USE_SECONDS: u64 = 120;
const PREVIEW_SECONDS: u64 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode { SlimExport, Direct, FullBackup }

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked { pub id: String, pub title: String, pub reason: String }

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub token: String,
    pub mode: Mode,
    pub sessions: Vec<Session>,
    pub children: usize,
    pub files: usize,
    pub bytes: u64,
    pub blocked: Vec<Blocked>,
    pub created: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobItem { pub id: String, pub title: String, pub status: String, pub detail: String }

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub id: String,
    pub state: String,
    pub done: usize,
    pub total: usize,
    pub items: Vec<JobItem>,
    pub export_dir: String,
    pub error: String,
}

static PREVIEWS: Mutex<Option<HashMap<String, (Preview, Vec<String>)>>> = Mutex::new(None);
static JOB: Mutex<Option<JobState>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 { return true; }
            }
            m.file_type().is_symlink()
        })
        .unwrap_or(false)
}

fn inside(root: &Path, path: &Path) -> bool {
    match (fs::canonicalize(root), fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => path.starts_with(root),
        _ => false,
    }
}

fn modified_secs(path: &Path) -> u64 {
    fs::metadata(path).and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs()).unwrap_or(0)
}

pub fn plan(sessions: &[Session], ids: &[String], roots: &Roots, now: u64) -> (Vec<(Session, Vec<PathBuf>)>, Vec<Blocked>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    let codex_ready = if ids.iter().any(|id| id.starts_with("codex:")) {
        super::codex_rpc::require_closed().and_then(|_| super::codex_rpc::capabilities())
    } else {
        Ok(())
    };
    for id in ids {
        let Some(session) = sessions.iter().find(|s| &s.id == id) else {
            blocked.push(Blocked { id: id.clone(), title: id.clone(), reason: "E_NOT_FOUND".into() });
            continue;
        };
        let block = |reason: &str| Blocked { id: session.id.clone(), title: session.title.clone(), reason: reason.into() };
        match session.agent {
            Agent::Claude => {
                let root = Path::new(&roots.claude);
                let transcript = Path::new(&session.path);
                if session.in_desktop_index {
                    blocked.push(block("E_IN_DESKTOP"));
                } else if now.saturating_sub(modified_secs(transcript)) < IN_USE_SECONDS {
                    blocked.push(block("E_IN_USE"));
                } else {
                    let paths = super::claude_catalog::related_paths(root, transcript, &session.native_id);
                    if paths.iter().any(|p| is_link(p) || !inside(root, p)) {
                        blocked.push(block("E_LINK"));
                    } else {
                        allowed.push((session.clone(), paths));
                    }
                }
            }
            Agent::Codex => match &codex_ready {
                Err(code) => blocked.push(block(code)),
                Ok(()) => allowed.push((session.clone(), vec![PathBuf::from(&session.path)])),
            },
        }
    }
    (allowed, blocked)
}

pub fn delete_claude(paths: &[PathBuf], roots: &Roots) -> Result<(), String> {
    let root = Path::new(&roots.claude);
    for path in paths {
        if !path.exists() { continue; }
        if is_link(path) || !inside(root, path) { return Err("E_LINK".into()); }
    }
    for path in paths {
        if !path.exists() { continue; }
        let result = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
        result.map_err(|e| if e.kind() == std::io::ErrorKind::PermissionDenied { "E_ACCESS".to_string() } else { super::err(e) })?;
    }
    Ok(())
}

fn fingerprints(allowed: &[(Session, Vec<PathBuf>)]) -> Vec<String> {
    allowed.iter().map(|(s, _)| format!("{}={}", s.id, super::annotations::quick_fingerprint(Path::new(&s.path)))).collect()
}

pub fn preview(ids: Vec<String>, mode: Mode) -> Result<Preview, String> {
    if ids.is_empty() || ids.len() > 500 { return Err("E_REQUEST".into()); }
    let (sessions, _, roots) = super::commands::annotated_catalog()?;
    let now = super::now();
    let (allowed, blocked) = plan(&sessions, &ids, &roots, now);
    let preview = Preview {
        token: format!("{:x}", rand_token()),
        mode,
        children: allowed.iter().map(|(s, _)| s.children.len()).sum(),
        files: allowed.iter().map(|(_, p)| p.len()).sum(),
        bytes: allowed.iter().map(|(s, _)| s.bytes).sum(),
        sessions: allowed.iter().map(|(s, _)| s.clone()).collect(),
        blocked,
        created: now,
    };
    let mut store = PREVIEWS.lock().map_err(super::err)?;
    let map = store.get_or_insert_with(HashMap::new);
    map.retain(|_, (p, _)| now.saturating_sub(p.created) <= PREVIEW_SECONDS);
    map.insert(preview.token.clone(), (preview.clone(), fingerprints(&allowed)));
    Ok(preview)
}

fn rand_token() -> u128 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    (u128::from(h.finish()) << 64) | u128::from(std::process::id())
}

fn publish(job: &JobState) {
    if let Ok(mut slot) = JOB.lock() { *slot = Some(job.clone()); }
}

pub fn job() -> Option<JobState> { JOB.lock().ok().and_then(|j| j.clone()) }

pub fn cancel() { CANCEL.store(true, Ordering::SeqCst); }

pub fn execute(token: String) -> Result<JobState, String> {
    if job().is_some_and(|j| j.state == "running") { return Err("E_BUSY".into()); }
    let (preview, expected) = {
        let mut store = PREVIEWS.lock().map_err(super::err)?;
        store.get_or_insert_with(HashMap::new).remove(&token).ok_or("E_PREVIEW")?
    };
    if super::now().saturating_sub(preview.created) > PREVIEW_SECONDS { return Err("E_PREVIEW".into()); }
    super::catalog::invalidate();
    let (sessions, _, roots) = super::commands::annotated_catalog()?;
    let ids: Vec<String> = preview.sessions.iter().map(|s| s.id.clone()).collect();
    let (allowed, blocked) = plan(&sessions, &ids, &roots, super::now());
    if !blocked.is_empty() || fingerprints(&allowed) != expected { return Err("E_CHANGED".into()); }
    let export_dir = super::annotations::root().join("exports");
    let job_id = format!("delete-{}", super::now());
    let mut state = JobState {
        id: job_id.clone(), state: "running".into(), total: allowed.len(),
        export_dir: export_dir.to_string_lossy().into_owned(), ..Default::default()
    };
    CANCEL.store(false, Ordering::SeqCst);
    publish(&state);
    let mode = preview.mode;
    std::thread::spawn(move || {
        let mut rpc = None;
        let mut completed = Vec::new();
        for (session, paths) in allowed {
            if CANCEL.load(Ordering::SeqCst) { state.state = "cancelled".into(); break; }
            let result = (|| -> Result<(), String> {
                match mode {
                    Mode::SlimExport => { super::export::write_slim(&session, &export_dir)?; }
                    Mode::FullBackup => {
                        let target = super::annotations::root().join("backups").join(&job_id)
                            .join(format!("{}-{}", session.agent.as_str(), session.native_id));
                        for path in &paths { copy_tree(path, &target.join(path.file_name().unwrap_or_default()))?; }
                    }
                    Mode::Direct => {}
                }
                match session.agent {
                    Agent::Claude => delete_claude(&paths, &roots),
                    Agent::Codex => {
                        if rpc.is_none() { rpc = Some(super::codex_rpc::Rpc::start(Path::new(&roots.codex))?); }
                        rpc.as_mut().ok_or("E_RPC")?.call("thread/delete", serde_json::json!({"threadId": session.native_id}))?;
                        if Path::new(&session.path).exists() { Err("E_VERIFY".into()) } else { Ok(()) }
                    }
                }
            })();
            if result.is_ok() { completed.push(session.id.clone()); }
            state.items.push(JobItem {
                id: session.id.clone(), title: session.title.clone(),
                status: if result.is_ok() { "completed" } else { "failed" }.into(),
                detail: result.err().unwrap_or_default(),
            });
            state.done += 1;
            publish(&state);
        }
        if let Ok(conn) = super::annotations::connect() { let _ = super::annotations::forget(&conn, &completed); }
        super::catalog::invalidate();
        if state.state == "running" {
            state.state = if state.items.iter().any(|i| i.status == "failed") { "failed" } else { "completed" }.into();
        }
        publish(&state);
    });
    Ok(job().unwrap_or_default())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if is_link(from) { return Err("E_LINK".into()); }
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|_| "E_STORAGE".to_string())?;
        for entry in fs::read_dir(from).map_err(super::err)?.flatten() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() { fs::create_dir_all(parent).map_err(|_| "E_STORAGE".to_string())?; }
        fs::copy(from, to).map(|_| ()).map_err(|_| "E_STORAGE".to_string())
    }
}
```

Commands in `commands.rs`:

```rust
#[tauri::command]
pub async fn sessions_delete_preview(ids: Vec<String>, mode: super::delete::Mode) -> Result<super::delete::Preview, String> {
    blocking(move || super::delete::preview(ids, mode)).await
}

#[tauri::command]
pub async fn sessions_delete_execute(token: String) -> Result<super::delete::JobState, String> {
    blocking(move || super::delete::execute(token)).await
}

#[tauri::command]
pub fn sessions_job() -> Option<super::delete::JobState> { super::delete::job() }

#[tauri::command]
pub fn sessions_cancel() { super::delete::cancel() }
```

`codex_rpc.rs`: move `conversations/codex.rs` and change `Rpc::start(source: &Source)` to `Rpc::start(root: &Path)` (sets `CODEX_HOME` to `root`); keep `call`, `descendants`, `require_closed`, `capabilities`, `hidden`, `command` unchanged; it uses `super::err`.

- [ ] **Step 4: Run tests and checks.**
- [ ] **Step 5: Commit** — `feat: delete sessions with slim export, direct or full backup modes`.

---

### Task 9: Remove the old conversation module

**Files:**
- Delete: `src-tauri/src/conversations/` (all files)
- Modify: `src-tauri/src/lib.rs` (remove `mod conversations;` and the 12 `conversations::` registrations), `src-tauri/src/sessions/mod.rs` (drop `#![allow(dead_code)]`)

- [ ] **Step 1:** `git rm -r src-tauri/src/conversations`, remove registrations, fix compile errors (the `sessions` module must not reference `conversations`).
- [ ] **Step 2:** Delete anything clippy now reports as dead in `sessions` or elsewhere that only served the old module (`crate::dpapi` usage for the model key, if no other caller remains — check with `rg -n "dpapi::" src-tauri/src`).
- [ ] **Step 3:** Full Rust checks.
- [ ] **Step 4: Commit** — `refactor: remove the conversation index module`.

---

### Task 10: Frontend types, API and view helpers

**Files:**
- Create: `src/features/sessions/types.ts`, `src/features/sessions/api.ts`, `src/features/sessions/sessionsView.ts`, `src/features/sessions/sessionsView.test.ts`

**Interfaces:**
- Produces (`types.ts`): TypeScript mirrors of `Session`, `ChildSummary`, `ProjectRef`, `SessionQuery`, `SessionPage`, `ProjectRow`, `Roots`, `RootsView`, `SessionDetail`, `Message`, `DeleteMode`, `DeletePreview`, `Blocked`, `DeleteJob`, `DeleteJobItem` (camelCase fields, `agent: "codex" | "claude"`, `client: "desktop" | "terminal" | "ide" | "automation" | "sdk" | "unknown"`, `status: "active" | "archived" | "orphaned"`, `titleSource`, `DeleteMode = "slim_export" | "direct" | "full_backup"`); `EMPTY_QUERY`; `ERRORS` map and `errorMessage()`.
- Produces (`api.ts`): `listSessions(query)`, `listProjects()`, `readSession(id, offset)`, `getRoots()`, `setRoots(overrides)`, `setFavorite(ids, favorite)`, `openSession(id, target)`, `previewDelete(ids, mode)`, `executeDelete(token)`, `deleteJob()`, `cancelDelete()` — thin `invoke` wrappers with the command names from Tasks 6 and 8.
- Produces (`sessionsView.ts`): `CLIENT_LABEL`, `STATUS_LABEL`, `AGENT_LABEL` records; `toggleSelection(current, id)`, `currentSelection(selected, ids)`; `formatAge(secs, nowSecs)`.

Error messages:

```ts
export const ERRORS: Record<string, string> = {
  E_STORAGE: "无法读写 Stacker 的会话数据目录，请检查剩余空间和目录权限。",
  E_SOURCE_MISSING: "数据目录不存在，请在「数据来源」中检查路径。",
  E_PATH: "路径不存在或不在允许范围内。",
  E_LINK: "涉及符号链接或目录联接，已阻止操作。",
  E_ACCESS: "文件访问被拒绝，请检查权限。",
  E_NOT_FOUND: "会话已不存在，请刷新列表。",
  E_BUSY: "已有删除任务正在执行，请等待或取消。",
  E_REQUEST: "请求无效（单次最多 500 条），请调整选择后重试。",
  E_PREVIEW: "删除预览已过期或已执行，请重新预览。",
  E_CHANGED: "会话在预览后发生了变化，请刷新后重新预览。",
  E_IN_DESKTOP: "该会话仍在 Claude 桌面端侧栏中，请先在桌面端删除。",
  E_IN_USE: "该会话最近 2 分钟内仍有写入，可能正在使用。",
  E_CLOSE_CODEX: "请先完全退出 Codex 桌面端和 CLI。",
  E_PROCESS_CHECK: "无法确认智能体是否已退出，已阻止操作。",
  E_CODEX_MISSING: "未找到可用的 Codex CLI，请在「安装更新」页安装。",
  E_CODEX_VERSION: "本机 Codex 版本过旧，不支持安全删除，请先更新。",
  E_RPC: "Codex 接口未完成操作，请查看任务结果。",
  E_VERIFY: "删除后核对未通过，请刷新检查。",
  E_TIMEOUT: "接口响应超时，请刷新核对结果。",
  E_CANCELLED: "操作已取消，已完成的项目保留。",
};
export function errorMessage(error: unknown): string {
  const text = String(error);
  return ERRORS[text] ?? text;
}
```

- [ ] **Step 1: Write the failing test** (`sessionsView.test.ts`)

```ts
import { describe, expect, it } from "vitest";
import { currentSelection, formatAge, toggleSelection } from "./sessionsView";

describe("session view helpers", () => {
  it("toggles and trims selections", () => {
    expect(toggleSelection(["a"], "b")).toEqual(["a", "b"]);
    expect(toggleSelection(["a", "b"], "a")).toEqual(["b"]);
    expect(currentSelection(["a", "x"], ["a", "b"])).toEqual(["a"]);
  });
  it("formats ages", () => {
    expect(formatAge(1000, 1030)).toBe("刚刚");
    expect(formatAge(1000, 1000 + 3 * 3600)).toBe("3 小时前");
    expect(formatAge(1000, 1000 + 2 * 86400)).toBe("2 天前");
  });
});
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** the three files as specified. `formatAge`: < 60 s → `刚刚`, < 3600 → `${n} 分钟前`, < 86400 → `${n} 小时前`, else `${n} 天前`.
- [ ] **Step 4: Frontend checks.**
- [ ] **Step 5: Commit** — `feat: session catalog frontend types and API`.

---

### Task 11: Session catalog page

**Files:**
- Create: `src/features/sessions/SessionCatalog.tsx`, `SessionList.tsx`, `ProjectList.tsx`, `SourcesPanel.tsx`, `DeleteDialog.tsx`, `SessionDetail.tsx`, `sessions.css`
- Modify: `src/pages/AgentData.tsx` (render `SessionCatalog`)
- Delete: `src/features/conversations/`
- Test: `src/features/sessions/SessionCatalog.test.tsx`

Behavior:

- `SessionCatalog`: tabs 会话 / 项目 / 数据来源 (remembered in a module variable), header with 刷新 (re-queries; backend cache is 10 s) and the current count + total size. Query state lives here; switching from 项目 to 会话 sets `query.project`.
- `SessionList`:
  - Filter row: search box (300 ms debounce), 原文搜索 checkbox, agent select (全部智能体 / Codex / Claude), project select (from `listProjects`), status select (全部状态 / 进行中 / 已归档 / 孤儿), client select (全部来源 / 桌面端 / 终端 / IDE / 自动化 / SDK), 仅收藏 toggle, 包含自动化运行 toggle.
  - Rows: checkbox, favorite star (`<i className="ti ti-star" />` with class `on` when favorite; CSS fills it with `color: var(--acc)` and `-webkit-text-stroke`), title (click → `SessionDetail`), project name + path (muted), client tag chip, `子任务 N` chip (click expands a nested list of `children` titles/sizes), status chip (孤儿 in amber, 已归档 muted), age, size.
  - Pagination 40 per page; 本页 checkbox; 选择全部结果 (uses `page.ids`).
  - Fixed bottom action bar when anything is selected: `已选 N 项 · 约 X`, 收藏 / 取消收藏, 删除…, 清除选择.
- `DeleteDialog`: calls `previewDelete(ids, mode)` on open and whenever the mode radio changes (精简导出后删除 default / 直接删除 / 完整备份后删除, each with one-line explanation). Shows 将删除 N 个会话（含 M 个子任务）· K 个文件 · 可释放 X, the blocked list with `errorMessage(reason)`, and for 精简导出 the export directory. Confirm → `executeDelete(token)`; then polls `deleteJob()` every 800 ms showing progress, per-item results, 取消 while running, and 打开导出目录 when done; on close refreshes the list.
- `SessionDetail`: modal with header (title, agent, client, project path, 打开所在文件夹 / 打开项目 / 在 Codex 中打开 for Codex), messages paged 200 at a time (角色 label + text in a `pre-wrap` block), `部分内容` notice when `complete` is false, and the stored summary (read-only, with 已过期 badge when stale).
- `ProjectList`: table from `listProjects()`: 项目名、路径（`exists` false → 已删除 chip）、智能体、会话数、孤儿数、占用、最后活动; row click → switch to 会话 with that project filter.
- `SourcesPanel`: three rows (Codex 根目录, Claude 根目录, Claude 桌面端索引) showing the effective path, override input with 选择… (tauri dialog `open({ directory: true })`) and 恢复默认; 保存 calls `setRoots`. Also shows the export directory with 打开.

- [ ] **Step 1: Write the failing test** (`SessionCatalog.test.tsx`)

```tsx
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../invoke", () => ({ invoke: vi.fn() }));
import { invoke } from "../../invoke";
import { SessionCatalog } from "./SessionCatalog";

const session = {
  id: "claude:s1", agent: "claude", nativeId: "s1", title: "桌面标题", titleSource: "client",
  project: { key: "p", name: "repo", path: "D:/repo", exists: true }, client: "desktop",
  createdAt: 1, updatedAt: 2, archived: false, pinned: false, status: "active",
  children: [{ id: "c", kind: "subagent", title: "sub", bytes: 1 }], bytes: 10, path: "D:/x.jsonl",
  inDesktopIndex: true, parentMissing: false, favorite: false, summary: null, summaryStale: false,
};

let host: HTMLDivElement; let root: Root;
beforeEach(() => {
  host = document.createElement("div"); document.body.appendChild(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "sessions_list") return { items: [session], total: 1, ids: ["claude:s1"], totalBytes: 10, warnings: [] };
    if (command === "sessions_projects") return [];
    if (command === "sessions_roots") return { effective: { codex: "", claude: "", claudeDesktopIndex: "" }, overrides: { codex: "", claude: "", claudeDesktopIndex: "" }, exportDir: "" };
    if (command === "sessions_job") return null;
    return null;
  });
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

describe("session catalog", () => {
  it("lists client titles with folded subtasks and hides automation by default", async () => {
    vi.useFakeTimers();
    await act(async () => { root.render(<SessionCatalog onCleanup={() => {}} />); });
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    expect(host.textContent).toContain("桌面标题");
    expect(host.textContent).toContain("子任务 1");
    const listCall = vi.mocked(invoke).mock.calls.find(([c]) => c === "sessions_list");
    expect((listCall?.[1] as { query: { includeAutomation: boolean } }).query.includeAutomation).toBe(false);
  });
});
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** the components as described, following the existing page styles (`.a` scoped classes, `gh sm` / `pr sm` buttons, `Modal`, `ConfirmModal`, `Select`, `useToast`, `formatSpaceBytes` from `src/features/space-analysis/components/SpaceOverview`). `AgentData.tsx` becomes:

```tsx
import { SessionCatalog } from "../features/sessions/SessionCatalog";
import type { Page } from "../pageState";

export default function AgentData({ goto }: { goto: (page: Page) => void }) {
  return <SessionCatalog onCleanup={() => goto("cleanup")} />;
}
```

`git rm -r src/features/conversations`.
- [ ] **Step 4:** Add English for every new string reported by `npm run check:i18n`; frontend checks pass.
- [ ] **Step 5: Commit** — `feat: rebuild 会话数据 as a session catalog`.

---

### Task 12: Docs and verification

**Files:**
- Delete: `docs/conversations.md`; Create: `docs/sessions.md` (behavior and safety boundaries: data sources table from the spec, classification rules, deletion modes and blocking rules, what Stacker stores)
- Modify: `docs/development.md` (module table row 会话数据 → `src/features/sessions` / `sessions/`; remove the ignored-test notes for the old reader and Codex lifecycle test, replace with the new ignored Codex lifecycle test if kept), `README.md` / `README.zh-CN.md` (会话与项目 section rewritten to the new behavior), spec status → `已实现`

- [ ] **Step 1:** Update docs.
- [ ] **Step 2:** Full automated checks (Rust + frontend + `npm run build`).
- [ ] **Step 3:** Manual acceptance in the dev app:
  1. 会话 tab default shows about 31 Claude and 70 Codex sessions; titles match the Claude and Codex sidebars.
  2. 包含自动化运行 adds the Codex `exec` runs and Claude SDK sessions.
  3. Status 孤儿 lists Claude desktop sessions deleted in the desktop app.
  4. A session with sub-agents shows 子任务 N and expands.
  5. 项目 tab groups worktree sessions under their repository.
  6. Delete one orphan with 精简导出后删除: the Markdown appears in the export directory and the files are gone; a desktop-sidebar session is blocked with the right message.
  7. The favorite star stays visible after clicking.
- [ ] **Step 4: Commit** — `docs: document the session catalog`.
