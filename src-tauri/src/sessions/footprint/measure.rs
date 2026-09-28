//! Folder sizes that count a directory once, even when it is reachable through redirected roots.
use crate::space_analysis::walker::is_link_or_reparse_point;
use crate::space_analysis::windows_fs::{display_path, file_identity, FileIdentity};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Shared by the threads that walk each agent's folders: a directory counted by one of them
/// is not counted again by another.
#[derive(Default, Clone)]
pub struct Meter {
    seen: Arc<Mutex<HashSet<FileIdentity>>>,
    warnings: Arc<Mutex<Vec<String>>>,
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    fn first_visit(&self, id: FileIdentity) -> bool {
        self.seen.lock().map_or(true, |mut seen| seen.insert(id))
    }

    /// What the walk could not read, once the walk is over.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.lock().map(|w| w.clone()).unwrap_or_default()
    }

    /// Marks a root as visited; false when the same directory was already scanned under another path.
    pub fn claim(&self, root: &Path) -> bool {
        file_identity(root).map_or(true, |id| self.first_visit(id))
    }

    /// Returns `(bytes, files)` not yet counted by this meter. Links are never followed.
    pub fn measure(&self, path: &Path) -> (u64, u64) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return (0, 0);
        };
        if is_link_or_reparse_point(&meta) {
            return (0, 0);
        }
        if meta.is_file() {
            return (meta.len(), 1);
        }
        // Redirected roots (MSIX) alias whole directories, so directory identity is enough;
        // opening every file for its identity would make large trees slow.
        if let Ok(identity) = file_identity(path) {
            if !self.first_visit(identity) {
                return (0, 0);
            }
        }
        let Ok(entries) = fs::read_dir(path) else {
            if let Ok(mut warnings) = self.warnings.lock() {
                warnings.push(format!("{}: E_ACCESS", display_path(path)));
            }
            return (0, 0);
        };
        let mut total = (0, 0);
        for entry in entries.flatten() {
            let (bytes, files) = self.measure(&entry.path());
            total.0 += bytes;
            total.1 += files;
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_roots_count_once() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("f"), vec![0u8; 100]).unwrap();
        let meter = Meter::new();
        assert_eq!(meter.measure(&a), (100, 1));
        assert_eq!(meter.measure(&a), (0, 0), "a root seen twice counts once");
    }

    #[test]
    fn links_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("big"), vec![0u8; 1000]).unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let link = root.join("link");
        let created = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !created {
            return;
        }
        assert_eq!(Meter::new().measure(&root), (0, 0));
    }
}
