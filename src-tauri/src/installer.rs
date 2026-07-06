//! 下载器：从 URL 下载 zip（带进度事件 install-progress）→ 解压到目标目录。
//! 给手动生态（JDK / Maven / Gradle / Go）"无管理器自带下载"用。strip_top 去掉压缩包顶层目录。

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Emitter;

// 下载/安装的取消标志（下载循环 + run_with_heartbeat 检查；前端取消按钮调 op_cancel）。
static OP_CANCEL: AtomicBool = AtomicBool::new(false);
const DOWNLOAD_STALL_TIMEOUT_SECS: u64 = 30;
const PROCESS_LONG_HINT_SECS: u64 = 180;
fn process_log_line(log_path: Option<&Path>, msg: impl AsRef<str>) {
    let Some(path) = log_path else {
        return;
    };
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "[{ts}] {}", msg.as_ref());
    }
}
pub fn op_reset() {
    OP_CANCEL.store(false, Ordering::SeqCst);
}
pub fn op_cancelled() -> bool {
    OP_CANCEL.load(Ordering::SeqCst)
}

/// 取消当前下载 / 安装。
#[tauri::command]
pub fn op_cancel() {
    OP_CANCEL.store(true, Ordering::SeqCst);
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("taskkill");
        c.args(["/PID", &child.id().to_string(), "/T", "/F"]);
        c.creation_flags(0x08000000);
        let _ = c.output();
    }
    let _ = child.kill();
}

fn extract_zip(zip_path: &Path, dest: &Path, strip_top: bool) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut ar = zip::ZipArchive::new(file).map_err(|e| format!("打开压缩包失败：{e}"))?;
    // 顶层公共目录（如 jdk-21.0.5+11/），strip 后直接落到 dest
    let prefix = if strip_top && !ar.is_empty() {
        let n = ar
            .by_index(0)
            .map_err(|e| e.to_string())?
            .name()
            .to_string();
        n.find('/').map(|i| n[..=i].to_string()).unwrap_or_default()
    } else {
        String::new()
    };
    fs::create_dir_all(dest).ok();
    for i in 0..ar.len() {
        let mut f = ar.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().replace('\\', "/");
        if name.contains("..") {
            continue;
        } // 防 zip-slip
        let rel = if !prefix.is_empty() && name.starts_with(&prefix) {
            name[prefix.len()..].to_string()
        } else {
            name.clone()
        };
        if rel.is_empty() {
            continue;
        }
        let Some(safe_rel) = safe_zip_rel(&rel) else {
            continue;
        };
        let outpath = dest.join(safe_rel);
        if f.is_dir() || rel.ends_with('/') {
            fs::create_dir_all(&outpath).ok();
            continue;
        }
        if let Some(p) = outpath.parent() {
            fs::create_dir_all(p).ok();
        }
        let mut out = fs::File::create(&outpath).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn safe_zip_rel(rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(part) => {
                if part.to_string_lossy().contains(':') {
                    return None;
                }
                out.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Stacker 所在目录（运行时下载的运行时默认装在它下面）。
#[tauri::command]
pub fn app_dir() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))
        .unwrap_or_default()
}

