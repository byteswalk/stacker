//! Rust 工具链管理：rustup 接管。检测 / 列工具链 / 设默认 / 装卸 / 一键安装（rsproxy 镜像）。

use serde::Serialize;

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let mut c = std::process::Command::new(program);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let out = c
        .output()
        .map_err(|e| format!("{program} 未找到或执行失败：{e}"))?;
    let so = String::from_utf8_lossy(&out.stdout).into_owned();
    let se = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok(so)
    } else {
        Err(if se.trim().is_empty() { so } else { se })
    }
}

/// rustup.exe 全路径：按注册表最新 PATH 解析（装好后进程旧 PATH 看不到 ~/.cargo/bin）。
fn rustup_exe() -> String {
    crate::env::resolve_fresh("rustup.exe")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rustup".into())
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
#[derive(Serialize, Default)]
pub struct RustupStatus {
    pub installed: bool,
    pub rustup_version: Option<String>,
    pub toolchains: Vec<Toolchain>,
    pub default: Option<String>,
}

// 异步：内含 rustup 子进程调用，避免阻塞主线程。
#[tauri::command]
pub async fn rustup_status() -> RustupStatus {
    tauri::async_runtime::spawn_blocking(rustup_status_impl)
        .await
        .unwrap_or_default()
}
fn rustup_status_impl() -> RustupStatus {
    let exe = rustup_exe();
    let rustup_version = run(&exe, &["--version"])
        .ok()
        .map(|s| s.lines().next().unwrap_or("").trim().to_string());
    let installed = rustup_version.is_some();
    let mut toolchains = Vec::new();
    let mut default = None;
    if installed {
        if let Ok(list) = run(&exe, &["toolchain", "list"]) {
            for line in list.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let is_default = line.contains("(default)");
                let raw = line.replace("(default)", "").replace("(override)", "");
                let name = short_name(raw.trim());
                if is_default {
                    default = Some(name.clone());
                }
                toolchains.push(Toolchain { name, is_default });
            }
        }
    }
    RustupStatus {
        installed,
        rustup_version,
        toolchains,
        default,
    }
}

#[tauri::command]
pub async fn rustup_set_default(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run(&rustup_exe(), &["default", &name]).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rustup_install(window: tauri::Window, channel: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::installer::run_with_heartbeat(
            &window,
            &rustup_exe(),
            &["toolchain", "install", &channel],
            &[],
            "通过 rustup 下载工具链中",
        )?;
        Ok::<String, String>(channel)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn rustup_uninstall(name: String) -> Result<(), String> {
    run(&rustup_exe(), &["toolchain", "uninstall", &name]).map(|_| ())
}

/// 更新 rustup 自身（`rustup self update`，走 rsproxy 镜像）。它本身就是"有更新才更新"。
#[tauri::command]
pub async fn rustup_self_update(window: tauri::Window) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::installer::run_with_heartbeat(
            &window,
            &rustup_exe(),
            &["self", "update"],
            &[("RUSTUP_UPDATE_ROOT", "https://rsproxy.cn/rustup")],
            "检查并更新 rustup 中",
        )?;
        Ok("rustup 已是最新（或已更新）".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// rsproxy 不直接托管 /rustup-init.exe（会 404）；Windows 的 exe 在 dist 路径下。官方 static 兜底。
const RUSTUP_INIT_URLS: [&str; 2] = [
    "https://rsproxy.cn/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe",
    "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe",
];

/// 一键安装 rustup：下 rsproxy 的 rustup-init.exe 静默安装，预设 RUSTUP_DIST_SERVER/UPDATE_ROOT 走 rsproxy.cn。
#[tauri::command]
pub async fn rustup_install_self(window: tauri::Window) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || install_self_impl(&window))
        .await
        .map_err(|e| e.to_string())?
}
fn install_self_impl(window: &tauri::Window) -> Result<String, String> {
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
    let mut last = String::new();
    let mut ok = false;
    for url in RUSTUP_INIT_URLS {
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
    crate::installer::run_with_heartbeat(
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
        &[
            ("RUSTUP_DIST_SERVER", "https://rsproxy.cn"),
            ("RUSTUP_UPDATE_ROOT", "https://rsproxy.cn/rustup"),
        ],
        "安装 rustup + stable 工具链中",
    )?;
    // 持久化镜像环境变量（后续 rustup update 也走 rsproxy）
    crate::backup::backup_env(
        crate::winenv::Hive::User,
        "rustup",
        &["RUSTUP_DIST_SERVER", "RUSTUP_UPDATE_ROOT"],
    );
    crate::winenv::set_user("RUSTUP_DIST_SERVER", "https://rsproxy.cn")?;
    crate::winenv::set_user("RUSTUP_UPDATE_ROOT", "https://rsproxy.cn/rustup")?;
    // rustup-init 默认会改 PATH，但 --no-modify-path 时我们自己加 ~/.cargo/bin（保证可控）
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        crate::winenv::prepend_path_in(crate::winenv::Hive::User, &cargo_bin.to_string_lossy())?;
    }
    let _ = std::fs::remove_file(&tmp);
    if crate::env::resolve_fresh("rustup.exe").is_some() {
        Ok("已安装 rustup（rsproxy 镜像）".into())
    } else {
        Err("rustup 已安装但 PATH 未即时刷新，请重启应用后重试".into())
    }
}
