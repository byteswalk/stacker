//! What a project folder keeps that can be made again: dependency folders, build output,
//! tool caches, and the packages of releases that have been superseded. Nothing here is read
//! from the project's source — only folder names, the files that say what kind of project it
//! is, and sizes. No agent is involved; these are rules.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// How deep a project is walked. Build output sits at the root or one or two levels in
/// (`packages/*/dist`, `src-tauri/target`), and stopping there keeps the scan quick.
const MAX_DEPTH: usize = 4;
const DAY: u64 = 86_400;
const LOG_KEEP_DAYS: u64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JunkKind {
    /// Made again by a build or an install.
    Rebuildable,
    /// Packages of versions that have been superseded.
    Releases,
    /// Caches and temporary files.
    Cache,
    /// A virtual environment: rebuildable, but rebuilding it downloads everything again.
    Environment,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkItem {
    /// The path relative to the project, which is also what a cleanup names.
    pub id: String,
    pub kind: JunkKind,
    pub label: String,
    pub explain: String,
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    /// Ticked by default: provably rebuildable, with nothing of the user's own inside.
    pub recommended: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkReport {
    pub root: String,
    pub items: Vec<JunkItem>,
    pub total: u64,
    /// What the ticked items add up to.
    pub recommended: u64,
    pub scanned_at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkCleaned {
    pub removed: Vec<String>,
    pub failed: Vec<String>,
    pub bytes: u64,
}

struct Rule {
    kind: JunkKind,
    label: &'static str,
    explain: &'static str,
    recommended: bool,
    /// A file that must sit next to the folder for the rule to apply; empty means always.
    evidence: &'static [&'static str],
    /// A file that must sit inside the folder itself, for folders that say what they are.
    contains: &'static [&'static str],
}

/// What a folder of this name is, when the folder beside it says the project is of that kind.
fn rule_for(name: &str) -> Option<Rule> {
    let rule = |kind, label, explain, recommended, evidence| {
        Some(Rule {
            kind,
            label,
            explain,
            recommended,
            evidence,
            contains: &[],
        })
    };
    use JunkKind::*;
    match name {
        "node_modules" => rule(
            Rebuildable,
            "依赖目录 node_modules",
            "npm / pnpm / yarn 安装的依赖，重新安装即可恢复。",
            true,
            &["package.json"],
        ),
        "target" => rule(
            Rebuildable,
            "构建产物 target",
            "Rust 或 Maven 的编译输出，重新编译即可恢复。",
            true,
            &["Cargo.toml", "pom.xml"],
        ),
        "dist" | "build" | "out" | ".next" | ".nuxt" | ".svelte-kit" | ".output" => rule(
            Rebuildable,
            "构建输出",
            "打包生成的文件，重新构建即可恢复。",
            true,
            &[
                "package.json",
                "pom.xml",
                "build.gradle",
                "build.gradle.kts",
            ],
        ),
        ".gradle" => rule(
            Rebuildable,
            "Gradle 项目缓存",
            "Gradle 为这个项目缓存的依赖与任务结果，下次构建会重建。",
            true,
            &[],
        ),
        "bin" | "obj" => rule(
            Rebuildable,
            ".NET 编译输出",
            "dotnet build 的输出目录，重新编译即可恢复。",
            true,
            &["*.csproj", "*.sln", "*.fsproj", "*.vbproj"],
        ),
        "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".ipynb_checkpoints" => {
            rule(
                Cache,
                "Python 缓存",
                "解释器与工具生成的缓存，删除后自动重建。",
                true,
                &[],
            )
        }
        ".turbo" | ".parcel-cache" | ".vite" | ".cache" | ".eslintcache" | "coverage" => rule(
            Cache,
            "工具缓存",
            "构建与测试工具的缓存，删除后自动重建。",
            true,
            &[],
        ),
        // A virtual environment says so from the inside.
        ".venv" | "venv" | "env" => Some(Rule {
            kind: Environment,
            label: "Python 虚拟环境",
            explain: "重建需要重新下载全部依赖包，确认不再用这个环境后再删。",
            recommended: false,
            evidence: &[],
            contains: &["pyvenv.cfg"],
        }),
        _ => None,
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return true;
                }
            }
            meta.file_type().is_symlink()
        })
        .unwrap_or(false)
}

fn size_of(path: &Path) -> (u64, u64) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return (0, 0);
    };
    if is_link(path) {
        return (0, 0);
    }
    if meta.is_file() {
        return (meta.len(), 1);
    }
    let Ok(entries) = fs::read_dir(path) else {
        return (0, 0);
    };
    let mut total = (0, 0);
    for entry in entries.flatten() {
        let (bytes, files) = size_of(&entry.path());
        total.0 += bytes;
        total.1 += files;
    }
    total
}

