//! Watching the disks: a look every five minutes, and a cleanup of each one that is low.

use super::find::{self, Env};
use super::model::{CheckOutcome, DiskSpace, Progress, RunRecord, Step};
use super::run::{self, System};
use super::settings;
use crate::sessions::leftovers::scan::Env as Guard;
use crate::space_analysis::history::{HistoryItem, HistoryRecord};
use crate::space_analysis::model::{CleanupItemState, CleanupTaskState};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

const FIRST_LOOK: Duration = Duration::from_secs(90);
const LOOK_EVERY: Duration = Duration::from_secs(300);
const HOUR: u64 = 3600;
const DAY: u64 = 86_400;
const GIB: u64 = 1 << 30;

/// What the page hears when a cleanup finished, with its record.
pub const EVENT: &str = "low-space-cleaned";
/// What the page hears while one runs: the disk and the step, or nothing once it is over.
pub const PROGRESS_EVENT: &str = "low-space-progress";

const HISTORY_TITLE: &str = "空间不足自动清理";
const NO_ADMIN: &str = "Windows 系统临时目录需要管理员权限，没有清理";

static BUSY: AtomicBool = AtomicBool::new(false);
static RUNNING: Mutex<Option<Progress>> = Mutex::new(None);

pub fn running() -> Option<Progress> {
    RUNNING.lock().ok().and_then(|slot| slot.clone())
}

fn set_running(app: &tauri::AppHandle, progress: Option<Progress>) {
    if let Ok(mut slot) = RUNNING.lock() {
        *slot = progress.clone();
    }
    let _ = app.emit(PROGRESS_EVENT, progress);
}

fn same_root(left: &str, right: &str) -> bool {
    find::norm(Path::new(left)) == find::norm(Path::new(right))
}

/// Whether a disk that is low again is cleaned again: not within an hour of its last run,
/// and then once more than a gigabyte has been used since, or a day has passed.
pub fn due(runs: &[RunRecord], disk: &DiskSpace, now: u64) -> bool {
    let Some(last) = runs.iter().find(|run| same_root(&run.drive, &disk.root)) else {
        return true;
    };
    let since = now.saturating_sub(last.finished_at);
    since >= DAY || (since >= HOUR && disk.free.saturating_add(GIB) < last.free_after)
}

/// The disks to clean now. Asked for from the page, every low disk is.
pub fn low_disks(
    disks: &[DiskSpace],
    threshold_percent: u8,
    runs: &[RunRecord],
    now: u64,
    manual: bool,
) -> Vec<DiskSpace> {
    disks
        .iter()
        .filter(|disk| run::is_low(disk.free, disk.total, threshold_percent))
        .filter(|disk| manual || due(runs, disk, now))
        .cloned()
        .collect()
}

#[cfg(windows)]
fn elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: the token handle is closed once read; the answer is a plain struct the call fills.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
fn elevated() -> bool {
    false
}

pub(super) fn system_env() -> Env {
    let (sessions, roots) = match crate::sessions::commands::annotated_catalog() {
        Ok((sessions, _, roots)) => (sessions, roots),
        Err(error) => {
            log::warn!("low-space cleanup could not read the sessions: {error}");
            (
                Vec::new(),
                crate::sessions::roots::resolve(&Default::default()),
            )
        }
    };
    let mut projects: HashMap<String, (PathBuf, u64)> = HashMap::new();
    for session in &sessions {
        if session.project.path.is_empty() {
            continue;
        }
        let path = PathBuf::from(&session.project.path);
        let entry = projects.entry(find::norm(&path)).or_insert((path, 0));
        entry.1 = entry.1.max(session.updated_at);
    }
    let mut temp_dirs = vec![std::env::temp_dir()];
    if cfg!(windows) && elevated() {
        temp_dirs.extend(run::system_temp());
    }
    Env {
        guard: Guard::system(&roots),
        inactive_days: crate::sessions::leftovers::settings::load().inactive_days,
        sessions: projects.into_values().collect(),
        temp_dirs,
        local: dirs::data_local_dir().unwrap_or_default(),
        roaming: dirs::data_dir().unwrap_or_default(),
    }
}

