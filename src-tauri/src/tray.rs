//! The tray icon: a left click shows the window; the right-click menu holds what is done most
//! often (making every place follow the system proxy, the API service, locking the vault) and
//! jumps to pages. The menu is rebuilt each time the pointer reaches the icon, so it shows the
//! current state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IconMenuItem, Menu, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// Pages the "Go to" submenu offers, as `(page id, Chinese, English)`; the names are the sidebar's.
const PAGES: &[(&str, &str, &str)] = &[
    ("overview", "环境体检", "Checkup"),
    ("agents", "安装更新", "Install & Update"),
    ("agent-data", "会话数据", "Sessions & Data"),
    ("gateway-log", "接口日志", "API Log"),
    ("proxy", "代理设置", "Proxy settings"),
    ("cleanup", "磁盘清理", "Disk Cleanup"),
    ("vault", "密钥保管", "Key Vault"),
    ("settings", "偏好设置", "Settings"),
];

const GOTO: &str = "goto:";
const PROXY_PAGE: &str = "goto:proxy";
const FOLLOW: &str = "proxy_follow";

/// What the menu shows, read before it opens.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    /// The Windows proxy, `host:port`, when one is in use.
    system: Option<String>,
    /// Places that do not follow the system, once counted.
    differ: Option<usize>,
    gateway_on: bool,
    gateway_port: u16,
    vault_open: bool,
}

/// One menu line: its id, text and whether it can be clicked.
type Line = (&'static str, String, bool);

struct Lines {
    system: Line,
    follow: Line,
    gateway: Line,
    vault: Line,
}

fn lines(shown: &Shown, english: bool) -> Lines {
    let pick = |zh: &str, en: &str| {
        if english {
            en.to_string()
        } else {
            zh.to_string()
        }
    };
    let system = match &shown.system {
        Some(at) => (
            PROXY_PAGE,
            format!("{}  {at}", pick("系统代理", "System proxy")),
            true,
        ),
        None => (
            PROXY_PAGE,
            pick("系统代理未开启", "System proxy is off"),
            true,
        ),
    };
    // One line whatever the state, so the menu is changed in place and never rebuilt while open.
    let follow = match (&shown.system, shown.differ) {
        (Some(_), None) => (
            FOLLOW,
            pick("各处代理跟随系统", "Make every place follow it"),
            true,
        ),
        (Some(_), Some(0)) => (
            FOLLOW,
            pick("各处代理已跟随系统", "Every place follows it"),
            false,
        ),
        (Some(_), Some(n)) => (
            FOLLOW,
            if english {
                format!("Make every place follow it ({n} differ)")
            } else {
                format!("各处代理跟随系统（{n} 处不同）")
            },
            true,
        ),
        // Taking proxies back out is asked for on the page, where the places are listed.
        (None, Some(n)) if n > 0 => (
            FOLLOW,
            if english {
                format!("{n} place(s) still set to a proxy…")
            } else {
                format!("还有 {n} 处留着代理…")
            },
            true,
        ),
        (None, _) => (
            FOLLOW,
            pick("各处代理跟随系统", "Make every place follow it"),
            false,
        ),
    };
    let gateway = (
        "gateway_toggle",
        format!(
            "{}  {}",
            pick("接口服务", "API service"),
            if english {
                format!("port {}", shown.gateway_port)
            } else {
                format!("端口 {}", shown.gateway_port)
            }
        ),
        true,
    );
    let vault = if shown.vault_open {
        (
            "vault_lock",
            pick("锁定密钥保管", "Lock the key vault"),
            true,
        )
    } else {
        (
            "vault_lock",
            pick("密钥保管已锁定", "Key vault is locked"),
            false,
        )
    };
    Lines {
        system,
        follow,
        gateway,
        vault,
    }
}

/// How many places differ from the system proxy, for the system proxy it was counted
/// against. Counting reads every place (and asks netsh), so it is done off the menu's thread
/// and kept for a while.
static COUNTED: Mutex<Option<(Option<String>, usize, Instant)>> = Mutex::new(None);
static COUNTING: AtomicBool = AtomicBool::new(false);
const COUNT_FOR: Duration = Duration::from_secs(30);

fn system_proxy() -> Option<String> {
    let system = crate::proxy_system::system();
    (system.state == crate::proxy_system::SystemState::On)
        .then(|| crate::proxy_system::first_endpoint(&system.server))
        .filter(|server| !server.is_empty())
}

fn remember(system: Option<String>, differ: usize) {
    *COUNTED.lock().unwrap_or_else(|e| e.into_inner()) = Some((system, differ, Instant::now()));
}

fn counted(system: &Option<String>) -> Option<(usize, Instant)> {
    COUNTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|(against, _, _)| against == system)
        .map(|(_, differ, at)| (*differ, *at))
}

