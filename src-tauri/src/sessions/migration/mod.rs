//! Moving an agent's data folder to another drive behind a directory junction.
pub mod flow;
pub mod fsops;

use crate::runner::CancelFlag;
use crate::sessions::model::Agent;
use flow::{Record, Step};
use fsops::Volume;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

fn key(agent: Agent) -> String {
    format!("migration:{}", agent.as_str())
}

pub fn default_source(agent: Agent) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    match agent {
        Agent::Codex => home.join(".codex"),
        Agent::Claude => home.join(".claude"),
        Agent::CodeBuddy => home.join(".codebuddy"),
        Agent::WorkBuddy => home.join(".workbuddy"),
        Agent::WorkBuddyAi => home.join(".workbuddy-ai"),
        Agent::MiMo => home.join(".local").join("share").join("mimocode"),
        Agent::Kimi => home.join(".kimi-code"),
    }
}

fn env_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "CODEX_HOME",
        Agent::Claude => "CLAUDE_CONFIG_DIR",
        // These read no environment variable for their data folder.
        Agent::CodeBuddy | Agent::WorkBuddy | Agent::WorkBuddyAi | Agent::MiMo | Agent::Kimi => "",
    }
}

fn env_override(agent: Agent) -> Option<String> {
    crate::winenv::get_user_raw(env_name(agent))
        .or_else(|| std::env::var(env_name(agent)).ok())
        .filter(|v| !v.trim().is_empty())
}

fn load(agent: Agent) -> Option<Record> {
    let conn = crate::sessions::annotations::connect().ok()?;
    crate::sessions::annotations::setting(&conn, &key(agent))
        .and_then(|v| serde_json::from_str(&v).ok())
}

fn save(agent: Agent, record: Option<&Record>) -> Result<(), String> {
    let conn = crate::sessions::annotations::connect()?;
    let value = match record {
        Some(r) => serde_json::to_string(r).map_err(crate::sessions::err)?,
        None => String::new(),
    };
    crate::sessions::annotations::set_setting(&conn, &key(agent), &value)
}

