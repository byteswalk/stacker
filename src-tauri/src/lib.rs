mod backup;
mod binary;
mod bundle;
mod catalog;
mod checkup;
mod cleanup;
mod custom;
mod dpapi;
mod env;
mod fnm;
mod installer;
mod jdk;
mod profile;
mod proxy;
mod pyenv;
mod rustup;
mod settings;
mod sources;
mod update;
mod versions;
mod winadmin;
mod winenv;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 提权实例：写完 HKLM 系统级环境变量就退出，不起 GUI
    if let Some((file, token)) = winadmin::syssetenv_arg() {
        std::process::exit(winadmin::apply_from_file(&file, &token));
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            settings::init();
            build_tray(app.handle())?;
            Ok(())
        })
        // 「最小化到托盘」开启时，关闭窗口改为隐藏到托盘而非退出
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && settings::minimize_to_tray() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            sources::list_sources,
            sources::apply_source,
            sources::apply_source_scoped,
            sources::apply_source_file,
            sources::clear_source_file,
            sources::source_proxy_state,
            sources::source_file_state,
            sources::pip_config_state,
            sources::pip_apply_source,
            sources::pip_clear_source,
            sources::speedtest_hosts,
            sources::list_backups,
            sources::restore_backup,
            sources::backup_detail,
            sources::delete_backup,
            sources::clear_backups,
            proxy::proxy_status,
            proxy::proxy_enable,
            proxy::proxy_disable,
            proxy::proxy_gen_scripts,
            binary::binary_mirror_status,
            binary::binary_mirror_apply,
            binary::binary_mirror_clear,
            catalog::source_catalog_status,
            catalog::source_catalog_export,
            catalog::source_catalog_import,
            env::env_state,
            env::env_scan,
            env::env_set_default,
            env::env_set_default_system,
            env::env_system_info,
            env::env_java_effective,
            env::env_cancel,
            env::list_drives,
            cleanup::cleanup_scan,
            cleanup::cleanup_delete,
            cleanup::cleanup_delete_safe,
            cleanup::cleanup_aged_stats,
            cleanup::cleanup_delete_aged,
            fnm::fnm_status,
            fnm::fnm_root_dir,
            fnm::fnm_set_default,
            fnm::fnm_install_version,
            fnm::fnm_uninstall_version,
            fnm::fnm_ls_remote,
            fnm::fnm_write_integration,
            fnm::fnm_install_self,
            fnm::fnm_check_update,
            fnm::fnm_self_update,
            fnm::fnm_migrate_from_nvm,
            fnm::fnm_speedtest_sources,
            checkup::checkup_extra,
            pyenv::pyenv_status,
            pyenv::pyenv_root_dir,
            pyenv::pyenv_set_global,
            pyenv::pyenv_install_version,
            pyenv::pyenv_uninstall_version,
            pyenv::pyenv_cleanup_stale_registrations,
            pyenv::pyenv_install_list,
            pyenv::pyenv_install_self,
            pyenv::pyenv_check_update,
            pyenv::pyenv_self_update,
            pyenv::pyenv_write_integration,
            pyenv::pyenv_speedtest_sources,
            rustup::rustup_status,
            rustup::rustup_set_default,
            rustup::rustup_install,
            rustup::rustup_uninstall,
            rustup::rustup_install_self,
            rustup::rustup_self_update,
            installer::installer_download,
            installer::app_dir,
            installer::open_shell,
            installer::shells_available,
            installer::op_cancel,
            jdk::jdk_resolve,
            jdk::dragonwell_resolve,
            jdk::zulu_resolve,
            versions::maven_versions,
            versions::gradle_versions,
            profile::profile_save,
            profile::profile_list,
            profile::profile_apply,
            profile::profile_delete,
            custom::custom_list,
            custom::custom_save,
            custom::custom_delete,
            bundle::bundle_export,
            bundle::bundle_import,
            update::mirrors_status,
            update::mirrors_check_update,
            update::mirrors_update,
            update::mirrors_seed,
            update::app_check_update,
            update::app_open_url,
            settings::settings_get,
            settings::settings_set_tray,
            settings::settings_set_theme,
            settings::settings_set_proxy_addr,
            settings::os_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// 系统托盘：左键单击显示窗口；右键菜单＝显示 / 开关终端代理 / 退出。
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::Manager;

    let show = MenuItem::with_id(app, "show", "显示 Stacker", true, None::<&str>)?;
    let proxy = MenuItem::with_id(app, "proxy_toggle", "开关终端代理", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Stacker", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &proxy, &sep, &quit])?;

    let show_main = |app: &tauri::AppHandle| {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    };

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Stacker")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "proxy_toggle" => {
                let st = crate::proxy::status();
                let _ = if st.enabled {
                    crate::proxy::disable(false)
                } else {
                    crate::proxy::enable(&st.host, st.port, false, vec![])
                };
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