/// 跑一个长命令：流式发真实输出进度（按 \r/\n 切分）+ 心跳兜底 + 可取消（杀子进程）。
/// 进度走 "install-progress"；stderr 收集，失败时返回末行。
pub fn run_with_heartbeat(
    window: &tauri::Window,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> Result<(), String> {
    run_with_heartbeat_impl(window, program, args, envs, label, None, None)
}

#[allow(dead_code)]
pub fn run_with_heartbeat_logged(
    window: &tauri::Window,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
    log_path: &Path,
) -> Result<(), String> {
    run_with_heartbeat_impl(
        window,
        program,
        args,
        envs,
        label,
        None,
        Some(log_path.to_path_buf()),
    )
}

pub fn run_with_heartbeat_until(
    window: &tauri::Window,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
    success_probe: &dyn Fn() -> bool,
) -> Result<(), String> {
    run_with_heartbeat_impl(
        window,
        program,
        args,
        envs,
        label,
        Some(success_probe),
        None,
    )
}

fn run_with_heartbeat_impl(
    window: &tauri::Window,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
    success_probe: Option<&dyn Fn() -> bool>,
    log_path: Option<PathBuf>,
) -> Result<(), String> {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    op_reset();
    process_log_line(
        log_path.as_deref(),
        format!(
            "process start program={} args={:?} envs={:?} label={label}",
            program, args, envs
        ),
    );
    let mut c = std::process::Command::new(program);
    c.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let mut child = c.spawn().map_err(|e| {
        process_log_line(log_path.as_deref(), format!("process spawn failed err={e}"));
        format!("启动失败：{e}")
    })?;

    // stdout 线程：按 \r/\n 切分，逐段当进度发（捕获 \r 刷新的下载条）
    let mut out = child.stdout.take().unwrap();
    let win = window.clone();
    let out_log = log_path.clone();
    let t_out = std::thread::spawn(move || {
        let mut line: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 512];
        while let Ok(n) = out.read(&mut chunk) {
            if n == 0 {
                break;
            }
            for &b in &chunk[..n] {
                if b == b'\r' || b == b'\n' {
                    if !line.is_empty() {
                        let l = String::from_utf8_lossy(&line).trim().to_string();
                        if !l.is_empty() {
                            let _ = win.emit("install-progress", l.clone());
                            process_log_line(out_log.as_deref(), format!("stdout {l}"));
                        }
                        line.clear();
                    }
                } else {
                    line.push(b);
                }
            }
        }
    });
    // stderr 线程：收集（失败时取末行）
    let mut err = child.stderr.take().unwrap();
    let buf = Arc::new(Mutex::new(String::new()));
    let b2 = buf.clone();
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        *b2.lock().unwrap() = s;
    });

    let start = Instant::now();
    let mut ready_since: Option<Instant> = None;
    let status = loop {
        if op_cancelled() {
            process_log_line(log_path.as_deref(), "process cancelled by user");
            kill_process_tree(&mut child);
            let _ = child.wait();
            let _ = t_out.join();
            let _ = t_err.join();
            return Err("已取消".into());
        }
        if let Some(probe) = success_probe {
            if probe() {
                let ready_at = *ready_since.get_or_insert_with(Instant::now);
                let _ = window.emit(
                    "install-progress",
                    format!("{label} · 已检测到版本文件就绪，正在等待安装器收尾…"),
                );
                if ready_at.elapsed() > Duration::from_secs(8) {
                    kill_process_tree(&mut child);
                    let _ = child.wait();
                    let _ = t_out.join();
                    let _ = t_err.join();
                    return Ok(());
                }
            } else {
                ready_since = None;
            }
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                let elapsed = start.elapsed().as_secs();
                let hint = if elapsed >= PROCESS_LONG_HINT_SECS {
                    "安装程序仍在处理，首次安装可能需要较长时间"
                } else {
                    "正在处理，请等待完成"
                };
                let _ = window.emit(
                    "install-progress",
                    format!("{label} · 已 {elapsed}s（{hint}）"),
                );
                std::thread::sleep(Duration::from_millis(900));
            }
            Err(e) => {
                process_log_line(log_path.as_deref(), format!("process wait failed err={e}"));
                return Err(e.to_string());
            }
        }
    };
    let _ = t_out.join();
    let _ = t_err.join();
    let stderr_text = buf.lock().unwrap().clone();
    if !stderr_text.trim().is_empty() {
        process_log_line(
            log_path.as_deref(),
            format!("stderr\n{}", stderr_text.trim()),
        );
    }
    process_log_line(
        log_path.as_deref(),
        format!(
            "process exit success={} code={:?}",
            status.success(),
            status.code()
        ),
    );
    if status.success() {
        Ok(())
    } else if ready_since.is_some() && success_probe.map(|probe| probe()).unwrap_or(false) {
        Ok(())
    } else {
        let tail = stderr_text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .to_string();
        Err(if tail.is_empty() {
            "执行失败（无输出）".into()
        } else {
            tail
        })
    }
}