/// Counts again in the background when the last count is old or was for another proxy.
fn recount(app: &AppHandle) {
    let fresh = counted(&system_proxy()).is_some_and(|(_, at)| at.elapsed() < COUNT_FOR);
    if fresh || COUNTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let system = system_proxy();
        let differ = crate::proxy_ledger::sync_report().rows.len();
        remember(system, differ);
        COUNTING.store(false, Ordering::SeqCst);
        update(&app);
    });
}

fn read_state() -> Shown {
    let system = system_proxy();
    let (gateway_on, gateway_port) = crate::gateway::running_port();
    Shown {
        differ: counted(&system).map(|(differ, _)| differ),
        system,
        gateway_on,
        gateway_port,
        vault_open: crate::vault::vault().is_unlocked(),
    }
}

/// The menu's icons: the app's own Tabler glyphs, drawn grey so they read on a light menu
/// and a dark one.
fn icon(name: &str) -> Option<Image<'static>> {
    let bytes: &'static [u8] = match name {
        "show" => include_bytes!("../icons/tray/show.png"),
        "proxy" => include_bytes!("../icons/tray/proxy.png"),
        "follow" => include_bytes!("../icons/tray/follow.png"),
        "lock" => include_bytes!("../icons/tray/lock.png"),
        "overview" => include_bytes!("../icons/tray/overview.png"),
        "agents" => include_bytes!("../icons/tray/agents.png"),
        "agent-data" => include_bytes!("../icons/tray/agent-data.png"),
        "gateway-log" => include_bytes!("../icons/tray/gateway-log.png"),
        "proxy-page" => include_bytes!("../icons/tray/proxy-page.png"),
        "cleanup" => include_bytes!("../icons/tray/cleanup.png"),
        "vault" => include_bytes!("../icons/tray/vault.png"),
        "settings" => include_bytes!("../icons/tray/settings.png"),
        "quit" => include_bytes!("../icons/tray/quit.png"),
        _ => return None,
    };
    Image::from_bytes(bytes).ok()
}

/// The icon of a page in the "Go to" menu: the sidebar's.
fn page_icon(page: &str) -> &'static str {
    match page {
        "proxy" => "proxy-page",
        "overview" => "overview",
        "agents" => "agents",
        "agent-data" => "agent-data",
        "gateway-log" => "gateway-log",
        "cleanup" => "cleanup",
        "vault" => "vault",
        _ => "settings",
    }
}

fn english() -> bool {
    crate::settings::load().locale == "en-US"
}

fn item(
    app: &AppHandle,
    (id, text, enabled): Line,
    glyph: &str,
) -> tauri::Result<IconMenuItem<Wry>> {
    IconMenuItem::with_id(app, id, text, enabled, icon(glyph), None::<&str>)
}

/// The lines that change, kept so they can be updated in place: replacing the menu while it
/// is open closes it.
#[derive(Clone)]
struct Items {
    system: IconMenuItem<Wry>,
    follow: IconMenuItem<Wry>,
    gateway: CheckMenuItem<Wry>,
    vault: IconMenuItem<Wry>,
}

static ITEMS: Mutex<Option<Items>> = Mutex::new(None);

