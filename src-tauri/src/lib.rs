mod agents;
mod ai_config;
mod ai_features;
mod backup;
mod binary;
mod bundle;
mod catalog;
mod checkup;
mod cleanup;
mod composer;
mod console_audit;
mod custom;
mod distill;
mod dpapi;
mod env;
mod fnm;
mod gateway;
mod git;
mod gradle;
mod installer;
mod jdk;
mod logging;
mod profile;
mod project_junk;
mod proxy;
mod proxy_ledger;
mod proxy_system;
mod proxy_targets;
mod pyenv;
pub(crate) mod python_env;
pub(crate) mod python_venv;
mod runner;
mod rustup;
mod sessions;
mod settings;
mod sources;
mod space_analysis;
mod storage;
mod tool_relocation;
mod tray;
pub mod update;
mod vault;
mod versions;
mod webchat;
mod winadmin;
mod winenv;
mod winget_settings;
/// The smallest the main window may be, in logical pixels: every page is drawn for it.
const MIN_WINDOW: (f64, f64) = (1280.0, 720.0);

/// The floor Windows is given: the client area Stacker needs, plus the frame it measures
/// along with it. Without the frame the client area ends up a dozen pixels short.
fn set_minimum_size(window: &tauri::WebviewWindow) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let (Ok(inner), Ok(outer)) = (window.inner_size(), window.outer_size()) else {
        return;
    };
    let frame_width = outer.width.saturating_sub(inner.width);
    let frame_height = outer.height.saturating_sub(inner.height);
    let _ = window.set_min_size(Some(tauri::PhysicalSize::new(
        (MIN_WINDOW.0 * scale).ceil() as u32 + frame_width,
        (MIN_WINDOW.1 * scale).ceil() as u32 + frame_height,
    )));
}

/// Whatever the frame does, the size that matters is put back here. The last correction is
/// remembered so a window that cannot reach the floor is not pushed at forever.
fn hold_minimum_size(window: &tauri::Window, size: tauri::PhysicalSize<u32>) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<((u32, u32), Instant)>> = Mutex::new(None);
    if window.is_maximized().unwrap_or(false)
        || window.is_minimized().unwrap_or(false)
        || window.is_fullscreen().unwrap_or(false)
    {
        return;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let min_width = (MIN_WINDOW.0 * scale).ceil() as u32;
    let min_height = (MIN_WINDOW.1 * scale).ceil() as u32;
    if size.width >= min_width && size.height >= min_height {
        return;
    }
    let target = (size.width.max(min_width), size.height.max(min_height));
    if let Ok(mut last) = LAST.lock() {
        if last
            .is_some_and(|(size, at)| size == target && at.elapsed() < Duration::from_millis(400))
        {
            return;
        }
        *last = Some((target, Instant::now()));
    }
    let _ = window.set_size(tauri::PhysicalSize::new(target.0, target.1));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// A program Stacker starts that cannot load (a file another process holds, for one: a
/// security suite inspecting a new program's children does that) would put up a system
/// "Application Error" box of its own. Child processes inherit this error mode, so the
/// failure comes back to Stacker as an exit code, which it reports, instead of a box on the
/// user's screen.
#[cfg(windows)]
fn quiet_child_failures() {
    use winapi::um::errhandlingapi::SetErrorMode;
    use winapi::um::winbase::{SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX};
    // SAFETY: changes only this process's error mode, which its children start from.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX);
    }
}

