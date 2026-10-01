//! Large and duplicate files the user picks, moved to the Recycle Bin.
//!
//! These are the user's own files, not caches Stacker recognises, so the cleanup plans (which
//! prove a folder is a known artifact before touching it) do not apply. What applies instead:
//! the files go to the Recycle Bin, where a mistake can be undone; nothing under Windows or
//! a program's install folder is touched, since a duplicate there is usually a file Windows
//! or the program needs in both places; and each file must still be the one the scan saw.

use super::model::{CleanupItemState, CleanupTaskState};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileToRemove {
    pub path: String,
    /// The size the scan saw; a file that has changed since is left alone.
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalFailure {
    pub path: String,
    /// `protected`, `changed`, `missing`, or the Recycle Bin's own refusal.
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalResult {
    pub removed: usize,
    pub released_bytes: u64,
    pub failures: Vec<RemovalFailure>,
}

fn system_roots() -> Vec<PathBuf> {
    [
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
    ]
    .iter()
    .filter_map(std::env::var_os)
    .map(PathBuf::from)
    .collect()
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Under Windows, a program's install folder, or a volume's own bookkeeping folders.
pub fn is_protected_with(path: &Path, roots: &[PathBuf]) -> bool {
    let p = normalized(path);
    let under = |root: &str| p == root || p.starts_with(&format!("{root}\\"));
    roots.iter().any(|root| under(&normalized(root)))
        || p.contains("\\$recycle.bin\\")
        || p.contains("\\system volume information\\")
}

/// Which of these paths may be offered for removal at all.
#[tauri::command]
pub fn space_protected_paths(paths: Vec<String>) -> Vec<bool> {
    let roots = system_roots();
    paths
        .iter()
        .map(|path| is_protected_with(Path::new(path), &roots))
        .collect()
}

#[cfg(windows)]
fn recycle(path: &Path) -> Result<(), String> {
    crate::python_env::recycle(path)
}

#[cfg(not(windows))]
fn recycle(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

fn remove_one(
    file: &FileToRemove,
    roots: &[PathBuf],
    permanent: bool,
) -> Result<u64, RemovalFailure> {
    let path = Path::new(&file.path);
    let fail = |reason: &str| RemovalFailure {
        path: file.path.clone(),
        reason: reason.to_string(),
    };
    if is_protected_with(path, roots) {
        return Err(fail("protected"));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| fail("missing"))?;
    if !meta.is_file() || meta.len() != file.bytes {
        return Err(fail("changed"));
    }
    if permanent {
        std::fs::remove_file(path).map_err(|error| fail(&error.to_string()))?;
    } else {
        recycle(path).map_err(|error| fail(&error))?;
    }
    Ok(meta.len())
}

pub fn remove(files: &[FileToRemove], permanent: bool) -> RemovalResult {
    let roots = system_roots();
    let mut result = RemovalResult::default();
    let mut history = Vec::new();
    for file in files {
        match remove_one(file, &roots, permanent) {
            Ok(bytes) => {
                result.removed += 1;
                result.released_bytes += bytes;
                history.push(super::history::HistoryItem {
                    path: file.path.clone(),
                    state: CleanupItemState::Completed,
                    released_bytes: bytes,
                    reason_key: Some(if permanent { "deleted" } else { "recycled" }.into()),
                });
            }
            Err(failure) => {
                history.push(super::history::HistoryItem {
                    path: file.path.clone(),
                    state: CleanupItemState::Skipped,
                    released_bytes: 0,
                    reason_key: Some(failure.reason.clone()),
                });
                result.failures.push(failure);
            }
        }
    }
    // Nothing moved at all is a failed run; anything moved is a finished one, skips and all.
    let state = if result.removed == 0 && !result.failures.is_empty() {
        CleanupTaskState::Failed
    } else {
        CleanupTaskState::Completed
    };
    super::history::append_record(super::history::HistoryRecord {
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        state,
        released_bytes: result.released_bytes,
        items: history,
    });
    result
}

/// Moves the picked files to the Recycle Bin, or deletes them outright when asked: the bin
/// keeps the space until it is emptied.
#[tauri::command]
pub async fn space_recycle_files(
    files: Vec<FileToRemove>,
    permanent: Option<bool>,
) -> Result<RemovalResult, String> {
    let permanent = permanent.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || remove(&files, permanent))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from(r"C:\Windows"),
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\Program Files (x86)"),
            PathBuf::from(r"C:\ProgramData"),
        ]
    }

    #[test]
    fn system_and_program_folders_are_off_limits() {
        for path in [
            r"C:\Windows\Installer\471dfe.msi",
            r"C:\Program Files (x86)\Microsoft\EdgeCore\msedge.dll",
            r"c:\program files\Quark\app.asar",
            r"C:\ProgramData\Package Cache\x.msi",
            r"D:\$Recycle.Bin\S-1\file",
        ] {
            assert!(is_protected_with(Path::new(path), &roots()), "{path}");
        }
        for path in [
            r"C:\Users\me\Downloads\setup.exe",
            r"C:\Windowsy\file",
            r"D:\Program Files Backup\file",
        ] {
            assert!(!is_protected_with(Path::new(path), &roots()), "{path}");
        }
    }

    #[test]
    fn a_file_that_changed_or_went_away_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"12345").unwrap();
        let file = |bytes| FileToRemove {
            path: path.to_string_lossy().into_owned(),
            bytes,
        };
        assert_eq!(
            remove_one(&file(4), &[], true).unwrap_err().reason,
            "changed"
        );
        let gone = FileToRemove {
            path: dir.path().join("gone").to_string_lossy().into_owned(),
            bytes: 1,
        };
        assert_eq!(remove_one(&gone, &[], true).unwrap_err().reason, "missing");
        assert!(path.exists());
        // The right size goes.
        assert_eq!(remove_one(&file(5), &[], true).unwrap(), 5);
        assert!(!path.exists());
    }
}
