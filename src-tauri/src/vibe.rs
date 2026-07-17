use encoding_rs::GBK;
use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tauri::Emitter;

const VIBE_PROGRESS_EVENT: &str = "vibe-progress";

#[derive(Serialize, Clone)]
pub struct VibeSurface {
    pub available: bool,
    pub label: String,
    pub kind: String,
    pub description: String,
    pub installed: bool,
    pub status: String, // installed | update | missing | unknown
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
}

#[derive(Serialize, Clone)]
pub struct VibeTool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub docs_url: String,
    pub cli: VibeSurface,
    pub desktop: VibeSurface,
}

#[derive(Clone)]
struct CliSpec {
    name: &'static str,
    description: &'static str,
    command: &'static str,
    candidates: &'static [&'static str],
    npm_package: Option<&'static str>,
    winget_id: Option<&'static str>,
    install_url: &'static str,
    docs_url: &'static str,
}

#[derive(Clone)]
struct DesktopSpec {
    name: &'static str,
    description: &'static str,
    winget_id: Option<&'static str>,
    winget_source: Option<&'static str>,
    appx_names: &'static [&'static str],
    install_url: &'static str,
    docs_url: &'static str,
    keywords: &'static [&'static str],
    excludes: &'static [&'static str],
}

#[derive(Clone)]
struct ToolSpec {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    docs_url: &'static str,
    cli: CliSpec,
    desktop: DesktopSpec,
}

struct DesktopFound {
    path: Option<PathBuf>,
    version: Option<String>,
    method: Option<String>,
    uninstall: Option<String>,
    launch: Option<String>,
}

#[derive(Clone, Copy)]
struct DirectDesktopInstaller {
    url: &'static str,
    file_name: &'static str,
    silent_args: &'static [&'static str],
}

#[tauri::command]
pub async fn vibe_tools() -> Vec<VibeTool> {
    tauri::async_runtime::spawn_blocking(|| scan_vibe_tools(true))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn vibe_tool(id: String) -> Result<VibeTool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scan_vibe_tool(&id, true).ok_or_else(|| "未知的工作智能体工具".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn vibe_environment_prompt() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(build_environment_prompt)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn vibe_tool_action(
    window: tauri::Window,
    id: String,
    target: String,
    action: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_tool_action(&id, &target, &action, Some(window))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn vibe_open_desktop(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_desktop_tool(&id))
        .await
        .map_err(|e| e.to_string())?
}

fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "claude",
            name: "Claude Code",
            description:
                "Anthropic 的工作智能体工具，CLI 适合终端工作流，桌面端适合多会话与可视化审查。",
            docs_url: "https://docs.anthropic.com/en/docs/claude-code/overview",
            cli: CliSpec {
                name: "Claude Code CLI",
                description: "在终端中理解、修改和运行项目代码。",
                command: "claude",
                candidates: &["claude.exe", "claude.cmd", "claude.bat", "claude.ps1"],
                npm_package: Some("@anthropic-ai/claude-code"),
                winget_id: Some("Anthropic.ClaudeCode"),
                install_url: "https://docs.anthropic.com/en/docs/claude-code/setup",
                docs_url: "https://docs.anthropic.com/en/docs/claude-code/overview",
            },
            desktop: DesktopSpec {
                name: "Claude 桌面端",
                description: "Claude 的 Windows 桌面应用，包含 Claude Code 图形界面入口。",
                winget_id: Some("Anthropic.Claude"),
                winget_source: None,
                appx_names: &["Claude"],
                install_url:
                    "https://support.claude.com/en/articles/10065433-install-claude-desktop",
                docs_url: "https://code.claude.com/docs/en/desktop-quickstart",
                keywords: &["claude"],
                excludes: &["claude code", "claudecode"],
            },
        },
        ToolSpec {
            id: "codex",
            name: "Codex",
            description:
                "OpenAI 的本地工作智能体工具，CLI 适合终端自动化，桌面端适合线程、工作区和审查。",
            docs_url: "https://developers.openai.com/codex/",
            cli: CliSpec {
                name: "Codex CLI",
                description: "在终端中运行 Codex，适合项目维护、自动化修改和脚本化任务。",
                command: "codex",
                candidates: &["codex.exe", "codex.cmd", "codex.bat", "codex.ps1"],
                npm_package: Some("@openai/codex"),
                winget_id: Some("OpenAI.Codex"),
                install_url: "https://developers.openai.com/codex/cli",
                docs_url: "https://developers.openai.com/codex/cli",
            },
            desktop: DesktopSpec {
                name: "Codex 桌面端",
                description: "OpenAI Codex 桌面应用，用于并行管理 Codex 线程和本地工作区。",
                winget_id: Some("9PLM9XGG6VKS"),
                winget_source: Some("msstore"),
                appx_names: &["OpenAI.CodexBeta", "OpenAI.Codex"],
                install_url: "https://developers.openai.com/codex/app/windows",
                docs_url: "https://developers.openai.com/codex/app",
                keywords: &["codex"],
                excludes: &["cli"],
            },
        },
        ToolSpec {
            id: "antigravity",
            name: "Antigravity",
            description:
                "Google 的 agent-first 开发平台；CLI 与桌面端共享 Antigravity agent 工作流。",
            docs_url: "https://antigravity.google/docs/home",
            cli: CliSpec {
                name: "Antigravity CLI",
                description: "命令名 agy，适合终端内运行 Google Antigravity agent 工作流。",
                command: "agy",
                candidates: &["agy.exe", "agy.cmd", "agy.bat", "agy.ps1"],
                npm_package: None,
                winget_id: Some("Google.AntigravityCLI"),
                install_url: "https://antigravity.google/docs/cli-install",
                docs_url: "https://antigravity.google/docs/cli/overview",
            },
            desktop: DesktopSpec {
                name: "Antigravity 桌面端",
                description: "Google Antigravity 桌面开发平台，用于管理多个本地 agent 和工作区。",
                winget_id: Some("Google.Antigravity"),
                winget_source: None,
                appx_names: &[],
                install_url: "https://antigravity.google/download",
                docs_url: "https://antigravity.google/docs/home",
                keywords: &["antigravity"],
                excludes: &["cli"],
            },
        },
        ToolSpec {
            id: "opencode",
            name: "OpenCode",
            description: "开源工作智能体，支持终端自动化与图形化项目协作。",
            docs_url: "https://opencode.ai/docs/",
            cli: CliSpec {
                name: "OpenCode CLI",
                description: "开源终端工作智能体工具，支持多模型和项目协作。",
                command: "opencode",
                candidates: &[
                    "opencode.exe",
                    "opencode.cmd",
                    "opencode.bat",
                    "opencode.ps1",
                ],
                npm_package: Some("opencode-ai"),
                winget_id: Some("SST.opencode"),
                install_url: "https://opencode.ai/docs/",
                docs_url: "https://opencode.ai/docs/cli/",
            },
            desktop: DesktopSpec {
                name: "OpenCode 桌面端",
                description: "OpenCode 图形化客户端，用于管理会话并开展项目协作。",
                winget_id: Some("SST.OpenCodeDesktop"),
                winget_source: None,
                appx_names: &[],
                install_url: "https://opencode.ai/download",
                docs_url: "https://opencode.ai/download",
                keywords: &["opencode", "open code"],
                excludes: &["cli"],
            },
        },
        ToolSpec {
            id: "zcode",
            name: "ZCode",
            description: "Z.ai 的桌面工作智能体，用于在本地工作区中规划、修改和验证代码。",
            docs_url: "https://zcode.z.ai/en/docs/install",
            cli: CliSpec {
                name: "ZCode CLI",
                description: "ZCode 当前未提供独立的 Windows CLI。",
                command: "",
                candidates: &[],
                npm_package: None,
                winget_id: None,
                install_url: "https://zcode.z.ai/en/docs/install",
                docs_url: "https://zcode.z.ai/en/docs/install",
            },
            desktop: DesktopSpec {
                name: "ZCode 桌面端",
                description: "ZCode Windows 桌面应用，提供完整的 Agent 开发工作流。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://zcode.z.ai/en/docs/install",
                docs_url: "https://zcode.z.ai/en/docs/install",
                keywords: &["zcode"],
                excludes: &[],
            },
        },
        ToolSpec {
            id: "kimi",
            name: "Kimi Code",
            description: "Kimi 的终端工作智能体，可读取和修改项目、运行命令并完成开发任务。",
            docs_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
            cli: CliSpec {
                name: "Kimi Code CLI",
                description: "命令名 kimi，适合在终端中开展完整的项目开发工作流。",
                command: "kimi",
                candidates: &["kimi.exe", "kimi.cmd", "kimi.bat", "kimi.ps1"],
                npm_package: Some("@moonshot-ai/kimi-code"),
                winget_id: None,
                install_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
                docs_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
            },
            desktop: DesktopSpec {
                name: "Kimi Code 桌面端",
                description: "Kimi Code 当前以 CLI 和编辑器扩展为主要形态。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
                docs_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
                keywords: &[],
                excludes: &[],
            },
        },
        ToolSpec {
            id: "workbuddy",
            name: "WorkBuddy",
            description: "腾讯 WorkBuddy 桌面智能体，支持本地文件处理、开发任务与多步骤工作流。",
            docs_url: "https://www.workbuddy.ai/docs/workbuddy/Quickstart",
            cli: CliSpec {
                name: "WorkBuddy CLI",
                description: "WorkBuddy 当前未提供独立的 Windows CLI。",
                command: "",
                candidates: &[],
                npm_package: None,
                winget_id: None,
                install_url: "https://www.workbuddy.ai/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide",
                docs_url: "https://www.workbuddy.ai/docs/workbuddy/Quickstart",
            },
            desktop: DesktopSpec {
                name: "WorkBuddy 桌面端",
                description: "腾讯 WorkBuddy Windows 桌面应用。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.workbuddy.ai/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide",
                docs_url: "https://www.workbuddy.ai/docs/workbuddy/Quickstart",
                keywords: &["workbuddy", "work buddy"],
                excludes: &[],
            },
        },
        ToolSpec {
            id: "qoder",
            name: "Qoder",
            description: "Qoder AI 编程平台，提供终端智能体与桌面 IDE。",
            docs_url: "https://docs.qoder.com/",
            cli: CliSpec {
                name: "Qoder CLI",
                description: "命令名 qodercli，可在终端中执行代码理解、修改与自动化任务。",
                command: "qodercli",
                candidates: &[
                    "qodercli.exe",
                    "qodercli.cmd",
                    "qodercli.bat",
                    "qodercli.ps1",
                ],
                npm_package: Some("@qoder-ai/qodercli"),
                winget_id: None,
                install_url: "https://docs.qoder.com/en/cli/quick-start",
                docs_url: "https://docs.qoder.com/en/cli/quick-start",
            },
            desktop: DesktopSpec {
                name: "Qoder 桌面端",
                description: "Qoder IDE Windows 桌面应用。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://qoder.com/download",
                docs_url: "https://docs.qoder.com/quick-start",
                keywords: &["qoder"],
                excludes: &["cli"],
            },
        },
        ToolSpec {
            id: "trae-work",
            name: "TRAE Work",
            description: "TRAE Work 桌面智能体，支持工作区文件处理、开发任务和并行执行。",
            docs_url: "https://www.trae.ai/work",
            cli: CliSpec {
                name: "TRAE Work CLI",
                description: "TRAE Work 当前未提供独立的 Windows CLI。",
                command: "",
                candidates: &[],
                npm_package: None,
                winget_id: None,
                install_url: "https://www.trae.ai/download",
                docs_url: "https://www.trae.ai/work",
            },
            desktop: DesktopSpec {
                name: "TRAE Work 桌面端",
                description: "TRAE Work Windows 桌面应用，包含工作模式与代码模式。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.trae.ai/download",
                docs_url: "https://www.trae.ai/work",
                keywords: &["trae work"],
                excludes: &["trae ide"],
            },
        },
        ToolSpec {
            id: "openclaw",
            name: "OpenClaw",
            description: "开源个人 AI 智能体平台，可在终端中管理网关、会话、工具和本地自动化任务。",
            docs_url: "https://docs.openclaw.ai/",
            cli: CliSpec {
                name: "OpenClaw CLI",
                description: "命令名 openclaw，用于配置并运行 OpenClaw 网关、智能体和本地工具。",
                command: "openclaw",
                candidates: &[
                    "openclaw.exe",
                    "openclaw.cmd",
                    "openclaw.bat",
                    "openclaw.ps1",
                ],
                npm_package: Some("openclaw"),
                winget_id: None,
                install_url: "https://docs.openclaw.ai/install",
                docs_url: "https://docs.openclaw.ai/",
            },
            desktop: DesktopSpec {
                name: "OpenClaw Hub",
                description: "OpenClaw 的 Windows 桌面伴侣，用于设置、托盘状态、聊天和本地 MCP。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://docs.openclaw.ai/windows",
                docs_url: "https://docs.openclaw.ai/windows",
                keywords: &["openclaw hub", "openclaw"],
                excludes: &["cli"],
            },
        },
        ToolSpec {
            id: "hermes",
            name: "Hermes Agent",
            description: "Nous Research 的本地 AI 智能体，CLI 与桌面端共享配置、会话、技能和记忆。",
            docs_url: "https://hermes-agent.nousresearch.com/docs/",
            cli: CliSpec {
                name: "Hermes CLI",
                description:
                    "命令名 hermes，用于运行智能体、管理模型、技能、网关和本地自动化任务。",
                command: "hermes",
                candidates: &["hermes.exe", "hermes.cmd", "hermes.bat", "hermes.ps1"],
                npm_package: None,
                winget_id: None,
                install_url:
                    "https://hermes-agent.nousresearch.com/docs/getting-started/installation",
                docs_url: "https://hermes-agent.nousresearch.com/docs/",
            },
            desktop: DesktopSpec {
                name: "Hermes 桌面端",
                description: "Hermes 原生桌面应用，与 CLI 共用智能体运行时、配置、会话和技能。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://hermes-agent.nousresearch.com/docs/user-guide/desktop",
                docs_url: "https://hermes-agent.nousresearch.com/docs/user-guide/desktop",
                keywords: &["hermes agent", "hermes desktop", "hermes"],
                excludes: &["hermes browser"],
            },
        },
    ]
}

