//! The main window starts hidden and appears when the page says its first screen is drawn, so
//! nobody watches an empty webview load. A page that never says so must not leave the app
//! invisible, hence the deadline.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::window::Color;
use tauri::{AppHandle, Manager, Theme, WebviewWindow};

/// Long enough for a slow disk to load the webview, short enough that a failed page still shows.
const SHOW_DEADLINE: Duration = Duration::from_secs(6);

static SHOWN: AtomicBool = AtomicBool::new(false);

fn show_once(app: &AppHandle) -> bool {
    if SHOWN.swap(true, Ordering::SeqCst) {
        return false;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    true
}

#[tauri::command]
pub fn main_window_ready(app: AppHandle) {
    show_once(&app);
}

pub fn show_anyway_after_a_while(app: AppHandle) {
    let started = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_DEADLINE);
        if show_once(&app) {
            log::warn!(
                target: "stacker::window",
                "first screen not ready after {:?}; showing the window anyway",
                started.elapsed()
            );
        }
    });
}

/// The webview's own background before the page paints: the page's, not the default white.
pub fn paint_behind(window: &WebviewWindow, theme: &str) {
    let light = match theme {
        "light" => true,
        "dark" => false,
        _ => matches!(window.theme(), Ok(Theme::Light)),
    };
    let color = if light {
        Color(0xf5, 0xf6, 0xf8, 0xff)
    } else {
        Color(0x13, 0x16, 0x1c, 0xff)
    };
    let _ = window.set_background_color(Some(color));
}
