//! Removing what a low-space cleanup chose: as much as can go, past files a program holds
//! open. Links and junctions are removed as links, never followed, and nothing is removed
//! that resolves outside the folder being cleaned.

use super::model::Removal;
use crate::space_analysis::walker::is_link_or_reparse_point;
use crate::space_analysis::windows_fs::{allocated_size, file_link_count};
use std::fs;
use std::path::Path;

pub const REASON_MISSING: &str = "spaceAnalysis.cleanup.reason.missing";
pub const REASON_LINK: &str = "spaceAnalysis.cleanup.reason.linkDetected";
pub const REASON_ACCESS_DENIED: &str = "spaceAnalysis.cleanup.reason.accessDenied";
pub const REASON_DELETE_FAILED: &str = "spaceAnalysis.cleanup.reason.deleteFailed";

/// The bytes freed, and why something stayed when it did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub bytes: u64,
    pub reason: Option<&'static str>,
}

impl Removed {
    fn add(&mut self, other: Removed) {
        self.bytes += other.bytes;
        if self.reason.is_none() {
            self.reason = other.reason;
        }
    }
}

fn reason_of(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => REASON_MISSING,
        std::io::ErrorKind::PermissionDenied => REASON_ACCESS_DENIED,
        _ => REASON_DELETE_FAILED,
    }
}

fn remove_file(path: &Path, meta: &fs::Metadata) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                && meta.permissions().readonly() =>
        {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
            fs::remove_file(path)
        }
        result => result,
    }
}

/// A file's bytes when removing it frees them: a hard link shared with another name frees
/// nothing.
fn freed_by(path: &Path, meta: &fs::Metadata) -> u64 {
    if file_link_count(path).unwrap_or(1) <= 1 {
        allocated_size(path, meta)
    } else {
        0
    }
}

fn empty_folder(dir: &Path, root: &Path, removed: &mut Removed) {
    let stays_inside = fs::canonicalize(dir).is_ok_and(|real| real.starts_with(root));
    if !stays_inside {
        removed.add(Removed {
            bytes: 0,
            reason: Some(REASON_LINK),
        });
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            removed.add(Removed {
                bytes: 0,
                reason: Some(reason_of(&error)),
            });
            return;
        }
    };
    for path in entries.flatten().map(|e| e.path()) {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if is_link_or_reparse_point(&meta) {
            // A symlink or junction goes as a link. Other reparse points (cloud placeholders,
            // deduplicated files) are left alone.
            let unlinked = meta.file_type().is_symlink()
                && fs::remove_dir(&path)
                    .or_else(|_| fs::remove_file(&path))
                    .is_ok();
            if !unlinked {
                removed.add(Removed {
                    bytes: 0,
                    reason: Some(REASON_LINK),
                });
            }
            continue;
        }
        if meta.is_dir() {
            empty_folder(&path, root, removed);
            // A folder something stayed in cannot go, and that was already noted.
            let _ = fs::remove_dir(&path);
            continue;
        }
        let bytes = freed_by(&path, &meta);
        match remove_file(&path, &meta) {
            Ok(()) => removed.bytes += bytes,
            Err(error) => removed.add(Removed {
                bytes: 0,
                reason: Some(reason_of(&error)),
            }),
        }
    }
}

/// Removes a file or folder, or only what a folder holds, as far as it can.
pub fn remove_path(path: &Path, whole: bool) -> Removed {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) => {
            return Removed {
                bytes: 0,
                reason: Some(reason_of(&error)),
            }
        }
    };
    if is_link_or_reparse_point(&meta) {
        return Removed {
            bytes: 0,
            reason: Some(REASON_LINK),
        };
    }
    if !meta.is_dir() {
        let bytes = freed_by(path, &meta);
        return match remove_file(path, &meta) {
            Ok(()) => Removed {
                bytes,
                reason: None,
            },
            Err(error) => Removed {
                bytes: 0,
                reason: Some(reason_of(&error)),
            },
        };
    }
    let Ok(root) = fs::canonicalize(path) else {
        return Removed {
            bytes: 0,
            reason: Some(REASON_MISSING),
        };
    };
    let mut removed = Removed::default();
    empty_folder(path, &root, &mut removed);
    if whole && fs::remove_dir(path).is_err() && removed.reason.is_none() {
        removed.reason = Some(REASON_DELETE_FAILED);
    }
    removed
}

/// Removes a candidate the way it says. A temporary folder's entries are judged again one by
/// one, so one written to since it was measured stays.
pub fn remove(path: &Path, removal: Removal, may_go: &dyn Fn(&Path, u64) -> bool) -> Removed {
    match removal {
        Removal::Whole => remove_path(path, true),
        Removal::Contents => remove_path(path, false),
        Removal::OldEntries(_) => {
            let mut removed = Removed::default();
            let Ok(entries) = fs::read_dir(path) else {
                return Removed {
                    bytes: 0,
                    reason: Some(REASON_ACCESS_DENIED),
                };
            };
            for entry in entries.flatten().map(|e| e.path()) {
                let (_, _, newest) = crate::project_junk::size_of(&entry);
                if may_go(&entry, newest) {
                    removed.add(remove_path(&entry, true));
                }
            }
            removed
        }
    }
}
