//! `webchat.sqlite3`: accounts, conversations, folders and excerpts synced from the extension.
//! Kept apart from `sessions.sqlite3`; bodies live in gzip files (see `bodies`).
use super::protocol::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const FILE: &str = "webchat.sqlite3";

/// Schema versions, applied in order; `PRAGMA user_version` records how many ran.
const MIGRATIONS: &[&str] = &["
CREATE TABLE web_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE web_accounts (
  key TEXT PRIMARY KEY, site TEXT NOT NULL, remote_id TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '', alias TEXT NOT NULL DEFAULT '',
  last_seen INTEGER NOT NULL DEFAULT 0, local_updated_at INTEGER NOT NULL DEFAULT 0);
CREATE TABLE web_conversations (
  key TEXT PRIMARY KEY, site TEXT NOT NULL, account TEXT NOT NULL, id TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0, removed_at INTEGER, listed_at INTEGER NOT NULL DEFAULT 0,
  folder_id TEXT, tags TEXT NOT NULL DEFAULT '[]', favorite INTEGER NOT NULL DEFAULT 0,
  note TEXT NOT NULL DEFAULT '', local_updated_at INTEGER NOT NULL DEFAULT 0,
  body_fetched_at INTEGER, body_updated_at INTEGER, body_messages INTEGER NOT NULL DEFAULT 0,
  summary TEXT NOT NULL DEFAULT '', summary_by TEXT NOT NULL DEFAULT '',
  summary_at INTEGER NOT NULL DEFAULT 0, summary_body_at INTEGER);
CREATE INDEX web_conversations_account ON web_conversations(account);
CREATE TABLE web_folders (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT 0,
  local_updated_at INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER);
CREATE TABLE web_excerpts (
  id TEXT PRIMARY KEY, site TEXT NOT NULL, conversation_id TEXT, url TEXT NOT NULL DEFAULT '',
  page_title TEXT NOT NULL DEFAULT '', text TEXT NOT NULL, note TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL DEFAULT 0, local_updated_at INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER);
CREATE TABLE web_body_parts (
  key TEXT NOT NULL, fetched_at INTEGER NOT NULL, chunk INTEGER NOT NULL, chunks INTEGER NOT NULL,
  messages TEXT NOT NULL, PRIMARY KEY (key, fetched_at, chunk));
"];

fn db_err<E>(_: E) -> String {
    "E_STORAGE".into()
}

pub fn open(root: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(root).map_err(db_err)?;
    let conn = Connection::open(root.join(FILE)).map_err(db_err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(db_err)?;
    // The very first write to a brand-new database file (switching journal mode here, or the
    // first BEGIN IMMEDIATE in migrate() below) can return SQLITE_BUSY immediately when two
    // connections race for it, without `busy_timeout`'s handler ever being invoked to retry it;
    // retry it ourselves.
    retry_busy(|| conn.execute_batch("PRAGMA journal_mode=WAL;"))?;
    migrate(&conn)?;
    Ok(conn)
}

/// Retries `attempt` while it fails with SQLITE_BUSY, for up to 5 seconds.
fn retry_busy<T>(mut attempt: impl FnMut() -> rusqlite::Result<T>) -> Result<T, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match attempt() {
            Ok(v) => return Ok(v),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(e) => return Err(db_err(e)),
        }
    }
}

// BEGIN IMMEDIATE takes the write lock before anything is read, so two connections opening the
// same brand-new database at once cannot both see user_version = 0 and both try to create the
// same tables: the second one waits until the first commits, then re-reads user_version inside
// its own transaction and finds there is nothing left to do.
fn migrate(conn: &Connection) -> Result<(), String> {
    retry_busy(|| conn.execute_batch("BEGIN IMMEDIATE"))?;
    let outcome = run_pending_migrations(conn);
    if outcome.is_ok() {
        conn.execute_batch("COMMIT").map_err(db_err)?;
    } else {
        let _ = conn.execute_batch("ROLLBACK");
    }
    outcome
}

fn run_pending_migrations(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_err)?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
        conn.execute_batch(sql).map_err(db_err)?;
        conn.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
            .map_err(db_err)?;
    }
    Ok(())
}

pub fn meta(conn: &Connection, key: &str) -> Option<i64> {
    conn.query_row("SELECT value FROM web_meta WHERE key=?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .optional()
    .ok()
    .flatten()
    .and_then(|v| v.parse().ok())
}

pub fn set_meta(conn: &Connection, key: &str, value: i64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO web_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value.to_string()],
    )
    .map(|_| ())
    .map_err(db_err)
}

