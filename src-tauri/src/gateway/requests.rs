//! What the API service was asked to do: one row per request, kept in Stacker's own database
//! so the page can search it, remove rows and forget old ones on a schedule. Only the
//! endpoint, model, status and timing are recorded — never a prompt or a reply.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRow {
    pub id: i64,
    pub at: u64,
    pub endpoint: String,
    pub model: String,
    pub status: u16,
    pub elapsed_ms: u64,
    /// What else is known about the call, as JSON: who called, how, sizes, the error. Never
    /// the content of a message.
    pub detail: String,
}

/// The facts about one call that are not its content, for the request's details.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Detail {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogQuery {
    /// Matches the endpoint or the model.
    pub search: String,
    /// "" | "ok" (below 400) | "error" (400 and above).
    pub outcome: String,
    /// Only rows from this agent, by the part of the model before the slash.
    pub agent: String,
    /// Only rows at or after this second.
    pub since: u64,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub items: Vec<LogRow>,
    pub total: usize,
    /// Rows in the whole log, whatever the filter matched.
    pub kept: usize,
    /// Agents seen in the log, for the filter.
    pub agents: Vec<String>,
}

pub const PAGE: usize = 50;

fn ensure(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gateway_requests (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at INTEGER NOT NULL,
            endpoint TEXT NOT NULL,
            model TEXT NOT NULL,
            status INTEGER NOT NULL,
            elapsed_ms INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS gateway_requests_at ON gateway_requests(at);",
    )
    .map_err(crate::sessions::err)?;
    // Logs from before details were kept get the column, empty.
    let has_detail = conn
        .prepare("SELECT detail FROM gateway_requests LIMIT 0")
        .is_ok();
    if !has_detail {
        conn.execute_batch(
            "ALTER TABLE gateway_requests ADD COLUMN detail TEXT NOT NULL DEFAULT ''",
        )
        .map_err(crate::sessions::err)?;
    }
    Ok(())
}

fn connect() -> Result<Connection, String> {
    let conn = crate::sessions::annotations::connect()?;
    ensure(&conn)?;
    Ok(conn)
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The endpoint of a row Stacker's own AI features wrote rather than a caller of the service.
pub const INTERNAL: &str = "stacker://internal";

/// The status a failed internal call is logged with: the API's own when it answered, 499 when
/// the user cancelled, 500 for anything else.
pub fn internal_status(result: &Result<String, String>) -> u16 {
    match result {
        Ok(_) => 200,
        Err(error) if error == "E_CANCELLED" => 499,
        Err(error) => error
            .strip_prefix("HTTP ")
            .and_then(|rest| rest.get(..3))
            .and_then(|code| code.parse().ok())
            .unwrap_or(500),
    }
}

/// Logs one call made by Stacker's own AI features, whichever model answered it.
pub fn record_internal(
    model: &str,
    prompt: &str,
    result: &Result<String, String>,
    started: std::time::Instant,
) {
    let detail = Detail {
        client: Some("Stacker".into()),
        input_chars: Some(prompt.chars().count()),
        output_chars: result.as_ref().ok().map(|text| text.chars().count()),
        error: result.as_ref().err().cloned(),
        ..Detail::default()
    };
    record(
        INTERNAL,
        model,
        internal_status(result),
        started.elapsed().as_millis() as u64,
        &detail.to_json(),
    );
}

/// Writes one request, then drops anything older than the retention the user set.
pub fn record(endpoint: &str, model: &str, status: u16, elapsed_ms: u64, detail: &str) {
    let config = super::load();
    if !config.log_enabled {
        return;
    }
    let Ok(conn) = connect() else {
        return;
    };
    let _ = conn.execute(
        "INSERT INTO gateway_requests (at, endpoint, model, status, elapsed_ms, detail) VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![now() as i64, endpoint, model, status as i64, elapsed_ms as i64, detail],
    );
    prune(&conn, config.log_retention_days);
}

/// Forgets rows older than `days`; 0 keeps them until the user removes them.
pub fn prune(conn: &Connection, days: u32) {
    if days == 0 {
        return;
    }
    let cutoff = now().saturating_sub(u64::from(days) * 86_400) as i64;
    let _ = conn.execute("DELETE FROM gateway_requests WHERE at < ?1", [cutoff]);
}

/// The `codex` of `codex/gpt-5`: which agent served the request.
pub fn agent_of(model: &str) -> String {
    model
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// The WHERE clause for a query, with its values.
fn filter(query: &LogQuery) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut sql = String::from(" WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let search = query.search.trim();
    if !search.is_empty() {
        sql.push_str(" AND (LOWER(endpoint) LIKE ?  OR LOWER(model) LIKE ?)");
        let pattern = format!("%{}%", search.to_lowercase());
        args.push(Box::new(pattern.clone()));
        args.push(Box::new(pattern));
    }
    match query.outcome.as_str() {
        "ok" => sql.push_str(" AND status < 400"),
        "error" => sql.push_str(" AND status >= 400"),
        _ => {}
    }
    let agent = query.agent.trim().to_ascii_lowercase();
    if !agent.is_empty() {
        sql.push_str(" AND (LOWER(model) = ? OR LOWER(model) LIKE ?)");
        args.push(Box::new(agent.clone()));
        args.push(Box::new(format!("{agent}/%")));
    }
    if query.since > 0 {
        sql.push_str(" AND at >= ?");
        args.push(Box::new(query.since as i64));
    }
    (sql, args)
}

pub fn list(query: &LogQuery) -> Result<LogPage, String> {
    let conn = connect()?;
    let (where_sql, args) = filter(query);
    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let total: usize = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM gateway_requests{where_sql}"),
            refs.as_slice(),
            |r| r.get::<_, i64>(0),
        )
        .map_err(crate::sessions::err)? as usize;
    let kept: usize = conn
        .query_row("SELECT COUNT(*) FROM gateway_requests", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(crate::sessions::err)? as usize;
    let limit = if query.limit == 0 { PAGE } else { query.limit };
    let sql = format!(
        "SELECT id, at, endpoint, model, status, elapsed_ms, detail FROM gateway_requests{where_sql}
         ORDER BY at DESC, id DESC LIMIT {limit} OFFSET {}",
        query.offset
    );
    let mut stmt = conn.prepare(&sql).map_err(crate::sessions::err)?;
    let items = stmt
        .query_map(refs.as_slice(), |r| {
            Ok(LogRow {
                id: r.get(0)?,
                at: r.get::<_, i64>(1)?.max(0) as u64,
                endpoint: r.get(2)?,
                model: r.get(3)?,
                status: r.get::<_, i64>(4)?.clamp(0, 599) as u16,
                elapsed_ms: r.get::<_, i64>(5)?.max(0) as u64,
                detail: r.get(6)?,
            })
        })
        .map_err(crate::sessions::err)?
        .filter_map(Result::ok)
        .collect();
    let mut agents: Vec<String> = conn
        .prepare("SELECT DISTINCT model FROM gateway_requests")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(0))
                .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .map_err(crate::sessions::err)?
        .iter()
        .map(|model| agent_of(model))
        .filter(|a| !a.is_empty() && a != "—")
        .collect();
    agents.sort();
    agents.dedup();
    Ok(LogPage {
        items,
        total,
        kept,
        agents,
    })
}

pub fn remove(ids: &[i64]) -> Result<usize, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    let conn = connect()?;
    let places = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let refs: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    conn.execute(
        &format!("DELETE FROM gateway_requests WHERE id IN ({places})"),
        refs.as_slice(),
    )
    .map_err(crate::sessions::err)
}

