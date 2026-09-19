mod agents;
mod backup;
mod binary;
mod bundle;
mod catalog;
mod checkup;
mod cleanup;
mod composer;
mod custom;
mod dpapi;
mod env;
mod fnm;
mod git;
mod gradle;
mod installer;
mod jdk;
mod logging;
mod profile;
mod proxy;
mod pyenv;
mod runner;
mod rustup;
mod sessions;
mod settings;
mod sources;
mod space_analysis;
mod storage;
mod update;
mod versions;
mod winadmin;
mod winenv;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Some((file, token)) = space_analysis::elevated::helper_arg() {
        std::process::exit(space_analysis::elevated::run_helper_from_file(
            &file, &token,
        ));
    }

    // 提权实例：写完 HKLM 系统级环境变量就退出，不起 GUI
    if let Some((file, token)) = winadmin::syssetenv_arg() {
        std::process::exit(winadmin::apply_from_file(&file, &token));
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            use tauri::Manager;

            log::info!(
                target: "stacker::startup",
                "secondary launch redirected to the running instance: cwd={} args={:?}",
                cwd,
                args
            );
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(space_analysis::SpaceTaskManager::default())
        .manage(space_analysis::CleanupTaskManager::default())
        .manage(space_analysis::SpaceMonitorManager::default())
        .setup(|app| {
            {
                use tauri::{Emitter, Manager};
                let handle = app.handle().clone();
                app.manage(agents::tasks::AgentTaskManager::new(
                    std::sync::Arc::new(agents::tasks::runner::ProductionRunner),
                    std::sync::Arc::new(move |task: &agents::tasks::AgentTask| {
                        let _ = handle.emit("agent-task", task);
                    }),
                ));
            }
            let app_settings = settings::load();
            let log_target = logging::target(settings::logs_dir())?;
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .clear_targets()
                    .filter(|metadata| {
                        let target = metadata.target();
                        target.starts_with("stacker")
                            || target.starts_with(tauri_plugin_log::WEBVIEW_TARGET)
                            || metadata.level() <= log::Level::Warn
                    })
                    .target(log_target)
                    .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                    .level(log::LevelFilter::Debug)
                    .build(),
            )?;
            log::set_max_level(settings::log_level_filter(&app_settings.log_level));
            logging::install_panic_hook();
            log::debug!(
                target: "stacker::startup",
                "Stacker {} started; os={} arch={} log_level={} log_file={} max_log_file_bytes={}",
                app.package_info().version,
                std::env::consts::OS,
                std::env::consts::ARCH,
                app_settings.log_level,
                logging::current_log_path(&settings::logs_dir()).display(),
                logging::MAX_LOG_FILE_BYTES
            );
            settings::init();
            settings::start_log_retention_worker();
            if let Err(error) = settings::settings_set_proxy_mode(app_settings.proxy_mode.clone()) {
                log::warn!(
                    target: "stacker::proxy",
                    "failed to reconcile persisted proxy settings during startup: {error}"
                );
            }
            binary::migrate_legacy_envs();
            build_tray(app.handle())?;
            Ok(())
        })
        // 主窗口按已保存策略退出、隐藏到托盘，或通知前端首次询问。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    match settings::close_behavior() {
                        settings::CloseBehavior::Ask => {
                            use tauri::Emitter;
                            api.prevent_close();
                            if let Err(error) = window.emit("app-close-choice-required", ()) {
                                log::error!(
                                    target: "stacker::window",
                                    "failed to request close behavior choice: {error}"
                                );
                            }
                        }
                        settings::CloseBehavior::Tray => {
                            api.prevent_close();
                            if let Err(error) = window.hide() {
                                log::error!(
                                    target: "stacker::window",
                                    "failed to hide main window to tray: {error}"
                                );
                            }
                        }
                        settings::CloseBehavior::Exit => {
                            use tauri::Manager;
                            if running_agent_tasks(window.app_handle()) > 0 {
                                api.prevent_close();
                                confirm_exit(window.app_handle().clone());
                            }
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            sessions::commands::sessions_list,
            sessions::commands::sessions_projects,
            sessions::commands::sessions_read,
            sessions::commands::sessions_roots,
            sessions::commands::sessions_set_roots,
            sessions::commands::sessions_favorite,
            sessions::commands::sessions_open,
            sessions::commands::sessions_delete_preview,
            sessions::commands::sessions_delete_execute,
            sessions::commands::sessions_job,
            sessions::commands::sessions_cancel,
            sessions::commands::footprint_scan,
            sessions::commands::footprint_preview,
            sessions::commands::footprint_execute,
            sessions::commands::footprint_job,
            sessions::commands::runner_options,
            sessions::commands::summary_settings,
            sessions::commands::summary_save_settings,
            sessions::commands::summary_preview,
            sessions::commands::summary_start,
            sessions::commands::summary_job,
            sessions::commands::summary_cancel,
            sessions::commands::handoff_preview,
            sessions::commands::handoff_start,
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
            storage::storage_locations,
            storage::storage_apply,
            storage::storage_reset,
            composer::composer_status,
            composer::composer_install,
            composer::composer_clear,
            sources::list_backups,
            sources::restore_backup,
            sources::backup_detail,
            sources::delete_backup,
            sources::clear_backups,
            proxy::proxy_status,
            proxy::proxy_enable,
            proxy::proxy_disable,
            binary::binary_mirror_status,
            binary::binary_mirror_apply,
            binary::binary_mirror_clear,
            catalog::source_catalog_status,
            catalog::source_catalog_export,
            catalog::source_catalog_import,
            env::env_state,
            env::env_register_install,
            env::env_remove_managed,
            env::env_scan,
            env::env_set_default,
            env::env_set_default_system,
            env::env_system_info,
            env::env_java_effective,
            env::env_cancel,
            env::list_drives,
            git::git_status,
            git::git_check_update,
            git::git_install,
            git::git_github_accounts,
            git::git_account_save_token,
            git::git_account_save_custom_token,
            git::git_account_remove_token,
            git::git_account_profiles,
            git::git_account_save_identity,
            git::git_account_set_global,
            git::git_account_ai_context,
            git::git_account_open_shell,
            git::git_init_repository,
            git::git_auto_migrate_repository,
            git::git_apply_proxy,
            git::git_clear_proxy,
            cleanup::cleanup_scan,
            cleanup::cleanup_delete,
            cleanup::cleanup_delete_safe,
            cleanup::cleanup_aged_stats,
            cleanup::cleanup_delete_aged,
            space_analysis::space_fixed_volumes,
            space_analysis::space_scan_start,
            space_analysis::space_scan_start_elevated,
            space_analysis::space_scan_status,
            space_analysis::space_scan_cancel,
            space_analysis::space_scan_quick_result,
            space_analysis::space_scan_summary,
            space_analysis::space_scan_children,
            space_analysis::space_scan_large_files,
            space_analysis::space_cleanup_candidates,
            space_analysis::space_cleanup_plan,
            space_analysis::space_cleanup_start,
            space_analysis::space_cleanup_status,
            space_analysis::space_cleanup_cancel,
            space_analysis::space_cleanup_result,
            space_analysis::space_snapshot_save,
            space_analysis::space_snapshot_list,
            space_analysis::space_snapshot_compare,
            space_analysis::space_snapshot_delete,
            space_analysis::space_snapshot_clear,
            space_analysis::space_open_directory,
            space_analysis::space_monitor_start,
            space_analysis::space_monitor_status,
            space_analysis::space_monitor_stop,
            space_analysis::space_monitor_dispose,
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
            fnm::fnm_speedtest_sources,
            gradle::gradle_wrapper_state,
            gradle::gradle_wrapper_scan,
            gradle::gradle_wrapper_apply,
            checkup::checkup_extra,
            checkup::checkup_page,
            checkup::coding_ecosystem_check,
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
            rustup::rust_versions,
            rustup::rustup_set_default,
            rustup::rustup_install,
            rustup::rustup_uninstall,
            rustup::rustup_update,
            rustup::rustup_install_self,
            rustup::rustup_self_update,
            rustup::rustup_components,
            rustup::rustup_targets,
            rustup::rustup_component_set,
            rustup::rustup_target_set,
            installer::installer_download,
            installer::app_dir,
            installer::managed_runtime_dir,
            installer::open_shell,
            installer::open_ecosystem_verify_shell,
            installer::ecosystem_activation_commands,
            installer::shells_available,
            installer::op_cancel,
            jdk::jdk_resolve,
            jdk::dragonwell_resolve,
            jdk::zulu_resolve,
            versions::maven_versions,
            versions::gradle_versions,
            versions::go_versions,
            versions::php_versions,
            versions::php_download_url,
            profile::profile_save,
            profile::profile_list,
            profile::profile_apply,
            profile::profile_delete,
            custom::custom_list,
            custom::custom_save,
            custom::custom_delete,
            bundle::bundle_export,
            bundle::bundle_import,
            update::mirrors_check_update,
            update::mirrors_update,
            update::app_check_update,
            update::app_download_update,
            update::app_open_url,
            settings::settings_get,
            settings::settings_set_tray,
            settings::settings_set_close_behavior,
            settings::settings_set_theme,
            settings::settings_set_locale,
            settings::settings_set_log_level,
            settings::settings_set_log_retention_days,
            settings::settings_set_space_analysis,
            settings::settings_open_logs_dir,
            settings::settings_open_log_window,
            settings::settings_read_log,
            settings::settings_clear_old_logs,
            settings::settings_set_proxy_addr,
            settings::settings_set_proxy_mode,
            settings::settings_sync_system_proxy,
            settings::settings_set_proxy_manual,
            settings::os_info,
            agents::commands::vibe_catalog,
            agents::commands::vibe_tools,
            agents::commands::vibe_tools_refresh,
            agents::commands::vibe_tool,
            agents::commands::vibe_environment_prompt,
            agents::commands::vibe_tool_action,
            agents::commands::vibe_open_desktop,
            agents::commands::agent_task_start,
            agents::commands::agent_task_cancel,
            agents::commands::agent_task_retry,
            agents::commands::agent_tasks,
            agents::commands::agent_task_log,
            agents::commands::agent_update_plan,
            agents::commands::agent_update_all,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            use tauri::Manager;

            app_handle
                .state::<space_analysis::SpaceTaskManager>()
                .cancel_all_and_wait(std::time::Duration::from_secs(3));
            app_handle
                .state::<space_analysis::CleanupTaskManager>()
                .cancel_all_and_wait(std::time::Duration::from_secs(3));
        }
    });
}

