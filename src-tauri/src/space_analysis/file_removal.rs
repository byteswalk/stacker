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
/// A folder that is never removed whole from here: a drive's root, the user's own folder or
/// one above it, or anything protected.
fn folder_off_limits(path: &Path, roots: &[PathBuf], home: Option<&Path>) -> bool {
    let p = normalized(path);
    // `normalized` drops the trailing backslash, so a drive's root is just `k:`.
    let drive_root = p.ends_with(':');
    let above_home = home.is_some_and(|home| {
        let home = normalized(home);
        home == p || home.starts_with(&format!("{p}\\"))
    });
    drive_root || p.matches('\\').count() == 0 || above_home || is_protected_with(path, roots)
}

fn remove_folder(
    path: &str,
    roots: &[PathBuf],
    home: Option<&Path>,
    permanent: bool,
) -> Result<(), RemovalFailure> {
    let fail = |reason: &str| RemovalFailure {
        path: path.to_string(),
        reason: reason.to_string(),
    };
    let folder = Path::new(path);
    if folder_off_limits(folder, roots, home) {
        return Err(fail("protected"));
    }
    let meta = std::fs::symlink_metadata(folder).map_err(|_| fail("missing"))?;
    if !meta.is_dir() {
        return Err(fail("changed"));
    }
    if permanent {
        std::fs::remove_dir_all(folder).map_err(|error| fail(&error.to_string()))
    } else {
        recycle(folder).map_err(|error| fail(&error))
    }
}

/// Removes whole folders (projects), to the Recycle Bin unless `permanent`. `bytes` is what the
/// scan counted for each, which is what the result reports as freed.
pub fn remove_folders(folders: &[FileToRemove], permanent: bool) -> RemovalResult {
    let roots = system_roots();
    let home = dirs::home_dir();
    let mut result = RemovalResult::default();
    let mut history = Vec::new();
    for folder in folders {
        let outcome = remove_folder(&folder.path, &roots, home.as_deref(), permanent);
        history.push(super::history::HistoryItem {
            path: folder.path.clone(),
            state: if outcome.is_ok() {
                CleanupItemState::Completed
            } else {
                CleanupItemState::Skipped
            },
            released_bytes: if outcome.is_ok() { folder.bytes } else { 0 },
            reason_key: Some(match &outcome {
                Ok(()) => if permanent { "deleted" } else { "recycled" }.into(),
                Err(failure) => failure.reason.clone(),
            }),
        });
        match outcome {
            Ok(()) => {
                result.removed += 1;
                result.released_bytes += folder.bytes;
            }
            Err(failure) => result.failures.push(failure),
        }
    }
    super::history::append_record(super::history::HistoryRecord {
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        state: if result.removed == 0 && !result.failures.is_empty() {
            CleanupTaskState::Failed
        } else {
            CleanupTaskState::Completed
        },
        released_bytes: result.released_bytes,
        items: history,
    });
    result
}

#[tauri::command]
pub async fn space_recycle_folders(
    folders: Vec<FileToRemove>,
    permanent: Option<bool>,
) -> Result<RemovalResult, String> {
    let permanent = permanent.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || remove_folders(&folders, permanent))
        .await
        .map_err(|e| e.to_string())
}

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
    fn a_whole_project_goes_but_never_a_drive_home_or_system_folder() {
        let home = PathBuf::from(r"C:\Users\me");
        let off = |path: &str| folder_off_limits(Path::new(path), &roots(), Some(&home));
        assert!(off(r"K:\"));
        assert!(off("K:"));
        assert!(off(r"C:\Users"));
        assert!(off(r"C:\Users\me"));
        assert!(off(r"C:\Program Files\App"));
        assert!(!off(r"K:\IdeaProjects\cctar-core"));
        assert!(!off(r"C:\Users\me\code\app"));

        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("app");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("src").join("main.rs"), b"fn main() {}").unwrap();
        let path = project.to_string_lossy().into_owned();
        assert!(remove_folder(&path, &[], None, true).is_ok());
        assert!(!project.exists());
        assert_eq!(
            remove_folder(&path, &[], None, true).unwrap_err().reason,
            "missing"
        );
        let file = dir.path().join("note.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            remove_folder(&file.to_string_lossy(), &[], None, true)
                .unwrap_err()
                .reason,
            "changed"
        );
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