fn spec_by_id(id: &str) -> Option<ToolSpec> {
    tool_specs().into_iter().find(|s| s.id == id)
}

fn scan_vibe_tools(check_latest: bool) -> Vec<VibeTool> {
    std::thread::scope(|scope| {
        let handles = tool_specs()
            .into_iter()
            .map(|spec| scope.spawn(move || vibe_tool_from_spec(spec, check_latest)))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    })
}

fn scan_vibe_tool(id: &str, check_latest: bool) -> Option<VibeTool> {
    spec_by_id(id).map(|spec| vibe_tool_from_spec(spec, check_latest))
}

fn vibe_tool_from_spec(spec: ToolSpec, check_latest: bool) -> VibeTool {
    VibeTool {
        id: spec.id.into(),
        name: spec.name.into(),
        description: spec.description.into(),
        docs_url: spec.docs_url.into(),
        cli: cli_surface(&spec, check_latest),
        desktop: desktop_surface(&spec, check_latest),
    }
}

fn cli_surface(spec: &ToolSpec, check_latest: bool) -> VibeSurface {
    if spec.cli.command.is_empty() {
        return unavailable_surface(
            spec.cli.name,
            "CLI",
            spec.cli.description,
            spec.cli.install_url,
            spec.cli.docs_url,
        );
    }
    let program = resolve_command(spec.cli.candidates);
    let probe = program
        .as_deref()
        .map(|p| run_program_probe(spec.cli.name, p, &["--version"], Duration::from_secs(5)));
    let version = probe
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .map(|s| s.to_string());
    let method = detect_install_method(spec, program.as_deref());
    let probe_error = probe
        .as_ref()
        .and_then(|r| r.as_ref().err())
        .map(|s| s.to_string());
    let installed =
        program.is_some() && (probe.as_ref().is_some_and(|r| r.is_ok()) || method.is_some());
    let latest = if check_latest && installed {
        latest_for_cli(spec, method.as_deref(), version.as_deref())
            .ok()
            .flatten()
    } else {
        None
    };
    let update_available = installed
        && version
            .as_deref()
            .zip(latest.as_deref())
            .is_some_and(|(cur, next)| crate::update::ver_lt(cur, next));
    let status = if update_available {
        "update"
    } else if installed {
        "installed"
    } else if program.is_some() {
        "unknown"
    } else {
        "missing"
    };
    VibeSurface {
        available: true,
        label: spec.cli.name.into(),
        kind: "CLI".into(),
        description: spec.cli.description.into(),
        installed,
        status: status.into(),
        version,
        probe_error,
        latest,
        update_available,
        path: program.map(|p| p.to_string_lossy().into_owned()),
        command: Some(spec.cli.command.into()),
        install_method_label: method.as_deref().and_then(install_method_label),
        install_method: method,
        install_url: spec.cli.install_url.into(),
        docs_url: spec.cli.docs_url.into(),
        can_install: true,
        install_unavailable_reason: None,
        can_update: installed,
        can_uninstall: installed,
        can_open: installed,
    }
}

fn desktop_surface(spec: &ToolSpec, check_latest: bool) -> VibeSurface {
    if spec.desktop.keywords.is_empty()
        && spec.desktop.winget_id.is_none()
        && spec.desktop.appx_names.is_empty()
    {
        return unavailable_surface(
            spec.desktop.name,
            "桌面端",
            spec.desktop.description,
            spec.desktop.install_url,
            spec.desktop.docs_url,
        );
    }
    let found = detect_desktop_app(&spec.desktop);
    let installed = found.is_some();
    let method = found.as_ref().and_then(|f| f.method.clone());
    let version = found.as_ref().and_then(|f| f.version.clone());
    let path = found
        .as_ref()
        .and_then(|f| f.path.as_ref())
        .map(|p| p.to_string_lossy().into_owned());
    let latest = if check_latest && installed {
        desktop_internal_latest(spec, version.as_deref())
            .or_else(|| {
                spec.desktop
                    .winget_id
                    .and_then(|id| winget_available_update(id).ok().flatten())
            })
            .or_else(|| version.clone())
    } else {
        None
    };
    let update_available = installed
        && version
            .as_deref()
            .zip(latest.as_deref())
            .is_some_and(|(cur, next)| crate::update::ver_lt(cur, next));
    let can_install =
        spec.desktop.winget_id.is_some() || direct_desktop_installer(spec.id).is_some();
    VibeSurface {
        available: true,
        label: spec.desktop.name.into(),
        kind: "桌面端".into(),
        description: spec.desktop.description.into(),
        installed,
        status: if update_available {
            "update".into()
        } else if installed {
            "installed".into()
        } else {
            "missing".into()
        },
        version,
        probe_error: None,
        latest,
        update_available,
        path,
        command: None,
        install_method_label: method.as_deref().and_then(install_method_label),
        install_method: method,
        install_url: spec.desktop.install_url.into(),
        docs_url: spec.desktop.docs_url.into(),
        can_install,
        install_unavailable_reason: (!can_install)
            .then(|| desktop_install_unavailable_reason(spec).to_string()),
        can_update: installed && (spec.desktop.winget_id.is_some() || update_available),
        can_uninstall: installed,
        can_open: installed,
    }
}

