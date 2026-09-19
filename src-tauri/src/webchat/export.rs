//! `saveExport`: the extension's Markdown / JSON exports, written under `<exports>/web`.
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<site>/<file>` today; a little room for later layouts, never deep trees.
const MAX_PARTS: usize = 4;

fn has_export_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".json")
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
        .map(|p| crate::sessions::export::safe(p, 120))
        .collect();
    match cleaned.last() {
        Some(name) if has_export_extension(name) => Ok(cleaned),
        _ => Err("E_PATH".into()),
    }
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
}
