//! 「提炼」的 Tauri 命令。
use super::job::{self, DistillJob, StartRequest};
use super::prompts;
use super::skills;
use super::sources::{self, Candidate, SourceRef};
use super::store::{self, DistillQuery, DistillResult, KindCounts};
use crate::sessions::commands::{annotated_catalog, blocking, explorer};
use crate::sessions::model::Session;
use crate::sessions::summary::{self, RunnerChoice, SummarySettings};
use crate::sessions::summary_job::live_runner;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub title: String,
    pub chars: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistillPreview {
    pub items: Vec<PreviewItem>,
    pub total_chars: usize,
    pub runner: RunnerChoice,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistillPage {
    pub items: Vec<DistillResult>,
    pub total: usize,
    pub counts: KindCounts,
}

fn settings_or_saved(settings: Option<SummarySettings>) -> Result<SummarySettings, String> {
    match settings {
        Some(s) => Ok(s),
        None => Ok(summary::load_settings(
            &crate::sessions::annotations::connect()?,
        )),
    }
}

/// 提炼没有「同源」可言：沿用网页对话摘要的选法（同源 = Claude）。
fn runner_for(settings: &SummarySettings) -> RunnerChoice {
    crate::webchat::commands::runner_for(settings)
}

/// 只有在真的需要 `session:` 来源时才去读本机会话目录。
fn sessions_if_needed(refs: &[SourceRef]) -> Vec<Session> {
    if refs.iter().any(|r| r.kind == "session") {
        annotated_catalog().map(|c| c.0).unwrap_or_default()
    } else {
        Vec::new()
    }
}

#[tauri::command]
pub async fn distill_candidates(search: String) -> Result<Vec<Candidate>, String> {
    blocking(move || {
        let sessions = annotated_catalog().map(|c| c.0).unwrap_or_default();
        sources::candidates(&crate::webchat::root(), &sessions, &search)
    })
    .await
}

#[tauri::command]
pub async fn distill_preview(
    sources: Vec<SourceRef>,
    settings: Option<SummarySettings>,
) -> Result<DistillPreview, String> {
    blocking(move || {
        let settings = settings_or_saved(settings)?;
        let sessions = sessions_if_needed(&sources);
        let units = super::sources::gather(&crate::webchat::root(), &sessions, &sources)?;
        let items: Vec<PreviewItem> = units
            .iter()
            .map(|u| PreviewItem {
                title: u.title.clone(),
                chars: u.markdown.chars().count(),
            })
            .collect();
        Ok(DistillPreview {
            total_chars: items.iter().map(|i| i.chars).sum(),
            items,
            runner: runner_for(&settings),
        })
    })
    .await
}

#[tauri::command]
pub async fn distill_start(
    sources: Vec<SourceRef>,
    kinds: Vec<String>,
    settings: Option<SummarySettings>,
    locale: String,
) -> Result<DistillJob, String> {
    blocking(move || {
        let settings = settings_or_saved(settings)?;
        let sessions = sessions_if_needed(&sources);
        job::start(
            StartRequest {
                root: crate::webchat::root(),
                sessions,
                refs: sources,
                kinds,
                choice: runner_for(&settings),
                locale,
            },
            live_runner(),
        )
    })
    .await
}

#[tauri::command]
pub fn distill_job() -> Option<DistillJob> {
    job::job()
}

#[tauri::command]
pub fn distill_cancel() {
    job::cancel()
}

#[tauri::command]
pub async fn distill_list(query: DistillQuery) -> Result<DistillPage, String> {
    blocking(move || {
        let conn = crate::webchat::store::open(&crate::webchat::root())?;
        let items = store::list(&conn, &query)?;
        Ok(DistillPage {
            total: items.len(),
            items,
            counts: store::counts(&conn)?,
        })
    })
    .await
}

#[tauri::command]
pub async fn distill_save(
    id: String,
    title: String,
    body: String,
) -> Result<DistillResult, String> {
    blocking(move || {
        let conn = crate::webchat::store::open(&crate::webchat::root())?;
        store::save_text(&conn, &id, &title, &body, crate::webchat::now_ms())?;
        store::get(&conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn distill_state(id: String, state: String) -> Result<DistillResult, String> {
    blocking(move || {
        let conn = crate::webchat::store::open(&crate::webchat::root())?;
        store::set_state(&conn, &id, &state, crate::webchat::now_ms())?;
        store::get(&conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn distill_delete(id: String) -> Result<(), String> {
    blocking(move || {
        let conn = crate::webchat::store::open(&crate::webchat::root())?;
        store::delete(&conn, &id)
    })
    .await
}

/// 当前筛选下的结果导出成一个 Markdown 文件。
pub fn export_markdown(items: &[DistillResult], locale: &str) -> String {
    let mut out = format!(
        "# {}\n\n",
        if locale.starts_with("zh") {
            "提炼结果"
        } else {
            "Distilled results"
        }
    );
    out.push_str(&format!("- {}\n\n", items.len()));
    for item in items {
        out.push_str(&format!(
            "## {} · {}\n\n",
            prompts::kind_label(&item.kind, locale),
            item.title
        ));
        out.push_str(&format!("- {}\n", item.state));
        out.push_str(&format!("- {}\n", item.by));
        for s in &item.sources {
            out.push_str(&format!("- {} ({})\n", s.title, s.key));
        }
        if !item.folder.is_empty() {
            out.push_str(&format!("- {}\n", item.folder));
        }
        out.push_str(&format!("\n{}\n\n", item.body));
    }
    out
}

#[tauri::command]
pub async fn distill_export(query: DistillQuery, locale: String) -> Result<String, String> {
    blocking(move || {
        let root = crate::webchat::root();
        let conn = crate::webchat::store::open(&root)?;
        let items = store::list(&conn, &query)?;
        if items.is_empty() {
            return Err("E_REQUEST".into());
        }
        let dir = super::exports_in(&root);
        std::fs::create_dir_all(&dir).map_err(|_| "E_STORAGE".to_string())?;
        let file = dir.join(format!("distill-{}.md", crate::webchat::now_ms()));
        std::fs::write(&file, export_markdown(&items, &locale))
            .map_err(|_| "E_STORAGE".to_string())?;
        Ok(file.to_string_lossy().into_owned())
    })
    .await
}

/// `target`：`skill`（某个草稿文件夹）、`skills`（草稿总目录）、`exports`（提炼导出目录）。
pub fn open_target(root: &Path, target: &str, name: &str) -> Result<(), String> {
    match target {
        "skill" => skills::open_folder(&super::skills_in(root), name),
        "skills" | "exports" => {
            let dir = if target == "skills" {
                super::skills_in(root)
            } else {
                super::exports_in(root)
            };
            std::fs::create_dir_all(&dir).map_err(|_| "E_STORAGE".to_string())?;
            explorer(dir)
        }
        _ => Err("E_REQUEST".into()),
    }
}

#[tauri::command]
pub async fn distill_open(target: String, name: String) -> Result<(), String> {
    blocking(move || open_target(&crate::webchat::root(), &target, &name)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distill::DistillSource;

    fn result(kind: &str, title: &str) -> DistillResult {
        DistillResult {
            id: format!("{kind}-1"),
            kind: kind.into(),
            title: title.into(),
            body: "Body.".into(),
            sources: vec![DistillSource {
                key: "web:chatgpt:a".into(),
                kind: "web".into(),
                title: "Trip plan".into(),
                link: String::new(),
            }],
            state: "adopted".into(),
            by: "claude / sonnet / low".into(),
            folder: String::new(),
            created_at: 1,
            updated_at: 2,
        }
    }

    #[test]
    fn the_export_lists_every_result_with_its_kind_state_and_sources() {
        let text = export_markdown(
            &[result("qa", "Where to go"), result("skill", "Plan a trip")],
            "en",
        );
        assert!(text.starts_with("# "));
        assert!(text.contains("## Experience Q&A · Where to go"));
        assert!(text.contains("## Skill drafts · Plan a trip"));
        assert!(text.contains("Trip plan (web:chatgpt:a)"));
        assert!(text.contains("claude / sonnet / low"));
        assert!(text.contains("Body."));
    }

    #[test]
    fn opening_a_target_is_checked_before_anything_is_touched() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            open_target(dir.path(), "nonsense", "").unwrap_err(),
            "E_REQUEST"
        );
        assert_eq!(
            open_target(dir.path(), "skill", "../x").unwrap_err(),
            "E_REQUEST"
        );
        assert_eq!(
            open_target(dir.path(), "skill", "missing").unwrap_err(),
            "E_NOT_FOUND"
        );
    }
}
