//! `saveExport`: the extension's Markdown / JSON exports, written under `<exports>/web`.
use crate::space_analysis::walker::is_link_or_reparse_point;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<site>/<file>` today; a little room for later layouts, never deep trees.
const MAX_PARTS: usize = 4;

/// Windows device names, reserved whatever the extension; same list as `bodies::component`.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn has_export_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".json")
}

/// A reserved device name (`CON`, `NUL`, `COM1`, …) would refer to the device, not a file,
/// whatever follows the first dot; prefix it away, as `bodies::component` does.
fn avoid_reserved(name: String) -> String {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        format!("_{name}")
    } else {
        name
    }
}

/// Checks a relative path from the extension and returns its cleaned components.
pub fn safe_parts(rel: &str) -> Result<Vec<String>, String> {
    let parts: Vec<&str> = rel.split(['/', '\\']).collect();
    if parts.len() > MAX_PARTS
        || parts
            .iter()
            .any(|p| p.trim().is_empty() || *p == "." || *p == ".." || p.contains(':'))
    {
        return Err("E_PATH".into());
    }
    let cleaned: Vec<String> = parts
        .iter()
        .map(|p| avoid_reserved(crate::sessions::export::safe(p, 120)))
        .collect();
    match cleaned.last() {
        Some(name) if has_export_extension(name) => Ok(cleaned),
        _ => Err("E_PATH".into()),
    }
}

/// True if `path` (or anything it inherits from an already-existing ancestor between
/// `base` and `path`) is a junction or symlink: a way to point `path` outside `base`
/// without any `..` ever appearing in the request.
fn escapes_through_a_link(base: &Path, path: &Path) -> bool {
    let is_link =
        |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| is_link_or_reparse_point(&m));
    if is_link(base) {
        return true;
    }
    let Ok(rel) = path.strip_prefix(base) else {
        return true;
    };
    let mut cur = base.to_path_buf();
    for component in rel.components() {
        cur.push(component);
        if is_link(&cur) {
            return true;
        }
    }
    false
}

#[derive(Debug)]
pub struct Saved {
    /// Relative to `<exports>/web`, `/`-separated; the extension appends to this path.
    pub path: String,
    pub full_path: PathBuf,
}

/// `name`, or `name (1)`, `name (2)`… when taken, like the browser's download folder.
fn create_unused(dir: &Path, name: &str) -> Result<(PathBuf, std::fs::File), String> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 0..1000 {
        let candidate = match (n, ext.is_empty()) {
            (0, _) => name.to_string(),
            (_, true) => format!("{stem} ({n})"),
            (_, false) => format!("{stem} ({n}).{ext}"),
        };
        let path = dir.join(candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("E_STORAGE".into()),
        }
    }
    Err("E_STORAGE".into())
}

pub fn save_export(
    exports: &Path,
    rel: &str,
    text: &str,
    append: bool,
    written: &mut HashSet<PathBuf>,
) -> Result<Saved, String> {
    let parts = safe_parts(rel)?;
    let base = exports.join("web");
    let (name, dirs) = parts.split_last().ok_or("E_PATH")?;
    let dir = dirs.iter().fold(base.clone(), |d, p| d.join(p));
    std::fs::create_dir_all(&dir).map_err(|_| "E_STORAGE".to_string())?;
    if escapes_through_a_link(&base, &dir) {
        return Err("E_PATH".into());
    }
    // Belt and braces: a junction that `symlink_metadata` did not catch (or one introduced
    // between the checks above and here) still cannot get past a canonicalized-prefix check.
    let canonical_base = std::fs::canonicalize(&base).map_err(|_| "E_STORAGE".to_string())?;
    let canonical_dir = std::fs::canonicalize(&dir).map_err(|_| "E_STORAGE".to_string())?;
    if !canonical_dir.starts_with(&canonical_base) {
        return Err("E_PATH".into());
    }
    let (full_path, mut file) = if append {
        let path = dir.join(name);
        if !written.contains(&path) {
            return Err("E_PATH".into());
        }
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|_| "E_STORAGE".to_string())?;
        (path, file)
    } else {
        create_unused(&dir, name)?
    };
    file.write_all(text.as_bytes())
        .map_err(|_| "E_STORAGE".to_string())?;
    written.insert(full_path.clone());
    let path = full_path
        .strip_prefix(&base)
        .map_err(|_| "E_PATH".to_string())?
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Ok(Saved { path, full_path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_and_odd_paths_are_refused() {
        for bad in [
            "",
            "a.md/",
            "/abs.md",
            "../a.md",
            "a/../../b.md",
            "./a.md",
            "C:\\x.md",
            "C:x.md",
            "a/b/c/d/e.md",
            "run.exe",
            "notes.txt",
            "a\\..\\b.md",
        ] {
            assert_eq!(safe_parts(bad).unwrap_err(), "E_PATH", "{bad}");
        }
        assert_eq!(
            safe_parts("chatgpt\\2026-09-19 a*b? id.md").unwrap(),
            vec!["chatgpt".to_string(), "2026-09-19 a_b_ id.md".to_string()]
        );
    }

    #[test]
    fn writes_under_the_web_folder_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("exports");
        let mut written = HashSet::new();
        let first = save_export(&exports, "chatgpt/a.md", "one", false, &mut written).unwrap();
        let second = save_export(&exports, "chatgpt/a.md", "two", false, &mut written).unwrap();
        assert_eq!(first.path, "chatgpt/a.md");
        assert_eq!(second.path, "chatgpt/a (1).md");
        assert!(first.full_path.starts_with(exports.join("web")));
        assert_eq!(std::fs::read_to_string(&first.full_path).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(&second.full_path).unwrap(), "two");
    }

    #[test]
    fn appends_only_to_files_this_bridge_created() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("exports");
        let mut written = HashSet::new();
        let saved =
            save_export(&exports, "claude/long.md", "part 1,", false, &mut written).unwrap();
        save_export(&exports, &saved.path, " part 2", true, &mut written).unwrap();
        assert_eq!(
            std::fs::read_to_string(&saved.full_path).unwrap(),
            "part 1, part 2"
        );
        std::fs::create_dir_all(exports.join("web").join("claude")).unwrap();
        std::fs::write(exports.join("web").join("claude").join("other.md"), "keep").unwrap();
        let err = save_export(&exports, "claude/other.md", "x", true, &mut written).unwrap_err();
        assert_eq!(err, "E_PATH");
        let other =
            std::fs::read_to_string(exports.join("web").join("claude").join("other.md")).unwrap();
        assert_eq!(other, "keep");
    }

    #[test]
    fn reserved_device_names_are_prefixed_in_any_segment() {
        assert_eq!(
            safe_parts("con/notes.md").unwrap(),
            vec!["_con".to_string(), "notes.md".to_string()]
        );
        assert_eq!(
            safe_parts("chatgpt/NUL.md").unwrap(),
            vec!["chatgpt".to_string(), "_NUL.md".to_string()]
        );
        assert_eq!(
            safe_parts("chatgpt/com1.json").unwrap(),
            vec!["chatgpt".to_string(), "_com1.json".to_string()]
        );
    }

    /// Creates a directory junction (falling back to a directory symlink); `None` if
    /// this environment allows neither, in which case the caller skips the test.
    fn link_dir(link: &Path, target: &Path) -> Option<()> {
        #[cfg(windows)]
        {
            let mut cmd = std::process::Command::new("cmd.exe");
            cmd.args(["/d", "/c", "mklink", "/J"]).arg(link).arg(target);
            if cmd.output().is_ok_and(|o| o.status.success()) {
                return Some(());
            }
            std::os::windows::fs::symlink_dir(target, link).ok()
        }
        #[cfg(not(windows))]
        {
            std::os::unix::fs::symlink(target, link).ok()
        }
    }

    #[test]
    fn a_junction_already_inside_the_export_folder_cannot_be_used_to_escape_it() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("exports");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(exports.join("web")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let link = exports.join("web").join("escape");
        if link_dir(&link, &outside).is_none() {
            eprintln!("skipping: this environment allows neither junctions nor symlinks");
            return;
        }
        let mut written = HashSet::new();
        let err = save_export(&exports, "escape/x.md", "data", false, &mut written).unwrap_err();
        assert_eq!(err, "E_PATH");
        assert!(!outside.join("x.md").exists());
    }
}