fn unavailable_surface(
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
    }
}

fn run_tool_action(
    id: &str,
    target: &str,
    action: &str,
    window: Option<tauri::Window>,
) -> Result<String, String> {
    crate::installer::op_reset();
    let spec = spec_by_id(id).ok_or_else(|| "未知的工作智能体工具".to_string())?;
    let res = match (target, action) {
        ("cli", "install") => install_cli_tool(&spec, &window),
        ("cli", "update") => update_cli_tool(&spec, &window),
        ("cli", "uninstall") => uninstall_cli_tool(&spec, &window),
        ("desktop", "install") => install_desktop_tool(&spec, &window),
        ("desktop", "update") => update_desktop_tool(&spec, &window),
        ("desktop", "uninstall") => uninstall_desktop_tool(&spec, &window),
        _ => Err("不支持的操作".into()),
    };
    emit_progress(&window, "__done__");
    res
}

fn install_cli_tool(spec: &ToolSpec, window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, format!("正在安装 {}…", spec.cli.name));
    match spec.id {
        "claude" => install_or_update_claude(None, None, window),
        "codex" => run_codex_installer().or_else(|_| {
            if let Some(pkg) = spec.cli.npm_package {
                emit_progress(window, "官方安装器不可用，尝试通过 npm 安装 Codex CLI…");
                npm_install_latest(pkg, None)?;
                Ok("Codex CLI 已通过 npm 安装".into())
            } else {
                Err("Codex CLI 安装失败".into())
            }
        }),
        "antigravity" => {
            if let Some(id) = spec.cli.winget_id {
                if winget_command().is_some() {
                    emit_progress(window, "正在通过 WinGet 安装 Antigravity CLI…");
                    let result = run_winget_owned(
                        winget_args("install", id, None, true),
                        Duration::from_secs(900),
                        window,
                    );
                    match result {
                        Ok(_) => {}
                        Err(err) => {
                            if cli_installed_after_action(spec) {
                                return Ok("Antigravity CLI 已安装".into());
                            }
                            return Err(err);
                        }
                    }
                    return Ok("Antigravity CLI 已通过 WinGet 安装".into());
                }
            }
            install_or_update_antigravity_cli(window, "安装")
        }
        "opencode" => install_opencode(window),
        "openclaw" => install_openclaw(window),
        "hermes" => install_hermes(window),
        _ => {
            if let Some(pkg) = spec.cli.npm_package {
                npm_install_latest(pkg, None)?;
                Ok(format!("{} 已通过 npm 安装", spec.cli.name))
            } else {
                Err("该 CLI 暂无自动安装方案，请查看官方文档。".into())
            }
        }
    }
}

fn update_cli_tool(spec: &ToolSpec, window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, "正在检测当前安装来源…");
    let program = resolve_command(spec.cli.candidates);
    let method = detect_install_method(spec, program.as_deref());
    match spec.id {
        "claude" => install_or_update_claude(program.as_deref(), method.as_deref(), window),
        "codex" => match (program.as_deref(), method.as_deref()) {
            (Some(program), Some("npm")) => {
                update_with_npm_source(spec, program, window)?;
                Ok("Codex CLI 已通过 npm 更新".into())
            }
            (Some(_), Some("winget")) => {
                let id = spec.cli.winget_id.ok_or("Codex CLI 缺少 WinGet 包 ID")?;
                emit_progress(window, "正在通过 WinGet 更新 Codex CLI…");
                run_winget_owned(
                    winget_args("upgrade", id, None, true),
                    Duration::from_secs(900),
                    window,
                )?;
                Ok("Codex CLI 已通过 WinGet 更新".into())
            }
            (Some(_), _) => {
                emit_progress(window, "正在运行 Codex 官方 Windows 安装器…");
                run_codex_installer()?;
                Ok("Codex CLI 已通过官方 Windows 安装器更新".into())
            }
            (None, _) => install_cli_tool(spec, window),
        },
        "antigravity" => {
            if method.as_deref() == Some("winget") {
                let id = spec
                    .cli
                    .winget_id
                    .ok_or("Antigravity CLI 缺少 WinGet 包 ID")?;
                emit_progress(window, "正在通过 WinGet 更新 Antigravity CLI…");
                run_winget_owned(
                    winget_args("upgrade", id, None, true),
                    Duration::from_secs(900),
                    window,
                )?;
                return Ok("Antigravity CLI 已通过 WinGet 更新".into());
            }
            if let Some(program) = program {
                emit_progress(window, "正在执行 agy update…");
                match run_command_text(
                    &program,
                    &["update"],
                    "agy update",
                    Duration::from_secs(900),
                ) {
                    Ok(_) => return Ok("Antigravity CLI 已更新".into()),
                    Err(err) => emit_progress(
                        window,
                        format!("agy update 未完成，改用官方安装脚本：{err}"),
                    ),
                }
            }
            install_or_update_antigravity_cli(window, "更新")
        }
        "opencode" => match (program.as_deref(), method.as_deref()) {
            (Some(program), Some("npm")) => {
                update_with_npm_source(spec, program, window)?;
                Ok("OpenCode CLI 已通过 npm 更新".into())
            }
            (Some(_), Some("winget")) => {
                let id = spec.cli.winget_id.ok_or("OpenCode CLI 缺少 WinGet 包 ID")?;
                emit_progress(window, "正在通过 WinGet 更新 OpenCode CLI…");
                run_winget_owned(
                    winget_args("upgrade", id, None, true),
                    Duration::from_secs(900),
                    window,
                )?;
                Ok("OpenCode CLI 已通过 WinGet 更新".into())
            }
            (Some(_), Some("scoop")) => {
                emit_progress(window, "正在通过 Scoop 更新 OpenCode CLI…");
                run_scoop(&["update", "opencode"], Duration::from_secs(900))?;
                Ok("OpenCode CLI 已通过 Scoop 更新".into())
            }
            (Some(_), Some("chocolatey")) => {
                emit_progress(window, "正在通过 Chocolatey 更新 OpenCode CLI…");
                run_choco(&["upgrade", "opencode", "-y"], Duration::from_secs(900))?;
                Ok("OpenCode CLI 已通过 Chocolatey 更新".into())
            }
            (Some(_), _) => Err(
                "已检测到 OpenCode CLI，但无法判断安装来源。请按官方文档使用原安装方式更新。"
                    .into(),
            ),
            (None, _) => install_cli_tool(spec, window),
        },
        "hermes" => {
            let program = program.ok_or_else(|| "未检测到 Hermes CLI。".to_string())?;
            emit_progress(window, "正在执行 hermes update…");
            run_command_text(
                &program,
                &["update"],
                "hermes update",
                Duration::from_secs(1200),
            )?;
            Ok("Hermes CLI 已更新".into())
        }
        _ => {
            let program = program.ok_or_else(|| format!("未检测到 {}。", spec.cli.name))?;
            update_with_npm_source(spec, &program, window)?;
            Ok(format!("{} 已更新", spec.cli.name))
        }
    }
}

fn uninstall_cli_tool(spec: &ToolSpec, window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, "正在检测当前安装来源…");
    let program = resolve_command(spec.cli.candidates);
    let method = detect_install_method(spec, program.as_deref());
    if spec.id == "hermes" {
        let program = program.ok_or_else(|| "未检测到 Hermes CLI。".to_string())?;
        emit_progress(window, "正在运行 Hermes 官方卸载程序…");
        run_command_text(
            &program,
            &["uninstall", "--yes"],
            "hermes uninstall",
            Duration::from_secs(900),
        )?;
        return Ok("Hermes CLI 已卸载，用户配置和会话数据已保留".into());
    }
    if spec.id == "openclaw" {
        let program = program.ok_or_else(|| "未检测到 OpenClaw CLI。".to_string())?;
        emit_progress(window, "正在移除 OpenClaw 网关服务…");
        let _ = run_command_text(
            &program,
            &["gateway", "uninstall"],
            "openclaw gateway uninstall",
            Duration::from_secs(180),
        );
        let pkg = spec.cli.npm_package.ok_or("OpenClaw 缺少 npm 包信息。")?;
        emit_progress(window, "正在卸载 OpenClaw CLI…");
        npm_uninstall(pkg, Some(&program))?;
        return Ok("OpenClaw CLI 与网关服务已卸载，配置和工作区已保留".into());
    }
    match method.as_deref() {
        Some("winget") => {
            let id = spec.cli.winget_id.ok_or("该 CLI 缺少 WinGet 包 ID")?;
            emit_progress(window, format!("正在通过 WinGet 卸载 {}…", spec.cli.name));
            run_winget_owned(
                winget_args("uninstall", id, None, true),
                Duration::from_secs(900),
                window,
            )?;
            Ok(format!("{} 已通过 WinGet 卸载", spec.cli.name))
        }
        Some("npm") | Some("conda-npm") => {
            let pkg = spec
                .cli
                .npm_package
                .ok_or("该 CLI 不是 npm 包，无法通过 npm 卸载。")?;
            emit_progress(window, format!("正在通过 npm 卸载 {}…", spec.cli.name));
            npm_uninstall(pkg, program.as_deref())?;
            Ok(format!("{} 已通过 npm 卸载", spec.cli.name))
        }
        Some("scoop") => {
            emit_progress(window, format!("正在通过 Scoop 卸载 {}…", spec.cli.name));
            run_scoop(&["uninstall", spec.cli.command], Duration::from_secs(900))?;
            Ok(format!("{} 已通过 Scoop 卸载", spec.cli.name))
        }
        Some("chocolatey") => {
            emit_progress(
                window,
                format!("正在通过 Chocolatey 卸载 {}…", spec.cli.name),
            );
            run_choco(
                &["uninstall", spec.cli.command, "-y"],
                Duration::from_secs(900),
            )?;
            Ok(format!("{} 已通过 Chocolatey 卸载", spec.cli.name))
        }
        Some("native") => {
            let program = program.ok_or_else(|| "未找到可卸载的命令入口。".to_string())?;
            remove_cli_binary(&program, spec.cli.command)?;
            Ok(format!(
                "{} 命令文件已移除，登录配置和历史数据已保留。",
                spec.cli.name
            ))
        }
        _ => Err(format!(
            "无法判断 {} 的安装来源。为避免误删文件，请按官方文档卸载。",
            spec.cli.name
        )),
    }
}

