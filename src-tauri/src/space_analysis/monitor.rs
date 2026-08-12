use super::windows_fs::{allocated_size, display_path};
use crate::vibe::scan_agent_activity;
use chrono::Local;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

const EVENT_WAIT: Duration = Duration::from_secs(1);
const AGENT_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const MAX_EVENTS: usize = 100;
const MAX_TRACKED_FILES: usize = 750_000;
const MAX_DIRECTORY_TOTALS: usize = 20_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorFileChange {
    pub path: String,
    pub kind: String,
    pub size_bytes: u64,
    pub delta_bytes: i64,
    pub modified_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDirectoryChange {
    pub path: String,
    pub delta_bytes: i64,
    pub files_changed: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorAgentProcess {
    #[serde(default)]
    pub agent_id: String,
    pub agent: String,
    pub pid: u32,
    pub parent_pid: u32,
    pub process_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSnapshot {
    pub task_id: String,
    pub state: String,
    pub roots: Vec<String>,
    pub started_at: String,
    pub updated_at: String,
    pub baseline_bytes: u64,
    pub current_bytes: u64,
    pub delta_bytes: i64,
    pub files_scanned: u64,
    pub files_changed: u64,
    pub directories_scanned: u64,
    pub skipped_paths: u64,
    pub running_agents: Vec<MonitorAgentProcess>,
    pub directories: Vec<MonitorDirectoryChange>,
    pub events: Vec<MonitorFileChange>,
    pub attribution_note: String,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
struct FileMark {
    size_bytes: u64,
    modified_ms: u128,
}

struct MonitorRecord {
    snapshot: MonitorSnapshot,
    cancel: Arc<AtomicBool>,
}

type Records = Arc<Mutex<HashMap<String, MonitorRecord>>>;

pub struct SpaceMonitorManager {
    next_id: AtomicU64,
    records: Records,
}

impl Default for SpaceMonitorManager {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            records: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SpaceMonitorManager {
    pub fn start(&self, roots: Vec<String>) -> Result<String, String> {
        let roots = normalize_roots(roots)?;
        let task_id = format!(
            "space-monitor-{}",
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let now = now();
        let snapshot = MonitorSnapshot {
            task_id: task_id.clone(),
            state: "starting".into(),
            roots: roots.iter().map(|root| display_path(root)).collect(),
            started_at: now.clone(),
            updated_at: now,
            baseline_bytes: 0,
            current_bytes: 0,
            delta_bytes: 0,
            files_scanned: 0,
            files_changed: 0,
            directories_scanned: 0,
            skipped_paths: 0,
            running_agents: Vec::new(),
            directories: Vec::new(),
            events: Vec::new(),
            attribution_note: "File changes cannot be reliably attributed to a specific process. Running agents are shown for context only.".into(),
            error: None,
        };
        self.records
            .lock()
            .map_err(|_| "Space monitor state is unavailable.".to_string())?
            .insert(
                task_id.clone(),
                MonitorRecord {
                    snapshot,
                    cancel: cancel.clone(),
                },
            );

        let records = self.records.clone();
        let worker_id = task_id.clone();
        thread::spawn(move || run_monitor(worker_id, roots, cancel, records));
        Ok(task_id)
    }

    pub fn status(&self, task_id: &str) -> Result<MonitorSnapshot, String> {
        self.records
            .lock()
            .map_err(|_| "Space monitor state is unavailable.".to_string())?
            .get(task_id)
            .map(|record| record.snapshot.clone())
            .ok_or_else(|| "Space monitor was not found.".to_string())
    }

    pub fn stop(&self, task_id: &str) -> Result<(), String> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| "Space monitor state is unavailable.".to_string())?;
        let record = records
            .get_mut(task_id)
            .ok_or_else(|| "Space monitor was not found.".to_string())?;
        record.cancel.store(true, Ordering::Relaxed);
        if record.snapshot.state == "running" || record.snapshot.state == "starting" {
            record.snapshot.state = "stopping".into();
            record.snapshot.updated_at = now();
        }
        Ok(())
    }

    pub fn dispose(&self, task_id: &str) -> Result<(), String> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| "Space monitor state is unavailable.".to_string())?;
        if let Some(record) = records.remove(task_id) {
            record.cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

fn run_monitor(task_id: String, roots: Vec<PathBuf>, cancel: Arc<AtomicBool>, records: Records) {
    let initial = snapshot_roots_controlled(&roots, Some(cancel.as_ref()), |progress| {
        update_snapshot(&records, &task_id, |snapshot| {
            snapshot.updated_at = now();
            snapshot.current_bytes = progress.total_bytes;
            snapshot.files_scanned = progress.files;
            snapshot.directories_scanned = progress.directories;
            snapshot.skipped_paths = progress.skipped;
        });
    });
    if cancel.load(Ordering::Relaxed) {
        update_snapshot(&records, &task_id, |snapshot| {
            snapshot.state = "stopped".into();
            snapshot.updated_at = now();
        });
        return;
    }
    if initial.limit_reached {
        fail_monitor(
            &records,
            &task_id,
            format!(
                "The selected tracking scope contains more than {} files. Narrow the scope or disable large cache folders before starting the work session.",
                MAX_TRACKED_FILES
            ),
        );
        return;
    }
    let mut files = initial.files;
    let baseline_bytes = initial.total_bytes;
    let mut current_bytes = initial.total_bytes;
    let mut files_changed = 0u64;
    let mut events = Vec::new();
    let mut directory_deltas = HashMap::new();
    let (event_tx, event_rx) = mpsc::channel();
    let mut watcher = match RecommendedWatcher::new(event_tx, notify::Config::default()) {
        Ok(watcher) => watcher,
        Err(error) => {
            fail_monitor(
                &records,
                &task_id,
                format!("Unable to start file monitoring: {error}"),
            );
            return;
        }
    };
    for root in &roots {
        if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
            fail_monitor(
                &records,
                &task_id,
                format!("Unable to monitor {}: {error}", display_path(root)),
            );
            return;
        }
    }
    let agents = scan_agent_activity()
        .map(|snapshot| monitor_processes(snapshot.processes))
        .unwrap_or_default();
    let mut last_agent_refresh = std::time::Instant::now();
    update_snapshot(&records, &task_id, |snapshot| {
        snapshot.state = "running".into();
        snapshot.baseline_bytes = baseline_bytes;
        snapshot.current_bytes = current_bytes;
        snapshot.files_scanned = initial.files_scanned;
        snapshot.directories_scanned = initial.directories;
        snapshot.skipped_paths = initial.skipped;
        snapshot.running_agents = agents;
    });

    while !cancel.load(Ordering::Relaxed) {
        let mut batch = Vec::new();
        match event_rx.recv_timeout(EVENT_WAIT) {
            Ok(Ok(event)) => batch.push(event),
            Ok(Err(error)) => set_monitor_error(&records, &task_id, error.to_string()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                fail_monitor(
                    &records,
                    &task_id,
                    "File monitoring stopped unexpectedly.".into(),
                );
                return;
            }
        }
        loop {
            match event_rx.try_recv() {
                Ok(Ok(event)) => batch.push(event),
                Ok(Err(error)) => set_monitor_error(&records, &task_id, error.to_string()),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    fail_monitor(
                        &records,
                        &task_id,
                        "File monitoring stopped unexpectedly.".into(),
                    );
                    return;
                }
            }
        }

        let mut changes = Vec::new();
        for event in batch {
            apply_event(event, &mut files, &mut current_bytes, &mut changes);
        }
        let had_changes = !changes.is_empty();
        if had_changes {
            files_changed = files_changed.saturating_add(changes.len() as u64);
            accumulate_directory_changes(&mut directory_deltas, &changes);
            changes.reverse();
            changes.extend(events);
            events = changes;
            events.truncate(MAX_EVENTS);
        }

        let agents = if last_agent_refresh.elapsed() >= AGENT_REFRESH_INTERVAL {
            last_agent_refresh = std::time::Instant::now();
            Some(
                scan_agent_activity()
                    .map(|snapshot| monitor_processes(snapshot.processes))
                    .unwrap_or_default(),
            )
        } else {
            None
        };
        if had_changes || agents.is_some() {
            update_snapshot(&records, &task_id, |snapshot| {
                snapshot.updated_at = now();
                snapshot.current_bytes = current_bytes;
                snapshot.delta_bytes = signed_delta(current_bytes, baseline_bytes);
                snapshot.files_changed = files_changed;
                snapshot.directories = rank_directory_changes(&directory_deltas);
                snapshot.events = events.clone();
                if let Some(agents) = agents {
                    snapshot.running_agents = agents;
                }
            });
        }
    }

    update_snapshot(&records, &task_id, |snapshot| {
        snapshot.state = "stopped".into();
        snapshot.updated_at = now();
    });
}

fn monitor_processes(processes: Vec<crate::vibe::AgentProcess>) -> Vec<MonitorAgentProcess> {
    processes
        .into_iter()
        .map(|process| MonitorAgentProcess {
            agent_id: process.agent_id,
            agent: process.agent,
            pid: process.pid,
            parent_pid: process.parent_pid,
            process_name: process.process_name,
        })
        .collect()
}

fn apply_event(
    event: Event,
    files: &mut HashMap<String, FileMark>,
    current_bytes: &mut u64,
    changes: &mut Vec<MonitorFileChange>,
) {
    match event.kind {
        EventKind::Remove(_) => {
            for path in event.paths {
                remove_path(&path, files, current_bytes, changes);
            }
        }
        EventKind::Create(_) => {
            for path in event.paths {
                add_or_update_path(&path, files, current_bytes, changes, true);
            }
        }
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
            if event.paths.len() >= 2 {
                remove_path(&event.paths[0], files, current_bytes, changes);
                for path in event.paths.iter().skip(1) {
                    add_or_update_path(path, files, current_bytes, changes, true);
                }
            } else if let Some(path) = event.paths.first() {
                if path.exists() {
                    add_or_update_path(path, files, current_bytes, changes, true);
                } else {
                    remove_path(path, files, current_bytes, changes);
                }
            }
        }
        EventKind::Modify(_) => {
            for path in event.paths {
                add_or_update_path(&path, files, current_bytes, changes, false);
            }
        }
        _ => {}
    }
}

fn add_or_update_path(
    path: &Path,
    files: &mut HashMap<String, FileMark>,
    current_bytes: &mut u64,
    changes: &mut Vec<MonitorFileChange>,
    include_directory: bool,
) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        remove_path(path, files, current_bytes, changes);
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        upsert_file(path, &metadata, files, current_bytes, changes);
    } else if metadata.is_dir() && include_directory {
        let discovered = snapshot_roots(&[path.to_path_buf()]);
        for (child, mark) in discovered.files {
            upsert_mark(&child, mark, files, current_bytes, changes);
        }
    }
}

