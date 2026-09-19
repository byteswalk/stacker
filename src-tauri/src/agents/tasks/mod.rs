//! Background agent install/update/uninstall tasks. Each task declares the resources it
//! needs; the scheduler starts every queued task whose resources are free, and each task
//! runs on its own thread with its own cancel flag and log.

pub(crate) mod plan;
pub(crate) mod runner;
pub(crate) mod schedule;

use schedule::{runnable, Resource};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_LOG_LINES: usize = 200;
const MAX_FINISHED: usize = 50;
const EMIT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Cli,
    Desktop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Install,
    Update,
    Uninstall,
    Repair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequest {
    pub product_id: String,
    pub surface: Surface,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub surface: Surface,
    pub surface_label: String,
    pub cli_id: Option<String>,
    pub action: Action,
    pub state: TaskState,
    pub message: Option<String>,
    pub last_line: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// What a request needs before it can be queued.
pub(crate) struct TaskPlan {
    /// Tasks with the same key and surface never run twice at once.
    pub key: String,
    pub product_name: String,
    pub surface_label: String,
    pub cli_id: Option<String>,
    pub resources: Vec<Resource>,
}

pub(crate) trait TaskRunner: Send + Sync + 'static {
    fn plan(&self, request: &TaskRequest) -> Result<TaskPlan, String>;
    fn run(&self, request: &TaskRequest) -> Result<String, String>;
}

type Emit = Arc<dyn Fn(&AgentTask) + Send + Sync>;

struct Record {
    task: AgentTask,
    request: TaskRequest,
    key: String,
    resources: Vec<Resource>,
    cancel: Arc<AtomicBool>,
    log: VecDeque<String>,
    last_emit: Option<Instant>,
}

struct Inner {
    next_id: u64,
    records: Vec<Record>,
}

