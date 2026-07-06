use crate::{backup, winenv};
use serde::Serialize;

struct BinaryMirrorSpec {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    description: &'static str,
    envs: &'static [(&'static str, &'static str)],
}

#[derive(Serialize)]
pub struct BinaryMirrorVar {
    pub name: String,
    pub value: String,
    pub current: Option<String>,
    pub matched: bool,
}

#[derive(Serialize)]
pub struct BinaryMirrorState {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub enabled: bool,
    pub configured: bool,
    pub status_label: String,
    pub vars: Vec<BinaryMirrorVar>,
}

#[derive(Serialize, Clone)]
pub struct BinaryCatalogEntry {
    pub id: String,
    pub name: String,
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
            icon: "ti-bolt",
            description: "Electron 与 electron-builder 下载预编译二进制时使用国内镜像。",
            envs: ELECTRON_ENVS,
        },
        BinaryMirrorSpec {
            id: "browser",
            name: "Puppeteer / Playwright",
            icon: "ti-browser",
            description: "浏览器自动化工具下载 Chromium / Playwright 浏览器包时使用国内镜像。",
            envs: BROWSER_ENVS,
        },
        BinaryMirrorSpec {
            id: "cypress",
            name: "Cypress",
            icon: "ti-test-pipe",
            description: "Cypress 安装或更新桌面运行器时使用国内镜像。",
            envs: CYPRESS_ENVS,
        },
        BinaryMirrorSpec {
            id: "native",
            name: "常见原生二进制",
            icon: "ti-package",
            description: "node-sass、SWC、sharp、Prisma、better-sqlite3 等安装时使用国内镜像。",
            envs: NATIVE_ENVS,
        },
        BinaryMirrorSpec {
            id: "huggingface",
            name: "HuggingFace 模型",
            icon: "ti-robot",
            description: "huggingface_hub / transformers 下载模型和数据集时使用 HF-Mirror。",
            envs: MODEL_ENVS,
        },
    ]
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
        .last()
        .unwrap_or("")
        .to_string()
}

pub fn catalog_entries() -> Vec<BinaryCatalogEntry> {
    specs()
        .iter()
        .filter_map(|spec| {
            let (_, url) = spec.envs.first()?;
            Some(BinaryCatalogEntry {
                id: spec.id.into(),
                name: spec.name.into(),
                description: spec.description.into(),
                url: (*url).into(),
                host: host_of(url),
            })
        })
        .collect()
}

fn read_effective_env(name: &str) -> Option<String> {
    winenv::get_raw_in(winenv::Hive::User, name)
        .or_else(|| winenv::get_raw_in(winenv::Hive::System, name))
        .or_else(|| std::env::var(name).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn state_for(spec: &BinaryMirrorSpec) -> BinaryMirrorState {
    let vars: Vec<BinaryMirrorVar> = spec
        .envs
        .iter()
        .map(|(name, value)| {
            let current = read_effective_env(name);
            let matched = current
                .as_deref()
                .map(|cur| normalize(cur) == normalize(value))
                .unwrap_or(false);
            BinaryMirrorVar {
                name: (*name).into(),
                value: (*value).into(),
                current,
                matched,
            }
        })
        .collect();
    let configured = vars.iter().any(|v| v.current.is_some());
    let enabled = !vars.is_empty() && vars.iter().all(|v| v.matched);
    let status_label = if enabled {
        "已加速"
    } else if configured {
        "自定义"
    } else {
        "默认"
    };
    BinaryMirrorState {
        id: spec.id.into(),
        name: spec.name.into(),
        icon: spec.icon.into(),
        description: spec.description.into(),
        enabled,
        configured,
        status_label: status_label.into(),
        vars,
    }
}

#[tauri::command]
pub fn binary_mirror_status() -> Vec<BinaryMirrorState> {
    specs().iter().map(state_for).collect()
}

#[tauri::command]
pub fn binary_mirror_apply(id: String) -> Result<(), String> {
    let spec = find_spec(&id)?;
    let vars: Vec<&str> = spec.envs.iter().map(|(name, _)| *name).collect();
    backup::backup_env(winenv::Hive::User, "binary-mirror", &vars);
    for (name, value) in spec.envs {
        winenv::set_user(name, value)?;
    }
    Ok(())
}

#[tauri::command]
pub fn binary_mirror_clear(id: String) -> Result<(), String> {
    let spec = find_spec(&id)?;
    let vars: Vec<&str> = spec.envs.iter().map(|(name, _)| *name).collect();
    backup::backup_env(winenv::Hive::User, "binary-mirror", &vars);
    for (name, _) in spec.envs {
        winenv::remove_user(name)?;
    }
    Ok(())
}
