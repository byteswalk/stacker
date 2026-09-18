use super::model::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Catalog {
    pub sessions: Vec<Session>,
    pub warnings: Vec<String>,
}

struct Cached {
    at: Instant,
    roots: Roots,
    sessions: Vec<Session>,
    warnings: Vec<String>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
const CACHE_TTL: Duration = Duration::from_secs(10);
pub const PAGE_SIZE: usize = 40;

pub fn invalidate() {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}

pub fn load(roots: &Roots) -> Catalog {
    if let Ok(cache) = CACHE.lock() {
        if let Some(cached) = cache.as_ref() {
            if cached.at.elapsed() < CACHE_TTL && &cached.roots == roots {
                return Catalog {
                    sessions: cached.sessions.clone(),
                    warnings: cached.warnings.clone(),
                };
            }
        }
    }
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();
    match super::codex_catalog::load(Path::new(&roots.codex)) {
        Ok(found) => sessions.extend(found),
        Err(code) => warnings.push(format!("codex:{code}")),
    }
    match super::claude_catalog::load(
        Path::new(&roots.claude),
        Path::new(&roots.claude_desktop_index),
    ) {
        Ok(found) => sessions.extend(found),
        Err(code) => warnings.push(format!("claude:{code}")),
    }
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(Cached {
            at: Instant::now(),
            roots: roots.clone(),
            sessions: sessions.clone(),
            warnings: warnings.clone(),
        });
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

pub fn is_automation(s: &Session) -> bool {
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
                || s.summary
                    .as_deref()
                    .is_some_and(|t| t.to_lowercase().contains(&search))
        })
        .cloned()
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
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
        let row = rows
            .entry(s.project.key.clone())
            .or_insert_with(|| ProjectRow {
                project: s.project.clone(),
                agents: vec![],
                sessions: 0,
                orphans: 0,
                bytes: 0,
                updated_at: 0,
            });
        if !row.agents.contains(&s.agent) {
            row.agents.push(s.agent);
        }
        row.sessions += 1;
        row.orphans += usize::from(s.status == SessionStatus::Orphaned);
        row.bytes += s.bytes;
        row.updated_at = row.updated_at.max(s.updated_at);
    }
    let mut out: Vec<_> = rows.into_values().collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    out
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::super::model::*;

    pub fn session(id: &str) -> Session {
        Session {
            id: id.into(),
            agent: Agent::Codex,
            native_id: id.into(),
            title: id.into(),
            title_source: TitleSource::Client,
            project: ProjectRef::default(),
            client: ClientTag::Desktop,
            created_at: 0,
            updated_at: 0,
            archived: false,
            pinned: false,
            status: SessionStatus::Active,
            children: vec![],
            bytes: 0,
            path: String::new(),
            in_desktop_index: false,
            parent_missing: false,
            favorite: false,
            summary: None,
            summary_stale: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(
        id: &str,
        agent: Agent,
        client: ClientTag,
        status: SessionStatus,
        project: &str,
        updated: u64,
    ) -> Session {
        let mut s = tests_support::session(id);
        s.agent = agent;
        s.client = client;
        s.status = status;
        s.title = format!("title {id}");
        s.project = ProjectRef {
            key: project.into(),
            name: project.into(),
            path: project.into(),
            exists: status != SessionStatus::Orphaned,
        };
        s.updated_at = updated;
        s.bytes = 10;
        s
    }

    fn sample() -> Vec<Session> {
        vec![
            session(
                "a",
                Agent::Codex,
                ClientTag::Desktop,
                SessionStatus::Active,
                "p1",
                3,
            ),
            session(
                "b",
                Agent::Codex,
                ClientTag::Automation,
                SessionStatus::Active,
                "p1",
                5,
            ),
            session(
                "c",
                Agent::Claude,
                ClientTag::Desktop,
                SessionStatus::Orphaned,
                "p2",
                4,
            ),
            session(
                "d",
                Agent::Claude,
                ClientTag::Terminal,
                SessionStatus::Archived,
                "p2",
                1,
            ),
        ]
    }

    #[test]
    fn automation_is_hidden_unless_requested() {
        let ids = |q: &SessionQuery| {
            filter(&sample(), q)
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&SessionQuery::default()), vec!["c", "a", "d"]);
        assert_eq!(
            ids(&SessionQuery {
                include_automation: true,
                ..Default::default()
            }),
            vec!["b", "c", "a", "d"]
        );
        assert_eq!(
            ids(&SessionQuery {
                status: "orphaned".into(),
                ..Default::default()
            }),
            vec!["c"]
        );
        assert_eq!(
            ids(&SessionQuery {
                agent: "claude".into(),
                project: "p2".into(),
                ..Default::default()
            }),
            vec!["c", "d"]
        );
        assert_eq!(
            ids(&SessionQuery {
                search: "TITLE A".into(),
                ..Default::default()
            }),
            vec!["a"]
        );
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

    #[test]
    fn pages_keep_every_matching_id() {
        let many: Vec<_> = (0..45)
            .map(|i| tests_support::session(&i.to_string()))
            .collect();
        let page = page(many, 40, vec![]);
        assert_eq!(page.items.len(), 5);
        assert_eq!(page.ids.len(), 45);
    }

    /// Live check against this machine: `cargo test --lib live_catalog -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_catalog() {
        let roots = super::super::roots::resolve(&Roots::default());
        let catalog = load(&roots);
        let visible = filter(&catalog.sessions, &SessionQuery::default());
        for agent in [Agent::Codex, Agent::Claude] {
            let list: Vec<_> = visible.iter().filter(|s| s.agent == agent).collect();
            println!("{} visible: {}", agent.as_str(), list.len());
            for s in list.iter().take(8) {
                println!(
                    "  {:?} {:?} {} | {}",
                    s.client, s.status, s.title, s.project.name
                );
            }
        }
        let orphans = visible
            .iter()
            .filter(|s| s.status == SessionStatus::Orphaned)
            .count();
        println!(
            "orphans: {orphans}, total {}, warnings {:?}",
            catalog.sessions.len(),
            catalog.warnings
        );
    }
}
