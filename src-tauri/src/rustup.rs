//! Rust 工具链管理：rustup 接管。检测 / 列工具链 / 设默认 / 装卸 / 一键安装。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn run(program: &Path, args: &[&str]) -> Result<String, String> {
    let mut c = Command::new(program);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    apply_rustup_env(&mut c, program);
    let out = c
        .output()
        .map_err(|e| format!("{} 未找到或执行失败：{e}", program.display()))?;
    let so = String::from_utf8_lossy(&out.stdout).into_owned();
    let se = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok(so)
    } else {
        Err(if se.trim().is_empty() { so } else { se })
    }
}

fn run_quick(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut c = Command::new(program);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    apply_rustup_env(&mut c, program);
    let mut child = c
        .spawn()
        .map_err(|e| format!("{} 无法启动：{e}", program.display()))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if out.status.success() {
                    return Ok(stdout);
                }
                return Err(if stderr.is_empty() { stdout } else { stderr });
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("rustup 命令在 {} 秒内未响应", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// rustup.exe 全路径：按注册表最新 PATH 解析（装好后进程旧 PATH 看不到 ~/.cargo/bin）。
fn rustup_exe() -> PathBuf {
    crate::env::resolve_fresh("rustup.exe").unwrap_or_else(|| PathBuf::from("rustup"))
}

fn inferred_cargo_home(rustup: &Path) -> Option<PathBuf> {
    let bin = rustup.parent()?;
    if !bin
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("bin"))
    {
        return None;
    }
    Some(bin.parent()?.to_path_buf())
}

fn rustup_cargo_home_override(rustup: &Path) -> Option<String> {
    let inferred = inferred_cargo_home(rustup)?;
    let current = std::env::var_os("CARGO_HOME").map(PathBuf::from);
    let current_rustup = current.as_ref().map(|p| p.join("bin").join("rustup.exe"));
    if current_rustup.as_ref().is_some_and(|p| {
        p.exists() && std::fs::canonicalize(p).ok() == std::fs::canonicalize(rustup).ok()
    }) {
        return None;
    }
    Some(inferred.to_string_lossy().into_owned())
}

fn apply_rustup_env(c: &mut std::process::Command, rustup: &Path) {
    if let Some(home) = rustup_cargo_home_override(rustup) {
        c.env("CARGO_HOME", home);
    }
    if let Some(bin) = rustup.parent() {
        let mut dirs = vec![bin.to_path_buf()];
        if let Some(path) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
        if let Ok(path) = std::env::join_paths(dirs) {
            c.env("PATH", path);
        }
    }
}

fn rustup_envs<'a>(
    cargo_home: &'a Option<String>,
    extra: &[(&'a str, &'a str)],
) -> Vec<(&'a str, &'a str)> {
    let mut envs = extra.to_vec();
    if let Some(home) = cargo_home.as_deref() {
        envs.push(("CARGO_HOME", home));
    }
    envs
}

fn run_rustup_download(
    window: &tauri::Window,
    program: &str,
    args: &[&str],
    cargo_home: &Option<String>,
    label: &str,
) -> Result<(), String> {
    use tauri::Emitter;

    let official_env = rustup_envs(cargo_home, &[]);
    match crate::installer::run_with_heartbeat(window, program, args, &official_env, label) {
        Ok(()) => Ok(()),
        Err(err) if err.contains("已取消") => Err(err),
        Err(official_err) => {
            let _ = window.emit("install-progress", "官方下载未完成，正在尝试备用下载源…");
            let fallback_env = rustup_envs(
                cargo_home,
                &[
                    ("RUSTUP_DIST_SERVER", "https://rsproxy.cn"),
                    ("RUSTUP_UPDATE_ROOT", "https://rsproxy.cn/rustup"),
                ],
            );
            crate::installer::run_with_heartbeat(window, program, args, &fallback_env, label)
                .map_err(|fallback_err| {
                    format!("官方下载失败：{official_err}；备用下载源失败：{fallback_err}")
                })
        }
    }
}

