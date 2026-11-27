//! Freeing space by itself when a disk runs low: build output first, then download caches,
//! then — on the disk that holds them — temporary files and the caches programs rebuild.
//! Only what a rule recognises as made again by a build, an install or the program itself
//! goes; the user's files, virtual environments, models and the Recycle Bin never do.

pub mod find;
pub mod model;
pub mod remove;
pub mod run;
pub mod settings;
pub mod watch;

#[cfg(test)]
mod tests;

use model::{CheckOutcome, DiskSpace, Status};

/// A disk's total and free bytes.
#[cfg(windows)]
pub fn space_of(root: &str) -> Option<(u64, u64)> {
    use winapi::shared::ntdef::ULARGE_INTEGER;
    use winapi::um::fileapi::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and the two outputs are plain integers the call fills.
    unsafe {
        let mut total: ULARGE_INTEGER = std::mem::zeroed();
        let mut free: ULARGE_INTEGER = std::mem::zeroed();
        if GetDiskFreeSpaceExW(wide.as_ptr(), std::ptr::null_mut(), &mut total, &mut free) == 0 {
            return None;
        }
        Some((*total.QuadPart(), *free.QuadPart()))
    }
}

#[cfg(target_os = "macos")]
pub fn space_of(root: &str) -> Option<(u64, u64)> {
    crate::platform::disk_usage(std::path::Path::new(root))
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn space_of(_root: &str) -> Option<(u64, u64)> {
    None
}

/// Local disks only: a network share or a USB stick is not Stacker's to clean.
#[cfg(windows)]
fn is_local_fixed(root: &str) -> bool {
    use winapi::um::fileapi::GetDriveTypeW;
    use winapi::um::winbase::DRIVE_FIXED;
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated.
    unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_FIXED }
}

#[cfg(not(windows))]
fn is_local_fixed(_root: &str) -> bool {
    true
}

/// The local disks and their space now.
pub fn disks() -> Vec<DiskSpace> {
    super::targets::list_fixed_volumes()
        .into_iter()
        .filter(|volume| volume.total_bytes > 0 && is_local_fixed(&volume.root))
        .map(|volume| DiskSpace {
            root: volume.root,
            total: volume.total_bytes,
            free: volume.free_bytes,
        })
        .collect()
}

pub fn status() -> Status {
    let settings = settings::load();
    Status {
        enabled: settings.enabled,
        threshold_percent: settings.threshold_percent,
        running: watch::running(),
        runs: settings.runs,
        disks: disks(),
    }
}

#[tauri::command]
pub async fn low_space_status() -> Result<Status, String> {
    crate::sessions::commands::blocking(|| Ok(status())).await
}

#[tauri::command]
pub async fn low_space_settings_set(update: settings::SettingsUpdate) -> Result<Status, String> {
    crate::sessions::commands::blocking(move || {
        settings::apply(update)?;
        Ok(status())
    })
    .await
}

/// Looks at every disk now and cleans the low ones in the background, whatever the last run
/// was and whether the automatic cleanup is on.
#[tauri::command]
pub fn low_space_check_now(app: tauri::AppHandle) -> Result<CheckOutcome, String> {
    watch::check_now(app)
}
