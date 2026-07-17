//! 体检引擎：聚合"换源之外"的可优化项检测（代理端口是否在监听、开发缓存是否偏高）。
//! 换源类的可优化项前端已用 list_sources 算出；这里补需要后端能力的检测。

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use std::time::Instant;

use encoding_rs::GBK;
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

#[derive(Serialize, Clone)]
pub struct AgentCheck {
    pub id: String,
    pub name: String,
    pub category: String,
    pub required: bool,
    pub status: String, // ok | warn | missing
    pub version: Option<String>,
    pub path: Option<String>,
    pub message: String,
    pub page: Option<String>,
    pub action: Option<String>,
}

#[derive(Serialize, Default)]
pub struct AgentReadiness {
    pub status: String, // ready | partial | blocked
    pub score: u8,
    pub title: String,
    pub summary: String,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub checks: Vec<AgentCheck>,
}

#[derive(Serialize, Clone)]
pub struct EcosystemSnapshot {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub status: String, // ok | warn | missing
    pub summary: String,
    pub detail: String,
}

#[derive(Serialize)]
pub struct CodingEcosystemCheck {
    pub ready: bool,
    pub title: String,
    pub summary: String,
    pub ecosystems: Vec<EcosystemSnapshot>,
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

fn command_output_timeout_named(
    mut c: Command,
    name: &str,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 {name} 命令失败：{e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{name} 命令响应超时"));
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

fn command_dirs() -> Vec<PathBuf> {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    dirs
}

fn resolve_command(candidates: &[&str]) -> Option<PathBuf> {
    for dir in command_dirs() {
        if windowsapps_dir(&dir) {
            continue;
        }
        for name in candidates {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn command_for_path(program: &Path, args: &[&str]) -> Command {
    let ext = program
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if matches!(ext.as_str(), "bat" | "cmd") {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/d", "/c", "call"]).arg(program);
        cmd.args(args);
        cmd
    } else if ext == "ps1" {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(program);
        cmd.args(args);
        cmd
    } else {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    }
}

fn decode_command_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (text, _, _) = GBK.decode(bytes);
            text.into_owned()
        }
    }
}

fn output_text(out: &Output) -> String {
    let bytes = if out.stdout.is_empty() {
        &out.stderr
    } else {
        &out.stdout
    };
    decode_command_bytes(bytes).trim().to_string()
}

fn first_output_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| is_meaningful_output_line(line))
        .map(|line| {
            let mut out = line.to_string();
            if out.chars().count() > 180 {
                out = out.chars().take(180).collect::<String>() + "...";
            }
            out
        })
}

fn is_meaningful_output_line(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let stripped =
        line.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '=' | '_' | '*'));
    !stripped.is_empty()
}