/// Empties the log, or only what a filter matches.
pub fn clear(query: &LogQuery) -> Result<usize, String> {
    let conn = connect()?;
    let (where_sql, args) = filter(query);
    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
    conn.execute(
        &format!("DELETE FROM gateway_requests{where_sql}"),
        refs.as_slice(),
    )
    .map_err(crate::sessions::err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_internal_call_is_logged_with_the_status_it_ended_in() {
        assert_eq!(internal_status(&Ok("OK".into())), 200);
        assert_eq!(internal_status(&Err("HTTP 429: slow down".into())), 429);
        assert_eq!(internal_status(&Err("HTTP 400（提示）: bad".into())), 400);
        assert_eq!(internal_status(&Err("E_CANCELLED".into())), 499);
        assert_eq!(internal_status(&Err("E_AI_REPLY".into())), 500);
    }

    fn seeded() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure(&conn).unwrap();
        let rows = [
            (
                1_000_i64,
                "/v1/chat/completions",
                "codex/gpt-5",
                200_i64,
                4200_i64,
            ),
            (2_000, "/v1/chat/completions", "claude/haiku", 200, 10_200),
            (
                3_000,
                "/v1/chat/completions",
                "agy/gemini-3.1-pro-low",
                502,
                7_500,
            ),
            (4_000, "/v1/models", "", 200, 700),
        ];
        for (at, endpoint, model, status, elapsed) in rows {
            conn.execute(
                "INSERT INTO gateway_requests (at, endpoint, model, status, elapsed_ms) VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![at, endpoint, model, status, elapsed],
            )
            .unwrap();
        }
        conn
    }

    fn count(conn: &Connection, query: &LogQuery) -> i64 {
        let (where_sql, args) = filter(query);
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        conn.query_row(
            &format!("SELECT COUNT(*) FROM gateway_requests{where_sql}"),
            refs.as_slice(),
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_filter_narrows_by_text_outcome_and_agent() {
        let conn = seeded();
        assert_eq!(count(&conn, &LogQuery::default()), 4);
        assert_eq!(
            count(
                &conn,
                &LogQuery {
                    outcome: "error".into(),
                    ..Default::default()
                }
            ),
            1
        );
        assert_eq!(
            count(
                &conn,
                &LogQuery {
                    outcome: "ok".into(),
                    ..Default::default()
                }
            ),
            3
        );
        assert_eq!(
            count(
                &conn,
                &LogQuery {
                    agent: "CODEX".into(),
                    ..Default::default()
                }
            ),
            1,
            "the agent filter ignores case and matches the part before the slash"
        );
        assert_eq!(
            count(
                &conn,
                &LogQuery {
                    search: "MODELS".into(),
                    ..Default::default()
                }
            ),
            1
        );
        assert_eq!(
            count(
                &conn,
                &LogQuery {
                    since: 2_500,
                    ..Default::default()
                }
            ),
            2
        );
    }

    #[test]
    fn retention_forgets_what_is_older_than_the_days_kept() {
        let conn = Connection::open_in_memory().unwrap();
        ensure(&conn).unwrap();
        let old = (now() - 10 * 86_400) as i64;
        let fresh = (now() - 3_600) as i64;
        for at in [old, fresh] {
            conn.execute(
                "INSERT INTO gateway_requests (at, endpoint, model, status, elapsed_ms) VALUES (?1,'/v1/models','',200,1)",
                [at],
            )
            .unwrap();
        }
        prune(&conn, 0);
        assert_eq!(
            count(&conn, &LogQuery::default()),
            2,
            "0 days keeps everything"
        );
        prune(&conn, 7);
        assert_eq!(count(&conn, &LogQuery::default()), 1);
    }

    #[test]
    fn an_agent_is_the_name_before_the_slash() {
        assert_eq!(agent_of("codex/gpt-5"), "codex");
        assert_eq!(agent_of("agy"), "agy");
        assert_eq!(agent_of(""), "");
    }
}
