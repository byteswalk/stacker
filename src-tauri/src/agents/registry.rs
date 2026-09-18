#[derive(Clone)]
pub(crate) struct CliSpec {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) command: &'static str,
    pub(crate) candidates: &'static [&'static str],
    pub(crate) npm_package: Option<&'static str>,
    pub(crate) winget_id: Option<&'static str>,
    pub(crate) install_url: &'static str,
    pub(crate) docs_url: &'static str,
}

#[derive(Clone)]
pub(crate) struct DesktopSpec {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) winget_id: Option<&'static str>,
    pub(crate) winget_source: Option<&'static str>,
    pub(crate) appx_names: &'static [&'static str],
    pub(crate) install_url: &'static str,
    pub(crate) docs_url: &'static str,
    pub(crate) keywords: &'static [&'static str],
    pub(crate) excludes: &'static [&'static str],
}

#[derive(Clone)]
pub(crate) struct ToolSpec {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) docs_url: &'static str,
    pub(crate) cli: CliSpec,
    pub(crate) desktop: DesktopSpec,
}

pub(crate) fn tool_metadata(id: &str) -> (&'static str, &'static str, &'static str, u16) {
    match id {
        "claude" => ("claude", "global", "", 10),
        "codex" => ("codex", "global", "", 20),
        "antigravity" => ("antigravity", "global", "", 30),
        "opencode" => ("opencode", "global", "", 40),
        "zcode" => ("zcode", "global", "", 50),
        "kimi" => ("kimi", "unified", "", 60),
        "workbuddy" => ("workbuddy", "unified", "", 70),
        "qoder" => ("qoder", "global", "国际版", 80),
        "qoder-cn" => ("qoder", "cn", "中国版", 81),
        "trae-work" => ("trae", "cn", "中国版", 90),
        "trae-global" => ("trae", "global", "国际版", 91),
        "deepseek-harness" => ("deepseek-harness", "global", "开发者预览", 100),
        "openclaw" => ("openclaw", "global", "", 110),
        "hermes" => ("hermes", "global", "", 120),
        _ => ("other", "unified", "", 999),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DirectDesktopInstaller {
    pub(crate) url: &'static str,
    pub(crate) file_name: &'static str,
    pub(crate) silent_args: &'static [&'static str],
}

pub(crate) fn tool_specs() -> Vec<ToolSpec> {
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
            name: "Kimi",
            description: "Kimi 提供终端开发智能体和 Windows 本地工作智能体，可处理项目代码、文档与本地任务。",
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
                name: "Kimi Work 桌面端",
                description: "Kimi Work Windows 本地工作智能体，可处理本地文件、执行自动化任务并协助完成知识工作。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.kimi.com/products/kimi-work",
                docs_url: "https://www.kimi.com/products/kimi-work",
                keywords: &["kimi work", "kimi"],
                excludes: &["kimi code", "kimi-code"],
            },
        },
        ToolSpec {
            id: "workbuddy",
            name: "WorkBuddy",
            description: "腾讯 WorkBuddy 提供桌面工作智能体和 CodeBuddy CLI，支持本地任务、项目开发与终端自动化。",
            docs_url: "https://www.workbuddy.ai/cli",
            cli: CliSpec {
                name: "CodeBuddy CLI",
                description: "命令名 codebuddy，可在终端中分析项目、修改代码、运行命令并完成开发任务。",
                command: "codebuddy",
                candidates: &[
                    "codebuddy.exe",
                    "codebuddy.cmd",
                    "codebuddy.bat",
                    "codebuddy.ps1",
                    "codebuddy-code.exe",
                    "codebuddy-code.cmd",
                    "codebuddy-code.bat",
                    "codebuddy-code.ps1",
                    "cbc.exe",
                    "cbc.cmd",
                    "cbc.bat",
                    "cbc.ps1",
                ],
                npm_package: Some("@tencent-ai/codebuddy-code"),
                winget_id: None,
                install_url: "https://www.workbuddy.ai/cli",
                docs_url: "https://www.workbuddy.ai/cli",
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
            name: "Qoder 国际版",
            description: "Qoder 国际版，提供终端智能体与桌面 IDE。",
            docs_url: "https://docs.qoder.com/",
            cli: CliSpec {
                name: "Qoder CLI",
                description: "命令名 qoder，可在终端中执行代码理解、修改与自动化任务。",
                command: "qoder",
                candidates: &[
                    "qoder.exe",
                    "qoder.cmd",
                    "qoder.bat",
                    "qoder.ps1",
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
            id: "qoder-cn",
            name: "Qoder 中国版",
            description: "Qoder 中国版，使用中国站点提供终端智能体与桌面 IDE。",
            docs_url: "https://qoder.com.cn/",
            cli: CliSpec {
                name: "Qoder CLI（中国版）",
                description: "命令名 qoder，安装与更新优先使用中国站点。",
                command: "qoder",
                candidates: &["qoder.exe", "qoder.cmd", "qoder.bat", "qoder.ps1", "qodercli.exe", "qodercli.cmd", "qodercli.bat", "qodercli.ps1"],
                npm_package: Some("@qoder-ai/qodercli"),
                winget_id: None,
                install_url: "https://qoder.com.cn/download",
                docs_url: "https://qoder.com.cn/",
            },
            desktop: DesktopSpec {
                name: "Qoder 桌面端（中国版）",
                description: "Qoder 中国版 Windows 桌面 IDE。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://qoder.com.cn/download",
                docs_url: "https://qoder.com.cn/",
                keywords: &["qoder"],
                excludes: &["cli"],
            },
        },
        ToolSpec {
            id: "trae-work",
            name: "TRAE 中国版",
            description: "TRAE 中国版，提供终端开发智能体和 Windows 桌面智能体。",
            docs_url: "https://docs.trae.cn/cli_get-started-with-trae-cli",
            cli: CliSpec {
                name: "TRAE CLI",
                description: "命令名 traecli，可在终端中运行 TRAE 智能体、处理项目任务并管理开发工作流。",
                command: "traecli",
                candidates: &[
                    "traecli.exe",
                    "traecli.cmd",
                    "traecli.bat",
                    "traecli.ps1",
                    "trae-cli.exe",
                    "trae-cli.cmd",
                    "trae-cli.bat",
                    "trae-cli.ps1",
                    "trae-agent.exe",
                    "ta.exe",
                ],
                npm_package: None,
                winget_id: None,
                install_url: "https://docs.trae.cn/cli_get-started-with-trae-cli",
                docs_url: "https://docs.trae.cn/cli_get-started-with-trae-cli",
            },
            desktop: DesktopSpec {
                name: "TRAE Work 桌面端",
                description: "TRAE Work Windows 桌面应用，包含工作模式与代码模式。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.trae.cn/ide/download",
                docs_url: "https://www.trae.cn/",
                // The current Windows installer registers this edition as
                // "TraeWork CN" / "TRAE SOLO CN", not "TRAE Work".
                keywords: &["traework cn", "trae solo cn"],
                excludes: &["trae ide"],
            },
        },
        ToolSpec {
            id: "trae-global",
            name: "TRAE 国际版",
            description: "TRAE 国际版桌面开发智能体，面向全球站点和服务。",
            docs_url: "https://www.trae.ai/",
            cli: CliSpec {
                name: "TRAE CLI（国际版）",
                description: "国际版暂未提供可由 Stacker 验证的独立 Windows CLI 安装入口。",
                command: "",
                candidates: &[],
                npm_package: None,
                winget_id: None,
                install_url: "https://www.trae.ai/",
                docs_url: "https://www.trae.ai/",
            },
            desktop: DesktopSpec {
                name: "TRAE 桌面端（国际版）",
                description: "TRAE 国际版 Windows 桌面开发环境。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.trae.ai/download",
                docs_url: "https://www.trae.ai/",
                // The global desktop app is currently registered as
                // "TraeWork" / "TRAE SOLO". Keep the CN edition separate.
                keywords: &["traework", "trae solo"],
                excludes: &["traework cn", "trae solo cn", "trae cli"],
            },
        },
        ToolSpec {
            id: "deepseek-harness",
            name: "DeepSeek Harness",
            description: "DeepSeek 官方开发者预览 Harness，可在本地启动 Web 工作台并编排智能体任务。",
            docs_url: "https://www.deepseek.com/harness/",
            cli: CliSpec {
                name: "DeepSeek Harness CLI",
                description: "命令名 dsh；安装后可运行 dsh web 启动本地工作台。",
                command: "dsh",
                candidates: &["dsh.exe", "dsh.cmd", "dsh.bat", "dsh.ps1"],
                npm_package: Some("@deepseek-ai/dsh"),
                winget_id: None,
                install_url: "https://www.deepseek.com/harness/",
                docs_url: "https://github.com/deepseek-ai/deepseek-harness",
            },
            desktop: DesktopSpec {
                name: "DeepSeek Harness Web",
                description: "Harness 通过本地 Web 工作台运行，不提供独立 Windows 桌面安装包。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.deepseek.com/harness/",
                docs_url: "https://github.com/deepseek-ai/deepseek-harness",
                keywords: &[],
                excludes: &[],
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

pub(crate) fn spec_by_id(id: &str) -> Option<ToolSpec> {
    tool_specs().into_iter().find(|s| s.id == id)
}

pub(crate) fn direct_desktop_installer(id: &str) -> Option<DirectDesktopInstaller> {
    match id {
        "kimi" => Some(DirectDesktopInstaller {
            url: "https://appsupport.moonshot.cn/api/app/pkg/latest/windows/download",
            file_name: "Kimi-Work-Setup.exe",
            silent_args: &["/S"],
        }),
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
pub(crate) fn openclaw_desktop_installer_url() -> &'static str {
    "https://github.com/openclaw/openclaw/releases/latest/download/OpenClawCompanion-Setup-arm64.exe"
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn openclaw_desktop_installer_url() -> &'static str {
    "https://github.com/openclaw/openclaw/releases/latest/download/OpenClawCompanion-Setup-x64.exe"
}

pub(crate) fn desktop_install_unavailable_reason(spec: &ToolSpec) -> &'static str {
    match spec.id {
        "deepseek-harness" => {
            "DeepSeek Harness 需要按官方文档在本地工作目录中安装；完成后 Stacker 会识别本地工作台启动器。"
        }
        "trae-work" => {
            "TRAE Work 官网动态下发安装地址，当前无法可靠自动下载安装，请通过官方文档安装。"
        }
        "zcode" | "workbuddy" | "qoder" => {
            "尚未找到可稳定调用的官方 Windows 安装接口，请通过官方文档安装。"
        }
        _ => "官方未提供可自动安装的独立 Windows 应用。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_agent_surfaces_use_official_install_sources() {
        let kimi = spec_by_id("kimi").expect("Kimi catalog entry");
        assert_eq!(kimi.desktop.name, "Kimi Work 桌面端");
        assert!(!kimi.desktop.keywords.is_empty());
        assert!(direct_desktop_installer("kimi").is_some());

        let trae = spec_by_id("trae-work").expect("TRAE catalog entry");
        assert_eq!(trae.cli.command, "traecli");
        assert!(trae.cli.candidates.contains(&"traecli.exe"));
        assert_eq!(
            trae.cli.docs_url,
            "https://docs.trae.cn/cli_get-started-with-trae-cli"
        );

        let workbuddy = spec_by_id("workbuddy").expect("WorkBuddy catalog entry");
        assert_eq!(workbuddy.cli.name, "CodeBuddy CLI");
        assert_eq!(
            workbuddy.cli.npm_package,
            Some("@tencent-ai/codebuddy-code")
        );
        assert!(workbuddy.cli.candidates.contains(&"codebuddy.cmd"));
    }
}
