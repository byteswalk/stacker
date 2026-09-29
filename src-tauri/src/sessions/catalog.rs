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
    // A missing store means the agent is not installed, which is not worth a warning.
    if let Ok(found) = super::codebuddy_catalog::load(Path::new(&roots.codebuddy)) {
        sessions.extend(found);
    }
    if let Ok(found) = super::mimo_catalog::load(Path::new(&roots.mimo)) {
        sessions.extend(found);
    }
    if let Ok(found) = super::workbuddy_catalog::load(Path::new(&roots.workbuddy), Agent::WorkBuddy)
    {
        sessions.extend(found);
    }
    if let Ok(found) =
        super::workbuddy_catalog::load(Path::new(&roots.workbuddy_ai), Agent::WorkBuddyAi)
    {
        sessions.extend(found);
    }
    if let Ok(found) = super::kimi_catalog::load(Path::new(&roots.kimi)) {
        sessions.extend(found);
    }
    for (root, agent) in [
        (&roots.qoder, Agent::Qoder),
        (&roots.qoder_cn, Agent::QoderCn),
    ] {
        if let Ok(found) = super::claude_catalog::load_as(Path::new(root), agent, None) {
            sessions.extend(found);
        }
    }
    // The desktop apps keep their own stores, which their CLIs never touch.
    if let Some(app_data) = dirs::data_dir() {
        for (folder, agent) in [
            ("Qoder", Agent::Qoder),
            ("QoderCN", Agent::QoderCn),
        ] {
            if let Ok(found) =
                super::desktop_catalog::qoder_app(&app_data.join(folder), agent)
            {
                sessions.extend(found);
            }
        }
    }
    if let Ok(found) =
        super::desktop_catalog::antigravity(Path::new(&roots.antigravity), Agent::Antigravity)
    {
        sessions.extend(found);
    }
    if let Ok(found) = super::desktop_catalog::trae_cli(Path::new(&roots.trae), Agent::Trae) {
        sessions.extend(found);
    }
    fold_imports(
        &mut sessions,
        &super::mimo_catalog::imports(Path::new(&roots.mimo)),
    );
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

/// An agent that imports another's history holds the same conversation twice. The one the
/// user actually had is the original, so the import is folded into it — counted once, shown
/// once — and only an import whose original is gone stays on its own.
pub fn fold_imports(sessions: &mut Vec<Session>, imports: &[super::mimo_catalog::Import]) {
    if imports.is_empty() {
        return;
    }
    let by_path: std::collections::HashMap<String, usize> = sessions
        .iter()
        .enumerate()
        .map(|(index, session)| (session.path.to_lowercase(), index))
        .collect();
    let mut folded: Vec<usize> = Vec::new();
    let mut holds: Vec<(usize, Agent)> = Vec::new();
    let mut came_from: Vec<(usize, Agent)> = Vec::new();
    for (index, session) in sessions.iter().enumerate() {
        if session.agent != Agent::MiMo {
            continue;
        }
        let Some(import) = imports.iter().find(|i| i.session_id == session.native_id) else {
            continue;
        };
        match by_path.get(&import.source_path.to_lowercase()) {
            Some(&original) if original != index => {
                folded.push(index);
                holds.push((original, session.agent));
            }
            // The conversation it was imported from is no longer on disk, so this copy is
            // all that is left of it: keep it, and say where it came from.
            _ => came_from.push((index, import.agent)),
        }
    }
    for (index, agent) in holds {
        if !sessions[index].imported_by.contains(&agent) {
            sessions[index].imported_by.push(agent);
        }
    }
    for (index, agent) in came_from {
        sessions[index].imported_from = Some(agent);
    }
    folded.sort_unstable();
    for index in folded.into_iter().rev() {
        sessions.remove(index);
    }
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
            "discarded" => s.status == SessionStatus::Discarded,
            // A conversation the user deleted in the agent's own app is only shown when
            // asked for: on disk it is leftovers, not a conversation they still keep.
            _ => s.status != SessionStatus::Discarded,
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
    if q.sort == "bytes" {
        out.sort_by_key(|s| std::cmp::Reverse(s.bytes));
    } else {
        out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    }
    out
}

