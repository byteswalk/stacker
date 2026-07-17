//! 应用自身的小配置（最小化到托盘、外观主题）。存 %APPDATA%\stacker\settings.json。
//! 主题主要由前端用 localStorage + data-theme 控制，这里也存一份便于换机/导出时一致。

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

// 缓存「最小化到托盘」开关，供窗口关闭事件同步读取（不走异步命令）。
static MIN_TO_TRAY: AtomicBool = AtomicBool::new(false);
static LOG_WINDOW_OPENING: AtomicBool = AtomicBool::new(false);

fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_default()
        .join("stacker")
        .join("settings.json")
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct AppSettings {
    #[serde(default)]
    pub minimize_to_tray: bool,
    #[serde(default = "default_theme")]
    pub theme: String, // "dark" | "light" | "system"
    #[serde(default = "default_proxy_host")]
    pub proxy_host: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(default)]
    pub no_proxy_manual: Vec<String>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u16,
    #[serde(default = "default_locale")]
    pub locale: String,
}
fn default_theme() -> String {
    "dark".into()
}
fn default_log_level() -> String {
    "error".into()
}
fn default_log_retention_days() -> u16 {
    7
}
fn default_locale() -> String {
    "zh-CN".into()
}
fn parse_proxy_addr(raw: &str) -> Option<(String, u16)> {
    let rest = raw
        .trim()
        .strip_prefix("http://")
        .or_else(|| raw.trim().strip_prefix("https://"))
        .or_else(|| raw.trim().strip_prefix("socks5://"))
        .unwrap_or(raw.trim());
    let rest = rest.trim_end_matches('/');
    let rest = rest.rsplit('@').next().unwrap_or(rest);
    let (host, port) = rest.rsplit_once(':')?;
    let port = port.parse::<u16>().ok().filter(|p| *p > 0)?;
    Some((host.to_string(), port))
}
#[cfg(windows)]
fn windows_system_proxy_addr() -> Option<(String, u16)> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled = key.get_value::<u32, _>("ProxyEnable").unwrap_or(0);
    if enabled == 0 {
        return None;
    }
    let raw = key.get_value::<String, _>("ProxyServer").ok()?;
    for part in raw.split(';') {
        let value = part
            .split_once('=')
            .map(|(_, value)| value)
            .unwrap_or(part)
            .trim();
        if let Some(addr) = parse_proxy_addr(value) {
            return Some(addr);
        }
    }
    None
}
fn detected_proxy_addr() -> (String, u16) {
    #[cfg(windows)]
    {
        for (hive, name) in [
            (crate::winenv::Hive::User, "HTTP_PROXY"),
            (crate::winenv::Hive::User, "HTTPS_PROXY"),
            (crate::winenv::Hive::User, "ALL_PROXY"),
            (crate::winenv::Hive::System, "HTTP_PROXY"),
            (crate::winenv::Hive::System, "HTTPS_PROXY"),
            (crate::winenv::Hive::System, "ALL_PROXY"),
        ] {
            if let Some(raw) = crate::winenv::get_raw_in(hive, name) {
                if let Some(addr) = parse_proxy_addr(&raw) {
                    return addr;
                }
            }
        }
        if let Some(addr) = windows_system_proxy_addr() {
            return addr;
        }
    }
    (
        "127.0.0.1".into(),
        crate::proxy::detect_clash_port().unwrap_or(7890),
    )
}
fn default_proxy_host() -> String {
    detected_proxy_addr().0
}
fn default_proxy_port() -> u16 {
    detected_proxy_addr().1
}
fn normalize(mut s: AppSettings) -> AppSettings {
    if s.theme.trim().is_empty() {
        s.theme = default_theme();
    }
    if s.proxy_host.trim().is_empty() || s.proxy_port == 0 {
        let (host, port) = detected_proxy_addr();
        if s.proxy_host.trim().is_empty() {
            s.proxy_host = host;
        }
        if s.proxy_port == 0 {
            s.proxy_port = port;
        }
    }
    s.log_level = normalize_log_level(&s.log_level).to_string();
    if s.log_retention_days == 0 {
        s.log_retention_days = default_log_retention_days();
    }
    s.log_retention_days = s.log_retention_days.min(365);
    s.locale = match s.locale.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" => "en-US".into(),
        _ => "zh-CN".into(),
    };
    s
}

fn normalize_log_level(level: &str) -> &'static str {
    match level.trim().to_ascii_lowercase().as_str() {
        "debug" => "debug",
        "info" => "info",
        "warn" | "warning" => "warn",
        _ => "error",
    }
}