fn upsert_file(
    path: &Path,
    metadata: &fs::Metadata,
    files: &mut HashMap<String, FileMark>,
    current_bytes: &mut u64,
    changes: &mut Vec<MonitorFileChange>,
) {
    upsert_mark(
        &path.to_string_lossy(),
        FileMark {
            size_bytes: allocated_size(path, metadata),
            modified_ms: modified_ms(metadata),
        },
        files,
        current_bytes,
        changes,
    );
}

fn upsert_mark(
    path: &str,
    mark: FileMark,
    files: &mut HashMap<String, FileMark>,
    current_bytes: &mut u64,
    changes: &mut Vec<MonitorFileChange>,
) {
    match files.insert(path.to_string(), mark) {
        Some(previous)
            if previous.size_bytes == mark.size_bytes
                && previous.modified_ms == mark.modified_ms => {}
        Some(previous) => {
            *current_bytes = current_bytes
                .saturating_sub(previous.size_bytes)
                .saturating_add(mark.size_bytes);
            changes.push(change(
                path,
                "modified",
                mark.size_bytes,
                signed_delta(mark.size_bytes, previous.size_bytes),
                mark.modified_ms,
            ));
        }
        None => {
            *current_bytes = current_bytes.saturating_add(mark.size_bytes);
            changes.push(change(
                path,
                "added",
                mark.size_bytes,
                mark.size_bytes.min(i64::MAX as u64) as i64,
                mark.modified_ms,
            ));
        }
    }
}

