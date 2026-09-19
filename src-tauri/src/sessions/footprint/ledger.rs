//! Builds the footprint report: resolve roots, apply rules, measure with dedup.
use super::measure::Meter;
use super::model::*;
use super::rules::{classify_root, Context, Draft, RootKind};
use crate::sessions::model::{Agent, Roots};
use crate::space_analysis::windows_fs::display_path;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Root {
    pub agent: Agent,
    pub kind: RootKind,
    pub path: PathBuf,
    /// Shown after labels when one agent has several app roots.
    pub suffix: Option<String>,
}

/// Where an item's paths live, kept for cleanup checks.
#[derive(Clone)]
pub struct ItemPaths {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
}

pub struct Scan {
    pub report: FootprintReport,
    pub paths: HashMap<String, ItemPaths>,
}

static CACHE: Mutex<Option<std::sync::Arc<Scan>>> = Mutex::new(None);

pub fn cached() -> Option<std::sync::Arc<Scan>> {
    CACHE.lock().ok().and_then(|c| c.clone())
}

pub fn invalidate() {
    if let Ok(mut c) = CACHE.lock() {
        *c = None;
    }
}

pub fn store(scan: Scan) -> std::sync::Arc<Scan> {
    let scan = std::sync::Arc::new(scan);
    if let Ok(mut c) = CACHE.lock() {
        *c = Some(scan.clone());
    }
    scan
}

fn packages(prefix: &str) -> Vec<PathBuf> {
    let Some(dir) = dirs::data_local_dir().map(|d| d.join("Packages")) else {
        return Vec::new();
    };
    std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with(prefix))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Real roots first, MSIX package folders last so redirected duplicates count once.
pub fn default_roots(roots: &Roots) -> Vec<Root> {
    let local = dirs::data_local_dir().unwrap_or_default();
    let roaming = dirs::data_dir().unwrap_or_default();
    let root = |agent, kind, path: PathBuf, suffix: Option<&str>| Root {
        agent,
        kind,
        path,
        suffix: suffix.map(str::to_string),
    };
    let mut list = vec![
        root(
            Agent::Codex,
            RootKind::CodexHome,
            PathBuf::from(&roots.codex),
            None,
        ),
        root(
            Agent::Codex,
            RootKind::CodexApp,
            local.join("OpenAI").join("Codex"),
            None,
        ),
        root(
            Agent::Claude,
            RootKind::ClaudeHome,
            PathBuf::from(&roots.claude),
            None,
        ),
        root(
            Agent::Claude,
            RootKind::ClaudeApp,
            roaming.join("Claude"),
            None,
        ),
        root(
            Agent::Claude,
            RootKind::ClaudeApp,
            local.join("Claude"),
            Some("本地"),
        ),
        root(
            Agent::Claude,
            RootKind::ClaudeApp,
            local.join("Claude-3p"),
            Some("Claude-3p"),
        ),
    ];
    list.extend(
        packages("openai.codex_")
            .into_iter()
            .map(|p| root(Agent::Codex, RootKind::CodexApp, p, Some("MSIX"))),
    );
    list.extend(
        packages("claude_")
            .into_iter()
            .map(|p| root(Agent::Claude, RootKind::ClaudeApp, p, Some("MSIX"))),
    );
    list.into_iter().filter(|r| r.path.is_dir()).collect()
}

fn item_id(rule: &str, paths: &[PathBuf]) -> String {
    let mut sorted: Vec<String> = paths.iter().map(|p| display_path(p)).collect();
    sorted.sort();
    let digest = Sha256::digest(sorted.join("\n").as_bytes());
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("{rule}:{hex}")
}