pub fn log_level_filter(level: &str) -> log::LevelFilter {
    match normalize_log_level(level) {
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        _ => log::LevelFilter::Error,
    }
}

pub fn logs_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Stacker")
        .join("logs")
}

pub fn load() -> AppSettings {
    let s = std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| AppSettings {
            minimize_to_tray: false,
            theme: default_theme(),
            proxy_host: default_proxy_host(),
            proxy_port: default_proxy_port(),
            no_proxy_manual: Vec::new(),
            log_level: default_log_level(),
            log_retention_days: default_log_retention_days(),
            locale: default_locale(),
        });
    normalize(s)
}

fn save(s: &AppSettings) -> Result<(), String> {
    let p = settings_path();
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(s).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 启动时把「最小化到托盘」读进原子缓存。
pub fn init() {
    let settings = load();
    MIN_TO_TRAY.store(settings.minimize_to_tray, Ordering::Relaxed);
    if let Err(error) = cleanup_expired_logs(settings.log_retention_days) {
        log::warn!("清理过期日志失败：{error}");
    }
}

pub fn start_log_retention_worker() {
    let _ = std::thread::Builder::new()
        .name("stacker-log-retention".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(6 * 60 * 60));
            let days = load().log_retention_days;
            if let Err(error) = cleanup_expired_logs(days) {
                log::warn!("定时清理过期日志失败：{error}");
            }
        });
}
pub fn minimize_to_tray() -> bool {
    MIN_TO_TRAY.load(Ordering::Relaxed)
}

#[tauri::command]
pub fn settings_get() -> AppSettings {
    load()
}

#[tauri::command]
pub fn settings_set_tray(enabled: bool) -> Result<(), String> {
    let mut s = load();
    s.minimize_to_tray = enabled;
    save(&s)?;
    MIN_TO_TRAY.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn settings_set_theme(theme: String) -> Result<(), String> {
    let mut s = load();
    s.theme = theme;
    save(&s)
}

#[tauri::command]
pub fn settings_set_locale(app: tauri::AppHandle, locale: String) -> Result<String, String> {
    use tauri::Manager;
    let locale = match locale.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" => "en-US".to_string(),
        _ => "zh-CN".to_string(),
    };
    let mut settings = load();
    settings.locale = locale.clone();
    save(&settings)?;
    crate::refresh_tray_menu(&app)?;
    if let Some(window) = app.get_webview_window("live-log") {
        let title = if locale == "en-US" {
            "Stacker Live Logs"
        } else {
            "Stacker 实时日志"
        };
        window.set_title(title).map_err(|error| error.to_string())?;
    }
    Ok(locale)
}

#[tauri::command]
pub fn settings_set_log_level(level: String) -> Result<String, String> {
    let level = normalize_log_level(&level).to_string();
    let mut settings = load();
    settings.log_level = level.clone();
    save(&settings)?;
    log::set_max_level(log_level_filter(&level));
    log::info!("日志级别已切换为 {}", level.to_ascii_uppercase());
    Ok(level)
}

#[tauri::command]
pub fn settings_set_log_retention_days(days: u16) -> Result<u16, String> {
    let days = days.clamp(1, 365);
    let mut settings = load();
    settings.log_retention_days = days;
    save(&settings)?;
    cleanup_expired_logs(days)?;
    Ok(days)
}

fn current_log_path() -> PathBuf {
    logs_dir().join(format!(
        "stacker-{}.log",
        chrono::Local::now().format("%Y-%m-%d")
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    pub path: String,
    pub content: String,
    pub offset: u64,
    pub truncated: bool,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogCleanupResult {
    pub deleted_files: u32,
    pub released_bytes: u64,
    pub failed_files: u32,
}

fn cleanup_logs_before(oldest_kept: chrono::NaiveDate) -> Result<LogCleanupResult, String> {
    let dir = logs_dir();
    if !dir.exists() {
        return Ok(LogCleanupResult::default());
    }
    let mut result = LogCleanupResult::default();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("读取日志目录失败：{e}"))? {
        let Ok(entry) = entry else {
            result.failed_files += 1;
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("log") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            result.failed_files += 1;
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .map(chrono::DateTime::<chrono::Local>::from)
            .map(|value| value.date_naive());
        if modified.is_ok_and(|date| date < oldest_kept) {
            let size = metadata.len();
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    result.deleted_files += 1;
                    result.released_bytes += size;
                }
                Err(_) => result.failed_files += 1,
            }
        }
    }
    Ok(result)
}

fn cleanup_expired_logs(retention_days: u16) -> Result<LogCleanupResult, String> {
    let oldest_kept = chrono::Local::now().date_naive()
        - chrono::Duration::days(i64::from(retention_days.saturating_sub(1)));
    cleanup_logs_before(oldest_kept)
}

#[tauri::command]
pub fn settings_clear_old_logs() -> Result<LogCleanupResult, String> {
    cleanup_logs_before(chrono::Local::now().date_naive())
}

#[tauri::command]
pub async fn settings_open_log_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("live-log") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    if LOG_WINDOW_OPENING.swap(true, Ordering::AcqRel) {
        return Ok(());
    }

    let title = if load().locale == "en-US" {
        "Stacker Live Logs"
    } else {
        "Stacker 实时日志"
    };
    let result = tauri::WebviewWindowBuilder::new(
        &app,
        "live-log",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(title)
    .inner_size(860.0, 560.0)
    .min_inner_size(640.0, 400.0)
    .center()
    .build()
    .map(|_| ())
    .map_err(|e| format!("打开实时日志窗口失败：{e}"));
    LOG_WINDOW_OPENING.store(false, Ordering::Release);
    result
}

#[tauri::command]
pub fn settings_open_logs_dir() -> Result<(), String> {
    let dir = logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建日志目录失败：{e}"))?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开日志目录失败：{e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开日志目录失败：{e}"))?;
        Ok(())
    }
}

