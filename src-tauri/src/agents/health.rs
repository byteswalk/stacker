//! Decides whether a detected CLI entry can actually run, instead of trusting that a file
//! exists. npm can leave a shell-script placeholder named `claude.exe` behind when the
//! native optional dependency fails to install.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallInfo {
    pub path: String,
    pub healthy: bool,
    pub version: Option<String>,
    pub reason: Option<String>,
}

/// Real PE files start with the `MZ` DOS header; npm placeholders do not.
pub(crate) fn has_pe_header(path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 2];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header == b"MZ"
}

/// Every entry point on PATH in order, taking the first matching candidate per directory
/// (a directory with `tool.cmd` and `tool.ps1` is one install, not two).
pub(crate) fn enumerate_candidates(dirs: &[PathBuf], candidates: &[&str]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for dir in dirs {
        let key = dir.to_string_lossy().trim_end_matches('\\').to_lowercase();
        if key.contains("\\windowsapps") || !seen.insert(key) {
            continue;
        }
        if let Some(path) = candidates
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
        {
            found.push(path);
        }
    }
    found
}

pub(crate) fn check_install(
    path: &Path,
    probe: &dyn Fn(&Path) -> Result<String, String>,
) -> InstallInfo {
    let display = path.to_string_lossy().into_owned();
    let is_exe = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if is_exe && !has_pe_header(path) {
        return InstallInfo {
            path: display,
            healthy: false,
            version: None,
            reason: Some("不是有效的 Windows 程序（可能是安装失败留下的占位文件）".into()),
        };
    }
    match probe(path) {
        Ok(version) => InstallInfo {
            path: display,
            healthy: true,
            version: Some(version),
            reason: None,
        },
        Err(reason) => InstallInfo {
            path: display,
            healthy: false,
            version: None,
            reason: Some(reason),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exe_without_mz_header_is_broken_without_running_it() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("claude.exe");
        fs::write(&exe, b"#!/bin/sh\necho native binary not installed\n").unwrap();
        let info = check_install(&exe, &|_| panic!("must not execute a non-PE file"));
        assert!(!info.healthy);
        assert!(info.reason.unwrap().contains("不是有效的 Windows 程序"));
    }

    #[test]
    fn exe_with_mz_header_is_probed() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool.exe");
        fs::write(&exe, b"MZ\x90\x00").unwrap();
        let info = check_install(&exe, &|_| Ok("1.2.3".into()));
        assert!(info.healthy);
        assert_eq!(info.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn failed_probe_is_broken_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = dir.path().join("tool.cmd");
        fs::write(&cmd, b"@echo off").unwrap();
        let info = check_install(&cmd, &|_| Err("不支持的 16 位应用程序".into()));
        assert!(!info.healthy);
        assert_eq!(info.reason.as_deref(), Some("不支持的 16 位应用程序"));
    }

    #[test]
    fn candidates_follow_path_order_one_per_directory() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for dir in [first.path(), second.path()] {
            fs::write(dir.join("tool.cmd"), b"").unwrap();
            fs::write(dir.join("tool.ps1"), b"").unwrap();
        }
        let found = enumerate_candidates(
            &[
                first.path().to_path_buf(),
                second.path().to_path_buf(),
                first.path().to_path_buf(),
            ],
            &["tool.cmd", "tool.ps1"],
        );
        assert_eq!(
            found,
            vec![
                first.path().join("tool.cmd"),
                second.path().join("tool.cmd")
            ]
        );
    }
}
