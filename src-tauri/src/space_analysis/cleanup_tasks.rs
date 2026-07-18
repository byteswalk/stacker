use super::classifier::{detect_project_kind, is_project_marker_file_name, matches_artifact_rule};
use super::cleanup_plan::{PlanValidation, StoredCleanupPlan, ValidationSource};
use super::known::{known_candidates, CleanupKind};
use super::model::{
    CleanupItemResult, CleanupItemState, CleanupProgress, CleanupResult, CleanupTaskState,
};
use super::walker::{is_link_or_reparse_point, measure_path, CancellationToken, ScanWalkError};
use super::windows_fs::{allocated_size, file_identity, file_link_count};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TASK_NOT_FOUND: &str = "Cleanup task was not found.";
const TASK_NOT_FINISHED: &str = "Cleanup task has not finished.";
const MAX_RETAINED_TASKS: usize = 32;

const REASON_MISSING: &str = "spaceAnalysis.cleanup.reason.missing";
const REASON_OUTSIDE_ROOT: &str = "spaceAnalysis.cleanup.reason.outsideRoot";
const REASON_LINK: &str = "spaceAnalysis.cleanup.reason.linkDetected";
const REASON_IDENTITY_CHANGED: &str = "spaceAnalysis.cleanup.reason.identityChanged";
const REASON_CLASSIFICATION_CHANGED: &str = "spaceAnalysis.cleanup.reason.classificationChanged";
const REASON_ACCESS_DENIED: &str = "spaceAnalysis.cleanup.reason.accessDenied";
const REASON_DELETE_FAILED: &str = "spaceAnalysis.cleanup.reason.deleteFailed";
const REASON_CANCELLED: &str = "spaceAnalysis.cleanup.reason.cancelled";

struct CleanupTaskRecord {
    token: CancellationToken,
    progress: CleanupProgress,
    result: Option<CleanupResult>,
    terminal_order: Option<u64>,
    handle: Option<JoinHandle<()>>,
}

type CleanupTaskRecords = Arc<Mutex<HashMap<String, CleanupTaskRecord>>>;

pub struct CleanupTaskManager {
    next_id: AtomicU64,
    next_terminal_order: Arc<AtomicU64>,
    tasks: CleanupTaskRecords,
}

