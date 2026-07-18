pub mod classifier;
pub mod cleanup_plan;
pub mod cleanup_tasks;
pub mod elevated;
pub mod known;
pub mod model;
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
use self::targets::{list_fixed_volumes, validate_targets};
pub use self::tasks::SpaceTaskManager;

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

#[tauri::command]
pub fn space_cleanup_start(
    plan_id: String,
    node_ids: Vec<String>,
    scan_manager: tauri::State<'_, SpaceTaskManager>,
    cleanup_manager: tauri::State<'_, CleanupTaskManager>,
    window: tauri::Window,
) -> Result<String, String> {
    use tauri::Emitter;

    let plan = scan_manager.cleanup_plan_record(&plan_id)?;
    let needs_elevation = plan.plan.items.iter().any(|item| {
        item.requires_elevation && node_ids.iter().any(|node_id| node_id == &item.node_id)
    });
    if needs_elevation {
        let result = elevated::run_cleanup(plan, &node_ids)?;
        let task_id = cleanup_manager.import_completed(result);
        let progress = cleanup_manager.status(&task_id)?;
        if let Err(error) = window.emit("space-cleanup-progress", &progress) {
            log::warn!(
                "failed to emit progress for elevated space cleanup task {}: {}",
                progress.task_id,
                error
            );
        }
        return Ok(task_id);
    }
    cleanup_manager.start(plan, &node_ids, move |progress| {
        if let Err(error) = window.emit("space-cleanup-progress", progress) {
            log::warn!(
                "failed to emit progress for space cleanup task {}: {}",
                progress.task_id,
                error
            );
        }
    })
}

#[tauri::command]
pub fn space_scan_supplement_elevated(
    scan_task_id: String,
    manager: tauri::State<'_, SpaceTaskManager>,
) -> Result<AnalysisSummary, String> {
    let summary = manager.summary(&scan_task_id)?;
    elevated::run_supplement_scan(&scan_task_id, &summary.targets)
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

#[tauri::command]
pub fn space_cleanup_result(
    task_id: String,
    manager: tauri::State<'_, CleanupTaskManager>,
) -> Result<CleanupResult, String> {
    manager.result(&task_id)
}

#[tauri::command]
pub fn space_snapshot_save(task_id: String, manager: tauri::State<'_, SpaceTaskManager>) -> Result<Option<SnapshotMetadata>, String> {
    snapshots::save_completed(&manager, &task_id)
}

#[tauri::command]
pub fn space_snapshot_list() -> Result<Vec<SnapshotMetadata>, String> { snapshots::list() }

#[tauri::command]
pub fn space_snapshot_compare(base_id: String, current_id: String, offset: u64, limit: u64) -> Result<SnapshotComparison, String> {
    snapshots::compare(&base_id, &current_id, offset, limit)
}

#[tauri::command]
pub fn space_snapshot_delete(id: String) -> Result<(), String> { snapshots::delete(&id) }

#[tauri::command]
pub fn space_snapshot_clear() -> Result<(), String> { snapshots::clear() }

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
