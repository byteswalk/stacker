//! Directory junctions, volume facts and a copy that rebuilds inner junctions.
use crate::space_analysis::walker::is_link_or_reparse_point;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p,
    }
}

pub fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| is_link_or_reparse_point(&m))
}

/// Target of a junction or symlink, without the `\\?\` prefix.
pub fn link_target(path: &Path) -> Option<PathBuf> {
    fs::read_link(path).ok().map(strip_verbatim)
}

/// True for directory reparse points (junctions and directory symlinks), which can be
/// recreated as junctions; file symlinks are false.
#[cfg(windows)]
pub fn is_junction(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    fs::symlink_metadata(path).is_ok_and(|m| {
        let attrs = m.file_attributes();
        attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 && attrs & FILE_ATTRIBUTE_DIRECTORY != 0
    })
}

#[cfg(not(windows))]
pub fn is_junction(_path: &Path) -> bool {
    false
}

/// `mklink /J` needs no elevation for junctions.
pub fn create_junction(link: &Path, target: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new("cmd.exe");
    cmd.args(["/d", "/c", "mklink", "/J"]).arg(link).arg(target);
    crate::sessions::codex_rpc::hidden(&mut cmd);
    let out = cmd.output().map_err(|_| "E_LINK_CREATE".to_string())?;
    if out.status.success() && is_junction(link) {
        Ok(())
    } else {
        Err("E_LINK_CREATE".into())
    }
}

/// Removes only the junction itself, never what it points to.
pub fn remove_junction(link: &Path) -> Result<(), String> {
    if !is_link(link) {
        return Err("E_NOT_LINK".into());
    }
    fs::remove_dir(link).map_err(|_| "E_LINK_REMOVE".to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeStats {
    pub files: u64,
    pub bytes: u64,
    pub junctions: u64,
}

/// Counts a tree without following links; symlinks that are not junctions are an error.
pub fn stats(root: &Path) -> Result<TreeStats, String> {
    let mut s = TreeStats::default();
    walk_stats(root, &mut s)?;
    Ok(s)
}

fn walk_stats(dir: &Path, s: &mut TreeStats) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|_| "E_ACCESS".to_string())? {
        let path = entry.map_err(|_| "E_ACCESS".to_string())?.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| "E_ACCESS".to_string())?;
        if is_link_or_reparse_point(&meta) {
            if is_junction(&path) {
                s.junctions += 1;
                continue;
            }
            return Err("E_LINK_INSIDE".into());
        }
        if meta.is_dir() {
            walk_stats(&path, s)?;
        } else {
            s.files += 1;
            s.bytes += meta.len();
        }
    }
    Ok(())
}

/// Where an inner junction should point once `from` has been copied to `to`.
pub fn remap(target: &Path, from: &Path, to: &Path) -> PathBuf {
    let lower = |p: &Path| p.to_string_lossy().to_lowercase();
    let (t, f) = (lower(target), lower(from));
    let f = f.trim_end_matches('\\');
    if t == f || t.starts_with(&format!("{f}\\")) {
        let rest = &target.to_string_lossy()[f.len()..];
        PathBuf::from(format!(
            "{}{}",
            to.to_string_lossy().trim_end_matches('\\'),
            rest
        ))
    } else {
        target.to_path_buf()
    }
}

/// Copies `from` into the new directory `to`, keeping modification times and rebuilding junctions.
pub fn copy_tree(
    from: &Path,
    to: &Path,
    copied: &AtomicU64,
    cancel: &crate::runner::CancelFlag,
) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|_| "E_STORAGE".to_string())?;
    copy_dir(from, from, to, to, copied, cancel)
}

fn copy_dir(
    root_from: &Path,
    dir: &Path,
    root_to: &Path,
    out: &Path,
    copied: &AtomicU64,
    cancel: &crate::runner::CancelFlag,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|_| "E_ACCESS".to_string())? {
        if cancel.is_cancelled() {
            return Err("E_CANCELLED".into());
        }
        let entry = entry.map_err(|_| "E_ACCESS".to_string())?;
        let path = entry.path();
        let dest = out.join(entry.file_name());
        let meta = fs::symlink_metadata(&path).map_err(|_| "E_ACCESS".to_string())?;
        if is_link_or_reparse_point(&meta) {
            if !is_junction(&path) {
                return Err("E_LINK_INSIDE".into());
            }
            let target = link_target(&path).ok_or("E_LINK_INSIDE")?;
            create_junction(&dest, &remap(&target, root_from, root_to))?;
            continue;
        }
        if meta.is_dir() {
            fs::create_dir(&dest).map_err(|_| "E_STORAGE".to_string())?;
            copy_dir(root_from, &path, root_to, &dest, copied, cancel)?;
        } else {
            fs::copy(&path, &dest).map_err(copy_error)?;
            if let Ok(modified) = meta.modified() {
                if let Ok(file) = fs::File::options().write(true).open(&dest) {
                    let _ = file.set_modified(modified);
                }
            }
            copied.fetch_add(meta.len(), Ordering::Relaxed);
        }
    }
    Ok(())
}

fn copy_error(e: io::Error) -> String {
    match e.raw_os_error() {
        Some(32) | Some(33) => "E_IN_USE".into(),
        Some(112) => "E_SPACE".into(),
        _ if e.kind() == io::ErrorKind::PermissionDenied => "E_ACCESS".into(),
        _ => "E_STORAGE".into(),
    }
}