impl Default for CleanupTaskManager {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            next_terminal_order: Arc::new(AtomicU64::new(1)),
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl CleanupTaskManager {
    pub fn start<E>(
        &self,
        stored_plan: StoredCleanupPlan,
        selected_node_ids: &[String],
        emit: E,
    ) -> Result<String, String>
    where
        E: Fn(&CleanupProgress) + Send + Sync + 'static,
    {
        let execution_plan = select_plan_items(stored_plan, selected_node_ids)?;
        let task_id = format!("cleanup-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let token = CancellationToken::default();
        let progress = CleanupProgress {
            task_id: task_id.clone(),
            plan_id: execution_plan.plan.plan_id.clone(),
            state: CleanupTaskState::Queued,
            completed_items: 0,
            total_items: execution_plan.plan.items.len() as u64,
            actual_released_bytes: 0,
            current_node_id: None,
        };

        let mut tasks = lock_tasks(&self.tasks);
        tasks.insert(
            task_id.clone(),
            CleanupTaskRecord {
                token: token.clone(),
                progress,
                result: None,
                terminal_order: None,
                handle: None,
            },
        );

        let worker_task_id = task_id.clone();
        let worker_tasks = Arc::clone(&self.tasks);
        let worker_terminal_order = Arc::clone(&self.next_terminal_order);
        let handle = thread::Builder::new().name(task_id.clone()).spawn(move || {
            update_progress(&worker_tasks, &worker_task_id, &emit, |progress| {
                progress.state = CleanupTaskState::Running;
            });

            let result = execute_plan(
                &worker_task_id,
                execution_plan,
                &token,
                |item_result, current_index, released_bytes| {
                    update_progress(&worker_tasks, &worker_task_id, &emit, |progress| {
                        progress.completed_items = current_index as u64;
                        progress.actual_released_bytes = released_bytes;
                        progress.current_node_id = Some(item_result.node_id.clone());
                    });
                },
            );

            let final_progress = {
                let mut tasks = lock_tasks(&worker_tasks);
                let Some(record) = tasks.get_mut(&worker_task_id) else {
                    return;
                };
                record.progress.state = result.state;
                record.progress.completed_items = result.items.len() as u64;
                record.progress.actual_released_bytes = result.actual_released_bytes;
                record.progress.current_node_id = None;
                record.result = Some(result);
                record.terminal_order = Some(worker_terminal_order.fetch_add(1, Ordering::Relaxed));
                let progress = record.progress.clone();
                prune_tasks(&mut tasks);
                progress
            };
            emit(&final_progress);

            let completed_handle = lock_tasks(&worker_tasks)
                .get_mut(&worker_task_id)
                .and_then(|record| record.handle.take());
            drop(completed_handle);
        });

        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => {
                tasks.remove(&task_id);
                return Err(format!("Unable to start cleanup task: {error}"));
            }
        };

        tasks
            .get_mut(&task_id)
            .expect("cleanup task was just inserted")
            .handle = Some(handle);
        Ok(task_id)
    }

    pub fn status(&self, task_id: &str) -> Result<CleanupProgress, String> {
        lock_tasks(&self.tasks)
            .get(task_id)
            .map(|record| record.progress.clone())
            .ok_or_else(|| TASK_NOT_FOUND.to_string())
    }

    pub fn cancel(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = lock_tasks(&self.tasks);
        let record = tasks
            .get_mut(task_id)
            .ok_or_else(|| TASK_NOT_FOUND.to_string())?;
        if !is_terminal(record.progress.state) {
            record.token.cancel();
            record.progress.state = CleanupTaskState::Cancelling;
        }
        Ok(())
    }

    pub fn result(&self, task_id: &str) -> Result<CleanupResult, String> {
        let tasks = lock_tasks(&self.tasks);
        let record = tasks
            .get(task_id)
            .ok_or_else(|| TASK_NOT_FOUND.to_string())?;
        record
            .result
            .clone()
            .ok_or_else(|| TASK_NOT_FINISHED.to_string())
    }

    pub fn cancel_all_and_wait(&self, timeout: Duration) {
        let mut handles = {
            let mut tasks = lock_tasks(&self.tasks);
            let mut handles = Vec::new();
            for (task_id, record) in tasks.iter_mut() {
                if !is_terminal(record.progress.state) {
                    record.token.cancel();
                    record.progress.state = CleanupTaskState::Cancelling;
                }
                if let Some(handle) = record.handle.take() {
                    handles.push((task_id.clone(), handle));
                }
            }
            handles
        };

        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        while !handles.is_empty() && Instant::now() < deadline {
            if let Some(index) = handles.iter().position(|(_, handle)| handle.is_finished()) {
                let (_, handle) = handles.swap_remove(index);
                let _ = handle.join();
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }

        if !handles.is_empty() {
            let mut tasks = lock_tasks(&self.tasks);
            for (task_id, handle) in handles {
                if let Some(record) = tasks.get_mut(&task_id) {
                    record.handle = Some(handle);
                }
            }
        }
    }

    #[cfg(test)]
    fn wait_for_test(&self, task_id: &str) {
        let handle = lock_tasks(&self.tasks)
            .get_mut(task_id)
            .and_then(|record| record.handle.take());
        if let Some(handle) = handle {
            handle.join().unwrap();
        }
    }
}

fn select_plan_items(
    mut stored: StoredCleanupPlan,
    selected_node_ids: &[String],
) -> Result<StoredCleanupPlan, String> {
    if selected_node_ids.is_empty() {
        return Err("Select at least one cleanup item.".into());
    }
    let selected = selected_node_ids.iter().cloned().collect::<HashSet<_>>();
    if selected.len() != selected_node_ids.len() {
        return Err("A cleanup item was selected more than once.".into());
    }
    if selected
        .iter()
        .any(|node_id| !stored.validation.contains_key(node_id))
    {
        return Err("A selected cleanup item is not part of this plan.".into());
    }
    stored
        .plan
        .items
        .retain(|item| selected.contains(&item.node_id));
    stored
        .validation
        .retain(|node_id, _| selected.contains(node_id));
    Ok(stored)
}

fn execute_plan<F>(
    task_id: &str,
    stored: StoredCleanupPlan,
    token: &CancellationToken,
    mut item_completed: F,
) -> CleanupResult
where
    F: FnMut(&CleanupItemResult, usize, u64),
{
    let mut items = Vec::with_capacity(stored.plan.items.len());
    let mut total_released = 0u64;

    for (index, plan_item) in stored.plan.items.iter().enumerate() {
        if token.is_cancelled() {
            append_cancelled_items(&stored, index, &mut items);
            break;
        }
        let mut result = CleanupItemResult {
            node_id: plan_item.node_id.clone(),
            path: plan_item.path.clone(),
            state: CleanupItemState::Running,
            validated_bytes: 0,
            actual_released_bytes: 0,
            reason_key: None,
        };

        let Some(validation) = stored.validation.get(&plan_item.node_id) else {
            result.state = CleanupItemState::Skipped;
            result.reason_key = Some(REASON_CLASSIFICATION_CHANGED.into());
            items.push(result);
            if let Some(item) = items.last() {
                item_completed(item, index + 1, total_released);
            }
            continue;
        };

        match revalidate(plan_item, validation, token) {
            Ok(validated_bytes) => {
                result.validated_bytes = validated_bytes;
                let Some(cleanup_kind) = CleanupKind::from_stable_str(&plan_item.cleanup_kind)
                else {
                    result.state = CleanupItemState::Skipped;
                    result.reason_key = Some(REASON_CLASSIFICATION_CHANGED.into());
                    items.push(result);
                    if let Some(item) = items.last() {
                        item_completed(item, index + 1, total_released);
                    }
                    continue;
                };
                let outcome =
                    delete_validated_path(Path::new(&plan_item.path), cleanup_kind, token);
                result.actual_released_bytes = outcome.released_bytes;
                total_released = total_released.saturating_add(outcome.released_bytes);
                match outcome.reason_key {
                    None => result.state = CleanupItemState::Completed,
                    Some(reason) if reason == REASON_CANCELLED => {
                        result.state = CleanupItemState::Cancelled;
                        result.reason_key = Some(reason.into());
                    }
                    Some(reason) => {
                        result.state = CleanupItemState::Failed;
                        result.reason_key = Some(reason.into());
                    }
                }
            }
            Err(reason) if reason == REASON_CANCELLED => {
                result.state = CleanupItemState::Cancelled;
                result.reason_key = Some(reason.into());
            }
            Err(reason) => {
                result.state = CleanupItemState::Skipped;
                result.reason_key = Some(reason.into());
            }
        }

        items.push(result);
        if let Some(item) = items.last() {
            item_completed(item, index + 1, total_released);
        }
        if token.is_cancelled() {
            append_cancelled_items(&stored, index + 1, &mut items);
            break;
        }
    }

    let state = if token.is_cancelled()
        || items
            .iter()
            .any(|item| item.state == CleanupItemState::Cancelled)
    {
        CleanupTaskState::Cancelled
    } else {
        CleanupTaskState::Completed
    };
    CleanupResult {
        task_id: task_id.into(),
        plan_id: stored.plan.plan_id,
        state,
        actual_released_bytes: total_released,
        items,
    }
}

fn append_cancelled_items(
    stored: &StoredCleanupPlan,
    start: usize,
    items: &mut Vec<CleanupItemResult>,
) {
    items.extend(
        stored
            .plan
            .items
            .iter()
            .skip(start)
            .map(|item| CleanupItemResult {
                node_id: item.node_id.clone(),
                path: item.path.clone(),
                state: CleanupItemState::Cancelled,
                validated_bytes: 0,
                actual_released_bytes: 0,
                reason_key: Some(REASON_CANCELLED.into()),
            }),
    );
}

fn revalidate(
    plan_item: &super::model::CleanupPlanItem,
    validation: &PlanValidation,
    token: &CancellationToken,
) -> Result<u64, &'static str> {
    if token.is_cancelled() {
        return Err(REASON_CANCELLED);
    }
    let path = Path::new(&plan_item.path);
    let metadata = fs::symlink_metadata(path).map_err(map_metadata_error)?;
    if is_link_or_reparse_point(&metadata) {
        return Err(REASON_LINK);
    }
    let canonical = path.canonicalize().map_err(map_metadata_error)?;
    if !is_inside_any_root(&canonical, &validation.allowed_roots) {
        return Err(REASON_OUTSIDE_ROOT);
    }
    if file_identity(&canonical).map_err(map_metadata_error)? != validation.expected_identity {
        return Err(REASON_IDENTITY_CHANGED);
    }

    match &validation.source {
        ValidationSource::Known { candidate_id } => {
            let still_known = known_candidates().into_iter().any(|candidate| {
                candidate.id == *candidate_id
                    && candidate
                        .path
                        .canonicalize()
                        .is_ok_and(|candidate_path| same_path(&candidate_path, &canonical))
                    && candidate.safety == plan_item.safety
                    && candidate.cleanup_kind.as_str() == plan_item.cleanup_kind
            });
            if !still_known {
                return Err(REASON_CLASSIFICATION_CHANGED);
            }
        }
        ValidationSource::ProjectArtifact {
            project_root,
            project_kind,
            project_evidence,
        } => {
            let root_metadata = fs::symlink_metadata(project_root).map_err(map_metadata_error)?;
            if is_link_or_reparse_point(&root_metadata) {
                return Err(REASON_LINK);
            }
            let canonical_root = project_root.canonicalize().map_err(map_metadata_error)?;
            if !is_inside_any_root(&canonical_root, &validation.allowed_roots)
                || !is_same_or_descendant(&canonical, &canonical_root)
            {
                return Err(REASON_OUTSIDE_ROOT);
            }
            let current_evidence = project_marker_evidence(&canonical_root)?;
            if !project_evidence.is_subset(&current_evidence)
                || detect_project_kind(&current_evidence) != Some(*project_kind)
            {
                return Err(REASON_CLASSIFICATION_CHANGED);
            }
            let cleanup_kind = CleanupKind::from_stable_str(&plan_item.cleanup_kind)
                .ok_or(REASON_CLASSIFICATION_CHANGED)?;
            if !matches_artifact_rule(
                *project_kind,
                &canonical_root,
                &canonical,
                &current_evidence,
                cleanup_kind,
                &plan_item.impact_key,
                plan_item.safety,
            ) {
                return Err(REASON_CLASSIFICATION_CHANGED);
            }
        }
    }

    measure_path(path, token, |_| {})
        .map(|stats| stats.allocated_bytes)
        .map_err(|error| match error {
            ScanWalkError::Cancelled => REASON_CANCELLED,
        })
}

fn project_marker_evidence(project_root: &Path) -> Result<HashSet<String>, &'static str> {
    let entries = fs::read_dir(project_root).map_err(map_metadata_error)?;
    let mut evidence = HashSet::new();
    for entry in entries {
        let entry = entry.map_err(map_metadata_error)?;
        let metadata = entry.file_type().map_err(map_metadata_error)?;
        if metadata.is_file() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if is_project_marker_file_name(&name) {
                evidence.insert(name);
            }
        }
    }
    Ok(evidence)
}