/// Brings the changing lines up to date without touching the menu itself.
fn update(app: &AppHandle) {
    let shown = read_state();
    let lines = lines(&shown, english());
    // Copied out first: off the main thread each change waits for the main thread, which may
    // itself be waiting here.
    let items = ITEMS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(items) = items else {
        let _ = refresh(app);
        return;
    };
    for (item, (_, text, enabled)) in [
        (&items.system, lines.system),
        (&items.follow, lines.follow),
        (&items.vault, lines.vault),
    ] {
        let _ = item.set_text(text);
        let _ = item.set_enabled(enabled);
    }
    let _ = items.gateway.set_text(lines.gateway.1);
    let _ = items.gateway.set_checked(shown.gateway_on);
}

fn create_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let english = english();
    let pick = |zh: &'static str, en: &'static str| if english { en } else { zh };
    let shown = read_state();
    let lines = lines(&shown, english);

    let show = IconMenuItem::with_id(
        app,
        "show",
        pick("打开 Stacker", "Open Stacker"),
        true,
        icon("show"),
        None::<&str>,
    )?;
    let system = item(app, lines.system, "proxy")?;
    let follow = item(app, lines.follow, "follow")?;
    let (id, text, enabled) = lines.gateway;
    let gateway = CheckMenuItem::with_id(app, id, text, enabled, shown.gateway_on, None::<&str>)?;
    let vault = item(app, lines.vault, "lock")?;
    let pages = PAGES
        .iter()
        .map(|(id, zh, en)| {
            IconMenuItem::with_id(
                app,
                format!("{GOTO}{id}"),
                if english { *en } else { *zh },
                true,
                icon(page_icon(id)),
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let page_refs = pages
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect::<Vec<_>>();
    let goto = Submenu::with_items(app, pick("前往", "Go to"), true, &page_refs)?;
    let quit = IconMenuItem::with_id(
        app,
        "quit",
        pick("退出 Stacker", "Quit Stacker"),
        true,
        icon("quit"),
        None::<&str>,
    )?;

    let separator = || PredefinedMenuItem::separator(app);
    let (one, two, three) = (separator()?, separator()?, separator()?);
    let menu = Menu::with_items(
        app,
        &[
            &show, &one, &system, &follow, &gateway, &vault, &two, &goto, &three, &quit,
        ],
    )?;
    *ITEMS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Items {
        system,
        follow,
        gateway,
        vault,
    });
    Ok(menu)
}

/// Builds the menu anew, for a change of language; everything else updates in place.
pub(crate) fn refresh(app: &AppHandle) -> Result<(), String> {
    let menu = create_menu(app).map_err(|error| error.to_string())?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Something that failed says why; the window may be hidden, so a dialog rather than a toast.
fn tell(app: &AppHandle, text: String) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(text)
        .kind(MessageDialogKind::Error)
        .title("Stacker")
        .show(|_| {});
}

/// Writes the system proxy everywhere, as the proxy page's "全部跟随系统" does. The service
/// proxy may ask Windows for approval, so this runs off the menu's thread.
fn follow(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        match crate::proxy_ledger::follow_system(false) {
            Ok(report) => remember(system_proxy(), report.rows.len()),
            Err(error) => {
                *COUNTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
                let english = english();
                tell(
                    &app,
                    match (error.as_str(), english) {
                        ("E_PROXY_ADDR", false) => {
                            "Windows 没有开启系统代理，没有可写入的地址。".into()
                        }
                        ("E_PROXY_ADDR", true) => {
                            "Windows has no system proxy on, so there is no address to write."
                                .into()
                        }
                        (_, false) => format!("有些地方没能跟随系统代理：{error}"),
                        (_, true) => {
                            format!("Some places could not follow the system proxy: {error}")
                        }
                    },
                );
            }
        }
        let _ = app.emit("proxy-changed", ());
        update(&app);
    });
}

