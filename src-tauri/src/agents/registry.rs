//! Single source of truth for the agents Stacker manages: products, shared CLIs,
//! vendors, icons, process signatures and data directories. No IO lives here.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Vendor {
    Claude,
    Codex,
    Antigravity,
    OpenCode,
    ZCode,
    Kimi,
    WorkBuddy,
    Qoder,
    Trae,
    DeepSeekHarness,
    OpenClaw,
    Hermes,
    Pi,
    Copilot,
    MiMo,
    Agnes,
    Xai,
    Cursor,
    Factory,
    Kiro,
    MiniMax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Edition {
    Cn,
    Global,
    Unified,
}

impl Edition {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Edition::Cn => "cn",
            Edition::Global => "global",
            Edition::Unified => "unified",
        }
    }
}

// Read by the sessions and storage-migration sub-projects; only registered for now.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum DataBase {
    Home,
    Roaming,
    Local,
}

/// An agent-owned data directory, registered for the sessions and migration sub-projects.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct DataDir {
    pub(crate) base: DataBase,
    pub(crate) relative: &'static str,
    pub(crate) env_override: Option<&'static str>,
}

const fn home(relative: &'static str) -> DataDir {
    DataDir {
        base: DataBase::Home,
        relative,
        env_override: None,
    }
}

const fn roaming(relative: &'static str) -> DataDir {
    DataDir {
        base: DataBase::Roaming,
        relative,
        env_override: None,
    }
}

const fn local(relative: &'static str) -> DataDir {
    DataDir {
        base: DataBase::Local,
        relative,
        env_override: None,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CliSpec {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) command: &'static str,
    pub(crate) candidates: &'static [&'static str],
    pub(crate) npm_package: Option<&'static str>,
    pub(crate) winget_id: Option<&'static str>,
    pub(crate) install_url: &'static str,
    pub(crate) docs_url: &'static str,
}

#[derive(Clone, Debug)]
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
    pub(crate) install_unavailable_reason: Option<&'static str>,
    /// A matched executable is ignored when its directory contains one of these files.
    /// Used to tell look-alike products apart, e.g. the Qoder IDE from the new Qoder app.
    pub(crate) reject_sibling_files: &'static [&'static str],
}

pub(crate) enum CliSlot {
    Shared(&'static str),
    Unavailable {
        name: &'static str,
        description: &'static str,
        url: &'static str,
    },
}

pub(crate) enum DesktopSlot {
    App(DesktopSpec),
    Unavailable {
        name: &'static str,
        description: &'static str,
        url: &'static str,
    },
}

pub(crate) struct ProductSpec {
    pub(crate) id: &'static str,
    pub(crate) vendor: Vendor,
    pub(crate) family: &'static str,
    pub(crate) edition: Edition,
    pub(crate) edition_label: &'static str,
    /// Default list order: popularity, from npm monthly downloads and GitHub stars (2026-09;
    /// Codex 78M, Claude Code 59M, OpenClaw 14M, Copilot 10M, pi 9M, OpenCode 9M, …). Tens
    /// per product; the China edition (+0) comes before the global one (+1).
    pub(crate) sort: u16,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) docs_url: &'static str,
    pub(crate) cli: CliSlot,
    pub(crate) cli_note: Option<&'static str>,
    pub(crate) desktop: DesktopSlot,
    /// A CLI subcommand that opens the agent's own local UI, e.g. `dsh web`.
    pub(crate) workbench_command: Option<&'static str>,
    /// PowerShell regex matched against "<process name> <command line>". Short vendor
    /// names must be anchored to a path segment or executable name.
    pub(crate) data_dirs: &'static [DataDir],
}

/// Resolved view used by detection and install code.
#[derive(Clone)]
pub(crate) struct ToolSpec {
    pub(crate) id: &'static str,
    pub(crate) vendor: Vendor,
    pub(crate) family: &'static str,
    pub(crate) edition: Edition,
    pub(crate) edition_label: &'static str,
    pub(crate) sort: u16,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) docs_url: &'static str,
    pub(crate) cli_id: Option<&'static str>,
    pub(crate) cli_note: Option<&'static str>,
    pub(crate) cli: CliSpec,
    pub(crate) desktop: DesktopSpec,
    pub(crate) desktop_available: bool,
    pub(crate) workbench_command: Option<&'static str>,
    #[allow(dead_code)]
    pub(crate) data_dirs: &'static [DataDir],
}

