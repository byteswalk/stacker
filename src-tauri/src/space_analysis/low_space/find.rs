//! What a disk that is running low can give up, step by step: the projects on it and what they
//! build, the package managers' caches, and temporary files. Only folders a rule recognises are
//! offered — never source, documents, virtual environments, models or the Recycle Bin.

use super::model::{Candidate, Removal, Step};
use crate::project_junk::{self, JunkKind, JunkReport};
use crate::sessions::leftovers::scan::{why_skipped, Env as Guard};
use crate::space_analysis::known::{known_candidates, CleanupKind};
use crate::space_analysis::model::SafetyClass;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const DAY: u64 = 86_400;
/// Smaller items are not worth a line.
pub const MIN_ITEM: u64 = 1 << 20;
/// Something written this recently is being built or used right now.
pub const RECENT: u64 = 30 * 60;
/// Temporary files nothing has written to for a week are not in use: the age Windows' own
/// Disk Cleanup uses.
pub const TEMP_AGE: u64 = 7 * DAY;
/// How deep a disk is walked looking for projects, and how many folders at most.
const WALK_DEPTH: usize = 6;
const WALK_LIMIT: usize = 150_000;
const SCAN_WORKERS: usize = 4;

/// The machine as a cleanup sees it; tests make their own.
pub struct Env {
    /// The system's and the agents' folders, the home folder and the running programs.
    pub guard: Guard,
    /// A project nobody worked in or built for this many days is idle.
    pub inactive_days: u32,
    /// Project folders agent sessions worked in, with when they last did.
    pub sessions: Vec<(PathBuf, u64)>,
    /// The user's temporary folder, and the system's when Stacker may clean it.
    pub temp_dirs: Vec<PathBuf>,
    pub local: PathBuf,
    pub roaming: PathBuf,
}

pub struct Project {
    pub path: PathBuf,
    /// The last time an agent session or git touched it (Unix seconds).
    pub activity: u64,
}

pub fn norm(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn inside(child: &str, parent: &str) -> bool {
    child == parent || child.starts_with(&format!("{parent}\\"))
}

/// The disk a path is on: the longest of the roots it starts with.
pub fn disk_of<'a>(path: &Path, roots: &'a [String]) -> Option<&'a str> {
    let here = norm(path);
    roots
        .iter()
        .filter(|root| inside(&here, &norm(Path::new(root.as_str()))))
        .max_by_key(|root| norm(Path::new(root.as_str())).len())
        .map(String::as_str)
}

fn on_disk(path: &Path, disk: &str, roots: &[String]) -> bool {
    disk_of(path, roots).is_some_and(|found| norm(Path::new(found)) == norm(Path::new(disk)))
}

fn written(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn running_inside(path: &Path, running: &[PathBuf]) -> bool {
    let here = norm(path);
    running.iter().any(|image| inside(&norm(image), &here))
}

/// The files that say a folder is a project of a kind whose build output is recognised.
fn is_marker(name: &str) -> bool {
    let name = name.to_lowercase();
    matches!(
        name.as_str(),
        "cargo.toml"
            | "package.json"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "pyproject.toml"
            | "setup.py"
            | "requirements.txt"
    ) || [".csproj", ".sln", ".fsproj", ".vbproj"]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Folders a walk never enters: the system's own, programs, app data, dependency folders,
/// and anything hidden.
fn walk_skips(name: &str) -> bool {
    let lower = name.to_lowercase();
    name.starts_with('.')
        || name.starts_with('$')
        || matches!(
            lower.as_str(),
            "windows"
                | "windows.old"
                | "program files"
                | "program files (x86)"
                | "programdata"
                | "appdata"
                | "node_modules"
                | "system volume information"
                | "recovery"
                | "perflogs"
                | "msocache"
                | "onedrivetemp"
        )
}

#[cfg(windows)]
fn hidden(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
    entry
        .metadata()
        .is_ok_and(|meta| meta.file_attributes() & HIDDEN_OR_SYSTEM != 0)
}

#[cfg(not(windows))]
fn hidden(_entry: &fs::DirEntry) -> bool {
    false
}

fn protected(path: &Path, guard: &Guard) -> bool {
    let here = norm(path);
    crate::platform::walk_skips(path)
        || guard
            .protected
            .iter()
            .any(|root| !root.as_os_str().is_empty() && inside(&here, &norm(root)))
}

/// Folders below `dir` that hold a project. A project's own folders are not walked further:
/// what it builds is found by the project scan.
pub fn walk_projects(dir: &Path, guard: &Guard) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut budget = WALK_LIMIT;
    walk(dir, 0, false, guard, &mut budget, &mut found);
    found
}

fn walk(
    dir: &Path,
    depth: usize,
    in_repo: bool,
    guard: &Guard,
    budget: &mut usize,
    found: &mut Vec<PathBuf>,
) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut marker = false;
    let mut repo = in_repo;
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            repo = true;
        }
        if kind.is_file() {
            marker |= is_marker(&name);
        } else if kind.is_dir() && !kind.is_symlink() && !walk_skips(&name) && !hidden(&entry) {
            folders.push(entry.path());
        }
    }
    // Only a folder under source control is taken for the user's own project: a program
    // installed outside Program Files (an editor's `resources/app`, a tool's bundled CLI)
    // has the same project files, and its `dist` is the program itself. A disk's root or
    // the home folder with a project file of its own is still walked into.
    if marker && repo && depth > 0 && why_skipped(dir, guard).is_none() && !installed(dir) {
        found.push(dir.to_path_buf());
        return;
    }
    if depth >= WALK_DEPTH {
        return;
    }
    for folder in folders {
        if !protected(&folder, guard) {
            walk(&folder, depth + 1, repo, guard, budget, found);
        }
    }
}