#[cfg(windows)]
pub fn git_bash() -> Option<String> {
    // 从 git.exe 路径上推 Git 根目录找 git-bash.exe
    let git = crate::env::resolve_fresh("git.exe")?;
    let root = git.parent()?.parent()?; // ...\Git\cmd\git.exe → ...\Git
    let gb = root.join("git-bash.exe");
    if gb.is_file() {
        return Some(gb.to_string_lossy().into_owned());
    }
    for p in [
        r"C:\Program Files\Git\git-bash.exe",
        r"C:\Program Files (x86)\Git\git-bash.exe",
    ] {
        if std::path::Path::new(p).is_file() {
            return Some(p.to_string());
        }
    }
    None
}

// app 进程的 env 是启动那刻的旧快照；从 Stacker 开的终端会继承它 → 看不到之后新装/新设的
// MAVEN_HOME、PATH 等（"设了默认但 mvn 不好用"就是这个）。这里用注册表最新值覆盖，
// 让"验证"终端和正常新开的终端一致。
#[cfg(windows)]
fn fresh_env_overrides() -> Vec<(String, String)> {
    use crate::winenv::{get_raw_in, Hive};
    let mut v = Vec::new();
    let path = crate::env::fresh_path_dirs()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    if !path.is_empty() {
        v.push(("PATH".to_string(), path));
    }
    for name in [
        "JAVA_HOME",
        "MAVEN_HOME",
        "M2_HOME",
        "GRADLE_HOME",
        "GOROOT",
        "GOPATH",
        "PYENV",
        "PYENV_HOME",
        "PYENV_ROOT",
        "FNM_DIR",
        "RUSTUP_DIST_SERVER",
        "RUSTUP_UPDATE_ROOT",
    ] {
        if let Some(val) = get_raw_in(Hive::User, name).or_else(|| get_raw_in(Hive::System, name)) {
            v.push((name.to_string(), val));
        }
    }
    v
}

/// 打开一个终端窗口（powershell / gitbash / cmd），工作目录默认 Stacker 所在目录。
#[tauri::command]
pub fn open_shell(kind: String, cwd: Option<String>) -> Result<(), String> {
    let cwd = cwd.filter(|s| !s.trim().is_empty()).unwrap_or_else(app_dir);
    let mut c = std::process::Command::new("cmd");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
        for (k, val) in fresh_env_overrides() {
            c.env(k, val);
        } // 注入注册表最新 env
    }
    match kind.as_str() {
        "powershell" => {
            c.args([
                "/c",
                "start",
                "powershell",
                "-NoExit",
                "-Command",
                &format!("Set-Location -LiteralPath '{}'", cwd.replace('\'', "''")),
            ]);
        }
        "cmd" => {
            c.args(["/c", "start", "cmd", "/k", &format!("cd /d \"{cwd}\"")]);
        }
        "gitbash" => {
            #[cfg(windows)]
            {
                let gb = git_bash().ok_or("未找到 Git Bash（git-bash.exe）")?;
                c.args(["/c", "start", "", &gb, &format!("--cd={cwd}")]);
            }
            #[cfg(not(windows))]
            {
                return Err("仅 Windows".into());
            }
        }
        _ => return Err("未知 shell".into()),
    }
    c.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ShellAvail {
    pub powershell: bool,
    pub gitbash: bool,
    pub cmd: bool,
}

/// 本机各终端是否可用：PowerShell / cmd 在 Windows 上恒有；Git Bash 取决于是否装了 Git for Windows。
#[tauri::command]
pub fn shells_available() -> ShellAvail {
    #[cfg(windows)]
    {
        ShellAvail {
            powershell: true,
            gitbash: git_bash().is_some(),
            cmd: true,
        }
    }
    #[cfg(not(windows))]
    {
        ShellAvail {
            powershell: false,
            gitbash: false,
            cmd: false,
        }
    }
}

fn dl_candidates(url: &str) -> Vec<String> {
    vec![url.to_string()]
}

fn host_of(u: &str) -> String {
    u.split('/').nth(2).unwrap_or(u).to_string()
}

/// 下载 url 的 zip 到 dest_dir 并解压。进度走 "install-progress" 事件（百分比 / __done__）。
/// 异步包装：放后台线程，避免阻塞主线程导致界面"未响应"。
#[tauri::command]
pub async fn installer_download(
    window: tauri::Window,
    url: String,
    dest_dir: String,
    strip_top: bool,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || download_impl(window, url, dest_dir, strip_top))
        .await
        .map_err(|e| e.to_string())?
}

