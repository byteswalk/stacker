pub mod classifier;
pub mod cleanup_plan;
pub mod cleanup_tasks;
pub mod duplicates;
pub mod elevated;
pub mod file_removal;
pub mod history;
pub mod known;
pub mod model;
pub mod monitor;
pub mod snapshots;
pub mod targets;
pub mod tasks;
pub mod walker;
pub mod windows_fs;

pub use self::cleanup_tasks::CleanupTaskManager;
use self::model::{
    AnalysisSummary, CleanupPlan, CleanupProgress, CleanupResult, DirectoryNode, LargeFileRow,
    Paged, QuickScanResult, ScanMode, ScanProgress, ScanRequest, SnapshotComparison,
    SnapshotMetadata, VolumeInfo,
};
pub use self::monitor::SpaceMonitorManager;
use self::targets::{list_fixed_volumes, validate_targets};
pub use self::tasks::SpaceTaskManager;

#[tauri::command]
pub fn space_monitor_start(
    roots: Vec<String>,
    manager: tauri::State<'_, SpaceMonitorManager>,
) -> Result<String, String> {
    manager.start(roots)
}

#[tauri::command]
pub fn space_monitor_status(
    task_id: String,
    manager: tauri::State<'_, SpaceMonitorManager>,
) -> Result<monitor::MonitorSnapshot, String> {
    manager.status(&task_id)
}

#[tauri::command]
pub fn space_monitor_stop(
    task_id: String,
    manager: tauri::State<'_, SpaceMonitorManager>,
) -> Result<(), String> {
    manager.stop(&task_id)
}

#[tauri::command]
pub fn space_monitor_dispose(
    task_id: String,
    manager: tauri::State<'_, SpaceMonitorManager>,
) -> Result<(), String> {
    manager.dispose(&task_id)
}

#[tauri::command]
pub fn space_fixed_volumes() -> Vec<VolumeInfo> {
    list_fixed_volumes()
}

#[tauri::command]
pub fn space_scan_start(
    request: ScanRequest,
    manager: tauri::State<'_, SpaceTaskManager>,
    window: tauri::Window,
) -> Result<String, String> {
    let targets = validate_targets(&request).map_err(|error| error.to_string())?;
    match request.mode {
        ScanMode::Quick => manager.start_quick(window),
        ScanMode::Directories | ScanMode::Drives => manager.start_deep(
            targets
                .into_iter()
                .map(|target| target.path().to_path_buf())
                .collect(),
            window,
        ),
    }
}

#[tauri::command]
pub fn space_scan_start_elevated(
    request: ScanRequest,
    manager: tauri::State<'_, SpaceTaskManager>,
    window: tauri::Window,
) -> Result<String, String> {
    if request.mode == ScanMode::Quick {
        return Err("Quick Scan does not require administrator access.".into());
    }
    let targets = validate_targets(&request).map_err(|error| error.to_string())?;
    let paths = targets
        .into_iter()
        .map(|target| target.path().to_path_buf())
        .collect::<Vec<_>>();
    manager.start_elevated_deep(paths, window)
}