#[derive(Clone, Copy)]
pub(crate) enum InstallerSource {
    Fixed {
        url: &'static str,
        file_name: &'static str,
    },
    /// An electron-builder GitHub release whose `latest.yml` names the installer and its
    /// SHA-512; `base_url` is the release's `.../releases/latest/download`.
    ElectronRelease { base_url: &'static str },
    /// An electron-builder manifest at a full URL whose `path` is the installer's own URL
    /// (ZCode's release service).
    ElectronManifest { manifest_url: &'static str },
    /// Asked of the vendor's own update service when installing (WorkBuddy, TRAE, Qoder).
    Service {
        resolve: fn() -> Result<super::install::direct::ResolvedInstaller, String>,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct DirectDesktopInstaller {
    pub(crate) source: InstallerSource,
    pub(crate) silent_args: &'static [&'static str],
    /// Authenticode-signed installers are checked by signature; unsigned ones only run
    /// when they match the SHA-512 their release publishes.
    pub(crate) signed: bool,
}

pub(crate) const DEFAULT_DESKTOP_UNAVAILABLE_REASON: &str =
    "官方未提供可自动安装的独立 Windows 应用。";

impl DataDir {
    /// Where this folder is on this machine, or nothing when the base is unknown.
    pub(crate) fn path(&self) -> Option<std::path::PathBuf> {
        if let Some(name) = self.env_override {
            let set = crate::winenv::get_user_raw(name)
                .or_else(|| std::env::var(name).ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            if let Some(value) = set {
                return Some(std::path::PathBuf::from(value));
            }
        }
        let base = match self.base {
            DataBase::Home => dirs::home_dir(),
            DataBase::Roaming => dirs::data_dir(),
            DataBase::Local => dirs::data_local_dir(),
        }?;
        Some(
            self.relative
                .split('/')
                .fold(base, |path, part| path.join(part)),
        )
    }
}

pub(crate) fn cli_by_id(id: &str) -> Option<&'static CliSpec> {
    CLIS.iter().find(|cli| cli.id == id)
}

fn resolve(product: &'static ProductSpec) -> ToolSpec {
    let (cli_id, cli) = match product.cli {
        CliSlot::Shared(id) => (
            Some(id),
            cli_by_id(id)
                .expect("registry invariant: referenced cli exists")
                .clone(),
        ),
        CliSlot::Unavailable {
            name,
            description,
            url,
        } => (
            None,
            CliSpec {
                id: "",
                name,
                description,
                command: "",
                candidates: &[],
                npm_package: None,
                winget_id: None,
                install_url: url,
                docs_url: url,
            },
        ),
    };
    let (desktop_available, desktop) = match &product.desktop {
        DesktopSlot::App(spec) => (true, spec.clone()),
        DesktopSlot::Unavailable {
            name,
            description,
            url,
        } => (
            false,
            DesktopSpec {
                name,
                description,
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: url,
                docs_url: url,
                keywords: &[],
                excludes: &[],
                install_unavailable_reason: None,
                reject_sibling_files: &[],
            },
        ),
    };
    ToolSpec {
        id: product.id,
        vendor: product.vendor,
        family: product.family,
        edition: product.edition,
        edition_label: product.edition_label,
        sort: product.sort,
        name: product.name,
        description: product.description,
        icon: product.icon,
        docs_url: product.docs_url,
        cli_id,
        cli_note: product.cli_note,
        cli,
        desktop,
        desktop_available,
        workbench_command: product.workbench_command,
        data_dirs: product.data_dirs,
    }
}

pub(crate) fn tool_specs() -> Vec<ToolSpec> {
    PRODUCTS.iter().map(resolve).collect()
}

pub(crate) fn spec_by_id(id: &str) -> Option<ToolSpec> {
    PRODUCTS
        .iter()
        .find(|product| product.id == id)
        .map(resolve)
}

pub(crate) static CLIS: &[CliSpec] = &[
    CliSpec {
        id: "claude",
        name: "Claude Code CLI",
        description: "在终端中理解、修改和运行项目代码。",
        command: "claude",
        candidates: &["claude.exe", "claude.cmd", "claude.bat", "claude.ps1"],
        npm_package: Some("@anthropic-ai/claude-code"),
        winget_id: Some("Anthropic.ClaudeCode"),
        install_url: "https://docs.anthropic.com/en/docs/claude-code/setup",
        docs_url: "https://docs.anthropic.com/en/docs/claude-code/overview",
    },
    CliSpec {
        id: "codex",
        name: "Codex CLI",
        description: "在终端中运行 Codex，适合项目维护、自动化修改和脚本化任务。",
        command: "codex",
        candidates: &["codex.exe", "codex.cmd", "codex.bat", "codex.ps1"],
        npm_package: Some("@openai/codex"),
        winget_id: Some("OpenAI.Codex"),
        install_url: "https://developers.openai.com/codex/cli",
        docs_url: "https://developers.openai.com/codex/cli",
    },
    CliSpec {
        id: "agy",
        name: "Antigravity CLI",
        description: "命令名 agy，适合终端内运行 Google Antigravity agent 工作流。",
        command: "agy",
        candidates: &["agy.exe", "agy.cmd", "agy.bat", "agy.ps1"],
        npm_package: None,
        winget_id: Some("Google.AntigravityCLI"),
        install_url: "https://antigravity.google/docs/cli-install",
        docs_url: "https://antigravity.google/docs/cli/overview",
    },
    // xAI's official agent CLI. A community project, `grok-dev`, installs the same `grok`
    // command; detection refuses it, since it is neither this product nor xAI's.
    CliSpec {
        id: "grok",
        name: "Grok Build CLI",
        description: "xAI 官方的终端编程智能体，命令名 grok；需要 SuperGrok 或 X Premium+ 订阅。",
        command: "grok",
        candidates: &["grok.exe", "grok.cmd", "grok.bat", "grok.ps1"],
        npm_package: None,
        winget_id: None,
        install_url: "https://x.ai/cli",
        docs_url: "https://x.ai/cli",
    },
    // Cursor's own terminal agent. The installer also copies it to a bare `agent`, a name
    // too generic to detect by; `cursor-agent` is the one counted.
    CliSpec {
        id: "cursor",
        name: "Cursor CLI",
        description: "Cursor 官方的终端智能体，命令名 cursor-agent（另有 agent 别名）；需要 Cursor 订阅。",
        command: "cursor-agent",
        candidates: &["cursor-agent.exe", "cursor-agent.cmd", "cursor-agent.ps1"],
        npm_package: None,
        winget_id: None,
        install_url: "https://cursor.com/cli",
        docs_url: "https://cursor.com/docs/cli/overview",
    },
    CliSpec {
        id: "droid",
        name: "Droid CLI",
        description: "Factory 的终端编程智能体，命令名 droid，可拆分子任务交给专门的 Droid。",
        command: "droid",
        candidates: &["droid.exe"],
        npm_package: None,
        winget_id: None,
        install_url: "https://docs.factory.ai/cli/getting-started/quickstart",
        docs_url: "https://docs.factory.ai/cli/getting-started/overview",
    },
    CliSpec {
        id: "kiro",
        name: "Kiro CLI",
        description: "AWS Kiro 的终端智能体，命令名 kiro-cli；Windows 版只支持 Windows 11。",
        command: "kiro-cli",
        candidates: &["kiro-cli.exe"],
        npm_package: None,
        winget_id: None,
        install_url: "https://kiro.dev/cli/",
        docs_url: "https://kiro.dev/docs/cli/",
    },
    CliSpec {
        id: "mcode",
        name: "MiniMax Code CLI",
        description: "MiniMax 开源的终端编程智能体，命令名 mcode；也可以自带 OpenAI 或 Anthropic 兼容的 Key。",
        command: "mcode",
        candidates: &["mcode.cmd", "mcode.exe", "mcode.ps1"],
        npm_package: Some("@minimax-ai/code"),
        winget_id: None,
        install_url: "https://github.com/MiniMax-AI/minimax-code",
        docs_url: "https://github.com/MiniMax-AI/minimax-code",
    },
    CliSpec {
        id: "opencode",
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
    CliSpec {
        id: "kimi",
        name: "Kimi Code CLI",
        description: "命令名 kimi，适合在终端中开展完整的项目开发工作流。",
        command: "kimi",
        candidates: &["kimi.exe", "kimi.cmd", "kimi.bat", "kimi.ps1"],
        npm_package: Some("@moonshot-ai/kimi-code"),
        winget_id: None,
        install_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
        docs_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
    },
    CliSpec {
        id: "codebuddy",
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
    CliSpec {
        id: "qoder",
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
    CliSpec {
        id: "qodercn",
        name: "Qoder CLI 中国版",
        description: "命令名 qodercn，中国版 Qoder 的命令行，账号与模型清单和国际版不通用。",
        command: "qodercn",
        candidates: &[
            "qodercn.exe",
            "qodercn.cmd",
            "qodercn.bat",
            "qodercn.ps1",
            "qoderclicn.exe",
            "qoderclicn.cmd",
            "qoderclicn.bat",
            "qoderclicn.ps1",
        ],
        npm_package: Some("@qodercn-ai/qoderclicn"),
        winget_id: None,
        install_url: "https://help.aliyun.com/zh/lingma/qodercli-cn/user-guide/qoder-cli-cn-get-started-quickly",
        docs_url: "https://help.aliyun.com/zh/lingma/qodercli-cn/user-guide/using-the-cli",
    },
    CliSpec {
        id: "dsh",
        name: "DeepSeek Harness CLI",
        description: "命令名 dsh；安装后可运行 dsh web 启动本地工作台。",
        command: "dsh",
        candidates: &["dsh.exe", "dsh.cmd", "dsh.bat", "dsh.ps1"],
        npm_package: Some("@deepseek-ai/dsh"),
        winget_id: None,
        install_url: "https://www.deepseek.com/harness/",
        docs_url: "https://github.com/deepseek-ai/deepseek-harness",
    },
    CliSpec {
        id: "openclaw",
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
    CliSpec {
        id: "hermes",
        name: "Hermes CLI",
        description: "命令名 hermes，用于运行智能体、管理模型、技能、网关和本地自动化任务。",
        command: "hermes",
        candidates: &["hermes.exe", "hermes.cmd", "hermes.bat", "hermes.ps1"],
        npm_package: None,
        winget_id: None,
        install_url: "https://hermes-agent.nousresearch.com/docs/getting-started/installation",
        docs_url: "https://hermes-agent.nousresearch.com/docs/",
    },
    CliSpec {
        id: "pi",
        name: "pi",
        description: "Earendil（Mario Zechner）维护的极简终端编程智能体，命令名 pi。",
        command: "pi",
        candidates: &["pi.cmd", "pi.exe", "pi.bat", "pi.ps1"],
        npm_package: Some("@earendil-works/pi-coding-agent"),
        winget_id: None,
        install_url: "https://pi.dev/",
        docs_url: "https://pi.dev/",
    },
    CliSpec {
        id: "copilot",
        name: "Copilot CLI",
        description: "命令名 copilot，在终端中与 GitHub Copilot 编程智能体协作。",
        command: "copilot",
        candidates: &["copilot.cmd", "copilot.exe", "copilot.bat", "copilot.ps1"],
        npm_package: Some("@github/copilot"),
        winget_id: Some("GitHub.Copilot"),
        install_url: "https://docs.github.com/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli",
        docs_url: "https://docs.github.com/copilot/how-tos/copilot-cli",
    },
    CliSpec {
        id: "mimo",
        name: "MiMo Code CLI",
        description: "命令名 mimo，首次运行会引导登录和配置。",
        command: "mimo",
        candidates: &["mimo.exe", "mimo.cmd", "mimo.bat", "mimo.ps1"],
        npm_package: Some("@mimo-ai/cli"),
        winget_id: None,
        install_url: "https://mimo.xiaomi.com/coder",
        docs_url: "https://mimo.xiaomi.com/coder",
    },
];

pub(crate) static PRODUCTS: &[ProductSpec] = &[
    ProductSpec {
        id: "claude",
        vendor: Vendor::Claude,
        family: "claude",
        edition: Edition::Global,
        edition_label: "",
        sort: 20,
        name: "Claude Code",
        description: "Anthropic 的工作智能体工具，CLI 适合终端工作流，桌面端适合多会话与可视化审查。",
        icon: "claude.png",
        docs_url: "https://docs.anthropic.com/en/docs/claude-code/overview",
        cli: CliSlot::Shared("claude"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
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

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[
            DataDir { base: DataBase::Home, relative: ".claude", env_override: Some("CLAUDE_CONFIG_DIR") },
            DataDir { base: DataBase::Roaming, relative: "Claude", env_override: None },
            roaming("Claude Code"),
            local("Claude"),
            local("Claude-3p"),
            local("Claude-Data"),
            local("claude-cli-nodejs"),
        ],
    },
    ProductSpec {
        id: "codex",
        vendor: Vendor::Codex,
        family: "codex",
        edition: Edition::Global,
        edition_label: "",
        sort: 10,
        name: "Codex",
        description: "OpenAI 的本地工作智能体工具，CLI 适合终端自动化，桌面端适合线程、工作区和审查。",
        icon: "codex.png",
        docs_url: "https://developers.openai.com/codex/",
        cli: CliSlot::Shared("codex"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
                name: "Codex 桌面端",
                description: "OpenAI Codex 桌面应用，用于并行管理 Codex 线程和本地工作区。",
                winget_id: Some("9PLM9XGG6VKS"),
                winget_source: Some("msstore"),
                appx_names: &["OpenAI.CodexBeta", "OpenAI.Codex"],
                install_url: "https://developers.openai.com/codex/app/windows",
                docs_url: "https://developers.openai.com/codex/app",
                keywords: &["codex"],
                excludes: &["cli"],

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[
            DataDir { base: DataBase::Home, relative: ".codex", env_override: Some("CODEX_HOME") },
            DataDir { base: DataBase::Roaming, relative: "Codex", env_override: None },
            local("OpenAI"),
        ],
    },
    ProductSpec {
        id: "antigravity",
        vendor: Vendor::Antigravity,
        family: "antigravity",
        edition: Edition::Global,
        edition_label: "",
        sort: 80,
        name: "Antigravity",
        description: "Google 的 agent-first 开发平台；CLI 与桌面端共享 Antigravity agent 工作流。",
        icon: "antigravity.png",
        docs_url: "https://antigravity.google/docs/home",
        cli: CliSlot::Shared("agy"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
                name: "Antigravity 桌面端",
                description: "Google Antigravity 桌面开发平台，用于管理多个本地 agent 和工作区。",
                winget_id: Some("Google.Antigravity"),
                winget_source: None,
                appx_names: &[],
                install_url: "https://antigravity.google/download",
                docs_url: "https://antigravity.google/docs/home",
                keywords: &["antigravity"],
                excludes: &["cli"],

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[
            home(".antigravity"),
            home(".gemini/antigravity"),
            home(".gemini/antigravity-cli"),
            roaming("Antigravity"),
            local("antigravity"),
            local("antigravity-updater"),
        ],
    },
    ProductSpec {
        id: "grok",
        vendor: Vendor::Xai,
        family: "grok",
        edition: Edition::Global,
        edition_label: "",
        sort: 85,
        name: "Grok Build",
        description: "xAI 的终端编程智能体，先写计划再动手，可并行子智能体；需要 SuperGrok 或 X Premium+ 订阅。",
        icon: "grok.svg",
        docs_url: "https://x.ai/cli",
        cli: CliSlot::Shared("grok"),
        cli_note: None,
        desktop: DesktopSlot::Unavailable {
            name: "Grok Build 桌面端",
            description: "官方只提供命令行版本。",
            url: "https://x.ai/cli",
        },
        workbench_command: None,
        data_dirs: &[home(".grok")],
    },
    ProductSpec {
        id: "opencode",
        vendor: Vendor::OpenCode,
        family: "opencode",
        edition: Edition::Global,
        edition_label: "",
        sort: 70,
        name: "OpenCode",
        description: "开源工作智能体，支持终端自动化与图形化项目协作。",
        icon: "opencode-icon.png",
        docs_url: "https://opencode.ai/docs/",
        cli: CliSlot::Shared("opencode"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
                name: "OpenCode 桌面端",
                description: "OpenCode 图形化客户端，用于管理会话并开展项目协作。",
                winget_id: Some("SST.OpenCodeDesktop"),
                winget_source: None,
                appx_names: &[],
                install_url: "https://opencode.ai/download",
                docs_url: "https://opencode.ai/download",
                keywords: &["opencode", "open code"],
                excludes: &["cli"],

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[
            home(".config/opencode"),
            local("opencode"),
            roaming("ai.opencode.desktop"),
            local("@opencode-aidesktop-updater"),
        ],
    },
    ProductSpec {
        id: "zcode",
        vendor: Vendor::ZCode,
        family: "zcode",
        edition: Edition::Global,
        edition_label: "",
        sort: 140,
        name: "ZCode",
        description: "Z.ai 的桌面工作智能体，用于在本地工作区中规划、修改和验证代码。",
        icon: "zcode.svg",
        docs_url: "https://zcode.z.ai/en/docs/install",
        cli: CliSlot::Unavailable {
            name: "ZCode CLI",
            description: "ZCode 当前未提供独立的 Windows CLI。",
            url: "https://zcode.z.ai/en/docs/install",
        },
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
                name: "ZCode 桌面端",
                description: "ZCode Windows 桌面应用，提供完整的 Agent 开发工作流。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://zcode.z.ai/en/docs/install",
                docs_url: "https://zcode.z.ai/en/docs/install",
                keywords: &["zcode"],
                excludes: &[],

            install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方文档安装。"),

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".zcode"), roaming("ZCode"), local("@zcodedesktop-updater")],
    },
    ProductSpec {
        id: "kimi",
        vendor: Vendor::Kimi,
        family: "kimi",
        edition: Edition::Unified,
        edition_label: "",
        sort: 120,
        name: "Kimi",
        description: "Kimi 提供终端开发智能体和 Windows 本地工作智能体，可处理项目代码、文档与本地任务。",
        icon: "kimi.ico",
        docs_url: "https://www.kimi.com/help/kimi-code/cli-getting-started",
        cli: CliSlot::Shared("kimi"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
                name: "Kimi Work 桌面端",
                description: "Kimi Work Windows 本地工作智能体，可处理本地文件、执行自动化任务并协助完成知识工作。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.kimi.com/products/kimi-work",
                docs_url: "https://www.kimi.com/products/kimi-work",
                keywords: &["kimi work", "kimi"],
                excludes: &["kimi code", "kimi-code"],

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".kimi-code"), home(".kimi"), roaming("kimi-desktop")],
    },
    ProductSpec {
        id: "workbuddy-cn",
        vendor: Vendor::WorkBuddy,
        family: "workbuddy",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 110,
        name: "WorkBuddy 中国版",
        description: "腾讯 WorkBuddy 中国站桌面工作智能体，配套 CodeBuddy CLI，支持本地任务、项目开发与终端自动化。",
        icon: "workbuddy.svg",
        docs_url: "https://www.workbuddy.cn/docs/workbuddy/Quickstart",
        cli: CliSlot::Shared("codebuddy"),
        cli_note: Some("与国际版共用 CodeBuddy CLI，登录时选择中国站。"),
        desktop: DesktopSlot::App(DesktopSpec {
            name: "WorkBuddy 桌面端（中国版）",
            description: "腾讯 WorkBuddy 中国站 Windows 桌面应用。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide",
            docs_url: "https://www.workbuddy.cn/docs/workbuddy/Quickstart",
            // Registered as "WorkBuddy <version>"; the global edition is "WorkBuddy AI <version>".
            keywords: &["workbuddy"],
            excludes: &["workbuddy ai", "workbuddyai", "switch"],
            install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。"),
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[
            home(".workbuddy"),
            roaming("WorkBuddy"),
            local("WorkBuddy"),
            local("@genieworkbuddy-desktop-updater"),
            home(".codebuddy"),
            roaming("CodeBuddy CN"),
            local("CodeBuddyExtension"),
        ],
    },
    ProductSpec {
        id: "workbuddy-global",
        vendor: Vendor::WorkBuddy,
        family: "workbuddy",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 111,
        name: "WorkBuddy 国际版",
        description: "腾讯 WorkBuddy 国际站桌面工作智能体，配套 CodeBuddy CLI，支持本地任务、项目开发与终端自动化。",
        icon: "workbuddy.svg",
        docs_url: "https://www.workbuddy.ai/docs/workbuddy/Quickstart",
        cli: CliSlot::Shared("codebuddy"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "WorkBuddy 桌面端（国际版）",
            description: "腾讯 WorkBuddy 国际站 Windows 桌面应用。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://www.workbuddy.ai/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide",
            docs_url: "https://www.workbuddy.ai/docs/workbuddy/Quickstart",
            // Registered as "WorkBuddy AI <version>"; the executable is WorkBuddyAI.exe.
            keywords: &["workbuddy ai", "workbuddyai"],
            excludes: &["switch"],
            install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。"),
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".workbuddy-ai"), roaming("WorkBuddy AI")],
    },
    ProductSpec {
        id: "qoder",
        vendor: Vendor::Qoder,
        family: "qoder",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 131,
        name: "Qoder 国际版",
        description: "Qoder 国际版，以编程智能体为核心的新版桌面应用与终端智能体。",
        icon: "qoder.svg",
        docs_url: "https://docs.qoder.com/",
        cli: CliSlot::Shared("qoder"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "Qoder 桌面端",
            description: "以编程智能体为核心的新版 Qoder Windows 桌面应用。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://qoder.com/en/download",
            docs_url: "https://docs.qoder.com/",
            keywords: &["qoder"],
            excludes: &["qoder cn", "cli", "ide", "qoderwork", "qoderwake"],
            install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。"),
            // The Qoder IDE ships the same Qoder.exe name but uses an Inno Setup uninstaller.
            reject_sibling_files: &["unins000.exe"],
        }),
        workbench_command: None,
        data_dirs: &[
            home(".qoder"),
            roaming("Qoder"),
            roaming("com.qoder.app.stable"),
            local("Qoder"),
        ],
    },
    ProductSpec {
        id: "qoder-cn",
        vendor: Vendor::Qoder,
        family: "qoder",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 130,
        name: "Qoder 中国版",
        description: "Qoder 中国版，以编程智能体为核心的新版桌面应用与终端智能体。",
        icon: "qoder.svg",
        docs_url: "https://qoder.com.cn/",
        cli: CliSlot::Shared("qodercn"),
        cli_note: Some("中国版自带命令 qodercn（npm 包 @qodercn-ai/qoderclicn），和国际版的 qoder 是两个程序。"),
        desktop: DesktopSlot::App(DesktopSpec {
            name: "Qoder 桌面端（中国版）",
            description: "以编程智能体为核心的新版 Qoder Windows 桌面应用。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://qoder.com.cn/download",
            docs_url: "https://qoder.com.cn/",
            keywords: &["qoder cn"],
            excludes: &["cli", "ide", "qoderwork", "qoderwake"],
            install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。"),
            // The Qoder IDE ships the same Qoder.exe name but uses an Inno Setup uninstaller.
            reject_sibling_files: &["unins000.exe"],
        }),
        workbench_command: None,
        data_dirs: &[
            home(".qoder-cn"),
            roaming("QoderCN"),
            roaming("com.qodercn.app.stable"),
            local("Qoder CN"),
        ],
    },
    ProductSpec {
        id: "trae-work",
        vendor: Vendor::Trae,
        family: "trae",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 90,
        name: "TRAE 中国版",
        description: "TRAE 中国版，提供终端开发智能体和 Windows 桌面智能体。",
        icon: "trae-work.png",
        docs_url: "https://docs.trae.cn/cli_get-started-with-trae-cli",
        cli: CliSlot::Unavailable {
            name: "TraeCode CLI",
            description: "官方仅向 TRAE 企业版旗舰套餐开放，个人版账号无法登录，因此不纳入管理。",
            url: "https://docs.trae.cn/cli_about-trae-code-cli-2",
        },
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
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

            install_unavailable_reason: Some("TRAE Work 官网动态下发安装地址，当前无法可靠自动下载安装，请通过官方文档安装。"),

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".trae"), roaming("TRAE SOLO CN")],
    },
    ProductSpec {
        id: "trae-global",
        vendor: Vendor::Trae,
        family: "trae",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 91,
        name: "TRAE 国际版",
        description: "TRAE 国际版桌面开发智能体，面向全球站点和服务。",
        icon: "trae-work.png",
        docs_url: "https://www.trae.ai/",
        cli: CliSlot::Unavailable {
            name: "TRAE CLI（国际版）",
            description: "国际版暂未提供可由 Stacker 验证的独立 Windows CLI 安装入口。",
            url: "https://www.trae.ai/",
        },
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
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

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".trae"), roaming("TRAE SOLO")],
    },
    ProductSpec {
        id: "deepseek-harness",
        vendor: Vendor::DeepSeekHarness,
        family: "deepseek-harness",
        edition: Edition::Global,
        edition_label: "开发者预览",
        sort: 100,
        name: "DeepSeek Harness",
        description: "DeepSeek 官方开发者预览 Harness，可在本地启动 Web 工作台并编排智能体任务。",
        icon: "deepseek.svg",
        docs_url: "https://www.deepseek.com/harness/",
        cli: CliSlot::Shared("dsh"),
        cli_note: None,
        desktop: DesktopSlot::Unavailable {
            name: "DeepSeek Harness 桌面端",
            description: "官方不提供独立桌面端；本地 Web 工作台随 CLI 提供，用「打开 Web 工作台」启动。",
            url: "https://www.deepseek.com/harness/",
        },
        workbench_command: Some("dsh web"),
        data_dirs: &[home(".deepseek"), roaming("DeepSeek Harness"), local("DeepSeekHarness")],
    },
    ProductSpec {
        id: "openclaw",
        vendor: Vendor::OpenClaw,
        family: "openclaw",
        edition: Edition::Global,
        edition_label: "",
        sort: 30,
        name: "OpenClaw",
        description: "开源个人 AI 智能体平台，可在终端中管理网关、会话、工具和本地自动化任务。",
        icon: "openclaw.svg",
        docs_url: "https://docs.openclaw.ai/",
        cli: CliSlot::Shared("openclaw"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
                name: "OpenClaw Hub",
                description: "OpenClaw 的 Windows 桌面伴侣，用于设置、托盘状态、聊天和本地 MCP。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://docs.openclaw.ai/windows",
                docs_url: "https://docs.openclaw.ai/windows",
                keywords: &["openclaw hub", "openclaw"],
                excludes: &["cli"],

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[
            home(".openclaw"),
            roaming("OpenClaw"),
            roaming("OpenClawTray"),
            local("OpenClawTray"),
        ],
    },
    ProductSpec {
        id: "hermes",
        vendor: Vendor::Hermes,
        family: "hermes",
        edition: Edition::Global,
        edition_label: "",
        sort: 40,
        name: "Hermes Agent",
        description: "Nous Research 的本地 AI 智能体，CLI 与桌面端共享配置、会话、技能和记忆。",
        icon: "hermes.png",
        docs_url: "https://hermes-agent.nousresearch.com/docs/",
        cli: CliSlot::Shared("hermes"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
                name: "Hermes 桌面端",
                description: "Hermes 原生桌面应用，与 CLI 共用智能体运行时、配置、会话和技能。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://hermes-agent.nousresearch.com/docs/user-guide/desktop",
                docs_url: "https://hermes-agent.nousresearch.com/docs/user-guide/desktop",
                keywords: &["hermes agent", "hermes desktop", "hermes"],
                excludes: &["hermes browser"],

            install_unavailable_reason: None,

            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".hermes"), roaming("Hermes"), local("com.nousresearch.hermes.setup")],
    },
    ProductSpec {
        id: "pi",
        vendor: Vendor::Pi,
        family: "pi",
        edition: Edition::Global,
        edition_label: "",
        sort: 60,
        name: "pi",
        description: "极简开源终端编程智能体，内置读、写、编辑、命令四种工具，可通过扩展和技能定制。",
        icon: "pi.svg",
        docs_url: "https://pi.dev/",
        cli: CliSlot::Shared("pi"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "PI-Desktop（第三方）",
            description: "社区开源的 pi 桌面工作台 vastsa/PI-Desktop（LGPL-3.0），内置 pi 运行时。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://github.com/vastsa/PI-Desktop/releases/latest",
            docs_url: "https://github.com/vastsa/PI-Desktop",
            // Registered as "PI-Desktop <version>", executable PI-Desktop.exe.
            keywords: &["pi-desktop"],
            excludes: &[],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".pi")],
    },
    ProductSpec {
        id: "copilot",
        vendor: Vendor::Copilot,
        family: "copilot",
        edition: Edition::Global,
        edition_label: "",
        sort: 50,
        name: "GitHub Copilot",
        description: "GitHub 的编程智能体，CLI 在终端中理解和修改代码，桌面 App 管理 Copilot 编程会话。",
        icon: "copilot.svg",
        docs_url: "https://docs.github.com/copilot/how-tos/copilot-cli",
        cli: CliSlot::Shared("copilot"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "GitHub Copilot 桌面端",
            description: "GitHub Copilot Windows 桌面应用。",
            winget_id: Some("GitHub.CopilotApp"),
            winget_source: None,
            appx_names: &[],
            install_url: "https://github.com/features/copilot",
            docs_url: "https://docs.github.com/copilot",
            keywords: &["github copilot"],
            excludes: &["copilot cli"],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".copilot")],
    },
    ProductSpec {
        id: "cursor",
        vendor: Vendor::Cursor,
        family: "cursor",
        edition: Edition::Global,
        edition_label: "",
        sort: 55,
        name: "Cursor",
        description: "用得最广的 AI 编辑器之一，编辑器里的智能体和终端里的 Cursor CLI 共用同一个账号和订阅。",
        icon: "cursor.svg",
        docs_url: "https://cursor.com/docs",
        cli: CliSlot::Shared("cursor"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "Cursor 编辑器",
            description: "Cursor 的 Windows 编辑器（基于 VS Code）。",
            winget_id: Some("Anysphere.Cursor"),
            winget_source: None,
            appx_names: &[],
            install_url: "https://cursor.com/downloads",
            docs_url: "https://cursor.com/docs",
            keywords: &["cursor"],
            // The CLI's own files, and unrelated mouse-pointer tools that share the word.
            excludes: &["cursor-agent", "cursor agent", "custom cursor", "cursorfx", "mouse"],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".cursor"), roaming("Cursor")],
    },
    ProductSpec {
        id: "factory",
        vendor: Vendor::Factory,
        family: "factory",
        edition: Edition::Global,
        edition_label: "",
        sort: 95,
        name: "Factory",
        description: "Factory 的 Droid 编程智能体：终端里的 Droid CLI 常年排在 Terminal-Bench 前列，桌面端可并行管理多个 Droid 会话。",
        icon: "factory.svg",
        docs_url: "https://docs.factory.ai/",
        cli: CliSlot::Shared("droid"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "Factory 桌面端",
            description: "Factory 的 Windows 桌面应用，运行和管理 Droid 会话。",
            winget_id: Some("FactoryAI.Factory"),
            winget_source: None,
            appx_names: &[],
            install_url: "https://factory.ai/",
            docs_url: "https://docs.factory.ai/factory-app/quickstart",
            keywords: &["factory"],
            excludes: &["pdffactory", "pdf factory", "satisfactory", "setup factory", "quotation"],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".factory")],
    },
    ProductSpec {
        id: "kiro",
        vendor: Vendor::Kiro,
        family: "kiro",
        edition: Edition::Global,
        edition_label: "",
        sort: 97,
        name: "Kiro",
        description: "AWS 的智能体编辑器，先把需求写成规范再生成代码、文档和测试；终端里另有 Kiro CLI。",
        icon: "kiro.svg",
        docs_url: "https://kiro.dev/docs/",
        cli: CliSlot::Shared("kiro"),
        cli_note: Some("Kiro CLI 只支持 Windows 11；编辑器 Windows 10 也能用。"),
        desktop: DesktopSlot::App(DesktopSpec {
            name: "Kiro 编辑器",
            description: "Kiro 的 Windows 编辑器。",
            winget_id: Some("Amazon.Kiro"),
            winget_source: None,
            appx_names: &[],
            install_url: "https://kiro.dev/downloads/",
            docs_url: "https://kiro.dev/docs/",
            keywords: &["kiro"],
            excludes: &["kiro-cli", "kiro cli", "kirocli"],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".kiro"), roaming("Kiro")],
    },
    ProductSpec {
        id: "minimax-cn",
        vendor: Vendor::MiniMax,
        family: "minimax",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 170,
        name: "MiniMax Code 中国版",
        description: "MiniMax 的编程和办公智能体：终端里的 mcode 开源，桌面端连接本地文件、工具和浏览器。",
        icon: "minimax.svg",
        docs_url: "https://agent.minimax.cn/download",
        cli: CliSlot::Shared("mcode"),
        cli_note: Some("与国际版共用同一个 mcode 命令，登录时选中国区账号。两版桌面端安装后同名，Stacker 暂时分不出装的是哪一版。"),
        desktop: DesktopSlot::App(DesktopSpec {
            name: "MiniMax Code 桌面端（中国版）",
            description: "MiniMax Code 中国版桌面智能体。",
            winget_id: Some("MiniMax.MiniMaxCode"),
            winget_source: None,
            appx_names: &[],
            install_url: "https://agent.minimax.cn/download",
            docs_url: "https://agent.minimax.cn/download",
            keywords: &["minimax code"],
            excludes: &["minimax design", "minimax hub"],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".minimax"), home(".minimax-code")],
    },
    ProductSpec {
        id: "minimax-global",
        vendor: Vendor::MiniMax,
        family: "minimax",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 171,
        name: "MiniMax Code 国际版",
        description: "MiniMax 的编程和办公智能体：终端里的 mcode 开源，桌面端连接本地文件、工具和浏览器。",
        icon: "minimax.svg",
        docs_url: "https://agent.minimax.io/docs/code/welcome",
        cli: CliSlot::Shared("mcode"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "MiniMax Code 桌面端（国际版）",
            description: "MiniMax Code 国际版桌面智能体，通过 Microsoft Store 安装。",
            winget_id: Some("XPDFFNR4R84TGC"),
            winget_source: Some("msstore"),
            appx_names: &[],
            install_url: "https://agent.minimax.io/download",
            docs_url: "https://agent.minimax.io/docs/code/get-started/download",
            keywords: &["minimax code"],
            excludes: &["minimax design", "minimax hub"],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".minimax"), home(".minimax-code")],
    },
    ProductSpec {
        id: "mimo-cn",
        vendor: Vendor::MiMo,
        family: "mimo",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 150,
        name: "小米 MiMo 中国版",
        description: "小米 MiMo 大模型驱动的编程智能体：终端 CLI 可读写代码、搜索项目并执行命令，另有 MiMo Desktop 桌面端。",
        icon: "mimo.svg",
        docs_url: "https://mimo.xiaomi.com/coder",
        cli: CliSlot::Shared("mimo"),
        cli_note: Some("与国际版共用同一个 mimo 命令，登录时使用中国区平台账号。"),
        desktop: DesktopSlot::App(DesktopSpec {
            name: "MiMo Desktop（中国版）",
            description: "小米 MiMo Desktop 中国版桌面智能体。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://mimo.xiaomimimo.com/desktop/",
            docs_url: "https://mimo.mi.com/docs/en-US/news/latest/mimo-desktop",
            // The installer's product name: "Xiaomi MiMo" here, "Xiaomi MiMo AI" for the global edition.
            keywords: &["xiaomi mimo", "mimo desktop"],
            excludes: &["mimo ai", "global", "international", "intl", "overseas"],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[home(".mimocode"), home(".local/share/mimocode"), roaming("Xiaomi MiMo")],
    },
    ProductSpec {
        id: "mimo-global",
        vendor: Vendor::MiMo,
        family: "mimo",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 151,
        name: "Xiaomi MiMo 国际版",
        description: "小米 MiMo 大模型驱动的编程智能体：终端 CLI 可读写代码、搜索项目并执行命令，另有 MiMo Desktop 桌面端。",
        icon: "mimo.svg",
        docs_url: "https://mimo.xiaomi.com/coder",
        cli: CliSlot::Shared("mimo"),
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "MiMo Desktop（国际版）",
            description: "Xiaomi MiMo Desktop 国际版桌面智能体；电脑操控（Computer Use）仅国际版提供。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://mimo-ai.xiaomimimo.com/desktop/",
            docs_url: "https://mimo.mi.com/docs/en-US/news/latest/mimo-desktop",
            keywords: &["xiaomi mimo ai", "mimo desktop global", "mimo desktop international"],
            excludes: &[],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[roaming("Xiaomi MiMo AI")],
    },
    ProductSpec {
        id: "agnes-cn",
        vendor: Vendor::Agnes,
        family: "agnes",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 160,
        name: "Agnes Code 中国版",
        description: "桌面 AI 工作台：一个输入框连接本地项目、专家模式、技能扩展与 MCP 应用连接，账号与网页端同步。",
        icon: "agnes.svg",
        docs_url: "https://agnes-ai.cn/agnescode",
        cli: CliSlot::Unavailable {
            name: "Agnes Code CLI",
            description: "Agnes 只发布桌面应用，官方没有提供命令行。",
            url: "https://agnes-ai.cn/agnescode",
        },
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "Agnes Code 桌面端（中国版）",
            description: "agnes-ai.cn 发布的 Windows 桌面应用，安装后以发行方名称“爱思办公”登记，账号与国际站不互通。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://agnes-ai.cn/agnescode",
            docs_url: "https://agnes-ai.cn/agnescode",
            // This edition ships under the publisher's own name: the installer's product
            // name, its folder and its executable are all "爱思办公".
            keywords: &["爱思办公"],
            excludes: &[],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[roaming("爱思办公"), home(".agnes")],
    },
    ProductSpec {
        id: "agnes-global",
        vendor: Vendor::Agnes,
        family: "agnes",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 161,
        name: "Agnes Code 国际版",
        description: "桌面 AI 工作台：一个输入框连接本地项目、专家模式、技能扩展与 MCP 应用连接，账号与网页端同步。",
        icon: "agnes.svg",
        docs_url: "https://agnes-ai.com/agnescode",
        cli: CliSlot::Unavailable {
            name: "Agnes Code CLI",
            description: "Agnes 只发布桌面应用，官方没有提供命令行。",
            url: "https://agnes-ai.com/agnescode",
        },
        cli_note: None,
        desktop: DesktopSlot::App(DesktopSpec {
            name: "Agnes Code 桌面端（国际版）",
            description: "agnes-ai.com 发布的 Windows 桌面应用，安装包未带数字签名，按官方发布的 SHA-512 校验后安装。",
            winget_id: None,
            winget_source: None,
            appx_names: &[],
            install_url: "https://agnes-ai.com/agnescode",
            docs_url: "https://agnes-ai.com/agnescode",
            // Registered as "Agnes Code <version>"; the executable is AgnesCode.exe.
            keywords: &["agnes"],
            excludes: &[],
            install_unavailable_reason: None,
            reject_sibling_files: &[],
        }),
        workbench_command: None,
        data_dirs: &[roaming("Agnes Code"), roaming("AgnesCode"), home(".agnes")],
    },
];

/// The two MiMo Desktop editions' official installers (x64 only; the site offers no ARM build).
pub(crate) const MIMO_DESKTOP_CN_INSTALLER: &str =
    "https://mimocode-cdn.xiaomimimo.com/mimocode/mimodesktop/XiaomiMiMo-latest-x64-setup.exe";
pub(crate) const MIMO_DESKTOP_GLOBAL_INSTALLER: &str =
    "https://mimocode-cdn.xiaomimimo.com/mimocode/mimodesktopai/XiaomiMiMo-AI-latest-x64-setup.exe";

/// Agnes Code keeps an electron-builder feed per edition on the same CDN its own updater
/// reads: `release-cn` for the China site, `release` for the international one.
pub(crate) const AGNES_CN_RELEASE_BASE: &str =
    "https://cos-agnes-code.agnes-ai.cn/release-cn/latest";
pub(crate) const AGNES_GLOBAL_RELEASE_BASE: &str =
    "https://cos-agnes-code.agnes-ai.cn/release/latest";

pub(crate) fn direct_desktop_installer(
    vendor: Vendor,
    edition: Edition,
) -> Option<DirectDesktopInstaller> {
    match vendor {
        Vendor::Kimi => Some(DirectDesktopInstaller {
            source: InstallerSource::Fixed {
                url: "https://appsupport.moonshot.cn/api/app/pkg/latest/windows/download",
                file_name: "Kimi-Work-Setup.exe",
            },
            silent_args: &["/S"],
            signed: true,
        }),
        Vendor::OpenClaw => Some(DirectDesktopInstaller {
            source: InstallerSource::Fixed {
                url: openclaw_desktop_installer_url(),
                file_name: "OpenClawCompanion-Setup.exe",
            },
            // Inno Setup, not NSIS: /S means nothing to it and its wizard opened. Per-user, as
            // earlier releases installed, so no UAC prompt; the previous folder is kept.
            silent_args: &[
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/SP-",
                "/CURRENTUSER",
            ],
            signed: true,
        }),
        Vendor::Hermes => Some(DirectDesktopInstaller {
            source: InstallerSource::Fixed {
                url: "https://hermes-assets.nousresearch.com/Hermes-Setup.exe",
                file_name: "Hermes-Setup.exe",
            },
            // Not an NSIS installer: it ignores /S and waits for its Install button.
            silent_args: &[],
            signed: true,
        }),
        // Signed by Xiaomi with a DigiCert code-signing certificate; electron-builder NSIS.
        Vendor::MiMo => Some(DirectDesktopInstaller {
            source: if edition == Edition::Global {
                InstallerSource::Fixed {
                    url: MIMO_DESKTOP_GLOBAL_INSTALLER,
                    file_name: "XiaomiMiMo-AI-Setup.exe",
                }
            } else {
                InstallerSource::Fixed {
                    url: MIMO_DESKTOP_CN_INSTALLER,
                    file_name: "XiaomiMiMo-Setup.exe",
                }
            },
            silent_args: &["/S"],
            signed: true,
        }),
        // Each from the service the app's own updater (or website) asks; all three are signed.
        // WorkBuddy: electron-builder NSIS, per user.
        Vendor::WorkBuddy => Some(DirectDesktopInstaller {
            source: InstallerSource::Service {
                resolve: if edition == Edition::Global {
                    super::feeds::workbuddy_global_installer
                } else {
                    super::feeds::workbuddy_cn_installer
                },
            },
            // `/S` alone: an electron-builder installer refuses a silent run that also
            // carries a scope switch, and this package is per user already.
            silent_args: &["/S"],
            signed: true,
        }),
        // TRAE Work: Inno Setup, per user.
        Vendor::Trae => Some(DirectDesktopInstaller {
            source: InstallerSource::Service {
                resolve: if edition == Edition::Global {
                    super::feeds::trae_global_installer
                } else {
                    super::feeds::trae_cn_installer
                },
            },
            silent_args: &[
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/SP-",
                "/CURRENTUSER",
            ],
            signed: true,
        }),
        // Qoder: electron-builder NSIS, per user for a new install; an install made for all
        // users is updated by its per-machine package (see feeds::qoder_installer).
        Vendor::Qoder => Some(DirectDesktopInstaller {
            source: InstallerSource::Service {
                resolve: if edition == Edition::Global {
                    super::feeds::qoder_global_installer
                } else {
                    super::feeds::qoder_cn_installer
                },
            },
            silent_args: &["/S"],
            signed: true,
        }),
        // Signed electron-builder NSIS installer named by the manifest ZCode's updater reads.
        Vendor::ZCode => Some(DirectDesktopInstaller {
            source: InstallerSource::ElectronManifest {
                manifest_url: zcode_manifest_url(),
            },
            silent_args: &["/S"],
            signed: true,
        }),
        // Agnes Code: electron-builder NSIS, per user. Only the China package carries an
        // Authenticode signature (DigiCert EV); the international one is checked against the
        // SHA-512 its own feed publishes.
        Vendor::Agnes => Some(DirectDesktopInstaller {
            source: InstallerSource::ElectronRelease {
                base_url: if edition == Edition::Cn {
                    AGNES_CN_RELEASE_BASE
                } else {
                    AGNES_GLOBAL_RELEASE_BASE
                },
            },
            silent_args: &["/S"],
            signed: edition == Edition::Cn,
        }),
        // Third-party open-source pi desktop app; its Windows installer is not signed.
        Vendor::Pi => Some(DirectDesktopInstaller {
            source: InstallerSource::ElectronRelease {
                base_url: "https://github.com/vastsa/PI-Desktop/releases/latest/download",
            },
            silent_args: &["/S"],
            signed: false,
        }),
        _ => None,
    }
}

/// The release manifest ZCode's own updater reads (stable channel).
pub(crate) fn zcode_manifest_url() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "https://zcode.z.ai/api/v1/releases/electron/manifest?platform=windows-aarch64&channel=1"
    } else {
        "https://zcode.z.ai/api/v1/releases/electron/manifest?platform=windows-x86_64&channel=1"
    }
}

