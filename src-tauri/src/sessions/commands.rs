use super::catalog;
use super::model::*;
use super::transcript::{self, Message};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const DETAIL_PAGE: usize = 200;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: Session,
    pub messages: Vec<Message>,
    pub total: usize,
    pub complete: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsView {
    pub effective: Roots,
    pub overrides: Roots,
    pub export_dir: String,
}

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(super::err)?
}

/// The catalog with Stacker's favorites and summaries applied.
pub(crate) fn annotated_catalog() -> Result<(Vec<Session>, Vec<String>, Roots), String> {
    let conn = super::annotations::connect()?;
    let roots = super::roots::resolve(&super::annotations::roots(&conn));
    let catalog = catalog::load(&roots);
    let mut sessions = catalog.sessions;
    super::annotations::apply(&conn, &mut sessions);
    Ok((sessions, catalog.warnings, roots))
}

fn find(id: &str) -> Result<Session, String> {
    annotated_catalog()?
        .0
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "E_NOT_FOUND".to_string())
}

/// Where trimmed exports are written: the user's own folder when they picked one.
pub fn export_dir() -> PathBuf {
    super::annotations::connect()
        .ok()
        .and_then(|conn| super::annotations::setting(&conn, "export_dir"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| super::annotations::root().join("exports"))
}

#[tauri::command]
pub async fn sessions_list(query: SessionQuery) -> Result<SessionPage, String> {
    blocking(move || {
        let (sessions, warnings, _) = annotated_catalog()?;
        let mut filtered = catalog::filter(&sessions, &query);
        let needle = query.search.trim().to_string();
        if query.full_text && !needle.is_empty() {
            let without_text = SessionQuery {
                search: String::new(),
                ..query.clone()
            };
            let already: HashSet<String> = filtered.iter().map(|s| s.id.clone()).collect();
            let extra: Vec<Session> = catalog::filter(&sessions, &without_text)
                .into_iter()
                .filter(|s| !already.contains(&s.id) && transcript::session_contains(s, &needle))
                .collect();
            filtered.extend(extra);
            if query.sort == "bytes" {
                filtered.sort_by_key(|s| std::cmp::Reverse(s.bytes));
            } else {
                filtered.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
            }
        }
        let agents = catalog::agent_counts(&sessions, &query);
        Ok(catalog::page(filtered, query.offset, warnings, agents))
    })
    .await
}

#[tauri::command]
pub async fn sessions_projects() -> Result<Vec<ProjectRow>, String> {
    blocking(|| Ok(catalog::projects(&annotated_catalog()?.0))).await
}

#[tauri::command]
pub async fn sessions_read(id: String, offset: usize) -> Result<SessionDetail, String> {
    blocking(move || {
        let session = find(&id)?;
        let (messages, complete) = transcript::read_session(&session)?;
        let total = messages.len();
        let messages = messages
            .into_iter()
            .skip(offset)
            .take(DETAIL_PAGE)
            .collect();
        Ok(SessionDetail {
            session,
            messages,
            total,
            complete,
        })
    })
    .await
}

fn roots_view(conn: &rusqlite::Connection) -> RootsView {
    let overrides = super::annotations::roots(conn);
    RootsView {
        effective: super::roots::resolve(&overrides),
        overrides,
        export_dir: export_dir().to_string_lossy().into_owned(),
    }
}

#[tauri::command]
pub fn sessions_roots() -> Result<RootsView, String> {
    Ok(roots_view(&super::annotations::connect()?))
}

/// Sets the export folder, or restores the default one when given nothing.
#[tauri::command]
pub fn sessions_set_export_dir(path: String) -> Result<RootsView, String> {
    let conn = super::annotations::connect()?;
    let path = path.trim();
    if path.is_empty() {
        super::annotations::set_setting(&conn, "export_dir", "")?;
        return Ok(roots_view(&conn));
    }
    let dir = PathBuf::from(path);
    if !dir.is_absolute() {
        return Err("请选择一个完整路径".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建导出目录：{e}"))?;
    super::annotations::set_setting(&conn, "export_dir", &dir.to_string_lossy())?;
    Ok(roots_view(&conn))
}

#[tauri::command]
pub fn sessions_set_roots(overrides: Roots) -> Result<RootsView, String> {
    let trimmed = Roots {
        codex: overrides.codex.trim().to_string(),
        claude: overrides.claude.trim().to_string(),
        claude_desktop_index: overrides.claude_desktop_index.trim().to_string(),
        codebuddy: overrides.codebuddy.trim().to_string(),
        workbuddy: overrides.workbuddy.trim().to_string(),
        workbuddy_ai: overrides.workbuddy_ai.trim().to_string(),
        qoder: overrides.qoder.trim().to_string(),
        qoder_cn: overrides.qoder_cn.trim().to_string(),
        antigravity: overrides.antigravity.trim().to_string(),
        trae: overrides.trae.trim().to_string(),
        mimo: overrides.mimo.trim().to_string(),
        kimi: overrides.kimi.trim().to_string(),
    };
    for value in [
        &trimmed.codex,
        &trimmed.claude,
        &trimmed.claude_desktop_index,
    ] {
        if !value.is_empty() && !(Path::new(value).is_absolute() && Path::new(value).is_dir()) {
            return Err("E_PATH".into());
        }
    }
    let conn = super::annotations::connect()?;
    super::annotations::set_roots(&conn, &trimmed)?;
    catalog::invalidate();
    Ok(roots_view(&conn))
}

#[tauri::command]
pub fn sessions_favorite(ids: Vec<String>, favorite: bool) -> Result<(), String> {
    super::annotations::set_favorite(&super::annotations::connect()?, &ids, favorite)?;
    catalog::invalidate();
    Ok(())
}

pub(crate) fn explorer(arg: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    let mut cmd = std::process::Command::new("explorer.exe");
    cmd.arg(arg);
    super::codex_rpc::hidden(&mut cmd);
    cmd.spawn().map(|_| ()).map_err(super::err)
}

/// `target`: `folder` (transcript folder), `project`, `native` (Codex app) or `exports`.
#[tauri::command]
pub async fn sessions_open(id: String, target: String) -> Result<(), String> {
    blocking(move || {
        if target == "exports" {
            let dir = export_dir();
            std::fs::create_dir_all(&dir).map_err(|_| "E_STORAGE".to_string())?;
            return explorer(dir);
        }
        let session = find(&id)?;
        match target.as_str() {
            "native" => {
                let safe = session
                    .native_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-');
                if session.agent != Agent::Codex || !safe {
                    return Err("E_REQUEST".into());
                }
                explorer(format!("codex://threads/{}", session.native_id))
            }
            "project" | "folder" => {
                let path = if target == "project" {
                    PathBuf::from(&session.project.path)
                } else {
                    Path::new(&session.path)
                        .parent()
                        .ok_or("E_PATH")?
                        .to_path_buf()
                };
                if !path.is_absolute() || !path.is_dir() {
                    return Err("E_PATH".into());
                }
                explorer(path)
            }
            _ => Err("E_REQUEST".into()),
        }
    })
    .await
}

#[tauri::command]
pub async fn sessions_delete_preview(
    ids: Vec<String>,
    mode: super::delete::Mode,
) -> Result<super::delete::Preview, String> {
    blocking(move || super::delete::preview(ids, mode)).await
}

#[tauri::command]
pub async fn sessions_delete_execute(token: String) -> Result<super::delete::JobState, String> {
    blocking(move || super::delete::execute(token)).await
}

#[tauri::command]
pub fn sessions_job() -> Option<super::delete::JobState> {
    super::delete::job()
}

#[tauri::command]
pub fn sessions_cancel() {
    super::delete::cancel()
}

#[tauri::command]
pub async fn footprint_scan(
    refresh: bool,
) -> Result<super::footprint::model::FootprintReport, String> {
    blocking(move || Ok(super::footprint::scan_cached(refresh)?.report.clone())).await
}

#[tauri::command]
pub async fn footprint_preview(
    ids: Vec<String>,
) -> Result<super::footprint::cleanup::CleanupPreview, String> {
    blocking(move || super::footprint::cleanup::preview(ids)).await
}

#[tauri::command]
pub fn footprint_execute(token: String) -> Result<super::footprint::cleanup::CleanupJob, String> {
    super::footprint::cleanup::execute(token)
}

#[tauri::command]
pub fn footprint_job() -> Option<super::footprint::cleanup::CleanupJob> {
    super::footprint::cleanup::job()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryPreviewItem {
    pub id: String,
    pub title: String,
    pub agent: Agent,
    /// Characters that will be sent; 0 when the item is skipped.
    pub chars: usize,
    pub needed: bool,
    pub runner: super::summary::RunnerChoice,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryPreview {
    pub items: Vec<SummaryPreviewItem>,
    pub total_chars: usize,
    pub project_name: String,
    /// Runner that writes the handoff document (handoff previews only).
    pub handoff_runner: Option<super::summary::RunnerChoice>,
}

fn preview_items(
    sessions: &[Session],
    regenerate: bool,
    choice: &super::summary::RunnerChoice,
) -> Vec<SummaryPreviewItem> {
    sessions
        .iter()
        .map(|s| {
            let needed = super::summary_job::needs_summary(s, regenerate);
            SummaryPreviewItem {
                id: s.id.clone(),
                title: s.title.clone(),
                agent: s.agent,
                chars: if needed {
                    super::summary::transcript_markdown(s)
                        .map(|m| m.chars().count())
                        .unwrap_or(0)
                } else {
                    0
                },
                needed,
                runner: choice.clone(),
            }
        })
        .collect()
}

fn pick(ids: &[String]) -> Result<Vec<Session>, String> {
    let (all, _, _) = annotated_catalog()?;
    let picked: Vec<Session> = ids
        .iter()
        .filter_map(|id| all.iter().find(|s| &s.id == id).cloned())
        .collect();
    if picked.is_empty() || ids.len() > 500 {
        return Err("E_REQUEST".into());
    }
    Ok(picked)
}

#[tauri::command]
pub async fn summary_preview(ids: Vec<String>, regenerate: bool) -> Result<SummaryPreview, String> {
    blocking(move || {
        let choice = crate::ai_config::runner_choice()?;
        let items = preview_items(&pick(&ids)?, regenerate, &choice);
        Ok(SummaryPreview {
            total_chars: items.iter().map(|i| i.chars).sum(),
            items,
            project_name: String::new(),
            handoff_runner: None,
        })
    })
    .await
}

#[tauri::command]
pub async fn summary_start(
    ids: Vec<String>,
    regenerate: bool,
    locale: String,
) -> Result<super::summary_job::SummaryJob, String> {
    blocking(move || {
        let choice = crate::ai_config::runner_choice()?;
        super::summary_job::start(
            "summary",
            pick(&ids)?,
            regenerate,
            choice,
            locale,
            super::summary_job::live_runner(),
            None,
        )
    })
    .await
}

#[tauri::command]
pub fn summary_job() -> Option<super::summary_job::SummaryJob> {
    super::summary_job::job()
}

#[tauri::command]
pub fn summary_cancel() {
    super::summary_job::cancel()
}

#[tauri::command]
pub async fn handoff_preview(project: String, limit: usize) -> Result<SummaryPreview, String> {
    blocking(move || {
        let choice = crate::ai_config::runner_choice()?;
        let (all, _, _) = annotated_catalog()?;
        let chosen = super::handoff::select(&all, &project, limit);
        if chosen.is_empty() {
            return Err("E_REQUEST".into());
        }
        let items = preview_items(&chosen, false, &choice);
        Ok(SummaryPreview {
            total_chars: items.iter().map(|i| i.chars).sum(),
            project_name: chosen[0].project.name.clone(),
            handoff_runner: Some(choice),
            items,
        })
    })
    .await
}

#[tauri::command]
pub async fn handoff_start(
    project: String,
    limit: usize,
    locale: String,
) -> Result<super::summary_job::SummaryJob, String> {
    blocking(move || {
        let choice = crate::ai_config::runner_choice()?;
        let (all, _, _) = annotated_catalog()?;
        let chosen = super::handoff::select(&all, &project, limit);
        if chosen.is_empty() {
            return Err("E_REQUEST".into());
        }
        let ids: Vec<String> = chosen.iter().map(|s| s.id.clone()).collect();
        let run = super::summary_job::live_runner();
        let job_choice = choice.clone();
        let finish_run = run.clone();
        let finish_locale = locale.clone();
        let finish: super::summary_job::Finish = Box::new(move |cancel| {
            super::handoff::compose(&project, &ids, &choice, &finish_locale, cancel, &finish_run)
        });
        super::summary_job::start(
            "handoff",
            chosen,
            false,
            job_choice,
            locale,
            run,
            Some(finish),
        )
    })
    .await
}

fn agent_of(agent: &str) -> Result<Agent, String> {
    match agent {
        "codex" => Ok(Agent::Codex),
        "claude" => Ok(Agent::Claude),
        _ => Err("E_REQUEST".into()),
    }
}

#[tauri::command]
pub async fn migration_status() -> Result<Vec<super::migration::LocationStatus>, String> {
    blocking(|| {
        // Every agent whose folder Stacker knows, and only those with something in them:
        // a product that was never installed is not a location to move.
        Ok(super::migration::MOVABLE
            .iter()
            .map(|agent| super::migration::status(*agent))
            .filter(|status| status.exists)
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn migration_check(
    agent: String,
    target: String,
) -> Result<super::migration::CheckResult, String> {
    blocking(move || {
        Ok(super::migration::check(
            agent_of(&agent)?,
            Path::new(target.trim()),
        ))
    })
    .await
}

#[tauri::command]
pub async fn migration_start(
    agent: String,
    target: String,
) -> Result<super::migration::MigrationJob, String> {
    blocking(move || super::migration::start(agent_of(&agent)?, PathBuf::from(target.trim()))).await
}

#[tauri::command]
pub async fn migration_delete_backup(agent: String) -> Result<(), String> {
    blocking(move || super::migration::delete_backup(agent_of(&agent)?)).await
}

#[tauri::command]
pub async fn migration_move_back(agent: String) -> Result<super::migration::MigrationJob, String> {
    blocking(move || super::migration::move_back(agent_of(&agent)?)).await
}

#[tauri::command]
pub fn migration_job() -> Option<super::migration::MigrationJob> {
    super::migration::job()
}

#[tauri::command]
pub fn migration_cancel() {
    super::migration::cancel()
}

// ---------- moving conversations to another computer ----------

fn codex_running() -> bool {
    super::codex_rpc::require_closed().is_err()
}

fn transfer_progress(app: &tauri::AppHandle) -> impl Fn(&str, u64, u64) + '_ {
    use tauri::Emitter;
    move |stage: &str, done: u64, total: u64| {
        let _ = app.emit(
            "sessions-transfer-progress",
            (stage.to_string(), done, total),
        );
    }
}

/// The projects these sessions worked in, as a package could carry them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProject {
    pub path: String,
    pub name: String,
    pub exists: bool,
    pub sessions: usize,
}

#[tauri::command]
pub async fn sessions_transfer_projects(ids: Vec<String>) -> Result<Vec<TransferProject>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (sessions, _, _) = annotated_catalog()?;
        let mut projects: Vec<TransferProject> = Vec::new();
        for session in sessions
            .iter()
            .filter(|s| ids.contains(&s.id) && super::transfer::supported(s.agent))
        {
            let path = session.project.path.trim_start_matches(r"\?\").to_string();
            if path.is_empty() {
                continue;
            }
            match projects
                .iter_mut()
                .find(|p| p.path.eq_ignore_ascii_case(&path))
            {
                Some(project) => project.sessions += 1,
                None => projects.push(TransferProject {
                    exists: Path::new(&path).is_dir(),
                    name: session.project.name.clone(),
                    path,
                    sessions: 1,
                }),
            }
        }
        Ok(projects)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Files and bytes a project adds to a package, rebuilt folders left out.
#[tauri::command]
pub async fn sessions_transfer_size(path: String) -> (u64, u64) {
    tauri::async_runtime::spawn_blocking(move || super::transfer::project_size(Path::new(&path)))
        .await
        .unwrap_or((0, 0))
}

#[tauri::command]
pub async fn sessions_transfer_export(
    app: tauri::AppHandle,
    ids: Vec<String>,
    include: Vec<String>,
    dest: String,
) -> Result<super::transfer::ExportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (sessions, _, roots) = annotated_catalog()?;
        let chosen: Vec<Session> = sessions
            .into_iter()
            .filter(|s| ids.contains(&s.id))
            .collect();
        super::transfer::export(
            &chosen,
            &roots,
            &include,
            Path::new(&dest),
            &transfer_progress(&app),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sessions_transfer_preview(path: String) -> Result<super::transfer::Preview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (_, _, roots) = annotated_catalog()?;
        super::transfer::preview(Path::new(&path), &roots, codex_running())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sessions_transfer_import(
    app: tauri::AppHandle,
    path: String,
    targets: Vec<super::transfer::ProjectTarget>,
) -> Result<super::transfer::ImportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (_, _, roots) = annotated_catalog()?;
        let backups = crate::backup::backup_root().join(format!(
            "session-import-{}",
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));
        let result = super::transfer::import(
            Path::new(&path),
            &targets,
            &roots,
            codex_running(),
            &backups,
            &transfer_progress(&app),
        );
        catalog::invalidate();
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