fn history_record(run: &RunRecord) -> HistoryRecord {
    HistoryRecord {
        at: run.finished_at,
        state: CleanupTaskState::Completed,
        released_bytes: run.freed,
        items: run
            .steps
            .iter()
            .flat_map(|step| step.items.iter())
            .map(|item| HistoryItem {
                path: item.path.clone(),
                state: if item.freed == 0 && item.error.is_some() {
                    CleanupItemState::Failed
                } else {
                    CleanupItemState::Completed
                },
                released_bytes: item.freed,
                reason_key: item.error.clone(),
            })
            .collect(),
        title: Some(format!(
            "{HISTORY_TITLE} {}",
            run.drive.trim_end_matches('\\')
        )),
    }
}

/// Resets the run state however a cleanup ends.
struct Busy<'a>(&'a tauri::AppHandle);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        set_running(self.0, None);
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn clean(app: &tauri::AppHandle, low: Vec<DiskSpace>, threshold_percent: u8, manual: bool) {
    let _busy = Busy(app);
    let env = system_env();
    let roots: Vec<String> = super::disks().into_iter().map(|disk| disk.root).collect();
    for disk in low {
        set_running(
            app,
            Some(Progress {
                drive: disk.root.clone(),
                step: None,
            }),
        );
        let drive = disk.root.clone();
        let on_step = Box::new(move |step: Step| {
            set_running(
                app,
                Some(Progress {
                    drive: drive.clone(),
                    step: Some(step),
                }),
            )
        });
        let mut source = System::new(disk.root.clone(), roots.clone(), &env, on_step);
        let now = crate::sessions::now();
        let mut record = run::run_disk(&disk, threshold_percent, manual, now, &mut source);
        let system_temp_here = run::system_temp().is_some_and(|temp| {
            find::disk_of(&temp, &roots).is_some_and(|d| same_root(d, &disk.root))
        });
        if system_temp_here
            && !elevated()
            && run::is_low(record.free_after, record.total, threshold_percent)
        {
            record.notes.push(NO_ADMIN.into());
        }
        log::info!(
            "low-space cleanup on {}: freed={} free {} -> {} manual={}",
            record.drive,
            record.freed,
            record.free_before,
            record.free_after,
            manual
        );
        settings::remember(record.clone());
        crate::space_analysis::history::append_record(history_record(&record));
        crate::space_analysis::known::forget_known();
        let _ = app.emit(EVENT, &record);
    }
}

fn look(app: &tauri::AppHandle) {
    let settings = settings::load();
    if !settings.enabled {
        return;
    }
    let low = low_disks(
        &super::disks(),
        settings.threshold_percent,
        &settings.runs,
        crate::sessions::now(),
        false,
    );
    if low.is_empty() || BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    clean(app, low, settings.threshold_percent, false);
}

/// Looks a minute and a half after start, then every five minutes.
pub fn start(app: tauri::AppHandle) {
    let started = std::thread::Builder::new()
        .name("low-space".into())
        .spawn(move || {
            std::thread::sleep(FIRST_LOOK);
            loop {
                look(&app);
                std::thread::sleep(LOOK_EVERY);
            }
        });
    if let Err(error) = started {
        log::warn!("low-space watch could not start: {error}");
    }
}

pub fn check_now(app: tauri::AppHandle) -> Result<CheckOutcome, String> {
    let settings = settings::load();
    let disks = super::disks();
    let low = low_disks(
        &disks,
        settings.threshold_percent,
        &settings.runs,
        crate::sessions::now(),
        true,
    );
    let outcome = CheckOutcome {
        cleaning: low.iter().map(|disk| disk.root.clone()).collect(),
        disks,
    };
    if low.is_empty() {
        return Ok(outcome);
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("E_BUSY".into());
    }
    let threshold = settings.threshold_percent;
    std::thread::Builder::new()
        .name("low-space-now".into())
        .spawn(move || clean(&app, low, threshold, true))
        .map_err(|error| {
            BUSY.store(false, Ordering::SeqCst);
            error.to_string()
        })?;
    Ok(outcome)
}