fn install_desktop_tool(spec: &ToolSpec, window: &Option<tauri::Window>) -> Result<String, String> {
    if let Some(id) = spec.desktop.winget_id {
        emit_progress(
            window,
            format!("正在通过 WinGet 安装 {}…", spec.desktop.name),
        );
        let result = run_winget_owned(
            winget_args("install", id, spec.desktop.winget_source, true),
            Duration::from_secs(1200),
            window,
        );
        match result {
            Ok(_) => {}
            Err(err) => {
                if desktop_installed_after_action(spec) {
                    return Ok(format!("{} 已安装", spec.desktop.name));
                }
                return Err(err);
            }
        }
        return Ok(format!("{} 已通过 WinGet 安装", spec.desktop.name));
    }
    if let Some(installer) = direct_desktop_installer(spec.id) {
        return match install_desktop_from_official_package(spec, installer, window) {
            Ok(message) => Ok(message),
            Err(err) if err.contains("已取消") => Err(err),
            Err(err) => match open_external_target(spec.desktop.install_url) {
                Ok(()) => Err(format!(
                    "{err}。已打开 {} 官方安装页，可在浏览器中继续下载。",
                    spec.desktop.name
                )),
                Err(open_err) => Err(format!("{err}；同时无法打开官方安装页：{open_err}")),
            },
        };
    }
    Err(format!(
        "{} 无法自动安装：{}",
        spec.desktop.name,
        desktop_install_unavailable_reason(spec)
    ))
}

fn update_desktop_tool(spec: &ToolSpec, window: &Option<tauri::Window>) -> Result<String, String> {
    let found = detect_desktop_app(&spec.desktop);
    let current = found.as_ref().and_then(|f| f.version.as_deref());
    if let Some(next) = desktop_internal_latest(spec, current) {
        emit_progress(
            window,
            format!("{} 已下载 {next}，需要重启应用完成更新…", spec.desktop.name),
        );
        open_desktop_tool(spec.id)?;
        return Ok(format!(
            "{} 已下载 {next}，请在应用内点击 Relaunch to update 完成更新。",
            spec.desktop.name
        ));
    }
    if let Some(id) = spec.desktop.winget_id {
        emit_progress(
            window,
            format!("正在通过 WinGet 更新 {}…", spec.desktop.name),
        );
        run_winget_owned(
            winget_args("upgrade", id, spec.desktop.winget_source, true),
            Duration::from_secs(1200),
            window,
        )?;
        return Ok(format!("{} 已通过 WinGet 更新", spec.desktop.name));
    }
    Err(format!(
        "{} 暂无可自动执行的 Windows 更新源，已取消操作。",
        spec.desktop.name
    ))
}

fn uninstall_desktop_tool(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let found = detect_desktop_app(&spec.desktop);
    if let Some(uninstall) = found
        .as_ref()
        .and_then(|f| f.uninstall.as_deref())
        .filter(|s| s.starts_with("appx:"))
    {
        let package = uninstall.trim_start_matches("appx:");
        emit_progress(window, format!("正在卸载 {}…", spec.desktop.name));
        uninstall_appx_package(package)?;
        return Ok(format!("{} 已卸载", spec.desktop.name));
    }
    if let Some(id) = spec.desktop.winget_id {
        emit_progress(
            window,
            format!("正在通过 WinGet 卸载 {}…", spec.desktop.name),
        );
        run_winget_owned(
            winget_args("uninstall", id, spec.desktop.winget_source, true),
            Duration::from_secs(1200),
            window,
        )?;
        return Ok(format!("{} 已通过 WinGet 卸载", spec.desktop.name));
    }
    if let Some(uninstall) = found.and_then(|f| f.uninstall) {
        emit_progress(window, format!("正在运行 {} 卸载程序…", spec.desktop.name));
        run_uninstall_string(&uninstall)?;
        return Ok(format!("{} 卸载程序已启动", spec.desktop.name));
    }
    Err(format!(
        "未找到 {} 的自动卸载入口。请在 Windows“已安装的应用”中卸载。",
        spec.desktop.name
    ))
}

fn open_desktop_tool(id: &str) -> Result<(), String> {
    let spec = spec_by_id(id).ok_or_else(|| "未知的工作智能体工具".to_string())?;
    if let Some(found) = detect_desktop_app(&spec.desktop) {
        if let Some(launch) = found.launch {
            return open_external_target(&launch);
        }
        if let Some(path) = found.path {
            return open_external_target(&path.to_string_lossy());
        }
    }
    if spec.id == "codex" {
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

fn install_or_update_claude(
    program: Option<&Path>,
    method: Option<&str>,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    match (program, method) {
        (Some(_), Some("winget")) => {
            emit_progress(window, "正在通过 WinGet 更新 Claude Code CLI…");
            run_winget_owned(
                winget_args("upgrade", "Anthropic.ClaudeCode", None, true),
                Duration::from_secs(900),
                window,
            )?;
            Ok("Claude Code CLI 已通过 WinGet 更新".into())
        }
        (Some(program), Some("npm")) => {
            emit_progress(window, "正在通过 npm 更新 Claude Code CLI…");
            npm_install_latest("@anthropic-ai/claude-code", Some(program))?;
            Ok("Claude Code CLI 已通过 npm 更新".into())
        }
        (Some(program), Some("native")) => {
            emit_progress(window, "正在执行 claude update…");
            run_command_text(
                program,
                &["update"],
                "claude update",
                Duration::from_secs(900),
            )?;
            Ok("Claude Code CLI 已执行 claude update".into())
        }
        (Some(_), _) => Err(
            "已检测到 Claude Code CLI，但无法判断安装来源。请按官方文档使用原安装方式更新。".into(),
        ),
        (None, _) => {
            if winget_command().is_some() {
                emit_progress(window, "正在通过 WinGet 安装 Claude Code CLI…");
                let result = run_winget_owned(
                    winget_args("install", "Anthropic.ClaudeCode", Some("winget"), true),
                    Duration::from_secs(900),
                    window,
                );
                match result {
                    Ok(_) => {}
                    Err(err) => {
                        if let Some(spec) = spec_by_id("claude") {
                            if cli_installed_after_action(&spec) {
                                return Ok("Claude Code CLI 已安装".into());
                            }
                        }
                        return Err(err);
                    }
                }
                Ok("Claude Code CLI 已通过 WinGet 安装".into())
            } else {
                emit_progress(window, "正在运行 Claude Code 官方 Native Installer…");
                run_powershell(
                    &[
                        "-NoProfile",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-Command",
                        "irm https://claude.ai/install.ps1 | iex",
                    ],
                    "Claude Code Native Install",
                    Duration::from_secs(900),
                )?;
                Ok("Claude Code CLI 已通过官方 Native Installer 安装".into())
            }
        }
    }
}

fn update_with_npm_source(
    spec: &ToolSpec,
    program: &Path,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let pkg = spec
        .cli
        .npm_package
        .ok_or_else(|| format!("{} 不是 npm 包。", spec.cli.name))?;
    emit_progress(window, format!("正在通过 npm 更新 {}…", spec.cli.name));
    npm_install_latest(pkg, Some(program))
}

fn install_opencode(window: &Option<tauri::Window>) -> Result<String, String> {
    if resolve_command(&["npm.cmd", "npm.exe", "npm.bat"]).is_some() {
        emit_progress(window, "正在通过 npm 安装 OpenCode CLI…");
        npm_install_latest("opencode-ai", None)?;
        return Ok("OpenCode CLI 已通过 npm 安装".into());
    }
    if winget_command().is_some() {
        emit_progress(window, "正在通过 WinGet 安装 OpenCode CLI…");
        let result = run_winget_owned(
            winget_args("install", "SST.opencode", None, true),
            Duration::from_secs(900),
            window,
        );
        match result {
            Ok(_) => {}
            Err(err) => {
                if let Some(spec) = spec_by_id("opencode") {
                    if cli_installed_after_action(&spec) {
                        return Ok("OpenCode CLI 已安装".into());
                    }
                }
                return Err(err);
            }
        }
        return Ok("OpenCode CLI 已通过 WinGet 安装".into());
    }
    if scoop_command().is_some() {
        emit_progress(window, "正在通过 Scoop 安装 OpenCode CLI…");
        run_scoop(&["install", "opencode"], Duration::from_secs(900))?;
        return Ok("OpenCode CLI 已通过 Scoop 安装".into());
    }
    if choco_command().is_some() {
        emit_progress(window, "正在通过 Chocolatey 安装 OpenCode CLI…");
        run_choco(&["install", "opencode", "-y"], Duration::from_secs(900))?;
        return Ok("OpenCode CLI 已通过 Chocolatey 安装".into());
    }
    Err("未检测到 npm、WinGet、Scoop 或 Chocolatey。请先安装 Node.js，或按 OpenCode 官方文档选择安装方式。".into())
}

fn install_openclaw(window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, "正在运行 OpenClaw 官方 Windows 安装器…");
    run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& ([scriptblock]::Create((iwr -useb https://openclaw.ai/install.ps1))) -NoOnboard",
        ],
        "OpenClaw Windows Installer",
        Duration::from_secs(1200),
    )?;
    Ok("OpenClaw CLI 已通过官方安装器安装".into())
}

fn install_hermes(window: &Option<tauri::Window>) -> Result<String, String> {
    emit_progress(window, "正在运行 Hermes 官方 Windows 安装器…");
    run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "iex (irm https://hermes-agent.nousresearch.com/install.ps1)",
        ],
        "Hermes Windows Installer",
        Duration::from_secs(1200),
    )?;
    Ok("Hermes CLI 已通过官方安装器安装".into())
}