/// 同步实现（供 fnm/pyenv 等在自己的后台线程里直接调用）。
pub fn download_impl(
    window: tauri::Window,
    url: String,
    dest_dir: String,
    strip_top: bool,
) -> Result<String, String> {
    download_impl_candidates(window, dl_candidates(&url), dest_dir, strip_top)
}

/// 同步实现：按调用方给出的候选 URL 顺序下载并解压。用于明确选择官方/镜像。
pub fn download_impl_candidates(
    window: tauri::Window,
    candidates: Vec<String>,
    dest_dir: String,
    strip_top: bool,
) -> Result<String, String> {
    use std::time::Duration;
    op_reset();
    // 连接/无进度超过 30s 失败；总下载时长不限制，避免大文件在正常下载中被误杀。
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(DOWNLOAD_STALL_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(DOWNLOAD_STALL_TIMEOUT_SECS))
        .timeout_write(Duration::from_secs(DOWNLOAD_STALL_TIMEOUT_SECS))
        .build();
    let mut resp = None;
    let mut last = String::new();
    for (i, u) in candidates.iter().enumerate() {
        if op_cancelled() {
            return Err("已取消".into());
        }
        let _ = window.emit(
            "install-progress",
            format!(
                "连接下载源 {}（{}/{}）…",
                host_of(u),
                i + 1,
                candidates.len()
            ),
        );
        match agent.get(u).call() {
            Ok(r) => {
                resp = Some(r);
                break;
            }
            Err(e) => {
                last = e.to_string();
                let _ = window.emit(
                    "install-progress",
                    format!("{} 连不上，换下一个…", host_of(u)),
                );
            }
        }
    }
    let resp = resp.ok_or_else(|| format!("所有下载源都连不上：{last}"))?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!(
        "stacker_download_{}_{}.zip",
        std::process::id(),
        chrono::Local::now().timestamp_millis()
    ));
    {
        let mut reader = resp.into_reader();
        let mut out = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 16];
        let (mut got, mut last) = (0u64, 0u64);
        loop {
            if op_cancelled() {
                drop(out);
                let _ = fs::remove_file(&tmp);
                return Err("已取消下载".into());
            }
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            got += n as u64;
            if got - last > (1 << 20) {
                last = got;
                let msg = if total > 0 {
                    format!(
                        "下载 {:.0}% · {:.1}/{:.1} MB",
                        got as f64 * 100.0 / total as f64,
                        got as f64 / 1048576.0,
                        total as f64 / 1048576.0
                    )
                } else {
                    format!("下载 {:.1} MB", got as f64 / 1048576.0)
                };
                let _ = window.emit("install-progress", msg);
            }
        }
    }
    let _ = window.emit("install-progress", "解压中…".to_string());
    extract_zip(&tmp, Path::new(&dest_dir), strip_top).map_err(|e| format!("解压失败：{e}"))?;
    let _ = fs::remove_file(&tmp);
    let _ = window.emit("install-progress", "__done__".to_string());
    Ok(dest_dir)
}

/// 解压编译进程序的内置 zip 资源。用于首次安装版本管理工具，避免干净机卡在联网下载。
pub fn extract_embedded_zip(
    window: tauri::Window,
    bytes: &[u8],
    label: &str,
    dest_dir: String,
    strip_top: bool,
) -> Result<String, String> {
    op_reset();
    if op_cancelled() {
        return Err("已取消".into());
    }
    let _ = window.emit("install-progress", format!("正在安装内置{label}…"));
    let tmp = std::env::temp_dir().join(format!(
        "stacker_embedded_{}_{}_{}.zip",
        label.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|', ' '], "_"),
        std::process::id(),
        chrono::Local::now().timestamp_millis()
    ));
    {
        let mut out = fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败：{e}"))?;
        out.write_all(bytes)
            .map_err(|e| format!("写入内置资源失败：{e}"))?;
    }
    let _ = window.emit("install-progress", "正在解压内置工具…".to_string());
    let result = extract_zip(&tmp, Path::new(&dest_dir), strip_top)
        .map_err(|e| format!("解压内置{label}失败：{e}"));
    let _ = fs::remove_file(&tmp);
    result?;
    let _ = window.emit("install-progress", "__done__".to_string());
    Ok(dest_dir)
}
