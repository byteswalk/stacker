//! What Claude Code and Codex sessions leave behind, and cleaning it: build outputs and
//! scratch folders cost no tokens to make again, so they go; the conversation itself stays,
//! at most without its screenshots.

pub mod auto;
pub mod clean;
pub mod model;
pub mod scan;
pub mod settings;
pub mod slim;

#[cfg(test)]
mod tests;

#[tauri::command]
pub async fn leftovers_scan(refresh: bool) -> Result<model::LeftoverReport, String> {
    crate::sessions::commands::blocking(move || Ok(scan::cached(refresh)?.report.clone())).await
}

#[tauri::command]
pub async fn leftovers_preview(ids: Vec<String>) -> Result<clean::LeftoverPreview, String> {
    crate::sessions::commands::blocking(move || clean::preview(ids)).await
}

#[tauri::command]
pub fn leftovers_execute(
    token: String,
) -> Result<crate::sessions::footprint::cleanup::CleanupJob, String> {
    clean::execute(token)
}

#[tauri::command]
pub fn leftovers_job() -> Option<crate::sessions::footprint::cleanup::CleanupJob> {
    clean::job()
}

/// Saves the settings; what is ticked depends on them, so the last scan is dropped.
#[tauri::command]
pub fn leftovers_settings_set(
    update: settings::SettingsUpdate,
) -> Result<settings::Settings, String> {
    let saved = settings::apply(update)?;
    scan::invalidate();
    Ok(saved)
}
