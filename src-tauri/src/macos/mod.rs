//! macOS counterparts of the Windows-only pieces: the Trash, and other system services.

use std::path::Path;

/// Moves a file or folder to the user's Trash through NSFileManager. (The Finder route asks
/// for Automation permission and plays a sound for every item.)
pub fn trash(path: &Path) -> Result<(), String> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut context = trash::TrashContext::default();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context
        .delete(path)
        .map_err(|e| format!("移到废纸篓失败：{e}"))
}

/// The name Finder shows for the volume holding `path`.
pub fn volume_name(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/diskutil")
        .arg("info")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().strip_prefix("Volume Name:"))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// What `scutil --proxy` prints: the proxy setting in effect for the current network.
pub fn scutil_proxy() -> Option<String> {
    std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub mod keychain;
pub mod protect;
pub mod shell_env;
pub mod updater;
pub mod user_env;
