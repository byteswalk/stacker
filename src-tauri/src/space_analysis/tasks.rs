use super::cleanup_plan::{build_deep_plan_record, build_quick_plan_record, StoredCleanupPlan};
use super::known::scan_known_candidates;
use super::model::{
    AnalysisSummary, CleanupPlan, DirectoryNode, LargeFileRow, Paged, QuickScanResult,
    ScanProgress, ScanTaskState,
};
use super::walker::{
    build_indexed_result, CancellationToken, IndexedScanResult, ScanWalkError, WalkStats,
};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TASK_NOT_FOUND: &str = "未找到扫描任务";
const TASK_START_FAILED: &str = "无法启动扫描任务";
const TASK_PANICKED: &str = "扫描任务异常终止";
const TASK_QUEUED: &str = "扫描任务正在排队";
const TASK_RUNNING: &str = "扫描任务正在运行";
const TASK_CANCELLING: &str = "扫描任务正在取消";
const TASK_CANCELLED: &str = "扫描任务已取消";
const TASK_RESULT_UNAVAILABLE: &str = "扫描结果不可用";
const DEEP_TASK_LIMIT_REACHED: &str = "at most two deep scan tasks may run concurrently";
const DUPLICATE_DEEP_TASK: &str = "an active deep scan already has the same targets";
const OVERLAPPING_TARGETS: &str = "deep scan targets must not overlap";
const DIRECTORY_NODE_NOT_FOUND: &str = "directory node was not found";
// Three deep indexes cover recent navigation; 32 lightweight records cover route recovery and retries.
const MAX_RETAINED_DEEP_RESULTS: usize = 3;
const MAX_RETAINED_LIGHTWEIGHT_TASKS: usize = 32;
const MAX_RETAINED_CLEANUP_PLANS: usize = 32;

enum TaskKind {
    Quick,
    Deep { normalized_targets: Vec<String> },
}

enum TaskResult {
    Quick(QuickScanResult),
    Deep(IndexedScanResult),
}

struct TaskRecord {
    kind: TaskKind,
    token: CancellationToken,
    progress: ScanProgress,
    result: Option<TaskResult>,
    failure: Option<String>,
    terminal_order: Option<u64>,
    handle: Option<JoinHandle<()>>,
}

type TaskRecords = Arc<Mutex<HashMap<String, TaskRecord>>>;

pub struct SpaceTaskManager {
    next_id: AtomicU64,
    next_plan_id: AtomicU64,
    next_terminal_order: Arc<AtomicU64>,
    tasks: TaskRecords,
    cleanup_plans: Arc<Mutex<HashMap<String, StoredCleanupPlan>>>,
}

