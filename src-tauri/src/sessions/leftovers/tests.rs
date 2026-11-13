use super::model::LeftoverKind;
use super::scan::{scan_with, why_skipped, Env, ORPHAN_KEY};
use super::settings::Settings;
use crate::sessions::catalog::tests_support::session;
use crate::sessions::model::{Agent, ProjectRef, Session};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const DAY: u64 = 86_400;
const NOW: u64 = 1_800_000_000;
const MB: usize = 1 << 20;
const TARGET: u64 = 12 * (1 << 20) + 10;

fn write(path: &Path, bytes: usize, at: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; bytes]).unwrap();
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(at);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

struct Machine {
    _dir: tempfile::TempDir,
    root: PathBuf,
    env: Env,
}

fn machine() -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let env = Env {
        now: NOW,
        temp_claude: root.join("temp").join("claude"),
        file_history: root.join("claude").join("file-history"),
        home: root.join("home"),
        protected: vec![root.join("claude"), root.join("codex"), root.join("temp")],
        running: Vec::new(),
    };
    Machine {
        _dir: dir,
        root,
        env,
    }
}

fn work(agent: Agent, id: &str, project: &Path, updated: u64) -> Session {
    let mut s = session(id);
    s.agent = agent;
    s.updated_at = updated;
    s.project = ProjectRef {
        key: project.to_string_lossy().into_owned(),
        name: project.file_name().unwrap().to_string_lossy().into_owned(),
        path: project.to_string_lossy().into_owned(),
        exists: true,
    };
    s
}

fn rust_project(root: &Path, built: u64) -> PathBuf {
    let project = root.join("projects").join("app");
    write(&project.join("Cargo.toml"), 10, built);
    write(&project.join("target").join("CACHEDIR.TAG"), 10, built);
    write(
        &project.join("target").join("debug").join("app.exe"),
        4 * MB,
        built,
    );
    write(
        &project
            .join("target")
            .join("debug")
            .join("incremental")
            .join("a.bin"),
        2 * MB,
        built,
    );
    write(
        &project.join("target").join("release").join("app.exe"),
        6 * MB,
        built - 30 * DAY,
    );
    project
}