fn remove_path(
    path: &Path,
    files: &mut HashMap<String, FileMark>,
    current_bytes: &mut u64,
    changes: &mut Vec<MonitorFileChange>,
) {
    let path = path.to_string_lossy();
    let prefix = format!(
        "{}{}",
        path.trim_end_matches(['\\', '/']),
        std::path::MAIN_SEPARATOR
    );
    let removed = files
        .keys()
        .filter(|candidate| candidate.as_str() == path || candidate.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    for removed_path in removed {
        if let Some(previous) = files.remove(&removed_path) {
            *current_bytes = current_bytes.saturating_sub(previous.size_bytes);
            changes.push(change(
                &removed_path,
                "removed",
                0,
                -(previous.size_bytes.min(i64::MAX as u64) as i64),
                previous.modified_ms,
            ));
        }
    }
}

fn set_monitor_error(records: &Records, task_id: &str, error: String) {
    update_snapshot(records, task_id, |snapshot| {
        snapshot.updated_at = now();
        snapshot.error = Some(error);
    });
}

fn fail_monitor(records: &Records, task_id: &str, error: String) {
    update_snapshot(records, task_id, |snapshot| {
        snapshot.state = "failed".into();
        snapshot.updated_at = now();
        snapshot.error = Some(error);
    });
}

fn accumulate_directory_changes(
    totals: &mut HashMap<String, (i64, u64)>,
    changes: &[MonitorFileChange],
) {
    for change in changes {
        if change.delta_bytes == 0 {
            continue;
        }
        let directory = Path::new(&change.path)
            .parent()
            .map(display_path)
            .unwrap_or_else(|| change.path.clone());
        if !totals.contains_key(&directory) && totals.len() >= MAX_DIRECTORY_TOTALS {
            continue;
        }
        let entry = totals.entry(directory).or_default();
        entry.0 = entry.0.saturating_add(change.delta_bytes);
        entry.1 = entry.1.saturating_add(1);
    }
}

fn rank_directory_changes(totals: &HashMap<String, (i64, u64)>) -> Vec<MonitorDirectoryChange> {
    let mut rows: Vec<_> = totals
        .iter()
        .filter(|(_, (delta, _))| *delta != 0)
        .map(
            |(path, (delta_bytes, files_changed))| MonitorDirectoryChange {
                path: path.clone(),
                delta_bytes: *delta_bytes,
                files_changed: *files_changed,
            },
        )
        .collect();
    rows.sort_by(|left, right| {
        right
            .delta_bytes
            .unsigned_abs()
            .cmp(&left.delta_bytes.unsigned_abs())
    });
    rows.truncate(12);
    rows
}

fn update_snapshot(records: &Records, task_id: &str, update: impl FnOnce(&mut MonitorSnapshot)) {
    if let Ok(mut records) = records.lock() {
        if let Some(record) = records.get_mut(task_id) {
            update(&mut record.snapshot);
        }
    }
}

struct SnapshotFiles {
    files: HashMap<String, FileMark>,
    total_bytes: u64,
    files_scanned: u64,
    directories: u64,
    skipped: u64,
    limit_reached: bool,
}

#[derive(Clone, Copy)]
struct SnapshotProgress {
    total_bytes: u64,
    files: u64,
    directories: u64,
    skipped: u64,
}

fn snapshot_roots(roots: &[PathBuf]) -> SnapshotFiles {
    snapshot_roots_controlled(roots, None, |_| {})
}

fn snapshot_roots_controlled(
    roots: &[PathBuf],
    cancel: Option<&AtomicBool>,
    mut on_progress: impl FnMut(SnapshotProgress),
) -> SnapshotFiles {
    let mut snapshot = SnapshotFiles {
        files: HashMap::new(),
        total_bytes: 0,
        files_scanned: 0,
        directories: 0,
        skipped: 0,
        limit_reached: false,
    };
    let mut pending = roots.to_vec();
    snapshot.directories = pending.len() as u64;
    let mut last_progress = Instant::now();
    while let Some(directory) = pending.pop() {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            break;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                snapshot.skipped = snapshot.skipped.saturating_add(1);
                continue;
            }
        };
        for entry in entries {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    snapshot.skipped = snapshot.skipped.saturating_add(1);
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    snapshot.skipped = snapshot.skipped.saturating_add(1);
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                snapshot.directories = snapshot.directories.saturating_add(1);
                pending.push(path);
            } else if metadata.is_file() {
                if snapshot.files.len() >= MAX_TRACKED_FILES {
                    snapshot.limit_reached = true;
                    pending.clear();
                    break;
                }
                let size = allocated_size(&path, &metadata);
                snapshot.total_bytes = snapshot.total_bytes.saturating_add(size);
                snapshot.files_scanned = snapshot.files_scanned.saturating_add(1);
                snapshot.files.insert(
                    path.to_string_lossy().into_owned(),
                    FileMark {
                        size_bytes: size,
                        modified_ms: modified_ms(&metadata),
                    },
                );
            }
            if last_progress.elapsed() >= Duration::from_millis(180) {
                on_progress(snapshot.progress());
                last_progress = Instant::now();
            }
        }
    }
    on_progress(snapshot.progress());
    snapshot
}