struct DeleteOutcome {
    released_bytes: u64,
    reason_key: Option<&'static str>,
}

struct DeleteEntry {
    path: PathBuf,
    expanded: bool,
    remove_directory: bool,
}

fn delete_validated_path(
    path: &Path,
    cleanup_kind: CleanupKind,
    token: &CancellationToken,
) -> DeleteOutcome {
    let remove_root = cleanup_kind == CleanupKind::WholeDirectory;
    let mut stack = vec![DeleteEntry {
        path: path.to_path_buf(),
        expanded: false,
        remove_directory: remove_root,
    }];
    let mut released_bytes = 0u64;

    while let Some(entry) = stack.pop() {
        if token.is_cancelled() {
            return DeleteOutcome {
                released_bytes,
                reason_key: Some(REASON_CANCELLED),
            };
        }
        let metadata = match fs::symlink_metadata(&entry.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return DeleteOutcome {
                    released_bytes,
                    reason_key: Some(map_delete_error(&error)),
                }
            }
        };
        if is_link_or_reparse_point(&metadata) {
            return DeleteOutcome {
                released_bytes,
                reason_key: Some(REASON_LINK),
            };
        }
        if metadata.is_dir() {
            if entry.expanded {
                if entry.remove_directory && fs::remove_dir(&entry.path).is_err() {
                    return DeleteOutcome {
                        released_bytes,
                        reason_key: Some(REASON_DELETE_FAILED),
                    };
                }
                continue;
            }
            stack.push(DeleteEntry {
                path: entry.path.clone(),
                expanded: true,
                remove_directory: entry.remove_directory,
            });
            let children = match fs::read_dir(&entry.path) {
                Ok(children) => children,
                Err(error) => {
                    return DeleteOutcome {
                        released_bytes,
                        reason_key: Some(map_delete_error(&error)),
                    }
                }
            };
            for child in children {
                let child = match child {
                    Ok(child) => child,
                    Err(error) => {
                        return DeleteOutcome {
                            released_bytes,
                            reason_key: Some(map_delete_error(&error)),
                        }
                    }
                };
                stack.push(DeleteEntry {
                    path: child.path(),
                    expanded: false,
                    remove_directory: true,
                });
            }
        } else {
            let released = if file_link_count(&entry.path).unwrap_or(1) <= 1 {
                allocated_size(&entry.path, &metadata)
            } else {
                0
            };
            if let Err(error) = fs::remove_file(&entry.path) {
                return DeleteOutcome {
                    released_bytes,
                    reason_key: Some(map_delete_error(&error)),
                };
            }
            released_bytes = released_bytes.saturating_add(released);
        }
    }

    DeleteOutcome {
        released_bytes,
        reason_key: None,
    }
}

