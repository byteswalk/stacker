//! The once-a-day cleanup, for users who turned it on: only what is ticked by default.

use std::time::Duration;
use tauri::Emitter;

const FIRST_LOOK: Duration = Duration::from_secs(120);
const LOOK_EVERY: Duration = Duration::from_secs(6 * 3600);
const RUN_EVERY: u64 = 23 * 3600;

/// What the page hears after an automatic cleanup.
pub const EVENT: &str = "leftovers-auto-cleaned";

/// Whether an automatic run is due.
pub fn due(settings: &super::settings::Settings, now: u64) -> bool {
    settings.auto && now.saturating_sub(settings.auto_last_run) >= RUN_EVERY
}

fn run_once(app: &tauri::AppHandle) {
    let mut settings = super::settings::load();
    let now = crate::sessions::now();
    if !due(&settings, now) || super::clean::busy() {
        return;
    }
    let outcome = super::scan::run().map(|scan| {
        let items: Vec<_> = scan
            .report
            .projects
            .iter()
            .flat_map(|p| p.items.iter())
            .filter(|item| item.recommended && item.blocked.is_none())
            .filter_map(|item| Some((item.clone(), scan.operations.get(&item.id)?.clone())))
            .collect();
        super::clean::run_items(items, &|_| {})
    });
    settings.auto_last_run = now;
    match outcome {
        Ok(job) => {
            settings.auto_last_freed = job.freed;
            settings.auto_last_error.clear();
            log::info!(
                "leftover cleanup ran by itself: freed={} items={}",
                job.freed,
                job.total
            );
        }
        Err(error) => {
            settings.auto_last_freed = 0;
            settings.auto_last_error = error.clone();
            log::warn!("leftover cleanup could not run by itself: {error}");
        }
    }
    let _ = super::settings::save(&settings);
    let _ = app.emit(EVENT, settings.auto_last_freed);
}

/// Looks two minutes after start, then every six hours; runs at most once a day.
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_LOOK);
        loop {
            run_once(&app);
            std::thread::sleep(LOOK_EVERY);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_runs_only_when_turned_on_and_a_day_has_passed() {
        let mut settings = super::super::settings::Settings::default();
        assert!(!due(&settings, 1_000_000));
        settings.auto = true;
        assert!(due(&settings, 1_000_000));
        settings.auto_last_run = 1_000_000 - 3600;
        assert!(!due(&settings, 1_000_000));
        settings.auto_last_run = 1_000_000 - 24 * 3600;
        assert!(due(&settings, 1_000_000));
    }
}
