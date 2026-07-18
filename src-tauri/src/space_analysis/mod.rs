pub mod known;
pub mod model;
pub mod targets;
pub mod tasks;
pub mod walker;
pub mod windows_fs;

use self::model::{
    AnalysisSummary, DirectoryNode, LargeFileRow, Paged, QuickScanResult, ScanMode, ScanProgress,
    ScanRequest, VolumeInfo,
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
