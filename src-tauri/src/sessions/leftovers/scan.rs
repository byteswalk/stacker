//! Groups Claude and Codex sessions by the folder they worked in, and lists what each project's
//! sessions left behind: build outputs, scratch folders, checkpoints and screenshots.

use super::model::{LeftoverItem, LeftoverKind, LeftoverReport, Operation, ProjectLeftovers, Scan};
use super::settings::Settings;
use super::slim;
use crate::project_junk::{self, JunkKind};
use crate::sessions::model::{Agent, Roots, Session};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const DAY: u64 = 86_400;
/// Smaller items are not listed: a 3 KB `__pycache__` is not worth a line.
const MIN_ITEM: u64 = 1 << 20;
const SCAN_WORKERS: usize = 4;
const CACHE_FOR: Duration = Duration::from_secs(300);
pub const ORPHAN_KEY: &str = "orphan-temp";

/// The machine as the scan sees it; tests give their own.
pub struct Env {
    pub now: u64,
    /// `%TEMP%\claude`, where Claude keeps a scratch folder per session.
    pub temp_claude: PathBuf,
    /// `<Claude data>\file-history`, one folder of snapshots per session.
    pub file_history: PathBuf,
    /// A project here or above it would be everything the user has.
    pub home: PathBuf,
    /// A project inside one of these, or containing one, is not looked into: the system,
    /// programs, app data and the agents' own data folders.
    pub protected: Vec<PathBuf>,
    pub running: Vec<PathBuf>,
}

impl Env {
    pub fn system(roots: &Roots) -> Env {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let protected = [
            var("SystemRoot"),
            var("ProgramFiles"),
            var("ProgramFiles(x86)"),
            var("ProgramData"),
            dirs::data_dir(),
            dirs::data_local_dir(),
            Some(std::env::temp_dir()),
            Some(PathBuf::from(&roots.claude)),
            Some(PathBuf::from(&roots.codex)),
        ]
        .into_iter()
        .flatten()
        .filter(|path| !path.as_os_str().is_empty())
        .collect();
        Env {
            now: crate::sessions::now(),
            temp_claude: std::env::temp_dir().join("claude"),
            file_history: PathBuf::from(&roots.claude).join("file-history"),
            home: dirs::home_dir().unwrap_or_default(),
            protected,
            running: crate::sessions::footprint::processes::running_images(),
        }
    }
}

fn norm(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn inside(child: &str, parent: &str) -> bool {
    !parent.is_empty() && (child == parent || child.starts_with(&format!("{parent}\\")))
}

/// Why a project's own folder is not looked into.
pub fn why_skipped(path: &Path, env: &Env) -> Option<&'static str> {
    if path.as_os_str().is_empty() {
        return Some("没有记录工作目录");
    }
    if !path.is_dir() {
        return Some("目录已不存在");
    }
    let here = norm(path);
    if path.parent().is_none() || here.len() <= 3 {
        return Some("是磁盘根目录");
    }
    if inside(&norm(&env.home), &here) {
        return Some("是用户主目录或它的上级");
    }
    let protected = env.protected.iter().map(|root| norm(root));
    for root in protected {
        if inside(&here, &root) || inside(&root, &here) {
            return Some("在系统、程序或智能体自己的数据目录里");
        }
    }
    None
}

/// Claude names each session's scratch folder after the session; nothing else is taken.
fn is_session_id(name: &str) -> bool {
    name.len() == 36
        && name.chars().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// Session id → its scratch folders under `%TEMP%\claude\<project>\`.
fn temp_folders(temp_claude: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut found: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let Ok(projects) = std::fs::read_dir(temp_claude) else {
        return found;
    };
    for project in projects.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Ok(sessions) = std::fs::read_dir(&project) else {
            continue;
        };
        for folder in sessions.flatten().map(|e| e.path()) {
            let name = folder
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if folder.is_dir() && is_session_id(&name) {
                found.entry(name).or_default().push(folder);
            }
        }
    }
    found
}

/// A transcript's size and write time when it was measured, and what slimming it saves.
type Measured = HashMap<PathBuf, (u64, SystemTime, slim::Saving)>;

