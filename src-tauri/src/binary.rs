use crate::{backup, winenv};
use serde::Serialize;

struct BinaryMirrorSpec {
    id: &'static str,
    name: &'static str,
    source_name: &'static str,
    icon: &'static str,
    description: &'static str,
    envs: &'static [(&'static str, &'static str)],
}

#[derive(Serialize)]
pub struct BinaryMirrorVar {
    pub name: String,
    pub value: String,
    pub current: Option<String>,
    pub scope: Option<String>,
    pub matched: bool,
}

#[derive(Serialize)]
pub struct BinaryMirrorState {
    pub id: String,
    pub name: String,
    pub source_name: String,
    pub icon: String,
    pub description: String,
    pub enabled: bool,
    pub configured: bool,
    pub user_configured: bool,
    pub system_configured: bool,
    pub status_label: String,
    pub vars: Vec<BinaryMirrorVar>,
}

#[derive(Serialize, Clone)]
pub struct BinaryCatalogEntry {
    pub id: String,
    pub tool_id: String,
    pub name: String,
    pub source_name: String,
    pub description: String,
    pub url: String,
    pub host: String,
}

const ELECTRON_ENVS: &[(&str, &str)] = &[
    (
        "ELECTRON_MIRROR",
        "https://cdn.npmmirror.com/binaries/electron/",
    ),
    (
        "ELECTRON_BUILDER_BINARIES_MIRROR",
        "https://cdn.npmmirror.com/binaries/electron-builder-binaries/",
    ),
];

const BROWSER_ENVS: &[(&str, &str)] = &[
    (
        "PUPPETEER_DOWNLOAD_BASE_URL",
        "https://cdn.npmmirror.com/binaries/chrome-for-testing",
    ),
    (
        "PUPPETEER_DOWNLOAD_HOST",
        "https://cdn.npmmirror.com/binaries/chrome-for-testing",
    ),
    (
        "PUPPETEER_CHROME_DOWNLOAD_BASE_URL",
        "https://cdn.npmmirror.com/binaries/chrome-for-testing",
    ),
    (
        "PUPPETEER_CHROME_HEADLESS_SHELL_DOWNLOAD_BASE_URL",
        "https://cdn.npmmirror.com/binaries/chrome-for-testing",
    ),
    (
        "PLAYWRIGHT_DOWNLOAD_HOST",
        "https://cdn.npmmirror.com/binaries/playwright",
    ),
    (
        "PLAYWRIGHT_CHROMIUM_DOWNLOAD_HOST",
        "https://cdn.npmmirror.com/binaries/chrome-for-testing",
    ),
];

const CYPRESS_ENVS: &[(&str, &str)] = &[
    (
        "CYPRESS_DOWNLOAD_MIRROR",
        "https://cdn.npmmirror.com/binaries/cypress",
    ),
    (
        "CYPRESS_DOWNLOAD_PATH_TEMPLATE",
        "https://cdn.npmmirror.com/binaries/cypress/${version}/${platform}-${arch}/cypress.zip",
    ),
];

const NATIVE_ENVS: &[(&str, &str)] = &[
    (
        "SASS_BINARY_SITE",
        "https://cdn.npmmirror.com/binaries/node-sass",
    ),
    (
        "SWC_BINARY_SITE",
        "https://cdn.npmmirror.com/binaries/node-swc",
    ),
    (
        "PRISMA_ENGINES_MIRROR",
        "https://cdn.npmmirror.com/binaries/prisma",
    ),
];

// npm 11 会把这些旧式 npm_config_* 变量当作无效 npm 配置并在每次命令启动时警告。
// 旧版本 Stacker 曾写入这些值；启动时仅清理与旧内置值完全一致的用户级变量。
const LEGACY_NATIVE_ENVS: &[(&str, &str)] = &[
    (
        "npm_config_sharp_binary_host",
        "https://cdn.npmmirror.com/binaries/sharp",
    ),
    (
        "npm_config_sharp_libvips_binary_host",
        "https://cdn.npmmirror.com/binaries/sharp-libvips",
    ),
    (
        "npm_config_better_sqlite3_binary_host",
        "https://cdn.npmmirror.com/binaries/better-sqlite3",
    ),
];

const MODEL_ENVS: &[(&str, &str)] = &[("HF_ENDPOINT", "https://hf-mirror.com")];

fn specs() -> &'static [BinaryMirrorSpec] {
    &[
        BinaryMirrorSpec {
            id: "electron",
            name: "Electron",
            source_name: "npmmirror",
            icon: "ti-bolt",
            description: "Electron 与 electron-builder 下载预编译二进制时使用指定镜像。",
            envs: ELECTRON_ENVS,
        },
        BinaryMirrorSpec {
            id: "browser",
            name: "Puppeteer / Playwright",
            source_name: "npmmirror",
            icon: "ti-browser",
            description: "浏览器自动化工具下载 Chromium / Playwright 浏览器包时使用指定镜像。",
            envs: BROWSER_ENVS,
        },
        BinaryMirrorSpec {
            id: "cypress",
            name: "Cypress",
            source_name: "npmmirror",
            icon: "ti-test-pipe",
            description: "Cypress 安装或更新桌面运行器时使用指定镜像。",
            envs: CYPRESS_ENVS,
        },
        BinaryMirrorSpec {
            id: "native",
            name: "Node 原生工具",
            source_name: "npmmirror",
            icon: "ti-package",
            description: "node-sass、SWC 与 Prisma 下载原生组件时使用指定镜像。",
            envs: NATIVE_ENVS,
        },
        BinaryMirrorSpec {
            id: "huggingface",
            name: "HuggingFace 模型",
            source_name: "HF-Mirror",
            icon: "ti-robot",
            description: "huggingface_hub / transformers 下载模型和数据集时使用 HF-Mirror。",
            envs: MODEL_ENVS,
        },
    ]
}