impl Default for SpaceTaskManager {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            next_plan_id: AtomicU64::new(1),
            next_terminal_order: Arc::new(AtomicU64::new(1)),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            cleanup_plans: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SpaceTaskManager {
    pub fn start_quick(&self, window: tauri::Window) -> Result<String, String> {
        use tauri::Emitter;

        self.start_worker(
            TaskKind::Quick,
            move |token, report_progress| {
                scan_known_candidates(token, |stats| report_progress(stats)).map(TaskResult::Quick)
            },
            move |progress| {
                if let Err(error) = window.emit("space-scan-progress", progress) {
                    log::warn!(
                        "failed to emit progress for space scan task {}: {}",
                        progress.task_id,
                        error
                    );
                }
            },
        )
    }

    pub fn start_deep(
        &self,
        targets: Vec<PathBuf>,
        window: tauri::Window,
    ) -> Result<String, String> {
        use tauri::Emitter;

        self.start_deep_worker(
            targets,
            move |progress| {
                if let Err(error) = window.emit("space-scan-progress", progress) {
                    log::warn!(
                        "failed to emit progress for space scan task {}: {}",
                        progress.task_id,
                        error
                    );
                }
            },
            |targets, token, report_progress| {
                build_indexed_result(targets, token, report_progress).map(TaskResult::Deep)
            },
        )
    }

    pub fn status(&self, task_id: &str) -> Result<ScanProgress, String> {
        lock_records(&self.tasks)
            .get(task_id)
            .map(|record| record.progress.clone())
            .ok_or_else(|| TASK_NOT_FOUND.into())
    }

    pub fn cancel(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = lock_records(&self.tasks);
        let record = tasks
            .get_mut(task_id)
            .ok_or_else(|| TASK_NOT_FOUND.to_string())?;
        if !is_terminal(record.progress.state) {
            record.token.cancel();
            record.progress.state = ScanTaskState::Cancelling;
        }
        Ok(())
    }

    pub fn quick_result(&self, task_id: &str) -> Result<QuickScanResult, String> {
        let tasks = lock_records(&self.tasks);
        let record = completed_record(&tasks, task_id)?;
        match record.result.as_ref() {
            Some(TaskResult::Quick(result)) => Ok(result.clone()),
            _ => Err(TASK_RESULT_UNAVAILABLE.into()),
        }
    }

    pub fn summary(&self, task_id: &str) -> Result<AnalysisSummary, String> {
        let tasks = lock_records(&self.tasks);
        let result = completed_deep_result(&tasks, task_id)?;
        Ok(result.summary())
    }

    pub fn children(
        &self,
        task_id: &str,
        parent_id: &str,
        offset: u64,
        limit: u64,
    ) -> Result<Paged<DirectoryNode>, String> {
        let tasks = lock_records(&self.tasks);
        let result = completed_deep_result(&tasks, task_id)?;
        result
            .children(parent_id, offset, limit)
            .ok_or_else(|| DIRECTORY_NODE_NOT_FOUND.into())
    }

    pub fn large_files(
        &self,
        task_id: &str,
        min_bytes: u64,
        offset: u64,
        limit: u64,
    ) -> Result<Paged<LargeFileRow>, String> {
        let tasks = lock_records(&self.tasks);
        let result = completed_deep_result(&tasks, task_id)?;
        Ok(result.large_files(min_bytes, offset, limit))
    }

    pub fn cleanup_candidates(&self, task_id: &str) -> Result<Vec<DirectoryNode>, String> {
        let tasks = lock_records(&self.tasks);
        let result = completed_deep_result(&tasks, task_id)?;
        Ok(result.cleanup_candidates())
    }

    pub fn create_cleanup_plan(
        &self,
        scan_task_id: &str,
        node_ids: &[String],
    ) -> Result<CleanupPlan, String> {
        let plan_id = format!(
            "cleanup-plan-{}",
            self.next_plan_id.fetch_add(1, Ordering::Relaxed)
        );
        let plan = {
            let tasks = lock_records(&self.tasks);
            let record = completed_record(&tasks, scan_task_id)?;
            match record.result.as_ref() {
                Some(TaskResult::Quick(result)) => {
                    build_quick_plan_record(result, scan_task_id, plan_id, node_ids)
                }
                Some(TaskResult::Deep(result)) => {
                    build_deep_plan_record(result, scan_task_id, plan_id, node_ids)
                }
                None => return Err(TASK_RESULT_UNAVAILABLE.into()),
            }
            .map_err(|error| error.to_string())?
        };

        let mut plans = self
            .cleanup_plans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        plans.insert(plan.plan.plan_id.clone(), plan.clone());
        prune_cleanup_plans(&mut plans);
        Ok(plan.plan)
    }

    #[cfg(test)]
    pub(crate) fn cleanup_plan(&self, plan_id: &str) -> Result<CleanupPlan, String> {
        self.cleanup_plans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(plan_id)
            .map(|stored| stored.plan.clone())
            .ok_or_else(|| "Cleanup plan was not found or has expired.".to_string())
    }

    pub(crate) fn cleanup_plan_record(&self, plan_id: &str) -> Result<StoredCleanupPlan, String> {
        self.cleanup_plans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(plan_id)
            .cloned()
            .ok_or_else(|| "Cleanup plan was not found or has expired.".to_string())
    }

    pub fn cancel_all_and_wait(&self, timeout: Duration) {
        let mut handles = {
            let mut tasks = lock_records(&self.tasks);
            let mut handles = Vec::new();
            for (task_id, record) in tasks.iter_mut() {
                if !is_terminal(record.progress.state) {
                    record.token.cancel();
                    record.progress.state = ScanTaskState::Cancelling;
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
        while !handles.is_empty() {
            if let Some(index) = handles.iter().position(|(_, handle)| handle.is_finished()) {
                let (_, handle) = handles.swap_remove(index);
                let _ = handle.join();
                continue;
            }

            let now = Instant::now();
            if now >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10).min(deadline.saturating_duration_since(now)));
        }

        if !handles.is_empty() {
            let mut tasks = lock_records(&self.tasks);
            for (task_id, handle) in handles {
                if let Some(record) = tasks.get_mut(&task_id) {
                    record.handle = Some(handle);
                }
            }
        }
    }

    fn start_deep_worker<F, E>(
        &self,
        targets: Vec<PathBuf>,
        emit: E,
        worker: F,
    ) -> Result<String, String>
    where
        F: FnOnce(
                &[PathBuf],
                &CancellationToken,
                &mut dyn FnMut(&WalkStats),
            ) -> Result<TaskResult, ScanWalkError>
            + Send
            + 'static,
        E: Fn(&ScanProgress) + Send + Sync + 'static,
    {
        let normalized_targets = normalize_target_set(&targets)?;
        let worker_targets = targets;
        self.start_worker(
            TaskKind::Deep { normalized_targets },
            move |token, report_progress| worker(&worker_targets, token, report_progress),
            emit,
        )
    }

    fn start_worker<F, E>(&self, kind: TaskKind, worker: F, emit: E) -> Result<String, String>
    where
        F: FnOnce(
                &CancellationToken,
                &mut dyn FnMut(&WalkStats),
            ) -> Result<TaskResult, ScanWalkError>
            + Send
            + 'static,
        E: Fn(&ScanProgress) + Send + Sync + 'static,
    {
        let mut tasks = lock_records(&self.tasks);
        match &kind {
            TaskKind::Quick => {
                if let Some((task_id, _)) = tasks.iter().find(|(_, record)| {
                    matches!(&record.kind, TaskKind::Quick) && !is_terminal(record.progress.state)
                }) {
                    return Ok(task_id.clone());
                }
            }
            TaskKind::Deep { normalized_targets } => {
                let active_deep = tasks
                    .values()
                    .filter(|record| {
                        matches!(&record.kind, TaskKind::Deep { .. })
                            && !is_terminal(record.progress.state)
                    })
                    .collect::<Vec<_>>();
                if active_deep.iter().any(|record| {
                    matches!(
                        &record.kind,
                        TaskKind::Deep {
                            normalized_targets: active
                        } if active == normalized_targets
                    )
                }) {
                    return Err(DUPLICATE_DEEP_TASK.into());
                }
                if active_deep.len() >= 2 {
                    return Err(DEEP_TASK_LIMIT_REACHED.into());
                }
            }
        }

        let task_id = format!("scan-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let token = CancellationToken::default();
        tasks.insert(
            task_id.clone(),
            TaskRecord {
                kind,
                token: token.clone(),
                progress: initial_progress(&task_id),
                result: None,
                failure: None,
                terminal_order: None,
                handle: None,
            },
        );

        let worker_task_id = task_id.clone();
        let worker_tasks = Arc::clone(&self.tasks);
        let worker_terminal_order = Arc::clone(&self.next_terminal_order);
        let handle = thread::Builder::new().name(task_id.clone()).spawn(move || {
            let started_at = Instant::now();
            let running = {
                let mut tasks = lock_records(&worker_tasks);
                let Some(record) = tasks.get_mut(&worker_task_id) else {
                    return;
                };
                if record.progress.state == ScanTaskState::Queued {
                    record.progress.state = ScanTaskState::Running;
                }
                record.progress.clone()
            };
            emit(&running);

            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let mut report_progress = |stats: &WalkStats| {
                    let progress = {
                        let mut tasks = lock_records(&worker_tasks);
                        let Some(record) = tasks.get_mut(&worker_task_id) else {
                            return;
                        };
                        apply_walk_stats(&mut record.progress, stats, started_at);
                        record.progress.clone()
                    };
                    emit(&progress);
                };
                worker(&token, &mut report_progress)
            }));

            let final_progress = {
                let mut tasks = lock_records(&worker_tasks);
                let progress = {
                    let Some(record) = tasks.get_mut(&worker_task_id) else {
                        return;
                    };
                    record.progress.elapsed_ms = elapsed_ms(started_at);
                    match outcome {
                        Ok(Ok(mut result))
                            if !token.is_cancelled()
                                && record.progress.state != ScanTaskState::Cancelling =>
                        {
                            match &mut result {
                                TaskResult::Quick(result) => {
                                    result.task_id = worker_task_id.clone();
                                    result.completed = true;
                                }
                                TaskResult::Deep(result) => result.set_task_id(&worker_task_id),
                            }
                            record.result = Some(result);
                            record.progress.state = ScanTaskState::Completed;
                        }
                        Ok(Ok(_)) | Ok(Err(ScanWalkError::Cancelled)) => {
                            record.progress.state = ScanTaskState::Cancelled;
                        }
                        Err(_) => {
                            log::error!("space scan task {} panicked", worker_task_id);
                            record.failure = Some(TASK_PANICKED.into());
                            record.progress.state = ScanTaskState::Failed;
                        }
                    }
                    record.terminal_order =
                        Some(worker_terminal_order.fetch_add(1, Ordering::Relaxed));
                    record.progress.clone()
                };
                prune_terminal_tasks(&mut tasks);
                progress
            };
            emit(&final_progress);

            // A worker cannot join itself. Dropping its own handle detaches it without
            // retaining an OS thread resource in the terminal task record.
            let completed_handle = lock_records(&worker_tasks)
                .get_mut(&worker_task_id)
                .and_then(|record| record.handle.take());
            drop(completed_handle);
        });

        match handle {
            Ok(handle) => {
                tasks
                    .get_mut(&task_id)
                    .expect("task was just inserted")
                    .handle = Some(handle);
                Ok(task_id)
            }
            Err(error) => {
                tasks.remove(&task_id);
                log::error!("failed to start space scan task {task_id}: {error}");
                Err(TASK_START_FAILED.into())
            }
        }
    }

