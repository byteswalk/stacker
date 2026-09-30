//! Every cleanup that ran, kept after the result dialog is closed.
//!
//! "What did I delete last week?" has no answer once the dialog is gone, so each finished
//! cleanup is written down: when, what was asked for, what each item came to, and what it
//! freed. Only paths and sizes are kept — never file contents — and the list is bounded.

use super::model::{CleanupItemState, CleanupResult, CleanupTaskState};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How many cleanups are remembered; older ones fall off the end.
const MAX_RECORDS: usize = 200;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub path: String,
    pub state: CleanupItemState,
    pub released_bytes: u64,
    pub reason_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    /// Seconds since the epoch.
    pub at: u64,
    pub state: CleanupTaskState,
    pub released_bytes: u64,
    pub items: Vec<HistoryItem>,
}

impl HistoryRecord {
    pub fn from_result(result: &CleanupResult, at: u64) -> Self {
        Self {
            at,
            state: result.state,
            released_bytes: result.actual_released_bytes,
            items: result
                .items
                .iter()
                .map(|item| HistoryItem {
                    path: item.path.clone(),
                    state: item.state,
                    released_bytes: item.actual_released_bytes,
                    reason_key: item.reason_key.clone(),
                })
                .collect(),
        }
    }
}

fn file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Stacker")
        .join("cleanup-history.json")
}

/// Newest first.
pub fn load() -> Vec<HistoryRecord> {
    load_from(&file())
}

fn load_from(path: &std::path::Path) -> Vec<HistoryRecord> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// A finished cleanup joins the list. A run that touched nothing is not worth a line.
pub fn append(result: &CleanupResult) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Err(error) = append_to(&file(), HistoryRecord::from_result(result, at)) {
        log::warn!("failed to record the cleanup in its history: {error}");
    }
}

fn append_to(path: &std::path::Path, record: HistoryRecord) -> Result<(), String> {
    if record.items.is_empty() {
        return Ok(());
    }
    let mut records = load_from(path);
    records.insert(0, record);
    records.truncate(MAX_RECORDS);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec(&records).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

pub fn clear() -> Result<(), String> {
    match std::fs::remove_file(file()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::model::CleanupItemResult;

    fn result(items: usize) -> CleanupResult {
        CleanupResult {
            task_id: "cleanup-1".into(),
            plan_id: "plan-1".into(),
            state: CleanupTaskState::Completed,
            actual_released_bytes: 4096 * items as u64,
            items: (0..items)
                .map(|index| CleanupItemResult {
                    node_id: format!("node-{index}"),
                    path: format!("C:\\work\\target-{index}"),
                    state: CleanupItemState::Completed,
                    validated_bytes: 4096,
                    actual_released_bytes: 4096,
                    reason_key: None,
                })
                .collect(),
        }
    }

    #[test]
    fn a_cleanup_is_remembered_newest_first_and_the_list_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        append_to(&path, HistoryRecord::from_result(&result(1), 100)).unwrap();
        append_to(&path, HistoryRecord::from_result(&result(2), 200)).unwrap();
        let records = load_from(&path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].at, 200);
        assert_eq!(records[0].items.len(), 2);
        assert_eq!(records[0].released_bytes, 8192);
        assert_eq!(records[0].items[0].state, CleanupItemState::Completed);

        for at in 0..(MAX_RECORDS as u64 + 10) {
            append_to(&path, HistoryRecord::from_result(&result(1), 1000 + at)).unwrap();
        }
        assert_eq!(load_from(&path).len(), MAX_RECORDS);
    }

    #[test]
    fn a_run_that_touched_nothing_leaves_no_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        append_to(&path, HistoryRecord::from_result(&result(0), 100)).unwrap();
        assert!(load_from(&path).is_empty());
    }
}