/// Where the OpenClaw Windows companion is released; the main repository ships no Windows build.
pub(crate) const OPENCLAW_WINDOWS_REPO: &str = "openclaw/openclaw-windows-node";

#[cfg(target_arch = "aarch64")]
pub(crate) fn openclaw_desktop_installer_url() -> &'static str {
    "https://github.com/openclaw/openclaw-windows-node/releases/latest/download/OpenClawCompanion-Setup-arm64.exe"
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn openclaw_desktop_installer_url() -> &'static str {
    "https://github.com/openclaw/openclaw-windows-node/releases/latest/download/OpenClawCompanion-Setup-x64.exe"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn new_agent_surfaces_use_official_install_sources() {
        let kimi = spec_by_id("kimi").expect("Kimi catalog entry");
        assert_eq!(kimi.desktop.name, "Kimi Work 桌面端");
        assert!(!kimi.desktop.keywords.is_empty());
        assert!(direct_desktop_installer(Vendor::Kimi, Edition::Unified).is_some());

        // TraeCode CLI only serves TRAE Enterprise flagship seats, so it is listed as
        // unavailable rather than offered and then refusing to sign the user in.
        let trae = spec_by_id("trae-work").expect("TRAE catalog entry");
        assert_eq!(trae.cli_id, None);
        assert!(trae.cli.description.contains("个人版账号无法登录"));
    }

    #[test]
    fn product_and_cli_ids_are_unique() {
        let mut products = HashSet::new();
        for product in PRODUCTS {
            assert!(
                products.insert(product.id),
                "duplicate product id {}",
                product.id
            );
        }
        let mut clis = HashSet::new();
        for cli in CLIS {
            assert!(clis.insert(cli.id), "duplicate cli id {}", cli.id);
        }
    }

    #[test]
    fn product_cli_references_exist() {
        for product in PRODUCTS {
            if let CliSlot::Shared(id) = product.cli {
                assert!(
                    cli_by_id(id).is_some(),
                    "{} references missing cli {id}",
                    product.id
                );
            }
        }
    }

    #[test]
    fn every_product_offers_a_surface_and_has_an_icon_file() {
        let brands = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../public/brands");
        for product in PRODUCTS {
            let has_cli = matches!(product.cli, CliSlot::Shared(_));
            let has_desktop = matches!(product.desktop, DesktopSlot::App(_));
            assert!(has_cli || has_desktop, "{} offers no surface", product.id);
            assert!(
                brands.join(product.icon).is_file(),
                "{} icon {} missing",
                product.id,
                product.icon
            );
        }
    }

    #[test]
    fn editions_are_unique_within_a_family() {
        let mut seen = HashSet::new();
        for product in PRODUCTS {
            assert!(
                seen.insert((product.family, product.edition)),
                "{} repeats an edition",
                product.id
            );
        }
    }

    #[test]
    fn tool_view_resolves_shared_cli() {
        let spec = spec_by_id("codex").unwrap();
        assert_eq!(spec.cli.command, "codex");
        assert_eq!(spec.cli_id, Some("codex"));
        assert_eq!(spec.icon, "codex.png");
    }
    #[test]
    fn workbuddy_editions_share_the_codebuddy_cli() {
        let cn = spec_by_id("workbuddy-cn").unwrap();
        let global = spec_by_id("workbuddy-global").unwrap();
        assert_eq!(cn.cli_id, Some("codebuddy"));
        assert_eq!(global.cli_id, Some("codebuddy"));
        assert_eq!(cn.edition, Edition::Cn);
        assert_eq!(global.edition, Edition::Global);
        assert!(cn.cli_note.unwrap().contains("中国站"));
        assert!(spec_by_id("workbuddy").is_none());
        // Registered names on Windows: "WorkBuddy 5.5.6" and "WorkBuddy AI 5.5.2".
        assert!(desktop_matches(&cn.desktop, "WorkBuddy 5.5.6"));
        assert!(!desktop_matches(&cn.desktop, "WorkBuddy AI 5.5.2"));
        assert!(!desktop_matches(&cn.desktop, "workbuddy-switch"));
        assert!(desktop_matches(&global.desktop, "WorkBuddy AI 5.5.2"));
        assert!(desktop_matches(&global.desktop, "WorkBuddyAI.exe"));
        assert!(!desktop_matches(&cn.desktop, "WorkBuddyAI.exe"));
        assert!(!desktop_matches(&global.desktop, "WorkBuddy 5.5.6"));
    }

    #[test]
    fn qoder_editions_track_the_new_desktop_line_and_reject_the_ide() {
        let global = spec_by_id("qoder").unwrap();
        let cn = spec_by_id("qoder-cn").unwrap();
        // Each edition ships its own CLI package: qoder for the global one, qodercn for China.
        assert_eq!(global.cli_id, Some("qoder"));
        assert_eq!(cn.cli_id, Some("qodercn"));
        let cn_cli = cli_by_id("qodercn").unwrap();
        assert_eq!(cn_cli.command, "qodercn");
        assert_eq!(cn_cli.npm_package, Some("@qodercn-ai/qoderclicn"));
        assert!(desktop_matches(&global.desktop, "Qoder 0.2.5"));
        assert!(!desktop_matches(&global.desktop, "Qoder CN 0.2.5"));
        assert!(desktop_matches(&cn.desktop, "Qoder CN 0.2.5"));
        for spec in [&global, &cn] {
            assert!(spec.desktop.reject_sibling_files.contains(&"unins000.exe"));
            assert!(!desktop_matches(&spec.desktop, "QoderWork 1.0"));
        }
    }

    #[test]
    fn pi_uses_the_new_package_and_pi_desktop() {
        let pi = spec_by_id("pi").unwrap();
        assert_eq!(pi.cli.command, "pi");
        assert_eq!(pi.cli.npm_package, Some("@earendil-works/pi-coding-agent"));
        assert!(pi.desktop_available);
        assert!(desktop_matches(&pi.desktop, "PI-Desktop 0.15.0"));
        let installer = direct_desktop_installer(Vendor::Pi, Edition::Unified).unwrap();
        assert!(!installer.signed);
        assert!(matches!(
            installer.source,
            InstallerSource::ElectronRelease { .. }
        ));
    }

    #[test]
    fn agnes_editions_are_told_apart_by_the_name_each_one_registers() {
        let cn = spec_by_id("agnes-cn").unwrap();
        let global = spec_by_id("agnes-global").unwrap();
        // The China build installs under the publisher's own name, the international one
        // under the product name, so one installation never lights up both cards.
        assert!(desktop_matches(&cn.desktop, "爱思办公 1.0.67"));
        assert!(!desktop_matches(&cn.desktop, "Agnes Code 1.0.67"));
        assert!(desktop_matches(&global.desktop, "Agnes Code 1.0.67"));
        assert!(!desktop_matches(&global.desktop, "爱思办公 1.0.67"));
        // Neither edition ships a CLI.
        assert_eq!(cn.cli_id, None);
        assert_eq!(global.cli_id, None);

        let cn_installer = direct_desktop_installer(Vendor::Agnes, Edition::Cn).unwrap();
        assert!(cn_installer.signed);
        assert!(matches!(
            cn_installer.source,
            InstallerSource::ElectronRelease {
                base_url: AGNES_CN_RELEASE_BASE
            }
        ));
        // The international package carries no signature; its SHA-512 stands in.
        let global_installer = direct_desktop_installer(Vendor::Agnes, Edition::Global).unwrap();
        assert!(!global_installer.signed);
        assert!(matches!(
            global_installer.source,
            InstallerSource::ElectronRelease {
                base_url: AGNES_GLOBAL_RELEASE_BASE
            }
        ));
    }

    #[test]
    fn copilot_and_mimo_are_registered() {
        let copilot = spec_by_id("copilot").unwrap();
        assert_eq!(copilot.cli.command, "copilot");
        assert_eq!(copilot.cli.winget_id, Some("GitHub.Copilot"));
        assert_eq!(copilot.desktop.winget_id, Some("GitHub.CopilotApp"));
        let cn = spec_by_id("mimo-cn").unwrap();
        let global = spec_by_id("mimo-global").unwrap();
        assert_eq!(cn.cli_id, Some("mimo"));
        assert_eq!(global.cli_id, Some("mimo"));
        assert_eq!(cn.cli.command, "mimo");
        assert!(cn.cli_note.unwrap().contains("中国区"));
        // Each MiMo Desktop edition has its own download page, installer and registry name.
        assert!(cn
            .desktop
            .install_url
            .contains("mimo.xiaomimimo.com/desktop/"));
        assert!(global
            .desktop
            .install_url
            .contains("mimo-ai.xiaomimimo.com/desktop/"));
        let url = |spec: &ToolSpec| match direct_desktop_installer(spec.vendor, spec.edition) {
            Some(DirectDesktopInstaller {
                source: InstallerSource::Fixed { url, .. },
                signed: true,
                ..
            }) => url,
            _ => panic!("MiMo Desktop needs its signed official installer"),
        };
        assert_eq!(url(&cn), MIMO_DESKTOP_CN_INSTALLER);
        assert_eq!(url(&global), MIMO_DESKTOP_GLOBAL_INSTALLER);
        assert!(desktop_matches(&cn.desktop, "Xiaomi MiMo 26.922.220226"));
        assert!(!desktop_matches(
            &cn.desktop,
            "Xiaomi MiMo AI 26.922.220226"
        ));
        assert!(desktop_matches(
            &global.desktop,
            "Xiaomi MiMo AI 26.922.220226"
        ));
        assert!(!desktop_matches(
            &global.desktop,
            "Xiaomi MiMo 26.922.220226"
        ));
        assert!(spec_by_id("mimo").is_none());
    }

    fn desktop_matches(spec: &DesktopSpec, name: &str) -> bool {
        let lower = name.to_lowercase();
        spec.keywords.iter().any(|k| lower.contains(k))
            && !spec.excludes.iter().any(|k| lower.contains(k))
    }
}