    #[cfg(test)]
    fn start_for_test<F>(&self, worker: F) -> String
    where
        F: FnOnce(CancellationToken) -> Result<QuickScanResult, ScanWalkError> + Send + 'static,
    {
        self.start_worker(
            TaskKind::Quick,
            move |token, _| worker(token.clone()).map(TaskResult::Quick),
            |_| {},
        )
        .unwrap()
    }

    #[cfg(test)]
    fn start_deep_for_test<F>(&self, targets: Vec<PathBuf>, worker: F) -> Result<String, String>
    where
        F: FnOnce(
                &[PathBuf],
                &CancellationToken,
                &mut dyn FnMut(&WalkStats),
            ) -> Result<IndexedScanResult, ScanWalkError>
            + Send
            + 'static,
    {
        self.start_deep_worker(
            targets,
            |_| {},
            move |targets, token, progress| worker(targets, token, progress).map(TaskResult::Deep),
        )
    }

    #[cfg(test)]
    fn wait_for_test(&self, task_id: &str) {
        let handle = lock_records(&self.tasks)
            .get_mut(task_id)
            .and_then(|record| record.handle.take());
        if let Some(handle) = handle {
            handle.join().unwrap();
        }
    }

    #[cfg(test)]
    fn wait_until_terminal_for_test(&self, task_id: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = self.status(task_id).unwrap().state;
            if is_terminal(state) {
                return;
            }
            assert!(Instant::now() < deadline, "task {task_id} did not finish");
            thread::yield_now();
        }
    }

    #[cfg(test)]
    fn wait_until_terminal_handles_are_released_for_test(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let released = lock_records(&self.tasks)
                .values()
                .filter(|record| is_terminal(record.progress.state))
                .all(|record| record.handle.is_none());
            if released {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "terminal task handles were not released"
            );
            thread::yield_now();
        }
    }
}

