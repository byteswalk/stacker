//! Preview → scan again and check → delete or slim, for the ticked leftovers.

use super::model::{LeftoverItem, LeftoverKind, Operation, Scan};
use crate::sessions::footprint::cleanup::{delete_item, CleanupItemResult, CleanupJob};
use crate::sessions::footprint::ledger::ItemPaths;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

const PREVIEW_SECONDS: u64 = 600;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverPreview {
    pub token: String,
    pub items: Vec<LeftoverItem>,
    pub blocked: Vec<LeftoverItem>,
    pub bytes: u64,
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

fn find<'a>(scan: &'a Scan, id: &str) -> Option<&'a LeftoverItem> {
    scan.report
        .projects
        .iter()
        .flat_map(|p| p.items.iter())
        .find(|i| i.id == id)
}

/// The build folder an old-profiles or incremental-cache item is part of.
pub fn parent_of(id: &str) -> Option<String> {
    let base = id
        .strip_suffix(":stale")
        .or_else(|| id.strip_suffix(":incremental"))?;
    Some(
        base.replacen("|Stale|", "|Build|", 1)
            .replacen("|Incremental|", "|Build|", 1),
    )
}

pub fn preview_with(scan: &Scan, ids: Vec<String>) -> Result<LeftoverPreview, String> {
    if ids.is_empty() {
        return Err("E_REQUEST".into());
    }
    let mut items = Vec::new();
    let mut blocked = Vec::new();
    // Part of a build folder that is cleaned whole anyway.
    let ids: Vec<String> = ids
        .iter()
        .filter(|id| !parent_of(id).is_some_and(|parent| ids.contains(&parent)))
        .cloned()
        .collect();
    for id in &ids {
        let item = find(scan, id).ok_or("E_CHANGED")?;
        if item.blocked.is_some() {
            blocked.push(item.clone());
        } else {
            items.push(item.clone());
        }
    }
    let now = crate::sessions::now();
    let preview = LeftoverPreview {
        token: token(),
        bytes: items.iter().map(|i| i.bytes).sum(),
        items,
        blocked,
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

/// The items to clean, found again in a fresh scan: still there, nothing running inside, and
/// within 1 % of the previewed size (screenshots only need to be there still).
pub fn verify(
    scan: &Scan,
    ids: &[String],
    bytes: &HashMap<String, u64>,
) -> Result<Vec<(LeftoverItem, Operation)>, String> {
    let mut found = Vec::new();
    for id in ids {
        let item = find(scan, id).ok_or("E_CHANGED")?;
        if item.blocked.is_some() {
            return Err("E_APP_RUNNING".into());
        }
        let before = bytes.get(id).copied().unwrap_or(0);
        if item.kind != LeftoverKind::Images && before.abs_diff(item.bytes) > before / 100 {
            return Err("E_CHANGED".into());
        }
        let operation = scan.operations.get(id).ok_or("E_CHANGED")?;
        found.push((item.clone(), operation.clone()));
    }
    Ok(found)
}

/// Cleans one item. A folder that cannot go (a file in use) does not stop the others.
fn clean_one(item: &LeftoverItem, operation: &Operation) -> (u64, Vec<String>) {
    let mut freed = 0;
    let mut failures = Vec::new();
    for path in &operation.paths {
        if operation.slim {
            match super::slim::slim_file(path) {
                Ok(saving) => freed += saving.bytes,
                Err(code) => failures.push(code),
            }
            continue;
        }
        let (bytes, _, _) = crate::project_junk::size_of(path);
        let one = ItemPaths {
            root: operation.root.clone(),
            paths: vec![path.clone()],
        };
        match delete_item(&one) {
            Ok(()) => freed += bytes,
            Err(code) => {
                // What a partly removed folder still holds was not freed.
                freed += bytes.saturating_sub(crate::project_junk::size_of(path).0);
                failures.push(code);
            }
        }
    }
    let _ = item;
    (freed, failures)
}

/// Runs the cleaning in the calling thread, reporting each step through `publish`.
pub fn run_items(
    items: Vec<(LeftoverItem, Operation)>,
    publish: &dyn Fn(&CleanupJob),
) -> CleanupJob {
    let mut state = CleanupJob {
        state: "running".into(),
        total: items.len(),
        ..Default::default()
    };
    publish(&state);
    for (item, operation) in items {
        let (freed, failures) = clean_one(&item, &operation);
        state.freed += freed;
        let all_failed = !failures.is_empty() && freed == 0;
        state.items.push(CleanupItemResult {
            id: item.id.clone(),
            label: item.label.clone(),
            status: if all_failed { "failed" } else { "completed" }.into(),
            detail: match failures.len() {
                0 => String::new(),
                n => format!("{n} 处被占用或无法删除，已跳过：{}", failures[0]),
            },
            freed,
        });
        state.done += 1;
        publish(&state);
    }
    super::scan::invalidate();
    crate::sessions::footprint::ledger::invalidate();
    state.state = if state.items.iter().any(|i| i.status == "failed") {
        "failed"
    } else {
        "completed"
    }
    .into();
    publish(&state);
    state
}

fn publish(job: &CleanupJob) {
    if let Ok(mut slot) = JOB.lock() {
        *slot = Some(job.clone());
    }
}

pub fn job() -> Option<CleanupJob> {
    JOB.lock().ok().and_then(|j| j.clone())
}

pub fn busy() -> bool {
    job().is_some_and(|j| matches!(j.state.as_str(), "verifying" | "running"))
}

pub fn preview(ids: Vec<String>) -> Result<LeftoverPreview, String> {
    let scan = super::scan::cached(false)?;
    preview_with(&scan, ids)
}

/// Starts the job; the fresh scan and the checks run in the background.
pub fn execute(token: String) -> Result<CleanupJob, String> {
    if busy() {
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
    let started = CleanupJob {
        state: "verifying".into(),
        total: stored.ids.len(),
        ..Default::default()
    };
    publish(&started);
    let mut failed = started.clone();
    std::thread::spawn(move || {
        let checked = super::scan::run().and_then(|scan| verify(&scan, &stored.ids, &stored.bytes));
        match checked {
            Ok(items) => {
                run_items(items, &publish);
            }
            Err(code) => {
                failed.state = "failed".into();
                failed.error = code;
                publish(&failed);
            }
        }
    });
    Ok(started)
}