/// Deletes a tree without following junctions (they are unlinked, not entered).
pub fn remove_tree(path: &Path) -> Result<(), String> {
    if is_link(path) {
        return remove_junction(path);
    }
    for entry in fs::read_dir(path).map_err(|_| "E_ACCESS".to_string())? {
        let p = entry.map_err(|_| "E_ACCESS".to_string())?.path();
        if is_link(&p) {
            fs::remove_dir(&p)
                .or_else(|_| fs::remove_file(&p))
                .map_err(|_| "E_ACCESS".to_string())?;
        } else if p.is_dir() {
            remove_tree(&p)?;
        } else {
            fs::remove_file(&p).map_err(copy_error)?;
        }
    }
    fs::remove_dir(path).map_err(|_| "E_ACCESS".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub root: String,
    pub file_system: String,
    pub free: u64,
    pub fixed: bool,
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

/// Volume holding `path` (the nearest existing ancestor is used).
#[cfg(windows)]
pub fn volume_of(path: &Path) -> Option<Volume> {
    use winapi::um::fileapi::GetVolumePathNameW;
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    let mut buf = [0u16; 260];
    let ok = unsafe {
        GetVolumePathNameW(
            wide(&probe.to_string_lossy()).as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(0);
    volume_info(&String::from_utf16_lossy(&buf[..len]))
}

#[cfg(windows)]
fn volume_info(root: &str) -> Option<Volume> {
    use winapi::shared::ntdef::ULARGE_INTEGER;
    use winapi::um::fileapi::{GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeInformationW};
    let root_w = wide(root);
    let mut fs_name = [0u16; 64];
    let ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = fs_name.iter().position(|c| *c == 0).unwrap_or(0);
    let mut free: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
    unsafe {
        GetDiskFreeSpaceExW(
            root_w.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    // DRIVE_FIXED == 3
    let fixed = unsafe { GetDriveTypeW(root_w.as_ptr()) } == 3;
    Some(Volume {
        root: root.to_string(),
        file_system: String::from_utf16_lossy(&fs_name[..len]),
        free: unsafe { *free.QuadPart() },
        fixed,
    })
}

/// Fixed NTFS drives other than the system drive, most free space first.
#[cfg(windows)]
pub fn other_drives() -> Vec<Volume> {
    use winapi::um::fileapi::GetLogicalDrives;
    let mask = unsafe { GetLogicalDrives() };
    let system = std::env::var("SystemDrive")
        .unwrap_or_else(|_| "C:".into())
        .to_uppercase();
    let mut list: Vec<Volume> = (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| !root.starts_with(&system))
        .filter_map(|root| volume_info(&root))
        .filter(|v| v.fixed && v.file_system.eq_ignore_ascii_case("NTFS"))
        .collect();
    list.sort_by_key(|v| std::cmp::Reverse(v.free));
    list
}

#[cfg(not(windows))]
pub fn volume_of(_path: &Path) -> Option<Volume> {
    None
}

#[cfg(not(windows))]
pub fn other_drives() -> Vec<Volume> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::CancelFlag;

    #[test]
    fn inner_junctions_are_rebuilt_inside_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("plugins").join("v1")).unwrap();
        fs::write(src.join("plugins").join("v1").join("a.txt"), b"hello").unwrap();
        fs::write(src.join("config.toml"), b"x=1").unwrap();
        create_junction(
            &src.join("plugins").join("latest"),
            &src.join("plugins").join("v1"),
        )
        .unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        create_junction(&src.join("ext"), &outside).unwrap();

        let before = stats(&src).unwrap();
        assert_eq!(
            before,
            TreeStats {
                files: 2,
                bytes: 8,
                junctions: 2
            }
        );

        let dst = dir.path().join("dst");
        let copied = AtomicU64::new(0);
        copy_tree(&src, &dst, &copied, &CancelFlag::default()).unwrap();
        assert_eq!(stats(&dst).unwrap(), before);
        assert_eq!(copied.load(Ordering::Relaxed), 8);
        let latest = link_target(&dst.join("plugins").join("latest")).unwrap();
        assert_eq!(latest, dst.join("plugins").join("v1"));
        assert_eq!(link_target(&dst.join("ext")).unwrap(), outside);
        assert_eq!(
            fs::read(dst.join("plugins").join("latest").join("a.txt")).unwrap(),
            b"hello"
        );

        remove_tree(&dst).unwrap();
        assert!(!dst.exists());
        assert!(
            src.join("plugins").join("v1").join("a.txt").exists(),
            "junction targets survive"
        );
        assert!(outside.exists());
    }

    #[test]
    fn removing_a_junction_keeps_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("f"), b"1").unwrap();
        let link = dir.path().join("l");
        create_junction(&link, &target).unwrap();
        assert!(is_junction(&link));
        remove_junction(&link).unwrap();
        assert!(!link.exists() && target.join("f").exists());
        assert_eq!(remove_junction(&target).unwrap_err(), "E_NOT_LINK");
    }

    #[test]
    fn remap_only_moves_inner_targets() {
        let from = Path::new(r"C:\Users\u\.codex");
        let to = Path::new(r"D:\AgentData\codex");
        assert_eq!(
            remap(Path::new(r"C:\Users\u\.codex\plugins\v1"), from, to),
            PathBuf::from(r"D:\AgentData\codex\plugins\v1")
        );
        assert_eq!(
            remap(Path::new(r"C:\Users\u\.codex-other"), from, to),
            PathBuf::from(r"C:\Users\u\.codex-other")
        );
        assert_eq!(remap(Path::new(r"E:\x"), from, to), PathBuf::from(r"E:\x"));
    }

    #[test]
    fn volumes_are_described() {
        let v = volume_of(&std::env::temp_dir()).unwrap();
        assert!(v.fixed && !v.file_system.is_empty());
    }
}
