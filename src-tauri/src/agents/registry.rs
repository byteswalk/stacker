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
    pub(crate) sort: u16,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) docs_url: &'static str,
    pub(crate) cli: CliSlot,
    pub(crate) cli_note: Option<&'static str>,
    pub(crate) desktop: DesktopSlot,
    /// PowerShell regex matched against "<process name> <command line>". Short vendor
    /// names must be anchored to a path segment or executable name.
    pub(crate) process_pattern: &'static str,
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
    #[allow(dead_code)] // Consumed when process detection moves to the registry.
    pub(crate) process_pattern: &'static str,
    #[allow(dead_code)]
    pub(crate) data_dirs: &'static [DataDir],
}

#[derive(Clone, Copy)]
pub(crate) struct DirectDesktopInstaller {
    pub(crate) url: &'static str,
    pub(crate) file_name: &'static str,
    pub(crate) silent_args: &'static [&'static str],
}

pub(crate) const DEFAULT_DESKTOP_UNAVAILABLE_REASON: &str =
    "官方未提供可自动安装的独立 Windows 应用。";

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
        process_pattern: product.process_pattern,
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
        id: "qoder-cn",
        name: "Qoder CLI（中国版）",
        description: "命令名 qoder，安装与更新优先使用中国站点。",
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
        install_url: "https://qoder.com.cn/download",
        docs_url: "https://qoder.com.cn/",
    },
    CliSpec {
        id: "traecli",
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
        description: "Mario Zechner 的极简终端编程智能体，命令名 pi。",
        command: "pi",
        candidates: &["pi.cmd", "pi.exe", "pi.bat", "pi.ps1"],
        npm_package: Some("@mariozechner/pi-coding-agent"),
        winget_id: None,
        install_url: "https://pi.dev/",
        docs_url: "https://pi.dev/",
    },
];