/// Places that hold a program's or a package's own files, never a project's build output:
/// Python packages, Electron apps' bundled code, globally installed npm packages.
pub fn installed(path: &Path) -> bool {
    let here = format!("{}\\", norm(path));
    [
        r"\site-packages\",
        r"\dist-packages\",
        r"\node_modules\",
        r"\resources\app\",
        r".asar.unpacked\",
    ]
    .iter()
    .any(|part| here.contains(part))
}

fn git_activity(project: &Path) -> u64 {
    written(&project.join(".git").join("index"))
}

/// The projects on one disk: the folders agent sessions worked in and what a walk of the disk
/// finds, each once.
pub fn projects_on(disk: &str, roots: &[String], env: &Env, walked: Vec<PathBuf>) -> Vec<Project> {
    let mut activity: HashMap<String, (PathBuf, u64)> = HashMap::new();
    for (path, at) in env
        .sessions
        .iter()
        .cloned()
        .chain(walked.into_iter().map(|p| (p, 0)))
    {
        if !on_disk(&path, disk, roots)
            || why_skipped(&path, &env.guard).is_some()
            || installed(&path)
        {
            continue;
        }
        let entry = activity.entry(norm(&path)).or_insert((path, 0));
        entry.1 = entry.1.max(at);
    }
    activity
        .into_values()
        .map(|(path, at)| Project {
            activity: at.max(git_activity(&path)),
            path,
        })
        .collect()
}

/// Folders a build writes into every time it runs, so their write time says when the project
/// was last built. `node_modules` is written when packages are installed and only read after.
fn written_by_builds(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                "target"
                    | "dist"
                    | "build"
                    | "out"
                    | ".next"
                    | ".nuxt"
                    | ".svelte-kit"
                    | ".output"
                    | "bin"
                    | "obj"
            )
        })
}

/// What one project gives up, and in which step. Virtual environments, released packages and
/// log files never go; nothing being written now or holding a running program goes either;
/// an active project keeps its dependency folders.
pub fn project_candidates(project: &Project, report: &JunkReport, env: &Env) -> Vec<Candidate> {
    let now = env.guard.now;
    let inactive = u64::from(env.inactive_days) * DAY;
    let built = report
        .items
        .iter()
        .filter(|item| written_by_builds(Path::new(&item.path)))
        .map(|item| item.newest)
        .max()
        .unwrap_or(0);
    let idle_project = project.activity.max(built).saturating_add(inactive) < now;
    report
        .items
        .iter()
        .filter_map(|item| {
            if matches!(item.kind, JunkKind::Releases | JunkKind::Environment)
                || item.id.ends_with("*.log")
                || item.bytes < MIN_ITEM
                || item.newest.saturating_add(RECENT) > now
            {
                return None;
            }
            let path = PathBuf::from(&item.path);
            // `out` is also where videos are rendered and apps exported: kept out of an
            // unattended cleanup.
            let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase());
            if name.as_deref() == Some("out")
                || path.parent().is_some_and(installed)
                || running_inside(&path, &env.guard.running)
            {
                return None;
            }
            let dependency = path
                .file_name()
                .is_some_and(|name| name == "node_modules" || name == ".gradle");
            let step = if dependency {
                if !idle_project {
                    return None;
                }
                Step::Caches
            } else if idle_project || item.newest.saturating_add(inactive) < now {
                Step::IdleBuild
            } else {
                Step::Build
            };
            Some(Candidate {
                step,
                path,
                removal: Removal::Whole,
                bytes: item.bytes,
                label: item.label.clone(),
                newest: item.newest,
            })
        })
        .collect()
}