fn is_inside_any_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        root.canonicalize()
            .is_ok_and(|root| is_same_or_descendant(path, &root))
    })
}

fn is_same_or_descendant(path: &Path, root: &Path) -> bool {
    let path = normalize_path(path);
    let root = normalize_path(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('\\'))
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn map_metadata_error(error: std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => REASON_MISSING,
        std::io::ErrorKind::PermissionDenied => REASON_ACCESS_DENIED,
        _ => REASON_CLASSIFICATION_CHANGED,
    }
}

fn map_delete_error(error: &std::io::Error) -> &'static str {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        REASON_ACCESS_DENIED
    } else {
        REASON_DELETE_FAILED
    }
}

fn update_progress<E, F>(tasks: &CleanupTaskRecords, task_id: &str, emit: &E, update: F)
where
    E: Fn(&CleanupProgress),
    F: FnOnce(&mut CleanupProgress),
{
    let progress = {
        let mut tasks = lock_tasks(tasks);
        let Some(record) = tasks.get_mut(task_id) else {
            return;
        };
        update(&mut record.progress);
        record.progress.clone()
    };
    emit(&progress);
}

fn prune_tasks(tasks: &mut HashMap<String, CleanupTaskRecord>) {
    let mut terminal = tasks
        .iter()
        .filter_map(|(task_id, record)| {
            is_terminal(record.progress.state)
                .then_some((task_id.clone(), record.terminal_order.unwrap_or_default()))
        })
        .collect::<Vec<_>>();
    terminal.sort_by_key(|(_, order)| std::cmp::Reverse(*order));
    for (task_id, _) in terminal.into_iter().skip(MAX_RETAINED_TASKS) {
        tasks.remove(&task_id);
    }
}

