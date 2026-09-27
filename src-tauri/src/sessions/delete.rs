//! Previewed, re-verified session deletion with slim export, direct or full backup modes.
use super::claude_catalog::is_link;
use super::model::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// A transcript written this recently is treated as in use.
const IN_USE_SECONDS: u64 = 120;
const PREVIEW_SECONDS: u64 = 600;
const MAX_SELECTION: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    SlimExport,
    Direct,
    FullBackup,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    pub id: String,
    pub title: String,
    pub reason: String,
}

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
    pub export_dir: String,
    pub created: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub id: String,
    /// running | completed | failed | cancelled
    pub state: String,
    pub done: usize,
    pub total: usize,
    pub items: Vec<JobItem>,
    pub export_dir: String,
    pub error: String,
}

struct Stored {
    preview: Preview,
    fingerprints: Vec<String>,
}

static PREVIEWS: Mutex<Option<HashMap<String, Stored>>> = Mutex::new(None);
static JOB: Mutex<Option<JobState>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn inside(root: &Path, path: &Path) -> bool {
    match (fs::canonicalize(root), fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => path.starts_with(root),
        _ => false,
    }
}

fn modified_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn codex_paths(session: &Session) -> Vec<PathBuf> {
    std::iter::once(session.path.as_str())
        .chain(session.children.iter().map(|c| c.path.as_str()))
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

/// Splits the selection into deletable sessions (with the files they own) and blocked ones.
pub fn plan(
    sessions: &[Session],
    ids: &[String],
    roots: &Roots,
    now: u64,
) -> (Vec<(Session, Vec<PathBuf>)>, Vec<Blocked>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    let codex_ready = if ids.iter().any(|id| id.starts_with("codex:")) {
        super::codex_rpc::require_closed().and_then(|_| super::codex_rpc::capabilities())
    } else {
        Ok(())
    };
    for id in ids {
        let Some(session) = sessions.iter().find(|s| &s.id == id) else {
            blocked.push(Blocked {
                id: id.clone(),
                title: id.clone(),
                reason: "E_NOT_FOUND".into(),
            });
            continue;
        };
        let block = |reason: &str| Blocked {
            id: session.id.clone(),
            title: session.title.clone(),
            reason: reason.into(),
        };
        match session.agent {
            Agent::Claude => {
                let root = Path::new(&roots.claude);
                let transcript = Path::new(&session.path);
                let transcripts: Vec<&Path> = std::iter::once(session.path.as_str())
                    .chain(session.copies.iter().map(String::as_str))
                    .map(Path::new)
                    .collect();
                let newest = transcripts
                    .iter()
                    .map(|p| modified_secs(p))
                    .max()
                    .unwrap_or(0);
                if session.in_desktop_index {
                    blocked.push(block("E_IN_DESKTOP"));
                } else if now.saturating_sub(newest) < IN_USE_SECONDS {
                    blocked.push(block("E_IN_USE"));
                } else {
                    let mut paths = Vec::new();
                    for t in &transcripts {
                        for p in super::claude_catalog::related_paths(root, t, &session.native_id) {
                            if !paths.contains(&p) {
                                paths.push(p);
                            }
                        }
                    }
                    if is_link(transcript) || paths.iter().any(|p| is_link(p) || !inside(root, p)) {
                        blocked.push(block("E_LINK"));
                    } else {
                        allowed.push((session.clone(), paths));
                    }
                }
            }
            Agent::Codex => match &codex_ready {
                Err(code) => blocked.push(block(code)),
                Ok(()) => allowed.push((session.clone(), codex_paths(session))),
            },
            // MiMo's turns live in a database shared by every session, so its own CLI
            // removes them; nothing here is a file Stacker may delete.
            Agent::MiMo => allowed.push((session.clone(), Vec::new())),
            // One transcript file, nothing beside it.
            Agent::CodeBuddy => {
                let transcript = PathBuf::from(&session.path);
                let root = PathBuf::from(&roots.codebuddy);
                if now.saturating_sub(modified_secs(&transcript)) < IN_USE_SECONDS {
                    blocked.push(block("E_IN_USE"));
                } else if is_link(&transcript) || !inside(&root, &transcript) {
                    blocked.push(block("E_LINK"));
                } else {
                    allowed.push((session.clone(), vec![transcript]));
                }
            }
        }
    }
    (allowed, blocked)
}

/// Removes a Claude session's files after checking every path again.
pub fn delete_claude(paths: &[PathBuf], roots: &Roots) -> Result<(), String> {
    delete_files(paths, Path::new(&roots.claude))
}

/// Removes a session's files after checking every path again: still inside the agent's own
/// folder, and not a link pointing somewhere else.
pub fn delete_files(paths: &[PathBuf], root: &Path) -> Result<(), String> {
    for path in paths.iter().filter(|p| p.exists()) {
        if is_link(path) || !inside(root, path) {
            return Err("E_LINK".into());
        }
    }
    for path in paths.iter().filter(|p| p.exists()) {
        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        result.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "E_ACCESS".to_string()
            } else {
                super::err(e)
            }
        })?;
    }
    Ok(())
}