pub fn run() {
    #[cfg(windows)]
    quiet_child_failures();
    // 浏览器插件的本地消息桥：Chrome / Edge 以插件来源为第一个参数启动本程序。
    // 只走标准输入输出；在单实例插件和界面之前退出，因此不会打开任何窗口。
    let args: Vec<String> = std::env::args().collect();
    if let Some(origin) = webchat::bridge::origin_arg(&args) {
        std::process::exit(webchat::bridge::run_stdio(&origin));
    }

    #[cfg(debug_assertions)]
    if let Some(code) = agents::devcli::run(&args) {
        std::process::exit(code);
    }

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
            vault::guard::set_auto_lock_minutes(app_settings.vault_auto_lock_minutes);
            vault::guard::start(app.handle().clone());
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
            std::thread::spawn(|| {
                if let Ok(exe) = std::env::current_exe() {
                    webchat::host::repoint_if_gone(&webchat::root(), &exe, webchat::extension_id());
                }
            });
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
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    set_minimum_size(&window);
                }
            }
            settings::init();
            settings::start_log_retention_worker();
            gateway::restore();
            binary::migrate_legacy_envs();
            tray::build(app.handle())?;
            Ok(())
        })
        // 主窗口按已保存策略退出、隐藏到托盘，或通知前端首次询问。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Resized(size) = event {
                if window.label() == "main" {
                    hold_minimum_size(window, *size);
                }
            }
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
            sessions::commands::sessions_set_export_dir,
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
            sessions::commands::summary_preview,
            sessions::commands::summary_start,
            sessions::commands::summary_job,
            sessions::commands::summary_cancel,
            sessions::commands::handoff_preview,
            sessions::commands::handoff_start,
            sessions::commands::migration_status,
            sessions::commands::migration_check,
            sessions::commands::migration_start,
            sessions::commands::migration_delete_backup,
            sessions::commands::migration_move_back,
            sessions::commands::migration_job,
            sessions::commands::sessions_transfer_projects,
            sessions::commands::sessions_transfer_size,
            sessions::commands::sessions_transfer_export,
            sessions::commands::sessions_transfer_preview,
            sessions::commands::sessions_transfer_import,
            sessions::commands::migration_cancel,
            webchat::commands::webchat_status,
            webchat::commands::webchat_connect,
            webchat::commands::webchat_disconnect,
            webchat::commands::webchat_open,
            webchat::commands::webchat_list,
            webchat::commands::webchat_read,
            webchat::commands::webchat_summarize,
            webchat::commands::webchat_summary_cancel,
            distill::commands::distill_candidates,
            distill::commands::distill_preview,
            distill::commands::distill_start,
            distill::commands::distill_job,
            distill::commands::distill_cancel,
            distill::commands::distill_list,
            distill::commands::distill_save,
            distill::commands::distill_state,
            distill::commands::distill_delete,
            distill::commands::distill_export,
            distill::commands::distill_open,
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
            cleanup::cleanup_delete_category,
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
            space_analysis::space_duplicates,
            space_analysis::space_cleanup_candidates,
            space_analysis::space_cleanup_plan,
            space_analysis::space_cleanup_start,
            space_analysis::space_cleanup_status,
            space_analysis::space_cleanup_cancel,
            space_analysis::space_cleanup_result,
            space_analysis::file_removal::space_recycle_files,
            space_analysis::file_removal::space_recycle_folders,
            space_analysis::space_verify_duplicates,
            space_analysis::space_file_times,
            space_analysis::file_removal::space_protected_paths,
            space_analysis::space_cleanup_history,
            space_analysis::space_cleanup_history_clear,
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
            python_env::python_env_report,
            python_env::python_remove_runtimes,
            python_venv::python_venv_inspect,
            python_venv::python_venv_create,
            python_venv::python_venv_remove,
            python_venv::python_venv_rebuild,
            python_venv::python_env_scan,
            python_venv::python_env_scan_cancel,
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
            tool_relocation::tool_relocation_plan,
            tool_relocation::tool_relocation_apply,
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
            app_close_once,
            settings::settings_set_theme,
            settings::settings_get_theme,
            settings::settings_set_locale,
            settings::settings_set_log_level,
            settings::settings_set_log_retention_days,
            settings::settings_set_space_analysis,
            settings::settings_open_logs_dir,
            settings::settings_open_log_window,
            settings::settings_read_log,
            settings::settings_clear_old_logs,
            settings::settings_set_proxy_manual,
            proxy_ledger::proxy_overview,
            gateway::gateway_status,
            gateway::gateway_set,
            gateway::gateway_set_lan,
            gateway::gateway_allow_firewall,
            ai_config::ai_config_get,
            ai_config::ai_config_set,
            ai_config::ai_config_use_local,
            ai_config::ai_config_test,
            ai_config::ai_explain_path,
            ai_features::ai_ask,
            ai_features::ai_session_filter,
            ai_features::ai_vault_filter,
            ai_features::ai_list_filter,
            ai_features::ai_update_notes,
            ai_features::proxy_probe,
            project_junk::project_junk_scan,
            project_junk::project_junk_clean,
            gateway::gateway_new_token,
            gateway::agents::gateway_agents,
            gateway::agents::gateway_log,
            gateway::agents::gateway_log_remove,
            gateway::agents::gateway_log_clear,
            gateway::agents::gateway_set_log,
            gateway::agents::gateway_set_agent,
            gateway::agents::gateway_test,
            proxy_ledger::proxy_sync_report,
            proxy_ledger::proxy_follow_system,
            proxy_ledger::proxy_targets_list,
            proxy_ledger::proxy_target_save,
            proxy_ledger::proxy_target_remove,
            proxy_ledger::proxy_service_set,
            proxy_ledger::proxy_location_write,
            proxy_ledger::proxy_location_clear,
            proxy_ledger::proxy_clear_stale,
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
            agents::commands::agent_tasks_clear,
            agents::commands::agent_task_dismiss,
            agents::commands::agent_update_plan,
            agents::commands::agent_update_all,
            settings::settings_set_vault,
            vault::commands::vault_status,
            vault::commands::vault_create_begin,
            vault::commands::vault_copy_recovery,
            vault::commands::vault_confirm_recovery,
            vault::commands::vault_cancel_pending,
            vault::commands::vault_unlock,
            vault::commands::vault_unlock_recovery,
            vault::commands::vault_recovery_set_password,
            vault::commands::vault_change_password,
            vault::commands::vault_reset_recovery,
            vault::commands::vault_lock,
            vault::commands::vault_touch,
            vault::commands::vault_list,
            vault::commands::vault_save,
            vault::commands::vault_reveal,
            vault::commands::vault_copy,
            vault::commands::vault_history,
            vault::commands::vault_history_reveal,
            vault::commands::vault_history_copy,
            vault::commands::vault_delete,
            vault::commands::vault_restore,
            vault::commands::vault_purge,
            vault::commands::vault_favorite,
            vault::commands::vault_ssh_export,
            vault::commands::vault_export,
            vault::commands::vault_import_preview,
            vault::commands::vault_import_apply,
            vault::commands::vault_restore_backup,
            vault::commands::vault_reset,
            vault::commands::vault_ssh_generate,
            vault::commands::vault_ssh_install_local,
            vault::commands::vault_ssh_local,
            vault::commands::vault_ssh_public_of,
            vault::commands::vault_env_holders,
            vault::commands::vault_credential_targets,
            vault::commands::vault_set_windows,
            vault::commands::vault_merge,
            vault::commands::vault_merge_preview,
            vault::commands::vault_browser_exportable,
            vault::commands::vault_export_browser,
            vault::commands::vault_fill_titles,
            winget_settings::winget_downloader,
            winget_settings::winget_downloader_set,
            vault::commands::vault_import_browser,
            vault::commands::vault_ssh_set_passphrase,
            vault::commands::vault_retired,
            vault::commands::vault_clipboard_text,
            vault::commands::vault_discover_start,
            vault::commands::vault_discover_status,
            vault::commands::vault_discover_cancel,
            vault::commands::vault_discover_clear,
            vault::commands::vault_discover_import,
            vault::commands::vault_discover_ignore,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            use tauri::Manager;

            vault::guard::lock_everything();

            app_handle
                .state::<space_analysis::SpaceTaskManager>()
                .cancel_all_and_wait(std::time::Duration::from_secs(3));
            app_handle
                .state::<space_analysis::CleanupTaskManager>()
                .cancel_all_and_wait(std::time::Duration::from_secs(3));
        }
    });
}

/// Closes the main window this once as chosen ("tray" or "exit"), without saving the choice.
#[tauri::command]
fn app_close_once(app: tauri::AppHandle, behavior: String) -> Result<(), String> {
    use tauri::Manager;
    match behavior.as_str() {
        "tray" => app
            .get_webview_window("main")
            .ok_or("E_NO_WINDOW")?
            .hide()
            .map_err(|error| error.to_string()),
        "exit" => {
            quit(&app);
            Ok(())
        }
        _ => Err("E_CLOSE_BEHAVIOR".into()),
    }
}

/// Quits, asking first while agent installs are still running.
pub(crate) fn quit(app: &tauri::AppHandle) {
    if running_agent_tasks(app) == 0 {
        app.exit(0);
    } else {
        confirm_exit(app.clone());
    }
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