// In an UPDATE, SQLite evaluates every SET expression against the old row, so the CASEs
// compare the incoming time with the stored one before either is replaced.
const ACCOUNT_UPSERT: &str = "
INSERT INTO web_accounts(key,site,remote_id,name,alias,last_seen,local_updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)
ON CONFLICT(key) DO UPDATE SET
  name = CASE WHEN excluded.last_seen >= web_accounts.last_seen THEN excluded.name ELSE web_accounts.name END,
  last_seen = MAX(web_accounts.last_seen, excluded.last_seen),
  alias = CASE WHEN excluded.local_updated_at >= web_accounts.local_updated_at THEN excluded.alias ELSE web_accounts.alias END,
  local_updated_at = MAX(web_accounts.local_updated_at, excluded.local_updated_at)";

const CONVERSATION_UPSERT: &str = "
INSERT INTO web_conversations(key,site,account,id,title,created_at,updated_at,archived,removed_at,listed_at,
  folder_id,tags,favorite,note,local_updated_at)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
ON CONFLICT(key) DO UPDATE SET
  account = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.account ELSE web_conversations.account END,
  title = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.title ELSE web_conversations.title END,
  created_at = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.created_at ELSE web_conversations.created_at END,
  updated_at = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.updated_at ELSE web_conversations.updated_at END,
  archived = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.archived ELSE web_conversations.archived END,
  removed_at = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.removed_at ELSE web_conversations.removed_at END,
  listed_at = MAX(web_conversations.listed_at, excluded.listed_at),
  folder_id = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.folder_id ELSE web_conversations.folder_id END,
  tags = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.tags ELSE web_conversations.tags END,
  favorite = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.favorite ELSE web_conversations.favorite END,
  note = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.note ELSE web_conversations.note END,
  local_updated_at = MAX(web_conversations.local_updated_at, excluded.local_updated_at)";

const FOLDER_UPSERT: &str = "
INSERT INTO web_folders(id,name,created_at,local_updated_at) VALUES(?1,?2,?3,?4)
ON CONFLICT(id) DO UPDATE SET
  name = CASE WHEN excluded.local_updated_at >= web_folders.local_updated_at THEN excluded.name ELSE web_folders.name END,
  local_updated_at = MAX(web_folders.local_updated_at, excluded.local_updated_at)";

const EXCERPT_UPSERT: &str = "
INSERT INTO web_excerpts(id,site,conversation_id,url,page_title,text,note,created_at,local_updated_at)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
ON CONFLICT(id) DO UPDATE SET
  text = CASE WHEN excluded.local_updated_at >= web_excerpts.local_updated_at THEN excluded.text ELSE web_excerpts.text END,
  note = CASE WHEN excluded.local_updated_at >= web_excerpts.local_updated_at THEN excluded.note ELSE web_excerpts.note END,
  local_updated_at = MAX(web_excerpts.local_updated_at, excluded.local_updated_at)";

fn short_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200
}

