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
    // stderr 线程：收集（失败时取末行），同时把 rustup/安装器常见的 stderr 进度实时发给前端。
    let mut err = child.stderr.take().unwrap();
    let buf = Arc::new(Mutex::new(String::new()));
    let b2 = buf.clone();
    let win_err = window.clone();
    let err_log = log_path.clone();
    let t_err = std::thread::spawn(move || {
        let mut line: Vec<u8> = Vec::new();
        let mut all: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 512];
        while let Ok(n) = err.read(&mut chunk) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&chunk[..n]);
            for &b in &chunk[..n] {
                if b == b'\r' || b == b'\n' {
                    if !line.is_empty() {
                        let l = String::from_utf8_lossy(&line).trim().to_string();
                        if !l.is_empty() {
                            let _ = win_err.emit("install-progress", l.clone());
                            process_log_line(err_log.as_deref(), format!("stderr {l}"));
                        }
                        line.clear();
                    }
                } else {
                    line.push(b);
                }
            }
        }
        if !line.is_empty() {
            let l = String::from_utf8_lossy(&line).trim().to_string();
            if !l.is_empty() {
                let _ = win_err.emit("install-progress", l.clone());
                process_log_line(err_log.as_deref(), format!("stderr {l}"));
            }
        }
        *b2.lock().unwrap() = String::from_utf8_lossy(&all).into_owned();
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
    if status.success()
        || (ready_since.is_some() && success_probe.map(|probe| probe()).unwrap_or(false))
    {
        Ok(())
    } else {
        let lines = stderr_text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>();
        let tail = lines
            .iter()
            .rev()
            .find(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("error:")
                    || lower.contains("failed")
                    || lower.contains("denied")
                    || lower.contains("拒绝访问")
                    || lower.contains("timed out")
                    || lower.contains("timeout")
            })
            .or_else(|| {
                lines.iter().rev().find(|line| {
                    let lower = line.to_ascii_lowercase();
                    !lower.starts_with("info:")
                        && !lower.starts_with("warning:")
                        && !lower.contains("cleaning up")
                })
            })
            .or_else(|| lines.last())
            .copied()
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
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_DIST_SERVER",
        "RUSTUP_UPDATE_ROOT",
    ] {
        if let Some(val) = get_raw_in(Hive::User, name).or_else(|| get_raw_in(Hive::System, name)) {
            v.push((name.to_string(), val));
        }
    }
    v
}

#[derive(serde::Serialize)]
pub struct EcosystemActivationCommands {
    pub powershell: String,
    pub gitbash: String,
    pub cmd: String,
}

