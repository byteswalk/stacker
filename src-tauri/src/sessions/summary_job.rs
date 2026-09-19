//! Background summary jobs: two sessions at a time, cancellable, optionally ending in a handoff.
use super::model::Session;
use super::summary::{self, RunnerChoice, SummarySettings};
use crate::runner::{CancelFlag, RunOutput, RunRequest};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const WORKERS: usize = 2;

pub type Runner = Arc<dyn Fn(&RunRequest, &CancelFlag) -> Result<RunOutput, String> + Send + Sync>;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobItem {
    pub id: String,
    pub title: String,
    /// queued | running | completed | failed | skipped
    pub status: String,
    pub detail: String,
    pub elapsed_ms: u64,
    pub by: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryJob {
    pub id: String,
    /// "summary" | "handoff"
    pub kind: String,
    /// running | completed | failed | cancelled
    pub state: String,
    pub done: usize,
    pub total: usize,
    pub items: Vec<JobItem>,
    pub error: String,
    pub result_path: String,
    pub result_text: String,
}

/// Runs after every summary finished; returns (path, text) of the produced document.
pub type Finish = Box<dyn FnOnce(&CancelFlag) -> Result<(String, String), String> + Send>;

static JOB: Mutex<Option<SummaryJob>> = Mutex::new(None);
static CANCEL: Mutex<Option<CancelFlag>> = Mutex::new(None);

pub fn job() -> Option<SummaryJob> {
    current()
}

fn current() -> Option<SummaryJob> {
    JOB.lock().ok().and_then(|j| j.clone())
}

pub fn cancel() {
    if let Ok(flag) = CANCEL.lock() {
        if let Some(flag) = flag.as_ref() {
            flag.cancel();
        }
    }
}

fn update(f: impl FnOnce(&mut SummaryJob)) {
    if let Ok(mut slot) = JOB.lock() {
        if let Some(job) = slot.as_mut() {
            f(job);
        }
    }
}

pub fn live_runner() -> Runner {
    Arc::new(|req: &RunRequest, cancel: &CancelFlag| crate::runner::run(req, cancel))
}

fn summarize_one(
    session: &Session,
    choice: &RunnerChoice,
    locale: &str,
    cancel: &CancelFlag,
    run: &Runner,
) -> Result<(), String> {
    let markdown = summary::transcript_markdown(session)?;
    let fingerprint = super::annotations::quick_fingerprint(std::path::Path::new(&session.path));
    let text = summary::summarize_text(&markdown, choice, locale, cancel, run.as_ref())?;
    let conn = super::annotations::connect()?;
    super::annotations::save_summary(
        &conn,
        &session.id,
        &text,
        &fingerprint,
        &choice.label(),
        super::now(),
    )?;
    super::catalog::invalidate();
    Ok(())
}

/// Needs a (new) summary: none yet, stale, or regeneration requested.
pub fn needs_summary(session: &Session, regenerate: bool) -> bool {
    regenerate || session.summary.is_none() || session.summary_stale
}

pub fn start(
    kind: &str,
    sessions: Vec<Session>,
    regenerate: bool,
    settings: SummarySettings,
    locale: String,
    run: Runner,
    finish: Option<Finish>,
) -> Result<SummaryJob, String> {
    if job().is_some_and(|j| j.state == "running") {
        return Err("E_BUSY".into());
    }
    let flag = CancelFlag::default();
    *CANCEL.lock().map_err(super::err)? = Some(flag.clone());
    let items: Vec<JobItem> = sessions
        .iter()
        .map(|s| JobItem {
            id: s.id.clone(),
            title: s.title.clone(),
            status: if needs_summary(s, regenerate) {
                "queued"
            } else {
                "skipped"
            }
            .into(),
            by: s.summary_by.clone(),
            ..Default::default()
        })
        .collect();
    let job = SummaryJob {
        id: format!("{kind}-{}", super::now()),
        kind: kind.into(),
        state: "running".into(),
        done: items.iter().filter(|i| i.status == "skipped").count(),
        total: items.len(),
        items,
        ..Default::default()
    };
    *JOB.lock().map_err(super::err)? = Some(job.clone());
    let queue: Arc<Mutex<VecDeque<Session>>> = Arc::new(Mutex::new(
        sessions
            .into_iter()
            .filter(|s| needs_summary(s, regenerate))
            .collect(),
    ));
    std::thread::spawn(move || {
        let workers: Vec<_> = (0..WORKERS)
            .map(|_| {
                let queue = queue.clone();
                let run = run.clone();
                let flag = flag.clone();
                let settings = settings.clone();
                let locale = locale.clone();
                std::thread::spawn(move || loop {
                    if flag.is_cancelled() {
                        break;
                    }
                    let Some(session) = queue.lock().ok().and_then(|mut q| q.pop_front()) else {
                        break;
                    };
                    let choice = summary::choose(&settings, session.agent);
                    update(|j| {
                        if let Some(i) = j.items.iter_mut().find(|i| i.id == session.id) {
                            i.status = "running".into();
                        }
                    });
                    let started = Instant::now();
                    let result = summarize_one(&session, &choice, &locale, &flag, &run);
                    update(|j| {
                        if let Some(i) = j.items.iter_mut().find(|i| i.id == session.id) {
                            i.elapsed_ms = started.elapsed().as_millis() as u64;
                            match &result {
                                Ok(()) => {
                                    i.status = "completed".into();
                                    i.by = choice.label();
                                }
                                Err(code) => {
                                    i.status = "failed".into();
                                    i.detail = code.clone();
                                }
                            }
                        }
                        j.done += 1;
                    });
                })
            })
            .collect();
        for w in workers {
            let _ = w.join();
        }
        let cancelled = flag.is_cancelled();
        let failed = current().is_some_and(|j| j.items.iter().any(|i| i.status == "failed"));
        let mut outcome: Result<(String, String), String> = Ok(Default::default());
        if let Some(finish) = finish {
            outcome = if cancelled {
                Err("E_CANCELLED".into())
            } else {
                finish(&flag)
            };
        }
        update(|j| {
            match outcome {
                Ok((path, text)) => {
                    j.result_path = path;
                    j.result_text = text;
                }
                Err(code) => j.error = code,
            }
            for i in j.items.iter_mut().filter(|i| i.status == "queued") {
                i.status = "skipped".into();
                i.detail = "E_CANCELLED".into();
            }
            j.state = if cancelled {
                "cancelled"
            } else if failed || !j.error.is_empty() {
                "failed"
            } else {
                "completed"
            }
            .into();
        });
    });
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_summaries_are_skipped_unless_regenerating() {
        let mut s = super::super::catalog::tests_support::session("codex:a");
        assert!(needs_summary(&s, false));
        s.summary = Some("x".into());
        assert!(!needs_summary(&s, false));
        assert!(needs_summary(&s, true));
        s.summary_stale = true;
        assert!(needs_summary(&s, false));
    }
}
