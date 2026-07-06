//! 应用自身的小配置（最小化到托盘、外观主题）。存 %APPDATA%\stacker\settings.json。
//! 主题主要由前端用 localStorage + data-theme 控制，这里也存一份便于换机/导出时一致。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

// 缓存「最小化到托盘」开关，供窗口关闭事件同步读取（不走异步命令）。
static MIN_TO_TRAY: AtomicBool = AtomicBool::new(false);

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
}
fn default_theme() -> String {
    "dark".into()
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
    s
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
    MIN_TO_TRAY.store(load().minimize_to_tray, Ordering::Relaxed);
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

pub fn proxy_addr() -> (String, u16) {
    let s = load();
    (s.proxy_host, s.proxy_port)
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
