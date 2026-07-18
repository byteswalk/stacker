use super::model::{Paged, SnapshotChangeRow, SnapshotComparison, SnapshotMetadata};
use super::tasks::SpaceTaskManager;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const SNAPSHOT_SCHEMA_VERSION: u16 = 1;
const SNAPSHOT_RULE_VERSION: u16 = 1;
const MAX_COMPARE_PAGE: u64 = 500;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotEntry {
    relative_path: String,
    allocated_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    schema_version: u16,
    rule_version: u16,
    metadata: SnapshotMetadata,
    entries: Vec<SnapshotEntry>,
}

fn root_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir)
        .join("Stacker").join("space-analysis").join("snapshots")
}

fn normalize_target(value: &str) -> String {
    value.trim().replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase()
}

pub(crate) fn target_fingerprint(targets: &[String]) -> String {
    let mut normalized = targets.iter().map(|value| normalize_target(value)).collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in normalized.join("\0").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn relative_path(path: &str, targets: &[String]) -> Option<String> {
    targets.iter().filter_map(|target| {
        Path::new(path).strip_prefix(Path::new(target)).ok().map(|relative| {
            let value = relative.to_string_lossy().replace('\\', "/");
            if value.is_empty() { ".".into() } else { value }
        })
    }).min_by_key(|value| value.len())
}

fn snapshot_path(id: &str) -> Result<PathBuf, String> {
    let (fingerprint, file) = id.split_once('/').ok_or_else(|| "Invalid snapshot id.".to_string())?;
    if fingerprint.len() != 16 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
        || file.contains('/') || file.contains('\\') || !file.ends_with(".json") {
        return Err("Invalid snapshot id.".into());
    }
    Ok(root_dir().join(fingerprint).join(file))
}

fn read_snapshot(path: &Path) -> Result<StoredSnapshot, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let snapshot: StoredSnapshot = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION || snapshot.rule_version != SNAPSHOT_RULE_VERSION {
        return Err("Snapshot format is not compatible with this version.".into());
    }
    Ok(snapshot)
}

fn write_atomic(path: &Path, value: &StoredSnapshot) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "Snapshot directory is unavailable.".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    std::fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        error.to_string()
    })
}

pub fn save_completed(manager: &SpaceTaskManager, task_id: &str) -> Result<Option<SnapshotMetadata>, String> {
    let settings = crate::settings::load();
    if !settings.snapshots_enabled { return Ok(None); }
    let (summary, rows) = manager.snapshot_data(task_id)?;
    let fingerprint = target_fingerprint(&summary.targets);
    let created_at = Utc::now();
    let filename = format!("{}.json", created_at.format("%Y%m%dT%H%M%S%3fZ"));
    let id = format!("{fingerprint}/{filename}");
    let metadata = SnapshotMetadata {
        id: id.clone(), target_fingerprint: fingerprint.clone(), created_at: created_at.to_rfc3339(),
        targets: summary.targets.clone(), allocated_bytes: summary.allocated_bytes,
        directory_count: summary.directory_count,
    };
    let entries = rows.into_iter().filter_map(|(path, allocated_bytes)| {
        relative_path(&path, &summary.targets).map(|relative_path| SnapshotEntry { relative_path, allocated_bytes })
    }).collect();
    let snapshot = StoredSnapshot { schema_version: SNAPSHOT_SCHEMA_VERSION, rule_version: SNAPSHOT_RULE_VERSION, metadata: metadata.clone(), entries };
    write_atomic(&snapshot_path(&id)?, &snapshot)?;
    prune_group(&fingerprint, settings.snapshot_retention_days, settings.snapshot_max_per_target)?;
    Ok(Some(metadata))
}

