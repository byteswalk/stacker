use super::known::scan_known_candidates;
use super::model::{QuickScanResult, ScanProgress, ScanTaskState};
use super::walker::{CancellationToken, ScanWalkError, WalkStats};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TASK_NOT_FOUND: &str = "未找到扫描任务";
const TASK_START_FAILED: &str = "无法启动扫描任务";
const TASK_PANICKED: &str = "扫描任务异常终止";
const TASK_NOT_COMPLETE: &str = "扫描任务尚未完成";
const TASK_CANCELLED: &str = "扫描任务已取消";
const TASK_RESULT_UNAVAILABLE: &str = "扫描结果不可用";

struct TaskRecord {
    token: CancellationToken,
    progress: ScanProgress,
    result: Option<QuickScanResult>,
    failure: Option<String>,
    handle: Option<JoinHandle<()>>,
}

type TaskRecords = Arc<Mutex<HashMap<String, TaskRecord>>>;

pub struct SpaceTaskManager {
    next_id: AtomicU64,
    tasks: TaskRecords,
}

impl Default for SpaceTaskManager {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SpaceTaskManager {
    pub fn start_quick(&self, window: tauri::Window) -> Result<String, String> {
        use tauri::Emitter;

        self.start_worker(
            move |token, report_progress| {
                scan_known_candidates(token, |stats| report_progress(stats))
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
        let record = tasks
            .get(task_id)
            .ok_or_else(|| TASK_NOT_FOUND.to_string())?;
        match record.progress.state {
            ScanTaskState::Completed => record
                .result
                .as_ref()
                .map(clone_quick_result)
                .ok_or_else(|| TASK_RESULT_UNAVAILABLE.into()),
            ScanTaskState::Failed => Err(record
                .failure
                .clone()
                .unwrap_or_else(|| TASK_RESULT_UNAVAILABLE.into())),
            ScanTaskState::Cancelled | ScanTaskState::Cancelling => Err(TASK_CANCELLED.into()),
            ScanTaskState::Queued | ScanTaskState::Running => Err(TASK_NOT_COMPLETE.into()),
        }
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

    fn start_worker<F, E>(&self, worker: F, emit: E) -> Result<String, String>
    where
        F: FnOnce(
                &CancellationToken,
                &mut dyn FnMut(&WalkStats),
            ) -> Result<QuickScanResult, ScanWalkError>
            + Send
            + 'static,
        E: Fn(&ScanProgress) + Send + Sync + 'static,
    {
        let mut tasks = lock_records(&self.tasks);
        if let Some((task_id, _)) = tasks
            .iter()
            .find(|(_, record)| !is_terminal(record.progress.state))
        {
            return Ok(task_id.clone());
        }

        let task_id = format!("scan-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let token = CancellationToken::default();
        tasks.insert(
            task_id.clone(),
            TaskRecord {
                token: token.clone(),
                progress: initial_progress(&task_id),
                result: None,
                failure: None,
                handle: None,
            },
        );

        let worker_task_id = task_id.clone();
        let worker_tasks = Arc::clone(&self.tasks);
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
                let Some(record) = tasks.get_mut(&worker_task_id) else {
                    return;
                };
                record.progress.elapsed_ms = elapsed_ms(started_at);
                match outcome {
                    Ok(Ok(mut result))
                        if !token.is_cancelled()
                            && record.progress.state != ScanTaskState::Cancelling =>
                    {
                        result.task_id = worker_task_id.clone();
                        result.completed = true;
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
                record.progress.clone()
            };
            emit(&final_progress);
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
        self.start_worker(move |token, _| worker(token.clone()), |_| {})
            .unwrap()
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
    progress.accounted_bytes = stats.logical_bytes;
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

fn clone_quick_result(result: &QuickScanResult) -> QuickScanResult {
    QuickScanResult {
        task_id: result.task_id.clone(),
        completed: result.completed,
        total_bytes: result.total_bytes,
        safely_releasable_bytes: result.safely_releasable_bytes,
        items: result.items.clone(),
        errors: result.errors.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::model::{QuickScanResult, ScanTaskState};
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
                |_, report_progress| {
                    report_progress(&WalkStats {
                        files: 2,
                        logical_bytes: 128,
                        ..WalkStats::default()
                    });
                    Ok(QuickScanResult::default())
                },
                move |progress| emitted_events.lock().unwrap().push(progress.clone()),
            )
            .unwrap();
        manager.wait_for_test(&id);

        let events = events.lock().unwrap();
        assert_eq!(events.first().unwrap().state, ScanTaskState::Running);
        assert!(events
            .iter()
            .any(|progress| progress.scanned_files == 2 && progress.accounted_bytes == 128));
        assert_eq!(events.last().unwrap().state, ScanTaskState::Completed);
    }
}