// 系统托盘：左键单击显示窗口；右键菜单＝显示 / 开关终端代理 / 退出。
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::Manager;

    let menu = create_tray_menu(app)?;

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
                    crate::proxy::enable(&st.host, st.port, false, st.no_proxy_manual)
                };
            }
            "quit" => {
                if running_agent_tasks(app) == 0 {
                    app.exit(0);
                } else {
                    confirm_exit(app.clone());
                }
            }
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

fn create_tray_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let english = crate::settings::load().locale == "en-US";
    let show = MenuItem::with_id(
        app,
        "show",
        if english {
            "Show Stacker"
        } else {
            "显示 Stacker"
        },
        true,
        None::<&str>,
    )?;
    let proxy = MenuItem::with_id(
        app,
        "proxy_toggle",
        if english {
            "Toggle Terminal Proxy"
        } else {
            "开关终端代理"
        },
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        if english {
            "Quit Stacker"
        } else {
            "退出 Stacker"
        },
        true,
        None::<&str>,
    )?;
    Menu::with_items(app, &[&show, &proxy, &sep, &quit])
}

pub(crate) fn refresh_tray_menu(app: &tauri::AppHandle) -> Result<(), String> {
    let menu = create_tray_menu(app).map_err(|error| error.to_string())?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn running_agent_tasks(app: &tauri::AppHandle) -> usize {
    use tauri::Manager;
    app.try_state::<agents::tasks::AgentTaskManager>()
        .map_or(0, |manager| manager.running_count())
}

/// Asks before quitting while agent installs are still running, then cancels them.
fn confirm_exit(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        use tauri::Manager;
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        let running = running_agent_tasks(&app);
        let confirmed = app
            .dialog()
            .message(format!(
                "还有 {running} 个智能体任务正在执行，退出会中断这些任务。确定退出吗？"
            ))
            .buttons(MessageDialogButtons::OkCancelCustom(
                "退出".into(),
                "取消".into(),
            ))
            .blocking_show();
        if confirmed {
            app.state::<agents::tasks::AgentTaskManager>().cancel_all();
            app.exit(0);
        }
    });
}