fn fingerprints(allowed: &[(Session, Vec<PathBuf>)]) -> Vec<String> {
    allowed
        .iter()
        .map(|(s, _)| {
            format!(
                "{}={}",
                s.id,
                super::annotations::quick_fingerprint(Path::new(&s.path))
            )
        })
        .collect()
}

fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(nanos);
    format!("{:016x}{:08x}", h.finish(), std::process::id())
}

pub fn preview(ids: Vec<String>, mode: Mode) -> Result<Preview, String> {
    if ids.is_empty() || ids.len() > MAX_SELECTION {
        return Err("E_REQUEST".into());
    }
    let (sessions, _, roots) = super::commands::annotated_catalog()?;
    let now = super::now();
    let (allowed, blocked) = plan(&sessions, &ids, &roots, now);
    let preview = Preview {
        token: token(),
        mode,
        children: allowed.iter().map(|(s, _)| s.children.len()).sum(),
        files: allowed.iter().map(|(_, p)| p.len()).sum(),
        bytes: allowed.iter().map(|(s, _)| s.bytes).sum(),
        sessions: allowed.iter().map(|(s, _)| s.clone()).collect(),
        blocked,
        export_dir: super::commands::export_dir().to_string_lossy().into_owned(),
        created: now,
    };
    let mut store = PREVIEWS.lock().map_err(super::err)?;
    let map = store.get_or_insert_with(HashMap::new);
    map.retain(|_, s| now.saturating_sub(s.preview.created) <= PREVIEW_SECONDS);
    map.insert(
        preview.token.clone(),
        Stored {
            preview: preview.clone(),
            fingerprints: fingerprints(&allowed),
        },
    );
    Ok(preview)
}

fn publish(job: &JobState) {
    if let Ok(mut slot) = JOB.lock() {
        *slot = Some(job.clone());
    }
}

pub fn job() -> Option<JobState> {
    JOB.lock().ok().and_then(|j| j.clone())
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if is_link(from) {
        return Err("E_LINK".into());
    }
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|_| "E_STORAGE".to_string())?;
        for entry in fs::read_dir(from).map_err(super::err)?.flatten() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|_| "E_STORAGE".to_string())?;
        }
        fs::copy(from, to)
            .map(|_| ())
            .map_err(|_| "E_STORAGE".to_string())
    }
}

/// `mimo session delete <id>`: the only supported way to remove a row from its database.
fn delete_mimo(session_id: &str) -> Result<(), String> {
    let program = crate::agents::process::resolve_command(&["mimo.exe", "mimo.cmd", "mimo.bat"])
        .ok_or("E_RUNNER_MISSING")?;
    crate::agents::process::run_command_text(
        &program,
        &["session", "delete", session_id],
        "mimo session delete",
        std::time::Duration::from_secs(120),
    )
    .map(|_| ())
}

fn delete_codex(
    rpc: &mut Option<super::codex_rpc::Rpc>,
    root: &Path,
    session: &Session,
) -> Result<(), String> {
    if rpc.is_none() {
        *rpc = Some(super::codex_rpc::Rpc::start(root)?);
    }
    let client = rpc.as_mut().ok_or("E_RPC")?;
    // Children first, then the parent thread.
    for child in &session.children {
        let child_id = child.id.trim_start_matches("codex:");
        if Path::new(&child.path).exists() {
            client.call("thread/delete", serde_json::json!({ "threadId": child_id }))?;
        }
    }
    client.call(
        "thread/delete",
        serde_json::json!({ "threadId": session.native_id }),
    )?;
    if Path::new(&session.path).exists() {
        Err("E_VERIFY".into())
    } else {
        Ok(())
    }
}

fn process(
    mode: Mode,
    job_id: &str,
    roots: &Roots,
    rpc: &mut Option<super::codex_rpc::Rpc>,
    session: &Session,
    paths: &[PathBuf],
) -> Result<String, String> {
    let mut detail = String::new();
    match mode {
        Mode::SlimExport => {
            let file = super::export::write_slim(session, &super::commands::export_dir())?;
            detail = file.to_string_lossy().into_owned();
        }
        Mode::FullBackup => {
            let target = super::annotations::root()
                .join("backups")
                .join(job_id)
                .join(format!("{}-{}", session.agent.as_str(), session.native_id));
            for path in paths {
                copy_tree(path, &target.join(path.file_name().unwrap_or_default()))?;
            }
            detail = target.to_string_lossy().into_owned();
        }
        Mode::Direct => {}
    }
    match session.agent {
        Agent::Claude => delete_claude(paths, roots)?,
        Agent::Codex => delete_codex(rpc, Path::new(&roots.codex), session)?,
        Agent::CodeBuddy => delete_files(paths, Path::new(&roots.codebuddy))?,
        Agent::MiMo => delete_mimo(&session.native_id)?,
    }
    Ok(detail)
}