fn cli_installed_after_action(spec: &ToolSpec) -> bool {
    resolve_command(spec.cli.candidates).is_some()
        || spec.cli.winget_id.is_some_and(winget_package_installed)
}

fn desktop_installed_after_action(spec: &ToolSpec) -> bool {
    detect_desktop_app(&spec.desktop).is_some()
}

fn direct_desktop_installer(id: &str) -> Option<DirectDesktopInstaller> {
    match id {
        "openclaw" => Some(DirectDesktopInstaller {
            url: openclaw_desktop_installer_url(),
            file_name: "OpenClawCompanion-Setup.exe",
            silent_args: &["/S"],
        }),
        "hermes" => Some(DirectDesktopInstaller {
            url: "https://hermes-assets.nousresearch.com/Hermes-Setup.exe",
            file_name: "Hermes-Setup.exe",
            silent_args: &["/S"],
        }),
        _ => None,
    }
}

#[cfg(target_arch = "aarch64")]
fn openclaw_desktop_installer_url() -> &'static str {
    "https://github.com/openclaw/openclaw/releases/latest/download/OpenClawCompanion-Setup-arm64.exe"
}

#[cfg(not(target_arch = "aarch64"))]
fn openclaw_desktop_installer_url() -> &'static str {
    "https://github.com/openclaw/openclaw/releases/latest/download/OpenClawCompanion-Setup-x64.exe"
}

fn desktop_install_unavailable_reason(spec: &ToolSpec) -> &'static str {
    match spec.id {
        "trae-work" => {
            "TRAE Work 官网动态下发安装地址，当前无法可靠自动下载安装，请通过官方文档安装。"
        }
        "zcode" | "workbuddy" | "qoder" => {
            "尚未找到可稳定调用的官方 Windows 安装接口，请通过官方文档安装。"
        }
        _ => "官方未提供可自动安装的独立 Windows 应用。",
    }
}

fn install_desktop_from_official_package(
    spec: &ToolSpec,
    installer: DirectDesktopInstaller,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    emit_progress(
        window,
        format!("正在连接 {} 官方下载地址…", spec.desktop.name),
    );
    let path = download_desktop_installer(spec, installer, window)?;
    let result = (|| {
        emit_progress(window, "正在验证安装程序数字签名…");
        let signer = verify_desktop_installer_signature(&path)?;
        emit_progress(window, format!("数字签名有效 · {signer}"));
        run_downloaded_desktop_installer(spec, installer, &path, window)?;

        emit_progress(window, format!("正在确认 {} 安装状态…", spec.desktop.name));
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            if desktop_installed_after_action(spec) {
                return Ok(format!("{} 已安装", spec.desktop.name));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!(
            "{} 安装程序已结束，但尚未检测到桌面应用。请检查安装程序提示后重试。",
            spec.desktop.name
        ))
    })();
    let _ = std::fs::remove_file(&path);
    result
}

fn download_desktop_installer(
    spec: &ToolSpec,
    installer: DirectDesktopInstaller,
    window: &Option<tauri::Window>,
) -> Result<PathBuf, String> {
    let target = std::env::temp_dir().join(format!(
        "stacker-{}-{}-{}",
        spec.id,
        chrono::Local::now().timestamp_millis(),
        installer.file_name
    ));
    let result = (|| {
        let proxy_status = crate::proxy::status();
        if proxy_status.enabled {
            emit_progress(
                window,
                format!(
                    "正在通过全局代理 {}:{} 连接官方下载地址…",
                    proxy_status.host, proxy_status.port
                ),
            );
        }
        let mut response = None;
        let mut last_error = None;
        for attempt in 1..=3 {
            if crate::installer::op_cancelled() {
                return Err(format!("已取消下载 {}", spec.desktop.name));
            }
            if attempt > 1 {
                emit_progress(window, format!("连接中断，正在进行第 {attempt} 次尝试…"));
                std::thread::sleep(Duration::from_millis(800));
            }
            let agent = desktop_download_agent(&proxy_status)?;
            match agent
                .get(installer.url)
                .set("User-Agent", "Stacker")
                .set("Accept", "application/octet-stream")
                .call()
            {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(err) => last_error = Some(err.to_string()),
            }
        }
        let response = response.ok_or_else(|| {
            format!(
                "连接 {} 官方下载地址失败：{}",
                spec.desktop.name,
                last_error.unwrap_or_else(|| "连接无响应".into())
            )
        })?;
        let total = response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let mut reader = response.into_reader();
        let mut output =
            std::fs::File::create(&target).map_err(|e| format!("创建安装程序临时文件失败：{e}"))?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut received = 0u64;
        let mut last_reported = 0u64;
        loop {
            if crate::installer::op_cancelled() {
                return Err(format!("已取消下载 {}", spec.desktop.name));
            }
            let count = reader
                .read(&mut buffer)
                .map_err(|e| format!("下载 {} 时连接中断：{e}", spec.desktop.name))?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .map_err(|e| format!("保存安装程序失败：{e}"))?;
            received += count as u64;
            if received.saturating_sub(last_reported) >= 512 * 1024 {
                last_reported = received;
                let progress = if total > 0 {
                    format!(
                        "正在下载 {:.0}% · {:.1}/{:.1} MB",
                        received as f64 * 100.0 / total as f64,
                        received as f64 / 1_048_576.0,
                        total as f64 / 1_048_576.0
                    )
                } else {
                    format!("已下载 {:.1} MB", received as f64 / 1_048_576.0)
                };
                emit_progress(window, progress);
            }
        }
        output
            .flush()
            .map_err(|e| format!("保存安装程序失败：{e}"))?;
        if received < 1_048_576 {
            return Err("下载到的安装程序不完整，已停止安装。".into());
        }
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&target);
    }
    result
}

fn desktop_download_agent(status: &crate::proxy::ProxyStatus) -> Result<ureq::Agent, String> {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30));
    if status.enabled {
        let address = if status.http.trim().is_empty() {
            format!("http://{}:{}", status.host, status.port)
        } else {
            status.http.clone()
        };
        let proxy = ureq::Proxy::new(&address)
            .map_err(|e| format!("全局代理地址无效（{address}）：{e}"))?;
        builder = builder.proxy(proxy);
    }
    Ok(builder.build())
}

fn verify_desktop_installer_signature(path: &Path) -> Result<String, String> {
    let path = ps_single_quoted(&path.to_string_lossy());
    let script = format!(
        "$s=Get-AuthenticodeSignature -LiteralPath {path}; if ($s.Status -ne 'Valid') {{ throw ('数字签名状态：' + $s.Status) }}; $s.SignerCertificate.Subject"
    );
    let output = run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "安装程序签名验证",
        Duration::from_secs(30),
    )?;
    first_output_line(&output).ok_or_else(|| "安装程序没有有效的发布者签名。".into())
}

fn run_downloaded_desktop_installer(
    spec: &ToolSpec,
    installer: DirectDesktopInstaller,
    path: &Path,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let mut command = Command::new(path);
    command.args(installer.silent_args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 {} 安装程序失败：{e}", spec.desktop.name))?;
    let started = Instant::now();
    let mut last_reported = u64::MAX;
    loop {
        if crate::installer::op_cancelled() {
            terminate_command_tree(&mut child);
            let _ = child.wait();
            return Err(format!("已取消安装 {}", spec.desktop.name));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "{} 安装未完成，安装程序退出代码：{}",
                    spec.desktop.name,
                    status.code().unwrap_or(-1)
                ))
            }
            Ok(None) => {
                let elapsed = started.elapsed().as_secs();
                if elapsed >= 1200 {
                    terminate_command_tree(&mut child);
                    let _ = child.wait();
                    return Err(format!(
                        "{} 安装超过 20 分钟，已停止操作。",
                        spec.desktop.name
                    ));
                }
                if elapsed != last_reported {
                    last_reported = elapsed;
                    emit_progress(
                        window,
                        format!("正在安装 {} · 已 {} 秒", spec.desktop.name, elapsed),
                    );
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => return Err(format!("读取 {} 安装状态失败：{e}", spec.desktop.name)),
        }
    }
}

fn run_codex_installer() -> Result<String, String> {
    run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://chatgpt.com/codex/install.ps1 | iex",
        ],
        "Codex Windows Installer",
        Duration::from_secs(900),
    )
    .map(|_| "Codex CLI 已安装".into())
}

