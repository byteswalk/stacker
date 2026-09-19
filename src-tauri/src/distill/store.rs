//! 提炼结果的读写：`webchat.sqlite3` 里的 `distill_results` 与 `distill_sources`。
use super::DistillSource;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

fn db_err<E>(_: E) -> String {
    "E_STORAGE".into()
}

/// 一条提炼结果。时间是毫秒，和网页对话一致。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DistillResult {
    pub id: String,
    /// qa | requirement | prompt | skill
    pub kind: String,
    pub title: String,
    pub body: String,
    pub sources: Vec<DistillSource>,
    /// draft | adopted
    pub state: String,
    /// 执行者标签，例如 "claude / sonnet / low"。
    pub by: String,
    /// skill 草稿的文件夹名；其他类型为空。
    pub folder: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DistillQuery {
    pub kind: String,
    pub state: String,
    pub search: String,
    /// 只看某一个来源的结果（`DistillSource::key`）。
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindCounts {
    pub qa: i64,
    pub requirement: i64,
    pub prompt: i64,
    pub skill: i64,
    pub total: i64,
}

const SELECT: &str =
    "SELECT id,kind,title,body,sources,state,by_runner,folder,created_at,updated_at FROM distill_results";

fn row(r: &rusqlite::Row) -> rusqlite::Result<DistillResult> {
    let sources: String = r.get(4)?;
    Ok(DistillResult {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        body: r.get(3)?,
        sources: serde_json::from_str(&sources).unwrap_or_default(),
        state: r.get(5)?,
        by: r.get(6)?,
        folder: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

/// 写入一条结果；来源同时写进 `distill_sources`，插件按对话反查走它。
pub fn insert(conn: &Connection, result: &DistillResult) -> Result<(), String> {
    if !super::is_kind(&result.kind) || !super::is_state(&result.state) || result.id.is_empty() {
        return Err("E_REQUEST".into());
    }
    let sources = serde_json::to_string(&result.sources).map_err(db_err)?;
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    tx.execute(
        "INSERT OR REPLACE INTO distill_results(id,kind,title,body,sources,state,by_runner,folder,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            result.id,
            result.kind,
            result.title,
            result.body,
            sources,
            result.state,
            result.by,
            result.folder,
            result.created_at,
            result.updated_at
        ],
    )
    .map_err(db_err)?;
    tx.execute(
        "DELETE FROM distill_sources WHERE result_id=?1",
        [&result.id],
    )
    .map_err(db_err)?;
    for source in &result.sources {
        tx.execute(
            "INSERT OR IGNORE INTO distill_sources(result_id,source_key) VALUES(?1,?2)",
            params![result.id, source.key],
        )
        .map_err(db_err)?;
    }
    tx.commit().map_err(db_err)
}

pub fn get(conn: &Connection, id: &str) -> Result<DistillResult, String> {
    conn.query_row(&format!("{SELECT} WHERE id=?1"), [id], row)
        .optional()
        .map_err(db_err)?
        .ok_or_else(|| "E_NOT_FOUND".to_string())
}

fn matches(r: &DistillResult, needle: &str) -> bool {
    r.title.to_lowercase().contains(needle)
        || r.body.to_lowercase().contains(needle)
        || r.sources
            .iter()
            .any(|s| s.title.to_lowercase().contains(needle))
}

fn all(conn: &Connection) -> Result<Vec<DistillResult>, String> {
    let mut stmt = conn
        .prepare(&format!("{SELECT} ORDER BY updated_at DESC, id"))
        .map_err(db_err)?;
    let rows = stmt.query_map([], row).map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

/// 最近更新的在前；筛选在内存里做（结果量是人工规模的）。
pub fn list(conn: &Connection, q: &DistillQuery) -> Result<Vec<DistillResult>, String> {
    let needle = q.search.trim().to_lowercase();
    Ok(all(conn)?
        .into_iter()
        .filter(|r| q.kind.is_empty() || r.kind == q.kind)
        .filter(|r| q.state.is_empty() || r.state == q.state)
        .filter(|r| q.source.is_empty() || r.sources.iter().any(|s| s.key == q.source))
        .filter(|r| needle.is_empty() || matches(r, &needle))
        .collect())
}

/// 按来源反查（插件用），走 `distill_sources` 索引。
pub fn for_source(conn: &Connection, source_key: &str) -> Result<Vec<DistillResult>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "{SELECT} WHERE id IN (SELECT result_id FROM distill_sources WHERE source_key=?1)
             ORDER BY updated_at DESC, id"
        ))
        .map_err(db_err)?;
    let rows = stmt.query_map([source_key], row).map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

fn one_row(changed: usize) -> Result<(), String> {
    if changed == 0 {
        return Err("E_NOT_FOUND".into());
    }
    Ok(())
}

pub fn save_text(
    conn: &Connection,
    id: &str,
    title: &str,
    body: &str,
    at: i64,
) -> Result<(), String> {
    let changed = conn
        .execute(
            "UPDATE distill_results SET title=?2, body=?3, updated_at=?4 WHERE id=?1",
            params![id, title, body, at],
        )
        .map_err(db_err)?;
    one_row(changed)
}

pub fn set_state(conn: &Connection, id: &str, state: &str, at: i64) -> Result<(), String> {
    if !super::is_state(state) {
        return Err("E_REQUEST".into());
    }
    let changed = conn
        .execute(
            "UPDATE distill_results SET state=?2, updated_at=?3 WHERE id=?1",
            params![id, state, at],
        )
        .map_err(db_err)?;
    one_row(changed)
}

/// 删除结果本身与它的反查索引；skill 草稿文件夹保留在磁盘上（用户自己处理）。
pub fn delete(conn: &Connection, id: &str) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    tx.execute("DELETE FROM distill_sources WHERE result_id=?1", [id])
        .map_err(db_err)?;
    let changed = tx
        .execute("DELETE FROM distill_results WHERE id=?1", [id])
        .map_err(db_err)?;
    tx.commit().map_err(db_err)?;
    one_row(changed)
}

pub fn counts(conn: &Connection) -> Result<KindCounts, String> {
    conn.query_row(
        "SELECT (SELECT count(*) FROM distill_results WHERE kind='qa'),
                (SELECT count(*) FROM distill_results WHERE kind='requirement'),
                (SELECT count(*) FROM distill_results WHERE kind='prompt'),
                (SELECT count(*) FROM distill_results WHERE kind='skill'),
                (SELECT count(*) FROM distill_results)",
        [],
        |r| {
            Ok(KindCounts {
                qa: r.get(0)?,
                requirement: r.get(1)?,
                prompt: r.get(2)?,
                skill: r.get(3)?,
                total: r.get(4)?,
            })
        },
    )
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webchat::store::open;

    fn source(key: &str, title: &str) -> DistillSource {
        DistillSource {
            key: key.into(),
            kind: key.split(':').next().unwrap_or("web").into(),
            title: title.into(),
            link: String::new(),
        }
    }

    fn result(id: &str, kind: &str, title: &str, keys: &[&str]) -> DistillResult {
        DistillResult {
            id: id.into(),
            kind: kind.into(),
            title: title.into(),
            body: format!("Body of {title}"),
            sources: keys.iter().map(|k| source(k, "Trip plan")).collect(),
            state: "draft".into(),
            by: "claude / sonnet / low".into(),
            folder: String::new(),
            created_at: 10,
            updated_at: 10,
        }
    }

    #[test]
    fn results_and_their_sources_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        insert(
            &conn,
            &result("r1", "qa", "Hide the console window", &["web:chatgpt:a"]),
        )
        .unwrap();
        insert(
            &conn,
            &result(
                "r2",
                "requirement",
                "Errors are stable codes",
                &["web:chatgpt:a", "session:codex:s1"],
            ),
        )
        .unwrap();
        let saved = get(&conn, "r1").unwrap();
        assert_eq!(saved.title, "Hide the console window");
        assert_eq!(saved.sources.len(), 1);
        assert_eq!(saved.sources[0].key, "web:chatgpt:a");
        assert_eq!(get(&conn, "missing").unwrap_err(), "E_NOT_FOUND");
        // 反查索引：按来源找结果。
        let by_source = for_source(&conn, "web:chatgpt:a").unwrap();
        assert_eq!(by_source.len(), 2);
        assert_eq!(for_source(&conn, "session:codex:s1").unwrap().len(), 1);
        assert!(for_source(&conn, "web:chatgpt:zzz").unwrap().is_empty());
        let c = counts(&conn).unwrap();
        assert_eq!(
            (c.qa, c.requirement, c.prompt, c.skill, c.total),
            (1, 1, 0, 0, 2)
        );
    }

    #[test]
    fn listing_filters_by_kind_state_source_and_text() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        insert(
            &conn,
            &result("r1", "qa", "Hide the console window", &["web:chatgpt:a"]),
        )
        .unwrap();
        let mut newer = result("r2", "prompt", "Review prompt", &["session:codex:s1"]);
        newer.updated_at = 20;
        insert(&conn, &newer).unwrap();
        let ids = |q: &DistillQuery| {
            list(&conn, q)
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>()
        };
        // 最近更新的在前。
        assert_eq!(ids(&DistillQuery::default()), vec!["r2", "r1"]);
        assert_eq!(
            ids(&DistillQuery {
                kind: "qa".into(),
                ..Default::default()
            }),
            vec!["r1"]
        );
        assert_eq!(
            ids(&DistillQuery {
                source: "session:codex:s1".into(),
                ..Default::default()
            }),
            vec!["r2"]
        );
        assert_eq!(
            ids(&DistillQuery {
                search: "CONSOLE".into(),
                ..Default::default()
            }),
            vec!["r1"]
        );
        assert_eq!(
            ids(&DistillQuery {
                search: "body of review".into(),
                ..Default::default()
            }),
            vec!["r2"]
        );
        assert!(ids(&DistillQuery {
            state: "adopted".into(),
            ..Default::default()
        })
        .is_empty());
    }

    #[test]
    fn editing_adopting_and_deleting() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        insert(
            &conn,
            &result("r1", "qa", "Hide the console window", &["web:chatgpt:a"]),
        )
        .unwrap();
        save_text(&conn, "r1", "Hide it", "New body", 30).unwrap();
        let edited = get(&conn, "r1").unwrap();
        assert_eq!(
            (edited.title.as_str(), edited.body.as_str()),
            ("Hide it", "New body")
        );
        assert_eq!(edited.updated_at, 30);
        assert_eq!(
            save_text(&conn, "gone", "x", "y", 30).unwrap_err(),
            "E_NOT_FOUND"
        );
        set_state(&conn, "r1", "adopted", 40).unwrap();
        assert_eq!(get(&conn, "r1").unwrap().state, "adopted");
        assert_eq!(
            set_state(&conn, "r1", "nonsense", 40).unwrap_err(),
            "E_REQUEST"
        );
        delete(&conn, "r1").unwrap();
        assert_eq!(get(&conn, "r1").unwrap_err(), "E_NOT_FOUND");
        assert!(
            for_source(&conn, "web:chatgpt:a").unwrap().is_empty(),
            "删除结果时反查索引一起删"
        );
    }

    #[test]
    fn an_unknown_kind_or_state_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let mut bad = result("r1", "poem", "x", &[]);
        assert_eq!(insert(&conn, &bad).unwrap_err(), "E_REQUEST");
        bad.kind = "qa".into();
        bad.state = "published".into();
        assert_eq!(insert(&conn, &bad).unwrap_err(), "E_REQUEST");
    }
}