fn toggle_gateway(app: &AppHandle) {
    let status = crate::gateway::status();
    let english = english();
    match crate::gateway::set(!status.running, status.port) {
        Ok(after) if !status.running && !after.running => tell(
            app,
            if english {
                format!(
                    "The API service did not start ({}). Open API Service for details.",
                    after.error
                )
            } else {
                format!("接口服务没能启动（{}），到“接口服务”页查看。", after.error)
            },
        ),
        Ok(_) => {}
        Err(error) => tell(app, error),
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "show" => show_main(app),
        // With no system proxy the line points at the leftovers, which the page lists.
        FOLLOW if system_proxy().is_some() => follow(app),
        FOLLOW => {
            show_main(app);
            let _ = app.emit("tray-goto", "proxy");
        }
        "gateway_toggle" => toggle_gateway(app),
        "vault_lock" => {
            crate::vault::guard::lock_everything();
            let _ = app.emit("vault-locked", "manual");
        }
        "quit" => crate::quit(app),
        _ => {
            if let Some(page) = id.strip_prefix(GOTO) {
                show_main(app);
                let _ = app.emit("tray-goto", page);
            }
        }
    }
    update(app);
}

pub(crate) fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = create_menu(app)?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Stacker")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main(tray.app_handle()),
            // Before the menu opens: the proxy or the service may have changed elsewhere.
            TrayIconEvent::Enter { .. }
            | TrayIconEvent::Click {
                button: MouseButton::Right,
                button_state: MouseButtonState::Down,
                ..
            } => {
                update(tray.app_handle());
                recount(tray.app_handle());
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    // Counted now, so the first right-click already knows how many places differ.
    recount(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown() -> Shown {
        Shown {
            system: Some("127.0.0.1:7890".into()),
            differ: None,
            gateway_on: true,
            gateway_port: 8848,
            vault_open: true,
        }
    }

    fn follow_of(shown: Shown) -> (String, bool) {
        let (_, text, enabled) = lines(&shown, false).follow;
        (text, enabled)
    }

    #[test]
    fn the_menu_says_where_the_system_proxy_points() {
        let lines = lines(&shown(), false);
        assert_eq!(lines.system.1, "系统代理  127.0.0.1:7890");
        assert_eq!(lines.gateway.1, "接口服务  端口 8848");
        assert_eq!(
            lines.vault,
            ("vault_lock", "锁定密钥保管".to_string(), true)
        );
        let off = Shown {
            system: None,
            ..shown()
        };
        assert_eq!(super::lines(&off, false).system.1, "系统代理未开启");
        assert_eq!(
            super::lines(&shown(), true).gateway.1,
            "API service  port 8848"
        );
    }

    #[test]
    fn following_the_system_is_offered_while_something_differs() {
        assert_eq!(
            follow_of(shown()),
            ("各处代理跟随系统".into(), true),
            "not counted yet"
        );
        assert_eq!(
            follow_of(Shown {
                differ: Some(3),
                ..shown()
            }),
            ("各处代理跟随系统（3 处不同）".into(), true)
        );
        assert_eq!(
            follow_of(Shown {
                differ: Some(0),
                ..shown()
            }),
            ("各处代理已跟随系统".into(), false)
        );
    }

    #[test]
    fn with_the_system_proxy_off_leftovers_lead_to_the_page() {
        let off = |differ| Shown {
            system: None,
            differ,
            ..shown()
        };
        assert_eq!(follow_of(off(None)), ("各处代理跟随系统".into(), false));
        assert_eq!(follow_of(off(Some(0))), ("各处代理跟随系统".into(), false));
        assert_eq!(follow_of(off(Some(2))), ("还有 2 处留着代理…".into(), true));
    }

    #[test]
    fn a_locked_vault_is_greyed_out() {
        let locked = Shown {
            vault_open: false,
            ..shown()
        };
        assert!(!lines(&locked, false).vault.2);
    }

    #[test]
    fn every_page_offered_exists() {
        let pages = include_str!("../../src/pageState.ts");
        for (id, _, _) in PAGES {
            assert!(pages.contains(&format!("\"{id}\"")), "{id}");
        }
    }
}
