//! 体检引擎：聚合"换源之外"的可优化项检测（代理端口是否在监听、开发缓存是否偏高）。
//! 换源类的可优化项前端已用 list_sources 算出；这里补需要后端能力的检测。

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use std::time::Instant;

use serde::Serialize;

#[derive(Serialize)]
pub struct CheckItem {
    pub id: String,
    pub sev: String, // warn | mid | info
    pub title: String,
    pub desc: String,
    pub page: String, // 点击跳转的页面
    pub action: String,
}

fn port_listening(host: &str, port: u16) -> bool {
    let addr = format!("{host}:{port}");
    match addr.to_socket_addrs() {
        Ok(mut it) => {
            it.any(|sa| TcpStream::connect_timeout(&sa, Duration::from_millis(800)).is_ok())
        }
        Err(_) => false,
    }
}

const GB: u64 = 1024 * 1024 * 1024;

fn windowsapps_dir(dir: &Path) -> bool {
    dir.to_string_lossy()
        .to_lowercase()
        .contains("\\windowsapps")
}

fn fresh_env_value(name: &str) -> Option<String> {
    crate::winenv::get_raw_in(crate::winenv::Hive::User, name)
        .or_else(|| crate::winenv::get_raw_in(crate::winenv::Hive::System, name))
        .or_else(|| std::env::var(name).ok())
        .filter(|s| !s.trim().is_empty())
}