/// Whether one of the named files sits in `dir`; `*.ext` matches by extension.
fn has_evidence(dir: &Path, evidence: &[&str]) -> bool {
    if evidence.is_empty() {
        return true;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let names: Vec<String> = entries
        .flatten()
        .map(|e| name_of(&e.path()).to_lowercase())
        .collect();
    evidence
        .iter()
        .any(|wanted| match wanted.strip_prefix("*.") {
            Some(ext) => names.iter().any(|n| n.ends_with(&format!(".{ext}"))),
            None => names.iter().any(|n| n == &wanted.to_lowercase()),
        })
}

fn age_days(path: &Path, now: u64) -> u64 {
    fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| now.saturating_sub(d.as_secs()) / DAY)
        .unwrap_or(0)
}

/// A folder of released packages: subfolders whose names start with a version. The newest
/// is what the project ships now, so only the ones behind it are offered.
fn superseded_releases(dir: &Path) -> Vec<PathBuf> {
    let mut versions: Vec<(Vec<u64>, PathBuf)> = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    for path in entries.flatten().map(|e| e.path()) {
        if !path.is_dir() || is_link(&path) {
            continue;
        }
        if let Some(version) = version_of(&name_of(&path)) {
            versions.push((version, path));
        }
    }
    if versions.len() < 2 {
        return Vec::new();
    }
    versions.sort();
    versions.pop();
    versions.into_iter().map(|(_, path)| path).collect()
}

/// "v0.3.4-r22" → [0, 3, 4, 22]; a name that does not start with a number is not a version.
fn version_of(name: &str) -> Option<Vec<u64>> {
    let rest = name.trim_start_matches(['v', 'V']);
    if !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<u64> = rest
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    (!parts.is_empty()).then_some(parts)
}

fn item(root: &Path, path: &Path, rule: &Rule) -> Option<JunkItem> {
    let (bytes, files) = size_of(path);
    if bytes == 0 {
        return None;
    }
    let id = path
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some(JunkItem {
        id,
        kind: rule.kind,
        label: rule.label.to_string(),
        explain: rule.explain.to_string(),
        path: path.to_string_lossy().into_owned(),
        bytes,
        files,
        recommended: rule.recommended,
    })
}