pub fn page(
    filtered: Vec<Session>,
    offset: usize,
    warnings: Vec<String>,
    agents: Vec<AgentCount>,
) -> SessionPage {
    SessionPage {
        total: filtered.len(),
        ids: filtered.iter().map(|s| s.id.clone()).collect(),
        total_bytes: filtered.iter().map(|s| s.bytes).sum(),
        items: filtered.into_iter().skip(offset).take(PAGE_SIZE).collect(),
        warnings,
        agents,
    }
}

/// What every agent holds under the current filters, the agent filter itself aside: the
/// picker has to show the other agents' counts to be worth opening.
pub fn agent_counts(sessions: &[Session], q: &SessionQuery) -> Vec<AgentCount> {
    let without_agent = SessionQuery {
        agent: String::new(),
        offset: 0,
        ..q.clone()
    };
    let mut rows: Vec<AgentCount> = Vec::new();
    for session in filter(sessions, &without_agent) {
        match rows.iter_mut().find(|row| row.agent == session.agent) {
            Some(row) => {
                row.sessions += 1;
                row.bytes += session.bytes;
            }
            None => rows.push(AgentCount {
                agent: session.agent,
                sessions: 1,
                bytes: session.bytes,
            }),
        }
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.sessions));
    rows
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
                discarded: 0,
                bytes: 0,
                updated_at: 0,
            });
        if !row.agents.contains(&s.agent) {
            row.agents.push(s.agent);
        }
        if s.status == SessionStatus::Discarded {
            row.discarded += 1;
        } else {
            row.sessions += 1;
        }
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
            summary_by: String::new(),
            summary_at: 0,
            copies: Vec::new(),
            imported_from: None,
            imported_by: Vec::new(),
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

    #[test]
    fn an_imported_conversation_is_folded_into_the_one_it_came_from() {
        use super::super::mimo_catalog::Import;
        let mut original = session(
            "claude:a",
            Agent::Claude,
            ClientTag::Terminal,
            SessionStatus::Active,
            "p1",
            10,
        );
        original.path = r"C:\Users\me\.claude\projects\p1\a.jsonl".into();
        let mut copy = session(
            "mimo:x",
            Agent::MiMo,
            ClientTag::Terminal,
            SessionStatus::Active,
            "p1",
            11,
        );
        copy.native_id = "x".into();
        let mut orphan = session(
            "mimo:y",
            Agent::MiMo,
            ClientTag::Terminal,
            SessionStatus::Active,
            "p1",
            12,
        );
        orphan.native_id = "y".into();
        let mut sessions = vec![original.clone(), copy, orphan];

        let imports = vec![
            Import {
                session_id: "x".into(),
                agent: Agent::Claude,
                // The same file, spelled the way MiMo recorded it.
                source_path: r"C:\USERS\me\.claude\projects\p1\A.JSONL".into(),
            },
            Import {
                session_id: "y".into(),
                agent: Agent::Claude,
                source_path: r"C:\Users\me\.claude\projects\p1\gone.jsonl".into(),
            },
        ];
        fold_imports(&mut sessions, &imports);

        // The conversation the user actually had is listed once, and says who else holds it.
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "claude:a");
        assert_eq!(sessions[0].imported_by, vec![Agent::MiMo]);
        assert_eq!(sessions[0].imported_from, None);
        // The import whose original is gone stays, and says where it came from.
        assert_eq!(sessions[1].id, "mimo:y");
        assert_eq!(sessions[1].imported_from, Some(Agent::Claude));
        assert!(sessions[1].imported_by.is_empty());
    }

    #[test]
    fn a_conversation_discarded_in_its_app_is_out_of_the_way_until_asked_for() {
        let mut sessions = sample();
        let mut gone = session(
            "workbuddy:z",
            Agent::WorkBuddy,
            ClientTag::Desktop,
            SessionStatus::Discarded,
            "p1",
            99,
        );
        gone.title = "deleted in the app".into();
        sessions.push(gone);
        let listed = filter(&sessions, &SessionQuery::default());
        assert!(!listed.iter().any(|s| s.id == "workbuddy:z"));
        let asked = filter(
            &sessions,
            &SessionQuery {
                status: "discarded".into(),
                ..Default::default()
            },
        );
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].id, "workbuddy:z");
    }

    #[test]
    fn a_project_counts_what_the_list_will_show_apart_from_what_it_will_not() {
        let mut live = session(
            "workbuddy:a",
            Agent::WorkBuddy,
            ClientTag::Desktop,
            SessionStatus::Active,
            "p1",
            10,
        );
        live.project.key = "p1".into();
        let mut gone = session(
            "workbuddy:b",
            Agent::WorkBuddy,
            ClientTag::Desktop,
            SessionStatus::Discarded,
            "p2",
            11,
        );
        gone.project.key = "p2".into();
        let rows = projects(&[live, gone]);
        let p1 = rows.iter().find(|r| r.project.key == "p1").unwrap();
        let p2 = rows.iter().find(|r| r.project.key == "p2").unwrap();
        assert_eq!((p1.sessions, p1.discarded), (1, 0));
        // Everything in p2 was deleted inside the app, so the sessions tab has nothing to
        // show for it and the filter must not offer it.
        assert_eq!((p2.sessions, p2.discarded), (0, 1));
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
    fn sorts_by_size_when_asked() {
        let mut list = sample();
        list[3].bytes = 99;
        let ids: Vec<_> = filter(
            &list,
            &SessionQuery {
                sort: "bytes".into(),
                ..Default::default()
            },
        )
        .into_iter()
        .map(|s| s.id)
        .collect();
        assert_eq!(ids[0], "d");
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

    /// Live, read-only: `cargo test --lib live_status_filter -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_status_filter() {
        let roots = super::super::roots::resolve(&Roots::default());
        let catalog = load(&roots);
        let q = SessionQuery {
            agent: "claude".into(),
            status: "active".into(),
            sort: "bytes".into(),
            ..Default::default()
        };
        for s in filter(&catalog.sessions, &q).iter().take(6) {
            println!("{:?} {:?} {} {}", s.status, s.client, s.title, s.bytes);
        }
        let q: SessionQuery = serde_json::from_str(r#"{"agent":"claude","project":"","status":"active","client":"","search":"","fullText":false,"includeAutomation":false,"favoritesOnly":false,"updatedAfter":0,"sort":"bytes","offset":0}"#).unwrap();
        println!("deserialized status={:?} sort={:?}", q.status, q.sort);
        let all = filter(
            &catalog.sessions,
            &SessionQuery {
                agent: "claude".into(),
                ..Default::default()
            },
        );
        let active = filter(
            &catalog.sessions,
            &SessionQuery {
                agent: "claude".into(),
                status: "active".into(),
                ..Default::default()
            },
        );
        println!("claude all={} active={}", all.len(), active.len());
        let mut seen = std::collections::HashMap::new();
        for s in &catalog.sessions {
            *seen.entry(s.id.clone()).or_insert(0) += 1;
        }
        for (id, n) in seen.iter().filter(|(_, n)| **n > 1) {
            println!("DUP {id} x{n}");
            for s in catalog.sessions.iter().filter(|s| &s.id == id) {
                println!("   {:?} {}", s.status, s.path);
            }
        }
    }

    #[test]
    fn pages_keep_every_matching_id() {
        let many: Vec<_> = (0..45)
            .map(|i| tests_support::session(&i.to_string()))
            .collect();
        let page = page(many, 40, vec![], vec![]);
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