fn prune_group(fingerprint: &str, retention_days: u16, max_count: u16) -> Result<(), String> {
    let directory = root_dir().join(fingerprint);
    if !directory.exists() { return Ok(()); }
    let cutoff = Utc::now() - Duration::days(i64::from(retention_days));
    let mut snapshots = std::fs::read_dir(&directory).map_err(|error| error.to_string())?
        .filter_map(Result::ok).filter_map(|entry| read_snapshot(&entry.path()).ok().map(|snapshot| (entry.path(), snapshot.metadata.created_at)))
        .collect::<Vec<_>>();
    snapshots.sort_by(|left, right| right.1.cmp(&left.1));
    for (index, (path, created)) in snapshots.into_iter().enumerate() {
        let expired = DateTime::parse_from_rfc3339(&created).map(|value| value.with_timezone(&Utc) < cutoff).unwrap_or(true);
        if expired || index >= usize::from(max_count) { let _ = std::fs::remove_file(path); }
    }
    Ok(())
}

pub fn list() -> Result<Vec<SnapshotMetadata>, String> {
    let root = root_dir();
    if !root.exists() { return Ok(Vec::new()); }
    let mut items = Vec::new();
    for group in std::fs::read_dir(root).map_err(|error| error.to_string())?.filter_map(Result::ok) {
        if !group.path().is_dir() { continue; }
        for file in std::fs::read_dir(group.path()).map_err(|error| error.to_string())?.filter_map(Result::ok) {
            if let Ok(snapshot) = read_snapshot(&file.path()) { items.push(snapshot.metadata); }
        }
    }
    items.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(items)
}

pub fn compare(base_id: &str, current_id: &str, offset: u64, limit: u64) -> Result<SnapshotComparison, String> {
    let base = read_snapshot(&snapshot_path(base_id)?)?;
    let current = read_snapshot(&snapshot_path(current_id)?)?;
    if base.metadata.target_fingerprint != current.metadata.target_fingerprint {
        return Err("Snapshots belong to different scan targets.".into());
    }
    let before = base.entries.iter().map(|entry| (entry.relative_path.as_str(), entry.allocated_bytes)).collect::<HashMap<_, _>>();
    let after = current.entries.iter().map(|entry| (entry.relative_path.as_str(), entry.allocated_bytes)).collect::<HashMap<_, _>>();
    let mut paths = BTreeMap::new();
    for path in before.keys().chain(after.keys()) { paths.insert((*path).to_string(), ()); }
    let mut rows = paths.into_keys().filter_map(|path| {
        let before_bytes = before.get(path.as_str()).copied().unwrap_or(0);
        let after_bytes = after.get(path.as_str()).copied().unwrap_or(0);
        (before_bytes != after_bytes).then(|| SnapshotChangeRow {
            relative_path: path, before_bytes, after_bytes,
            delta_bytes: i128::from(after_bytes).saturating_sub(i128::from(before_bytes)).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64,
        })
    }).collect::<Vec<_>>();
    rows.sort_by(|left, right| right.delta_bytes.unsigned_abs().cmp(&left.delta_bytes.unsigned_abs()));
    let total = rows.len() as u64;
    let limit = limit.clamp(1, MAX_COMPARE_PAGE);
    let start = usize::try_from(offset).unwrap_or(usize::MAX).min(rows.len());
    let end = start.saturating_add(limit as usize).min(rows.len());
    Ok(SnapshotComparison {
        delta_bytes: i128::from(current.metadata.allocated_bytes).saturating_sub(i128::from(base.metadata.allocated_bytes)).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64,
        base: base.metadata, current: current.metadata,
        changes: Paged { items: rows[start..end].to_vec(), offset, limit, total },
    })
}

pub fn delete(id: &str) -> Result<(), String> { std::fs::remove_file(snapshot_path(id)?).map_err(|error| error.to_string()) }
pub fn clear() -> Result<(), String> {
    let root = root_dir();
    if root.exists() { std::fs::remove_dir_all(root).map_err(|error| error.to_string())?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprint_is_order_independent() {
        assert_eq!(target_fingerprint(&["D:\\b".into(), "C:\\a".into()]), target_fingerprint(&["c:/a/".into(), "d:/b".into()]));
    }
    #[test]
    fn relative_entries_do_not_expose_target_prefix() {
        assert_eq!(relative_path(r"C:\Users\demo\project\target", &[r"C:\Users\demo\project".into()]), Some("target".into()));
    }
}