#[derive(Clone)]
pub struct AgentTaskManager {
    inner: Arc<Mutex<Inner>>,
    runner: Arc<dyn TaskRunner>,
    emit: Emit,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

fn is_open(state: TaskState) -> bool {
    matches!(state, TaskState::Queued | TaskState::Running)
}

impl AgentTaskManager {
    pub(crate) fn new(runner: Arc<dyn TaskRunner>, emit: Emit) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next_id: 1,
                records: Vec::new(),
            })),
            runner,
            emit,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Queues a task, or returns the open task for the same product and surface.
    pub fn start(&self, request: TaskRequest) -> Result<AgentTask, String> {
        let plan = self.runner.plan(&request)?;
        let task = {
            let mut inner = self.lock();
            if let Some(open) = inner.records.iter().find(|record| {
                record.key == plan.key
                    && record.request.surface == request.surface
                    && is_open(record.task.state)
            }) {
                return Ok(open.task.clone());
            }
            let id = format!("agent-task-{}", inner.next_id);
            inner.next_id += 1;
            let task = AgentTask {
                id,
                product_id: request.product_id.clone(),
                product_name: plan.product_name,
                surface: request.surface,
                surface_label: plan.surface_label,
                cli_id: plan.cli_id,
                action: request.action,
                state: TaskState::Queued,
                message: None,
                last_line: None,
                created_at: now(),
                started_at: None,
                finished_at: None,
            };
            inner.records.push(Record {
                task: task.clone(),
                request,
                key: plan.key,
                resources: plan.resources,
                cancel: Arc::new(AtomicBool::new(false)),
                log: VecDeque::new(),
                last_emit: None,
            });
            task
        };
        (self.emit)(&task);
        self.pump();
        Ok(task)
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let queued = {
            let mut inner = self.lock();
            let record = inner
                .records
                .iter_mut()
                .find(|record| record.task.id == id)
                .ok_or("任务不存在")?;
            record.cancel.store(true, Ordering::SeqCst);
            if record.task.state == TaskState::Queued {
                record.task.state = TaskState::Cancelled;
                record.task.message = Some("已取消".into());
                record.task.finished_at = Some(now());
                Some(record.task.clone())
            } else {
                None
            }
        };
        if let Some(task) = queued {
            (self.emit)(&task);
        }
        Ok(())
    }

    pub fn retry(&self, id: &str) -> Result<AgentTask, String> {
        let request = {
            let inner = self.lock();
            let record = inner
                .records
                .iter()
                .find(|record| record.task.id == id)
                .ok_or("任务不存在")?;
            if is_open(record.task.state) {
                return Err("任务仍在执行".into());
            }
            record.request.clone()
        };
        self.start(request)
    }

    pub fn list(&self) -> Vec<AgentTask> {
        self.lock()
            .records
            .iter()
            .map(|record| record.task.clone())
            .collect()
    }

    pub fn log(&self, id: &str) -> Result<Vec<String>, String> {
        let inner = self.lock();
        let record = inner
            .records
            .iter()
            .find(|record| record.task.id == id)
            .ok_or("任务不存在")?;
        Ok(record.log.iter().cloned().collect())
    }

    pub fn running_count(&self) -> usize {
        self.lock()
            .records
            .iter()
            .filter(|record| is_open(record.task.state))
            .count()
    }

    pub fn cancel_all(&self) {
        let ids: Vec<_> = self
            .lock()
            .records
            .iter()
            .filter(|record| is_open(record.task.state))
            .map(|record| record.task.id.clone())
            .collect();
        for id in ids {
            let _ = self.cancel(&id);
        }
    }

    /// Starts every queued task whose resources are free.
    fn pump(&self) {
        let started: Vec<(AgentTask, TaskRequest, Arc<AtomicBool>)> = {
            let mut inner = self.lock();
            let running: Vec<Vec<Resource>> = inner
                .records
                .iter()
                .filter(|record| record.task.state == TaskState::Running)
                .map(|record| record.resources.clone())
                .collect();
            let queued_indexes: Vec<usize> = inner
                .records
                .iter()
                .enumerate()
                .filter(|(_, record)| record.task.state == TaskState::Queued)
                .map(|(index, _)| index)
                .collect();
            let queued: Vec<Vec<Resource>> = queued_indexes
                .iter()
                .map(|&index| inner.records[index].resources.clone())
                .collect();
            runnable(&queued, &running)
                .into_iter()
                .map(|position| {
                    let record = &mut inner.records[queued_indexes[position]];
                    record.task.state = TaskState::Running;
                    record.task.started_at = Some(now());
                    (
                        record.task.clone(),
                        record.request.clone(),
                        record.cancel.clone(),
                    )
                })
                .collect()
        };
        for (task, request, cancel) in started {
            (self.emit)(&task);
            self.spawn(task.id, request, cancel);
        }
    }

    fn spawn(&self, id: String, request: TaskRequest, cancel: Arc<AtomicBool>) {
        let manager = self.clone();
        std::thread::spawn(move || {
            let log_manager = manager.clone();
            let log_id = id.clone();
            let context = crate::installer::TaskContext {
                cancel: cancel.clone(),
                log: Arc::new(move |line: &str| log_manager.append_log(&log_id, line)),
            };
            let runner = manager.runner.clone();
            let outcome = crate::installer::with_task_context(context, || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.run(&request)))
            });
            let (state, message) = match outcome {
                _ if cancel.load(Ordering::SeqCst) => {
                    (TaskState::Cancelled, Some("已取消".to_string()))
                }
                Ok(Ok(message)) => (TaskState::Succeeded, Some(message)),
                Ok(Err(error)) => (TaskState::Failed, Some(error)),
                Err(_) => (TaskState::Failed, Some("内部错误".to_string())),
            };
            manager.finish(&id, state, message);
            manager.pump();
        });
    }

    fn append_log(&self, id: &str, line: &str) {
        let emit = {
            let mut inner = self.lock();
            let Some(record) = inner.records.iter_mut().find(|record| record.task.id == id) else {
                return;
            };
            record.log.push_back(line.to_string());
            while record.log.len() > MAX_LOG_LINES {
                record.log.pop_front();
            }
            record.task.last_line = Some(line.to_string());
            let due = record
                .last_emit
                .map_or(true, |at| at.elapsed() >= EMIT_INTERVAL);
            if due {
                record.last_emit = Some(Instant::now());
            }
            due.then(|| record.task.clone())
        };
        if let Some(task) = emit {
            (self.emit)(&task);
        }
    }

    fn finish(&self, id: &str, state: TaskState, message: Option<String>) {
        let task = {
            let mut inner = self.lock();
            let Some(record) = inner.records.iter_mut().find(|record| record.task.id == id) else {
                return;
            };
            record.task.state = state;
            record.task.message = message;
            record.task.finished_at = Some(now());
            let task = record.task.clone();
            let finished = inner
                .records
                .iter()
                .filter(|record| !is_open(record.task.state))
                .count();
            let mut excess = finished.saturating_sub(MAX_FINISHED);
            inner.records.retain(|record| {
                if excess > 0 && !is_open(record.task.state) {
                    excess -= 1;
                    false
                } else {
                    true
                }
            });
            task
        };
        (self.emit)(&task);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct FakeRunner {
        gate: Mutex<mpsc::Receiver<Result<String, String>>>,
    }

    impl TaskRunner for FakeRunner {
        fn plan(&self, request: &TaskRequest) -> Result<TaskPlan, String> {
            Ok(TaskPlan {
                key: request.product_id.clone(),
                product_name: request.product_id.clone(),
                surface_label: "CLI".into(),
                cli_id: None,
                resources: vec![Resource::Product(request.product_id.clone()), Resource::Npm],
            })
        }

        fn run(&self, _request: &TaskRequest) -> Result<String, String> {
            crate::installer::task_log("working");
            loop {
                if crate::installer::op_cancelled() {
                    return Err("已取消操作".into());
                }
                if let Ok(result) = self
                    .gate
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_millis(10))
                {
                    return result;
                }
            }
        }
    }

    fn manager() -> (AgentTaskManager, mpsc::Sender<Result<String, String>>) {
        let (sender, receiver) = mpsc::channel();
        let runner = Arc::new(FakeRunner {
            gate: Mutex::new(receiver),
        });
        (AgentTaskManager::new(runner, Arc::new(|_| {})), sender)
    }

    fn request(id: &str) -> TaskRequest {
        TaskRequest {
            product_id: id.into(),
            surface: Surface::Cli,
            action: Action::Update,
        }
    }

    fn wait_for(manager: &AgentTaskManager, id: &str, state: TaskState) -> AgentTask {
        for _ in 0..500 {
            if let Some(task) = manager
                .list()
                .into_iter()
                .find(|task| task.id == id && task.state == state)
            {
                return task;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("task {id} never reached {state:?}");
    }

    fn state_of(manager: &AgentTaskManager, id: &str) -> TaskState {
        manager
            .list()
            .into_iter()
            .find(|task| task.id == id)
            .unwrap()
            .state
    }

    #[test]
    fn conflicting_tasks_queue_and_finish_in_order() {
        let (manager, gate) = manager();
        let first = manager.start(request("a")).unwrap();
        let second = manager.start(request("b")).unwrap();
        wait_for(&manager, &first.id, TaskState::Running);
        assert_eq!(state_of(&manager, &second.id), TaskState::Queued);
        gate.send(Ok("done".into())).unwrap();
        wait_for(&manager, &first.id, TaskState::Succeeded);
        wait_for(&manager, &second.id, TaskState::Running);
        gate.send(Err("boom".into())).unwrap();
        let failed = wait_for(&manager, &second.id, TaskState::Failed);
        assert_eq!(failed.message.as_deref(), Some("boom"));
    }

    #[test]
    fn cancelling_one_task_leaves_the_queue_alone() {
        let (manager, _gate) = manager();
        let first = manager.start(request("a")).unwrap();
        let second = manager.start(request("b")).unwrap();
        let third = manager.start(request("c")).unwrap();
        wait_for(&manager, &first.id, TaskState::Running);
        manager.cancel(&second.id).unwrap();
        wait_for(&manager, &second.id, TaskState::Cancelled);
        assert_eq!(state_of(&manager, &third.id), TaskState::Queued);
        manager.cancel(&first.id).unwrap();
        wait_for(&manager, &first.id, TaskState::Cancelled);
        wait_for(&manager, &third.id, TaskState::Running);
        manager.cancel_all();
        wait_for(&manager, &third.id, TaskState::Cancelled);
    }

    #[test]
    fn duplicate_start_returns_the_open_task_and_logs_are_kept() {
        let (manager, gate) = manager();
        let first = manager.start(request("a")).unwrap();
        let again = manager.start(request("a")).unwrap();
        assert_eq!(first.id, again.id);
        wait_for(&manager, &first.id, TaskState::Running);
        gate.send(Ok("done".into())).unwrap();
        wait_for(&manager, &first.id, TaskState::Succeeded);
        assert_eq!(manager.log(&first.id).unwrap(), vec!["working".to_string()]);
        let retried = manager.retry(&first.id).unwrap();
        assert_ne!(retried.id, first.id);
        gate.send(Ok("done".into())).unwrap();
        wait_for(&manager, &retried.id, TaskState::Succeeded);
        assert_eq!(manager.running_count(), 0);
    }
}