static MEASURED: Mutex<Option<Measured>> = Mutex::new(None);

/// What slimming a transcript saves, remembered while the file does not change: reading
/// gigabytes of transcripts again on every scan is what makes a scan slow.
fn images_in(path: &Path) -> slim::Saving {
    let Some(stamp) = std::fs::metadata(path)
        .ok()
        .and_then(|m| Some((m.len(), m.modified().ok()?)))
    else {
        return slim::Saving::default();
    };
    if let Ok(guard) = MEASURED.lock() {
        if let Some((len, at, saving)) = guard.as_ref().and_then(|map| map.get(path)) {
            if (*len, *at) == stamp {
                return *saving;
            }
        }
    }
    let saving = slim::measure(path).unwrap_or_default();
    if let Ok(mut guard) = MEASURED.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(path.to_path_buf(), (stamp.0, stamp.1, saving));
    }
    saving
}

struct Builder<'a> {
    key: String,
    items: Vec<LeftoverItem>,
    operations: &'a mut HashMap<String, Operation>,
    running: &'a [String],
}

impl Builder<'_> {
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        kind: LeftoverKind,
        part: &str,
        label: String,
        explain: &str,
        shown: String,
        (bytes, files): (u64, u64),
        recommended: bool,
        operation: Operation,
    ) {
        if bytes < MIN_ITEM || operation.paths.is_empty() {
            return;
        }
        let blocked = operation.paths.iter().find_map(|path| {
            let path = norm(path);
            self.running
                .iter()
                .find(|image| inside(image, &path))
                .map(|image| {
                    let name = image.rsplit('\\').next().unwrap_or(image);
                    format!("有程序正在运行：{name}")
                })
        });
        let id = format!("{}|{kind:?}|{part}", self.key);
        self.items.push(LeftoverItem {
            id: id.clone(),
            kind,
            label,
            explain: explain.to_string(),
            path: shown,
            bytes,
            files,
            recommended: recommended && blocked.is_none(),
            blocked,
        });
        self.operations.insert(id, operation);
    }
}

/// Folders a build writes into every time it runs, so their write time says when the project
/// was last built. `node_modules` is not one: it is written when packages are installed and
/// only read after that.
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

struct Group<'a> {
    key: String,
    name: String,
    path: PathBuf,
    sessions: Vec<&'a Session>,
}

fn groups(sessions: &[Session]) -> Vec<Group<'_>> {
    let mut by_key: HashMap<String, Group> = HashMap::new();
    for session in sessions
        .iter()
        .filter(|s| matches!(s.agent, Agent::Claude | Agent::Codex))
    {
        let path = PathBuf::from(&session.project.path);
        let key = norm(&path);
        let group = by_key.entry(key.clone()).or_insert_with(|| Group {
            name: if session.project.name.is_empty() {
                "未记录工作目录".into()
            } else {
                session.project.name.clone()
            },
            key,
            path,
            sessions: Vec::new(),
        });
        group.sessions.push(session);
    }
    by_key.into_values().collect()
}

