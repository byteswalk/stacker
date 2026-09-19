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

async fn blocking<T: Send + 'static>(
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

pub fn export_dir() -> PathBuf {
    super::annotations::root().join("exports")
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
                .filter(|s| {
                    !already.contains(&s.id)
                        && transcript::contains(s.agent, Path::new(&s.path), &needle)
                })
                .collect();
            filtered.extend(extra);
            filtered.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        }
        Ok(catalog::page(filtered, query.offset, warnings))
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
        let (messages, complete) = transcript::read(session.agent, Path::new(&session.path))?;
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

#[tauri::command]
pub fn sessions_set_roots(overrides: Roots) -> Result<RootsView, String> {
    let trimmed = Roots {
        codex: overrides.codex.trim().to_string(),
        claude: overrides.claude.trim().to_string(),
        claude_desktop_index: overrides.claude_desktop_index.trim().to_string(),
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

fn explorer(arg: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
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