fn prune_cleanup_plans(plans: &mut HashMap<String, StoredCleanupPlan>) {
    if plans.len() <= MAX_RETAINED_CLEANUP_PLANS {
        return;
    }
    let mut oldest = plans
        .values()
        .map(|stored| (stored.plan.plan_id.clone(), stored.plan.created_at.clone()))
        .collect::<Vec<_>>();
    oldest.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    let remove_count = oldest.len().saturating_sub(MAX_RETAINED_CLEANUP_PLANS);
    for (plan_id, _) in oldest.into_iter().take(remove_count) {
        plans.remove(&plan_id);
    }
}

fn prune_terminal_tasks(tasks: &mut HashMap<String, TaskRecord>) {
    let mut completed_deep = tasks
        .iter()
        .filter_map(|(task_id, record)| {
            matches!(&record.result, Some(TaskResult::Deep(_)))
                .then_some((task_id.clone(), record.terminal_order.unwrap_or_default()))
        })
        .collect::<Vec<_>>();
    completed_deep.sort_by_key(|(_, terminal_order)| std::cmp::Reverse(*terminal_order));

    let retained_deep = completed_deep
        .iter()
        .take(MAX_RETAINED_DEEP_RESULTS)
        .map(|(task_id, _)| task_id.clone())
        .collect::<Vec<_>>();
    for (task_id, _) in completed_deep.iter().skip(MAX_RETAINED_DEEP_RESULTS) {
        tasks.remove(task_id);
    }

    let mut lightweight_terminal = tasks
        .iter()
        .filter_map(|(task_id, record)| {
            (is_terminal(record.progress.state) && !retained_deep.contains(task_id))
                .then_some((task_id.clone(), record.terminal_order.unwrap_or_default()))
        })
        .collect::<Vec<_>>();
    lightweight_terminal.sort_by_key(|(_, terminal_order)| std::cmp::Reverse(*terminal_order));

    for (task_id, _) in lightweight_terminal
        .into_iter()
        .skip(MAX_RETAINED_LIGHTWEIGHT_TASKS)
    {
        tasks.remove(&task_id);
    }
}

fn completed_record<'a>(
    tasks: &'a HashMap<String, TaskRecord>,
    task_id: &str,
) -> Result<&'a TaskRecord, String> {
    let record = tasks
        .get(task_id)
        .ok_or_else(|| TASK_NOT_FOUND.to_string())?;
    match record.progress.state {
        ScanTaskState::Completed => Ok(record),
        ScanTaskState::Queued => Err(TASK_QUEUED.into()),
        ScanTaskState::Running => Err(TASK_RUNNING.into()),
        ScanTaskState::Cancelling => Err(TASK_CANCELLING.into()),
        ScanTaskState::Cancelled => Err(TASK_CANCELLED.into()),
        ScanTaskState::Failed => Err(record
            .failure
            .clone()
            .unwrap_or_else(|| "scan task failed".into())),
    }
}

fn completed_deep_result<'a>(
    tasks: &'a HashMap<String, TaskRecord>,
    task_id: &str,
) -> Result<&'a IndexedScanResult, String> {
    match completed_record(tasks, task_id)?.result.as_ref() {
        Some(TaskResult::Deep(result)) => Ok(result),
        _ => Err(TASK_RESULT_UNAVAILABLE.into()),
    }
}