#[tauri::command]
pub fn ecosystem_activation_commands(
    ecosystem: String,
) -> Result<EcosystemActivationCommands, String> {
    let names: &[&str] = match ecosystem.as_str() {
        "git" => &[],
        "python" => &["PYENV", "PYENV_HOME", "PYENV_ROOT"],
        "node" => &["FNM_DIR"],
        "java" => &["JAVA_HOME"],
        "maven" => &["MAVEN_HOME", "M2_HOME", "JAVA_HOME"],
        "gradle" => &["GRADLE_HOME", "JAVA_HOME"],
        "go" => &["GOROOT", "GOPATH"],
        "rust" => &[
            "CARGO_HOME",
            "RUSTUP_HOME",
            "RUSTUP_DIST_SERVER",
            "RUSTUP_UPDATE_ROOT",
        ],
        _ => return Err("未知生态类型".into()),
    };
    let all = fresh_env_overrides();
    let selected = all
        .into_iter()
        .filter(|(key, _)| key == "PATH" || names.contains(&key.as_str()))
        .collect::<Vec<_>>();
    if !selected.iter().any(|(key, _)| key == "PATH") {
        return Err("无法读取当前用户与系统 PATH。".into());
    }

    let powershell = selected
        .iter()
        .map(|(key, value)| format!("$env:{key}='{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join("; ");
    let cmd = selected
        .iter()
        .map(|(key, value)| format!("set \"{key}={value}\""))
        .collect::<Vec<_>>()
        .join(" & ");
    let gitbash = selected
        .iter()
        .map(|(key, value)| {
            let escaped = value.replace('\'', "'\\''");
            if key == "PATH" {
                format!("export PATH=\"$(cygpath -p -u '{escaped}')\"")
            } else {
                format!("export {key}='{escaped}'")
            }
        })
        .collect::<Vec<_>>()
        .join("; ");

    Ok(EcosystemActivationCommands {
        powershell,
        gitbash,
        cmd,
    })
}

#[cfg(windows)]
fn windows_terminal() -> Option<String> {
    crate::env::resolve_fresh("wt.exe")
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| {
                PathBuf::from(p)
                    .join("Microsoft")
                    .join("WindowsApps")
                    .join("wt.exe")
            })
        })
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn apply_fresh_env_to_command(c: &mut std::process::Command) {
    for (k, val) in fresh_env_overrides() {
        c.env(k, val);
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub(crate) fn powershell_encoded_command(script: &str) -> String {
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    base64_encode(&bytes)
}

fn powershell_launch_script(cwd: &str, command: Option<&str>) -> String {
    let prefix = format!(
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)\n$OutputEncoding=[Console]::OutputEncoding\nchcp 65001 > $null\n$machinePath=[Environment]::ExpandEnvironmentVariables([Environment]::GetEnvironmentVariable('Path','Machine'))\n$userPath=[Environment]::ExpandEnvironmentVariables([Environment]::GetEnvironmentVariable('Path','User'))\n$env:Path=(@($machinePath,$userPath) | Where-Object {{ $_ }}) -join ';'\nSet-Location -LiteralPath '{}'",
        cwd.replace('\'', "''")
    );
    match command {
        Some(command) => format!("{prefix}\n{command}"),
        None => prefix,
    }
}

/// 打开一个终端窗口（powershell / gitbash / cmd），工作目录默认 Stacker 所在目录。
#[tauri::command]
pub fn open_shell(
    kind: String,
    cwd: Option<String>,
    command: Option<String>,
) -> Result<(), String> {
    let cwd = cwd.filter(|s| !s.trim().is_empty()).unwrap_or_else(app_dir);
    let command = command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(validate_shell_launch_command)
        .transpose()?;
    launch_shell(&kind, &cwd, command.as_deref())
}

#[tauri::command]
pub fn open_ecosystem_verify_shell(kind: String, ecosystem: String) -> Result<(), String> {
    let command = verification_command(&kind, &ecosystem)?;
    launch_shell(&kind, &app_dir(), Some(&command))
}

fn launch_shell(kind: &str, cwd: &str, command: Option<&str>) -> Result<(), String> {
    let mut c = std::process::Command::new("cmd");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
        apply_fresh_env_to_command(&mut c); // 注入注册表最新 env
    }
    match kind {
        "powershell" => {
            let ps = powershell_launch_script(cwd, command);
            let encoded = powershell_encoded_command(&ps);
            let title = if command.is_some() {
                "Stacker Verify"
            } else {
                "Stacker PowerShell"
            };
            #[cfg(windows)]
            if let Some(wt) = windows_terminal() {
                let mut wt_cmd = std::process::Command::new(wt);
                apply_fresh_env_to_command(&mut wt_cmd);
                wt_cmd.args([
                    "new-tab",
                    "--title",
                    title,
                    "powershell.exe",
                    "-NoExit",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-EncodedCommand",
                    &encoded,
                ]);
                wt_cmd.spawn().map_err(|e| e.to_string())?;
                return Ok(());
            }
            c.args([
                "/c",
                "start",
                "",
                "powershell.exe",
                "-NoExit",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                &encoded,
            ]);
        }
        "cmd" => {
            if let Some(command) = command {
                let script = std::env::temp_dir().join(format!(
                    "stacker-verify-{}.cmd",
                    chrono::Local::now().format("%Y%m%d%H%M%S%3f")
                ));
                let body = format!(
                    "@echo off\r\n\
                     title Stacker Verify\r\n\
                     cd /d \"{cwd}\"\r\n\
                     {command}\r\n\
                     echo.\r\n\
                     echo Verification finished. You can continue using this shell.\r\n\
                     cmd /k\r\n"
                );
                fs::write(&script, body).map_err(|e| e.to_string())?;
                let script_arg = script.to_string_lossy().into_owned();
                c.args(["/c", "start", "", &script_arg]);
            } else {
                c.args([
                    "/c",
                    "start",
                    "",
                    "cmd.exe",
                    "/k",
                    &format!("cd /d \"{cwd}\""),
                ]);
            }
        }
        "gitbash" => {
            #[cfg(windows)]
            {
                let gb = git_bash().ok_or("未找到 Git Bash（git-bash.exe）")?;
                if let Some(command) = command {
                    let bash_command = format!("cd '{}'; {}", cwd.replace('\'', "'\\''"), command);
                    c.args(["/c", "start", "", &gb, "-lc", &bash_command]);
                } else {
                    c.args(["/c", "start", "", &gb, &format!("--cd={cwd}")]);
                }
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

fn verification_command(kind: &str, ecosystem: &str) -> Result<String, String> {
    let (label, commands): (&str, &[&str]) = match ecosystem {
        "python" => (
            "Python",
            &[
                "python --version",
                "pip --version",
                "where python",
                "where pip",
            ],
        ),
        "node" => ("Node.js", &["node -v", "npm -v", "where node", "where npm"]),
        "java" => (
            "Java",
            &[
                "java -version",
                "javac -version",
                "echo %JAVA_HOME%",
                "where java",
                "where javac",
            ],
        ),
        "maven" => ("Maven", &["mvn -v", "echo %MAVEN_HOME%", "where mvn"]),
        "gradle" => (
            "Gradle",
            &["gradle -v", "echo %GRADLE_HOME%", "where gradle"],
        ),
        "go" => (
            "Go",
            &[
                "go version",
                "go env GOROOT",
                "go env GOPATH",
                "go env GOPROXY",
                "where go",
            ],
        ),
        "rust" => (
            "Rust",
            &[
                "rustc --version",
                "cargo --version",
                "rustup show",
                "where rustc",
                "where cargo",
                "where rustup",
            ],
        ),
        "git" => (
            "Git",
            &[
                "git --version",
                "git config --global --get user.name",
                "git config --global --get user.email",
                "git config --global --get credential.helper",
                "where git",
            ],
        ),
        _ => return Err("未知生态类型".into()),
    };

    match kind {
        "powershell" => {
            let mut parts = vec![format!(
                "Write-Host 'Stacker environment verification: {label}'"
            )];
            if ecosystem == "node" {
                parts.push("$env:COREPACK_ENABLE_DOWNLOAD_PROMPT='0'".into());
            }
            for command in commands {
                parts.push(format!(
                    "Write-Host '{}:'",
                    verification_step_label(command)
                ));
                parts.push(command.replace("where ", "where.exe "));
            }
            if ecosystem == "node" {
                parts.push("Write-Host 'pnpm:'; if (Get-Command pnpm -ErrorAction SilentlyContinue) { pnpm -v; if ($LASTEXITCODE -ne 0) { Write-Host 'not installed or not prepared by Corepack' } } else { Write-Host 'not installed' }".into());
                parts.push("Write-Host 'Yarn:'; if (Get-Command yarn -ErrorAction SilentlyContinue) { yarn -v; if ($LASTEXITCODE -ne 0) { Write-Host 'not installed or not prepared by Corepack' } } else { Write-Host 'not installed' }".into());
            }
            parts.push("Write-Host ''".into());
            parts.push("Write-Host 'Verification finished. Review the output above.'".into());
            Ok(parts.join("; "))
        }
        "cmd" => {
            let mut parts = vec![format!("echo Stacker environment verification: {label}")];
            if ecosystem == "node" {
                parts.push("set COREPACK_ENABLE_DOWNLOAD_PROMPT=0".into());
            }
            for command in commands {
                parts.push(format!("echo {}:", verification_step_label(command)));
                parts.push(command.to_string());
            }
            if ecosystem == "node" {
                parts.push("echo pnpm:".into());
                parts.push("(where pnpm >nul 2>nul && (pnpm -v || echo not installed or not prepared by Corepack)) || echo not installed".into());
                parts.push("echo Yarn:".into());
                parts.push("(where yarn >nul 2>nul && (yarn -v || echo not installed or not prepared by Corepack)) || echo not installed".into());
            }
            Ok(parts.join(" & "))
        }
        "gitbash" => {
            let mut parts = vec![format!("echo 'Stacker environment verification: {label}'")];
            if ecosystem == "node" {
                parts.push("export COREPACK_ENABLE_DOWNLOAD_PROMPT=0".into());
            }
            for command in commands {
                parts.push(format!("echo '{}:'", verification_step_label(command)));
                let mapped = if let Some(rest) = command.strip_prefix("where ") {
                    format!("command -v {rest}")
                } else if *command == "echo %JAVA_HOME%" {
                    "echo \"$JAVA_HOME\"".into()
                } else if *command == "echo %MAVEN_HOME%" {
                    "echo \"$MAVEN_HOME\"".into()
                } else if *command == "echo %GRADLE_HOME%" {
                    "echo \"$GRADLE_HOME\"".into()
                } else {
                    command.to_string()
                };
                parts.push(mapped);
            }
            if ecosystem == "node" {
                parts.push("echo 'pnpm:'".into());
                parts.push("if command -v pnpm >/dev/null 2>&1; then pnpm -v || echo 'not installed or not prepared by Corepack'; else echo 'not installed'; fi".into());
                parts.push("echo 'Yarn:'".into());
                parts.push("if command -v yarn >/dev/null 2>&1; then yarn -v || echo 'not installed or not prepared by Corepack'; else echo 'not installed'; fi".into());
            }
            parts.push("echo".into());
            parts.push("echo 'Verification finished. You can continue using this shell.'".into());
            parts.push("exec bash -i".into());
            Ok(parts.join("; "))
        }
        _ => Err("未知 shell".into()),
    }
}

fn verification_step_label(command: &str) -> &'static str {
    match command {
        "python --version" => "Python",
        "pip --version" => "pip",
        "where python" => "Python path",
        "where pip" => "pip path",
        "node -v" => "Node.js",
        "npm -v" => "npm",
        "where node" => "Node.js path",
        "where npm" => "npm path",
        "java -version" => "Java",
        "javac -version" => "javac",
        "echo %JAVA_HOME%" => "JAVA_HOME",
        "where java" => "Java path",
        "where javac" => "javac path",
        "mvn -v" => "Maven",
        "echo %MAVEN_HOME%" => "MAVEN_HOME",
        "where mvn" => "Maven path",
        "gradle -v" => "Gradle",
        "echo %GRADLE_HOME%" => "GRADLE_HOME",
        "where gradle" => "Gradle path",
        "go version" => "Go",
        "go env GOROOT" => "GOROOT",
        "go env GOPATH" => "GOPATH",
        "go env GOPROXY" => "GOPROXY",
        "where go" => "Go path",
        "rustc --version" => "rustc",
        "cargo --version" => "Cargo",
        "rustup show" => "rustup show",
        "where rustc" => "rustc path",
        "where cargo" => "Cargo path",
        "where rustup" => "rustup path",
        "git --version" => "Git",
        "git config --global --get user.name" => "Global user.name",
        "git config --global --get user.email" => "Global user.email",
        "git config --global --get credential.helper" => "Credential helper",
        "where git" => "Git path",
        _ => "Command",
    }
}

#[allow(dead_code)]
pub(crate) fn open_scoped_shell(
    kind: &str,
    cwd: Option<&str>,
    title: &str,
    environment: &[(String, String)],
) -> Result<(), String> {
    open_scoped_shell_with_intro(kind, cwd, title, environment, &[])
}

pub(crate) fn open_scoped_shell_with_intro(
    kind: &str,
    cwd: Option<&str>,
    title: &str,
    environment: &[(String, String)],
    intro_lines: &[String],
) -> Result<(), String> {
    let cwd = cwd
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(app_dir)
        });
    if !std::path::Path::new(&cwd).is_dir() {
        return Err("所选终端工作目录不存在。".into());
    }
    validate_scoped_shell_values(title, environment)?;

    match kind {
        "powershell" => {
            let mut script = powershell_launch_script(&cwd, None);
            script.push_str(&format!(
                "\n$Host.UI.RawUI.WindowTitle='{}'",
                title.replace('\'', "''")
            ));
            for (key, value) in environment {
                script.push_str(&format!("\n$env:{}='{}'", key, value.replace('\'', "''")));
            }
            if !intro_lines.is_empty() {
                script.push_str("\nWrite-Host ''");
                for line in intro_lines {
                    script.push_str(&format!("\nWrite-Host '{}'", line.replace('\'', "''")));
                }
                script.push_str("\nWrite-Host ''");
            }
            let encoded = powershell_encoded_command(&script);
            #[cfg(windows)]
            if let Some(wt) = windows_terminal() {
                let mut command = std::process::Command::new(wt);
                apply_fresh_env_to_command(&mut command);
                command.args([
                    "-w",
                    "new",
                    "new-tab",
                    "--title",
                    title,
                    "powershell.exe",
                    "-NoExit",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-EncodedCommand",
                    &encoded,
                ]);
                command.spawn().map_err(|e| e.to_string())?;
                return Ok(());
            }
            let mut command = std::process::Command::new("cmd.exe");
            command.args([
                "/c",
                "start",
                "",
                "powershell.exe",
                "-NoExit",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                &encoded,
            ]);
            apply_fresh_env_to_command(&mut command);
            command.spawn().map_err(|e| e.to_string())?;
        }
        "cmd" => {
            let mut setup = environment
                .iter()
                .map(|(key, value)| format!("set \"{key}={value}\""))
                .collect::<Vec<_>>();
            setup.push(format!("title {title}"));
            setup.push(format!("cd /d \"{cwd}\""));
            for line in intro_lines {
                setup.push(format!("echo {}", cmd_echo_escape(line)));
            }
            if !intro_lines.is_empty() {
                setup.push("echo.".into());
            }
            let mut command = std::process::Command::new("cmd.exe");
            command.args(["/c", "start", title, "cmd.exe", "/k", &setup.join(" && ")]);
            apply_fresh_env_to_command(&mut command);
            command.spawn().map_err(|e| e.to_string())?;
        }
        "gitbash" => {
            let git_bash = git_bash().ok_or("未找到 Git Bash（git-bash.exe）。")?;
            let mut setup = environment
                .iter()
                .map(|(key, value)| format!("export {key}='{}'", value.replace('\'', "'\\''")))
                .collect::<Vec<_>>();
            setup.push(format!("cd '{}'", cwd.replace('\'', "'\\''")));
            setup.push(format!("printf '\\033]0;{}\\007'", title.replace('\'', "")));
            for line in intro_lines {
                setup.push(format!("echo '{}'", line.replace('\'', "'\\''")));
            }
            if !intro_lines.is_empty() {
                setup.push("echo".into());
            }
            setup.push("exec bash -i".into());
            let mut command = std::process::Command::new("cmd.exe");
            command.args(["/c", "start", title, &git_bash, "-lc", &setup.join("; ")]);
            apply_fresh_env_to_command(&mut command);
            command.spawn().map_err(|e| e.to_string())?;
        }
        _ => return Err("不支持的终端类型。".into()),
    }
    Ok(())
}

fn cmd_echo_escape(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
}

fn validate_scoped_shell_values(
    title: &str,
    environment: &[(String, String)],
) -> Result<(), String> {
    if title.is_empty() || title.len() > 100 || title.chars().any(char::is_control) {
        return Err("终端标题格式不正确。".into());
    }
    for (key, value) in environment {
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            || value.len() > 512
            || value.chars().any(char::is_control)
        {
            return Err("终端账号上下文包含无效内容。".into());
        }
    }
    Ok(())
}

fn validate_shell_launch_command(command: &str) -> Result<String, String> {
    if command.len() > 80 {
        return Err("启动命令过长".into());
    }
    if command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        Ok(command.to_string())
    } else {
        Err("启动命令包含不支持的字符".into())
    }
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
