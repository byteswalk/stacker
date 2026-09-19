//! Folder sizes that count every file once, even across hard links and redirected roots.
use crate::space_analysis::walker::is_link_or_reparse_point;
use crate::space_analysis::windows_fs::{display_path, file_identity, FileIdentity};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Default)]
pub struct Meter {
    seen: HashSet<FileIdentity>,
    pub warnings: Vec<String>,
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `(bytes, files)` not yet counted by this meter. Links are never followed.
    pub fn measure(&mut self, path: &Path) -> (u64, u64) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return (0, 0);
        };
        if is_link_or_reparse_point(&meta) {
            return (0, 0);
        }
        if let Ok(identity) = file_identity(path) {
            if !self.seen.insert(identity) {
                return (0, 0);
            }
        }
        if meta.is_file() {
            return (meta.len(), 1);
        }
        let Ok(entries) = fs::read_dir(path) else {
            self.warnings
                .push(format!("{}: E_ACCESS", display_path(path)));
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
    fn hard_links_and_repeated_roots_count_once() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("f"), vec![0u8; 100]).unwrap();
        fs::hard_link(a.join("f"), a.join("g")).unwrap();
        let mut meter = Meter::new();
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