pub(crate) static PRODUCTS: &[ProductSpec] = &[
    ProductSpec {
        id: "claude",
        vendor: Vendor::Claude,
        family: "claude",
        edition: Edition::Global,
        edition_label: "",
        sort: 10,
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
        process_pattern: r#"(?i)(^|[\\/\s"])claude([\\/\s".]|$)|@anthropic-ai[\\/]claude-code"#,
        data_dirs: &[DataDir { base: DataBase::Home, relative: ".claude", env_override: Some("CLAUDE_CONFIG_DIR") }, DataDir { base: DataBase::Roaming, relative: "Claude", env_override: None }],
    },
    ProductSpec {
        id: "codex",
        vendor: Vendor::Codex,
        family: "codex",
        edition: Edition::Global,
        edition_label: "",
        sort: 20,
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
        process_pattern: r#"(?i)(^|[\\/\s"])codex([\\/\s".]|$)|@openai[\\/]codex"#,
        data_dirs: &[DataDir { base: DataBase::Home, relative: ".codex", env_override: Some("CODEX_HOME") }, DataDir { base: DataBase::Roaming, relative: "Codex", env_override: None }],
    },
    ProductSpec {
        id: "antigravity",
        vendor: Vendor::Antigravity,
        family: "antigravity",
        edition: Edition::Global,
        edition_label: "",
        sort: 30,
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
        process_pattern: r#"(?i)antigravity|(^|[\\/])agy(\\.cmd|\\.exe)?"#,
        data_dirs: &[home(".antigravity"), roaming("Antigravity")],
    },
    ProductSpec {
        id: "opencode",
        vendor: Vendor::OpenCode,
        family: "opencode",
        edition: Edition::Global,
        edition_label: "",
        sort: 40,
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
        process_pattern: r#"(?i)opencode"#,
        data_dirs: &[home(".config/opencode"), local("opencode")],
    },
    ProductSpec {
        id: "zcode",
        vendor: Vendor::ZCode,
        family: "zcode",
        edition: Edition::Global,
        edition_label: "",
        sort: 50,
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
        process_pattern: r#"(?i)zcode|z\.ai"#,
        data_dirs: &[home(".zcode"), roaming("ZCode")],
    },
    ProductSpec {
        id: "kimi",
        vendor: Vendor::Kimi,
        family: "kimi",
        edition: Edition::Unified,
        edition_label: "",
        sort: 60,
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
        process_pattern: r#"(?i)(^|[\\/\s"])kimi(-cli|-code)?([\\/\s".]|$)"#,
        data_dirs: &[home(".kimi"), roaming("Kimi")],
    },
    ProductSpec {
        id: "workbuddy-cn",
        vendor: Vendor::WorkBuddy,
        family: "workbuddy",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 70,
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
            excludes: &["workbuddy ai", "switch"],
            install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。"),
            reject_sibling_files: &[],
        }),
        process_pattern: r#"(?i)workbuddy"#,
        data_dirs: &[home(".workbuddy"), roaming("WorkBuddy"), home(".codebuddy")],
    },
    ProductSpec {
        id: "workbuddy-global",
        vendor: Vendor::WorkBuddy,
        family: "workbuddy",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 71,
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
            keywords: &["workbuddy ai"],
            excludes: &["switch"],
            install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。"),
            reject_sibling_files: &[],
        }),
        process_pattern: r#"(?i)workbuddy"#,
        data_dirs: &[home(".workbuddy-ai"), roaming("WorkBuddy AI"), home(".codebuddy")],
    },
    ProductSpec {
        id: "qoder",
        vendor: Vendor::Qoder,
        family: "qoder",
        edition: Edition::Global,
        edition_label: "国际版",
        sort: 80,
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
        process_pattern: r#"(?i)(^|[\\/\s"])qoder([\\/\s".]|$)"#,
        data_dirs: &[home(".qoder"), roaming("Qoder")],
    },
    ProductSpec {
        id: "qoder-cn",
        vendor: Vendor::Qoder,
        family: "qoder",
        edition: Edition::Cn,
        edition_label: "中国版",
        sort: 81,
        name: "Qoder 中国版",
        description: "Qoder 中国版，以编程智能体为核心的新版桌面应用与终端智能体。",
        icon: "qoder.svg",
        docs_url: "https://qoder.com.cn/",
        cli: CliSlot::Shared("qoder-cn"),
        cli_note: None,
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
        process_pattern: r#"(?i)(^|[\\/\s"])qoder([\\/\s".]|$)"#,
        data_dirs: &[home(".qoder-cn"), roaming("QoderCN")],
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
        cli: CliSlot::Shared("traecli"),
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
        process_pattern: r#"(?i)(^|[\\/\s"])trae([\\/\s".]|$)"#,
        data_dirs: &[home(".trae"), roaming("Trae")],
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
        process_pattern: r#"(?i)(^|[\\/\s"])trae([\\/\s".]|$)"#,
        data_dirs: &[home(".trae"), roaming("Trae")],
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
        desktop: DesktopSlot::App(DesktopSpec {
                name: "DeepSeek Harness Web",
                description: "Harness 通过本地 Web 工作台运行，不提供独立 Windows 桌面安装包。",
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: "https://www.deepseek.com/harness/",
                docs_url: "https://github.com/deepseek-ai/deepseek-harness",
                keywords: &[],
                excludes: &[],

            install_unavailable_reason: Some("DeepSeek Harness 需要按官方文档在本地工作目录中安装；完成后 Stacker 会识别本地工作台启动器。"),

            reject_sibling_files: &[],
        }),
        process_pattern: r#"(?i)deepseek-harness|@deepseek-ai[\\/]dsh|(^|[\\/])dsh(\.cmd|\.exe)?"#,
        data_dirs: &[home(".deepseek"), roaming("DeepSeek Harness")],
    },
    ProductSpec {
        id: "openclaw",
        vendor: Vendor::OpenClaw,
        family: "openclaw",
        edition: Edition::Global,
        edition_label: "",
        sort: 110,
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
        process_pattern: r#"(?i)openclaw"#,
        data_dirs: &[home(".openclaw"), roaming("OpenClaw")],
    },
    ProductSpec {
        id: "hermes",
        vendor: Vendor::Hermes,
        family: "hermes",
        edition: Edition::Global,
        edition_label: "",
        sort: 120,
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
        process_pattern: r#"(?i)(^|[\\/\s"])hermes(-agent)?([\\/\s".]|$)"#,
        data_dirs: &[home(".hermes"), roaming("Hermes")],
    },
    ProductSpec {
        id: "pi",
        vendor: Vendor::Pi,
        family: "pi",
        edition: Edition::Global,
        edition_label: "",
        sort: 130,
        name: "pi",
        description: "极简开源终端编程智能体，内置读、写、编辑、命令四种工具，可通过扩展和技能定制。",
        icon: "pi.svg",
        docs_url: "https://pi.dev/",
        cli: CliSlot::Shared("pi"),
        cli_note: None,
        desktop: DesktopSlot::Unavailable {
            name: "pi 桌面端",
            description: "pi 只提供终端使用方式。",
            url: "https://pi.dev/",
        },
        process_pattern: r#"(?i)@mariozechner[\\/]pi-coding-agent|(^|[\\/\s"])pi(\.cmd|\.exe)([\s"]|$)"#,
        data_dirs: &[home(".pi")],
    },
];

