pub mod known;
pub mod model;
pub mod targets;
pub mod tasks;
pub mod walker;

use self::model::{QuickScanResult, ScanMode, ScanProgress, ScanRequest, VolumeInfo};
use self::targets::list_fixed_volumes;
pub use self::tasks::SpaceTaskManager;

const UNSUPPORTED_SCAN_SCOPE: &str = "当前版本尚未启用该扫描范围";

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
    if let Some(error) = unsupported_scan_mode_error(&request.mode) {
        return Err(error.into());
    }
    manager.start_quick(window)
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

fn unsupported_scan_mode_error(mode: &ScanMode) -> Option<&'static str> {
    match mode {
        ScanMode::Quick => None,
        ScanMode::Directories | ScanMode::Drives => Some(UNSUPPORTED_SCAN_SCOPE),
    }
}

#[cfg(test)]
mod tests {
    use super::model::ScanMode;
    use super::*;

    #[test]
    fn unsupported_scan_scopes_return_clear_chinese_error() {
        for mode in [ScanMode::Directories, ScanMode::Drives] {
            assert_eq!(
                unsupported_scan_mode_error(&mode),
                Some("当前版本尚未启用该扫描范围")
            );
        }
        assert_eq!(unsupported_scan_mode_error(&ScanMode::Quick), None);
    }
}
