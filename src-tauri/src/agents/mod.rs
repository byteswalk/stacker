mod activity;
pub mod commands;
mod detect;
mod health;
mod install;
mod process;
mod registry;

pub(crate) use activity::{desktop_agent_processes, scan_agent_activity, AgentProcess};
pub(crate) use process::command_for_path;

use crate::agents::{detect::*, install::*, process::*, registry::*};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::Emitter;

pub(crate) const VIBE_PROGRESS_EVENT: &str = "vibe-progress";

pub(crate) const VIBE_SCAN_WORKERS: usize = 4;

pub(crate) const VIBE_SCAN_CACHE_TTL: Duration = Duration::from_secs(15);

pub(crate) type VibeScanSnapshot = Option<(Instant, Vec<VibeTool>)>;

pub(crate) static VIBE_SCAN_CACHE: OnceLock<Mutex<VibeScanSnapshot>> = OnceLock::new();

#[derive(Serialize, Clone)]
pub struct VibeSurface {
    pub available: bool,
    pub label: String,
    pub kind: String,
    pub description: String,
    pub installed: bool,
    pub status: String, // installed | update | missing | broken | pending
    pub version: Option<String>,
    pub probe_error: Option<String>,
    pub latest: Option<String>,
    pub update_available: bool,
    pub path: Option<String>,
    pub command: Option<String>,
    pub install_method: Option<String>,
    pub install_method_label: Option<String>,
    pub install_url: String,
    pub docs_url: String,
    pub can_install: bool,
    pub install_unavailable_reason: Option<String>,
    pub can_update: bool,
    pub can_uninstall: bool,
    pub can_open: bool,
    /// healthy | broken | missing
    pub health: String,
    pub broken_reason: Option<String>,
    pub other_installs: Vec<health::InstallInfo>,
    /// The effective entry is broken while another install on PATH is healthy.
    pub can_repair: bool,
}

#[derive(Serialize, Clone)]
pub struct VibeTool {
    pub id: String,
    pub family_id: String,
    pub edition: String,
    pub edition_label: String,
    pub sort_order: u16,
    pub name: String,
    pub description: String,
    pub docs_url: String,
    pub icon: String,
    pub cli_id: Option<String>,
    pub cli_note: Option<String>,
    pub cli: VibeSurface,
    pub desktop: VibeSurface,
}

#[derive(Serialize, Clone)]
pub struct AgentEnvironmentSnapshot {
    pub variable_count: u32,
    pub fingerprint: String,
}

pub(crate) struct ManagedCli {
    pub id: String,
    pub agent_name: String,
    pub cli_name: String,
    pub path: PathBuf,
}

pub(crate) struct ManagedDesktop {
    pub id: String,
    pub agent_name: String,
    pub desktop_name: String,
    pub target: Option<String>,
}

pub(crate) fn managed_agent(id: &str) -> Result<(), String> {
    spec_by_id(id)
        .map(|_| ())
        .ok_or_else(|| "Unknown work agent.".to_string())
}

pub(crate) fn managed_cli(id: &str) -> Result<ManagedCli, String> {
    let spec = spec_by_id(id).ok_or_else(|| "Unknown work agent.".to_string())?;
    if spec.cli.command.is_empty() {
        return Err(format!("{} does not provide a supported CLI.", spec.name));
    }
    let path = resolve_command(spec.cli.candidates)
        .ok_or_else(|| format!("{} CLI is not installed or cannot be resolved.", spec.name))?;
    Ok(ManagedCli {
        id: spec.id.to_string(),
        agent_name: spec.name.to_string(),
        cli_name: spec.cli.name.to_string(),
        path,
    })
}

pub(crate) fn managed_desktop(id: &str) -> Result<ManagedDesktop, String> {
    let spec = spec_by_id(id).ok_or_else(|| "Unknown work agent.".to_string())?;
    let found = detect_desktop_app(&spec.desktop).ok_or_else(|| {
        format!(
            "{} desktop app is not installed or cannot be resolved.",
            spec.name
        )
    })?;
    let target = found
        .launch
        .or_else(|| found.path.map(|path| path.to_string_lossy().into_owned()));
    Ok(ManagedDesktop {
        id: spec.id.to_string(),
        agent_name: spec.name.to_string(),
        desktop_name: spec.desktop.name.to_string(),
        target,
    })
}

