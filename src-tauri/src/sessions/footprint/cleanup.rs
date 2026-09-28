//! Preview → re-verify → delete for selected footprint items.
use super::ledger::{ItemPaths, Scan};
use super::model::{FootprintItem, FootprintKind};
use crate::space_analysis::walker::is_link_or_reparse_point;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const PREVIEW_SECONDS: u64 = 600;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPreview {
    pub token: String,
    pub items: Vec<FootprintItem>,
    pub blocked: Vec<FootprintItem>,
    pub bytes: u64,
    pub created: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItemResult {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
    pub freed: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupJob {
    /// verifying | running | completed | failed
    pub state: String,
    pub done: usize,
    pub total: usize,
    pub freed: u64,
    pub items: Vec<CleanupItemResult>,
    pub error: String,
}

struct Stored {
    ids: Vec<String>,
    bytes: HashMap<String, u64>,
    created: u64,
}

static PREVIEWS: Mutex<Option<HashMap<String, Stored>>> = Mutex::new(None);
static JOB: Mutex<Option<CleanupJob>> = Mutex::new(None);

fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(crate::sessions::now());
    format!("{:016x}", h.finish())
}

fn items(scan: &Scan) -> impl Iterator<Item = &FootprintItem> {
    scan.report.agents.iter().flat_map(|a| a.items.iter())
}

/// Splits the selection into deletable and blocked items.
pub fn select(
    scan: &Scan,
    ids: &[String],
) -> Result<(Vec<FootprintItem>, Vec<FootprintItem>), String> {
    let mut ok = Vec::new();
    let mut blocked = Vec::new();
    for id in ids {
        let item = items(scan).find(|i| &i.id == id).ok_or("E_CHANGED")?;
        if !matches!(
            item.kind,
            FootprintKind::Reclaimable | FootprintKind::Review
        ) {
            return Err("E_REQUEST".into());
        }
        if item.blocked.is_some() {
            blocked.push(item.clone());
        } else {
            ok.push(item.clone());
        }
    }
    Ok((ok, blocked))
}

pub fn preview_with(scan: &Scan, ids: Vec<String>, now: u64) -> Result<CleanupPreview, String> {
    if ids.is_empty() {
        return Err("E_REQUEST".into());
    }
    let (items, blocked) = select(scan, &ids)?;
    let preview = CleanupPreview {
        token: token(),
        bytes: items.iter().map(|i| i.bytes).sum(),
        items,
        blocked,
        created: now,
    };
    let mut store = PREVIEWS.lock().map_err(crate::sessions::err)?;
    let map = store.get_or_insert_with(HashMap::new);
    map.retain(|_, s| now.saturating_sub(s.created) <= PREVIEW_SECONDS);
    map.insert(
        preview.token.clone(),
        Stored {
            ids: preview.items.iter().map(|i| i.id.clone()).collect(),
            bytes: preview
                .items
                .iter()
                .map(|i| (i.id.clone(), i.bytes))
                .collect(),
            created: now,
        },
    );
    Ok(preview)
}

/// Every stored item must still exist, be unblocked, and be within 1 % of its previewed size.
pub fn verify(
    scan: &Scan,
    ids: &[String],
    bytes: &HashMap<String, u64>,
) -> Result<Vec<FootprintItem>, String> {
    let (items, blocked) = select(scan, ids)?;
    if !blocked.is_empty() {
        return Err("E_APP_RUNNING".into());
    }
    for item in &items {
        let before = bytes.get(&item.id).copied().unwrap_or(0);
        if before.abs_diff(item.bytes) > before / 100 {
            return Err("E_CHANGED".into());
        }
    }
    Ok(items)
}

/// Deletes one item's paths; each must be a non-link path inside its root.
pub fn delete_item(paths: &ItemPaths) -> Result<(), String> {
    let root = fs::canonicalize(&paths.root).map_err(|_| "E_PATH".to_string())?;
    for path in &paths.paths {
        let Ok(meta) = fs::symlink_metadata(path) else {
            continue;
        };
        if is_link_or_reparse_point(&meta) {
            return Err("E_LINK".into());
        }
        let real = fs::canonicalize(path).map_err(|_| "E_PATH".to_string())?;
        if real == root || !real.starts_with(&root) {
            return Err("E_PATH".into());
        }
    }
    for path in &paths.paths {
        let Ok(meta) = fs::symlink_metadata(path) else {
            continue;
        };
        let result = if meta.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        result.map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => "E_ACCESS".to_string(),
            _ => format!("E_DELETE: {}", Path::new(path).display()),
        })?;
    }
    Ok(())
}

fn publish(job: &CleanupJob) {
    if let Ok(mut slot) = JOB.lock() {
        *slot = Some(job.clone());
    }
}

pub fn job() -> Option<CleanupJob> {
    JOB.lock().ok().and_then(|j| j.clone())
}

pub fn preview(ids: Vec<String>) -> Result<CleanupPreview, String> {
    let scan = super::run_scan()?;
    preview_with(&scan, ids, crate::sessions::now())
}