pub fn scan_with(sessions: &[Session], settings: &Settings, env: &Env) -> Scan {
    let inactive_after = env
        .now
        .saturating_sub(u64::from(settings.inactive_days) * DAY);
    let checkpoints_after = env
        .now
        .saturating_sub(u64::from(settings.checkpoint_days) * DAY);
    let slim_after = env.now.saturating_sub(u64::from(settings.slim_days) * DAY);
    let running: Vec<String> = env.running.iter().map(|p| norm(p)).collect();
    let mut temp = temp_folders(&env.temp_claude);
    let groups = groups(sessions);

    // Each project folder is looked into once, four at a time.
    let skipped: Vec<Option<&'static str>> =
        groups.iter().map(|g| why_skipped(&g.path, env)).collect();
    let roots: Vec<String> = groups
        .iter()
        .zip(&skipped)
        .filter(|(_, skip)| skip.is_none())
        .map(|(g, _)| g.key.clone())
        .collect();
    let junk: Vec<Option<project_junk::JunkReport>> = {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results: Mutex<Vec<Option<project_junk::JunkReport>>> =
            Mutex::new((0..groups.len()).map(|_| None).collect());
        std::thread::scope(|scope| {
            for _ in 0..SCAN_WORKERS.min(groups.len()) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(group) = groups.get(i) else { break };
                    if skipped[i].is_none() {
                        let report = project_junk::scan(&group.path).ok();
                        if let Ok(mut rows) = results.lock() {
                            rows[i] = report;
                        }
                    }
                });
            }
        });
        results.into_inner().unwrap_or_default()
    };

    let mut operations = HashMap::new();
    let mut projects = Vec::new();
    for ((group, skip), junk) in groups.iter().zip(&skipped).zip(junk) {
        // A folder inside another project's folder belongs to the deeper project.
        let deeper: Vec<&String> = roots
            .iter()
            .filter(|root| *root != &group.key && inside(root, &group.key))
            .collect();
        let junk_items: Vec<project_junk::JunkItem> = junk
            .map(|report| report.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| !matches!(item.kind, JunkKind::Releases) && !item.id.ends_with("*.log"))
            .filter(|item| {
                !deeper
                    .iter()
                    .any(|root| inside(&norm(Path::new(&item.path)), root))
            })
            .collect();
        let sessions_newest = group
            .sessions
            .iter()
            .map(|s| s.updated_at)
            .max()
            .unwrap_or(0);
        let builds_newest = junk_items
            .iter()
            .filter(|item| !matches!(item.kind, JunkKind::Environment))
            .map(|item| item.newest)
            .max()
            .unwrap_or(0);
        let last_activity = sessions_newest.max(builds_newest);
        let active = last_activity >= inactive_after;

        let mut builder = Builder {
            key: group.key.clone(),
            items: Vec::new(),
            operations: &mut operations,
            running: &running,
        };
        for item in &junk_items {
            let path = PathBuf::from(&item.path);
            let environment = matches!(item.kind, JunkKind::Environment);
            let kind = if environment {
                LeftoverKind::Environment
            } else {
                LeftoverKind::Build
            };
            builder.push(
                kind,
                &item.id,
                format!("{}（{}）", item.label, item.id),
                &item.explain,
                item.path.clone(),
                (item.bytes, item.files),
                // A project in use still gives up a build folder nothing was built into lately.
                !environment
                    && item.recommended
                    && (!active || (written_by_builds(&path) && item.newest < inactive_after)),
                Operation {
                    root: group.path.clone(),
                    paths: vec![path.clone()],
                    slim: false,
                },
            );
            if active && item.newest >= inactive_after && !item.parts.is_empty() {
                let stale: Vec<(PathBuf, u64, u64)> = item
                    .parts
                    .iter()
                    .filter(|part| part.bytes > 0 && part.newest < inactive_after)
                    .map(|part| (part.path.clone(), part.bytes, part.files))
                    .collect();
                let shown: Vec<String> = stale
                    .iter()
                    .map(|(p, _, _)| p.to_string_lossy().into_owned())
                    .collect();
                builder.push(
                    LeftoverKind::Stale,
                    &format!("{}:stale", item.id),
                    "很久没编译过的构建配置".into(),
                    "项目还在用，只删很久没编译过的构建配置；正在用的那一套留着，下次编译仍然快。",
                    shown.join("  "),
                    (
                        stale.iter().map(|s| s.1).sum(),
                        stale.iter().map(|s| s.2).sum(),
                    ),
                    true,
                    Operation {
                        root: group.path.clone(),
                        paths: stale.into_iter().map(|s| s.0).collect(),
                        slim: false,
                    },
                );
                let caches: Vec<(PathBuf, u64, u64)> = item
                    .parts
                    .iter()
                    .filter(|part| part.newest >= inactive_after)
                    .filter_map(|part| part.incremental.clone())
                    .collect();
                builder.push(
                    LeftoverKind::Incremental,
                    &format!("{}:incremental", item.id),
                    "增量编译缓存".into(),
                    "删掉后下一次编译会慢一些（依赖不用重编）。项目还在用，智能体编译变慢可能超时重试，所以默认不勾。",
                    caches
                        .iter()
                        .map(|c| c.0.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("  "),
                    (caches.iter().map(|c| c.1).sum(), caches.iter().map(|c| c.2).sum()),
                    false,
                    Operation {
                        root: group.path.clone(),
                        paths: caches.into_iter().map(|c| c.0).collect(),
                        slim: false,
                    },
                );
            }
        }

        // Per-session leftovers, for the sessions that have been left alone long enough.
        let claude: Vec<&&Session> = group
            .sessions
            .iter()
            .filter(|s| s.agent == Agent::Claude)
            .collect();
        let mut scratch = (Vec::new(), 0, 0, 0usize);
        let mut snapshots = (Vec::new(), 0, 0, 0usize);
        for session in &claude {
            let id = session.native_id.to_lowercase();
            let folders = temp.remove(&id).unwrap_or_default();
            if session.updated_at < inactive_after && !folders.is_empty() {
                for folder in folders {
                    let (bytes, files, _) = project_junk::size_of(&folder);
                    scratch.0.push(folder);
                    scratch.1 += bytes;
                    scratch.2 += files;
                }
                scratch.3 += 1;
            }
            let history = env.file_history.join(&session.native_id);
            if session.updated_at < checkpoints_after && history.is_dir() {
                let (bytes, files, _) = project_junk::size_of(&history);
                snapshots.0.push(history);
                snapshots.1 += bytes;
                snapshots.2 += files;
                snapshots.3 += 1;
            }
        }
        builder.push(
            LeftoverKind::Temp,
            "temp",
            "会话临时目录".into(),
            "Claude 为会话准备的草稿目录，会话结束后就用不上了。",
            format!("{}（{} 个会话）", env.temp_claude.display(), scratch.3),
            (scratch.1, scratch.2),
            true,
            Operation {
                root: env.temp_claude.clone(),
                paths: scratch.0,
                slim: false,
            },
        );
        builder.push(
            LeftoverKind::Checkpoints,
            "checkpoints",
            "旧会话的检查点".into(),
            "用于回退文件改动的快照；删除后这些旧会话不能再回退，但仍可查看和续聊。",
            format!("{}（{} 个会话）", env.file_history.display(), snapshots.3),
            (snapshots.1, snapshots.2),
            true,
            Operation {
                root: env.file_history.clone(),
                paths: snapshots.0,
                slim: false,
            },
        );
        if settings.slim {
            let mut files = Vec::new();
            let mut saving = slim::Saving::default();
            let mut count = 0usize;
            for session in group.sessions.iter().filter(|s| s.updated_at < slim_after) {
                let mut touched = false;
                for transcript in std::iter::once(&session.path).chain(&session.copies) {
                    let path = PathBuf::from(transcript);
                    let found = images_in(&path);
                    if found.images > 0 {
                        saving.bytes += found.bytes;
                        saving.images += found.images;
                        files.push(path);
                        touched = true;
                    }
                }
                count += usize::from(touched);
            }
            let root = common_root(&files);
            builder.push(
                LeftoverKind::Images,
                "images",
                "旧会话里的截图".into(),
                "把会话记录里的截图换成 1 像素小图，文字全部保留，照样能查看和续聊。",
                format!("{count} 个会话 · {} 张截图", saving.images),
                (saving.bytes, saving.images),
                true,
                Operation {
                    root,
                    paths: files,
                    slim: true,
                },
            );
        }

        let mut items = builder.items;
        items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
        projects.push(ProjectLeftovers {
            key: group.key.clone(),
            name: group.name.clone(),
            path: group.path.to_string_lossy().into_owned(),
            agents: group
                .sessions
                .iter()
                .map(|s| s.agent)
                .fold(Vec::new(), |mut list, agent| {
                    if !list.contains(&agent) {
                        list.push(agent);
                    }
                    list
                }),
            sessions: group.sessions.len(),
            last_activity,
            active,
            skipped: skip.map(str::to_string),
            items,
        });
    }

    // Scratch folders of sessions Claude no longer lists, left alone long enough.
    let orphans: Vec<(PathBuf, u64, u64)> = temp
        .into_values()
        .flatten()
        .filter_map(|folder| {
            let (bytes, files, newest) = project_junk::size_of(&folder);
            (newest < inactive_after).then_some((folder, bytes, files))
        })
        .collect();
    if !orphans.is_empty() {
        let mut builder = Builder {
            key: ORPHAN_KEY.into(),
            items: Vec::new(),
            operations: &mut operations,
            running: &running,
        };
        builder.push(
            LeftoverKind::Temp,
            "temp",
            "会话临时目录".into(),
            "这些会话已不在 Claude 的会话列表里，它们的草稿目录没人会再用。",
            format!("{}（{} 个会话）", env.temp_claude.display(), orphans.len()),
            (
                orphans.iter().map(|o| o.1).sum(),
                orphans.iter().map(|o| o.2).sum(),
            ),
            true,
            Operation {
                root: env.temp_claude.clone(),
                paths: orphans.into_iter().map(|o| o.0).collect(),
                slim: false,
            },
        );
        projects.push(ProjectLeftovers {
            key: ORPHAN_KEY.into(),
            name: "已不在会话列表中的会话".into(),
            path: env.temp_claude.to_string_lossy().into_owned(),
            agents: vec![Agent::Claude],
            sessions: 0,
            last_activity: 0,
            active: false,
            skipped: None,
            items: builder.items,
        });
    }

    let recommended_of = |p: &ProjectLeftovers| -> u64 {
        p.items
            .iter()
            .filter(|i| i.recommended)
            .map(|i| i.bytes)
            .sum()
    };
    projects.retain(|p| !p.items.is_empty() || p.skipped.is_some());
    projects.sort_by_key(|p| {
        (
            std::cmp::Reverse(recommended_of(p)),
            std::cmp::Reverse(p.items.iter().map(|i| i.bytes).sum::<u64>()),
        )
    });
    let all = projects.iter().flat_map(|p| p.items.iter());
    // Old profiles and the incremental cache are inside a build folder already counted.
    let total = all
        .clone()
        .filter(|i| !matches!(i.kind, LeftoverKind::Stale | LeftoverKind::Incremental))
        .map(|i| i.bytes)
        .sum();
    let recommended = all.filter(|i| i.recommended).map(|i| i.bytes).sum();
    Scan {
        report: LeftoverReport {
            projects,
            total,
            recommended,
            scanned_at: env.now,
            settings: settings.clone(),
        },
        operations,
    }
}