impl SnapshotFiles {
    fn progress(&self) -> SnapshotProgress {
        SnapshotProgress {
            total_bytes: self.total_bytes,
            files: self.files_scanned,
            directories: self.directories,
            skipped: self.skipped,
        }
    }
}

#[cfg(test)]
fn diff_files(
    previous: &HashMap<String, FileMark>,
    current: &HashMap<String, FileMark>,
) -> (Vec<MonitorFileChange>, u64) {
    let mut changes = Vec::new();
    for (path, mark) in current {
        let Some(old) = previous.get(path) else {
            changes.push(change(
                path,
                "added",
                mark.size_bytes,
                mark.size_bytes as i64,
                mark.modified_ms,
            ));
            continue;
        };
        if old.size_bytes != mark.size_bytes || old.modified_ms != mark.modified_ms {
            changes.push(change(
                path,
                "modified",
                mark.size_bytes,
                signed_delta(mark.size_bytes, old.size_bytes),
                mark.modified_ms,
            ));
        }
    }
    for (path, old) in previous {
        if !current.contains_key(path) {
            changes.push(change(
                path,
                "removed",
                0,
                -(old.size_bytes.min(i64::MAX as u64) as i64),
                old.modified_ms,
            ));
        }
    }
    let count = changes.len() as u64;
    changes.sort_by_key(|change| std::cmp::Reverse(change.delta_bytes.abs()));
    changes.truncate(24);
    (changes, count)
}

