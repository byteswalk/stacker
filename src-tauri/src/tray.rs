//! The tray icon: a left click shows the window; the right-click menu holds what is switched
//! on and off most often (terminal proxy, API service, locking the vault) and jumps to pages.
//! The menu is rebuilt each time the pointer reaches the icon, so it shows the current state.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
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

/// What the menu shows, read fresh before it opens.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    proxy_on: bool,
    /// Where the terminal proxy points, or `None` when there is no address to write.
    proxy_at: Option<String>,
    gateway_on: bool,
    gateway_port: u16,
    vault_open: bool,
}

fn read_state() -> Shown {
    let proxy = crate::proxy::status();
    let gateway = crate::gateway::status();
    Shown {
        proxy_on: proxy.enabled,
        proxy_at: proxy
            .endpoint_available
            .then(|| format!("{}:{}", proxy.host, proxy.port)),
        gateway_on: gateway.running,
        gateway_port: gateway.port,
        vault_open: crate::vault::vault().is_unlocked(),
    }
}

/// The label of each switch, and whether it can be clicked.
fn labels(shown: &Shown, english: bool) -> [(String, bool); 3] {
    let pick = |zh: &str, en: &str| {
        if english {
            en.to_string()
        } else {
            zh.to_string()
        }
    };
    let proxy = match &shown.proxy_at {
        Some(at) => (
            format!("{}  {at}", pick("终端代理", "Terminal proxy")),
            true,
        ),
        // On, but the address is gone: still let it be turned off.
        None if shown.proxy_on => (pick("终端代理", "Terminal proxy"), true),
        None => (
            pick(
                "终端代理（未检测到代理地址）",
                "Terminal proxy (no proxy address found)",
            ),
            false,
        ),
    };
    let gateway = (
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
        (pick("锁定密钥保管", "Lock the key vault"), true)
    } else {
        (pick("密钥保管已锁定", "Key vault is locked"), false)
    };
    [proxy, gateway, vault]
}

fn english() -> bool {
    crate::settings::load().locale == "en-US"
}

fn create_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let english = english();
    let pick = |zh: &'static str, en: &'static str| if english { en } else { zh };
    let shown = read_state();
    let [proxy, gateway, vault] = labels(&shown, english);

    let show = MenuItem::with_id(
        app,
        "show",
        pick("打开 Stacker", "Open Stacker"),
        true,
        None::<&str>,
    )?;
    let proxy = CheckMenuItem::with_id(
        app,
        "proxy_toggle",
        proxy.0,
        proxy.1,
        shown.proxy_on,
        None::<&str>,
    )?;
    let gateway = CheckMenuItem::with_id(
        app,
        "gateway_toggle",
        gateway.0,
        gateway.1,
        shown.gateway_on,
        None::<&str>,
    )?;
    let vault = MenuItem::with_id(app, "vault_lock", vault.0, vault.1, None::<&str>)?;
    let pages = PAGES
        .iter()
        .map(|(id, zh, en)| {
            MenuItem::with_id(
                app,
                format!("{GOTO}{id}"),
                if english { *en } else { *zh },
                true,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let page_refs = pages
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect::<Vec<_>>();
    let goto = Submenu::with_items(app, pick("前往", "Go to"), true, &page_refs)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        pick("退出 Stacker", "Quit Stacker"),
        true,
        None::<&str>,
    )?;
    Menu::with_items(
        app,
        &[
            &show,
            &PredefinedMenuItem::separator(app)?,
            &proxy,
            &gateway,
            &vault,
            &PredefinedMenuItem::separator(app)?,
            &goto,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

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

/// A switch that failed says why; the window may be hidden, so a dialog rather than a toast.
fn tell(app: &AppHandle, text: String) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(text)
        .kind(MessageDialogKind::Error)
        .title("Stacker")
        .show(|_| {});
}

fn toggle_proxy(app: &AppHandle) {
    let status = crate::proxy::status();
    let done = if status.enabled {
        crate::proxy::proxy_disable(false)
    } else {
        crate::proxy::proxy_enable(
            status.host.clone(),
            status.port,
            false,
            status.no_proxy_manual.clone(),
        )
    };
    if let Err(error) = done {
        tell(app, error);
    }
    let _ = app.emit("proxy-changed", ());
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
        "proxy_toggle" => toggle_proxy(app),
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
    let _ = refresh(app);
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
                let _ = refresh(tray.app_handle());
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown() -> Shown {
        Shown {
            proxy_on: false,
            proxy_at: Some("127.0.0.1:7890".into()),
            gateway_on: true,
            gateway_port: 8848,
            vault_open: true,
        }
    }

    #[test]
    fn the_switches_say_where_they_point() {
        let [proxy, gateway, vault] = labels(&shown(), false);
        assert_eq!(proxy, ("终端代理  127.0.0.1:7890".to_string(), true));
        assert_eq!(gateway, ("接口服务  端口 8848".to_string(), true));
        assert_eq!(vault, ("锁定密钥保管".to_string(), true));
        let [_, gateway, _] = labels(&shown(), true);
        assert_eq!(gateway.0, "API service  port 8848");
    }

    #[test]
    fn what_cannot_be_done_is_greyed_out() {
        let none = Shown {
            proxy_at: None,
            vault_open: false,
            ..shown()
        };
        let [proxy, _, vault] = labels(&none, false);
        assert!(!proxy.1, "no address to write");
        assert!(!vault.1, "already locked");
        let stale = Shown {
            proxy_on: true,
            ..none
        };
        assert!(
            labels(&stale, false)[0].1,
            "a proxy that is on can always be turned off"
        );
    }

    #[test]
    fn every_page_offered_exists() {
        let pages = include_str!("../../src/pageState.ts");
        for (id, _, _) in PAGES {
            assert!(pages.contains(&format!("\"{id}\"")), "{id}");
        }
    }
}