/// The deepest folder every transcript is inside: deletion and rewriting stay within it.
fn common_root(files: &[PathBuf]) -> PathBuf {
    let mut root = match files.first().and_then(|f| f.parent()) {
        Some(parent) => parent.to_path_buf(),
        None => return PathBuf::new(),
    };
    for file in files {
        while !file.starts_with(&root) {
            match root.parent() {
                Some(parent) => root = parent.to_path_buf(),
                None => return PathBuf::new(),
            }
        }
    }
    root
}

static CACHE: Mutex<Option<(Instant, Arc<Scan>)>> = Mutex::new(None);

/// A fresh scan of the catalog, with the settings saved now.
pub fn run() -> Result<Arc<Scan>, String> {
    let (sessions, _, roots) = crate::sessions::commands::annotated_catalog()?;
    let scan = Arc::new(scan_with(
        &sessions,
        &super::settings::load(),
        &Env::system(&roots),
    ));
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((Instant::now(), scan.clone()));
    }
    Ok(scan)
}

/// The last scan when it is recent, else a new one.
pub fn cached(refresh: bool) -> Result<Arc<Scan>, String> {
    if !refresh {
        if let Ok(cache) = CACHE.lock() {
            if let Some((at, scan)) = cache.as_ref() {
                if at.elapsed() < CACHE_FOR {
                    return Ok(scan.clone());
                }
            }
        }
    }
    run()
}

pub fn invalidate() {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}