fn install_or_update_antigravity_cli(
    window: &Option<tauri::Window>,
    action: &str,
) -> Result<String, String> {
    emit_progress(window, format!("正在通过官方脚本{action} Antigravity CLI…"));
    run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://antigravity.google/cli/install.ps1 | iex",
        ],
        "Antigravity CLI Install",
        Duration::from_secs(900),
    )?;
    Ok(format!("Antigravity CLI 已{action}"))
}

fn npm_install_latest(package: &str, installed_program: Option<&Path>) -> Result<(), String> {
    let npm = npm_for_program(installed_program)
        .or_else(|| resolve_command(&["npm.cmd", "npm.exe", "npm.bat"]))
        .ok_or_else(|| "未检测到 npm。请先在 Node 页面安装并设置默认 Node。".to_string())?;
    let spec = format!("{package}@latest");
    run_command_text(
        &npm,
        &["install", "-g", &spec],
        "npm install",
        Duration::from_secs(900),
    )?;
    Ok(())
}

fn npm_uninstall(package: &str, installed_program: Option<&Path>) -> Result<(), String> {
    let npm = npm_for_program(installed_program)
        .or_else(|| resolve_command(&["npm.cmd", "npm.exe", "npm.bat"]))
        .ok_or_else(|| "未检测到 npm。".to_string())?;
    run_command_text(
        &npm,
        &["uninstall", "-g", package],
        "npm uninstall",
        Duration::from_secs(900),
    )?;
    Ok(())
}

fn npm_for_program(program: Option<&Path>) -> Option<PathBuf> {
    let program = program?;
    let dir = program.parent()?;
    for name in ["npm.cmd", "npm.exe", "npm.bat"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn run_powershell(args: &[&str], name: &str, timeout: Duration) -> Result<String, String> {
    let program = resolve_command_including_windowsapps(&["powershell.exe", "powershell.cmd"])
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    run_command_text(&program, args, name, timeout)
}

fn ps_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn uninstall_appx_package(package_full_name: &str) -> Result<(), String> {
    let package = ps_single_quoted(package_full_name);
    let script = format!("Remove-AppxPackage -Package {package} -ErrorAction Stop");
    run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "Remove-AppxPackage",
        Duration::from_secs(120),
    )
    .map(|_| ())
}

fn run_command_text(
    program: &Path,
    args: &[&str],
    display_name: &str,
    timeout: Duration,
) -> Result<String, String> {
    let mut cmd = command_for_path(program, args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    apply_fresh_path(&mut cmd);
    let out = command_output_timeout_named(cmd, display_name, timeout)?;
    let text = output_all_text(&out);
    if !out.status.success() {
        return Err(first_output_line(&text).unwrap_or_else(|| format!("{display_name} 执行失败")));
    }
    Ok(text)
}

fn detect_install_method(spec: &ToolSpec, program: Option<&Path>) -> Option<String> {
    let program = program?;
    let p = program.to_string_lossy().replace('/', "\\").to_lowercase();
    if let Some(id) = spec.cli.winget_id {
        if (p.contains("\\winget\\links\\") || winget_package_installed(id))
            && !p.contains("\\node_modules\\")
        {
            return Some("winget".into());
        }
    }
    if spec.id == "claude"
        && (p.contains("\\.local\\bin\\claude") || p.contains("\\.local\\share\\claude\\"))
    {
        return Some("native".into());
    }
    if spec.id == "codex" && (p.contains("\\.codex\\") || p.contains("\\.local\\bin\\codex")) {
        return Some("native".into());
    }
    if spec.id == "antigravity"
        && (p.contains("\\antigravity\\")
            || p.contains("\\.local\\bin\\agy")
            || p.contains("\\agy\\bin\\agy"))
    {
        return Some("native".into());
    }
    if spec.id == "hermes"
        && (p.contains("\\appdata\\local\\hermes\\") || p.contains("\\.hermes\\"))
    {
        return Some("native".into());
    }
    if spec.id == "opencode" {
        if p.contains("\\scoop\\shims\\") || p.contains("\\scoop\\apps\\opencode\\") {
            return Some("scoop".into());
        }
        if p.contains("\\chocolatey\\bin\\") || p.contains("\\chocolatey\\lib\\opencode\\") {
            return Some("chocolatey".into());
        }
        if p.contains("\\.opencode\\bin\\") {
            return Some("native".into());
        }
    }
    if let Some(pkg) = spec.cli.npm_package {
        if is_conda_path(&p) && is_npm_shim(program, pkg) {
            return Some("conda-npm".into());
        }
        if is_npm_shim(program, pkg) {
            return Some("npm".into());
        }
    }
    None
}

fn install_method_label(method: &str) -> Option<String> {
    match method {
        "winget" => Some("WinGet".into()),
        "npm" => Some("npm".into()),
        "native" => Some("官方安装".into()),
        "scoop" => Some("Scoop".into()),
        "chocolatey" => Some("Chocolatey".into()),
        "conda-npm" => Some("Conda npm".into()),
        "appx" => Some("应用商店版".into()),
        "shortcut" => Some("快捷方式".into()),
        "registry" => Some("安装程序版".into()),
        "app" => Some("本地应用".into()),
        "download" => Some("官方下载".into()),
        _ => None,
    }
}

fn is_conda_path(path: &str) -> bool {
    path.contains("\\anaconda")
        || path.contains("\\miniconda")
        || path.contains("\\mambaforge")
        || path.contains("\\miniforge")
        || path.contains("\\conda\\envs\\")
        || path.contains("\\envs\\")
}

fn is_npm_shim(program: &Path, npm_package: &str) -> bool {
    let p = program.to_string_lossy().replace('/', "\\").to_lowercase();
    if p.contains("\\node_modules\\")
        || p.contains("\\npm\\")
        || p.contains("\\node_global\\")
        || p.contains("\\npm-global\\")
    {
        return true;
    }
    let pkg = npm_package.to_lowercase();
    std::fs::read_to_string(program)
        .map(|s| s.to_lowercase().contains(&pkg) || s.to_lowercase().contains("node_modules"))
        .unwrap_or(false)
}

fn latest_for_cli(
    spec: &ToolSpec,
    method: Option<&str>,
    current: Option<&str>,
) -> Result<Option<String>, String> {
    if let (Some("winget"), Some(id)) = (method, spec.cli.winget_id) {
        return Ok(winget_available_update(id)?.or_else(|| current.map(|s| s.to_string())));
    }
    if spec.id == "antigravity" || spec.id == "hermes" {
        return Ok(current.map(|s| s.to_string()));
    }
    if method == Some("native") && spec.id == "claude" {
        return Ok(current.map(|s| s.to_string()));
    }
    if let Some(pkg) = spec.cli.npm_package {
        return npm_latest(pkg).map(Some);
    }
    Ok(current.map(|s| s.to_string()))
}

fn desktop_internal_latest(spec: &ToolSpec, current: Option<&str>) -> Option<String> {
    match spec.id {
        "claude" => claude_desktop_ready_update(current),
        _ => None,
    }
}

fn claude_desktop_ready_update(current: Option<&str>) -> Option<String> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let candidates = [
        local.join("Claude-3p\\Logs\\main.log"),
        local.join("Claude-3p\\logs\\main.log"),
    ];
    let mut latest_ready: Option<String> = None;
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(version) = parse_claude_ready_update_version(line) {
                latest_ready = Some(version);
            }
        }
    }
    latest_ready.filter(|next| {
        current
            .map(|cur| crate::update::ver_lt(cur, next))
            .unwrap_or(true)
    })
}

fn parse_claude_ready_update_version(line: &str) -> Option<String> {
    if !line.contains("Update downloaded and ready to install") {
        return None;
    }
    let marker = "releaseName: 'Claude ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('\'')?;
    let version = rest[..end].trim();
    (!version.is_empty()).then(|| version.to_string())
}

fn detect_desktop_app(spec: &DesktopSpec) -> Option<DesktopFound> {
    desktop_appx_package(spec)
        .or_else(|| desktop_registry(spec))
        .or_else(|| desktop_start_menu_shortcut(spec))
        .or_else(|| desktop_exe_candidate(spec))
        .or_else(|| {
            spec.winget_id.and_then(|id| {
                winget_package_installed(id).then(|| DesktopFound {
                    path: None,
                    version: None,
                    method: Some("winget".into()),
                    uninstall: None,
                    launch: None,
                })
            })
        })
}

#[cfg(windows)]
fn desktop_appx_package(spec: &DesktopSpec) -> Option<DesktopFound> {
    if spec.appx_names.is_empty() {
        return None;
    }
    let names = spec
        .appx_names
        .iter()
        .map(|name| ps_single_quoted(name))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$names = @({names})
foreach ($name in $names) {{
  $pkg = Get-AppxPackage -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($pkg) {{
    $app = Get-StartApps | Where-Object {{ $_.AppID -like "$($pkg.PackageFamilyName)!*" }} | Select-Object -First 1
    $appId = if ($app) {{ $app.AppID }} else {{ "$($pkg.PackageFamilyName)!App" }}
    Write-Output ("{{0}}`t{{1}}`t{{2}}`t{{3}}`t{{4}}" -f $pkg.Name, $pkg.Version, $pkg.InstallLocation, $appId, $pkg.PackageFullName)
    break
  }}
}}
"#
    );
    let text = run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "Get-AppxPackage",
        Duration::from_secs(8),
    )
    .ok()?;
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let parts: Vec<&str> = line.splitn(5, '\t').collect();
    if parts.len() < 4 {
        return None;
    }
    let version = (!parts[1].trim().is_empty()).then(|| parts[1].trim().to_string());
    let install = parts[2].trim();
    let app_id = parts[3].trim();
    let package_full_name = parts.get(4).map(|s| s.trim()).unwrap_or_default();
    let path = (!install.is_empty()).then(|| PathBuf::from(install));
    let launch = (!app_id.is_empty()).then(|| format!("shell:AppsFolder\\{app_id}"));
    let uninstall = (!package_full_name.is_empty()).then(|| format!("appx:{package_full_name}"));
    Some(DesktopFound {
        path,
        version,
        method: Some("appx".into()),
        uninstall,
        launch,
    })
}