pub fn upsert_accounts(conn: &Connection, items: &[WebAccount]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for a in items
        .iter()
        .filter(|a| valid_key(&a.site, &a.remote_id, &a.key))
    {
        tx.execute(
            ACCOUNT_UPSERT,
            params![
                a.key,
                a.site,
                a.remote_id,
                a.name,
                a.alias,
                a.last_seen,
                a.local_updated_at
            ],
        )
        .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub fn upsert_conversations(conn: &Connection, items: &[WebConversation]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for c in items.iter().filter(|c| valid_key(&c.site, &c.id, &c.key)) {
        let tags = serde_json::to_string(&c.tags).map_err(db_err)?;
        tx.execute(
            CONVERSATION_UPSERT,
            params![
                c.key,
                c.site,
                c.account,
                c.id,
                c.title,
                c.created_at,
                c.updated_at,
                c.archived,
                c.removed_at,
                c.listed_at,
                c.folder_id,
                tags,
                c.favorite,
                c.note,
                c.local_updated_at
            ],
        )
        .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub fn upsert_folders(conn: &Connection, items: &[WebFolder]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for f in items.iter().filter(|f| short_id(&f.id)) {
        tx.execute(
            FOLDER_UPSERT,
            params![f.id, f.name, f.created_at, f.local_updated_at],
        )
        .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub fn upsert_excerpts(conn: &Connection, items: &[WebExcerpt]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for e in items
        .iter()
        .filter(|e| short_id(&e.id) && is_site(&e.site) && e.text.len() <= 100_000)
    {
        tx.execute(
            EXCERPT_UPSERT,
            params![
                e.id,
                e.site,
                e.conversation_id,
                e.url,
                e.page_title,
                e.text,
                e.note,
                e.created_at,
                e.local_updated_at
            ],
        )
        .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

/// Folder and excerpt deletions become tombstones, so a later sync of an old copy cannot revive them.
pub fn remove_records(conn: &Connection, items: &[Removal]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for r in items {
        let sql = match r.kind.as_str() {
            "folder" => "UPDATE web_folders SET deleted_at=?2 WHERE id=?1 AND deleted_at IS NULL",
            "excerpt" => "UPDATE web_excerpts SET deleted_at=?2 WHERE id=?1 AND deleted_at IS NULL",
            _ => continue,
        };
        tx.execute(sql, params![r.key, r.at]).map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub(crate) fn parse_tags(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub fn accounts(conn: &Connection) -> Result<Vec<WebAccount>, String> {
    let mut stmt = conn
        .prepare("SELECT key,site,remote_id,name,alias,last_seen,local_updated_at FROM web_accounts ORDER BY key")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebAccount {
                key: r.get(0)?,
                site: r.get(1)?,
                remote_id: r.get(2)?,
                name: r.get(3)?,
                alias: r.get(4)?,
                last_seen: r.get(5)?,
                local_updated_at: r.get(6)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn conversations(conn: &Connection) -> Result<Vec<WebConversation>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT key,site,account,id,title,created_at,updated_at,archived,removed_at,listed_at,
                    folder_id,tags,favorite,note,local_updated_at
             FROM web_conversations ORDER BY key",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebConversation {
                key: r.get(0)?,
                site: r.get(1)?,
                account: r.get(2)?,
                id: r.get(3)?,
                title: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
                archived: r.get(7)?,
                removed_at: r.get(8)?,
                listed_at: r.get(9)?,
                folder_id: r.get(10)?,
                tags: parse_tags(&r.get::<_, String>(11)?),
                favorite: r.get(12)?,
                note: r.get(13)?,
                local_updated_at: r.get(14)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn folders(conn: &Connection) -> Result<Vec<WebFolder>, String> {
    let mut stmt = conn
        .prepare("SELECT id,name,created_at,local_updated_at FROM web_folders WHERE deleted_at IS NULL ORDER BY id")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebFolder {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                local_updated_at: r.get(3)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn excerpts(conn: &Connection) -> Result<Vec<WebExcerpt>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id,site,conversation_id,url,page_title,text,note,created_at,local_updated_at
             FROM web_excerpts WHERE deleted_at IS NULL ORDER BY id",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebExcerpt {
                id: r.get(0)?,
                site: r.get(1)?,
                conversation_id: r.get(2)?,
                url: r.get(3)?,
                page_title: r.get(4)?,
                text: r.get(5)?,
                note: r.get(6)?,
                created_at: r.get(7)?,
                local_updated_at: r.get(8)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub accounts: i64,
    pub conversations: i64,
    pub bodies: i64,
    pub folders: i64,
    pub excerpts: i64,
}

pub fn counts(conn: &Connection) -> Result<Counts, String> {
    conn.query_row(
        "SELECT (SELECT count(*) FROM web_accounts),
                (SELECT count(*) FROM web_conversations),
                (SELECT count(*) FROM web_conversations WHERE body_fetched_at IS NOT NULL),
                (SELECT count(*) FROM web_folders WHERE deleted_at IS NULL),
                (SELECT count(*) FROM web_excerpts WHERE deleted_at IS NULL)",
        [],
        |r| {
            Ok(Counts {
                accounts: r.get(0)?,
                conversations: r.get(1)?,
                bodies: r.get(2)?,
                folders: r.get(3)?,
                excerpts: r.get(4)?,
            })
        },
    )
    .map_err(db_err)
}

/// Bytes of JSON per `pullBackup` page, well under the 1 MiB message limit.
pub const PULL_BUDGET: usize = 800_000;

#[derive(Debug, Serialize)]
pub struct PullPage {
    pub items: Vec<Value>,
    pub next: Option<usize>,
}

/// Items from `offset` until `budget` bytes of JSON; always at least one so paging moves on.
pub fn page_by_bytes(all: Vec<Value>, offset: usize, budget: usize) -> PullPage {
    let total = all.len();
    let mut used = 0;
    let mut items = Vec::new();
    for value in all.into_iter().skip(offset) {
        let size = value.to_string().len() + 1;
        if !items.is_empty() && used + size > budget {
            break;
        }
        used += size;
        items.push(value);
    }
    let end = offset + items.len();
    PullPage {
        next: if end < total { Some(end) } else { None },
        items,
    }
}

fn to_values<T: Serialize>(items: Vec<T>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|i| serde_json::to_value(i).ok())
        .collect()
}

/// One page of Stacker's copy of the extension's organizing data, for 「从 Stacker 恢复」.
pub fn pull(
    conn: &Connection,
    section: &str,
    offset: usize,
    budget: usize,
) -> Result<PullPage, String> {
    let all = match section {
        "accounts" => to_values(accounts(conn)?),
        "folders" => to_values(folders(conn)?),
        "conversations" => to_values(conversations(conn)?),
        "excerpts" => to_values(excerpts(conn)?),
        _ => return Err("E_REQUEST".into()),
    };
    Ok(page_by_bytes(all, offset, budget))
}

/// Most pieces one body may be split into (about 57 MB of messages).
pub const MAX_CHUNKS: usize = 64;

const BODY_UPSERT: &str = "
INSERT INTO web_conversations(key,site,account,id,title,updated_at,body_fetched_at,body_updated_at,body_messages)
VALUES(?1,?2,?3,?4,?5,?6,?7,?6,?8)
ON CONFLICT(key) DO UPDATE SET body_fetched_at=excluded.body_fetched_at,
  body_updated_at=excluded.body_updated_at, body_messages=excluded.body_messages";

/// Stores one piece of a body read; once every piece of that read is in, writes the body file.
/// Returns whether the file was written. An older read never replaces a newer one.
pub fn put_body_chunk(conn: &Connection, root: &Path, chunk: &BodyChunk) -> Result<bool, String> {
    if !valid_key(&chunk.site, &chunk.id, &chunk.key)
        || chunk.chunks == 0
        || chunk.chunks > MAX_CHUNKS
        || chunk.chunk >= chunk.chunks
    {
        return Err("E_REQUEST".into());
    }
    let stored: Option<i64> = conn
        .query_row(
            "SELECT body_fetched_at FROM web_conversations WHERE key=?1",
            [&chunk.key],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?
        .flatten();
    if stored.is_some_and(|at| at > chunk.fetched_at) {
        return Ok(false);
    }
    let messages = if chunk.chunks == 1 {
        // A single-chunk read needs no staging, but an earlier multi-chunk read of the
        // same key may have left parts behind; clear them so they never linger.
        conn.execute("DELETE FROM web_body_parts WHERE key=?1", [&chunk.key])
            .map_err(db_err)?;
        chunk.messages.clone()
    } else {
        match collect_parts(conn, chunk)? {
            Some(all) => all,
            None => return Ok(false),
        }
    };
    let count = messages.len() as i64;
    let body = StoredBody {
        key: chunk.key.clone(),
        site: chunk.site.clone(),
        account: chunk.account.clone(),
        id: chunk.id.clone(),
        title: chunk.title.clone(),
        updated_at: chunk.updated_at,
        fetched_at: chunk.fetched_at,
        messages,
    };
    let path = super::bodies::body_path(root, &chunk.site, &chunk.account, &chunk.id);
    super::bodies::write_body(&path, &body)?;
    conn.execute(
        BODY_UPSERT,
        params![
            chunk.key,
            chunk.site,
            chunk.account,
            chunk.id,
            chunk.title,
            chunk.updated_at,
            chunk.fetched_at,
            count
        ],
    )
    .map_err(db_err)?;
    Ok(true)
}

/// Keeps a piece until its siblings arrive; returns every message in order once complete.
fn collect_parts(conn: &Connection, chunk: &BodyChunk) -> Result<Option<Vec<WebMessage>>, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    tx.execute(
        "DELETE FROM web_body_parts WHERE key=?1 AND fetched_at<>?2",
        params![chunk.key, chunk.fetched_at],
    )
    .map_err(db_err)?;
    let json = serde_json::to_string(&chunk.messages).map_err(db_err)?;
    tx.execute(
        "INSERT OR REPLACE INTO web_body_parts(key,fetched_at,chunk,chunks,messages) VALUES(?1,?2,?3,?4,?5)",
        params![chunk.key, chunk.fetched_at, chunk.chunk as i64, chunk.chunks as i64, json],
    )
    .map_err(db_err)?;
    let parts: Vec<String> = {
        let mut stmt = tx
            .prepare(
                "SELECT messages FROM web_body_parts WHERE key=?1 AND fetched_at=?2 ORDER BY chunk",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![chunk.key, chunk.fetched_at], |r| {
                r.get::<_, String>(0)
            })
            .map_err(db_err)?;
        let parts: Result<Vec<String>, _> = rows.collect();
        parts.map_err(db_err)?
    };
    if parts.len() < chunk.chunks {
        tx.commit().map_err(db_err)?;
        return Ok(None);
    }
    let mut all = Vec::new();
    for part in parts {
        all.extend(serde_json::from_str::<Vec<WebMessage>>(&part).map_err(db_err)?);
    }
    tx.execute("DELETE FROM web_body_parts WHERE key=?1", [&chunk.key])
        .map_err(db_err)?;
    tx.commit().map_err(db_err)?;
    Ok(Some(all))
}

pub const WEB_PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebQuery {
    pub site: String,
    pub account: String,
    pub search: String,
    pub full_text: bool,
    pub offset: usize,
}

/// A web chat for the 网页对话 tab. Times are milliseconds (browser clocks).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebChatRow {
    pub key: String,
    pub site: String,
    pub account: String,
    pub account_name: String,
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
    pub removed_at: Option<i64>,
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub note: String,
    pub body_fetched_at: Option<i64>,
    pub body_messages: i64,
    /// The site changed the conversation after its body was read.
    pub body_stale: bool,
    pub summary: Option<String>,
    pub summary_by: String,
    pub summary_at: i64,
    /// A newer body arrived after the summary was written.
    pub summary_stale: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountOption {
    pub key: String,
    pub site: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPage {
    pub items: Vec<WebChatRow>,
    pub total: usize,
    pub accounts: Vec<AccountOption>,
}

const ROW_SELECT: &str = "
SELECT c.key, c.site, c.account, COALESCE(NULLIF(a.alias,''), NULLIF(a.name,''), c.account),
       c.id, c.title, c.created_at, c.updated_at, c.archived, c.removed_at, f.name, c.tags,
       c.favorite, c.note, c.body_fetched_at, c.body_messages, c.body_updated_at,
       c.summary, c.summary_by, c.summary_at, c.summary_body_at
FROM web_conversations c
LEFT JOIN web_accounts a ON a.key = c.account
LEFT JOIN web_folders f ON f.id = c.folder_id AND f.deleted_at IS NULL";

fn chat_row(r: &rusqlite::Row) -> rusqlite::Result<WebChatRow> {
    let updated_at: i64 = r.get(7)?;
    let body_updated_at: Option<i64> = r.get(16)?;
    let summary: String = r.get(17)?;
    let summary_body_at: Option<i64> = r.get(20)?;
    Ok(WebChatRow {
        key: r.get(0)?,
        site: r.get(1)?,
        account: r.get(2)?,
        account_name: r.get(3)?,
        id: r.get(4)?,
        title: r.get(5)?,
        created_at: r.get(6)?,
        updated_at,
        archived: r.get(8)?,
        removed_at: r.get(9)?,
        folder: r.get(10)?,
        tags: parse_tags(&r.get::<_, String>(11)?),
        favorite: r.get(12)?,
        note: r.get(13)?,
        body_fetched_at: r.get(14)?,
        body_messages: r.get(15)?,
        body_stale: body_updated_at.is_some_and(|at| at < updated_at),
        summary_stale: !summary.is_empty() && summary_body_at != body_updated_at,
        summary: Some(summary).filter(|s| !s.is_empty()),
        summary_by: r.get(18)?,
        summary_at: r.get(19)?,
    })
}

fn chat_rows(conn: &Connection) -> Result<Vec<WebChatRow>, String> {
    let mut stmt = conn
        .prepare(&format!("{ROW_SELECT} ORDER BY c.updated_at DESC, c.key"))
        .map_err(db_err)?;
    let rows = stmt.query_map([], chat_row).map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn chat(conn: &Connection, key: &str) -> Result<WebChatRow, String> {
    conn.query_row(&format!("{ROW_SELECT} WHERE c.key=?1"), [key], chat_row)
        .optional()
        .map_err(db_err)?
        .ok_or_else(|| "E_NOT_FOUND".to_string())
}

pub fn body(root: &Path, row: &WebChatRow) -> Result<StoredBody, String> {
    super::bodies::read_body(&super::bodies::body_path(
        root,
        &row.site,
        &row.account,
        &row.id,
    ))
}

fn account_options(conn: &Connection) -> Result<Vec<AccountOption>, String> {
    let mut stmt = conn
        .prepare("SELECT key, site, COALESCE(NULLIF(alias,''), NULLIF(name,''), key) FROM web_accounts ORDER BY site, key")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AccountOption {
                key: r.get(0)?,
                site: r.get(1)?,
                name: r.get(2)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

fn matches_fields(row: &WebChatRow, needle: &str) -> bool {
    row.title.to_lowercase().contains(needle)
        || row.note.to_lowercase().contains(needle)
        || row.tags.iter().any(|t| t.to_lowercase().contains(needle))
        || row
            .summary
            .as_deref()
            .is_some_and(|s| s.to_lowercase().contains(needle))
}

fn body_contains(root: &Path, row: &WebChatRow, needle: &str) -> bool {
    row.body_fetched_at.is_some()
        && body(root, row).is_ok_and(|b| {
            b.messages
                .iter()
                .any(|m| m.text.to_lowercase().contains(needle))
        })
}

/// Newest first; searching bodies unpacks each stored body, so it runs only when asked.
pub fn list(conn: &Connection, root: &Path, q: &WebQuery) -> Result<WebPage, String> {
    let needle = q.search.trim().to_lowercase();
    let matching: Vec<WebChatRow> = chat_rows(conn)?
        .into_iter()
        .filter(|r| q.site.is_empty() || r.site == q.site)
        .filter(|r| q.account.is_empty() || r.account == q.account)
        .filter(|r| {
            needle.is_empty()
                || matches_fields(r, &needle)
                || (q.full_text && body_contains(root, r, &needle))
        })
        .collect();
    Ok(WebPage {
        total: matching.len(),
        items: matching
            .into_iter()
            .skip(q.offset)
            .take(WEB_PAGE_SIZE)
            .collect(),
        accounts: account_options(conn)?,
    })
}

/// Saves a summary and remembers which body it was written from.
pub fn save_summary(
    conn: &Connection,
    key: &str,
    text: &str,
    by: &str,
    at: i64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE web_conversations SET summary=?2, summary_by=?3, summary_at=?4, summary_body_at=body_updated_at WHERE key=?1",
        params![key, text, by, at],
    )
    .map(|_| ())
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(id: &str, listed_at: i64, local_at: i64) -> WebConversation {
        WebConversation {
            key: format!("chatgpt:{id}"),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: id.into(),
            title: format!("Title {id}"),
            created_at: 1,
            updated_at: 2,
            listed_at,
            local_updated_at: local_at,
            ..Default::default()
        }
    }

    #[test]
    fn migrations_run_once() {
        let dir = tempfile::tempdir().unwrap();
        drop(open(dir.path()).unwrap());
        let conn = open(dir.path()).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        set_meta(&conn, "last_sync_at", 42).unwrap();
        assert_eq!(meta(&conn, "last_sync_at"), Some(42));
        assert_eq!(meta(&conn, "missing"), None);
    }

    /// A UI process and the bridge can both open a brand-new database at the same moment. Without
    /// BEGIN IMMEDIATE, both can read user_version = 0 before either creates the tables, and the
    /// second one then fails with "table already exists". With it, the second one waits and finds
    /// the tables already there.
    #[test]
    fn two_connections_opening_the_same_new_database_do_not_race() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().to_path_buf();
        let b = dir.path().to_path_buf();
        let t1 = std::thread::spawn(move || open(&a));
        let t2 = std::thread::spawn(move || open(&b));
        let conn1 = t1.join().unwrap().unwrap();
        let conn2 = t2.join().unwrap().unwrap();
        for conn in [&conn1, &conn2] {
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(version, MIGRATIONS.len() as i64);
        }
        // Both connections agree on one, unduplicated schema.
        set_meta(&conn1, "k", 1).unwrap();
        assert_eq!(meta(&conn2, "k"), Some(1));
    }

    #[test]
    fn accounts_keep_the_newest_name_and_the_newest_alias() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let a = WebAccount {
            key: "chatgpt:u1".into(),
            site: "chatgpt".into(),
            remote_id: "u1".into(),
            name: "Ada".into(),
            alias: String::new(),
            last_seen: 10,
            local_updated_at: 0,
        };
        assert_eq!(upsert_accounts(&conn, std::slice::from_ref(&a)).unwrap(), 1);
        let renamed = WebAccount {
            name: "Old name".into(),
            last_seen: 5,
            alias: "Work".into(),
            local_updated_at: 20,
            ..a.clone()
        };
        upsert_accounts(&conn, &[renamed]).unwrap();
        let wrong_key = WebAccount {
            key: "claude:u1".into(),
            ..a.clone()
        };
        assert_eq!(upsert_accounts(&conn, &[wrong_key]).unwrap(), 0);
        let saved = accounts(&conn).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(
            (saved[0].name.as_str(), saved[0].alias.as_str()),
            ("Ada", "Work")
        );
        assert_eq!((saved[0].last_seen, saved[0].local_updated_at), (10, 20));
    }

    #[test]
    fn newer_listing_wins_site_fields_and_newer_edit_wins_local_fields() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let mut first = conv("a", 10, 10);
        first.note = "first note".into();
        upsert_conversations(&conn, &[first]).unwrap();
        // An older listing carrying a newer local edit: the title stays, the local fields change.
        let mut edit = conv("a", 5, 20);
        edit.title = "Old title".into();
        edit.note = "edited".into();
        edit.favorite = true;
        edit.tags = vec!["work".into()];
        edit.folder_id = Some("f1".into());
        upsert_conversations(&conn, &[edit]).unwrap();
        // A newer listing carrying an older local copy: the title changes, the local fields stay.
        let mut relist = conv("a", 30, 15);
        relist.title = "Renamed".into();
        relist.removed_at = Some(30);
        relist.note = "stale".into();
        upsert_conversations(&conn, &[relist]).unwrap();
        let saved = conversations(&conn).unwrap().remove(0);
        assert_eq!(
            (saved.title.as_str(), saved.removed_at, saved.listed_at),
            ("Renamed", Some(30), 30)
        );
        assert_eq!(
            (saved.note.as_str(), saved.favorite, saved.local_updated_at),
            ("edited", true, 20)
        );
        assert_eq!(saved.tags, vec!["work".to_string()]);
        assert_eq!(saved.folder_id.as_deref(), Some("f1"));
    }

    #[test]
    fn invalid_conversations_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let mut bad_key = conv("a", 1, 1);
        bad_key.key = "chatgpt:b".into();
        let mut bad_site = conv("c", 1, 1);
        bad_site.site = "Chat GPT".into();
        bad_site.key = "Chat GPT:c".into();
        assert_eq!(
            upsert_conversations(&conn, &[bad_key, bad_site, conv("d", 1, 1)]).unwrap(),
            1
        );
        assert_eq!(counts(&conn).unwrap().conversations, 1);
    }

    #[test]
    fn deleted_folders_and_excerpts_stay_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let folder = WebFolder {
            id: "f1".into(),
            name: "Trips".into(),
            created_at: 1,
            local_updated_at: 5,
        };
        upsert_folders(&conn, std::slice::from_ref(&folder)).unwrap();
        let renamed = WebFolder {
            name: "Travel".into(),
            local_updated_at: 6,
            ..folder.clone()
        };
        upsert_folders(&conn, &[renamed]).unwrap();
        assert_eq!(folders(&conn).unwrap()[0].name, "Travel");
        let excerpt = WebExcerpt {
            id: "e1".into(),
            site: "chatgpt".into(),
            conversation_id: Some("a".into()),
            text: "tip".into(),
            created_at: 1,
            local_updated_at: 1,
            ..Default::default()
        };
        let bad_excerpt = WebExcerpt {
            id: "e2".into(),
            site: "Bad Site".into(),
            ..excerpt.clone()
        };
        assert_eq!(upsert_excerpts(&conn, &[excerpt, bad_excerpt]).unwrap(), 1);
        let removed = remove_records(
            &conn,
            &[
                Removal {
                    kind: "folder".into(),
                    key: "f1".into(),
                    at: 9,
                },
                Removal {
                    kind: "bogus".into(),
                    key: "x".into(),
                    at: 9,
                },
            ],
        )
        .unwrap();
        assert_eq!(removed, 1);
        let later = WebFolder {
            name: "Back".into(),
            local_updated_at: 99,
            ..folder
        };
        upsert_folders(&conn, &[later]).unwrap();
        assert!(
            folders(&conn).unwrap().is_empty(),
            "a deleted folder never comes back"
        );
        let c = counts(&conn).unwrap();
        assert_eq!((c.folders, c.excerpts, c.bodies), (0, 1, 0));
    }

    fn chunk(chunk: usize, chunks: usize, fetched_at: i64, text: &str) -> BodyChunk {
        BodyChunk {
            key: "chatgpt:a".into(),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: "a".into(),
            title: "A".into(),
            updated_at: 7,
            fetched_at,
            chunk,
            chunks,
            messages: vec![WebMessage {
                role: "user".into(),
                text: text.into(),
                at: None,
                attachments: vec![],
            }],
        }
    }

    fn body_texts(root: &Path) -> Vec<String> {
        let path = super::super::bodies::body_path(root, "chatgpt", "chatgpt:u1", "a");
        super::super::bodies::read_body(&path)
            .unwrap()
            .messages
            .into_iter()
            .map(|m| m.text)
            .collect()
    }

    #[test]
    fn a_single_chunk_is_stored_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        assert!(put_body_chunk(&conn, dir.path(), &chunk(0, 1, 100, "hello")).unwrap());
        assert_eq!(body_texts(dir.path()), vec!["hello"]);
        assert_eq!(counts(&conn).unwrap().bodies, 1);
    }

    #[test]
    fn chunks_are_assembled_in_order_whatever_order_they_arrive() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        assert!(!put_body_chunk(&conn, dir.path(), &chunk(1, 2, 100, "second")).unwrap());
        assert!(put_body_chunk(&conn, dir.path(), &chunk(0, 2, 100, "first")).unwrap());
        assert_eq!(body_texts(dir.path()), vec!["first", "second"]);
        let parts: i64 = conn
            .query_row("SELECT count(*) FROM web_body_parts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            parts, 0,
            "staged parts are cleared once the body is written"
        );
    }

    #[test]
    fn an_older_read_never_replaces_a_newer_body() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        put_body_chunk(&conn, dir.path(), &chunk(0, 1, 200, "new")).unwrap();
        assert!(!put_body_chunk(&conn, dir.path(), &chunk(0, 1, 100, "old")).unwrap());
        assert_eq!(body_texts(dir.path()), vec!["new"]);
    }

    #[test]
    fn a_body_before_its_listing_creates_a_row_the_listing_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        put_body_chunk(&conn, dir.path(), &chunk(0, 1, 100, "x")).unwrap();
        upsert_conversations(&conn, &[conv("a", 50, 0)]).unwrap();
        let saved = conversations(&conn).unwrap().remove(0);
        assert_eq!(saved.title, "Title a");
        assert_eq!(counts(&conn).unwrap().bodies, 1);
    }

    #[test]
    fn bad_chunk_numbers_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        for bad in [
            chunk(2, 2, 1, "x"),
            chunk(0, 0, 1, "x"),
            chunk(0, MAX_CHUNKS + 1, 1, "x"),
        ] {
            assert_eq!(
                put_body_chunk(&conn, dir.path(), &bad).unwrap_err(),
                "E_REQUEST"
            );
        }
        let mut wrong_key = chunk(0, 1, 1, "x");
        wrong_key.key = "chatgpt:b".into();
        assert_eq!(
            put_body_chunk(&conn, dir.path(), &wrong_key).unwrap_err(),
            "E_REQUEST"
        );
    }

    fn seed(conn: &Connection, root: &Path) {
        let account = |key: &str, site: &str, remote: &str, name: &str, alias: &str| WebAccount {
            key: key.into(),
            site: site.into(),
            remote_id: remote.into(),
            name: name.into(),
            alias: alias.into(),
            last_seen: 1,
            local_updated_at: 1,
        };
        upsert_accounts(
            conn,
            &[
                account("chatgpt:u1", "chatgpt", "u1", "Ada", "Work"),
                account("claude:o1", "claude", "o1", "Claude", ""),
            ],
        )
        .unwrap();
        upsert_folders(
            conn,
            &[WebFolder {
                id: "f1".into(),
                name: "Trips".into(),
                created_at: 1,
                local_updated_at: 1,
            }],
        )
        .unwrap();
        let mut a = conv("a", 10, 10);
        a.title = "Trip plan".into();
        a.folder_id = Some("f1".into());
        a.tags = vec!["travel".into()];
        a.updated_at = 20;
        let b = WebConversation {
            key: "claude:b".into(),
            site: "claude".into(),
            account: "claude:o1".into(),
            id: "b".into(),
            title: "Rust lifetimes".into(),
            note: "borrowck".into(),
            updated_at: 30,
            listed_at: 10,
            ..Default::default()
        };
        upsert_conversations(conn, &[a, b]).unwrap();
        let mut body = chunk(0, 1, 25, "We should visit Kyoto");
        body.updated_at = 20;
        put_body_chunk(conn, root, &body).unwrap();
    }

    fn keys(page: &WebPage) -> Vec<&str> {
        page.items.iter().map(|r| r.key.as_str()).collect()
    }

    #[test]
    fn lists_newest_first_with_names_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        seed(&conn, dir.path());
        let all = list(&conn, dir.path(), &WebQuery::default()).unwrap();
        assert_eq!(keys(&all), vec!["claude:b", "chatgpt:a"]);
        assert_eq!(all.total, 2);
        let a = &all.items[1];
        assert_eq!(
            (a.account_name.as_str(), a.folder.as_deref()),
            ("Work", Some("Trips"))
        );
        assert_eq!(a.body_messages, 1);
        assert_eq!(all.items[0].account_name, "Claude");
        assert_eq!(all.accounts.len(), 2);
        let site = WebQuery {
            site: "chatgpt".into(),
            ..Default::default()
        };
        assert_eq!(
            keys(&list(&conn, dir.path(), &site).unwrap()),
            vec!["chatgpt:a"]
        );
        let account = WebQuery {
            account: "claude:o1".into(),
            ..Default::default()
        };
        assert_eq!(
            keys(&list(&conn, dir.path(), &account).unwrap()),
            vec!["claude:b"]
        );
    }

    #[test]
    fn search_matches_titles_notes_tags_and_bodies_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        seed(&conn, dir.path());
        let search = |text: &str, full_text: bool| {
            let q = WebQuery {
                search: text.into(),
                full_text,
                ..Default::default()
            };
            keys(&list(&conn, dir.path(), &q).unwrap())
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(search("BORROWCK", false), vec!["claude:b"]);
        assert_eq!(search("travel", false), vec!["chatgpt:a"]);
        assert!(search("kyoto", false).is_empty());
        assert_eq!(search("kyoto", true), vec!["chatgpt:a"]);
    }

    #[test]
    fn stale_bodies_and_summaries_are_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        seed(&conn, dir.path());
        assert!(!chat(&conn, "chatgpt:a").unwrap().body_stale);
        save_summary(
            &conn,
            "chatgpt:a",
            "Go to Kyoto",
            "claude / sonnet / low",
            50,
        )
        .unwrap();
        let summarized = chat(&conn, "chatgpt:a").unwrap();
        assert_eq!(summarized.summary.as_deref(), Some("Go to Kyoto"));
        assert!(!summarized.summary_stale);
        let mut newer = conv("a", 40, 0);
        newer.updated_at = 40;
        upsert_conversations(&conn, &[newer]).unwrap();
        assert!(chat(&conn, "chatgpt:a").unwrap().body_stale);
        let mut fresh = chunk(0, 1, 60, "Now Osaka");
        fresh.updated_at = 40;
        put_body_chunk(&conn, dir.path(), &fresh).unwrap();
        let after = chat(&conn, "chatgpt:a").unwrap();
        assert!(!after.body_stale && after.summary_stale);
        assert_eq!(chat(&conn, "chatgpt:zzz").unwrap_err(), "E_NOT_FOUND");
    }
}