fn clean_source_url(source_url: Option<String>) -> String {
    source_url
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://static.rust-lang.org".into())
}

fn rustup_source_envs(base: &str) -> Vec<(String, String)> {
    if base == "https://static.rust-lang.org" {
        Vec::new()
    } else {
        vec![
            ("RUSTUP_DIST_SERVER".into(), base.into()),
            ("RUSTUP_UPDATE_ROOT".into(), format!("{base}/rustup")),
        ]
    }
}

fn rustup_envs_owned<'a>(
    cargo_home: &'a Option<String>,
    owned: &'a [(String, String)],
) -> Vec<(&'a str, &'a str)> {
    let mut envs = Vec::new();
    for (key, value) in owned {
        envs.push((key.as_str(), value.as_str()));
    }
    if let Some(home) = cargo_home.as_deref() {
        envs.push(("CARGO_HOME", home));
    }
    envs
}

fn run_rustup_download_from_source(
    window: &tauri::Window,
    program: &str,
    args: &[&str],
    cargo_home: &Option<String>,
    source_url: Option<String>,
    label: &str,
) -> Result<(), String> {
    use tauri::Emitter;
    let base = clean_source_url(source_url);
    let source_envs = rustup_source_envs(&base);
    let envs = rustup_envs_owned(cargo_home, &source_envs);
    let _ = window.emit("install-progress", format!("Rust 工具链下载源：{base}"));
    crate::installer::run_with_heartbeat(window, program, args, &envs, label)
}

fn parse_toolchain_line(line: &str) -> Option<Toolchain> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (raw, meta) = match line.split_once(" (") {
        Some((raw, meta)) => (raw.trim(), meta.trim_end_matches(')').to_lowercase()),
        None => (line, String::new()),
    };
    let is_default = meta.split(',').map(str::trim).any(|m| m == "default");
    Some(Toolchain {
        name: short_name(raw),
        is_default,
    })
}

/// 去掉工具链名里的宿主三元组后缀，便于展示与传参（stable-x86_64-pc-windows-msvc → stable）。
fn short_name(name: &str) -> String {
    for m in ["-x86_64-", "-aarch64-", "-i686-"] {
        if let Some(i) = name.find(m) {
            return name[..i].to_string();
        }
    }
    name.to_string()
}

#[derive(Serialize)]
pub struct Toolchain {
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize)]
pub struct RustupAddon {
    pub name: String,
    pub installed: bool,
}

fn parse_addon_list(output: &str) -> Vec<RustupAddon> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let installed = line.ends_with(" (installed)");
            let name = line
                .strip_suffix(" (installed)")
                .unwrap_or(line)
                .trim()
                .to_string();
            Some(RustupAddon { name, installed })
        })
        .collect()
}
#[derive(Serialize, Default)]
pub struct RustupStatus {
    pub installed: bool,
    pub rustup_version: Option<String>,
    pub toolchains: Vec<Toolchain>,
    pub default: Option<String>,
    pub default_version: Option<String>,
    pub probe_error: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
}

fn is_stable_toolchain_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| part.parse::<u32>().is_ok())
}

fn version_cmp_desc(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .map(|part| part.parse::<u32>().unwrap_or(0))
            .collect()
    };
    let av = parse(a);
    let bv = parse(b);
    for i in 0..3 {
        let ord = bv.get(i).unwrap_or(&0).cmp(av.get(i).unwrap_or(&0));
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    std::cmp::Ordering::Equal
}

fn release_versions_from_github() -> Result<Vec<String>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(2500))
        .timeout_read(Duration::from_millis(2500))
        .timeout_write(Duration::from_millis(2500))
        .build();
    let body = agent
        .get("https://api.github.com/repos/rust-lang/rust/releases?per_page=80")
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let releases: Vec<GithubRelease> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let mut versions = releases
        .into_iter()
        .filter(|release| !release.prerelease && !release.draft)
        .map(|release| release.tag_name.trim_start_matches('v').to_string())
        .filter(|version| is_stable_toolchain_version(version))
        .collect::<Vec<_>>();
    versions.sort_by(|a, b| version_cmp_desc(a, b));
    versions.dedup();
    Ok(versions)
}