#[cfg(not(windows))]
fn desktop_appx_package(_: &DesktopSpec) -> Option<DesktopFound> {
    None
}

#[cfg(windows)]
fn desktop_registry(spec: &DesktopSpec) -> Option<DesktopFound> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;
    let paths = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for path in paths {
            let Ok(uninstall) = root.open_subkey_with_flags(path, KEY_READ) else {
                continue;
            };
            for name in uninstall.enum_keys().flatten() {
                let Ok(key) = uninstall.open_subkey_with_flags(&name, KEY_READ) else {
                    continue;
                };
                let display: String = key.get_value("DisplayName").unwrap_or_default();
                if !desktop_name_matches(&display, spec.keywords, spec.excludes) {
                    continue;
                }
                let version: Option<String> = key.get_value("DisplayVersion").ok();
                let quiet: Option<String> = key.get_value("QuietUninstallString").ok();
                let normal: Option<String> = key.get_value("UninstallString").ok();
                let uninstall_string = quiet.or(normal);
                let icon: Option<String> = key.get_value("DisplayIcon").ok();
                let install_location: Option<String> = key.get_value("InstallLocation").ok();
                let icon_file = icon.as_deref().and_then(parse_registered_file);
                let uninstall_file = uninstall_string
                    .as_deref()
                    .and_then(executable_from_command);
                let path = icon_file
                    .as_ref()
                    .filter(|path| is_launchable_desktop_exe(path, spec.keywords, spec.excludes))
                    .cloned()
                    .or_else(|| {
                        install_location
                            .as_deref()
                            .map(str::trim)
                            .filter(|location| !location.is_empty())
                            .and_then(|location| {
                                find_exe_in_dir(
                                    &PathBuf::from(location),
                                    spec.keywords,
                                    spec.excludes,
                                )
                            })
                    })
                    .or_else(|| {
                        icon_file
                            .as_deref()
                            .and_then(Path::parent)
                            .and_then(|dir| find_exe_in_dir(dir, spec.keywords, spec.excludes))
                    })
                    .or_else(|| {
                        uninstall_file
                            .as_deref()
                            .and_then(Path::parent)
                            .and_then(|dir| find_exe_in_dir(dir, spec.keywords, spec.excludes))
                    });
                return Some(DesktopFound {
                    path,
                    version,
                    method: Some("registry".into()),
                    uninstall: uninstall_string,
                    launch: None,
                });
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn desktop_registry(_: &DesktopSpec) -> Option<DesktopFound> {
    None
}

fn desktop_start_menu_shortcut(spec: &DesktopSpec) -> Option<DesktopFound> {
    for root in start_menu_roots() {
        if let Some(path) = find_shortcut_recursive(&root, spec.keywords, spec.excludes, 4) {
            return Some(DesktopFound {
                path: Some(path),
                version: None,
                method: Some("shortcut".into()),
                uninstall: None,
                launch: None,
            });
        }
    }
    None
}

fn desktop_exe_candidate(spec: &DesktopSpec) -> Option<DesktopFound> {
    for path in desktop_candidate_paths(spec) {
        if path.is_file() {
            return Some(DesktopFound {
                path: Some(path),
                version: None,
                method: Some("app".into()),
                uninstall: None,
                launch: None,
            });
        }
    }
    None
}

fn desktop_name_matches(name: &str, keywords: &[&str], excludes: &[&str]) -> bool {
    let lower = name.to_lowercase();
    keywords.iter().any(|k| lower.contains(&k.to_lowercase()))
        && !excludes.iter().any(|k| lower.contains(&k.to_lowercase()))
}

fn parse_registered_file(value: &str) -> Option<PathBuf> {
    let mut s = value.trim().trim_matches('"').to_string();
    if let Some(idx) = s.rfind(',') {
        if s[idx + 1..].chars().all(|c| c == '-' || c.is_ascii_digit()) {
            s.truncate(idx);
        }
    }
    let p = PathBuf::from(s.trim().trim_matches('"'));
    p.is_file().then_some(p)
}

fn executable_from_command(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let executable = if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        &rest[..end]
    } else {
        let lower = value.to_ascii_lowercase();
        let end = lower.find(".exe")? + ".exe".len();
        &value[..end]
    };
    let path = PathBuf::from(executable.trim());
    path.is_file().then_some(path)
}

fn is_launchable_desktop_exe(path: &Path, keywords: &[&str], excludes: &[&str]) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lower = name.to_lowercase();
    if !lower.ends_with(".exe")
        || [
            "uninstall",
            "unins",
            "installer",
            "setup",
            "update",
            "crashpad",
            "helper",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return false;
    }
    desktop_name_matches(&lower, keywords, excludes)
}

fn find_exe_in_dir(dir: &Path, keywords: &[&str], excludes: &[&str]) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut fallback = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_launchable_desktop_exe(&path, keywords, excludes) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_lowercase()
            .replace([' ', '-', '_'], "");
        if keywords
            .iter()
            .any(|keyword| stem == keyword.to_lowercase().replace([' ', '-', '_'], ""))
        {
            return Some(path);
        }
        fallback.get_or_insert(path);
    }
    fallback
}

fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        roots.push(PathBuf::from(programdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    roots
}

fn find_shortcut_recursive(
    dir: &Path,
    keywords: &[&str],
    excludes: &[&str],
    depth: usize,
) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_shortcut_recursive(&path, keywords, excludes, depth - 1) {
                return Some(found);
            }
            continue;
        }
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        if name.ends_with(".lnk") && desktop_name_matches(&name, keywords, excludes) {
            return Some(path);
        }
    }
    None
}

fn desktop_candidate_paths(spec: &DesktopSpec) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let pf86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
    let mut add = |base: &Option<PathBuf>, rest: &str| {
        if let Some(base) = base {
            out.push(base.join(rest));
        }
    };
    match spec.name {
        name if name.contains("Claude") => {
            add(&local, "Programs\\Claude\\Claude.exe");
            add(&pf, "Claude\\Claude.exe");
        }
        name if name.contains("Codex") => {
            add(&local, "Programs\\Codex\\Codex.exe");
            add(&pf, "Codex\\Codex.exe");
        }
        name if name.contains("Antigravity") => {
            add(&local, "Programs\\Antigravity\\Antigravity.exe");
            add(&local, "Google\\Antigravity\\Application\\antigravity.exe");
            add(&pf, "Google\\Antigravity\\Application\\antigravity.exe");
            add(&pf86, "Google\\Antigravity\\Application\\antigravity.exe");
        }
        name if name.contains("OpenCode") => {
            add(&local, "Programs\\OpenCode\\OpenCode.exe");
            add(&pf, "OpenCode\\OpenCode.exe");
        }
        name if name.contains("ZCode") => {
            add(&local, "Programs\\ZCode\\ZCode.exe");
            add(&pf, "ZCode\\ZCode.exe");
        }
        name if name.contains("WorkBuddy") => {
            add(&local, "Programs\\WorkBuddy\\WorkBuddy.exe");
            add(&local, "WorkBuddy\\WorkBuddy.exe");
            add(&pf, "WorkBuddy\\WorkBuddy.exe");
        }
        name if name.contains("Qoder") => {
            add(&local, "Programs\\Qoder\\Qoder.exe");
            add(&pf, "Qoder\\Qoder.exe");
        }
        name if name.contains("TRAE Work") => {
            add(&local, "Programs\\TRAE Work\\TRAE Work.exe");
            add(&local, "TRAE Work\\TRAE Work.exe");
            add(&pf, "TRAE Work\\TRAE Work.exe");
        }
        name if name.contains("OpenClaw") => {
            add(&local, "Programs\\OpenClaw\\OpenClaw.exe");
            add(&local, "OpenClaw\\OpenClaw.exe");
            add(&pf, "OpenClaw\\OpenClaw.exe");
        }
        name if name.contains("Hermes") => {
            add(&local, "Programs\\Hermes\\Hermes.exe");
            add(&local, "hermes\\desktop\\Hermes.exe");
            add(&local, "hermes\\Hermes.exe");
            add(&pf, "Hermes\\Hermes.exe");
        }
        _ => {}
    }
    out
}

