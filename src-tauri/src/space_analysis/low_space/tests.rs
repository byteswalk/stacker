use super::find::{self, Env, Project};
use super::model::{Candidate, DiskSpace, Removal, RunRecord, Step};
use super::remove::{self, Removed};
use super::run::{self, Source};
use super::settings::Settings;
use super::watch;
use crate::project_junk::{JunkItem, JunkKind, JunkReport};
use crate::sessions::leftovers::scan::Env as Guard;
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};

const DAY: u64 = 86_400;
const NOW: u64 = 1_800_000_000;
const GIB: u64 = 1 << 30;

fn env(running: Vec<PathBuf>) -> Env {
    Env {
        guard: Guard {
            now: NOW,
            temp_claude: PathBuf::new(),
            file_history: PathBuf::new(),
            home: PathBuf::from(r"C:\Users\someone"),
            protected: vec![PathBuf::from(r"C:\Windows")],
            running,
        },
        inactive_days: 7,
        sessions: Vec::new(),
        temp_dirs: Vec::new(),
        local: PathBuf::new(),
        roaming: PathBuf::new(),
    }
}

fn item(path: &str, kind: JunkKind, bytes: u64, newest: u64) -> JunkItem {
    JunkItem {
        id: path.rsplit('\\').next().unwrap().into(),
        kind,
        label: "构建产物 target".into(),
        explain: String::new(),
        path: path.into(),
        bytes,
        files: 1,
        recommended: true,
        newest,
        parts: Vec::new(),
    }
}

fn report(items: Vec<JunkItem>) -> JunkReport {
    JunkReport {
        root: r"E:\work\app".into(),
        total: items.iter().map(|i| i.bytes).sum(),
        recommended: 0,
        items,
        scanned_at: NOW,
    }
}

fn steps_of(candidates: &[Candidate]) -> Vec<(String, Step)> {
    candidates
        .iter()
        .map(|c| {
            (
                c.path.file_name().unwrap().to_string_lossy().into_owned(),
                c.step,
            )
        })
        .collect()
}