fn normalize_target_set(targets: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut normalized = targets
        .iter()
        .map(|target| normalize_target(target))
        .collect::<Vec<_>>();
    for (index, left) in normalized.iter().enumerate() {
        if normalized
            .iter()
            .skip(index + 1)
            .any(|right| paths_overlap(left, right))
        {
            return Err(OVERLAPPING_TARGETS.into());
        }
    }
    normalized.sort();
    Ok(normalized)
}

fn normalize_target(target: &Path) -> String {
    let mut normalized = target.to_string_lossy().replace('/', "\\").to_lowercase();
    while normalized.len() > 1 && normalized.ends_with('\\') && !normalized.ends_with(":\\") {
        normalized.pop();
    }
    normalized
}

fn paths_overlap(left: &str, right: &str) -> bool {
    is_same_or_ancestor(left, right) || is_same_or_ancestor(right, left)
}

fn is_same_or_ancestor(ancestor: &str, descendant: &str) -> bool {
    if ancestor == descendant {
        return true;
    }
    if ancestor.ends_with('\\') {
        descendant.starts_with(ancestor)
    } else {
        descendant
            .strip_prefix(ancestor)
            .is_some_and(|suffix| suffix.starts_with('\\'))
    }
}

fn lock_records(
    tasks: &Mutex<HashMap<String, TaskRecord>>,
) -> MutexGuard<'_, HashMap<String, TaskRecord>> {
    tasks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn initial_progress(task_id: &str) -> ScanProgress {
    ScanProgress {
        task_id: task_id.into(),
        state: ScanTaskState::Queued,
        scanned_files: 0,
        scanned_directories: 0,
        accounted_bytes: 0,
        skipped_paths: 0,
        elapsed_ms: 0,
        current_path: None,
    }
}

