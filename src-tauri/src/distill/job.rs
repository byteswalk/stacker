//! 提炼任务：同一时间只跑一个，有进度、可取消。
//! 数据目录与会话列表都由调用方传进来，任务本身不去猜路径。
use super::pipeline::{self, DraftItem, SourceText};
use super::skills;
use super::sources::{self, SourceRef};
use super::store::{self, DistillResult};
use crate::runner::CancelFlag;
use crate::sessions::model::Session;
use crate::sessions::summary::RunnerChoice;
use crate::sessions::summary_job::Runner;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistillJob {
    pub id: String,
    /// running | completed | empty | failed | cancelled
    ///
    /// `empty` 是完成了但一条可用条目都没提炼出来的情况：不能让它看起来和 `completed` 一样，
    /// 界面据此显示「没有提炼出可用内容」而不是一个空的成功。
    pub state: String,
    /// reading | distilling | merging | saving
    pub stage: String,
    pub done: usize,
    pub total: usize,
    /// 已保存的条目数。
    pub saved: usize,
    /// 本次写出的 skill 草稿文件夹名。
    pub folders: Vec<String>,
    pub error: String,
    /// 执行者标签，例如 "claude / sonnet / low"。
    pub by: String,
}

pub struct StartRequest {
    pub root: PathBuf,
    /// 已经读好的本机会话；只用来解析 `session:` 来源。
    pub sessions: Vec<Session>,
    pub refs: Vec<SourceRef>,
    pub kinds: Vec<String>,
    pub choice: RunnerChoice,
    pub locale: String,
}

static JOB: Mutex<Option<DistillJob>> = Mutex::new(None);
static CANCEL: Mutex<Option<CancelFlag>> = Mutex::new(None);

pub fn job() -> Option<DistillJob> {
    JOB.lock().ok().and_then(|j| j.clone())
}

pub fn cancel() {
    if let Ok(flag) = CANCEL.lock() {
        if let Some(flag) = flag.as_ref() {
            flag.cancel();
        }
    }
}

fn update(f: impl FnOnce(&mut DistillJob)) {
    if let Ok(mut slot) = JOB.lock() {
        if let Some(job) = slot.as_mut() {
            f(job);
        }
    }
}

pub fn start(request: StartRequest, run: Runner) -> Result<DistillJob, String> {
    if request.refs.is_empty()
        || request.kinds.is_empty()
        || request.kinds.iter().any(|k| !super::is_kind(k))
    {
        return Err("E_REQUEST".into());
    }
    if job().is_some_and(|j| j.state == "running") {
        return Err("E_DISTILL_BUSY".into());
    }
    let flag = CancelFlag::default();
    *CANCEL.lock().map_err(crate::sessions::err)? = Some(flag.clone());
    let started = DistillJob {
        id: format!("distill-{}", crate::webchat::now_ms()),
        state: "running".into(),
        stage: "reading".into(),
        by: request.choice.label(),
        ..Default::default()
    };
    *JOB.lock().map_err(crate::sessions::err)? = Some(started.clone());
    std::thread::spawn(move || {
        let outcome = run_job(&request, &flag, &run);
        let cancelled = flag.is_cancelled();
        update(|j| {
            match outcome {
                Ok((saved, folders)) => {
                    j.saved = saved;
                    j.folders = folders;
                }
                Err(code) => j.error = code,
            }
            j.state = if cancelled {
                "cancelled"
            } else if !j.error.is_empty() {
                "failed"
            } else if j.saved == 0 {
                // 跑完了、没出错，但一条可用条目都没有：不能算作 completed。
                "empty"
            } else {
                "completed"
            }
            .into();
        });
    });
    Ok(started)
}

fn run_job(
    request: &StartRequest,
    cancel: &CancelFlag,
    run: &Runner,
) -> Result<(usize, Vec<String>), String> {
    let units = sources::gather(&request.root, &request.sessions, &request.refs)?;
    let progress = |done: usize, total: usize, stage: &str| {
        update(|j| {
            j.done = done;
            j.total = total;
            j.stage = stage.to_string();
        });
    };
    let items = pipeline::distil(
        &units,
        &request.kinds,
        &request.choice,
        &request.locale,
        cancel,
        run.as_ref(),
        &progress,
    )?;
    save(request, &units, &items)
}