#[tauri::command]
pub fn settings_read_log(offset: u64) -> Result<LogChunk, String> {
    const MAX_CHUNK: u64 = 256 * 1024;
    let path = current_log_path();
    let path_text = path.to_string_lossy().to_string();
    if !path.exists() {
        return Ok(LogChunk {
            path: path_text,
            content: String::new(),
            offset: 0,
            truncated: false,
        });
    }

    let mut file = std::fs::File::open(&path).map_err(|e| format!("读取日志失败：{e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("读取日志信息失败：{e}"))?
        .len();
    let requested = offset.min(len);
    let start = if len.saturating_sub(requested) > MAX_CHUNK {
        len.saturating_sub(MAX_CHUNK)
    } else {
        requested
    };
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("定位日志内容失败：{e}"))?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("读取日志内容失败：{e}"))?;

    Ok(LogChunk {
        path: path_text,
        content: String::from_utf8_lossy(&bytes).into_owned(),
        offset: len,
        truncated: start > requested,
    })
}

pub fn proxy_addr() -> (String, u16) {
    let s = load();
    (s.proxy_host, s.proxy_port)
}

pub fn proxy_manual() -> Vec<String> {
    load().no_proxy_manual
}

pub(crate) fn save_proxy_manual(manual: &[String]) -> Result<Vec<String>, String> {
    let mut cleaned = Vec::new();
    for value in manual {
        let value = value.trim().to_string();
        if !value.is_empty() && !cleaned.contains(&value) {
            cleaned.push(value);
        }
    }
    let mut settings = load();
    settings.no_proxy_manual = cleaned.clone();
    save(&settings)?;
    Ok(cleaned)
}

#[tauri::command]
pub fn settings_set_proxy_manual(manual: Vec<String>) -> Result<Vec<String>, String> {
    let manual = save_proxy_manual(&manual)?;
    crate::proxy::sync_manual(&manual)?;
    Ok(manual)
}

#[tauri::command]
pub fn settings_set_proxy_addr(host: String, port: u16) -> Result<(), String> {
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err("代理主机不能为空".into());
    }
    if port == 0 {
        return Err("代理端口无效".into());
    }
    let current_proxy = crate::proxy::status();
    let mut s = load();
    s.proxy_host = host;
    s.proxy_port = port;
    save(&s)?;
    if current_proxy.enabled {
        crate::proxy::enable(
            &s.proxy_host,
            s.proxy_port,
            false,
            current_proxy.no_proxy_manual,
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct OsInfo {
    pub name: String,
    pub build: u32,
    pub supported: bool, // Tauri 2 + WebView2 需 Windows 10（build≥10240）/ 11
}

/// 读取 Windows 版本（注册表 CurrentBuildNumber），判断是否满足运行要求。
#[tauri::command]
pub fn os_info() -> OsInfo {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        let cv = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .ok();
        let build: u32 = cv
            .as_ref()
            .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber").ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let name: String = cv
            .as_ref()
            .and_then(|k| k.get_value::<String, _>("ProductName").ok())
            .unwrap_or_else(|| "Windows".into());
        OsInfo {
            name,
            build,
            supported: build >= 10240,
        }
    }
    #[cfg(not(windows))]
    {
        OsInfo {
            name: "non-windows".into(),
            build: 0,
            supported: false,
        }
    }
}