fn remove_cli_binary(program: &Path, command_name: &str) -> Result<(), String> {
    let path = program
        .canonicalize()
        .unwrap_or_else(|_| program.to_path_buf());
    let file_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if file_name != command_name.to_lowercase() {
        return Err("命令文件名不匹配，已取消卸载。".into());
    }
    let p = path.to_string_lossy().to_lowercase();
    let user = std::env::var_os("USERPROFILE")
        .map(|s| PathBuf::from(s).to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let local = std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if (!user.is_empty() && p.starts_with(&user)) || (!local.is_empty() && p.starts_with(&local)) {
        std::fs::remove_file(&path).map_err(|e| format!("删除命令文件失败：{e}"))?;
        Ok(())
    } else {
        Err("命令不在当前用户目录内，为避免误删已取消卸载。".into())
    }
}

fn run_uninstall_string(uninstall: &str) -> Result<(), String> {
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/d", "/c", uninstall]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.spawn().map_err(|e| format!("启动卸载程序失败：{e}"))?;
    Ok(())
}

fn winget_args(action: &str, id: &str, source: Option<&str>, exact: bool) -> Vec<String> {
    let mut args = vec![action.to_string(), "--id".into(), id.into()];
    if exact {
        args.push("--exact".into());
    }
    if let Some(source) = source {
        args.push("--source".into());
        args.push(source.into());
    }
    args.push("--accept-source-agreements".into());
    args.push("--disable-interactivity".into());
    if action == "install" || action == "upgrade" {
        args.push("--accept-package-agreements".into());
        args.push("--silent".into());
    }
    args
}

fn winget_command() -> Option<PathBuf> {
    resolve_command_including_windowsapps(&["winget.exe", "winget.cmd", "winget.bat"])
        .or_else(|| known_winget_paths().into_iter().find(|p| p.exists()))
}

fn known_winget_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("Microsoft\\WindowsApps\\winget.exe"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        out.push(PathBuf::from(profile).join("AppData\\Local\\Microsoft\\WindowsApps\\winget.exe"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let windows_apps = PathBuf::from(program_files).join("WindowsApps");
        if let Ok(entries) = std::fs::read_dir(windows_apps) {
            let mut app_installer_paths = entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    name.starts_with("Microsoft.DesktopAppInstaller_")
                        .then(|| entry.path().join("winget.exe"))
                })
                .filter(|path| path.exists())
                .collect::<Vec<_>>();
            app_installer_paths.sort();
            app_installer_paths.reverse();
            out.extend(app_installer_paths);
        }
    }
    out
}

fn scoop_command() -> Option<PathBuf> {
    resolve_command_including_windowsapps(&["scoop.cmd", "scoop.exe", "scoop.bat"])
}

fn choco_command() -> Option<PathBuf> {
    resolve_command_including_windowsapps(&["choco.exe", "choco.cmd", "choco.bat"])
}

fn run_winget(args: &[&str], timeout: Duration) -> Result<String, String> {
    let winget = winget_command().ok_or_else(|| "未检测到 WinGet。".to_string())?;
    run_command_text(&winget, args, "winget", timeout)
}

fn run_winget_owned(
    args: Vec<String>,
    timeout: Duration,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let winget = winget_command().ok_or_else(|| "未检测到 WinGet。".to_string())?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_command_streamed(
        &winget,
        &refs,
        "WinGet",
        timeout,
        Duration::from_secs(30),
        window,
    )
}

fn run_command_streamed(
    program: &Path,
    args: &[&str],
    display_name: &str,
    timeout: Duration,
    stall_timeout: Duration,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    use std::io::Read;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    let mut cmd = command_for_path(program, args);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    apply_fresh_path(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 {display_name} 失败：{e}"))?;

    let started = Instant::now();
    let last_activity = Arc::new(AtomicU64::new(0));
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));

    let spawn_reader = |mut reader: Box<dyn Read + Send>, win: Option<tauri::Window>| {
        let activity = last_activity.clone();
        let captured = output.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 1024];
            let mut line = Vec::new();
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                activity.store(
                    started.elapsed().as_millis().max(1) as u64,
                    Ordering::Relaxed,
                );
                if let Ok(mut all) = captured.lock() {
                    all.extend_from_slice(&chunk[..n]);
                }
                for &byte in &chunk[..n] {
                    if byte == b'\r' || byte == b'\n' {
                        emit_command_progress(&win, &line);
                        line.clear();
                    } else {
                        line.push(byte);
                    }
                }
            }
            emit_command_progress(&win, &line);
        })
    };

    let stdout = child.stdout.take().ok_or("无法读取 WinGet 输出")?;
    let stderr = child.stderr.take().ok_or("无法读取 WinGet 错误输出")?;
    let stdout_reader = spawn_reader(Box::new(stdout), window.clone());
    let stderr_reader = spawn_reader(Box::new(stderr), window.clone());
    let mut last_heartbeat = Instant::now();

    let status = loop {
        if crate::installer::op_cancelled() {
            terminate_command_tree(&mut child);
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("已取消操作".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    terminate_command_tree(&mut child);
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(format!("{display_name} 执行超时，请检查网络后重试"));
                }
                let activity_ms = last_activity.load(Ordering::Relaxed);
                let inactive = if activity_ms == 0 {
                    elapsed
                } else {
                    elapsed.saturating_sub(Duration::from_millis(activity_ms))
                };
                if inactive >= stall_timeout {
                    terminate_command_tree(&mut child);
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(format!(
                        "{display_name} 连续 {} 秒没有响应，已停止操作。请检查网络或 WinGet 软件源后重试",
                        stall_timeout.as_secs()
                    ));
                }
                if last_heartbeat.elapsed() >= Duration::from_secs(1) {
                    emit_progress(
                        window,
                        format!("{display_name} 正在处理 · 已 {} 秒", elapsed.as_secs()),
                    );
                    last_heartbeat = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                terminate_command_tree(&mut child);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("读取 {display_name} 状态失败：{e}"));
            }
        }
    };

    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    let bytes = output.lock().map(|data| data.clone()).unwrap_or_default();
    let text = decode_command_bytes(&bytes).trim().to_string();
    if status.success() {
        Ok(text)
    } else {
        Err(first_output_line(&text).unwrap_or_else(|| format!("{display_name} 执行失败")))
    }
}

fn emit_command_progress(window: &Option<tauri::Window>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let decoded = decode_command_bytes(bytes);
    let cleaned = decoded
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\t')
        .collect::<String>();
    let line = cleaned.trim();
    if !line.is_empty() {
        emit_progress(window, line);
    }
}

fn terminate_command_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut taskkill = Command::new("taskkill.exe");
        taskkill.args(["/PID", &child.id().to_string(), "/T", "/F"]);
        taskkill.creation_flags(0x08000000);
        let _ = taskkill.output();
    }
    let _ = child.kill();
}

fn run_scoop(args: &[&str], timeout: Duration) -> Result<String, String> {
    let scoop = scoop_command().ok_or_else(|| "未检测到 Scoop。".to_string())?;
    run_command_text(&scoop, args, "scoop", timeout)
}

fn run_choco(args: &[&str], timeout: Duration) -> Result<String, String> {
    let choco = choco_command().ok_or_else(|| "未检测到 Chocolatey。".to_string())?;
    run_command_text(&choco, args, "choco", timeout)
}

fn winget_package_installed(id: &str) -> bool {
    run_winget(
        &["list", "--id", id, "--exact", "--accept-source-agreements"],
        Duration::from_secs(10),
    )
    .map(|text| text.to_lowercase().contains(&id.to_lowercase()))
    .unwrap_or(false)
}

fn winget_available_update(id: &str) -> Result<Option<String>, String> {
    let text = run_winget(
        &["upgrade", "--id", id, "--accept-source-agreements"],
        Duration::from_secs(15),
    )?;
    let id_lower = id.to_lowercase();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(pos) = parts
            .iter()
            .position(|part| part.to_lowercase() == id_lower)
        {
            if let Some(available) = parts.get(pos + 2) {
                return Ok(Some((*available).to_string()));
            }
        }
    }
    Ok(None)
}

fn build_environment_prompt() -> Result<String, String> {
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

fn npm_latest(package: &str) -> Result<String, String> {
    let package = package.replace('@', "%40").replace('/', "%2F");
    let url = format!("https://registry.npmjs.org/{package}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(1500))
        .timeout_read(Duration::from_millis(1500))
        .build();
    let body = agent
        .get(&url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    v.get("dist-tags")
        .and_then(|d| d.get("latest"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "npm registry 未返回 latest 版本".into())
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
        let lower = dir.to_string_lossy().to_lowercase();
        if lower.contains("\\windowsapps") {
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

fn resolve_command_including_windowsapps(candidates: &[&str]) -> Option<PathBuf> {
    for dir in command_dirs() {
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

fn apply_fresh_path(c: &mut Command) {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        c.env("PATH", path);
    }
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
    apply_fresh_path(&mut cmd);
    let out = command_output_timeout_named(cmd, display_name, timeout)?;
    let version_text = output_text(&out);
    if !out.status.success() {
        return Err(first_output_line(&version_text).unwrap_or_else(|| "命令返回失败状态".into()));
    }
    Ok(first_output_line(&version_text).unwrap_or_else(|| "可用".into()))
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

fn output_all_text(out: &Output) -> String {
    let mut text = String::new();
    let stdout = decode_command_bytes(&out.stdout);
    let stderr = decode_command_bytes(&out.stderr);
    if !stdout.trim().is_empty() {
        text.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim());
    }
    text
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

fn open_external_target(target: &str) -> Result<(), String> {
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

fn emit_progress<S: AsRef<str>>(window: &Option<tauri::Window>, msg: S) {
    if let Some(window) = window {
        let _ = window.emit(VIBE_PROGRESS_EVENT, msg.as_ref().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::is_launchable_desktop_exe;
    use std::path::Path;

    #[test]
    fn desktop_launcher_rejects_uninstaller_and_icon_files() {
        let keywords = &["zcode"];
        assert!(is_launchable_desktop_exe(
            Path::new(r"D:\AITools\ZCode\ZCode.exe"),
            keywords,
            &[],
        ));
        assert!(!is_launchable_desktop_exe(
            Path::new(r"D:\AITools\ZCode\Uninstall ZCode.exe"),
            keywords,
            &[],
        ));
        assert!(!is_launchable_desktop_exe(
            Path::new(r"D:\AITools\ZCode\uninstallerIcon.ico"),
            keywords,
            &[],
        ));
    }
}