fn fallback_versions() -> Vec<String> {
    [
        "1.97.0", "1.96.1", "1.96.0", "1.95.1", "1.95.0", "1.94.1", "1.94.0", "1.93.0", "1.92.0",
        "1.91.1", "1.91.0", "1.90.0", "1.89.0", "1.88.0", "1.87.0", "1.86.0", "1.85.1", "1.85.0",
        "1.84.1", "1.84.0", "1.83.0", "1.82.0", "1.81.0", "1.80.1", "1.80.0", "1.79.0", "1.78.0",
        "1.77.2", "1.77.1", "1.77.0", "1.76.0", "1.75.0", "1.74.1", "1.74.0", "1.73.0", "1.72.1",
        "1.72.0", "1.71.1", "1.71.0", "1.70.0",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn manifest_url(base: &str, version: &str) -> String {
    format!(
        "{}/dist/channel-rust-{}.toml",
        base.trim_end_matches('/'),
        version
    )
}

fn manifest_exists(base: &str, version: &str) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(1500))
        .timeout_read(Duration::from_millis(1500))
        .timeout_write(Duration::from_millis(1500))
        .build();
    agent
        .head(&manifest_url(base, version))
        .call()
        .map(|resp| resp.status() == 200)
        .unwrap_or(false)
}