fn change(
    path: &str,
    kind: &str,
    size_bytes: u64,
    delta_bytes: i64,
    modified_ms: u128,
) -> MonitorFileChange {
    MonitorFileChange {
        path: display_path(Path::new(path)),
        kind: kind.to_string(),
        size_bytes,
        delta_bytes,
        modified_at: Some(format_system_time(modified_ms)),
    }
}

fn normalize_roots(values: Vec<String>) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    for value in values {
        let path = fs::canonicalize(value.trim()).map_err(|_| {
            "The selected tracking folder does not exist or cannot be accessed.".to_string()
        })?;
        if !path.is_dir() {
            return Err("Space tracking requires folders, not files.".into());
        }
        roots.push(path);
    }
    roots.sort_by_key(|path| path.components().count());
    let mut collapsed = Vec::new();
    for root in roots {
        if !collapsed
            .iter()
            .any(|parent: &PathBuf| root.starts_with(parent))
        {
            collapsed.push(root);
        }
    }
    let roots = collapsed;
    if roots.is_empty() {
        Err("Select at least one folder to track.".into())
    } else {
        Ok(roots)
    }
}

fn signed_delta(current: u64, previous: u64) -> i64 {
    if current >= previous {
        (current - previous).min(i64::MAX as u64) as i64
    } else {
        -((previous - current).min(i64::MAX as u64) as i64)
    }
}