fn run_program_probe(
    display_name: &str,
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let mut cmd = command_for_path(program, args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    apply_fresh_command_env(&mut cmd);
    let out = command_output_timeout_named(cmd, display_name, timeout)?;
    let version_text = output_text(&out);
    if !out.status.success() {
        return Err(first_output_line(&version_text).unwrap_or_else(|| "命令返回失败状态".into()));
    }
    Ok(first_output_line(&version_text).unwrap_or_else(|| "可用".into()))
}

fn run_command_probe(
    display_name: &str,
    candidates: &[&str],
    args: &[&str],
) -> Result<(String, String), String> {
    let program = resolve_command(candidates).ok_or_else(|| "未找到命令".to_string())?;
    let version = run_program_probe(display_name, &program, args, Duration::from_secs(5))?;
    Ok((version, program.to_string_lossy().into_owned()))
}

#[allow(clippy::too_many_arguments)]
fn agent_check(
    id: &str,
    name: &str,
    category: &str,
    required: bool,
    candidates: &[&str],
    args: &[&str],
    page: Option<&str>,
    action: Option<&str>,
) -> AgentCheck {
    match run_command_probe(name, candidates, args) {
        Ok((version, path)) => AgentCheck {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            required,
            status: "ok".into(),
            version: Some(version),
            path: Some(path),
            message: "命令可用".into(),
            page: page.map(str::to_string),
            action: action.map(str::to_string),
        },
        Err(err) => AgentCheck {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            required,
            status: if required { "missing" } else { "warn" }.into(),
            version: None,
            path: None,
            message: if err == "未找到命令" {
                "未检测到命令".into()
            } else {
                format!("命令不可用：{err}")
            },
            page: page.map(str::to_string),
            action: action.map(str::to_string),
        },
    }
}

fn pyenv_default_python() -> Option<(String, PathBuf)> {
    let pyenv = crate::pyenv::pyenv_status_snapshot();
    if !pyenv.installed {
        return None;
    }
    let default = pyenv.default.as_deref()?.trim();
    if default.is_empty() || default.eq_ignore_ascii_case("system") {
        return None;
    }
    if !pyenv.versions.iter().any(|v| v.version.as_str() == default) {
        return None;
    }
    let python = crate::pyenv::pyenv_python_exe(default)?;
    Some((default.to_string(), python))
}

fn python_agent_check(pyenv_python: Option<&(String, PathBuf)>) -> AgentCheck {
    if let Some((version, path)) = pyenv_python {
        let integrated = crate::pyenv::pyenv_integration_ready();
        return AgentCheck {
            id: "python".into(),
            name: "Python".into(),
            category: "运行时".into(),
            required: true,
            status: if integrated { "ok" } else { "missing" }.into(),
            version: Some(format!("Python {version}")),
            path: Some(path.to_string_lossy().into_owned()),
            message: if integrated {
                "默认 Python 版本和 python.exe 命令入口一致"
            } else {
                "默认 Python 已安装，但 python.exe 命令入口指向其他版本"
            }
            .into(),
            page: Some("python".into()),
            action: Some("配置 Python".into()),
        };
    }

    match python_command_candidate() {
        Some(path) => match run_python_version(&path) {
            Ok(version) => AgentCheck {
                id: "python".into(),
                name: "Python".into(),
                category: "运行时".into(),
                required: true,
                status: "ok".into(),
                version: Some(format!("Python {version}")),
                path: Some(path.to_string_lossy().into_owned()),
                message: "Python 命令可用".into(),
                page: Some("python".into()),
                action: Some("配置 Python".into()),
            },
            Err(err) => AgentCheck {
                id: "python".into(),
                name: "Python".into(),
                category: "运行时".into(),
                required: true,
                status: "missing".into(),
                version: None,
                path: Some(path.to_string_lossy().into_owned()),
                message: format!("Python 命令不可用：{err}"),
                page: Some("python".into()),
                action: Some("修复 Python".into()),
            },
        },
        None => AgentCheck {
            id: "python".into(),
            name: "Python".into(),
            category: "运行时".into(),
            required: true,
            status: "missing".into(),
            version: None,
            path: None,
            message: "未检测到 python 命令".into(),
            page: Some("python".into()),
            action: Some("安装 Python".into()),
        },
    }
}

fn pip_agent_check(pyenv_python: Option<&(String, PathBuf)>) -> AgentCheck {
    if let Some((_, python)) = pyenv_python {
        match run_program_probe(
            "pip",
            python,
            &["-m", "pip", "--version"],
            Duration::from_secs(8),
        ) {
            Ok(version) => {
                return AgentCheck {
                    id: "pip".into(),
                    name: "pip".into(),
                    category: "包管理器".into(),
                    required: true,
                    status: "ok".into(),
                    version: Some(version),
                    path: Some(format!("{} -m pip", python.to_string_lossy())),
                    message: "默认 Python 的 pip 可用".into(),
                    page: Some("python".into()),
                    action: Some("配置 pip".into()),
                };
            }
            Err(pyenv_err) => {
                let fallback = agent_check(
                    "pip",
                    "pip",
                    "包管理器",
                    true,
                    &["pip.exe", "pip.cmd", "pip.bat"],
                    &["--version"],
                    Some("python"),
                    Some("配置 pip"),
                );
                if fallback.status == "ok" {
                    return fallback;
                }
                return AgentCheck {
                    id: "pip".into(),
                    name: "pip".into(),
                    category: "包管理器".into(),
                    required: true,
                    status: "missing".into(),
                    version: None,
                    path: Some(format!("{} -m pip", python.to_string_lossy())),
                    message: format!("默认 Python 未检测到 pip：{pyenv_err}"),
                    page: Some("python".into()),
                    action: Some("配置 pip".into()),
                };
            }
        }
    }

    agent_check(
        "pip",
        "pip",
        "包管理器",
        true,
        &["pip.exe", "pip.cmd", "pip.bat"],
        &["--version"],
        Some("python"),
        Some("配置 pip"),
    )
}

fn maven_agent_check() -> AgentCheck {
    let cleaned = crate::sources::clear_maven_legacy_proxy_opts().unwrap_or(false);
    let mut check = agent_check(
        "maven",
        "Maven",
        "构建工具",
        false,
        &["mvn.cmd", "mvn.exe", "mvn.bat"],
        &["--version"],
        Some("maven"),
        Some("配置 Maven"),
    );
    if cleaned {
        if check.status == "ok" {
            check.message = "Maven 命令可用；已清理旧版代理环境变量".into();
        } else {
            check.message = format!("已清理旧版代理环境变量；{}", check.message);
        }
    }
    check
}

fn run_python_version(program: &Path) -> Result<String, String> {
    let mut c = command_for_path(program, &["--version"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    apply_fresh_command_env(&mut c);
    let out = command_output_timeout(c, Duration::from_secs(5))?;
    if !out.status.success() {
        return Err("命令返回失败状态".into());
    }
    let text = output_text(&out);
    for line in text.lines() {
        let line = line.trim();
        if let Some(version) = line.strip_prefix("Python ") {
            return Ok(version.trim().to_string());
        }
    }
    Err("无法识别 Python 版本输出".into())
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
        if !default.is_empty() && default != "system" && !ready.contains(&default) {
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
        let managed_default = !default.is_empty() && default != "system";
        let ready = if managed_default {
            default_exe_ok && integration_ok
        } else {
            command_ok
        };
        if !ready {
            let desc = if default == "system" || default.is_empty() {
                "当前没有可用的 python 命令。请在 Python 页选择一个已安装版本设为默认，或安装新版本。".to_string()
            } else if default_exe_ok && !integration_ok {
                format!("当前默认版本为 {default}，但 python.exe 仍指向其他 Python。请在 Python 页重新应用默认版本，或点击“更新集成”。")
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
pub async fn checkup_extra() -> Result<Vec<CheckItem>, String> {
    // 放后台线程：内含网络探测 + 磁盘缓存扫描，避免堵主线程让界面卡死。
    tauri::async_runtime::spawn_blocking(checkup_impl)
        .await
        .map_err(|error| format!("开发环境体检任务异常结束：{error}"))
}

#[tauri::command]
pub async fn checkup_page(page: String) -> Result<Vec<CheckItem>, String> {
    tauri::async_runtime::spawn_blocking(move || checkup_page_impl(&page))
        .await
        .map_err(|error| format!("环境状态检查任务异常结束：{error}"))
}

fn checkup_page_impl(page: &str) -> Vec<CheckItem> {
    let mut out = Vec::new();
    match page {
        "python" => {
            if let Some(item) = python_default_check() {
                out.push(item);
            }
        }
        "node" => {
            let status = crate::fnm::fnm_status_impl();
            if let Some(item) = node_default_check(&status) {
                out.push(item);
            }
            if status.installed && !status.shell.powershell && !status.shell.gitbash {
                out.push(CheckItem {
                    id: "fnm_no_integration".into(),
                    sev: "warn".into(),
                    title: "fnm 已装，但未写 shell 集成".into(),
                    desc: "fnm 尚未完成终端集成，切换 Node 版本后不会自动生效。可一键写入 PowerShell、Git Bash 和 cmd 配置，改动前会自动备份。".into(),
                    page: "node".into(),
                    action: "写入集成".into(),
                });
            }
        }
        "proxy" => {
            let status = crate::proxy::status();
            if status.enabled && !port_listening(&status.host, status.port) {
                out.push(CheckItem {
                    id: "proxy_stale".into(),
                    sev: "warn".into(),
                    title: "终端代理已配置，但端口未在监听".into(),
                    desc: format!("终端代理指向 {}:{}，但该端口当前没有程序监听。请启动代理软件，或暂时关闭终端代理。", status.host, status.port),
                    page: "proxy".into(),
                    action: "关闭代理".into(),
                });
            }
        }
        "java" => {
            if let Some(java_home) = crate::env::java_home_reg() {
                let command = crate::env::java_cmd();
                let home = crate::env::java_of_home(&java_home);
                if let (Some((_, command_version)), Some((_, home_version))) = (command, home) {
                    if command_version != home_version {
                        out.push(CheckItem {
                            id: "java_split".into(),
                            sev: "mid".into(),
                            title: "Java 命令与 JAVA_HOME 指向不同版本".into(),
                            desc: format!("java 命令使用 JDK {command_version}，JAVA_HOME 指向 JDK {home_version}。命令行、Maven 与 IDE 可能因此使用不同版本。"),
                            page: "java".into(),
                            action: "去对齐".into(),
                        });
                    }
                }
            }
        }
        "cleanup" => out.extend(cleanup_check_items()),
        _ => {}
    }
    out
}

#[tauri::command]
pub async fn coding_ecosystem_check() -> Result<CodingEcosystemCheck, String> {
    tauri::async_runtime::spawn_blocking(coding_ecosystem_check_impl)
        .await
        .map_err(|error| format!("生态环境体检任务异常结束：{error}"))
}

fn coding_ecosystem_check_impl() -> CodingEcosystemCheck {
    let readiness = agent_readiness_impl();
    let pyenv = crate::pyenv::pyenv_status_snapshot();
    let fnm = crate::fnm::fnm_status_impl();
    let rustup = crate::rustup::rustup_status_snapshot();
    let git = crate::git::status_snapshot();

    let command = |id: &str| readiness.checks.iter().find(|check| check.id == id);
    let command_row = |id: &str, label: &str, kind: &str| {
        let check = command(id);
        let available = check.is_some_and(|item| item.status == "ok");
        EcosystemSnapshot {
            id: id.to_string(),
            label: label.to_string(),
            kind: kind.to_string(),
            status: if available { "ok" } else { "missing" }.to_string(),
            summary: check
                .and_then(|item| item.version.clone())
                .unwrap_or_else(|| "未检测到可用版本".to_string()),
            detail: if available {
                "命令可用".to_string()
            } else {
                check
                    .map(|item| item.message.clone())
                    .unwrap_or_else(|| "未检测到命令".to_string())
            },
        }
    };

    let py_default = pyenv
        .default
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let py_default_valid = py_default.map_or(true, |value| {
        value.eq_ignore_ascii_case("system")
            || pyenv
                .versions
                .iter()
                .any(|version| version.version == value)
    });
    let python_command_ok = command("python").is_some_and(|item| item.status == "ok");
    let conda = run_command_probe(
        "Conda",
        &["conda.exe", "conda.cmd", "conda.bat"],
        &["--version"],
    )
    .ok()
    .map(|(version, _)| version);
    let python_summary = match (
        pyenv.pyenv_version.as_deref(),
        py_default,
        conda.as_deref(),
        command("python").and_then(|item| item.version.as_deref()),
    ) {
        (Some(manager), Some(runtime), _, _) => format!("{manager} · Python {runtime}"),
        (Some(manager), None, _, Some(runtime)) => format!("{manager} · {runtime}"),
        (Some(manager), None, _, None) => format!("{manager} · 未设置默认 Python"),
        (None, _, Some(conda_version), _) => conda_version.to_string(),
        (None, _, None, Some(runtime)) => runtime.to_string(),
        _ => "未检测到 Python 运行时".to_string(),
    };
    let python_status = if !py_default_valid {
        "warn"
    } else if python_command_ok || conda.is_some() {
        "ok"
    } else {
        "missing"
    };

    let node_default = fnm
        .default
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let node_default_valid = node_default.map_or(true, |value| {
        fnm.versions
            .iter()
            .any(|version| version.version.trim_start_matches('v') == value.trim_start_matches('v'))
    });
    let node_command_ok = command("node").is_some_and(|item| item.status == "ok");
    let node_summary = match (
        fnm.fnm_version.as_deref(),
        node_default,
        command("node").and_then(|item| item.version.as_deref()),
    ) {
        (Some(manager), Some(runtime), _) => format!("{manager} · Node {runtime}"),
        (Some(manager), None, Some(runtime)) => format!("{manager} · {runtime}"),
        (Some(manager), None, None) => format!("{manager} · 未设置默认 Node"),
        (None, _, Some(runtime)) => runtime.to_string(),
        _ => "未检测到 Node.js 运行时".to_string(),
    };
    let node_status = if !node_default_valid {
        "warn"
    } else if node_command_ok {
        "ok"
    } else {
        "missing"
    };

    let git_ok = git.installed;
    let rust_summary = match (
        rustup.rustup_version.as_deref(),
        rustup.default_version.as_deref(),
    ) {
        (Some(manager), Some(runtime)) => format!("{manager} · Rust {runtime}"),
        (Some(manager), None) => format!("{manager} · 未设置默认工具链"),
        (None, Some(runtime)) => format!("Rust {runtime}"),
        _ => "未检测到 Rust 工具链".to_string(),
    };
    let rust_status = if rustup.probe_error.is_some() {
        "warn"
    } else if rustup.installed && rustup.default.is_some() {
        "ok"
    } else {
        "missing"
    };

    let mut ecosystems = vec![
        EcosystemSnapshot {
            id: "git".into(),
            label: "Git".into(),
            kind: "基础工具".into(),
            status: if git_ok { "ok" } else { "missing" }.into(),
            summary: git.version.unwrap_or_else(|| "未检测到 Git".into()),
            detail: if git_ok {
                "Git 命令可用".into()
            } else {
                "未检测到 Git for Windows".into()
            },
        },
        EcosystemSnapshot {
            id: "python".into(),
            label: "Python".into(),
            kind: "运行时".into(),
            status: python_status.into(),
            summary: python_summary,
            detail: if !py_default_valid {
                "默认 Python 指向已不存在的版本".into()
            } else if python_command_ok || conda.is_some() {
                "Python 环境可用".into()
            } else {
                "未检测到可用的 Python 或 Conda".into()
            },
        },
        EcosystemSnapshot {
            id: "node".into(),
            label: "Node.js".into(),
            kind: "运行时".into(),
            status: node_status.into(),
            summary: node_summary,
            detail: if !node_default_valid {
                "默认 Node 指向已不存在的版本".into()
            } else if node_command_ok {
                "Node.js 命令可用".into()
            } else {
                "未检测到可用的 Node.js".into()
            },
        },
        command_row("java", "Java", "运行时"),
        command_row("maven", "Maven", "构建工具"),
        command_row("gradle", "Gradle", "构建工具"),
        command_row("go", "Go", "运行时"),
        EcosystemSnapshot {
            id: "rust".into(),
            label: "Rust".into(),
            kind: "工具链".into(),
            status: rust_status.into(),
            summary: rust_summary,
            detail: rustup.probe_error.unwrap_or_else(|| {
                if rust_status == "ok" {
                    "默认 Rust 工具链可用".into()
                } else {
                    "未检测到默认 Rust 工具链".into()
                }
            }),
        },
    ];
    ecosystems.sort_by_key(|item| match item.id.as_str() {
        "git" => 0,
        "python" => 1,
        "node" => 2,
        "java" => 3,
        "maven" => 4,
        "gradle" => 5,
        "go" => 6,
        "rust" => 7,
        _ => 99,
    });

    let ready = git_ok
        && (python_command_ok || node_command_ok || conda.is_some())
        && py_default_valid
        && node_default_valid;
    let issue_count = ecosystems
        .iter()
        .filter(|item| item.status == "warn")
        .count();
    CodingEcosystemCheck {
        ready,
        title: if ready {
            "Coding Ready".into()
        } else {
            "需要处理".into()
        },
        summary: if ready {
            if issue_count == 0 {
                "基础开发环境已就绪".into()
            } else {
                format!("基础开发环境已就绪，另有 {issue_count} 项配置需要关注")
            }
        } else {
            "请先确保 Git 可用，并至少配置一个可用的 Python 或 Node.js 运行时".into()
        },
        ecosystems,
    }
}

fn agent_readiness_impl() -> AgentReadiness {
    let pyenv_python = pyenv_default_python();
    let mut checks = std::thread::scope(|scope| {
        let python_path = pyenv_python.clone();
        let pip_path = pyenv_python.clone();
        let handles = vec![
            scope.spawn(|| {
                agent_check(
                    "git",
                    "Git",
                    "基础工具",
                    true,
                    &["git.exe", "git.cmd", "git.bat"],
                    &["--version"],
                    Some("git"),
                    Some("配置 Git"),
                )
            }),
            scope.spawn(|| {
                agent_check(
                    "node",
                    "Node.js",
                    "运行时",
                    true,
                    &["node.exe", "node.cmd", "node.bat"],
                    &["--version"],
                    Some("node"),
                    Some("配置 Node"),
                )
            }),
            scope.spawn(|| {
                agent_check(
                    "npm",
                    "npm",
                    "包管理器",
                    true,
                    &["npm.cmd", "npm.exe", "npm.bat"],
                    &["--version"],
                    Some("node"),
                    Some("配置 npm"),
                )
            }),
            scope.spawn(move || python_agent_check(python_path.as_ref())),
            scope.spawn(move || pip_agent_check(pip_path.as_ref())),
            scope.spawn(|| {
                agent_check(
                    "java",
                    "Java",
                    "运行时",
                    false,
                    &["java.exe", "java.cmd", "java.bat"],
                    &["-version"],
                    Some("java"),
                    Some("配置 JDK"),
                )
            }),
            scope.spawn(maven_agent_check),
            scope.spawn(|| {
                agent_check(
                    "gradle",
                    "Gradle",
                    "构建工具",
                    false,
                    &["gradle.exe", "gradle.cmd", "gradle.bat"],
                    &["--version"],
                    Some("gradle"),
                    Some("配置 Gradle"),
                )
            }),
            scope.spawn(|| {
                agent_check(
                    "go",
                    "Go",
                    "运行时",
                    false,
                    &["go.exe", "go.cmd", "go.bat"],
                    &["version"],
                    Some("go"),
                    Some("配置 Go"),
                )
            }),
            scope.spawn(|| {
                agent_check(
                    "cargo",
                    "Cargo",
                    "构建工具",
                    false,
                    &["cargo.exe", "cargo.cmd", "cargo.bat"],
                    &["--version"],
                    Some("rust"),
                    Some("配置 Rust"),
                )
            }),
        ];
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect::<Vec<_>>()
    });

    let agent_cli_checks = std::thread::scope(|scope| {
        let handles = vec![
            scope.spawn(|| {
                agent_check(
                    "claude",
                    "Claude Code CLI",
                    "工作智能体 CLI",
                    false,
                    &["claude.exe", "claude.cmd", "claude.bat", "claude.ps1"],
                    &["--version"],
                    Some("vibe"),
                    Some("查看工具"),
                )
            }),
            scope.spawn(|| {
                agent_check(
                    "codex",
                    "Codex CLI",
                    "工作智能体 CLI",
                    false,
                    &["codex.exe", "codex.cmd", "codex.bat", "codex.ps1"],
                    &["--version"],
                    Some("vibe"),
                    Some("查看工具"),
                )
            }),
            scope.spawn(|| {
                agent_check(
                    "antigravity",
                    "Antigravity CLI",
                    "工作智能体 CLI",
                    false,
                    &["agy.exe", "agy.cmd", "agy.bat", "agy.ps1"],
                    &["--version"],
                    Some("vibe"),
                    Some("查看工具"),
                )
            }),
            scope.spawn(|| {
                agent_check(
                    "opencode",
                    "OpenCode CLI",
                    "工作智能体 CLI",
                    false,
                    &[
                        "opencode.exe",
                        "opencode.cmd",
                        "opencode.bat",
                        "opencode.ps1",
                    ],
                    &["--version"],
                    Some("vibe"),
                    Some("查看工具"),
                )
            }),
        ];
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect::<Vec<_>>()
    });
    let installed_agent_cli: Vec<_> = agent_cli_checks
        .into_iter()
        .filter(|c| c.status == "ok")
        .collect();
    if installed_agent_cli.is_empty() {
        checks.push(AgentCheck {
            id: "agent_cli".into(),
            name: "工作智能体 CLI".into(),
            category: "工作智能体 CLI".into(),
            required: false,
            status: "warn".into(),
            version: None,
            path: None,
            message: "未检测到 Claude Code CLI、Codex CLI、Antigravity CLI 或 OpenCode CLI。"
                .into(),
            page: Some("vibe".into()),
            action: Some("查看工具".into()),
        });
    } else {
        checks.extend(installed_agent_cli);
    }

    let blockers: Vec<String> = checks
        .iter()
        .filter(|c| c.required && c.status != "ok")
        .map(|c| c.name.clone())
        .collect();
    let warnings: Vec<String> = checks
        .iter()
        .filter(|c| !c.required && c.status != "ok")
        .map(|c| c.name.clone())
        .collect();
    let required_total = checks.iter().filter(|c| c.required).count();
    let required_ok = checks
        .iter()
        .filter(|c| c.required && c.status == "ok")
        .count();
    let optional_total = checks.iter().filter(|c| !c.required).count();
    let optional_ok = checks
        .iter()
        .filter(|c| !c.required && c.status == "ok")
        .count();
    let score = if checks.is_empty() {
        0
    } else if required_total == 0 {
        ((optional_ok * 100) / optional_total.max(1)) as u8
    } else {
        let required_score = (required_ok * 70).checked_div(required_total).unwrap_or(0);
        let optional_score = (optional_ok * 30).checked_div(optional_total).unwrap_or(30);
        (required_score + optional_score).min(100) as u8
    };
    let status = if !blockers.is_empty() {
        "blocked"
    } else if !warnings.is_empty() {
        "partial"
    } else {
        "ready"
    };
    let title = match status {
        "ready" => "Coding Ready",
        "partial" => "基本就绪",
        _ => "需要处理",
    };
    let summary = if !blockers.is_empty() {
        format!(
            "缺少 {}。可能影响依赖安装、测试运行或项目构建。",
            blockers.join("、")
        )
    } else if !warnings.is_empty() {
        format!(
            "核心开发命令可用；{} 属于项目相关或可选工具，可按需补齐。",
            warnings.join("、")
        )
    } else {
        "核心运行时、包管理器和常用构建工具可用，适合运行本地工作智能体和常规开发任务。".into()
    };

    AgentReadiness {
        status: status.into(),
        score,
        title: title.into(),
        summary,
        blockers,
        warnings,
        checks,
    }
}

fn cleanup_check_items() -> Vec<CheckItem> {
    let caches = crate::cleanup::cleanup_scan();
    let managed: u64 = caches
        .iter()
        .filter(|item| item.category == "safe" || item.category == "cautious")
        .map(|item| item.size)
        .sum();
    let safe: u64 = caches
        .iter()
        .filter(|item| item.category == "safe")
        .map(|item| item.size)
        .sum();
    let history_total: u64 = caches
        .iter()
        .filter(|item| item.category == "history")
        .map(|item| item.size)
        .sum();
    let temp_total: u64 = caches
        .iter()
        .filter(|item| item.category == "temp")
        .map(|item| item.size)
        .sum();
    let mut out = Vec::new();
    if managed > 5 * GB {
        let can_clean_directly = safe >= 256 * 1024 * 1024;
        out.push(CheckItem {
            id: if can_clean_directly { "cache_safe_high".into() } else { "cache_high".into() },
            sev: "info".into(),
            title: "开发缓存占用偏高".into(),
            desc: format!(
                "开发工具缓存共占用 {:.1} GB，其中可直接清理的缓存约 {:.1} GB。其余项目可在磁盘清理页按需处理。",
                managed as f64 / GB as f64,
                safe as f64 / GB as f64
            ),
            page: "cleanup".into(),
            action: if can_clean_directly { "清理安全项".into() } else { "查看详情".into() },
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
    out
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
            desc: format!("终端代理指向 {}:{}，但该端口当前没有程序监听。请启动代理软件，或暂时关闭终端代理。", s.host, s.port),
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
            desc: "fnm 尚未完成终端集成，切换 Node 版本后不会自动生效。可一键写入 PowerShell、Git Bash 和 cmd 配置，改动前会自动备份。".into(),
            page: "node".into(),
            action: "写入集成".into(),
        });
    }

    // 2) 开发缓存偏高 → 建议清理
    out.extend(cleanup_check_items());

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
                    desc: format!("java 命令使用 JDK {c}，JAVA_HOME 指向 JDK {h}。命令行、Maven 与 IDE 可能因此使用不同版本。"),
                    page: "java".into(),
                    action: "去对齐".into(),
                });
            }
        }
    }

    out
}