#[tauri::command]
pub async fn rust_versions(source_url: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base = clean_source_url(source_url);
        let candidates = release_versions_from_github().unwrap_or_else(|_| fallback_versions());
        let candidates = candidates.into_iter().take(36).collect::<Vec<_>>();
        let handles = candidates
            .into_iter()
            .enumerate()
            .map(|(idx, version)| {
                let base = base.clone();
                std::thread::spawn(move || (idx, version.clone(), manifest_exists(&base, &version)))
            })
            .collect::<Vec<_>>();
        let mut checked = handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .filter(|(_, _, ok)| *ok)
            .collect::<Vec<_>>();
        checked.sort_by_key(|(idx, _, _)| *idx);
        let versions = checked
            .into_iter()
            .map(|(_, version, _)| version)
            .collect::<Vec<_>>();
        if versions.is_empty() {
            Err("当前 Rust 工具链下载源未返回可安装版本，请切换下载源后重试".into())
        } else {
            Ok(versions)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// 异步：内含 rustup 子进程调用，避免阻塞主线程。
#[tauri::command]
pub async fn rustup_status() -> RustupStatus {
    tauri::async_runtime::spawn_blocking(rustup_status_snapshot)
        .await
        .unwrap_or_default()
}
pub(crate) fn rustup_status_snapshot() -> RustupStatus {
    let Some(exe) = crate::env::resolve_fresh("rustup.exe") else {
        return RustupStatus::default();
    };
    let version_result = run_quick(&exe, &["--version"], Duration::from_secs(5));
    let rustup_version = version_result
        .as_ref()
        .ok()
        .map(|s| s.lines().next().unwrap_or("").trim().to_string());
    let installed = true;
    let mut probe_error = version_result.err();
    let mut toolchains = Vec::new();
    let mut default = None;
    let mut default_version = None;
    if probe_error.is_none() {
        match run_quick(&exe, &["toolchain", "list"], Duration::from_secs(5)) {
            Ok(list) => {
                for line in list.lines() {
                    if let Some(toolchain) = parse_toolchain_line(line) {
                        if toolchain.is_default {
                            default = Some(toolchain.name.clone());
                        }
                        toolchains.push(toolchain);
                    }
                }
            }
            Err(err) => probe_error = Some(err),
        }
        if probe_error.is_none() {
            if let Ok(out) = run_quick(
                &exe,
                &["run", "default", "rustc", "--version"],
                Duration::from_secs(5),
            ) {
                default_version = parse_rustc_version(&out);
            }
        }
    }
    RustupStatus {
        installed,
        rustup_version,
        toolchains,
        default,
        default_version,
        probe_error,
    }
}

fn parse_rustc_version(out: &str) -> Option<String> {
    out.split_whitespace()
        .find(|part| is_stable_toolchain_version(part))
        .map(str::to_string)
}

#[tauri::command]
pub async fn rustup_set_default(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = rustup_exe();
        run(&exe, &["default", &name]).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rustup_install(
    window: tauri::Window,
    channel: String,
    source_url: Option<String>,
    set_default: Option<bool>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = rustup_exe();
        let program = exe.to_string_lossy().into_owned();
        let cargo_home = rustup_cargo_home_override(&exe);
        run_rustup_download_from_source(
            &window,
            &program,
            &["toolchain", "install", &channel],
            &cargo_home,
            source_url,
            "通过 rustup 下载工具链中",
        )
        .map_err(explain_rustup_update_error)?;
        if set_default.unwrap_or(false) {
            run(&exe, &["default", &channel]).map(|_| ())?;
        }
        Ok::<String, String>(channel)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn rustup_uninstall(name: String) -> Result<(), String> {
    let exe = rustup_exe();
    run(&exe, &["toolchain", "uninstall", &name]).map(|_| ())
}

fn explain_rustup_update_error(err: String) -> String {
    let lower = err.to_ascii_lowercase();
    if lower.contains("failure removing component") && lower.contains("directory does not exist") {
        format!("{err}。当前工具链目录可能不完整，常见原因是安全软件或手动清理删除了工具链文件。建议卸载对应工具链后重新安装。")
    } else if lower.contains("rustup is not installed at") {
        format!("{err}。当前 CARGO_HOME 与 rustup 实际安装目录不一致，请确认 CARGO_HOME 指向正确的 Cargo 目录，或重新安装 rustup。")
    } else {
        err
    }
}

/// 更新已安装工具链（`rustup update`）。
#[tauri::command]
pub async fn rustup_update(
    window: tauri::Window,
    source_url: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = rustup_exe();
        let program = exe.to_string_lossy().into_owned();
        let cargo_home = rustup_cargo_home_override(&exe);
        run_rustup_download_from_source(
            &window,
            &program,
            &["update"],
            &cargo_home,
            source_url,
            "检查并更新 Rust 工具链中",
        )
        .map_err(explain_rustup_update_error)?;
        Ok("Rust 工具链已是最新（或已更新）".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 更新 rustup 自身（`rustup self update`）。
#[tauri::command]
pub async fn rustup_self_update(window: tauri::Window) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = rustup_exe();
        let program = exe.to_string_lossy().into_owned();
        let cargo_home = rustup_cargo_home_override(&exe);
        run_rustup_download(
            &window,
            &program,
            &["self", "update"],
            &cargo_home,
            "检查并更新 rustup 中",
        )?;
        Ok("rustup 已是最新（或已更新）".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rustup_components() -> Result<Vec<RustupAddon>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let exe = rustup_exe();
        run_quick(&exe, &["component", "list"], Duration::from_secs(15))
            .map(|output| parse_addon_list(&output))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn rustup_targets() -> Result<Vec<RustupAddon>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let exe = rustup_exe();
        run_quick(&exe, &["target", "list"], Duration::from_secs(15))
            .map(|output| parse_addon_list(&output))
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn set_addon(
    window: tauri::Window,
    kind: &'static str,
    name: String,
    install: bool,
    source_url: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = rustup_exe();
        let cargo_home = rustup_cargo_home_override(&exe);
        let action = if install { "add" } else { "remove" };
        run_rustup_download_from_source(
            &window,
            exe.to_string_lossy().as_ref(),
            &[kind, action, name.as_str()],
            &cargo_home,
            source_url,
            if install {
                "安装 Rust 附加项中"
            } else {
                "卸载 Rust 附加项中"
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn rustup_component_set(
    window: tauri::Window,
    name: String,
    install: bool,
    source_url: Option<String>,
) -> Result<(), String> {
    set_addon(window, "component", name, install, source_url).await
}

#[tauri::command]
pub async fn rustup_target_set(
    window: tauri::Window,
    name: String,
    install: bool,
    source_url: Option<String>,
) -> Result<(), String> {
    set_addon(window, "target", name, install, source_url).await
}

// 优先官方 rustup-init；连接失败时尝试备用下载源。
fn rustup_init_url(source_url: Option<String>) -> String {
    let base = clean_source_url(source_url);
    if base == "https://static.rust-lang.org" {
        "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
            .to_string()
    } else if base.ends_with("/rustup") {
        format!("{base}/dist/x86_64-pc-windows-msvc/rustup-init.exe")
    } else {
        format!("{base}/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe")
    }
}

/// 一键安装 rustup：下载 rustup-init.exe 并静默安装 stable 工具链。
#[tauri::command]
pub async fn rustup_install_self(
    window: tauri::Window,
    source_url: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || install_self_impl(&window, source_url))
        .await
        .map_err(|e| e.to_string())?
}
fn install_self_impl(window: &tauri::Window, source_url: Option<String>) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::time::Duration;
    use tauri::Emitter;
    const STALL_TIMEOUT_SECS: u64 = 30;
    crate::installer::op_reset();
    let tmp = std::env::temp_dir().join("rustup-init.exe");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_write(Duration::from_secs(STALL_TIMEOUT_SECS))
        .build();
    let url = rustup_init_url(source_url.clone());
    let mut last = String::new();
    let mut ok = false;
    for url in [url.as_str()] {
        let host = url.split('/').nth(2).unwrap_or(url);
        let _ = window.emit("install-progress", format!("下载 rustup-init（{host}）…"));
        match agent.get(url).call() {
            Ok(resp) => {
                let mut reader = resp.into_reader();
                let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                let mut buf = vec![0u8; 1 << 16];
                loop {
                    if crate::installer::op_cancelled() {
                        let _ = std::fs::remove_file(&tmp);
                        return Err("已取消".into());
                    }
                    let n = match reader.read(&mut buf) {
                        Ok(n) => n,
                        Err(e) => {
                            last = e.to_string();
                            break;
                        }
                    };
                    if n == 0 {
                        ok = true;
                        break;
                    }
                    out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                }
                if ok {
                    break;
                }
            }
            Err(e) => {
                last = e.to_string();
                let _ = window.emit("install-progress", format!("{host} 连不上，换下一个…"));
            }
        }
    }
    if !ok {
        return Err(format!("下载 rustup-init 失败：{last}"));
    }
    run_rustup_download_from_source(
        window,
        &tmp.to_string_lossy(),
        &[
            "-y",
            "--default-toolchain",
            "stable",
            "--profile",
            "default",
            "--no-modify-path",
        ],
        &None,
        source_url,
        "安装 rustup + stable 工具链中",
    )?;
    // rustup-init 默认会改 PATH，但 --no-modify-path 时我们自己加 ~/.cargo/bin（保证可控）
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        crate::winenv::prepend_path_in(crate::winenv::Hive::User, &cargo_bin.to_string_lossy())?;
    }
    let _ = std::fs::remove_file(&tmp);
    if crate::env::resolve_fresh("rustup.exe").is_some() {
        Ok("rustup 与 stable 工具链已安装".into())
    } else {
        Err("rustup 已安装但 PATH 未即时刷新，请重启应用后重试".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_toolchain_and_removes_host_suffix() {
        let toolchain = parse_toolchain_line("stable-x86_64-pc-windows-msvc (active, default)")
            .expect("toolchain should parse");
        assert_eq!(toolchain.name, "stable");
        assert!(toolchain.is_default);
    }

    #[test]
    fn parses_named_toolchain_without_default_marker() {
        let toolchain =
            parse_toolchain_line("1.84.1-x86_64-pc-windows-msvc").expect("toolchain should parse");
        assert_eq!(toolchain.name, "1.84.1");
        assert!(!toolchain.is_default);
    }
}