pub fn scan(roots: &[Root], session_ids: &HashSet<String>, running: &[PathBuf], now: u64) -> Scan {
    let cx = Context {
        session_ids,
        running,
        now,
    };
    let mut meter = Meter::new();
    let mut agents: Vec<AgentFootprint> = Vec::new();
    let mut paths = HashMap::new();
    for root in roots {
        if !meter.claim(&root.path) {
            continue;
        }
        let drafts: Vec<Draft> = classify_root(root.kind, &root.path, &cx);
        let slot = match agents.iter().position(|a| a.agent == root.agent) {
            Some(i) => i,
            None => {
                agents.push(AgentFootprint {
                    agent: root.agent,
                    total: 0,
                    reclaimable: 0,
                    items: Vec::new(),
                });
                agents.len() - 1
            }
        };
        for d in drafts {
            let (mut bytes, mut files) = (0, 0);
            for p in &d.paths {
                let (b, f) = meter.measure(p);
                bytes += b;
                files += f;
            }
            if bytes == 0 {
                continue;
            }
            let agent = &mut agents[slot];
            // "other" merges across an agent's roots; everything else is per root.
            if d.rule == "other" {
                if let Some(existing) = agent.items.iter_mut().find(|i| i.id.starts_with("other:"))
                {
                    existing.bytes += bytes;
                    existing.files += files;
                    existing
                        .paths
                        .extend(d.paths.iter().map(|p| display_path(p)));
                    continue;
                }
            }
            let id = item_id(d.rule, &d.paths);
            let label = match (&root.suffix, d.rule) {
                (Some(s), rule) if rule != "other" => format!("{}（{s}）", d.label),
                _ => d.label.clone(),
            };
            paths.insert(
                id.clone(),
                ItemPaths {
                    root: root.path.clone(),
                    paths: d.paths.clone(),
                },
            );
            agent.items.push(FootprintItem {
                id,
                agent: root.agent,
                owner: d.owner,
                kind: d.kind,
                label,
                explain: d.explain,
                paths: d.paths.iter().map(|p| display_path(p)).collect(),
                bytes,
                files,
                blocked: d.blocked,
                note: None,
            });
        }
    }
    for agent in &mut agents {
        agent.items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
        agent.total = agent.items.iter().map(|i| i.bytes).sum();
        agent.reclaimable = agent
            .items
            .iter()
            .filter(|i| i.kind == FootprintKind::Reclaimable && i.blocked.is_none())
            .map(|i| i.bytes)
            .sum();
    }
    let report = FootprintReport {
        total: agents.iter().map(|a| a.total).sum(),
        reclaimable: agents.iter().map(|a| a.reclaimable).sum(),
        agents,
        scanned_at: now,
        warnings: meter.warnings,
    };
    Scan { report, paths }
}

/// Native ids of every session and sub-agent the catalog knows.
pub fn session_ids(sessions: &[crate::sessions::model::Session]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for s in sessions {
        ids.insert(s.native_id.clone());
        for c in &s.children {
            if let Some(id) = c.id.strip_prefix("codex:") {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn duplicate_roots_count_once_and_totals_add_up() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::write(home.join("sessions").join("a.jsonl"), vec![0u8; 500]).unwrap();
        fs::create_dir_all(home.join(".tmp")).unwrap();
        fs::write(home.join(".tmp").join("x"), vec![0u8; 200]).unwrap();
        fs::write(home.join("config.toml"), vec![0u8; 10]).unwrap();
        let roots = vec![
            Root {
                agent: Agent::Codex,
                kind: RootKind::CodexHome,
                path: home.clone(),
                suffix: None,
            },
            Root {
                agent: Agent::Codex,
                kind: RootKind::CodexHome,
                path: home.clone(),
                suffix: Some("MSIX".into()),
            },
        ];
        let scan = scan(&roots, &HashSet::new(), &[], 0);
        let codex = &scan.report.agents[0];
        assert_eq!(codex.total, 710);
        assert_eq!(codex.reclaimable, 200);
        assert_eq!(codex.items.len(), 3);
        let tmp = codex
            .items
            .iter()
            .find(|i| i.id.starts_with("codex-tmp:"))
            .unwrap();
        assert!(scan.paths.contains_key(&tmp.id));
    }

    /// Live, read-only: `cargo test --lib live_footprint -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_footprint() {
        let roots = crate::sessions::roots::resolve(&Roots::default());
        let catalog = crate::sessions::catalog::load(&roots);
        let ids = session_ids(&catalog.sessions);
        let running = super::super::processes::running_images();
        let started = std::time::Instant::now();
        let scan = scan(
            &default_roots(&roots),
            &ids,
            &running,
            crate::sessions::now(),
        );
        println!("scan took {:?}", started.elapsed());
        let gb = |b: u64| b as f64 / 1_073_741_824.0;
        for a in &scan.report.agents {
            println!(
                "{} total {:.2} GB reclaimable {:.2} GB",
                a.agent.as_str(),
                gb(a.total),
                gb(a.reclaimable)
            );
            for i in &a.items {
                println!(
                    "  {:?} {:>8.1} MB {} {:?}",
                    i.kind,
                    i.bytes as f64 / 1_048_576.0,
                    i.label,
                    i.blocked
                );
            }
        }
        println!("warnings: {}", scan.report.warnings.len());
    }
}