/// Scans the projects side by side; each scan measures its own build folders.
pub fn scan_projects(projects: &[Project]) -> Vec<Option<JunkReport>> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(vec![None; projects.len()]);
    std::thread::scope(|scope| {
        for _ in 0..projects.len().clamp(1, SCAN_WORKERS) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(project) = projects.get(i) else {
                    break;
                };
                let report = project_junk::scan(&project.path).ok();
                if let Ok(mut list) = results.lock() {
                    list[i] = report;
                }
            });
        }
    });
    results.into_inner().unwrap_or_default()
}

/// The programs that read a package cache while they work: with one running, the cache is
/// in use. (npm, pnpm and Gradle run inside `node` and `java`, which are always about.)
fn cache_tools(id: &str) -> &'static [&'static str] {
    match id {
        "cargo" | "cargo-src" => &["cargo", "rustc", "cargo-clippy", "clippy-driver"],
        "sccache" => &["sccache", "rustc"],
        "gomod" => &["go"],
        "pip" => &["pip", "pip3"],
        "uv" => &["uv"],
        "poetry" => &["poetry"],
        "bun" => &["bun"],
        "deno" => &["deno"],
        _ => &[],
    }
}

/// Whether a package cache may go. One written to within the idle window is in use: a build
/// downloaded or unpacked into it lately and will read it again, so removing it only makes
/// the next build fetch it all again (and a build running meanwhile fails). Nor while a
/// program that reads it is running.
pub fn cache_may_go(id: &str, newest: u64, env: &Env) -> bool {
    let idle_for = u64::from(env.inactive_days) * DAY;
    let tools = cache_tools(id);
    let tool_running = env.guard.running.iter().any(|image| {
        image
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_lowercase())
            .is_some_and(|stem| tools.contains(&stem.as_str()))
    });
    newest.saturating_add(idle_for) < env.guard.now && !tool_running
}

/// The package managers' download caches on this disk, when nothing uses them; browser
/// caches and crash dumps are the system step's, and go whenever it comes.
pub fn known_cache_candidates(disk: &str, roots: &[String], env: &Env) -> Vec<Candidate> {
    known_candidates()
        .into_iter()
        .filter(|known| {
            known.safety == SafetyClass::Safe
                && known.cleanup_kind == CleanupKind::Contents
                && on_disk(&known.path, disk, roots)
        })
        .filter_map(|known| {
            let step = match known.ecosystem.as_deref() {
                Some("browser") | Some("windows") => Step::System,
                _ => Step::Caches,
            };
            let (bytes, _, newest) = project_junk::size_of(&known.path);
            if bytes < MIN_ITEM || (step == Step::Caches && !cache_may_go(&known.id, newest, env)) {
                return None;
            }
            Some(Candidate {
                step,
                path: known.path,
                removal: Removal::Contents,
                bytes,
                label: known.name_key,
                newest,
            })
        })
        .collect()
}

/// Caches Windows and common programs rebuild by themselves.
#[cfg(windows)]
fn program_caches(env: &Env) -> Vec<(PathBuf, &'static str)> {
    let local = &env.local;
    let mut list = vec![
        (local.join(r"Microsoft\Windows\INetCache"), "网页组件缓存"),
        (local.join("D3DSCache"), "显卡着色器缓存"),
        (local.join(r"NVIDIA\DXCache"), "显卡着色器缓存"),
        (local.join(r"NVIDIA\GLCache"), "显卡着色器缓存"),
        (local.join(r"AMD\DxCache"), "显卡着色器缓存"),
        (local.join(r"AMD\DxcCache"), "显卡着色器缓存"),
        (local.join(r"AMD\VkCache"), "显卡着色器缓存"),
        (local.join(r"AMD\GLCache"), "显卡着色器缓存"),
        (
            local.join(r"Microsoft\Windows\WER\ReportArchive"),
            "Windows 错误报告",
        ),
        (
            local.join(r"Microsoft\Windows\WER\ReportQueue"),
            "Windows 错误报告",
        ),
    ];
    for app in [
        "Code",
        "Code - Insiders",
        "Cursor",
        "Windsurf",
        "Trae",
        "Kiro",
        "Qoder",
    ] {
        for part in [
            "Cache",
            "CachedData",
            "CachedExtensionVSIXs",
            "GPUCache",
            "Code Cache",
        ] {
            list.push((env.roaming.join(app).join(part), "编辑器缓存"));
        }
    }
    list
}