#[test]
fn on_by_default_at_five_percent() {
    let settings = Settings::default();
    assert!(settings.enabled);
    assert_eq!(settings.threshold_percent, 5);
    let partial: Settings = serde_json::from_str(r#"{"thresholdPercent":8}"#).unwrap();
    assert!(partial.enabled);
    assert_eq!(partial.threshold_percent, 8);
}

#[test]
fn low_means_the_threshold_or_less_and_the_goal_is_twice_that() {
    assert!(run::is_low(5 * GIB, 100 * GIB, 5));
    assert!(!run::is_low(5 * GIB + 1, 100 * GIB, 5));
    assert!(!run::is_low(0, 0, 5), "a disk with no size is never low");
    assert_eq!(run::goal(100 * GIB, 5), 10 * GIB);
    assert_eq!(run::goal(100, 50), 100, "never more than the disk");
}

fn ran(drive: &str, finished_at: u64, free_after: u64) -> RunRecord {
    RunRecord {
        drive: drive.into(),
        at: finished_at,
        finished_at,
        total: 100 * GIB,
        free_before: 0,
        free_after,
        freed: 0,
        manual: false,
        steps: Vec::new(),
        notes: Vec::new(),
    }
}

#[test]
fn a_disk_is_not_cleaned_again_until_space_is_used_or_a_day_passes() {
    let disk = |free| DiskSpace {
        root: r"D:\".into(),
        total: 100 * GIB,
        free,
    };
    let runs = vec![ran(r"d:\", NOW - 2 * 3600, 4 * GIB)];
    assert!(watch::due(&[], &disk(4 * GIB), NOW), "never cleaned");
    assert!(
        !watch::due(&runs, &disk(4 * GIB), NOW),
        "nothing used since"
    );
    assert!(
        watch::due(&runs, &disk(2 * GIB), NOW),
        "two gigabytes used since"
    );
    let recent = vec![ran(r"D:\", NOW - 600, 4 * GIB)];
    assert!(!watch::due(&recent, &disk(GIB), NOW), "within the hour");
    let old = vec![ran(r"D:\", NOW - DAY, 4 * GIB)];
    assert!(watch::due(&old, &disk(4 * GIB), NOW), "a day later");
    let other = vec![ran(r"C:\", NOW - 600, 4 * GIB)];
    assert!(
        watch::due(&other, &disk(4 * GIB), NOW),
        "another disk's run"
    );

    let disks = [
        disk(4 * GIB),
        DiskSpace {
            root: r"E:\".into(),
            total: 100 * GIB,
            free: 50 * GIB,
        },
    ];
    assert_eq!(watch::low_disks(&disks, 5, &runs, NOW, false).len(), 0);
    assert_eq!(
        watch::low_disks(&disks, 5, &runs, NOW, true).len(),
        1,
        "asked for, a low disk is cleaned"
    );
}

#[test]
fn a_path_belongs_to_the_disk_with_the_longest_root() {
    let roots = vec![r"C:\".to_string(), r"E:\".to_string()];
    assert_eq!(
        find::disk_of(Path::new(r"e:\Work\app"), &roots),
        Some(r"E:\")
    );
    assert_eq!(find::disk_of(Path::new(r"Z:\share"), &roots), None);
    let mac = vec!["/".to_string(), "/Volumes/Data".to_string()];
    assert_eq!(
        find::disk_of(Path::new("/Volumes/Data/x"), &mac),
        Some("/Volumes/Data")
    );
    assert_eq!(find::disk_of(Path::new("/Users/me/x"), &mac), Some("/"));
}

#[test]
fn build_output_is_idle_or_in_use_and_dependencies_go_only_from_idle_projects() {
    let env = env(Vec::new());
    let active = Project {
        path: PathBuf::from(r"E:\work\app"),
        activity: NOW - DAY,
    };
    let items = vec![
        item(
            r"E:\work\app\target",
            JunkKind::Rebuildable,
            50 * GIB,
            NOW - 2 * DAY,
        ),
        item(
            r"E:\work\app\old\dist",
            JunkKind::Rebuildable,
            GIB,
            NOW - 30 * DAY,
        ),
        item(
            r"E:\work\app\node_modules",
            JunkKind::Rebuildable,
            GIB,
            NOW - 60 * DAY,
        ),
        item(
            r"E:\work\app\.venv",
            JunkKind::Environment,
            GIB,
            NOW - 60 * DAY,
        ),
        item(
            r"E:\work\app\release\v1",
            JunkKind::Releases,
            GIB,
            NOW - 60 * DAY,
        ),
        item(r"E:\work\app\*.log", JunkKind::Cache, GIB, NOW - 60 * DAY),
        item(
            r"E:\work\app\building",
            JunkKind::Rebuildable,
            GIB,
            NOW - 60,
        ),
        item(r"E:\work\app\tiny", JunkKind::Cache, 1000, NOW - 60 * DAY),
    ];
    let found = find::project_candidates(&active, &report(items.clone()), &env);
    assert_eq!(
        steps_of(&found),
        vec![
            ("target".into(), Step::Build),
            ("dist".into(), Step::IdleBuild)
        ],
        "an active project keeps its dependencies; environments, releases, logs, what is being \
         written and tiny items never go"
    );

    let idle = Project {
        path: PathBuf::from(r"E:\work\app"),
        activity: NOW - 30 * DAY,
    };
    let idle_items = items
        .into_iter()
        .map(|mut i| {
            if i.newest > NOW - 7 * DAY && i.newest < NOW - 3600 {
                i.newest = NOW - 20 * DAY;
            }
            i
        })
        .collect();
    let found = find::project_candidates(&idle, &report(idle_items), &env);
    assert_eq!(
        steps_of(&found),
        vec![
            ("target".into(), Step::IdleBuild),
            ("dist".into(), Step::IdleBuild),
            ("node_modules".into(), Step::Caches),
        ]
    );
}

#[test]
fn a_recent_build_makes_the_project_active() {
    let env = env(Vec::new());
    // No session and no git, but its output was built yesterday.
    let project = Project {
        path: PathBuf::from(r"E:\work\app"),
        activity: 0,
    };
    let items = vec![
        item(r"E:\work\app\target", JunkKind::Rebuildable, GIB, NOW - DAY),
        item(
            r"E:\work\app\node_modules",
            JunkKind::Rebuildable,
            GIB,
            NOW - 90 * DAY,
        ),
    ];
    let found = find::project_candidates(&project, &report(items), &env);
    assert_eq!(steps_of(&found), vec![("target".into(), Step::Build)]);
}

#[test]
fn nothing_a_running_program_was_started_from_goes() {
    let env = env(vec![PathBuf::from(r"E:\work\app\target\debug\app.exe")]);
    let project = Project {
        path: PathBuf::from(r"E:\work\app"),
        activity: 0,
    };
    let items = vec![item(
        r"E:\work\app\target",
        JunkKind::Rebuildable,
        GIB,
        NOW - 30 * DAY,
    )];
    assert!(find::project_candidates(&project, &report(items), &env).is_empty());
}

#[test]
fn a_folder_inside_another_goes_with_it_and_steps_keep_their_order() {
    let candidate = |path: &str, step, removal, bytes, newest| Candidate {
        step,
        path: PathBuf::from(path),
        removal,
        bytes,
        label: String::new(),
        newest,
    };
    let kept = find::without_nested(vec![
        candidate(r"E:\a\sub\target", Step::IdleBuild, Removal::Whole, 1, 0),
        candidate(r"E:\a\sub", Step::IdleBuild, Removal::Whole, 1, 0),
        candidate(r"E:\a\sub", Step::IdleBuild, Removal::Whole, 1, 0),
        candidate(r"E:\a\subway", Step::IdleBuild, Removal::Whole, 1, 0),
    ]);
    assert_eq!(kept.len(), 2);

    let mut list = vec![
        candidate(r"E:\new", Step::Build, Removal::Whole, 9, NOW),
        candidate(r"E:\old", Step::Build, Removal::Whole, 1, NOW - DAY),
        candidate(r"E:\deps", Step::Caches, Removal::Whole, 50, 0),
        candidate(r"E:\npm", Step::Caches, Removal::Contents, 5, 0),
    ];
    find::order(&mut list);
    let names: Vec<_> = list
        .iter()
        .map(|c| c.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![r"E:\old", r"E:\new", r"E:\npm", r"E:\deps"]);
}

/// Seen on the user's machine: the cargo caches were removed, the next build fetched them
/// again, the next run removed them again — the last time under a build that then failed.
#[test]
fn a_package_cache_in_use_or_read_by_a_running_tool_stays() {
    let idle = env(Vec::new());
    assert!(find::cache_may_go("cargo-src", NOW - 30 * DAY, &idle));
    assert!(
        !find::cache_may_go("cargo-src", NOW - DAY, &idle),
        "unpacked into yesterday"
    );
    let building = env(vec![PathBuf::from(
        r"C:\Users\me\.rustup\toolchains\stable\bin\cargo.exe",
    )]);
    assert!(!find::cache_may_go("cargo", NOW - 30 * DAY, &building));
    assert!(
        find::cache_may_go("npm", NOW - 30 * DAY, &building),
        "cargo does not read npm's cache"
    );
    let compiling = env(vec![PathBuf::from(r"C:\tools\RUSTC.EXE")]);
    assert!(!find::cache_may_go("sccache", NOW - 30 * DAY, &compiling));
}

#[test]
fn old_temporary_entries_go_but_not_recent_ones_or_claude_s() {
    let env = env(vec![PathBuf::from(r"C:\Temp\setup\run.exe")]);
    let old = NOW - 8 * DAY;
    assert!(find::temp_entry_may_go(Path::new(r"C:\Temp\x"), old, &env));
    assert!(!find::temp_entry_may_go(
        Path::new(r"C:\Temp\x"),
        NOW - DAY,
        &env
    ));
    assert!(!find::temp_entry_may_go(
        Path::new(r"C:\Temp\claude"),
        old,
        &env
    ));
    assert!(!find::temp_entry_may_go(
        Path::new(r"C:\Temp\setup"),
        old,
        &env
    ));
}

/// A disk whose free space grows with what is removed.
struct Fake {
    free: Cell<u64>,
    offered: Vec<Candidate>,
    removed: RefCell<Vec<String>>,
}

impl Source for Fake {
    fn free(&self) -> u64 {
        self.free.get()
    }
    fn candidates(&mut self, step: Step) -> Vec<Candidate> {
        self.offered
            .iter()
            .filter(|c| c.step == step)
            .cloned()
            .collect()
    }
    fn remove(&self, candidate: &Candidate) -> Removed {
        self.free.set(self.free.get() + candidate.bytes);
        self.removed
            .borrow_mut()
            .push(candidate.path.to_string_lossy().into_owned());
        Removed {
            bytes: candidate.bytes,
            reason: None,
        }
    }
}

fn offer(name: &str, step: Step, bytes: u64) -> Candidate {
    Candidate {
        step,
        path: PathBuf::from(name),
        removal: Removal::Whole,
        bytes,
        label: String::new(),
        newest: 0,
    }
}

#[test]
fn steps_go_on_only_while_the_disk_is_low_and_stop_at_twice_the_threshold() {
    let disk = DiskSpace {
        root: r"D:\".into(),
        total: 100 * GIB,
        free: 2 * GIB,
    };
    // Idle output is cleaned whole, even past the goal.
    let mut fake = Fake {
        free: Cell::new(2 * GIB),
        offered: vec![
            offer("idle1", Step::IdleBuild, 6 * GIB),
            offer("idle2", Step::IdleBuild, 6 * GIB),
            offer("build", Step::Build, GIB),
        ],
        removed: RefCell::new(Vec::new()),
    };
    let record = run::run_disk(&disk, 5, false, NOW, &mut fake);
    assert_eq!(*fake.removed.borrow(), vec!["idle1", "idle2"]);
    assert_eq!(record.freed, 12 * GIB);
    assert_eq!(record.free_after, 14 * GIB);
    assert_eq!(record.steps.len(), 1);

    // Still low after it: build output in use goes until the goal, then caches are not needed.
    let mut fake = Fake {
        free: Cell::new(2 * GIB),
        offered: vec![
            offer("idle", Step::IdleBuild, GIB),
            offer("b1", Step::Build, 4 * GIB),
            offer("b2", Step::Build, 4 * GIB),
            offer("b3", Step::Build, 4 * GIB),
            offer("cache", Step::Caches, 4 * GIB),
        ],
        removed: RefCell::new(Vec::new()),
    };
    let record = run::run_disk(&disk, 5, true, NOW, &mut fake);
    assert_eq!(*fake.removed.borrow(), vec!["idle", "b1", "b2"]);
    assert_eq!(record.free_after, 11 * GIB);
    assert!(record.manual);

    // Nothing to remove: the run is still written down, with no steps.
    let mut fake = Fake {
        free: Cell::new(2 * GIB),
        offered: Vec::new(),
        removed: RefCell::new(Vec::new()),
    };
    let record = run::run_disk(&disk, 5, false, NOW, &mut fake);
    assert!(record.steps.is_empty());
    assert_eq!(record.freed, 0);
}

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![1u8; bytes]).unwrap();
}

#[test]
fn a_walk_finds_projects_under_source_control_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("work").join("app").join("Cargo.toml"), 10);
    write(
        &root.join("work").join("app").join(".git").join("index"),
        10,
    );
    write(
        &root
            .join("work")
            .join("app")
            .join("inner")
            .join("package.json"),
        10,
    );
    // A repository whose projects sit one level down.
    write(&root.join("mono").join(".git").join("HEAD"), 10);
    write(
        &root
            .join("mono")
            .join("packages")
            .join("ui")
            .join("package.json"),
        10,
    );
    // Not under source control: a program installed on this disk, a downloaded package.
    write(
        &root
            .join("Editor")
            .join("resources")
            .join("app")
            .join("package.json"),
        10,
    );
    write(
        &root.join("downloads").join("tool").join("package.json"),
        10,
    );
    // Hidden and dependency folders are never walked.
    write(
        &root.join(".cache").join("pkg").join(".git").join("HEAD"),
        10,
    );
    write(&root.join(".cache").join("pkg").join("package.json"), 10);
    write(
        &root.join("node_modules").join("x").join("package.json"),
        10,
    );
    write(&root.join("photos").join("a.jpg"), 10);
    let guard = env(Vec::new()).guard;
    let mut found: Vec<String> = find::walk_projects(root, &guard)
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    found.sort();
    assert_eq!(found, vec!["mono/packages/ui", "work/app"]);
}

#[test]
fn programs_and_packages_are_never_taken_for_projects() {
    for path in [
        r"D:\Microsoft VS Code\f6cfa2ea24\resources\app",
        r"D:\AITools\WorkBuddy\resources\app.asar.unpacked\cli",
        r"E:\AI_Models\envs\tts\Lib\site-packages\gradio",
        r"C:\Users\me\AppData\Roaming\npm\node_modules\pkg",
    ] {
        assert!(find::installed(Path::new(path)), "{path}");
    }
    assert!(!find::installed(Path::new(r"E:\work\app")));
    assert!(!find::installed(Path::new(
        r"E:\work\resources\application"
    )));

    let env = env(Vec::new());
    let project = Project {
        path: PathBuf::from(r"E:\work\app"),
        activity: 0,
    };
    let items = vec![
        item(
            r"E:\work\app\video\out",
            JunkKind::Rebuildable,
            GIB,
            NOW - 30 * DAY,
        ),
        item(
            r"E:\work\app\py\Lib\site-packages\x\dist",
            JunkKind::Rebuildable,
            GIB,
            NOW - 30 * DAY,
        ),
        item(
            r"E:\work\app\dist",
            JunkKind::Rebuildable,
            GIB,
            NOW - 30 * DAY,
        ),
    ];
    assert_eq!(
        steps_of(&find::project_candidates(&project, &report(items), &env)),
        vec![("dist".into(), Step::IdleBuild)],
        "rendered output and installed packages stay"
    );
}

#[cfg(windows)]
fn junction(target: &Path, link: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(unix)]
fn junction(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn removing_unlinks_links_without_touching_what_they_point_at() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("keep.bin"), 100);
    let cache = dir.path().join("cache");
    write(&cache.join("a").join("one.bin"), 5000);
    write(&cache.join("two.bin"), 5000);
    junction(outside.path(), &cache.join("linked"));

    let removed = remove::remove_path(&cache, false);
    assert!(removed.bytes >= 10_000);
    assert_eq!(removed.reason, None);
    assert!(cache.is_dir(), "the folder itself stays");
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
    assert!(outside.path().join("keep.bin").exists());

    write(&cache.join("three.bin"), 10);
    let removed = remove::remove_path(&cache, true);
    assert_eq!(removed.reason, None);
    assert!(!cache.exists());
}

#[test]
fn a_temporary_folder_loses_only_the_entries_that_may_go() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("old").join("a.bin"), 10);
    write(&dir.path().join("new.bin"), 10);
    let removed = remove::remove(
        dir.path(),
        Removal::OldEntries(0),
        &|path: &Path, _: u64| path.file_name().is_some_and(|n| n == "old"),
    );
    assert_eq!(removed.reason, None);
    assert!(!dir.path().join("old").exists());
    assert!(dir.path().join("new.bin").exists());
}

#[test]
fn a_record_lists_what_went_and_why_something_stayed() {
    let disk = DiskSpace {
        root: r"D:\".into(),
        total: 100 * GIB,
        free: GIB,
    };
    struct Stuck;
    impl Source for Stuck {
        fn free(&self) -> u64 {
            GIB
        }
        fn candidates(&mut self, step: Step) -> Vec<Candidate> {
            if step == Step::Caches {
                vec![
                    offer("locked", Step::Caches, GIB),
                    offer("gone", Step::Caches, GIB),
                ]
            } else {
                Vec::new()
            }
        }
        fn remove(&self, candidate: &Candidate) -> Removed {
            if candidate.path == Path::new("locked") {
                Removed {
                    bytes: 0,
                    reason: Some(remove::REASON_ACCESS_DENIED),
                }
            } else {
                Removed::default()
            }
        }
    }
    let record = run::run_disk(&disk, 5, false, NOW, &mut Stuck);
    assert_eq!(record.steps.len(), 1);
    assert_eq!(record.steps[0].step, Step::Caches);
    assert_eq!(
        record.steps[0].items.len(),
        1,
        "an item that freed nothing and failed nothing is not listed"
    );
    assert_eq!(
        record.steps[0].items[0].error.as_deref(),
        Some(remove::REASON_ACCESS_DENIED)
    );
}

/// Sets the write time of everything under `path`, folders included, `secs` into the past.
fn age(path: &Path, secs: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(secs);
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap().flatten() {
            age(&entry.path(), secs);
        }
    }
    let mut options = fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        options
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    #[cfg(not(windows))]
    options.read(true);
    options.open(path).unwrap().set_modified(when).unwrap();
}

/// The whole cleanup on real files, with a temporary folder standing in for a disk that is
/// low and stays low, so every step runs.
#[test]
fn a_low_disk_loses_build_output_caches_and_old_temporary_files_and_nothing_else() {
    let disk = tempfile::tempdir().unwrap();
    let root = disk.path();
    let mib = 2 << 20;
    let app = root.join("work").join("app");
    write(&app.join(".git").join("HEAD"), 10);
    write(&app.join("Cargo.toml"), 10);
    write(&app.join("package.json"), 10);
    write(&app.join("src").join("main.rs"), 100);
    write(&app.join("target").join("CACHEDIR.TAG"), 10);
    write(&app.join("target").join("debug").join("app.bin"), mib);
    write(&app.join("node_modules").join("x").join("index.js"), mib);
    write(&app.join(".venv").join("pyvenv.cfg"), 10);
    write(&app.join(".venv").join("lib.bin"), mib);
    write(&app.join("video").join("package.json"), 10);
    write(&app.join("video").join("out").join("promo.mp4"), mib);
    age(&app, 30 * DAY);
    let tmp = root.join("tmp");
    write(&tmp.join("old-entry").join("a.bin"), mib);
    write(&tmp.join("claude").join("session").join("b.bin"), mib);
    age(&tmp, 10 * DAY);
    write(&tmp.join("new.bin"), mib);
    let local = root.join("local");
    write(&local.join("D3DSCache").join("shader.bin"), mib);

    let env = Env {
        guard: Guard {
            now: crate::sessions::now(),
            temp_claude: PathBuf::new(),
            file_history: PathBuf::new(),
            home: PathBuf::from(r"C:\Users\someone"),
            protected: Vec::new(),
            running: Vec::new(),
        },
        inactive_days: 7,
        sessions: Vec::new(),
        temp_dirs: vec![tmp.clone()],
        local: local.clone(),
        roaming: root.join("roaming"),
    };
    let disk_root = root.to_string_lossy().into_owned();
    let free = super::space_of(&disk_root).unwrap().1;
    let low = DiskSpace {
        root: disk_root.clone(),
        total: free.saturating_mul(50),
        free,
    };
    let mut source = run::System::new(disk_root.clone(), vec![disk_root], &env, Box::new(|_| {}));
    let record = run::run_disk(&low, 5, true, env.guard.now, &mut source);

    let steps: Vec<Step> = record.steps.iter().map(|s| s.step).collect();
    assert_eq!(
        steps,
        vec![Step::IdleBuild, Step::Caches, Step::System],
        "{record:#?}"
    );
    assert!(record.freed >= 3 * mib as u64, "{record:#?}");
    assert!(!app.join("target").exists(), "idle build output goes");
    assert!(
        !app.join("node_modules").exists(),
        "an idle project's dependencies go"
    );
    assert!(
        !tmp.join("old-entry").exists(),
        "a week-old temporary entry goes"
    );
    for kept in [
        app.join("src").join("main.rs"),
        app.join("Cargo.toml"),
        app.join(".venv").join("lib.bin"),
        app.join("video").join("out").join("promo.mp4"),
        tmp.join("new.bin"),
        tmp.join("claude").join("session").join("b.bin"),
    ] {
        assert!(kept.exists(), "{} stays", kept.display());
    }
    if cfg!(windows) {
        assert!(
            local.join("D3DSCache").is_dir(),
            "a cache folder stays, emptied"
        );
        assert!(!local.join("D3DSCache").join("shader.bin").exists());
    }
}

/// What this machine's disks would give up, step by step, without removing anything:
/// `cargo test --lib live_low_space_plan -- --ignored --nocapture`
#[test]
#[ignore]
fn live_low_space_plan() {
    let env = watch::system_env();
    let disks = super::disks();
    let roots: Vec<String> = disks.iter().map(|d| d.root.clone()).collect();
    for disk in disks {
        let started = std::time::Instant::now();
        println!(
            "{} free {:.1} / {:.1} GB",
            disk.root,
            disk.free as f64 / GIB as f64,
            disk.total as f64 / GIB as f64
        );
        let mut source = run::System::new(disk.root.clone(), roots.clone(), &env, Box::new(|_| {}));
        for step in Step::ALL {
            let found = source.candidates(step);
            let total: u64 = found.iter().map(|c| c.bytes).sum();
            println!(
                "  {step:?}: {} items, {:.2} GB",
                found.len(),
                total as f64 / GIB as f64
            );
            for c in found.iter().take(6) {
                println!(
                    "    {:>7.2} GB  {}  {}",
                    c.bytes as f64 / GIB as f64,
                    c.label,
                    c.path.display()
                );
            }
        }
        println!("  took {:?}", started.elapsed());
    }
}
