//! Small facts about the operating system that several features share.

use std::path::PathBuf;

/// Folders that belong to the system and its programs, never to the user's projects:
/// nothing in them is offered for removal. Windows names its own through environment
/// variables where it is asked; these are the macOS ones.
pub fn system_roots() -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let mut roots: Vec<PathBuf> = [
        "/System",
        "/Library",
        "/Applications",
        "/usr",
        "/bin",
        "/sbin",
        "/private",
        "/opt",
        "/cores",
        "/Volumes/Recovery",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Library"));
        roots.push(home.join("Applications"));
    }
    roots
}

/// The name a Windows command (`git.exe`, `npm.cmd`) goes by here: on macOS commands have no
/// extension, so `git.exe` is looked for as `git`.
pub fn command_name(name: &str) -> &str {
    if cfg!(windows) {
        return name;
    }
    name.rsplit_once('.')
        .filter(|(stem, ext)| {
            !stem.is_empty()
                && ["exe", "cmd", "bat", "ps1", "com"].contains(&ext.to_ascii_lowercase().as_str())
        })
        .map_or(name, |(stem, _)| stem)
}

/// A volume's total and free bytes (free as an ordinary user may use it).
#[cfg(target_os = "macos")]
pub fn disk_usage(path: &std::path::Path) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    // SAFETY: `info` is plain data the call fills; `c` is NUL-terminated.
    let mut info: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut info) } != 0 {
        return None;
    }
    let unit = info.f_frsize as u64;
    Some((
        (info.f_blocks as u64).saturating_mul(unit),
        (info.f_bavail as u64).saturating_mul(unit),
    ))
}

/// Folders a disk walk never enters: on macOS the startup disk's own volumes are reached
/// again through `/System/Volumes` (the data volume is the one `/Users` already is), other
/// disks hang under `/Volumes`, and `/dev` is not files at all.
pub fn walk_skips(path: &std::path::Path) -> bool {
    cfg!(target_os = "macos")
        && ["/System/Volumes", "/Volumes", "/dev", "/private/var/vm"]
            .iter()
            .any(|skip| path == std::path::Path::new(skip))
}

/// A disk's top folder: `/`, or a mounted volume `/Volumes/<name>` on macOS.
pub fn is_volume_root(path: &std::path::Path) -> bool {
    use std::path::Component;
    let parts: Vec<Component> = path.components().collect();
    match parts.as_slice() {
        [Component::RootDir] => true,
        [Component::RootDir, Component::Normal(volumes), Component::Normal(_)] => {
            cfg!(target_os = "macos") && *volumes == "Volumes"
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn commands_lose_their_windows_extension_only_off_windows() {
        let expected = if cfg!(windows) { "git.exe" } else { "git" };
        assert_eq!(command_name("git.exe"), expected);
        assert_eq!(command_name("brew"), "brew");
        assert_eq!(command_name("node.js"), "node.js");
    }

    #[test]
    fn volume_roots_are_told_apart_from_folders_on_them() {
        assert!(is_volume_root(Path::new("/")));
        assert_eq!(
            is_volume_root(Path::new("/Volumes/Backup")),
            cfg!(target_os = "macos")
        );
        assert!(!is_volume_root(Path::new("/Volumes/Backup/work")));
        assert!(!is_volume_root(Path::new("/Users")));
    }
}