fn kinds(scan: &super::model::Scan, key: &str) -> Vec<(LeftoverKind, bool)> {
    scan.report
        .projects
        .iter()
        .find(|p| p.key == key)
        .map(|p| p.items.iter().map(|i| (i.kind, i.recommended)).collect())
        .unwrap_or_default()
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

#[test]
fn an_idle_project_gives_up_its_build_outputs() {
    let m = machine();
    let project = rust_project(&m.root, NOW - 20 * DAY);
    let sessions = vec![work(Agent::Codex, "a", &project, NOW - 20 * DAY)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    let found = kinds(&scan, &key(&project));
    assert_eq!(found, vec![(LeftoverKind::Build, true)]);
    let project_row = &scan.report.projects[0];
    assert!(!project_row.active);
    assert_eq!(scan.report.recommended, TARGET);
}

#[test]
fn a_project_in_use_keeps_its_build_but_drops_old_profiles() {
    let m = machine();
    let project = rust_project(&m.root, NOW - DAY);
    let sessions = vec![work(Agent::Claude, "a", &project, NOW - 20 * DAY)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    let row = scan
        .report
        .projects
        .iter()
        .find(|p| p.key == key(&project))
        .unwrap();
    assert!(row.active, "the build was written yesterday");
    let build = row
        .items
        .iter()
        .find(|i| i.kind == LeftoverKind::Build)
        .unwrap();
    assert!(!build.recommended);
    let stale = row
        .items
        .iter()
        .find(|i| i.kind == LeftoverKind::Stale)
        .unwrap();
    assert!(stale.recommended);
    assert_eq!(stale.bytes, 6 * MB as u64);
    let incremental = row
        .items
        .iter()
        .find(|i| i.kind == LeftoverKind::Incremental)
        .unwrap();
    assert!(
        !incremental.recommended,
        "a slower build may make an agent time out"
    );
    assert_eq!(incremental.bytes, 2 * MB as u64);
    assert!(stale.path.contains("release"));
    let operation = &scan.operations[&stale.id];
    assert_eq!(
        operation.paths,
        vec![project.join("target").join("release")]
    );
}

#[test]
fn a_project_in_use_gives_up_a_build_nobody_built_lately() {
    let m = machine();
    let project = rust_project(&m.root, NOW - 20 * DAY);
    let sessions = vec![work(Agent::Claude, "a", &project, NOW - 3600)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    let row = &scan.report.projects[0];
    assert!(row.active, "a session ran an hour ago");
    assert_eq!(
        kinds(&scan, &key(&project)),
        vec![(LeftoverKind::Build, true)]
    );
}

#[test]
fn dependencies_of_a_project_in_use_stay_however_old() {
    let m = machine();
    let project = m.root.join("projects").join("web");
    write(&project.join("package.json"), 10, NOW - 90 * DAY);
    write(
        &project.join("node_modules").join("a").join("index.js"),
        2 * MB,
        NOW - 90 * DAY,
    );
    let sessions = vec![work(Agent::Codex, "a", &project, NOW - 3600)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    assert_eq!(
        kinds(&scan, &key(&project)),
        vec![(LeftoverKind::Build, false)]
    );
}

#[test]
fn a_running_program_blocks_its_folder() {
    let mut m = machine();
    let project = rust_project(&m.root, NOW - 20 * DAY);
    m.env.running = vec![project.join("target").join("debug").join("app.exe")];
    let sessions = vec![work(Agent::Codex, "a", &project, NOW - 20 * DAY)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    let build = &scan.report.projects[0].items[0];
    assert!(build.blocked.as_deref().unwrap().contains("app.exe"));
    assert!(!build.recommended);
}

#[test]
fn home_drive_roots_and_agent_folders_are_not_looked_into() {
    let m = machine();
    fs::create_dir_all(m.root.join("home").join("code")).unwrap();
    fs::create_dir_all(m.root.join("codex").join("worktrees").join("x")).unwrap();
    assert_eq!(
        why_skipped(&m.root.join("home"), &m.env),
        Some("是用户主目录或它的上级")
    );
    assert_eq!(why_skipped(&m.root, &m.env), Some("是用户主目录或它的上级"));
    assert!(why_skipped(&m.root.join("codex").join("worktrees").join("x"), &m.env).is_some());
    assert!(why_skipped(&m.root.join("missing"), &m.env).is_some());
    assert!(why_skipped(Path::new(""), &m.env).is_some());
    assert_eq!(why_skipped(&m.root.join("home").join("code"), &m.env), None);
}

#[test]
fn a_nested_project_owns_its_own_build() {
    let m = machine();
    let outer = m.root.join("projects").join("mono");
    let inner = outer.join("web");
    write(&outer.join("package.json"), 10, NOW - 20 * DAY);
    write(&inner.join("package.json"), 10, NOW - 20 * DAY);
    write(
        &inner.join("node_modules").join("a").join("index.js"),
        2 * MB,
        NOW - 20 * DAY,
    );
    let sessions = vec![
        work(Agent::Codex, "a", &outer, NOW - 20 * DAY),
        work(Agent::Claude, "b", &inner, NOW - 20 * DAY),
    ];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    assert!(
        kinds(&scan, &key(&outer)).is_empty(),
        "the outer project lists nothing of the inner one"
    );
    assert_eq!(
        kinds(&scan, &key(&inner)),
        vec![(LeftoverKind::Build, true)]
    );
}

#[test]
fn old_sessions_give_up_scratch_folders_and_checkpoints() {
    let m = machine();
    let project = m.root.join("projects").join("notes");
    fs::create_dir_all(&project).unwrap();
    let old = "11111111-2222-3333-4444-555555555555";
    let fresh = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let lost = "99999999-8888-7777-6666-555555555555";
    for id in [old, fresh, lost] {
        write(
            &m.env
                .temp_claude
                .join("D--notes")
                .join(id)
                .join("scratchpad")
                .join("a.txt"),
            2 * MB,
            NOW - 40 * DAY,
        );
    }
    write(
        &m.env.file_history.join(old).join("f@v1"),
        3 * MB,
        NOW - 40 * DAY,
    );
    write(&m.env.file_history.join(fresh).join("f@v1"), 3 * MB, NOW);
    let mut sessions = vec![
        work(Agent::Claude, old, &project, NOW - 40 * DAY),
        work(Agent::Claude, fresh, &project, NOW - 3600),
    ];
    sessions[0].native_id = old.into();
    sessions[1].native_id = fresh.into();
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    let row = scan
        .report
        .projects
        .iter()
        .find(|p| p.key == key(&project))
        .unwrap();
    let temp = row
        .items
        .iter()
        .find(|i| i.kind == LeftoverKind::Temp)
        .unwrap();
    assert_eq!(temp.bytes, 2 * MB as u64, "only the old session's folder");
    let checkpoints = row
        .items
        .iter()
        .find(|i| i.kind == LeftoverKind::Checkpoints)
        .unwrap();
    assert_eq!(checkpoints.bytes, 3 * MB as u64);
    let orphan = scan
        .report
        .projects
        .iter()
        .find(|p| p.key == ORPHAN_KEY)
        .unwrap();
    assert_eq!(
        orphan.items[0].bytes,
        2 * MB as u64,
        "a folder no session lists"
    );
}

#[test]
fn screenshots_are_offered_only_when_slimming_is_on() {
    let m = machine();
    let project = m.root.join("projects").join("ui");
    fs::create_dir_all(&project).unwrap();
    let transcript = m.root.join("codex").join("sessions").join("rollout.jsonl");
    let image = format!(
        r#"{{"u":"data:image/png;base64,{}"}}"#,
        "QUJD".repeat(300_000)
    );
    write(&transcript, 0, NOW - 40 * DAY);
    fs::write(&transcript, format!("{image}\n")).unwrap();
    let mut s = work(Agent::Codex, "a", &project, NOW - 40 * DAY);
    s.path = transcript.to_string_lossy().into_owned();
    let off = scan_with(std::slice::from_ref(&s), &Settings::default(), &m.env);
    assert!(kinds(&off, &key(&project)).is_empty());
    let on = Settings {
        slim: true,
        ..Settings::default()
    };
    let scan = scan_with(&[s], &on, &m.env);
    assert_eq!(
        kinds(&scan, &key(&project)),
        vec![(LeftoverKind::Images, true)]
    );
    let item = &scan.report.projects[0].items[0];
    assert!(scan.operations[&item.id].slim);
}

#[test]
fn other_agents_are_left_to_their_own_pages() {
    let m = machine();
    let project = rust_project(&m.root, NOW - 20 * DAY);
    let sessions = vec![work(Agent::Kimi, "a", &project, NOW - 20 * DAY)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    assert!(scan.report.projects.is_empty());
}

#[test]
fn cleaning_deletes_inside_the_root_and_reports_what_was_freed() {
    let m = machine();
    let project = rust_project(&m.root, NOW - 20 * DAY);
    let sessions = vec![work(Agent::Codex, "a", &project, NOW - 20 * DAY)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    let ids: Vec<String> = scan.report.projects[0]
        .items
        .iter()
        .map(|i| i.id.clone())
        .collect();
    let bytes = scan.report.projects[0]
        .items
        .iter()
        .map(|i| (i.id.clone(), i.bytes))
        .collect();
    let items = super::clean::verify(&scan, &ids, &bytes).unwrap();
    let job = super::clean::run_items(items, &|_| {});
    assert_eq!(job.state, "completed");
    assert_eq!(job.freed, TARGET);
    assert!(!project.join("target").exists());
    assert!(project.join("Cargo.toml").exists());
}

/// Live: slims a copy of a real transcript and checks every line.
/// `STACKER_SLIM_SAMPLE=<path> cargo test --lib live_slim_copy -- --ignored --nocapture`
#[test]
#[ignore]
fn live_slim_copy() {
    let sample =
        PathBuf::from(std::env::var("STACKER_SLIM_SAMPLE").expect("set STACKER_SLIM_SAMPLE"));
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join(sample.file_name().unwrap());
    fs::copy(&sample, &copy).unwrap();
    let before = fs::read(&copy).unwrap();
    let started = std::time::Instant::now();
    let saving = super::slim::slim_file(&copy).unwrap();
    let after = fs::read(&copy).unwrap();
    let old: Vec<&[u8]> = before.split(|b| *b == b'\n').collect();
    let new: Vec<&[u8]> = after.split(|b| *b == b'\n').collect();
    assert_eq!(old.len(), new.len(), "same number of lines");
    let mut changed = 0;
    for (a, b) in old.iter().zip(&new) {
        if a == b {
            continue;
        }
        changed += 1;
        serde_json::from_slice::<serde_json::Value>(b).expect("a changed line is still JSON");
    }
    println!(
        "{}: {:.0} MB -> {:.0} MB, {} images in {} lines, {:?}",
        sample.display(),
        before.len() as f64 / 1048576.0,
        after.len() as f64 / 1048576.0,
        saving.images,
        changed,
        started.elapsed()
    );
    assert_eq!(before.len() - after.len(), saving.bytes as usize);
}

#[test]
fn a_build_folder_counts_once_with_its_parts() {
    let m = machine();
    let project = rust_project(&m.root, NOW - DAY);
    let sessions = vec![work(Agent::Claude, "a", &project, NOW - 3600)];
    let scan = scan_with(&sessions, &Settings::default(), &m.env);
    assert_eq!(scan.report.total, TARGET, "the parts are not added again");
    let ids: Vec<String> = scan.report.projects[0]
        .items
        .iter()
        .map(|i| i.id.clone())
        .collect();
    let preview = super::clean::preview_with(&scan, ids).unwrap();
    assert_eq!(preview.items.len(), 1, "the whole folder covers its parts");
    assert_eq!(preview.bytes, TARGET);
}