pub fn migrate_legacy_envs() {
    let stale = LEGACY_NATIVE_ENVS.iter().any(|(name, expected)| {
        winenv::get_raw_in(winenv::Hive::User, name)
            .is_some_and(|current| normalize(&current) == normalize(expected))
    });
    if !stale {
        return;
    }
    let names = LEGACY_NATIVE_ENVS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    backup::backup_env(winenv::Hive::User, "binary-mirror-legacy", &names);
    for (name, expected) in LEGACY_NATIVE_ENVS {
        if winenv::get_raw_in(winenv::Hive::User, name)
            .is_some_and(|current| normalize(&current) == normalize(expected))
        {
            let _ = winenv::remove_user(name);
        }
    }
}

fn find_spec(id: &str) -> Result<&'static BinaryMirrorSpec, String> {
    specs()
        .iter()
        .find(|spec| spec.id == id)
        .ok_or_else(|| "未知下载镜像项".into())
}

fn normalize(value: &str) -> String {
    value.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn host_of(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    without_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .to_string()
}

pub fn catalog_entries() -> Vec<BinaryCatalogEntry> {
    specs()
        .iter()
        .flat_map(|spec| {
            let (_, url) = spec.envs.first()?;
            let official = match spec.id {
                "electron" => "https://github.com/electron/electron/releases",
                "browser" => "https://playwright.azureedge.net",
                "cypress" => "https://download.cypress.io",
                "native" => "https://github.com",
                "huggingface" => "https://huggingface.co",
                _ => "",
            };
            Some(vec![
                BinaryCatalogEntry {
                    id: format!("{}-official", spec.id),
                    tool_id: spec.id.into(),
                    name: spec.name.into(),
                    source_name: "官方默认".into(),
                    description: format!("{}；不写入镜像环境变量。", spec.description),
                    url: official.into(),
                    host: host_of(official),
                },
                BinaryCatalogEntry {
                    id: format!("{}-recommended", spec.id),
                    tool_id: spec.id.into(),
                    name: spec.name.into(),
                    source_name: spec.source_name.into(),
                    description: spec.description.into(),
                    url: (*url).into(),
                    host: host_of(url),
                },
            ])
        })
        .flatten()
        .collect()
}

fn read_persistent_env(name: &str) -> (Option<String>, Option<String>) {
    let clean = |value: Option<String>| {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    (
        clean(winenv::get_raw_in(winenv::Hive::User, name)),
        clean(winenv::get_raw_in(winenv::Hive::System, name)),
    )
}

fn state_for(spec: &BinaryMirrorSpec) -> BinaryMirrorState {
    let vars: Vec<BinaryMirrorVar> = spec
        .envs
        .iter()
        .map(|(name, value)| {
            let (user_current, system_current) = read_persistent_env(name);
            let (current, scope) = if let Some(value) = user_current {
                (Some(value), Some("当前用户".to_string()))
            } else if let Some(value) = system_current {
                (Some(value), Some("系统级".to_string()))
            } else {
                (None, None)
            };
            let matched = current
                .as_deref()
                .map(|cur| normalize(cur) == normalize(value))
                .unwrap_or(false);
            BinaryMirrorVar {
                name: (*name).into(),
                value: (*value).into(),
                current,
                scope,
                matched,
            }
        })
        .collect();
    let user_configured = spec
        .envs
        .iter()
        .any(|(name, _)| winenv::get_raw_in(winenv::Hive::User, name).is_some());
    let system_configured = spec
        .envs
        .iter()
        .any(|(name, _)| winenv::get_raw_in(winenv::Hive::System, name).is_some());
    let configured = vars.iter().any(|v| v.current.is_some());
    let enabled = !vars.is_empty() && vars.iter().all(|v| v.matched);
    let status_label = if enabled {
        "已配置"
    } else if configured {
        "自定义"
    } else {
        "默认"
    };
    BinaryMirrorState {
        id: spec.id.into(),
        name: spec.name.into(),
        source_name: spec.source_name.into(),
        icon: spec.icon.into(),
        description: spec.description.into(),
        enabled,
        configured,
        user_configured,
        system_configured,
        status_label: status_label.into(),
        vars,
    }
}

#[tauri::command]
pub fn binary_mirror_status() -> Vec<BinaryMirrorState> {
    specs().iter().map(state_for).collect()
}

#[tauri::command]
pub fn binary_mirror_apply(id: String) -> Result<BinaryMirrorState, String> {
    let spec = find_spec(&id)?;
    let vars: Vec<&str> = spec.envs.iter().map(|(name, _)| *name).collect();
    backup::backup_env(winenv::Hive::User, "binary-mirror", &vars);
    for (name, value) in spec.envs {
        winenv::set_user(name, value)?;
    }
    let state = state_for(spec);
    if !state.user_configured || !state.enabled {
        return Err("写入完成，但重新读取用户环境变量时未检测到预期配置".into());
    }
    Ok(state)
}

#[tauri::command]
pub fn binary_mirror_clear(id: String) -> Result<BinaryMirrorState, String> {
    let spec = find_spec(&id)?;
    let vars: Vec<&str> = spec.envs.iter().map(|(name, _)| *name).collect();
    backup::backup_env(winenv::Hive::User, "binary-mirror", &vars);
    for (name, _) in spec.envs {
        winenv::remove_user(name)?;
    }
    let state = state_for(spec);
    if state.user_configured {
        return Err("清除完成，但重新读取时仍检测到用户级环境变量".into());
    }
    Ok(state)
}
