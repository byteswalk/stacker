//! `webchat.sqlite3`: accounts, conversations, folders and excerpts synced from the extension.
//! Kept apart from `sessions.sqlite3`; bodies live in gzip files (see `bodies`).
use super::protocol::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
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
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(db_err)?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_err)?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        tx.execute_batch(sql).map_err(db_err)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
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
}