pub fn execute(token: String) -> Result<JobState, String> {
    if job().is_some_and(|j| j.state == "running") {
        return Err("E_BUSY".into());
    }
    let stored = PREVIEWS
        .lock()
        .map_err(super::err)?
        .get_or_insert_with(HashMap::new)
        .remove(&token)
        .ok_or("E_PREVIEW")?;
    if super::now().saturating_sub(stored.preview.created) > PREVIEW_SECONDS {
        return Err("E_PREVIEW".into());
    }
    super::catalog::invalidate();
    let (sessions, _, roots) = super::commands::annotated_catalog()?;
    let ids: Vec<String> = stored
        .preview
        .sessions
        .iter()
        .map(|s| s.id.clone())
        .collect();
    let (allowed, blocked) = plan(&sessions, &ids, &roots, super::now());
    if !blocked.is_empty() || fingerprints(&allowed) != stored.fingerprints {
        return Err("E_CHANGED".into());
    }
    let job_id = format!("delete-{}", super::now());
    let mut state = JobState {
        id: job_id.clone(),
        state: "running".into(),
        total: allowed.len(),
        export_dir: stored.preview.export_dir.clone(),
        ..Default::default()
    };
    CANCEL.store(false, Ordering::SeqCst);
    publish(&state);
    let mode = stored.preview.mode;
    let started = state.clone();
    std::thread::spawn(move || {
        let mut rpc = None;
        let mut completed = Vec::new();
        for (session, paths) in allowed {
            if CANCEL.load(Ordering::SeqCst) {
                state.state = "cancelled".into();
                break;
            }
            let result = process(mode, &job_id, &roots, &mut rpc, &session, &paths);
            let (status, detail) = match result {
                Ok(detail) => {
                    completed.push(session.id.clone());
                    ("completed", detail)
                }
                Err(code) => {
                    log::warn!(target:"stacker::sessions","delete {} failed: {}", session.id, code);
                    ("failed", code)
                }
            };
            state.items.push(JobItem {
                id: session.id.clone(),
                title: session.title.clone(),
                status: status.into(),
                detail,
            });
            state.done += 1;
            publish(&state);
        }
        drop(rpc);
        if let Ok(conn) = super::annotations::connect() {
            let _ = super::annotations::forget(&conn, &completed);
        }
        super::catalog::invalidate();
        if state.state == "running" {
            state.state = if state.items.iter().any(|i| i.status == "failed") {
                "failed"
            } else {
                "completed"
            }
            .into();
        }
        publish(&state);
    });
    Ok(started)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        Roots {
            claude: root.to_string_lossy().into_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn desktop_sessions_and_fresh_writes_are_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".claude");
        let shown = claude_session(&root, "shown", true);
        let fresh = claude_session(&root, "fresh", false);
        let old = claude_session(&root, "old", false);
        let later = super::super::now() + 10;
        let (allowed, blocked) = plan(
            &[shown, fresh.clone(), old],
            &["claude:shown".into(), "claude:old".into()],
            &roots(&root),
            later + 1_000,
        );
        assert_eq!(
            blocked
                .iter()
                .map(|b| b.reason.as_str())
                .collect::<Vec<_>>(),
            vec!["E_IN_DESKTOP"]
        );
        assert_eq!(allowed.len(), 1);
        let files: Vec<_> = allowed[0]
            .1
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(files.contains(&"old.jsonl".to_string()) && files.contains(&"old".to_string()));
        let (_, blocked) = plan(&[fresh], &["claude:fresh".into()], &roots(&root), later);
        assert_eq!(blocked[0].reason, "E_IN_USE");
    }

    #[test]
    fn claude_deletion_removes_every_related_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".claude");
        let s = claude_session(&root, "gone", false);
        let (allowed, _) = plan(
            &[s],
            &["claude:gone".into()],
            &roots(&root),
            super::super::now() + 1_000,
        );
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
        assert!(delete_claude(std::slice::from_ref(&outside), &roots(&root)).is_err());
        assert!(outside.exists());
    }

    #[test]
    fn full_backup_copies_directories() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a");
        fs::create_dir_all(from.join("sub")).unwrap();
        fs::write(from.join("sub").join("f"), b"x").unwrap();
        copy_tree(&from, &dir.path().join("b")).unwrap();
        assert!(dir.path().join("b").join("sub").join("f").exists());
    }
}