fn modified_ms(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn format_system_time(milliseconds: u128) -> String {
    if milliseconds == 0 {
        return String::new();
    }
    let time = UNIX_EPOCH + Duration::from_millis(milliseconds.min(u64::MAX as u128) as u64);
    chrono::DateTime::<Local>::from(time).to_rfc3339()
}

fn now() -> String {
    Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::{
        accumulate_directory_changes, add_or_update_path, apply_event, diff_files, normalize_roots,
        rank_directory_changes, remove_path, snapshot_roots, FileMark, SpaceMonitorManager,
    };
    use notify::event::{ModifyKind, RenameMode};
    use notify::{Event, EventKind};
    use std::collections::HashMap;
    use std::fs;

    #[test]
    fn baseline_snapshot_counts_files_directories_and_bytes() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(root.path().join("one.bin"), [1u8; 4]).unwrap();
        fs::write(nested.join("two.bin"), [2u8; 8]).unwrap();

        let snapshot = snapshot_roots(&[root.path().to_path_buf()]);

        assert_eq!(snapshot.files_scanned, 2);
        assert_eq!(snapshot.directories, 2);
        assert_eq!(snapshot.files.len(), 2);
        assert!(snapshot.total_bytes >= 12);
    }

    #[test]
    fn nested_tracking_roots_are_collapsed_without_dropping_siblings() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        let sibling = tempfile::tempdir().unwrap();
        fs::create_dir(&child).unwrap();

        let roots = normalize_roots(vec![
            child.to_string_lossy().into_owned(),
            root.path().to_string_lossy().into_owned(),
            sibling.path().to_string_lossy().into_owned(),
        ])
        .unwrap();

        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&root.path().canonicalize().unwrap()));
        assert!(roots.contains(&sibling.path().canonicalize().unwrap()));
    }

    #[test]
    fn disposed_monitor_is_removed_and_can_be_disposed_again() {
        let root = tempfile::tempdir().unwrap();
        let manager = SpaceMonitorManager::default();
        let task_id = manager
            .start(vec![root.path().to_string_lossy().into_owned()])
            .unwrap();

        manager.dispose(&task_id).unwrap();
        manager.dispose(&task_id).unwrap();

        assert!(manager.status(&task_id).is_err());
    }

    #[test]
    fn diff_files_reports_added_modified_and_removed() {
        let previous = HashMap::from([
            (
                "removed.txt".to_string(),
                FileMark {
                    size_bytes: 4,
                    modified_ms: 1,
                },
            ),
            (
                "changed.txt".to_string(),
                FileMark {
                    size_bytes: 4,
                    modified_ms: 1,
                },
            ),
        ]);
        let current = HashMap::from([
            (
                "changed.txt".to_string(),
                FileMark {
                    size_bytes: 9,
                    modified_ms: 2,
                },
            ),
            (
                "added.txt".to_string(),
                FileMark {
                    size_bytes: 6,
                    modified_ms: 3,
                },
            ),
        ]);

        let (changes, count) = diff_files(&previous, &current);

        assert_eq!(count, 3);
        assert_eq!(changes.len(), 3);
        assert!(changes
            .iter()
            .any(|change| change.path == "added.txt" && change.kind == "added"));
        assert!(changes
            .iter()
            .any(|change| change.path == "changed.txt" && change.delta_bytes == 5));
        assert!(changes
            .iter()
            .any(|change| change.path == "removed.txt" && change.delta_bytes == -4));
    }

    #[test]
    fn directory_changes_are_accumulated_and_ranked() {
        let changes = vec![
            super::MonitorFileChange {
                path: r"C:\work\target\a.bin".into(),
                kind: "added".into(),
                size_bytes: 20,
                delta_bytes: 20,
                modified_at: None,
            },
            super::MonitorFileChange {
                path: r"C:\work\target\b.bin".into(),
                kind: "modified".into(),
                size_bytes: 15,
                delta_bytes: 10,
                modified_at: None,
            },
            super::MonitorFileChange {
                path: r"C:\work\logs\old.log".into(),
                kind: "removed".into(),
                size_bytes: 0,
                delta_bytes: -5,
                modified_at: None,
            },
        ];
        let mut totals = HashMap::new();

        accumulate_directory_changes(&mut totals, &changes);
        let ranked = rank_directory_changes(&totals);

        assert_eq!(ranked[0].path, r"C:\work\target");
        assert_eq!(ranked[0].delta_bytes, 30);
        assert_eq!(ranked[0].files_changed, 2);
        assert_eq!(ranked[1].delta_bytes, -5);
    }

    #[test]
    fn repeated_file_events_do_not_double_count_space() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.bin");
        fs::write(&path, [1u8; 5]).unwrap();
        let mut files = HashMap::new();
        let mut bytes = 0;
        let mut changes = Vec::new();
        let initial_size = super::allocated_size(&path, &fs::metadata(&path).unwrap());

        add_or_update_path(&path, &mut files, &mut bytes, &mut changes, false);
        assert_eq!(bytes, initial_size);
        assert_eq!(changes.len(), 1);

        changes.clear();
        add_or_update_path(&path, &mut files, &mut bytes, &mut changes, false);
        assert_eq!(bytes, initial_size);
        assert!(changes.is_empty());

        fs::write(&path, vec![2u8; 8193]).unwrap();
        let updated_size = super::allocated_size(&path, &fs::metadata(&path).unwrap());
        add_or_update_path(&path, &mut files, &mut bytes, &mut changes, false);
        assert_eq!(bytes, updated_size);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0].delta_bytes,
            super::signed_delta(updated_size, initial_size)
        );
    }

    #[test]
    fn removing_a_directory_removes_all_tracked_children() {
        let directory = tempfile::tempdir().unwrap();
        let removed = directory.path().join("target");
        let kept = directory.path().join("keep.bin");
        let first = removed.join("a.bin").to_string_lossy().into_owned();
        let second = removed
            .join("nested")
            .join("b.bin")
            .to_string_lossy()
            .into_owned();
        let kept = kept.to_string_lossy().into_owned();
        let mut files = HashMap::from([
            (
                first,
                FileMark {
                    size_bytes: 4,
                    modified_ms: 1,
                },
            ),
            (
                second,
                FileMark {
                    size_bytes: 6,
                    modified_ms: 1,
                },
            ),
            (
                kept.clone(),
                FileMark {
                    size_bytes: 3,
                    modified_ms: 1,
                },
            ),
        ]);
        let mut bytes = 13;
        let mut changes = Vec::new();

        remove_path(&removed, &mut files, &mut bytes, &mut changes);

        assert_eq!(bytes, 3);
        assert_eq!(changes.len(), 2);
        assert_eq!(
            changes.iter().map(|change| change.delta_bytes).sum::<i64>(),
            -10
        );
        assert!(files.contains_key(&kept));
    }

    #[test]
    fn one_sided_rename_events_reconcile_the_existing_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("renamed.bin");
        fs::write(&path, [3u8; 7]).unwrap();
        let mut files = HashMap::new();
        let mut bytes = 0;
        let mut changes = Vec::new();

        apply_event(
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To))).add_path(path.clone()),
            &mut files,
            &mut bytes,
            &mut changes,
        );
        assert_eq!(bytes, 7);
        assert_eq!(changes[0].kind, "added");

        changes.clear();
        fs::remove_file(&path).unwrap();
        apply_event(
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From))).add_path(path),
            &mut files,
            &mut bytes,
            &mut changes,
        );
        assert_eq!(bytes, 0);
        assert_eq!(changes[0].kind, "removed");
    }
}