pub(crate) fn scan_vibe_tools(check_latest: bool) -> Vec<VibeTool> {
    let specs = tool_specs();
    if specs.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let results = Mutex::new(vec![None; specs.len()]);
    let worker_count = specs.len().min(VIBE_SCAN_WORKERS);
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(spec) = specs.get(index).cloned() else {
                    break;
                };
                let tool = vibe_tool_from_spec(spec, check_latest);
                if let Ok(mut rows) = results.lock() {
                    rows[index] = Some(tool);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect()
}

pub(crate) fn scan_vibe_tools_cached() -> Vec<VibeTool> {
    let cache = VIBE_SCAN_CACHE.get_or_init(|| Mutex::new(None));
    let Ok(mut guard) = cache.lock() else {
        return scan_vibe_tools(true);
    };
    if let Some((created_at, tools)) = guard.as_ref() {
        if created_at.elapsed() < VIBE_SCAN_CACHE_TTL {
            return tools.clone();
        }
    }
    let tools = scan_vibe_tools(true);
    *guard = Some((Instant::now(), tools.clone()));
    tools
}

pub(crate) fn invalidate_vibe_scan_cache() {
    let Some(cache) = VIBE_SCAN_CACHE.get() else {
        return;
    };
    if let Ok(mut guard) = cache.lock() {
        *guard = None;
    }
}

pub(crate) fn cache_vibe_tool(tool: &VibeTool) {
    let Some(cache) = VIBE_SCAN_CACHE.get() else {
        return;
    };
    let Ok(mut guard) = cache.lock() else {
        return;
    };
    let Some((created_at, tools)) = guard.as_mut() else {
        return;
    };
    if let Some(current) = tools.iter_mut().find(|current| current.id == tool.id) {
        *current = tool.clone();
    } else {
        tools.push(tool.clone());
    }
    *created_at = Instant::now();
}

pub(crate) fn scan_vibe_tool(id: &str, check_latest: bool) -> Option<VibeTool> {
    spec_by_id(id).map(|spec| vibe_tool_from_spec(spec, check_latest))
}

pub(crate) fn vibe_catalog_tool(spec: ToolSpec) -> VibeTool {
    let cli_available = !spec.cli.command.is_empty();
    let desktop_available = spec.desktop_available;
    VibeTool {
        id: spec.id.into(),
        family_id: spec.family.into(),
        edition: spec.edition.as_str().into(),
        edition_label: spec.edition_label.into(),
        sort_order: spec.sort,
        name: spec.name.into(),
        description: spec.description.into(),
        docs_url: spec.docs_url.into(),
        icon: spec.icon.into(),
        cli_id: spec.cli_id.map(Into::into),
        cli_note: spec.cli_note.map(Into::into),
        cli: pending_surface(
            spec.cli.name,
            "CLI",
            spec.cli.description,
            spec.cli.command,
            spec.cli.install_url,
            spec.cli.docs_url,
            cli_available,
        ),
        desktop: pending_surface(
            spec.desktop.name,
            "桌面端",
            spec.desktop.description,
            "",
            spec.desktop.install_url,
            spec.desktop.docs_url,
            desktop_available,
        ),
    }
}

pub(crate) fn pending_surface(
    label: &str,
    kind: &str,
    description: &str,
    command: &str,
    install_url: &str,
    docs_url: &str,
    available: bool,
) -> VibeSurface {
    VibeSurface {
        available,
        label: label.into(),
        kind: kind.into(),
        description: description.into(),
        installed: false,
        status: "pending".into(),
        version: None,
        probe_error: None,
        latest: None,
        update_available: false,
        path: None,
        command: (!command.is_empty()).then(|| command.into()),
        install_method: None,
        install_method_label: None,
        install_url: install_url.into(),
        docs_url: docs_url.into(),
        can_install: false,
        install_unavailable_reason: None,
        can_update: false,
        can_uninstall: false,
        can_open: false,
        health: "missing".into(),
        broken_reason: None,
        other_installs: Vec::new(),
        can_repair: false,
    }
}

pub(crate) fn vibe_tool_from_spec(spec: ToolSpec, check_latest: bool) -> VibeTool {
    VibeTool {
        id: spec.id.into(),
        family_id: spec.family.into(),
        edition: spec.edition.as_str().into(),
        edition_label: spec.edition_label.into(),
        sort_order: spec.sort,
        name: spec.name.into(),
        description: spec.description.into(),
        docs_url: spec.docs_url.into(),
        icon: spec.icon.into(),
        cli_id: spec.cli_id.map(Into::into),
        cli_note: spec.cli_note.map(Into::into),
        cli: cli_surface(&spec, check_latest),
        desktop: desktop_surface(&spec, check_latest),
    }
}

pub(crate) fn unavailable_surface(
    label: &str,
    kind: &str,
    description: &str,
    install_url: &str,
    docs_url: &str,
) -> VibeSurface {
    VibeSurface {
        available: false,
        label: label.into(),
        kind: kind.into(),
        description: description.into(),
        installed: false,
        status: "missing".into(),
        version: None,
        probe_error: None,
        latest: None,
        update_available: false,
        path: None,
        command: None,
        install_method: None,
        install_method_label: None,
        install_url: install_url.into(),
        docs_url: docs_url.into(),
        can_install: false,
        install_unavailable_reason: Some("官方未提供可自动安装的独立 Windows 应用。".into()),
        can_update: false,
        can_uninstall: false,
        can_open: false,
        health: "missing".into(),
        broken_reason: None,
        other_installs: Vec::new(),
        can_repair: false,
    }
}

pub(crate) fn run_tool_action(
    id: &str,
    target: &str,
    action: &str,
    window: Option<tauri::Window>,
) -> Result<String, String> {
    crate::installer::op_reset();
    let spec = spec_by_id(id).ok_or_else(|| "未知的工作智能体工具".to_string())?;
    log::info!("work agent action started: id={id} target={target} action={action}");
    let res = match (target, action) {
        ("cli", "install") => install_cli_tool(&spec, &window),
        ("cli", "update") => update_cli_tool(&spec, &window),
        ("cli", "uninstall") => uninstall_cli_tool(&spec, &window),
        ("desktop", "install") => install_desktop_tool(&spec, &window),
        ("desktop", "update") => update_desktop_tool(&spec, &window),
        ("desktop", "uninstall") => uninstall_desktop_tool(&spec, &window),
        _ => Err("不支持的操作".into()),
    };
    match &res {
        Ok(message) => log::info!(
            "work agent action completed: id={id} target={target} action={action} result={message}"
        ),
        Err(error) => log::error!(
            "work agent action failed: id={id} target={target} action={action} error={error}"
        ),
    }
    emit_progress(&window, "__done__");
    res
}

pub(crate) fn open_desktop_tool(id: &str) -> Result<(), String> {
    let spec = spec_by_id(id).ok_or_else(|| "未知的工作智能体工具".to_string())?;
    if spec.vendor == Vendor::DeepSeekHarness {
        return open_deepseek_harness_workbench();
    }
    if let Some(found) = detect_desktop_app(&spec.desktop) {
        if let Some(launch) = found.launch {
            return open_external_target(&launch);
        }
        if let Some(path) = found.path {
            return open_external_target(&path.to_string_lossy());
        }
    }
    if spec.vendor == Vendor::Codex {
        if let Some(program) = resolve_command(spec.cli.candidates) {
            let mut cmd = command_for_path(&program, &["app"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            apply_fresh_path(&mut cmd);
            cmd.spawn()
                .map_err(|e| format!("启动 Codex app 失败：{e}"))?;
            return Ok(());
        }
    }
    Err(format!(
        "未检测到 {}。请先安装桌面端，或查看官方文档。",
        spec.desktop.name
    ))
}

pub(crate) fn open_deepseek_harness_workbench() -> Result<(), String> {
    let launcher = deepseek_harness_launcher().ok_or_else(|| {
        "未检测到可用的 DeepSeek Harness 本地工作台。请确认启动脚本及其项目目录仍存在。".to_string()
    })?;
    let program = resolve_command_including_windowsapps(&["powershell.exe", "powershell.cmd"])
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    let mut command = Command::new(program);
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        &launcher.to_string_lossy(),
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|e| format!("启动 DeepSeek Harness 本地工作台失败：{e}"))?;
    Ok(())
}

pub(crate) fn build_environment_prompt() -> Result<String, String> {
    let tools = scan_vibe_tools(false);
    let mut out = String::new();
    out.push_str("## 已安装的工作智能体\n\n");
    let mut count = 0;
    for tool in tools {
        let surfaces = [(&tool.cli, "CLI"), (&tool.desktop, "桌面端")]
            .into_iter()
            .filter(|(surface, _)| surface.installed || surface.path.is_some())
            .collect::<Vec<_>>();
        if surfaces.is_empty() {
            continue;
        }
        count += 1;
        out.push_str(&format!("- {}\n", tool.name));
        for (surface, kind) in surfaces {
            let mut details = Vec::new();
            if let Some(version) = surface.version.as_deref() {
                details.push(format!("版本：{version}"));
            }
            if let Some(command) = surface.command.as_deref().filter(|value| !value.is_empty()) {
                details.push(format!("命令：{command}"));
            }
            if let Some(path) = surface.path.as_deref() {
                details.push(format!("路径：{path}"));
            }
            if let Some(method) = surface.install_method_label.as_deref() {
                details.push(format!("安装方式：{method}"));
            }
            if details.is_empty() {
                details.push("已安装".into());
            }
            out.push_str(&format!("  - {kind}：{}\n", details.join("；")));
        }
    }
    if count == 0 {
        out.push_str("当前未检测到已安装的工作智能体。\n");
    }
    Ok(out)
}

pub(crate) fn open_external_target(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = OsStr::new(target).encode_wide().chain(Some(0)).collect();
        let rc = unsafe {
            winapi::um::shellapi::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                winapi::um::winuser::SW_SHOWNORMAL,
            )
        };
        if (rc as isize) <= 32 {
            Err(format!("打开失败：ShellExecuteW 返回 {}", rc as isize))
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        Command::new(opener)
            .arg(target)
            .spawn()
            .map_err(|e| format!("打开失败：{e}"))?;
        Ok(())
    }
}

pub(crate) fn emit_progress<S: AsRef<str>>(window: &Option<tauri::Window>, msg: S) {
    if let Some(window) = window {
        let _ = window.emit(VIBE_PROGRESS_EVENT, msg.as_ref().to_string());
    }
}