/// 每条结果存库；skill 草稿先写文件夹，文件夹名记在结果行上。
fn save(
    request: &StartRequest,
    units: &[SourceText],
    items: &[DraftItem],
) -> Result<(usize, Vec<String>), String> {
    let conn = crate::webchat::store::open(&request.root)?;
    let skills_dir = super::skills_in(&request.root);
    let evidence: Vec<(String, String)> = units
        .iter()
        .flat_map(|u| {
            u.sources
                .iter()
                .map(|s| (s.key.clone(), u.markdown.clone()))
        })
        .collect();
    let at = crate::webchat::now_ms();
    let by = request.choice.label();
    let mut folders: Vec<String> = Vec::new();
    let mut saved = 0;
    for (i, item) in items.iter().enumerate() {
        let folder = if item.kind == "skill" {
            let name = skills::write_draft(&skills_dir, item, &evidence)?;
            folders.push(name.clone());
            name
        } else {
            String::new()
        };
        store::insert(
            &conn,
            &DistillResult {
                id: format!("{}-{}-{}", item.kind, at, i + 1),
                kind: item.kind.clone(),
                title: item.title.clone(),
                body: item.body.clone(),
                sources: item.sources.clone(),
                state: "draft".into(),
                by: by.clone(),
                folder,
                created_at: at,
                updated_at: at,
            },
        )?;
        saved += 1;
    }
    Ok((saved, folders))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RunOutput, RunRequest};
    use crate::sessions::model::Agent;
    use crate::sessions::summary::{choose, SummarySettings};
    use crate::webchat::protocol::{BodyChunk, WebConversation, WebMessage};
    use crate::webchat::store as web;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn seed(root: &Path) {
        let conn = web::open(root).unwrap();
        web::upsert_conversations(
            &conn,
            &[WebConversation {
                key: "chatgpt:a".into(),
                site: "chatgpt".into(),
                account: "chatgpt:u1".into(),
                id: "a".into(),
                title: "Trip plan".into(),
                updated_at: 5,
                listed_at: 1,
                ..Default::default()
            }],
        )
        .unwrap();
        web::put_body_chunk(
            &conn,
            root,
            &BodyChunk {
                key: "chatgpt:a".into(),
                site: "chatgpt".into(),
                account: "chatgpt:u1".into(),
                id: "a".into(),
                title: "Trip plan".into(),
                updated_at: 5,
                fetched_at: 6,
                chunk: 0,
                chunks: 1,
                messages: vec![WebMessage {
                    role: "user".into(),
                    text: "Where should we go?".into(),
                    at: None,
                    attachments: vec![],
                }],
            },
        )
        .unwrap();
    }

    fn request(root: &Path) -> StartRequest {
        StartRequest {
            root: root.to_path_buf(),
            sessions: Vec::new(),
            refs: vec![SourceRef {
                kind: "web".into(),
                key: "chatgpt:a".into(),
            }],
            kinds: vec!["qa".into(), "skill".into()],
            choice: choose(&SummarySettings::default(), Agent::Claude),
            locale: "en".into(),
        }
    }

    /// 轮询到条件成立，最多 20 秒。
    fn wait_for(check: impl Fn(&DistillJob) -> bool) -> DistillJob {
        for _ in 0..2000 {
            if let Some(j) = job() {
                if check(&j) {
                    return j;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!(
            "job did not reach the expected state within 20s: {:?}",
            job()
        );
    }

    #[test]
    fn one_job_at_a_time_saves_results_and_can_be_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());

        // 1) 卡住执行器，确认任务在跑、第二次 start 被拒。
        let gate = Arc::new(AtomicBool::new(false));
        let waiting = gate.clone();
        let slow: Runner = Arc::new(move |_: &RunRequest, c: &CancelFlag| {
            while !waiting.load(Ordering::SeqCst) && !c.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(RunOutput {
                text: "### [QA] Where to go\nKyoto.\n### [SKILL] Plan a trip\nAsk, then book.\n"
                    .into(),
            })
        });
        assert_eq!(
            start(request(dir.path()), slow.clone()).unwrap().state,
            "running"
        );
        assert_eq!(
            start(request(dir.path()), slow.clone()).unwrap_err(),
            "E_DISTILL_BUSY"
        );

        // 2) 放行，等它完成，确认结果与 skill 文件夹。
        gate.store(true, Ordering::SeqCst);
        let done = wait_for(|j| j.state != "running");
        assert_eq!(done.state, "completed", "{}", done.error);
        assert_eq!(done.saved, 2);
        assert_eq!(done.folders, vec!["Plan a trip".to_string()]);
        assert!(done.total >= 1 && done.done == done.total);
        assert_eq!(done.by, "claude / sonnet / low");
        let conn = web::open(dir.path()).unwrap();
        let saved = crate::distill::store::list(&conn, &Default::default()).unwrap();
        assert_eq!(saved.len(), 2);
        assert!(saved.iter().all(|r| r.state == "draft"));
        assert!(saved.iter().all(|r| r.sources[0].key == "web:chatgpt:a"));
        let skill = saved.iter().find(|r| r.kind == "skill").unwrap();
        assert_eq!(skill.folder, "Plan a trip");
        assert!(dir
            .path()
            .join("distill")
            .join("skills")
            .join("Plan a trip")
            .join("SKILL.md")
            .is_file());

        // 3) 再跑一次并立刻取消。
        let blocked = Arc::new(AtomicBool::new(false));
        let waiting = blocked.clone();
        let stuck: Runner = Arc::new(move |_: &RunRequest, c: &CancelFlag| {
            while !waiting.load(Ordering::SeqCst) && !c.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err("E_CANCELLED".to_string())
        });
        start(request(dir.path()), stuck).unwrap();
        cancel();
        let stopped = wait_for(|j| j.state != "running");
        assert_eq!(stopped.state, "cancelled");

        // 4) 参数校验。
        let mut empty = request(dir.path());
        empty.kinds.clear();
        assert_eq!(start(empty, slow.clone()).unwrap_err(), "E_REQUEST");
        let mut unknown = request(dir.path());
        unknown.kinds = vec!["poem".into()];
        assert_eq!(start(unknown, slow.clone()).unwrap_err(), "E_REQUEST");

        // 5) 模型没有提炼出任何可用条目：任务跑完了、没出错，但不能看起来像一次成功。
        let nothing: Runner = Arc::new(|_: &RunRequest, _: &CancelFlag| {
            Ok(RunOutput {
                text: "Sorry, nothing worth keeping here.".into(),
            })
        });
        start(request(dir.path()), nothing).unwrap();
        let empty_result = wait_for(|j| j.state != "running");
        assert_eq!(empty_result.state, "empty");
        assert_eq!(empty_result.saved, 0);
        assert!(empty_result.folders.is_empty());
        assert!(empty_result.error.is_empty());
    }
}