#[cfg(not(windows))]
fn program_caches(_env: &Env) -> Vec<(PathBuf, &'static str)> {
    Vec::new()
}

fn plain_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| {
        meta.is_dir() && !crate::space_analysis::walker::is_link_or_reparse_point(&meta)
    })
}

/// Whether a temporary entry may go: old enough, nothing running from it, and not Claude's
/// scratch folders, which the session leftovers clean by each session's own activity.
pub fn temp_entry_may_go(path: &Path, newest: u64, env: &Env) -> bool {
    let claude = path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("claude"));
    !claude
        && newest.saturating_add(TEMP_AGE) < env.guard.now
        && !running_inside(path, &env.guard.running)
}

/// What a temporary folder's old entries add up to.
pub fn old_temp_bytes(dir: &Path, env: &Env) -> (u64, u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut total = (0, 0);
    for path in entries.flatten().map(|e| e.path()) {
        let (bytes, _, newest) = project_junk::size_of(&path);
        if temp_entry_may_go(&path, newest, env) {
            total.0 += bytes;
            total.1 = total.1.max(newest);
        }
    }
    total
}

/// Temporary files, browser caches, crash dumps and the caches Windows and editors rebuild.
pub fn system_candidates(disk: &str, roots: &[String], env: &Env) -> Vec<Candidate> {
    let mut found = Vec::new();
    for dir in &env.temp_dirs {
        if !on_disk(dir, disk, roots) || !plain_dir(dir) {
            continue;
        }
        let (bytes, newest) = old_temp_bytes(dir, env);
        if bytes >= MIN_ITEM {
            let system = norm(dir).contains("\\windows\\temp");
            found.push(Candidate {
                step: Step::System,
                path: dir.clone(),
                removal: Removal::OldEntries(TEMP_AGE),
                bytes,
                label: if system {
                    "spaceAnalysis.known.windowsTemp"
                } else {
                    "spaceAnalysis.known.userTemp"
                }
                .into(),
                newest,
            });
        }
    }
    for (path, label) in program_caches(env) {
        if !on_disk(&path, disk, roots) || !plain_dir(&path) {
            continue;
        }
        let (bytes, _, newest) = project_junk::size_of(&path);
        if bytes >= MIN_ITEM {
            found.push(Candidate {
                step: Step::System,
                path,
                removal: Removal::Contents,
                bytes,
                label: label.into(),
                newest,
            });
        }
    }
    found
}

/// One folder inside another is cleaned with it: only the outer one stays on the list.
pub fn without_nested(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by_key(|c| norm(&c.path).len());
    let mut kept: Vec<Candidate> = Vec::new();
    let mut seen = HashSet::new();
    for candidate in candidates {
        let here = norm(&candidate.path);
        if !seen.insert(here.clone()) || kept.iter().any(|k| inside(&here, &norm(&k.path))) {
            continue;
        }
        kept.push(candidate);
    }
    kept
}

/// The order within a step: idle output biggest first; build output of projects in use least
/// recently built first; download caches before idle projects' dependencies; temporary files
/// before program caches.
pub fn order(candidates: &mut [Candidate]) {
    candidates.sort_by_key(|c| {
        let rank = match (c.step, c.removal) {
            (Step::Build, _) => (0, c.newest),
            (Step::Caches, Removal::Contents) => (0, u64::MAX - c.bytes),
            (Step::System, Removal::OldEntries(_)) => (0, u64::MAX - c.bytes),
            _ => (1, u64::MAX - c.bytes),
        };
        (Step::ALL.iter().position(|s| *s == c.step), rank)
    });
}