pub(crate) fn direct_desktop_installer(vendor: Vendor) -> Option<DirectDesktopInstaller> {
    match vendor {
        Vendor::Kimi => Some(DirectDesktopInstaller {
            url: "https://appsupport.moonshot.cn/api/app/pkg/latest/windows/download",
            file_name: "Kimi-Work-Setup.exe",
            silent_args: &["/S"],
        }),
        Vendor::OpenClaw => Some(DirectDesktopInstaller {
            url: openclaw_desktop_installer_url(),
            file_name: "OpenClawCompanion-Setup.exe",
            silent_args: &["/S"],
        }),
        Vendor::Hermes => Some(DirectDesktopInstaller {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn new_agent_surfaces_use_official_install_sources() {
        let kimi = spec_by_id("kimi").expect("Kimi catalog entry");
        assert_eq!(kimi.desktop.name, "Kimi Work 桌面端");
        assert!(!kimi.desktop.keywords.is_empty());
        assert!(direct_desktop_installer(Vendor::Kimi).is_some());

        let trae = spec_by_id("trae-work").expect("TRAE catalog entry");
        assert_eq!(trae.cli.command, "traecli");
        assert!(trae.cli.candidates.contains(&"traecli.exe"));
        assert_eq!(
            trae.cli.docs_url,
            "https://docs.trae.cn/cli_get-started-with-trae-cli"
        );
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
        assert!(!desktop_matches(&global.desktop, "WorkBuddy 5.5.6"));
    }

    #[test]
    fn qoder_editions_track_the_new_desktop_line_and_reject_the_ide() {
        let global = spec_by_id("qoder").unwrap();
        let cn = spec_by_id("qoder-cn").unwrap();
        assert!(desktop_matches(&global.desktop, "Qoder 0.2.5"));
        assert!(!desktop_matches(&global.desktop, "Qoder CN 0.2.5"));
        assert!(desktop_matches(&cn.desktop, "Qoder CN 0.2.5"));
        for spec in [&global, &cn] {
            assert!(spec.desktop.reject_sibling_files.contains(&"unins000.exe"));
            assert!(!desktop_matches(&spec.desktop, "QoderWork 1.0"));
        }
    }

    #[test]
    fn pi_is_an_npm_cli_without_desktop() {
        let pi = spec_by_id("pi").unwrap();
        assert_eq!(pi.cli.command, "pi");
        assert_eq!(pi.cli.npm_package, Some("@mariozechner/pi-coding-agent"));
        assert!(!pi.desktop_available);
    }

    fn desktop_matches(spec: &DesktopSpec, name: &str) -> bool {
        let lower = name.to_lowercase();
        spec.keywords.iter().any(|k| lower.contains(k))
            && !spec.excludes.iter().any(|k| lower.contains(k))
    }
}
