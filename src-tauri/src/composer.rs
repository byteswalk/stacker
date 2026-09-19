use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tauri::Emitter;

use crate::{backup, installer, winenv};

const COMPOSER_PHAR_URL: &str = "https://getcomposer.org/composer-stable.phar";

#[derive(Serialize)]
pub struct ComposerStatus {
    pub installed: bool,
    pub managed: bool,
    pub version: String,
    pub path: String,
    pub home: String,
}

fn composer_home() -> PathBuf {
    winenv::get_raw_in(winenv::Hive::User, "COMPOSER_HOME")
        .or_else(|| winenv::get_raw_in(winenv::Hive::System, "COMPOSER_HOME"))
        .or_else(|| std::env::var("COMPOSER_HOME").ok())
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::home_dir()
                        .unwrap_or_default()
                        .join("AppData")
                        .join("Roaming")
                })
                .join("Composer")
        })
}

fn first_command_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("where.exe").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn php_executable() -> Option<PathBuf> {
    first_command_path("php").or_else(|| {
        crate::env::env_state_blocking()
            .into_iter()
            .find(|group| group.kind == "php")
            .and_then(|group| {
                let versions = group.versions;
                versions
                    .iter()
                    .find(|version| version.current)
                    .or_else(|| versions.first())
                    .map(|version| PathBuf::from(&version.path).join("php.exe"))
            })
            .filter(|path| path.exists())
    })
}

fn managed_entry() -> PathBuf {
    composer_home().join("bin").join("composer.bat")
}

fn composer_entry() -> Option<PathBuf> {
    let managed = managed_entry();
    if managed.exists() {
        Some(managed)
    } else {
        first_command_path("composer")
    }
}

fn composer_version(entry: &Path) -> String {
    Command::new("cmd.exe")
        .args(["/D", "/S", "/C"])
        .arg(format!("\"{}\" --version", entry.display()))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn composer_status() -> ComposerStatus {
    tauri::async_runtime::spawn_blocking(composer_status_blocking)
        .await
        .expect("blocking command worker panicked")
}

pub(crate) fn composer_status_blocking() -> ComposerStatus {
    let home = composer_home();
    let managed = managed_entry();
    let entry = composer_entry();
    let version = entry.as_deref().map(composer_version).unwrap_or_default();
    ComposerStatus {
        installed: entry.is_some() && !version.is_empty(),
        managed: entry.as_ref().is_some_and(|path| path == &managed),
        version,
        path: entry
            .as_deref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        home: home.to_string_lossy().into_owned(),
    }
}

#[tauri::command]
pub async fn composer_install(window: tauri::Window) -> Result<ComposerStatus, String> {
    tauri::async_runtime::spawn_blocking(move || composer_install_impl(window))
        .await
        .map_err(|error| error.to_string())?
}

fn composer_install_impl(window: tauri::Window) -> Result<ComposerStatus, String> {
    installer::op_reset();
    let php = php_executable().ok_or_else(|| {
        "未检测到可用的 PHP。请先安装 PHP 运行时并设为默认版本，再安装 Composer。".to_string()
    })?;
    let home = composer_home();
    let bin = home.join("bin");
    fs::create_dir_all(&bin).map_err(|error| format!("无法创建 Composer 目录：{error}"))?;

    let temp = std::env::temp_dir().join(format!(
        "stacker-composer-{}-{}.phar",
        std::process::id(),
        chrono::Local::now().timestamp_millis()
    ));
    let target = bin.join("composer.phar");
    let entry = bin.join("composer.bat");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .build();

    installer::download_file_candidates_with_agent(
        &agent,
        &[COMPOSER_PHAR_URL.to_string()],
        &temp,
        100_000,
        |message| {
            let _ = window.emit("install-progress", message);
        },
    )?;

    let _ = window.emit("install-progress", "正在验证 Composer 安装包…".to_string());
    let verify = Command::new(&php)
        .arg(&temp)
        .arg("--version")
        .output()
        .map_err(|error| format!("无法启动 PHP 验证 Composer：{error}"))?;
    if !verify.status.success() {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "Composer 安装包验证失败：{}",
            String::from_utf8_lossy(&verify.stderr).trim()
        ));
    }

    if target.exists() {
        backup::backup_file(&target);
        fs::remove_file(&target).map_err(|error| format!("无法替换 Composer：{error}"))?;
    }
    fs::rename(&temp, &target)
        .or_else(|_| {
            fs::copy(&temp, &target)?;
            fs::remove_file(&temp)
        })
        .map_err(|error| format!("无法保存 Composer：{error}"))?;

    // Follow the PHP selected in PATH so switching the default runtime also switches Composer.
    // Keep the installer-time executable as a fallback for terminals that have not refreshed yet.
    let launcher = format!(
        "@echo off\r\nwhere php.exe >nul 2>nul\r\nif not errorlevel 1 (\r\n  php \"%~dp0composer.phar\" %*\r\n  exit /b %errorlevel%\r\n)\r\n\"{}\" \"%~dp0composer.phar\" %*\r\n",
        php.to_string_lossy()
    );
    fs::write(&entry, launcher).map_err(|error| format!("无法创建 Composer 命令入口：{error}"))?;

    backup::backup_env(
        winenv::Hive::User,
        "composer-install",
        &["COMPOSER_HOME", "Path"],
    );
    winenv::set_user("COMPOSER_HOME", &home.to_string_lossy())?;
    winenv::prepend_path_in(winenv::Hive::User, &bin.to_string_lossy())?;

    let _ = window.emit("install-progress", "正在验证 Composer 命令…".to_string());
    let version = composer_version(&entry);
    if version.is_empty() {
        return Err("Composer 文件已写入，但命令验证未通过。请检查 PHP 运行时后重试。".into());
    }
    log::info!(
        target: "stacker::composer",
        "Composer installed path={} php={} version={}",
        entry.display(),
        php.display(),
        version
    );
    let _ = window.emit("install-progress", "__done__".to_string());
    Ok(ComposerStatus {
        installed: true,
        managed: true,
        version,
        path: entry.to_string_lossy().into_owned(),
        home: home.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn composer_clear() -> Result<ComposerStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = composer_home();
        let bin = home.join("bin");
        let entry = bin.join("composer.bat");
        if !entry.exists() {
            return Err("当前 Composer 不是由 Stacker 安装，不能在此卸载。".into());
        }

        backup::backup_env(
            winenv::Hive::User,
            "composer-remove",
            &["COMPOSER_HOME", "Path"],
        );
        winenv::remove_path_in(winenv::Hive::User, &bin.to_string_lossy())?;
        for path in [entry, bin.join("composer.phar")] {
            if path.exists() {
                backup::backup_file(&path);
                fs::remove_file(&path)
                    .map_err(|error| format!("无法删除 {}：{error}", path.display()))?;
            }
        }
        Ok(composer_status_blocking())
    })
    .await
    .map_err(|error| error.to_string())?
}