fn walk(root: &Path, dir: &Path, depth: usize, now: u64, out: &mut Vec<JunkItem>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut old_logs: Vec<PathBuf> = Vec::new();
    for path in entries.flatten().map(|e| e.path()) {
        if is_link(&path) {
            continue;
        }
        let name = name_of(&path);
        if path.is_file() {
            // A log the project stopped writing to a week ago is not being read now.
            if name.to_lowercase().ends_with(".log") && age_days(&path, now) > LOG_KEEP_DAYS {
                old_logs.push(path);
            }
            continue;
        }
        // Source control and editor state are never touched.
        if name == ".git" || name == ".svn" || name == ".hg" || name == ".idea" || name == ".vscode"
        {
            continue;
        }
        if name.ends_with(".egg-info") {
            if let Some(found) = item(
                root,
                &path,
                &Rule {
                    kind: JunkKind::Rebuildable,
                    label: "Python 打包元数据",
                    explain: "打包时生成的 egg-info 目录，重新打包即可恢复。",
                    recommended: true,
                    evidence: &[],
                    contains: &[],
                },
            ) {
                out.push(found);
            }
            continue;
        }
        if let Some(rule) = rule_for(&name)
            .filter(|rule| has_evidence(dir, rule.evidence) && has_evidence(&path, rule.contains))
        {
            if let Some(found) = item(root, &path, &rule) {
                out.push(found);
            }
            // What is inside a matched folder belongs to it.
            continue;
        }
        if name == "release" || name == "releases" {
            for old in superseded_releases(&path) {
                if let Some(mut found) = item(
                    root,
                    &old,
                    &Rule {
                        kind: JunkKind::Releases,
                        label: "历史发布版本",
                        explain: "更早版本的发布包，最新的一个版本会保留。",
                        recommended: false,
                        evidence: &[],
                        contains: &[],
                    },
                ) {
                    found.label = format!("历史发布版本 {}", name_of(&old));
                    out.push(found);
                }
            }
            continue;
        }
        if depth + 1 < MAX_DEPTH {
            walk(root, &path, depth + 1, now, out);
        }
    }
    if !old_logs.is_empty() {
        let bytes: u64 = old_logs.iter().map(|p| size_of(p).0).sum();
        if bytes > 0 {
            let id = dir
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            out.push(JunkItem {
                id: if id.is_empty() {
                    "*.log".into()
                } else {
                    format!("{id}/*.log")
                },
                kind: JunkKind::Cache,
                label: "7 天前的日志文件".into(),
                explain: "项目目录里的旧日志文件。".into(),
                path: dir.to_string_lossy().into_owned(),
                bytes,
                files: old_logs.len() as u64,
                recommended: true,
            });
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn scan(root: &Path) -> Result<JunkReport, String> {
    if !root.is_dir() {
        return Err("E_PROJECT_MISSING".into());
    }
    let now = now_secs();
    let mut items = Vec::new();
    walk(root, root, 0, now, &mut items);
    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    Ok(JunkReport {
        root: root.to_string_lossy().into_owned(),
        total: items.iter().map(|i| i.bytes).sum(),
        recommended: items
            .iter()
            .filter(|i| i.recommended)
            .map(|i| i.bytes)
            .sum(),
        items,
        scanned_at: now,
    })
}

/// Deletes the named items after checking each one again: still inside the project, still
/// something a rule recognises, and not a link pointing somewhere else.
pub fn clean(root: &Path, ids: &[String]) -> Result<JunkCleaned, String> {
    let report = scan(root)?;
    let mut cleaned = JunkCleaned {
        removed: Vec::new(),
        failed: Vec::new(),
        bytes: 0,
    };
    for id in ids {
        let Some(found) = report.items.iter().find(|i| &i.id == id) else {
            cleaned.failed.push(id.clone());
            continue;
        };
        let path = PathBuf::from(&found.path);
        if !path.starts_with(root) || is_link(&path) {
            cleaned.failed.push(id.clone());
            continue;
        }
        let outcome = if id.ends_with("/*.log") || id == "*.log" {
            remove_old_logs(&path)
        } else if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        match outcome {
            Ok(()) => {
                cleaned.bytes += found.bytes;
                cleaned.removed.push(id.clone());
            }
            Err(_) => cleaned.failed.push(id.clone()),
        }
    }
    Ok(cleaned)
}

fn remove_old_logs(dir: &Path) -> std::io::Result<()> {
    let now = now_secs();
    for path in fs::read_dir(dir)?.flatten().map(|e| e.path()) {
        let name = name_of(&path).to_lowercase();
        if path.is_file()
            && name.ends_with(".log")
            && age_days(&path, now) > LOG_KEEP_DAYS
            && !is_link(&path)
        {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn project_junk_scan(path: String) -> Result<JunkReport, String> {
    tauri::async_runtime::spawn_blocking(move || scan(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn project_junk_clean(path: String, ids: Vec<String>) -> Result<JunkCleaned, String> {
    tauri::async_runtime::spawn_blocking(move || clean(Path::new(&path), &ids))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn a_node_project_offers_its_dependencies_and_build_output() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("package.json"), 40);
        write(
            &root.join("node_modules").join("left-pad").join("index.js"),
            900,
        );
        write(&root.join("dist").join("app.js"), 500);
        write(&root.join("src").join("app.ts"), 300);

        let report = scan(root).unwrap();
        let ids: Vec<&str> = report.items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"node_modules"), "{ids:?}");
        assert!(ids.contains(&"dist"), "{ids:?}");
        assert!(!ids.contains(&"src"), "source is never offered");
        assert_eq!(report.recommended, 1400);
    }

    /// `dist` in a folder with no project file of its own could be anything.
    #[test]
    fn a_folder_named_like_build_output_needs_a_project_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("dist").join("photo.raw"), 2000);
        assert!(scan(dir.path()).unwrap().items.is_empty());
    }

    #[test]
    fn only_superseded_releases_are_offered_and_they_are_not_ticked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("release").join("v0.3.2").join("setup.exe"), 300);
        write(
            &root.join("release").join("v0.3.4-r22").join("setup.exe"),
            400,
        );
        write(
            &root.join("release").join("v0.3.4-r24").join("setup.exe"),
            500,
        );

        let report = scan(root).unwrap();
        let ids: Vec<&str> = report.items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"release/v0.3.2"), "{ids:?}");
        assert!(ids.contains(&"release/v0.3.4-r22"), "{ids:?}");
        assert!(
            !ids.contains(&"release/v0.3.4-r24"),
            "the newest release stays"
        );
        assert_eq!(
            report.recommended, 0,
            "released packages are the user's call"
        );
    }

    #[test]
    fn a_virtual_environment_is_listed_but_never_ticked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join(".venv").join("pyvenv.cfg"), 50);
        write(&root.join(".venv").join("Lib").join("site.py"), 800);
        let report = scan(root).unwrap();
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].kind, JunkKind::Environment);
        assert!(!report.items[0].recommended);
    }

    #[test]
    fn cleaning_removes_what_was_asked_for_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("package.json"), 40);
        write(&root.join("node_modules").join("a").join("index.js"), 900);
        write(&root.join("dist").join("app.js"), 500);

        let cleaned = clean(root, &["node_modules".to_string(), "nowhere".to_string()]).unwrap();
        assert_eq!(cleaned.removed, vec!["node_modules".to_string()]);
        assert_eq!(cleaned.failed, vec!["nowhere".to_string()]);
        assert_eq!(cleaned.bytes, 900);
        assert!(!root.join("node_modules").exists());
        assert!(root.join("dist").exists(), "what was not asked for stays");
        assert!(root.join("package.json").exists());
    }
}
