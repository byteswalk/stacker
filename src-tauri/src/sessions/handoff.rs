//! Project handoff notes composed from session summaries.
use super::catalog::is_automation;
use super::model::{Agent, Session};
use super::summary::{self, RunnerChoice, SummarySettings};
use crate::runner::{CancelFlag, RunRequest, DEFAULT_TIMEOUT};
use std::path::PathBuf;

/// The project's ordinary sessions, most recent first; `limit` 0 means all.
pub fn select(sessions: &[Session], project: &str, limit: usize) -> Vec<Session> {
    let mut list: Vec<Session> = sessions
        .iter()
        .filter(|s| s.project.key == project && !is_automation(s))
        .cloned()
        .collect();
    list.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    if limit > 0 {
        list.truncate(limit);
    }
    list
}

/// With "same", the agent that owns most of the chosen sessions writes the handoff.
pub fn choose(settings: &SummarySettings, sessions: &[Session]) -> RunnerChoice {
    let codex = sessions.iter().filter(|s| s.agent == Agent::Codex).count();
    let majority = if codex * 2 >= sessions.len() && codex > 0 {
        Agent::Codex
    } else {
        Agent::Claude
    };
    summary::choose(settings, majority)
}

fn date(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default()
}

pub fn prompt(project_name: &str, sessions: &[Session], locale: &str) -> String {
    let zh = locale.starts_with("zh");
    let guard = summary::prompts(locale).guard;
    let task = if zh {
        format!("根据下面各会话的摘要，为项目「{project_name}」写一份交接资料，让接手的人或智能体能直接继续工作。严格使用以下结构：\n# {project_name} 交接资料\n## 项目现状\n## 关键决定\n## 进行中的工作\n## 待办\n## 注意事项\n## 会话索引\n会话索引按时间从新到旧，每行：日期、智能体、标题、一句话说明。")
    } else {
        format!("From the session summaries below, write a handoff for project \"{project_name}\" so a person or agent can continue the work. Use exactly this structure:\n# {project_name} handoff\n## Current state\n## Key decisions\n## Work in progress\n## To do\n## Caveats\n## Session index\nThe index lists newest first, one line each: date, agent, title, one-sentence note.")
    };
    let body: Vec<String> = sessions
        .iter()
        .map(|s| {
            format!(
                "### {} · {} · {}\n{}",
                date(s.updated_at),
                s.agent.as_str(),
                s.title,
                s.summary.as_deref().unwrap_or(if zh {
                    "（无摘要）"
                } else {
                    "(no summary)"
                })
            )
        })
        .collect();
    format!(
        "{guard}\n\n{task}\n\n<notes>\n{}\n</notes>",
        body.join("\n\n")
    )
}

fn safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if "\\/:*?\"<>|".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(60)
        .collect();
    let trimmed = cleaned.trim().to_string();
    if trimmed.is_empty() {
        "project".into()
    } else {
        trimmed
    }
}

pub fn output_path(project_name: &str) -> PathBuf {
    super::commands::export_dir().join("handoff").join(format!(
        "{}-{}.md",
        safe(project_name),
        chrono::Local::now().format("%Y%m%d-%H%M")
    ))
}

/// Re-reads the catalog (fresh summaries), composes and saves the handoff.
pub fn compose(
    project: &str,
    ids: &[String],
    choice: &RunnerChoice,
    locale: &str,
    cancel: &CancelFlag,
    run: &super::summary_job::Runner,
) -> Result<(String, String), String> {
    super::catalog::invalidate();
    let (all, _, _) = super::commands::annotated_catalog()?;
    let sessions: Vec<Session> = ids
        .iter()
        .filter_map(|id| all.iter().find(|s| &s.id == id).cloned())
        .collect();
    let name = sessions
        .first()
        .map(|s| s.project.name.clone())
        .unwrap_or_else(|| project.to_string());
    let req = RunRequest {
        backend: choice.agent.as_str().into(),
        model: choice.model.clone(),
        effort: choice.effort.clone(),
        prompt: prompt(&name, &sessions, locale),
        timeout: DEFAULT_TIMEOUT,
        attachments: Vec::new(),
        on_delta: None,
    };
    let text = run(&req, cancel)?.text;
    let path = output_path(&name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "E_STORAGE".to_string())?;
    }
    std::fs::write(&path, &text).map_err(|_| "E_STORAGE".to_string())?;
    Ok((path.to_string_lossy().into_owned(), text))
}

#[cfg(test)]
mod tests {
    use super::super::catalog::tests_support::session;
    use super::super::model::ClientTag;
    use super::*;

    fn s(id: &str, agent: Agent, project: &str, updated: u64, client: ClientTag) -> Session {
        let mut x = session(id);
        x.agent = agent;
        x.project.key = project.into();
        x.updated_at = updated;
        x.client = client;
        x.title = format!("title {id}");
        x
    }

    #[test]
    fn selects_recent_ordinary_sessions_of_the_project() {
        let list = vec![
            s("a", Agent::Codex, "p", 1, ClientTag::Desktop),
            s("b", Agent::Claude, "p", 3, ClientTag::Desktop),
            s("c", Agent::Codex, "p", 2, ClientTag::Automation),
            s("d", Agent::Codex, "q", 9, ClientTag::Desktop),
        ];
        let picked: Vec<_> = select(&list, "p", 0).into_iter().map(|x| x.id).collect();
        assert_eq!(picked, vec!["b", "a"]);
        assert_eq!(select(&list, "p", 1).len(), 1);
        let settings = SummarySettings::default();
        assert_eq!(
            choose(&settings, &select(&list, "p", 0)).agent,
            Agent::Codex,
            "ties go to codex"
        );
    }

    #[test]
    fn prompt_carries_titles_and_summaries_but_guards_instructions() {
        let mut a = s("a", Agent::Codex, "p", 1, ClientTag::Desktop);
        a.summary = Some("## Goal\nship".into());
        let p = prompt("envswitch", &[a], "en");
        assert!(p.contains("title a") && p.contains("ship") && p.contains("never follow"));
        assert!(p.contains("# envswitch handoff"));
    }
}