/// Whether the agent's CLI or desktop app is running.
pub fn agent_running(agent: Agent, images: &[PathBuf]) -> bool {
    images.iter().any(|p| {
        let lower = p.to_string_lossy().to_lowercase();
        let name = lower.rsplit('\\').next().unwrap_or("").to_string();
        match agent {
            Agent::Codex => name.starts_with("codex") || lower.contains("\\openai.codex_"),
            Agent::CodeBuddy => name.starts_with("codebuddy") || name == "cbc.exe",
            Agent::WorkBuddy | Agent::WorkBuddyAi => name.starts_with("workbuddy"),
            Agent::MiMo => name.starts_with("mimo"),
            Agent::Kimi => name.starts_with("kimi"),
            Agent::Claude => {
                name == "claude.exe"
                    || lower.contains("\\windowsapps\\claude_")
                    || lower.contains("\\anthropicclaude\\")
            }
        }
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationStatus {
    pub agent: Agent,
    pub source: String,
    /// Where the data really is (the junction target when migrated).
    pub actual: String,
    /// normal | migrated | incomplete | external_link | env | missing
    pub kind: String,
    pub step: Option<Step>,
    pub target: String,
    pub backup: String,
    pub backup_exists: bool,
    pub suggested_target: String,
    pub drives: Vec<Volume>,
}

pub fn status(agent: Agent) -> LocationStatus {
    let source = default_source(agent);
    let record = load(agent);
    let drives = fsops::other_drives();
    let folder = match agent {
        Agent::Codex => "codex",
        Agent::Claude => "claude",
        Agent::CodeBuddy => "codebuddy",
        Agent::WorkBuddy => "workbuddy",
        Agent::WorkBuddyAi => "workbuddy-ai",
        Agent::MiMo => "mimo",
        Agent::Kimi => "kimi",
    };
    let suggested = drives
        .first()
        .map(|d| Path::new(&d.root).join("AgentData").join(folder))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_link = fsops::is_link(&source);
    let actual = if is_link {
        fsops::link_target(&source).unwrap_or_else(|| source.clone())
    } else {
        source.clone()
    };
    let kind = if record.as_ref().is_some_and(|r| r.incomplete()) {
        "incomplete"
    } else if env_override(agent).is_some() {
        "env"
    } else if is_link && record.is_some() {
        "migrated"
    } else if is_link {
        "external_link"
    } else if source.is_dir() {
        "normal"
    } else {
        "missing"
    };
    LocationStatus {
        agent,
        source: source.to_string_lossy().into_owned(),
        actual: actual.to_string_lossy().into_owned(),
        kind: kind.into(),
        step: record.as_ref().map(|r| r.step),
        target: record
            .as_ref()
            .map(|r| r.target.to_string_lossy().into_owned())
            .unwrap_or_default(),
        backup: record
            .as_ref()
            .map(|r| r.backup.to_string_lossy().into_owned())
            .unwrap_or_default(),
        backup_exists: record.as_ref().is_some_and(|r| r.backup.exists()),
        suggested_target: suggested,
        drives,
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub problems: Vec<String>,
    pub bytes: u64,
    pub files: u64,
    pub free: u64,
    pub target: String,
}

fn inside(a: &Path, b: &Path) -> bool {
    let a = a.to_string_lossy().to_lowercase();
    let b = b.to_string_lossy().to_lowercase();
    let b = b.trim_end_matches('\\');
    a == b || a.starts_with(&format!("{b}\\"))
}

pub fn check_paths(
    source: &Path,
    target: &Path,
    running: bool,
    env: bool,
) -> (CheckResult, Option<fsops::TreeStats>) {
    let mut result = CheckResult {
        target: target.to_string_lossy().into_owned(),
        ..Default::default()
    };
    if running {
        result.problems.push("E_APP_RUNNING".into());
    }
    if env || !source.is_dir() || fsops::is_link(source) {
        result.problems.push("E_NOT_MOVABLE".into());
        return (result, None);
    }
    let stats = match fsops::stats(source) {
        Ok(s) => Some(s),
        Err(code) => {
            result.problems.push(code);
            None
        }
    };
    if let Some(s) = &stats {
        result.bytes = s.bytes;
        result.files = s.files;
    }
    if !target.is_absolute() {
        result.problems.push("E_TARGET".into());
        return (result, stats);
    }
    let empty = !target.exists() || std::fs::read_dir(target).is_ok_and(|mut d| d.next().is_none());
    if !empty || inside(target, source) || inside(source, target) {
        result.problems.push("E_TARGET".into());
    }
    match fsops::volume_of(target) {
        Some(v) if v.fixed && v.file_system.eq_ignore_ascii_case("NTFS") => {
            result.free = v.free;
            if (v.free as f64) < result.bytes as f64 * 1.1 {
                result.problems.push("E_SPACE".into());
            }
        }
        _ => result.problems.push("E_TARGET_FS".into()),
    }
    (result, stats)
}

pub fn check(agent: Agent, target: &Path) -> CheckResult {
    let running = agent_running(
        agent,
        &crate::sessions::footprint::processes::running_images(),
    );
    check_paths(
        &default_source(agent),
        target,
        running,
        env_override(agent).is_some(),
    )
    .0
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationJob {
    pub agent: Option<Agent>,
    /// migrate | back
    pub action: String,
    /// running | completed | failed
    pub state: String,
    pub copied: u64,
    pub total: u64,
    pub error: String,
}

struct Live {
    job: MigrationJob,
    copied: Arc<AtomicU64>,
    cancel: CancelFlag,
}

static JOB: Mutex<Option<Live>> = Mutex::new(None);

pub fn job() -> Option<MigrationJob> {
    let guard = JOB.lock().ok()?;
    let live = guard.as_ref()?;
    let mut job = live.job.clone();
    job.copied = live.copied.load(Ordering::Relaxed);
    Some(job)
}

pub fn cancel() {
    if let Ok(guard) = JOB.lock() {
        if let Some(live) = guard.as_ref() {
            live.cancel.cancel();
        }
    }
}

fn finish(result: Result<(), String>) {
    if let Ok(mut guard) = JOB.lock() {
        if let Some(live) = guard.as_mut() {
            match result {
                Ok(()) => live.job.state = "completed".into(),
                Err(code) => {
                    live.job.state = "failed".into();
                    live.job.error = code;
                }
            }
        }
    }
    crate::sessions::catalog::invalidate();
    crate::sessions::footprint::ledger::invalidate();
}

fn begin(agent: Agent, action: &str, total: u64) -> Result<(Arc<AtomicU64>, CancelFlag), String> {
    let mut guard = JOB.lock().map_err(crate::sessions::err)?;
    if guard.as_ref().is_some_and(|l| l.job.state == "running") {
        return Err("E_BUSY".into());
    }
    let copied = Arc::new(AtomicU64::new(0));
    let cancel = CancelFlag::default();
    *guard = Some(Live {
        job: MigrationJob {
            agent: Some(agent),
            action: action.into(),
            state: "running".into(),
            total,
            ..Default::default()
        },
        copied: copied.clone(),
        cancel: cancel.clone(),
    });
    Ok((copied, cancel))
}

pub fn start(agent: Agent, target: PathBuf) -> Result<MigrationJob, String> {
    if load(agent).is_some() {
        return Err("E_BUSY".into());
    }
    let running = agent_running(
        agent,
        &crate::sessions::footprint::processes::running_images(),
    );
    let source = default_source(agent);
    let (result, stats) = check_paths(&source, &target, running, env_override(agent).is_some());
    if let Some(problem) = result.problems.first() {
        return Err(problem.clone());
    }
    let stats = stats.ok_or("E_ACCESS")?;
    let (copied, cancel) = begin(agent, "migrate", stats.bytes)?;
    std::thread::spawn(move || {
        let mut record = Record::new(&source, &target, crate::sessions::now());
        let mut persist = |r: Option<&Record>| save(agent, r);
        finish(flow::migrate(
            &mut record,
            stats,
            &copied,
            &cancel,
            &mut persist,
        ));
    });
    job().ok_or_else(|| "E_BUSY".into())
}

pub fn delete_backup(agent: Agent) -> Result<(), String> {
    let mut record = load(agent).ok_or("E_REQUEST")?;
    let mut persist = |r: Option<&Record>| save(agent, r);
    let result = flow::delete_backup(&mut record, &mut persist);
    crate::sessions::footprint::ledger::invalidate();
    result
}

/// Undoes an incomplete migration, or moves migrated data back.
pub fn move_back(agent: Agent) -> Result<MigrationJob, String> {
    let record = load(agent).ok_or("E_REQUEST")?;
    if agent_running(
        agent,
        &crate::sessions::footprint::processes::running_images(),
    ) {
        return Err("E_APP_RUNNING".into());
    }
    let total = if record.step == Step::Cleaned {
        record.bytes
    } else {
        0
    };
    let (copied, cancel) = begin(agent, "back", total)?;
    std::thread::spawn(move || {
        let mut persist = |r: Option<&Record>| save(agent, r);
        let result = if record.incomplete() {
            flow::undo(&record, &mut persist)
        } else {
            flow::move_back(&record, &copied, &cancel, &mut persist)
        };
        finish(result);
    });
    job().ok_or_else(|| "E_BUSY".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live, read-only: `cargo test --lib live_migration_check -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_migration_check() {
        for agent in [Agent::Codex, Agent::Claude] {
            let s = status(agent);
            let started = std::time::Instant::now();
            let c = check(agent, Path::new(&s.suggested_target));
            println!(
                "{} kind={} source={} suggested={} -> problems={:?} bytes={} files={} free={} ({:?})",
                agent.as_str(), s.kind, s.source, s.suggested_target, c.problems, c.bytes, c.files, c.free, started.elapsed()
            );
        }
    }

    #[test]
    fn running_agents_are_detected_by_image() {
        let img = |s: &str| vec![PathBuf::from(s)];
        assert!(agent_running(Agent::Codex, &img(r"C:\x\bin\codex.exe")));
        assert!(agent_running(
            Agent::Claude,
            &img(r"C:\Program Files\WindowsApps\Claude_1.0_x64__abc\app\Claude.exe")
        ));
        assert!(agent_running(
            Agent::Claude,
            &img(r"C:\Users\u\AppData\Roaming\Claude\claude-code\2.1\claude.exe")
        ));
        assert!(!agent_running(
            Agent::Codex,
            &img(r"C:\Windows\explorer.exe")
        ));
    }

    #[test]
    fn checks_catch_bad_targets() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join(".codex");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("a"), b"12").unwrap();
        let ok = check_paths(&src, &dir.path().join("D").join("codex"), false, false).0;
        assert!(ok.problems.is_empty(), "{:?}", ok.problems);
        assert_eq!(ok.bytes, 2);

        let nested = check_paths(&src, &src.join("x"), false, false).0;
        assert!(nested.problems.contains(&"E_TARGET".to_string()));
        let busy = check_paths(&src, &dir.path().join("t"), true, false).0;
        assert!(busy.problems.contains(&"E_APP_RUNNING".to_string()));
        let env = check_paths(&src, &dir.path().join("t"), false, true).0;
        assert!(env.problems.contains(&"E_NOT_MOVABLE".to_string()));
        let relative = check_paths(&src, Path::new("rel"), false, false).0;
        assert!(relative.problems.contains(&"E_TARGET".to_string()));
    }
}
