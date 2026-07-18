pub mod classifier;
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

fn directory_to_open(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let metadata = std::fs::metadata(path).map_err(|_| "The selected path is no longer available.".to_string())?;
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
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
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

        assert_eq!(directory_to_open(&file).unwrap(), temp.path().canonicalize().unwrap());
        assert_eq!(directory_to_open(temp.path()).unwrap(), temp.path().canonicalize().unwrap());
    }

    #[test]
    fn directory_opening_rejects_missing_paths() {
        let temp = tempfile::tempdir().unwrap();
        assert!(directory_to_open(&temp.path().join("missing.bin")).is_err());
    }
}
