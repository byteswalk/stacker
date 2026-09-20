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

/// 每条结果先存库，再给 skill 草稿写文件夹：这样磁盘上永远不会出现一个没有对应
/// 结果行指着它的 skill 文件夹（文件夹没写成功没关系，正文已经存住了，见下）。
/// 每条落地后立刻把已保存数与文件夹列表写回任务状态，这样中途失败时，已经落地
/// 的部分不会凭空消失、被汇报成一次什么都没保存的失败。
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
        let mut result = DistillResult {
            id: format!("{}-{}-{}", item.kind, at, i + 1),
            kind: item.kind.clone(),
            title: item.title.clone(),
            body: item.body.clone(),
            sources: item.sources.clone(),
            state: "draft".into(),
            by: by.clone(),
            folder: String::new(),
            created_at: at,
            updated_at: at,
        };
        // 先落一行没有文件夹的记录，再尝试写文件夹；文件夹写成功了再补一次把文件夹名存回去。
        store::insert(&conn, &result)?;
        if item.kind == "skill" {
            let name = skills::write_draft(&skills_dir, item, &evidence)?;
            result.folder = name.clone();
            store::insert(&conn, &result)?;
            folders.push(name);
        }
        saved += 1;
        update(|j| {
            j.saved = saved;
            j.folders = folders.clone();
        });
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

        // 6) 第二条在写 skill 文件夹时失败：已经落地的第一条不能凭空消失。
        // 把 unique_dir 的全部 99 个候选名占满，"Second" 这个 skill 就注定写不出文件夹。
        let skills_dir = dir.path().join("distill").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        for n in 1..100 {
            let name = if n == 1 {
                "Second".to_string()
            } else {
                format!("Second ({n})")
            };
            std::fs::create_dir_all(skills_dir.join(name)).unwrap();
        }
        let two_skills: Runner = Arc::new(|req: &RunRequest, _: &CancelFlag| {
            if req.prompt.contains("@merge") {
                Ok(RunOutput {
                    text: "no duplicates".into(),
                })
            } else {
                Ok(RunOutput {
                    text: "### [SKILL] First\nDo the first thing.\n\
                           ### [SKILL] Second\nDo the second thing.\n"
                        .into(),
                })
            }
        });
        let mut skills_only = request(dir.path());
        skills_only.kinds = vec!["skill".into()];
        start(skills_only, two_skills).unwrap();
        let partial = wait_for(|j| j.state != "running");
        assert_eq!(partial.state, "failed");
        assert_eq!(partial.error, "E_STORAGE");
        assert_eq!(
            partial.saved, 1,
            "the first item must still be reported as saved"
        );
        assert_eq!(partial.folders, vec!["First".to_string()]);
    }

    /// Live run: take the first web conversation with a body in the dev data dir (or a local
    /// session if there is none), distil it for real with the logged-in agent, and check it
    /// left no session behind.
    /// `cargo test --manifest-path src-tauri/Cargo.toml --lib live_distill -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_distill() {
        let root = crate::webchat::root();
        let conn = crate::webchat::store::open(&root).unwrap();
        let chats = crate::webchat::store::list(&conn, &root, &Default::default()).unwrap();
        let chat = chats
            .items
            .iter()
            .find(|c| c.body_fetched_at.is_some() && c.body_messages >= 4);
        let catalog = crate::sessions::commands::annotated_catalog().unwrap().0;
        let before = catalog.len();
        let (refs, sessions) = match chat {
            Some(c) => (
                vec![SourceRef {
                    kind: "web".into(),
                    key: c.key.clone(),
                }],
                Vec::new(),
            ),
            None => {
                let session = catalog
                    .iter()
                    .filter(|s| !crate::sessions::catalog::is_automation(s))
                    .find(|s| (200_000..5_000_000).contains(&s.bytes))
                    .expect("need a web conversation with a body or a medium-sized local session")
                    .clone();
                (
                    vec![SourceRef {
                        kind: "session".into(),
                        key: session.id.clone(),
                    }],
                    vec![session],
                )
            }
        };
        let request = StartRequest {
            root: root.clone(),
            sessions,
            refs,
            kinds: vec!["qa".into(), "requirement".into(), "skill".into()],
            choice: crate::webchat::commands::runner_for(&crate::sessions::summary::load_settings(
                &crate::sessions::annotations::connect().unwrap(),
            )),
            locale: "zh-CN".into(),
        };
        let started = std::time::Instant::now();
        start(request, crate::sessions::summary_job::live_runner()).unwrap();
        let done = wait_for_live(|j| j.state != "running");
        println!(
            "== {} stage={} {}/{} saved={} folders={:?} elapsed={:?} error={}",
            done.by,
            done.stage,
            done.done,
            done.total,
            done.saved,
            done.folders,
            started.elapsed(),
            done.error
        );
        assert_eq!(done.state, "completed", "{}", done.error);
        assert!(done.saved > 0, "should distil at least one item");
        for folder in &done.folders {
            let dir = crate::distill::skills_in(&root).join(folder);
            println!("-- {}", dir.display());
            assert!(dir.join("SKILL.md").is_file() && dir.join("excerpts.md").is_file());
        }
        crate::sessions::catalog::invalidate();
        let after = crate::sessions::commands::annotated_catalog()
            .unwrap()
            .0
            .len();
        assert_eq!(
            after, before,
            "distilling must not leave a new session behind"
        );
    }

    /// A live run may take a few minutes, so it gets its own, longer wait.
    fn wait_for_live(check: impl Fn(&DistillJob) -> bool) -> DistillJob {
        for _ in 0..3600 {
            if let Some(j) = job() {
                if check(&j) {
                    return j;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        panic!(
            "live distillation did not finish within 30 minutes: {:?}",
            job()
        );
    }
}
