//! 终端代理：设用户级 HTTP_PROXY/HTTPS_PROXY/ALL_PROXY，覆盖几乎所有 CLI 工具。
//! NO_PROXY 自动带上 localhost + 当前镜像源域名，避免镜像源请求进入终端代理。

use crate::{backup, sources, winenv};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct ProxyStatus {
    pub enabled: bool,
    pub http: String,
    pub host: String,
    pub port: u16,
    pub detected_port: Option<u16>,
    pub no_proxy_auto: Vec<String>, // localhost + 当前镜像源域名（只读展示）
    pub no_proxy_manual: Vec<String>, // 用户追加的（NO_PROXY 里非自动项）
    pub jvm: bool,                  // gradle.properties 是否已配代理
}

fn read_file(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

// ── 探测 Clash/mihomo 端口 ──
fn find_port(text: &str, field: &str) -> Option<u16> {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix(field) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix(':') {
                if let Ok(p) = rest.trim().parse::<u16>() {
                    if p > 0 {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

pub fn detect_clash_port() -> Option<u16> {
    let home = dirs::home_dir()?;
    let mut candidates: Vec<PathBuf> = vec![
        home.join(".config").join("clash").join("config.yaml"),
        home.join(".config").join("mihomo").join("config.yaml"),
        home.join(".config").join("clash.meta").join("config.yaml"),
        home.join(".config").join("clash-verge").join("config.yaml"),
    ];
    if let Some(ad) = dirs::config_dir() {
        candidates.push(
            ad.join("io.github.clash-verge-rev.clash-verge-rev")
                .join("config.yaml"),
        );
    }
    for p in candidates {
        if let Some(text) = read_file(&p) {
            for field in ["mixed-port", "port"] {
                if let Some(port) = find_port(&text, field) {
                    return Some(port);
                }
            }
        }
    }
    None
}

// ── 自动白名单：localhost + 当前镜像源域名 ──
fn auto_no_proxy() -> Vec<String> {
    let mut list = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    for h in sources::domestic_hosts() {
        if !list.contains(&h) {
            list.push(h);
        }
    }
    list
}

// ── 当前状态 ──
pub fn status() -> ProxyStatus {
    let http = winenv::get_user_raw("HTTP_PROXY").unwrap_or_default();
    let all = winenv::get_user_raw("ALL_PROXY").unwrap_or_default();
    let enabled = !http.is_empty() || !all.is_empty();

    let detected = detect_clash_port();
    let (host, port) = crate::settings::proxy_addr();

    let auto = auto_no_proxy();
    let current_np: Vec<String> = winenv::get_user_raw("NO_PROXY")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let manual: Vec<String> = current_np
        .into_iter()
        .filter(|h| !auto.contains(h))
        .collect();

    ProxyStatus {
        enabled,
        http,
        host,
        port,
        detected_port: detected,
        no_proxy_auto: auto,
        no_proxy_manual: manual,
        jvm: jvm_has_proxy(),
    }
}

fn jvm_has_proxy() -> bool {
    gradle_has_proxy()
        || winenv::get_user_raw("MAVEN_OPTS")
            .unwrap_or_default()
            .contains("-Dhttp.proxyHost")
}

// ── 开 / 关 ──
pub fn enable(host: &str, port: u16, also_jvm: bool, manual: Vec<String>) -> Result<(), String> {
    backup::backup_env(
        winenv::Hive::User,
        "proxy",
        &[
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "MAVEN_OPTS",
        ],
    );
    let http = format!("http://{host}:{port}");
    let socks = format!("socks5://{host}:{port}");
    winenv::set_user("HTTP_PROXY", &http)?;
    winenv::set_user("HTTPS_PROXY", &http)?;
    winenv::set_user("ALL_PROXY", &socks)?;

    let mut list = auto_no_proxy();
    for h in manual {
        let h = h.trim().to_string();
        if !h.is_empty() && !list.contains(&h) {
            list.push(h);
        }
    }
    winenv::set_user("NO_PROXY", &list.join(","))?;

    if also_jvm {
        gradle_set_proxy(host, port, &list)?;
        maven_set_proxy(host, port, &list)?;
    }
    Ok(())
}

pub fn disable(also_jvm: bool) -> Result<(), String> {
    backup::backup_env(
        winenv::Hive::User,
        "proxy",
        &[
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "MAVEN_OPTS",
        ],
    );
    winenv::remove_user("HTTP_PROXY")?;
    winenv::remove_user("HTTPS_PROXY")?;
    winenv::remove_user("ALL_PROXY")?;
    winenv::remove_user("NO_PROXY")?;
    if also_jvm {
        gradle_clear_proxy()?;
        maven_clear_proxy()?;
    }
    Ok(())
}

// ── gradle.properties 代理（JVM 不读环境变量）──
fn gradle_props_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".gradle")
        .join("gradle.properties")
}

const GRADLE_KEYS: [&str; 6] = [
    "systemProp.http.proxyHost",
    "systemProp.http.proxyPort",
    "systemProp.https.proxyHost",
    "systemProp.https.proxyPort",
    "systemProp.http.nonProxyHosts",
    "systemProp.https.nonProxyHosts",
];

fn gradle_has_proxy() -> bool {
    if let Some(text) = read_file(&gradle_props_path()) {
        return text
            .lines()
            .any(|l| l.trim_start().starts_with("systemProp.http.proxyHost"));
    }
    false
}

fn gradle_rewrite(pairs: &[(&str, String)]) -> Result<(), String> {
    let path = gradle_props_path();
    backup::backup_file(&path);
    let existing = read_file(&path).unwrap_or_default();
    // 去掉旧的受管行
    let mut out = String::new();
    for line in existing.lines() {
        let t = line.trim_start();
        if GRADLE_KEYS.iter().any(|k| t.starts_with(k)) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    for (k, v) in pairs {
        out.push_str(&format!("{k}={v}\n"));
    }
    if let Some(par) = path.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

fn gradle_set_proxy(host: &str, port: u16, no_proxy: &[String]) -> Result<(), String> {
    let non = no_proxy.join("|");
    let pairs = vec![
        ("systemProp.http.proxyHost", host.to_string()),
        ("systemProp.http.proxyPort", port.to_string()),
        ("systemProp.https.proxyHost", host.to_string()),
        ("systemProp.https.proxyPort", port.to_string()),
        ("systemProp.http.nonProxyHosts", non.clone()),
        ("systemProp.https.nonProxyHosts", non),
    ];
    gradle_rewrite(&pairs)
}

fn gradle_clear_proxy() -> Result<(), String> {
    gradle_rewrite(&[])
}

// ── Maven 代理（用 MAVEN_OPTS 传 JVM 系统属性，避免动 settings.xml 与换源冲突）──
const MAVEN_PROXY_FLAGS: [&str; 6] = [
    "-Dhttp.proxyHost",
    "-Dhttp.proxyPort",
    "-Dhttps.proxyHost",
    "-Dhttps.proxyPort",
    "-Dhttp.nonProxyHosts",
    "-Dhttps.nonProxyHosts",
];

// 取出 MAVEN_OPTS 里非代理的部分（保留用户自己的设置）
fn maven_opts_kept() -> Vec<String> {
    winenv::get_user_raw("MAVEN_OPTS")
        .unwrap_or_default()
        .split_whitespace()
        .filter(|t| !MAVEN_PROXY_FLAGS.iter().any(|k| t.starts_with(k)))
        .map(|s| s.to_string())
        .collect()
}

fn maven_set_proxy(host: &str, port: u16, no_proxy: &[String]) -> Result<(), String> {
    let non = no_proxy.join("|");
    let mut toks = maven_opts_kept();
    toks.push(format!("-Dhttp.proxyHost={host}"));
    toks.push(format!("-Dhttp.proxyPort={port}"));
    toks.push(format!("-Dhttps.proxyHost={host}"));
    toks.push(format!("-Dhttps.proxyPort={port}"));
    toks.push(format!("-Dhttp.nonProxyHosts={non}"));
    toks.push(format!("-Dhttps.nonProxyHosts={non}"));
    winenv::set_user("MAVEN_OPTS", &toks.join(" "))
}

fn maven_clear_proxy() -> Result<(), String> {
    let toks = maven_opts_kept();
    if toks.is_empty() {
        winenv::remove_user("MAVEN_OPTS")
    } else {
        winenv::set_user("MAVEN_OPTS", &toks.join(" "))
    }
}

// ── 立即生效脚本 ──
pub fn gen_scripts(host: &str, port: u16) -> Result<String, String> {
    let dir = dirs::config_dir().unwrap_or_default().join("stacker");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let http = format!("http://{host}:{port}");
    let socks = format!("socks5://{host}:{port}");
    let no_proxy = auto_no_proxy().join(",");

    let on = format!(
        "# 在当前 PowerShell 窗口立即开启代理：  . .\\proxy-on.ps1\n\
         $env:HTTP_PROXY  = \"{http}\"\n\
         $env:HTTPS_PROXY = \"{http}\"\n\
         $env:ALL_PROXY   = \"{socks}\"\n\
         $env:NO_PROXY    = \"{no_proxy}\"\n\
         Write-Host \"[stacker] 代理已开启 -> {http}\" -ForegroundColor Green\n"
    );
    let off = "# 在当前 PowerShell 窗口立即关闭代理：  . .\\proxy-off.ps1\n\
         Remove-Item Env:HTTP_PROXY  -ErrorAction SilentlyContinue\n\
         Remove-Item Env:HTTPS_PROXY -ErrorAction SilentlyContinue\n\
         Remove-Item Env:ALL_PROXY   -ErrorAction SilentlyContinue\n\
         Remove-Item Env:NO_PROXY    -ErrorAction SilentlyContinue\n\
         Write-Host \"[stacker] 代理已关闭\" -ForegroundColor Yellow\n";

    let on_path = dir.join("proxy-on.ps1");
    std::fs::write(&on_path, on).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("proxy-off.ps1"), off).map_err(|e| e.to_string())?;
    Ok(on_path.to_string_lossy().to_string())
}

// ── Tauri 命令 ──
#[tauri::command]
pub fn proxy_status() -> ProxyStatus {
    status()
}

#[tauri::command]
pub fn proxy_enable(
    host: String,
    port: u16,
    also_jvm: bool,
    manual: Vec<String>,
) -> Result<(), String> {
    enable(&host, port, also_jvm, manual)
}

#[tauri::command]
pub fn proxy_disable(also_jvm: bool) -> Result<(), String> {
    disable(also_jvm)
}

#[tauri::command]
pub fn proxy_gen_scripts(host: String, port: u16) -> Result<String, String> {
    gen_scripts(&host, port)
}