/// Starts the job; the fresh scan and verification run in the background.
pub fn execute(token: String) -> Result<CleanupJob, String> {
    if job().is_some_and(|j| matches!(j.state.as_str(), "verifying" | "running")) {
        return Err("E_BUSY".into());
    }
    let stored = PREVIEWS
        .lock()
        .map_err(crate::sessions::err)?
        .get_or_insert_with(HashMap::new)
        .remove(&token)
        .ok_or("E_PREVIEW")?;
    if crate::sessions::now().saturating_sub(stored.created) > PREVIEW_SECONDS {
        return Err("E_PREVIEW".into());
    }
    let mut state = CleanupJob {
        state: "verifying".into(),
        total: stored.ids.len(),
        ..Default::default()
    };
    publish(&state);
    let started = state.clone();
    std::thread::spawn(move || {
        let checked = super::run_scan()
            .and_then(|scan| verify(&scan, &stored.ids, &stored.bytes).map(|items| (scan, items)));
        let (scan, items) = match checked {
            Ok(v) => v,
            Err(code) => {
                state.state = "failed".into();
                state.error = code;
                publish(&state);
                return;
            }
        };
        state.state = "running".into();
        publish(&state);
        for item in items {
            let result = scan
                .paths
                .get(&item.id)
                .ok_or_else(|| "E_CHANGED".to_string())
                .and_then(delete_item);
            let freed = if result.is_ok() { item.bytes } else { 0 };
            state.freed += freed;
            state.items.push(CleanupItemResult {
                id: item.id.clone(),
                label: item.label.clone(),
                status: if result.is_ok() {
                    "completed"
                } else {
                    "failed"
                }
                .into(),
                detail: result.err().unwrap_or_default(),
                freed,
            });
            state.done += 1;
            publish(&state);
        }
        super::ledger::invalidate();
        state.state = if state.items.iter().any(|i| i.status == "failed") {
            "failed"
        } else {
            "completed"
        }
        .into();
        publish(&state);
    });
    Ok(started)
}

#[cfg(test)]
mod tests {
    use super::super::ledger::{scan, Root};
    use super::super::rules::RootKind;
    use super::*;
    use std::collections::HashSet;

    fn fixture() -> (tempfile::TempDir, Scan) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        fs::create_dir_all(home.join(".tmp")).unwrap();
        fs::write(home.join(".tmp").join("x"), vec![0u8; 1000]).unwrap();
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::write(home.join("sessions").join("a.jsonl"), b"{}").unwrap();
        let roots = vec![Root {
            product: crate::sessions::footprint::ledger::product("codex"),
            kind: RootKind::CodexHome,
            path: home,
            suffix: None,
        }];
        let s = scan(&roots, &HashSet::new(), &[], 0);
        (dir, s)
    }

    fn id(scan: &Scan, prefix: &str) -> String {
        items(scan)
            .find(|i| i.id.starts_with(prefix))
            .unwrap()
            .id
            .clone()
    }

    #[test]
    fn sessions_and_keep_items_cannot_be_selected() {
        let (_dir, s) = fixture();
        assert_eq!(
            preview_with(&s, vec![id(&s, "codex-sessions:")], 0).unwrap_err(),
            "E_REQUEST"
        );
    }

    #[test]
    fn changed_sizes_abort() {
        let (dir, s) = fixture();
        let tmp = id(&s, "codex-tmp:");
        let preview = preview_with(&s, vec![tmp.clone()], 0).unwrap();
        assert_eq!(preview.bytes, 1000);
        fs::write(dir.path().join("codex/.tmp/y"), vec![0u8; 500]).unwrap();
        let rescanned = scan(
            &[Root {
                product: crate::sessions::footprint::ledger::product("codex"),
                kind: RootKind::CodexHome,
                path: dir.path().join("codex"),
                suffix: None,
            }],
            &HashSet::new(),
            &[],
            0,
        );
        let bytes = HashMap::from([(tmp.clone(), 1000u64)]);
        assert_eq!(
            verify(&rescanned, std::slice::from_ref(&tmp), &bytes).unwrap_err(),
            "E_CHANGED"
        );
        let items = verify(&s, std::slice::from_ref(&tmp), &bytes).unwrap();
        delete_item(s.paths.get(&items[0].id).unwrap()).unwrap();
        assert!(!dir.path().join("codex/.tmp").exists());
    }

    #[test]
    fn paths_outside_the_root_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, b"keep").unwrap();
        let paths = ItemPaths {
            root: root.clone(),
            paths: vec![outside.clone()],
        };
        assert_eq!(delete_item(&paths).unwrap_err(), "E_PATH");
        assert!(outside.exists());
        let root_itself = ItemPaths {
            root: root.clone(),
            paths: vec![root.clone()],
        };
        assert_eq!(delete_item(&root_itself).unwrap_err(), "E_PATH");
    }
}