#[tauri::command]
pub fn space_scan_status(
    task_id: String,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<ScanProgress, String> {
    manager.status(&task_id)
}

#[tauri::command]
pub fn space_scan_cancel(
    task_id: String,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<(), String> {
    manager.cancel(&task_id)
}

#[tauri::command]
pub fn space_scan_quick_result(
    task_id: String,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<QuickScanResult, String> {
    manager.quick_result(&task_id)
}

#[tauri::command]
pub fn space_scan_summary(
    task_id: String,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<AnalysisSummary, String> {
    manager.summary(&task_id)
}

#[tauri::command]
pub fn space_scan_children(
    task_id: String,
    parent_id: String,
    offset: u64,
    limit: u64,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<Paged<DirectoryNode>, String> {
    manager.children(&task_id, &parent_id, offset, limit)
}

#[tauri::command]
pub fn space_scan_large_files(
    task_id: String,
    min_bytes: u64,
    offset: u64,
    limit: u64,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<Paged<LargeFileRow>, String> {
    manager.large_files(&task_id, min_bytes, offset, limit)
}

/// Identical files among what the scan found. Reading them takes a while, so it runs off the
/// UI thread and reports what it confirmed.
#[tauri::command]
pub async fn space_duplicates(
    task_id: String,
    min_bytes: u64,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<duplicates::DuplicateReport, String> {
    let files = manager.all_files(&task_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        duplicates::find(&files, &walker::CancellationToken::default(), min_bytes)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Settles a duplicate group checked by its ends only, by reading every file in full.
#[tauri::command]
pub async fn space_verify_duplicates(paths: Vec<String>) -> Result<Vec<Vec<String>>, String> {
    tauri::async_runtime::spawn_blocking(move || duplicates::verify(&paths))
        .await
        .map_err(|e| e.to_string())
}

/// When each file was made and last changed, so the copies of a group can be told apart.
#[tauri::command]
pub async fn space_file_times(paths: Vec<String>) -> Result<Vec<duplicates::FileTimes>, String> {
    tauri::async_runtime::spawn_blocking(move || duplicates::times(&paths))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn space_cleanup_candidates(
    task_id: String,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<Vec<DirectoryNode>, String> {
    manager.cleanup_candidates(&task_id)
}

#[tauri::command]
pub fn space_cleanup_plan(
    scan_task_id: String,
    node_ids: Vec<String>,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<CleanupPlan, String> {
    manager.create_cleanup_plan(&scan_task_id, &node_ids)
}

/// Runs blocking filesystem work off the main thread so the WebView stays responsive.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn space_cleanup_start(
    plan_id: String,
    node_ids: Vec<String>,
    app: tauri::AppHandle,
    window: tauri::Window,
) -> Result<String, String> {
    use tauri::{Emitter, Manager};

    let plan = app
        .state::<SpaceTaskManager>()
        .cleanup_plan_record(&plan_id)?;
    let needs_elevation = plan.plan.items.iter().any(|item| {
        item.requires_elevation && node_ids.iter().any(|node_id| node_id == &item.node_id)
    });
    let emit = move |progress: &CleanupProgress| {
        if let Err(error) = window.emit("space-cleanup-progress", progress) {
            log::warn!(
                "failed to emit progress for space cleanup task {}: {}",
                progress.task_id,
                error
            );
        }
    };
    if needs_elevation {
        // The UAC prompt and the delete both happen in the helper process, so the task runs on
        // its own thread and reports the same progress a plain cleanup does.
        return app.state::<CleanupTaskManager>().start_elevated(
            plan,
            &node_ids,
            emit,
            |task_id, plan, token, mut report| {
                elevated::run_cleanup(&task_id, plan, &token, &mut |completed, bytes, node| {
                    report(completed, bytes, node)
                })
            },
        );
    }
    app.state::<CleanupTaskManager>()
        .start(plan, &node_ids, emit)
}

#[tauri::command]
pub fn space_cleanup_status(
    task_id: String,
    manager: tauri::State<'_, CleanupTaskManager>,
) -> Result<CleanupProgress, String> {
    manager.status(&task_id)
}

#[tauri::command]
pub fn space_cleanup_cancel(
    task_id: String,
    manager: tauri::State<'_, CleanupTaskManager>,
) -> Result<(), String> {
    manager.cancel(&task_id)
}

/// Every cleanup that ran, newest first.
#[tauri::command]
pub async fn space_cleanup_history() -> Vec<history::HistoryRecord> {
    blocking(|| Ok(history::load())).await.unwrap_or_default()
}

#[tauri::command]
pub async fn space_cleanup_history_clear() -> Result<(), String> {
    blocking(history::clear).await
}

#[tauri::command]
pub fn space_cleanup_result(
    task_id: String,
    manager: tauri::State<'_, CleanupTaskManager>,
) -> Result<CleanupResult, String> {
    manager.result(&task_id)
}

#[tauri::command]
pub async fn space_snapshot_save(
    task_id: String,
    app: tauri::AppHandle,
) -> Result<Option<SnapshotMetadata>, String> {
    blocking(move || {
        use tauri::Manager;
        snapshots::save_completed(&app.state::<SpaceTaskManager>(), &task_id)
    })
    .await
}

#[tauri::command]
pub async fn space_snapshot_list() -> Result<Vec<SnapshotMetadata>, String> {
    blocking(snapshots::list).await
}

#[tauri::command]
pub async fn space_snapshot_compare(
    base_id: String,
    current_id: String,
    offset: u64,
    limit: u64,
) -> Result<SnapshotComparison, String> {
    blocking(move || snapshots::compare(&base_id, &current_id, offset, limit)).await
}

#[tauri::command]
pub async fn space_snapshot_delete(id: String) -> Result<(), String> {
    blocking(move || snapshots::delete(&id)).await
}

#[tauri::command]
pub async fn space_snapshot_clear() -> Result<(), String> {
    blocking(snapshots::clear).await
}

fn directory_to_open(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| "The selected path is no longer available.".to_string())?;
    let directory = if metadata.is_dir() {
        path
    } else {
        path.parent()
            .ok_or_else(|| "The containing directory is unavailable.".to_string())?
    };
    let canonical = directory
        .canonicalize()
        .map_err(|_| "The selected directory is no longer available.".to_string())?;
    if !canonical.is_dir() {
        return Err("The selected path is not a directory.".into());
    }
    Ok(canonical)
}

#[tauri::command]
pub fn space_open_directory(path: String) -> Result<(), String> {
    let directory = directory_to_open(std::path::Path::new(&path))?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(directory)
            .spawn()
            .map(|_| ())
            .map_err(|_| "Unable to open the selected directory.".to_string())
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(directory)
            .spawn()
            .map(|_| ())
            .map_err(|_| "Unable to open the selected directory.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::directory_to_open;

    #[test]
    fn directory_opening_resolves_files_to_their_parent() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("large.bin");
        std::fs::write(&file, b"data").unwrap();

        assert_eq!(
            directory_to_open(&file).unwrap(),
            temp.path().canonicalize().unwrap()
        );
        assert_eq!(
            directory_to_open(temp.path()).unwrap(),
            temp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn directory_opening_rejects_missing_paths() {
        let temp = tempfile::tempdir().unwrap();
        assert!(directory_to_open(&temp.path().join("missing.bin")).is_err());
    }
}