fn is_terminal(state: CleanupTaskState) -> bool {
    matches!(
        state,
        CleanupTaskState::Completed | CleanupTaskState::Cancelled | CleanupTaskState::Failed
    )
}

fn lock_tasks(
    tasks: &Mutex<HashMap<String, CleanupTaskRecord>>,
) -> MutexGuard<'_, HashMap<String, CleanupTaskRecord>> {
    tasks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::cleanup_plan::build_deep_plan_record;
    use crate::space_analysis::walker::{build_indexed_result, CancellationToken};

    fn rust_plan(root: &Path, artifact_names: &[&str]) -> StoredCleanupPlan {
        fs::write(root.join("Cargo.toml"), b"[package]").unwrap();
        for name in artifact_names {
            let artifact = root.join(name);
            fs::create_dir_all(&artifact).unwrap();
            fs::write(artifact.join("output.bin"), vec![1u8; 4096]).unwrap();
        }
        let result =
            build_indexed_result(&[root.to_path_buf()], &CancellationToken::default(), |_| {})
                .unwrap();
        let root_node = result.summary().root_nodes[0].clone();
        let children = result.children(&root_node.node_id, 0, 100).unwrap();
        let node_ids = children
            .items
            .iter()
            .filter(|item| artifact_names.contains(&item.name.as_str()))
            .map(|item| item.node_id.clone())
            .collect::<Vec<_>>();
        build_deep_plan_record(&result, "scan-1", "plan-1".into(), &node_ids).unwrap()
    }

    #[test]
    fn successful_cleanup_removes_only_the_validated_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let plan = rust_plan(temp.path(), &["target"]);
        let source = temp.path().join("src");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("lib.rs"), b"source").unwrap();

        let result = execute_plan(
            "cleanup-1",
            plan,
            &CancellationToken::default(),
            |_, _, _| {},
        );

        assert_eq!(result.state, CleanupTaskState::Completed);
        assert_eq!(result.items[0].state, CleanupItemState::Completed);
        assert!(!temp.path().join("target").exists());
        assert!(source.join("lib.rs").exists());
    }

    #[test]
    fn vanished_and_replaced_targets_are_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let plan = rust_plan(temp.path(), &["target"]);
        fs::remove_dir_all(temp.path().join("target")).unwrap();
        let vanished = execute_plan(
            "cleanup-1",
            plan.clone(),
            &CancellationToken::default(),
            |_, _, _| {},
        );
        assert_eq!(vanished.items[0].state, CleanupItemState::Skipped);
        assert_eq!(
            vanished.items[0].reason_key.as_deref(),
            Some(REASON_MISSING)
        );

        fs::create_dir(temp.path().join("target")).unwrap();
        fs::write(temp.path().join("target/new.bin"), b"new").unwrap();
        let replaced = execute_plan(
            "cleanup-2",
            plan,
            &CancellationToken::default(),
            |_, _, _| {},
        );
        assert_eq!(replaced.items[0].state, CleanupItemState::Skipped);
        assert_eq!(
            replaced.items[0].reason_key.as_deref(),
            Some(REASON_IDENTITY_CHANGED)
        );
        assert!(temp.path().join("target/new.bin").exists());
    }

    #[test]
    fn canonical_escape_is_skipped_without_deleting_outside_files() {
        let scan_root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_path = outside.path().join("target");
        fs::create_dir(&outside_path).unwrap();
        fs::write(outside_path.join("keep.bin"), b"keep").unwrap();
        let mut plan = rust_plan(scan_root.path(), &["target"]);
        let item = &mut plan.plan.items[0];
        item.path = outside_path.to_string_lossy().into_owned();
        let validation = plan.validation.get_mut(&item.node_id).unwrap();
        validation.expected_identity = file_identity(&outside_path).unwrap();

        let result = execute_plan(
            "cleanup-1",
            plan,
            &CancellationToken::default(),
            |_, _, _| {},
        );

        assert_eq!(result.items[0].state, CleanupItemState::Skipped);
        assert_eq!(
            result.items[0].reason_key.as_deref(),
            Some(REASON_OUTSIDE_ROOT)
        );
        assert!(outside_path.join("keep.bin").exists());
    }

    #[test]
    fn changed_link_target_is_skipped_without_following_it() {
        let scan_root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let plan = rust_plan(scan_root.path(), &["target"]);
        fs::remove_dir_all(scan_root.path().join("target")).unwrap();
        fs::write(outside.path().join("keep.bin"), b"keep").unwrap();
        let link = scan_root.path().join("target");
        create_directory_link(outside.path(), &link);

        let result = execute_plan(
            "cleanup-1",
            plan,
            &CancellationToken::default(),
            |_, _, _| {},
        );

        assert_eq!(result.items[0].state, CleanupItemState::Skipped);
        assert_eq!(result.items[0].reason_key.as_deref(), Some(REASON_LINK));
        assert!(outside.path().join("keep.bin").exists());
        fs::remove_dir(&link).unwrap();
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) {
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn one_invalid_item_does_not_block_another_valid_cleanup() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("settings.gradle"), b"root").unwrap();
        for name in [".gradle", "build"] {
            fs::create_dir(root.path().join(name)).unwrap();
            fs::write(root.path().join(name).join("file.bin"), vec![1u8; 1024]).unwrap();
        }
        let index = build_indexed_result(
            &[root.path().to_path_buf()],
            &CancellationToken::default(),
            |_| {},
        )
        .unwrap();
        let root_node = index.summary().root_nodes[0].clone();
        let ids = index
            .children(&root_node.node_id, 0, 10)
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.node_id)
            .collect::<Vec<_>>();
        let plan = build_deep_plan_record(&index, "scan-1", "plan-1".into(), &ids).unwrap();
        fs::remove_dir_all(root.path().join("build")).unwrap();
        fs::create_dir(root.path().join("build")).unwrap();
        fs::write(root.path().join("build/keep.bin"), b"keep").unwrap();

        let result = execute_plan(
            "cleanup-1",
            plan,
            &CancellationToken::default(),
            |_, _, _| {},
        );

        assert!(result
            .items
            .iter()
            .any(|item| item.state == CleanupItemState::Completed));
        assert!(result
            .items
            .iter()
            .any(|item| item.state == CleanupItemState::Skipped));
        assert!(root.path().join("build/keep.bin").exists());
    }

    #[test]
    fn cancellation_before_the_next_item_preserves_it() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("settings.gradle"), b"root").unwrap();
        for name in [".gradle", "build"] {
            fs::create_dir(root.path().join(name)).unwrap();
            fs::write(root.path().join(name).join("file.bin"), vec![1u8; 1024]).unwrap();
        }
        let index = build_indexed_result(
            &[root.path().to_path_buf()],
            &CancellationToken::default(),
            |_| {},
        )
        .unwrap();
        let root_node = index.summary().root_nodes[0].clone();
        let ids = index
            .children(&root_node.node_id, 0, 10)
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.node_id)
            .collect::<Vec<_>>();
        let plan = build_deep_plan_record(&index, "scan-1", "plan-1".into(), &ids).unwrap();
        let token = CancellationToken::default();
        let cancel = token.clone();
        let result = execute_plan("cleanup-1", plan, &token, move |_, completed, _| {
            if completed == 1 {
                cancel.cancel();
            }
        });

        assert_eq!(result.state, CleanupTaskState::Cancelled);
        assert_eq!(result.items[0].state, CleanupItemState::Completed);
        assert_eq!(result.items[1].state, CleanupItemState::Cancelled);
        assert!(root.path().join("build").exists() || root.path().join(".gradle").exists());
    }

    #[test]
    fn manager_retains_item_level_results() {
        let temp = tempfile::tempdir().unwrap();
        let plan = rust_plan(temp.path(), &["target"]);
        let selected = vec![plan.plan.items[0].node_id.clone()];
        let manager = CleanupTaskManager::default();
        let task_id = manager.start(plan, &selected, |_| {}).unwrap();
        manager.wait_for_test(&task_id);

        let result = manager.result(&task_id).unwrap();
        assert_eq!(result.task_id, task_id);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].state, CleanupItemState::Completed);
    }
}
