use super::{model::*, *};
use rusqlite::{params, Connection, OptionalExtension};

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

pub fn connect_at(root: &Path) -> Result<Connection, String> {
    fs::create_dir_all(root).map_err(err)?;
    let db = Connection::open(root.join("index.sqlite3")).map_err(err)?;
    db.busy_timeout(Duration::from_secs(5)).map_err(err)?;
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(err)?;
    if version > 1 {
        return Err("E_SCHEMA".into());
    }
    db.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS conversations (id TEXT PRIMARY KEY, source TEXT NOT NULL,
            path TEXT NOT NULL, fingerprint TEXT NOT NULL, data TEXT NOT NULL, present INTEGER NOT NULL DEFAULT 1);
        CREATE TABLE IF NOT EXISTS annotations (id TEXT PRIMARY KEY, favorite INTEGER NOT NULL DEFAULT 0,
            hidden INTEGER NOT NULL DEFAULT 0, group_name TEXT NOT NULL DEFAULT '',
            summary TEXT NOT NULL DEFAULT '', summary_fingerprint TEXT NOT NULL DEFAULT '');
        PRAGMA user_version=1;").map_err(err)?;
    Ok(db)
}

pub fn meta<T: serde::de::DeserializeOwned>(
    db: &Connection,
    key: &str,
) -> Result<Option<T>, String> {
    let value: Option<String> = db
        .query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))
        .optional()
        .map_err(err)?;
    value
        .map(|s| serde_json::from_str(&s).map_err(|_| "E_CORRUPT_INDEX".to_string()))
        .transpose()
}

pub fn set_meta<T: Serialize>(db: &Connection, key: &str, value: &T) -> Result<(), String> {
    db.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, serde_json::to_string(value).map_err(err)?]).map_err(err)?;
    Ok(())
}

pub fn upsert(db: &Connection, c: &Conversation) -> Result<(), String> {
    db.execute("INSERT INTO conversations(id,source,path,fingerprint,data,present) VALUES(?1,?2,?3,?4,?5,1)
        ON CONFLICT(id) DO UPDATE SET source=excluded.source,path=excluded.path,fingerprint=excluded.fingerprint,data=excluded.data,present=1",
        params![c.id,c.source_id,c.path,c.fingerprint,serde_json::to_string(c).map_err(err)?]).map_err(err)?;
    Ok(())
}

pub fn all(db: &Connection) -> Result<Vec<Conversation>, String> {
    let mut stmt = db.prepare("SELECT c.data,COALESCE(a.favorite,0),COALESCE(a.hidden,0),COALESCE(a.group_name,''),
        COALESCE(a.summary,''),COALESCE(a.summary_fingerprint,'') FROM conversations c LEFT JOIN annotations a ON c.id=a.id WHERE c.present=1").map_err(err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(err)?;
    let mut result = Vec::new();
    for row in rows {
        let (data, favorite, hidden, group, summary, fingerprint) = row.map_err(err)?;
        let mut c: Conversation =
            serde_json::from_str(&data).map_err(|_| "E_CORRUPT_INDEX".to_string())?;
        c.favorite = favorite;
        c.hidden = hidden;
        c.group_name = group;
        c.summary = summary;
        c.summary_fingerprint = fingerprint;
        result.push(c);
    }
    result.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.id.cmp(&b.id)));
    Ok(result)
}

pub fn get(db: &Connection, id: &str) -> Result<Conversation, String> {
    let row=db.query_row("SELECT c.data,COALESCE(a.favorite,0),COALESCE(a.hidden,0),COALESCE(a.group_name,''),
        COALESCE(a.summary,''),COALESCE(a.summary_fingerprint,'') FROM conversations c LEFT JOIN annotations a ON c.id=a.id WHERE c.id=? AND c.present=1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,bool>(1)?,r.get::<_,bool>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?))).optional().map_err(err)?.ok_or("E_NOT_FOUND")?;
    let mut c: Conversation = serde_json::from_str(&row.0).map_err(|_| "E_CORRUPT_INDEX")?;
    c.favorite = row.1;
    c.hidden = row.2;
    c.group_name = row.3;
    c.summary = row.4;
    c.summary_fingerprint = row.5;
    Ok(c)
}

pub fn annotate(db: &Connection, ids: &[String], field: &str, value: &str) -> Result<(), String> {
    if !["favorite", "hidden", "group_name"].contains(&field)
        || value.len() > 512
        || ids.len() > 10_000
        || (field != "group_name" && !matches!(value, "0" | "1"))
    {
        return Err("E_REQUEST".into());
    }
    let tx = db.unchecked_transaction().map_err(err)?;
    for id in ids {
        get(&tx, id)?;
        tx.execute("INSERT OR IGNORE INTO annotations(id) VALUES(?)", [id])
            .map_err(err)?;
        tx.execute(
            &format!("UPDATE annotations SET {field}=?1 WHERE id=?2"),
            params![value, id],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)
}

pub fn summary(db: &Connection, c: &Conversation, text: &str) -> Result<(), String> {
    db.execute("INSERT INTO annotations(id,summary,summary_fingerprint) VALUES(?1,?2,?3)
        ON CONFLICT(id) DO UPDATE SET summary=excluded.summary,summary_fingerprint=excluded.summary_fingerprint",
        params![c.id,text,c.fingerprint]).map_err(err)?;
    Ok(())
}

pub fn atomic_json<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let parent = path.parent().ok_or("E_PATH")?;
    fs::create_dir_all(parent).map_err(err)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    serde_json::to_writer_pretty(tmp.as_file_mut(), data).map_err(err)?;
    tmp.as_file().sync_all().map_err(err)?;
    tmp.persist(path).map_err(err)?;
    Ok(())
}