fn apply_walk_stats(progress: &mut ScanProgress, stats: &WalkStats, started_at: Instant) {
    progress.scanned_files = stats.files;
    progress.scanned_directories = stats.directories;
    progress.accounted_bytes = stats.allocated_bytes;
    progress.skipped_paths = stats.skipped;
    progress.elapsed_ms = elapsed_ms(started_at);
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn is_terminal(state: ScanTaskState) -> bool {
    matches!(
        state,
        ScanTaskState::Completed | ScanTaskState::Cancelled | ScanTaskState::Failed
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::model::{KnownSpaceItem, QuickScanResult, ScanTaskState};
    use crate::space_analysis::walker::ScanWalkError;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn task_transitions_from_running_to_completed() {
        let manager = SpaceTaskManager::default();
        let id = manager.start_for_test(|_| Ok(QuickScanResult::default()));
        manager.wait_for_test(&id);
        assert_eq!(manager.status(&id).unwrap().state, ScanTaskState::Completed);
    }

    #[test]
    fn cancel_is_idempotent() {
        let manager = SpaceTaskManager::default();
        let id = manager.start_for_test(|token| {
            while !token.is_cancelled() {
                std::thread::yield_now();
            }
            Err(ScanWalkError::Cancelled)
        });
        manager.cancel(&id).unwrap();
        manager.cancel(&id).unwrap();
        manager.wait_for_test(&id);
        assert_eq!(manager.status(&id).unwrap().state, ScanTaskState::Cancelled);
    }

    #[test]
    fn second_running_quick_scan_returns_existing_task_id() {
        let manager = SpaceTaskManager::default();
        let first = manager.start_for_test(|token| {
            while !token.is_cancelled() {
                std::thread::yield_now();
            }
            Err(ScanWalkError::Cancelled)
        });
        let second = manager.start_for_test(|_| panic!("duplicate worker must not start"));

        assert_eq!(second, first);
        manager.cancel(&first).unwrap();
        manager.wait_for_test(&first);
    }

    #[test]
    fn completed_result_is_retained_with_its_task_id() {
        let manager = SpaceTaskManager::default();
        let id = manager.start_for_test(|_| Ok(QuickScanResult::default()));
        manager.wait_for_test(&id);

        assert_eq!(manager.quick_result(&id).unwrap().task_id, id);
        assert!(manager.quick_result(&id).unwrap().completed);
    }

    #[test]
    fn cleanup_plans_are_server_generated_and_retained() {
        let manager = SpaceTaskManager::default();
        let temp = tempfile::tempdir().unwrap();
        let cache_path = temp.path().join("cache-1");
        std::fs::create_dir(&cache_path).unwrap();
        let worker_path = cache_path.clone();
        let id = manager.start_for_test(move |_| {
            Ok(QuickScanResult {
                items: vec![KnownSpaceItem {
                    id: "cache-1".into(),
                    name_key: "spaceAnalysis.known.cache1".into(),
                    path: worker_path.to_string_lossy().into_owned(),
                    bytes: 42,
                    safety: "safe".into(),
                    cleanup_kind: "contents".into(),
                    ecosystem: None,
                }],
                ..Default::default()
            })
        });
        manager.wait_for_test(&id);

        let first = manager
            .create_cleanup_plan(&id, &["cache-1".into()])
            .unwrap();
        let second = manager
            .create_cleanup_plan(&id, &["cache-1".into()])
            .unwrap();

        assert_ne!(first.plan_id, second.plan_id);
        assert_eq!(first.scan_task_id, id);
        assert_eq!(first.estimated_bytes, 42);
        assert_eq!(
            manager.cleanup_plan(&first.plan_id).unwrap().plan_id,
            first.plan_id
        );
    }

    #[test]
    fn cleanup_plan_rejects_an_incomplete_scan() {
        let manager = SpaceTaskManager::default();
        let id = manager.start_for_test(|token| {
            while !token.is_cancelled() {
                std::thread::yield_now();
            }
            Err(ScanWalkError::Cancelled)
        });

        assert!(manager
            .create_cleanup_plan(&id, &["cache-1".into()])
            .is_err());
        manager.cancel(&id).unwrap();
        manager.wait_for_test(&id);
    }

    #[test]
    fn repeated_quick_completions_are_bounded_and_release_handles() {
        let manager = SpaceTaskManager::default();
        let mut task_ids = Vec::new();

        for _ in 0..MAX_RETAINED_LIGHTWEIGHT_TASKS + 2 {
            let task_id = manager.start_for_test(|_| Ok(QuickScanResult::default()));
            manager.wait_until_terminal_for_test(&task_id);
            task_ids.push(task_id);
        }
        manager.wait_until_terminal_handles_are_released_for_test();

        let tasks = lock_records(&manager.tasks);
        assert_eq!(tasks.len(), MAX_RETAINED_LIGHTWEIGHT_TASKS);
        assert!(tasks.values().all(|record| record.handle.is_none()));
        drop(tasks);

        for task_id in &task_ids[..2] {
            assert_eq!(manager.status(task_id).unwrap_err(), TASK_NOT_FOUND);
        }
        for task_id in &task_ids[2..] {
            assert_eq!(
                manager.status(task_id).unwrap().state,
                ScanTaskState::Completed
            );
            assert_eq!(manager.quick_result(task_id).unwrap().task_id, *task_id);
        }
    }

    #[test]
    fn cancelled_and_failed_task_retention_is_bounded() {
        let manager = SpaceTaskManager::default();
        let mut cancelled_ids = Vec::new();

        for _ in 0..MAX_RETAINED_LIGHTWEIGHT_TASKS + 1 {
            let task_id = manager.start_for_test(|_| Err(ScanWalkError::Cancelled));
            manager.wait_until_terminal_for_test(&task_id);
            cancelled_ids.push(task_id);
        }
        let failed_id = manager.start_for_test(|_| panic!("retention test failure"));
        manager.wait_until_terminal_for_test(&failed_id);
        manager.wait_until_terminal_handles_are_released_for_test();

        assert_eq!(
            manager.status(&cancelled_ids[0]).unwrap_err(),
            TASK_NOT_FOUND
        );
        assert_eq!(
            manager.status(cancelled_ids.last().unwrap()).unwrap().state,
            ScanTaskState::Cancelled
        );
        assert_eq!(
            manager.status(&failed_id).unwrap().state,
            ScanTaskState::Failed
        );
        assert_eq!(
            lock_records(&manager.tasks).len(),
            MAX_RETAINED_LIGHTWEIGHT_TASKS
        );
    }

    #[test]
    fn worker_panic_becomes_a_user_facing_failure() {
        let manager = SpaceTaskManager::default();
        let id = manager.start_for_test(|_| panic!("test worker panic"));
        manager.wait_for_test(&id);

        assert_eq!(manager.status(&id).unwrap().state, ScanTaskState::Failed);
        assert_eq!(manager.quick_result(&id).unwrap_err(), TASK_PANICKED);
    }

    #[test]
    fn cancel_all_cancels_and_joins_active_workers() {
        let manager = SpaceTaskManager::default();
        let id = manager.start_for_test(|token| {
            while !token.is_cancelled() {
                std::thread::yield_now();
            }
            Err(ScanWalkError::Cancelled)
        });

        manager.cancel_all_and_wait(Duration::from_secs(1));

        assert_eq!(manager.status(&id).unwrap().state, ScanTaskState::Cancelled);
    }

    #[test]
    fn worker_emits_running_progress_and_completion_snapshots() {
        let manager = SpaceTaskManager::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let emitted_events = Arc::clone(&events);
        let id = manager
            .start_worker(
                TaskKind::Quick,
                |_, report_progress| {
                    report_progress(&WalkStats {
                        files: 2,
                        logical_bytes: 128,
                        allocated_bytes: 96,
                        ..WalkStats::default()
                    });
                    Ok(TaskResult::Quick(QuickScanResult::default()))
                },
                move |progress| emitted_events.lock().unwrap().push(progress.clone()),
            )
            .unwrap();
        manager.wait_for_test(&id);

        let events = events.lock().unwrap();
        assert_eq!(events.first().unwrap().state, ScanTaskState::Running);
        assert!(events
            .iter()
            .any(|progress| progress.scanned_files == 2 && progress.accounted_bytes == 96));
        assert_eq!(events.last().unwrap().state, ScanTaskState::Completed);
    }

    #[test]
    fn deep_queries_require_a_completed_task() {
        let manager = SpaceTaskManager::default();
        let target = tempfile::tempdir().unwrap();
        let id = manager
            .start_deep_for_test(vec![target.path().to_path_buf()], |_, token, _| {
                while !token.is_cancelled() {
                    std::thread::yield_now();
                }
                Err(ScanWalkError::Cancelled)
            })
            .unwrap();

        while manager.status(&id).unwrap().state == ScanTaskState::Queued {
            std::thread::yield_now();
        }
        assert_eq!(manager.summary(&id).unwrap_err(), TASK_RUNNING);
        manager.cancel(&id).unwrap();
        manager.wait_for_test(&id);
        assert_eq!(manager.summary(&id).unwrap_err(), TASK_CANCELLED);

        let failed_target = tempfile::tempdir().unwrap();
        let failed = manager
            .start_deep_for_test(vec![failed_target.path().to_path_buf()], |_, _, _| {
                panic!("deep worker panic")
            })
            .unwrap();
        manager.wait_for_test(&failed);
        assert_eq!(manager.summary(&failed).unwrap_err(), TASK_PANICKED);
    }

    #[test]
    fn completed_deep_results_are_paged_through_the_manager() {
        let manager = SpaceTaskManager::default();
        let target = tempfile::tempdir().unwrap();
        for index in 0..205 {
            std::fs::create_dir(target.path().join(format!("child-{index:03}"))).unwrap();
        }
        let id = manager
            .start_deep_for_test(
                vec![target.path().to_path_buf()],
                |targets, token, progress| build_indexed_result(targets, token, progress),
            )
            .unwrap();
        manager.wait_for_test(&id);

        let summary = manager.summary(&id).unwrap();
        assert_eq!(summary.task_id, id);
        let root_id = &summary.root_nodes[0].node_id;
        let page = manager.children(&id, root_id, 0, u64::MAX).unwrap();
        assert_eq!(page.limit, 200);
        assert_eq!(page.items.len(), 200);
        assert_eq!(page.total, 205);
        assert_eq!(
            manager.children(&id, "not-a-node", 0, 10).unwrap_err(),
            DIRECTORY_NODE_NOT_FOUND
        );
    }

    #[test]
    fn completed_deep_result_retention_is_bounded() {
        let manager = SpaceTaskManager::default();
        let targets = (0..MAX_RETAINED_DEEP_RESULTS + 2)
            .map(|_| tempfile::tempdir().unwrap())
            .collect::<Vec<_>>();
        let mut task_ids = Vec::new();

        for target in &targets {
            let task_id = manager
                .start_deep_for_test(
                    vec![target.path().to_path_buf()],
                    |targets, token, progress| build_indexed_result(targets, token, progress),
                )
                .unwrap();
            manager.wait_for_test(&task_id);
            task_ids.push(task_id);
        }

        let tasks = lock_records(&manager.tasks);
        let retained_results = tasks
            .values()
            .filter(|record| matches!(&record.result, Some(TaskResult::Deep(_))))
            .count();
        assert_eq!(retained_results, MAX_RETAINED_DEEP_RESULTS);
        assert_eq!(tasks.len(), MAX_RETAINED_DEEP_RESULTS);
        drop(tasks);

        for task_id in &task_ids[..2] {
            assert_eq!(manager.status(task_id).unwrap_err(), TASK_NOT_FOUND);
        }
        for task_id in &task_ids[2..] {
            assert_eq!(
                manager.status(task_id).unwrap().state,
                ScanTaskState::Completed
            );
        }
    }

    #[test]
    fn lightweight_pruning_preserves_active_tasks_and_retained_deep_results() {
        let manager = SpaceTaskManager::default();
        let active_target = tempfile::tempdir().unwrap();
        let active_id = manager
            .start_deep_for_test(vec![active_target.path().to_path_buf()], |_, token, _| {
                while !token.is_cancelled() {
                    std::thread::yield_now();
                }
                Err(ScanWalkError::Cancelled)
            })
            .unwrap();
        while manager.status(&active_id).unwrap().state == ScanTaskState::Queued {
            std::thread::yield_now();
        }

        let retained_target = tempfile::tempdir().unwrap();
        let retained_id = manager
            .start_deep_for_test(
                vec![retained_target.path().to_path_buf()],
                |targets, token, progress| build_indexed_result(targets, token, progress),
            )
            .unwrap();
        manager.wait_for_test(&retained_id);

        for _ in 0..MAX_RETAINED_LIGHTWEIGHT_TASKS + 2 {
            let task_id = manager.start_for_test(|_| Ok(QuickScanResult::default()));
            manager.wait_until_terminal_for_test(&task_id);
        }
        manager.wait_until_terminal_handles_are_released_for_test();

        assert_eq!(
            manager.status(&active_id).unwrap().state,
            ScanTaskState::Running
        );
        assert_eq!(manager.summary(&retained_id).unwrap().task_id, retained_id);
        let tasks = lock_records(&manager.tasks);
        assert!(tasks.contains_key(&active_id));
        assert!(matches!(
            tasks
                .get(&retained_id)
                .and_then(|record| record.result.as_ref()),
            Some(TaskResult::Deep(_))
        ));
        assert_eq!(tasks.len(), MAX_RETAINED_LIGHTWEIGHT_TASKS + 2);
        drop(tasks);

        manager.cancel(&active_id).unwrap();
        manager.wait_for_test(&active_id);
    }

    #[test]
    fn deep_result_eviction_never_removes_active_tasks() {
        let manager = SpaceTaskManager::default();
        let active_target = tempfile::tempdir().unwrap();
        let active_id = manager
            .start_deep_for_test(vec![active_target.path().to_path_buf()], |_, token, _| {
                while !token.is_cancelled() {
                    std::thread::yield_now();
                }
                Err(ScanWalkError::Cancelled)
            })
            .unwrap();
        while manager.status(&active_id).unwrap().state == ScanTaskState::Queued {
            std::thread::yield_now();
        }

        let completed_targets = (0..MAX_RETAINED_DEEP_RESULTS + 1)
            .map(|_| tempfile::tempdir().unwrap())
            .collect::<Vec<_>>();
        for target in &completed_targets {
            let task_id = manager
                .start_deep_for_test(
                    vec![target.path().to_path_buf()],
                    |targets, token, progress| build_indexed_result(targets, token, progress),
                )
                .unwrap();
            manager.wait_for_test(&task_id);
        }

        assert_eq!(
            manager.status(&active_id).unwrap().state,
            ScanTaskState::Running
        );
        let tasks = lock_records(&manager.tasks);
        assert!(tasks.contains_key(&active_id));
        assert_eq!(tasks.len(), MAX_RETAINED_DEEP_RESULTS + 1);
        assert_eq!(
            tasks
                .values()
                .filter(|record| matches!(&record.result, Some(TaskResult::Deep(_))))
                .count(),
            MAX_RETAINED_DEEP_RESULTS
        );
        drop(tasks);

        manager.cancel(&active_id).unwrap();
        manager.wait_for_test(&active_id);
    }

    #[test]
    fn duplicate_and_overlapping_deep_targets_are_rejected() {
        let manager = SpaceTaskManager::default();
        let target = tempfile::tempdir().unwrap();
        let nested = target.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let first = manager
            .start_deep_for_test(vec![target.path().to_path_buf()], |_, token, _| {
                while !token.is_cancelled() {
                    std::thread::yield_now();
                }
                Err(ScanWalkError::Cancelled)
            })
            .unwrap();

        assert_eq!(
            manager
                .start_deep_for_test(
                    vec![target.path().to_path_buf()],
                    |targets, token, progress| { build_indexed_result(targets, token, progress) }
                )
                .unwrap_err(),
            DUPLICATE_DEEP_TASK
        );
        assert_eq!(
            manager
                .start_deep_for_test(
                    vec![target.path().to_path_buf(), nested],
                    |targets, token, progress| build_indexed_result(targets, token, progress),
                )
                .unwrap_err(),
            OVERLAPPING_TARGETS
        );

        manager.cancel(&first).unwrap();
        manager.wait_for_test(&first);
    }

    #[test]
    fn only_two_deep_tasks_may_be_active() {
        let manager = SpaceTaskManager::default();
        let targets = [
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        ];
        let start_blocking = |path: PathBuf| {
            manager.start_deep_for_test(vec![path], |_, token, _| {
                while !token.is_cancelled() {
                    std::thread::yield_now();
                }
                Err(ScanWalkError::Cancelled)
            })
        };
        let first = start_blocking(targets[0].path().to_path_buf()).unwrap();
        let second = start_blocking(targets[1].path().to_path_buf()).unwrap();
        assert_eq!(
            start_blocking(targets[2].path().to_path_buf()).unwrap_err(),
            DEEP_TASK_LIMIT_REACHED
        );

        manager.cancel(&first).unwrap();
        manager.cancel(&second).unwrap();
        manager.wait_for_test(&first);
        manager.wait_for_test(&second);
    }
}