fn python_command_candidate() -> Option<PathBuf> {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    for dir in dirs {
        if windowsapps_dir(&dir) {
            continue;
        }
        for name in ["python.exe", "python.bat", "python.cmd"] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn command_output_timeout(mut c: Command, timeout: Duration) -> Result<Output, String> {
    let mut child = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 python 命令失败：{e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("python 命令响应超时".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn apply_fresh_command_env(c: &mut Command) {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        c.env("PATH", path);
    }
    for name in ["PYENV", "PYENV_HOME", "PYENV_ROOT"] {
        if let Some(value) = fresh_env_value(name) {
            c.env(name, value);
        }
    }
}

fn run_python_version(program: &Path) -> Result<String, String> {
    let ext = program
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let mut c = if matches!(ext.as_str(), "bat" | "cmd") {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/d", "/s", "/c"])
            .arg(format!("\"{}\" --version", program.display()));
        cmd
    } else {
        let mut cmd = Command::new(program);
        cmd.arg("--version");
        cmd
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    apply_fresh_command_env(&mut c);
    let out = command_output_timeout(c, Duration::from_secs(5))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        return Err(text.trim().to_string());
    }
    for line in text.lines() {
        let line = line.trim();
        if let Some(version) = line.strip_prefix("Python ") {
            return Ok(version.trim().to_string());
        }
    }
    Err(text.trim().to_string())
}

fn python_default_check() -> Option<CheckItem> {
    let pyenv = crate::pyenv::pyenv_status_snapshot();
    let candidate = python_command_candidate();
    let command_ok = candidate
        .as_deref()
        .map(run_python_version)
        .is_some_and(|r| r.is_ok());

    if pyenv.installed {
        let ready: Vec<&str> = pyenv.versions.iter().map(|v| v.version.as_str()).collect();
        let default = pyenv.default.as_deref().unwrap_or("").trim();
        if ready.is_empty() {
            return Some(CheckItem {
                id: "python_no_runtime".into(),
                sev: "warn".into(),
                title: "未检测到可用的 Python 版本".into(),
                desc: "Python 版本管理工具已安装，但当前没有可用的 Python 运行时。请进入 Python 页安装一个版本并设为默认。".into(),
                page: "python".into(),
                action: "安装版本".into(),
            });
        }
        if !default.is_empty() && default != "system" && !ready.iter().any(|v| *v == default) {
            return Some(CheckItem {
                id: "python_default_missing".into(),
                sev: "warn".into(),
                title: "默认 Python 版本已失效".into(),
                desc: format!("当前默认版本 {default} 已不存在或安装不完整。请重新安装该版本，或选择其他版本设为默认。"),
                page: "python".into(),
                action: "去修复".into(),
            });
        }
        let default_exe_ok = if default.is_empty() || default == "system" {
            false
        } else {
            crate::pyenv::pyenv_python_exe(default)
                .as_deref()
                .map(run_python_version)
                .is_some_and(|r| r.is_ok())
        };
        let integration_ok = crate::pyenv::pyenv_integration_ready();
        if !command_ok && !(default_exe_ok && integration_ok) {
            let desc = if default == "system" || default.is_empty() {
                "当前没有可用的 python 命令。请在 Python 页选择一个已安装版本设为默认，或安装新版本。".to_string()
            } else if default_exe_ok && !integration_ok {
                format!("当前默认版本为 {default}，但 Python 命令入口尚未写入当前用户环境。请刷新 Python 命令入口后再试。")
            } else {
                format!("当前默认版本为 {default}，但新终端无法运行 python。请刷新 Python 命令入口后再试。")
            };
            return Some(CheckItem {
                id: "python_command_unavailable".into(),
                sev: "warn".into(),
                title: "默认 Python 命令不可用".into(),
                desc,
                page: "python".into(),
                action: "去修复".into(),
            });
        }
    } else if let Some(candidate) = candidate {
        if run_python_version(&candidate).is_err() {
            return Some(CheckItem {
                id: "python_command_broken".into(),
                sev: "warn".into(),
                title: "Python 命令不可用".into(),
                desc: "检测到 python 命令，但无法获取有效版本。请进入 Python 页重新配置默认版本。"
                    .into(),
                page: "python".into(),
                action: "去修复".into(),
            });
        }
    }
    None
}

fn node_default_check(f: &crate::fnm::FnmStatus) -> Option<CheckItem> {
    if !f.installed {
        return None;
    }
    if f.versions.is_empty() {
        return Some(CheckItem {
            id: "node_no_runtime".into(),
            sev: "warn".into(),
            title: "未检测到可用的 Node 版本".into(),
            desc: "Node 版本管理工具已安装，但当前没有可用的 Node 运行时。请进入 Node 页安装一个版本并设为默认。".into(),
            page: "node".into(),
            action: "安装版本".into(),
        });
    }
    let default = f.default.as_deref().unwrap_or("").trim();
    if default.is_empty() {
        return Some(CheckItem {
            id: "node_no_default".into(),
            sev: "warn".into(),
            title: "未设置默认 Node 版本".into(),
            desc: "已安装 Node 版本，但尚未设置默认版本。请进入 Node 页选择一个版本设为默认。"
                .into(),
            page: "node".into(),
            action: "去设置".into(),
        });
    }
    if crate::fnm::default_node_dir().is_none() {
        return Some(CheckItem {
            id: "node_default_missing".into(),
            sev: "warn".into(),
            title: "默认 Node 版本已失效".into(),
            desc: format!(
                "当前默认版本 {default} 已不存在或安装不完整。请重新安装该版本，或选择其他版本设为默认。"
            ),
            page: "node".into(),
            action: "去修复".into(),
        });
    }
    None
}

#[tauri::command]
pub async fn checkup_extra() -> Vec<CheckItem> {
    // 放后台线程：内含网络探测 + 磁盘缓存扫描，避免堵主线程让界面卡死。
    tauri::async_runtime::spawn_blocking(checkup_impl)
        .await
        .unwrap_or_default()
}

fn checkup_impl() -> Vec<CheckItem> {
    let mut out = Vec::new();

    // 0) Python：pyenv-win 已安装时，必须有可用版本并且新终端可运行 `python --version`。
    // 只在检测到 pyenv 或真实 python 命令时提示，避免干净新机器被误报。
    if let Some(item) = python_default_check() {
        out.push(item);
    }

    // 0b) Node：fnm 已安装时，必须有可用版本与默认版本。
    let f = crate::fnm::fnm_status_impl();
    if let Some(item) = node_default_check(&f) {
        out.push(item);
    }

    // 1) 终端代理：已配置但端口没人监听 → 终端联网会超时
    let s = crate::proxy::status();
    if s.enabled && !port_listening(&s.host, s.port) {
        out.push(CheckItem {
            id: "proxy_stale".into(),
            sev: "warn".into(),
            title: "终端代理已配置，但端口未在监听".into(),
            desc: format!("环境变量指向 {}:{}，但该端口当前无程序监听 → 终端联网会超时。代理软件没在运行时，可先关闭终端代理避免超时。", s.host, s.port),
            page: "proxy".into(),
            action: "关闭代理".into(),
        });
    }

    // 1b) fnm 装了但没写 shell 集成 → 切版本不生效（与 Node 页红条同口径：PS / Git Bash 都没写）
    if f.installed && !f.shell.powershell && !f.shell.gitbash {
        out.push(CheckItem {
            id: "fnm_no_integration".into(),
            sev: "warn".into(),
            title: "fnm 已装，但未写 shell 集成".into(),
            desc: "fnm 要在终端启动时执行钩子才会接管 Node → 现在切版本不生效。可一键写入（PowerShell / Git Bash / cmd，改动前自动备份）。".into(),
            page: "node".into(),
            action: "写入集成".into(),
        });
    }

    // 2) 开发缓存偏高 → 建议清理
    let caches = crate::cleanup::cleanup_scan();
    let managed: u64 = caches
        .iter()
        .filter(|c| c.category == "safe" || c.category == "cautious")
        .map(|c| c.size)
        .sum();
    let safe: u64 = caches
        .iter()
        .filter(|c| c.category == "safe")
        .map(|c| c.size)
        .sum();
    let history_total: u64 = caches
        .iter()
        .filter(|c| c.category == "history")
        .map(|c| c.size)
        .sum();
    let temp_total: u64 = caches
        .iter()
        .filter(|c| c.category == "temp")
        .map(|c| c.size)
        .sum();
    if managed > 5 * GB {
        out.push(CheckItem {
            id: "cache_high".into(),
            sev: "info".into(),
            title: "开发缓存占用偏高".into(),
            desc: format!(
                "各类缓存共占用 {:.1} GB，可安全释放约 {:.1} GB（安全项＝纯缓存，删后自动重下）。",
                managed as f64 / GB as f64,
                safe as f64 / GB as f64
            ),
            page: "cleanup".into(),
            action: "清理安全项".into(),
        });
    }
    if history_total > 0 {
        out.push(CheckItem {
            id: "jetbrains_history".into(),
            sev: "info".into(),
            title: "JetBrains IDE 历史版本可清理".into(),
            desc: format!(
                "检测到旧版 JetBrains IDE 数据目录，占用约 {:.1} GB。清理时会保留同产品最新版本。",
                history_total as f64 / GB as f64
            ),
            page: "cleanup".into(),
            action: "去清理".into(),
        });
    }
    if temp_total > 0 {
        out.push(CheckItem {
            id: "windows_temp_high".into(),
            sev: "info".into(),
            title: "Windows 临时目录占用偏高".into(),
            desc: format!(
                "检测到临时目录占用约 {:.1} GB。可进入磁盘清理按需处理；正在被系统占用的文件会自动跳过。",
                temp_total as f64 / GB as f64
            ),
            page: "cleanup".into(),
            action: "去清理".into(),
        });
    }

    // 3) Java：命令 java（最新 PATH）与 JAVA_HOME 指向不同版本
    if let Some(jh) = crate::env::java_home_reg() {
        let cmd = crate::env::java_cmd();
        let home = crate::env::java_of_home(&jh);
        if let (Some((_, c)), Some((_, h))) = (cmd, home) {
            if c != h {
                out.push(CheckItem {
                    id: "java_split".into(),
                    sev: "mid".into(),
                    title: "Java 命令与 JAVA_HOME 指向不同版本".into(),
                    desc: format!("命令 java 是 JDK {c}，JAVA_HOME 是 JDK {h} → 命令行与 Maven / IDE 用了不同版本。"),
                    page: "java".into(),
                    action: "去对齐".into(),
                });
            }
        }
    }

    out
}
