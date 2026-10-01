//! 自动锁定：Stacker 窗口内无操作达设定时长、Windows 锁屏、系统睡眠（墙钟跳变）。

use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::{Duration, SystemTime};
use tauri::Emitter;

const TICK: Duration = Duration::from_secs(5);
const SLEEP_GAP: Duration = Duration::from_secs(60);
const ALLOWED_MINUTES: [u16; 4] = [5, 10, 30, 60];
static AUTO_LOCK_MINUTES: AtomicU16 = AtomicU16::new(10);
static STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    Idle,
    Session,
    Sleep,
}

impl Reason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Reason::Idle => "idle",
            Reason::Session => "session",
            Reason::Sleep => "sleep",
        }
    }
}

pub(crate) fn set_auto_lock_minutes(minutes: u16) {
    let minutes = if ALLOWED_MINUTES.contains(&minutes) { minutes } else { 10 };
    AUTO_LOCK_MINUTES.store(minutes, Ordering::Relaxed);
}

fn limit() -> Duration {
    Duration::from_secs(AUTO_LOCK_MINUTES.load(Ordering::Relaxed) as u64 * 60)
}

pub(crate) fn decide(idle: Duration, limit: Duration, session_locked: bool, wall_gap: Duration) -> Option<Reason> {
    if wall_gap >= SLEEP_GAP {
        Some(Reason::Sleep)
    } else if session_locked {
        Some(Reason::Session)
    } else if idle >= limit {
        Some(Reason::Idle)
    } else {
        None
    }
}

/// Locks the vault and forgets everything derived from it.
pub(crate) fn lock_everything() {
    super::vault().lock();
    super::discover::clear();
    super::clipboard::clear_now();
}

/// Starts the watcher thread; later calls in the same process do nothing.
pub(crate) fn start(app: tauri::AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let mut last_wall = SystemTime::now();
        loop {
            std::thread::sleep(TICK);
            let now = SystemTime::now();
            let gap = now.duration_since(last_wall).unwrap_or_default().saturating_sub(TICK);
            last_wall = now;
            let vault = super::vault();
            if !vault.is_open() {
                continue;
            }
            if let Some(reason) = decide(vault.idle_for(), limit(), session_locked(), gap) {
                lock_everything();
                let _ = app.emit("vault-locked", reason.as_str());
            }
        }
    });
}

/// The input desktop cannot be switched to while the workstation is locked.
#[cfg(windows)]
fn session_locked() -> bool {
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, SwitchDesktop, DESKTOP_SWITCHDESKTOP,
    };
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
        if desktop.is_null() {
            return true;
        }
        let switched = SwitchDesktop(desktop) != 0;
        CloseDesktop(desktop);
        !switched
    }
}

#[cfg(not(windows))]
fn session_locked() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn sleep_beats_session_beats_idle() {
        let limit = 10 * MIN;
        assert_eq!(decide(Duration::ZERO, limit, false, Duration::ZERO), None);
        assert_eq!(decide(10 * MIN, limit, false, Duration::ZERO), Some(Reason::Idle));
        assert_eq!(decide(Duration::ZERO, limit, true, Duration::ZERO), Some(Reason::Session));
        assert_eq!(decide(10 * MIN, limit, true, 2 * MIN), Some(Reason::Sleep));
        assert_eq!(decide(Duration::ZERO, limit, false, Duration::from_secs(59)), None);
    }

    #[test]
    fn auto_lock_minutes_fall_back_to_ten() {
        set_auto_lock_minutes(30);
        assert_eq!(limit(), 30 * MIN);
        set_auto_lock_minutes(7);
        assert_eq!(limit(), 10 * MIN);
    }
}
