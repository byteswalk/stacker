//! 软件换源：内置镜像清单、检测当前源、切换（改前自动备份）。
//! 默认操作"用户级"配置/环境变量，一处切换覆盖该用户所有版本。

use crate::{backup, winenv};
use ini::Ini;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Mirror {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub host: String,
}

#[derive(Clone)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub icon: String, // 前端头像配色 class：py/npm/go/...
    pub handler: String,
    pub probe: String, // 探测是否安装的命令名
    pub mirrors: Vec<Mirror>,
}

#[derive(Serialize, Clone)]
pub struct ToolState {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub config: String,
    pub installed: bool,
    pub current: Option<String>,
    pub current_label: String,
    pub mirrors: Vec<Mirror>,
}

fn m(id: &str, name: &str, url: &str, host: &str) -> Mirror {
    Mirror {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        host: host.into(),
    }
}

fn mk(id: &str, name: &str, icon: &str, handler: &str, probe: &str, mirrors: Vec<Mirror>) -> Tool {
    Tool {
        id: id.into(),
        name: name.into(),
        icon: icon.into(),
        handler: handler.into(),
        probe: probe.into(),
        mirrors,
    }
}

/// 全部工具（兜底内置清单 → 远程清单覆盖 → 合并自定义源镜像）。
pub fn tools() -> Vec<Tool> {
    let mut list = hardcoded();
    crate::update::overlay(&mut list);
    crate::custom::merge_into(&mut list);
    list
}

/// 内置工具 id 列表（用于校验自定义源所属工具）。
pub fn tools_builtin_ids() -> Vec<&'static str> {
    vec![
        "python-runtime",
        "node-runtime",
        "git-runtime",
        "maven-runtime",
        "gradle-runtime",
        "go-runtime",
        "rust-runtime",
        "pip",
        "npm",
        "yarn",
        "go",
        "maven",
        "gradle",
        "conda",
        "cargo",
    ]
}

pub const PYTHON_RUNTIME_TOOL_ID: &str = "python-runtime";
pub const NODE_RUNTIME_TOOL_ID: &str = "node-runtime";
pub const GIT_RUNTIME_TOOL_ID: &str = "git-runtime";
pub const MAVEN_RUNTIME_TOOL_ID: &str = "maven-runtime";
pub const GRADLE_RUNTIME_TOOL_ID: &str = "gradle-runtime";
pub const GO_RUNTIME_TOOL_ID: &str = "go-runtime";
pub const RUST_RUNTIME_TOOL_ID: &str = "rust-runtime";

fn rust_runtime_mirrors_builtin() -> Vec<Mirror> {
    vec![
        m(
            "official",
            "官方 Rust",
            "https://static.rust-lang.org",
            "static.rust-lang.org",
        ),
        m("rsproxy", "rsproxy.cn", "https://rsproxy.cn", "rsproxy.cn"),
        m(
            "tuna",
            "清华大学",
            "https://mirrors.tuna.tsinghua.edu.cn/rustup",
            "mirrors.tuna.tsinghua.edu.cn",
        ),
        m(
            "ustc",
            "中科大",
            "https://mirrors.ustc.edu.cn/rust-static",
            "mirrors.ustc.edu.cn",
        ),
    ]
}

#[allow(dead_code)]
pub fn rust_runtime_mirrors() -> Vec<Mirror> {
    tools()
        .into_iter()
        .find(|t| t.id == RUST_RUNTIME_TOOL_ID)
        .map(|t| t.mirrors)
        .unwrap_or_else(rust_runtime_mirrors_builtin)
}

fn python_runtime_mirrors_builtin() -> Vec<Mirror> {
    vec![
        m(
            "official",
            "官方",
            "https://www.python.org/ftp/python/{version}/{filename}",
            "www.python.org",
        ),
        m(
            "tuna",
            "清华",
            "https://mirrors.tuna.tsinghua.edu.cn/python/{version}/{filename}",
            "mirrors.tuna.tsinghua.edu.cn",
        ),
        m(
            "aliyun",
            "阿里云",
            "https://mirrors.aliyun.com/python-release/windows/{filename}",
            "mirrors.aliyun.com",
        ),
        m(
            "huawei",
            "华为云",
            "https://repo.huaweicloud.com/python/{version}/{filename}",
            "repo.huaweicloud.com",
        ),
        m(
            "ustc",
            "中科大",
            "https://mirrors.ustc.edu.cn/python/{version}/{filename}",
            "mirrors.ustc.edu.cn",
        ),
        m(
            "bfsu",
            "北外",
            "https://mirrors.bfsu.edu.cn/python/{version}/{filename}",
            "mirrors.bfsu.edu.cn",
        ),
        m(
            "nju",
            "南京大学",
            "https://mirror.nju.edu.cn/python/{version}/{filename}",
            "mirror.nju.edu.cn",
        ),
    ]
}

pub fn python_runtime_mirrors() -> Vec<Mirror> {
    tools()
        .into_iter()
        .find(|t| t.id == PYTHON_RUNTIME_TOOL_ID)
        .map(|t| t.mirrors)
        .unwrap_or_else(python_runtime_mirrors_builtin)
}

fn node_runtime_mirrors_builtin() -> Vec<Mirror> {
    vec![
        m("official", "官方", "https://nodejs.org/dist", "nodejs.org"),
        m(
            "npmmirror",
            "npmmirror",
            "https://npmmirror.com/mirrors/node",
            "npmmirror.com",
        ),
        m(
            "tencent",
            "腾讯云",
            "https://mirrors.cloud.tencent.com/nodejs-release",
            "mirrors.cloud.tencent.com",
        ),
        m(
            "huawei",
            "华为云",
            "https://repo.huaweicloud.com/nodejs",
            "repo.huaweicloud.com",
        ),
        m(
            "tuna",
            "清华",
            "https://mirrors.tuna.tsinghua.edu.cn/nodejs-release",
            "mirrors.tuna.tsinghua.edu.cn",
        ),
        m(
            "bfsu",
            "北外",
            "https://mirrors.bfsu.edu.cn/nodejs-release",
            "mirrors.bfsu.edu.cn",
        ),
        m(
            "nju",
            "南京大学",
            "https://mirror.nju.edu.cn/nodejs-release",
            "mirror.nju.edu.cn",
        ),
        m(
            "sjtug",
            "上交",
            "https://mirrors.sjtug.sjtu.edu.cn/nodejs-release",
            "mirrors.sjtug.sjtu.edu.cn",
        ),
    ]
}

pub fn node_runtime_mirrors() -> Vec<Mirror> {
    tools()
        .into_iter()
        .find(|t| t.id == NODE_RUNTIME_TOOL_ID)
        .map(|t| t.mirrors)
        .unwrap_or_else(node_runtime_mirrors_builtin)
}

fn git_runtime_mirrors_builtin() -> Vec<Mirror> {
    vec![
        m(
            "official",
            "官方",
            "https://github.com/git-for-windows/git/releases/latest",
            "github.com",
        ),
        m(
            "npmmirror",
            "npmmirror",
            "https://registry.npmmirror.com/-/binary/git-for-windows/",
            "registry.npmmirror.com",
        ),
        m(
            "tuna",
            "清华",
            "https://mirrors.tuna.tsinghua.edu.cn/github-release/git-for-windows/git/",
            "mirrors.tuna.tsinghua.edu.cn",
        ),
        m(
            "huawei",
            "华为云",
            "https://repo.huaweicloud.com/git-for-windows/",
            "repo.huaweicloud.com",
        ),
    ]
}

pub fn git_runtime_mirrors() -> Vec<Mirror> {
    tools()
        .into_iter()
        .find(|t| t.id == GIT_RUNTIME_TOOL_ID)
        .and_then(|t| (!t.mirrors.is_empty()).then_some(t.mirrors))
        .unwrap_or_else(git_runtime_mirrors_builtin)
}

fn maven_runtime_mirrors_builtin() -> Vec<Mirror> {
    vec![
        m(
            "official",
            "官方 Apache",
            "https://archive.apache.org/dist/maven",
            "archive.apache.org",
        ),
        m(
            "apache-cdn",
            "Apache CDN",
            "https://dlcdn.apache.org/maven",
            "dlcdn.apache.org",
        ),
        m(
            "tuna",
            "清华大学",
            "https://mirrors.tuna.tsinghua.edu.cn/apache/maven",
            "mirrors.tuna.tsinghua.edu.cn",
        ),
        m(
            "ustc",
            "中科大",
            "https://mirrors.ustc.edu.cn/apache/maven",
            "mirrors.ustc.edu.cn",
        ),
        m(
            "aliyun",
            "阿里云",
            "https://mirrors.aliyun.com/apache/maven",
            "mirrors.aliyun.com",
        ),
        m(
            "huawei",
            "华为云",
            "https://repo.huaweicloud.com/apache/maven",
            "repo.huaweicloud.com",
        ),
        m(
            "tencent",
            "腾讯云",
            "https://mirrors.cloud.tencent.com/apache/maven",
            "mirrors.cloud.tencent.com",
        ),
    ]
}

fn gradle_runtime_mirrors_builtin() -> Vec<Mirror> {
    vec![
        m(
            "official",
            "官方 Gradle",
            "https://services.gradle.org/distributions",
            "services.gradle.org",
        ),
        m(
            "tencent",
            "腾讯云",
            "https://mirrors.cloud.tencent.com/gradle",
            "mirrors.cloud.tencent.com",
        ),
        m(
            "aliyun",
            "阿里云",
            "https://mirrors.aliyun.com/gradle/distributions",
            "mirrors.aliyun.com",
        ),
        m(
            "huawei",
            "华为云",
            "https://repo.huaweicloud.com/gradle",
            "repo.huaweicloud.com",
        ),
    ]
}

fn go_runtime_mirrors_builtin() -> Vec<Mirror> {
    vec![
        m("official", "官方 go.dev", "https://go.dev/dl", "go.dev"),
        m(
            "aliyun",
            "阿里云镜像",
            "https://mirrors.aliyun.com/golang",
            "mirrors.aliyun.com",
        ),
    ]
}

/// 代码里写死的兜底工具 + 镜像清单（不含远程覆盖与自定义源）。
pub fn hardcoded() -> Vec<Tool> {
    vec![
        mk(
            PYTHON_RUNTIME_TOOL_ID,
            "Python 下载源",
            "py",
            "runtime_download",
            "",
            python_runtime_mirrors_builtin(),
        ),
        mk(
            NODE_RUNTIME_TOOL_ID,
            "Node 下载源",
            "npm",
            "runtime_download",
            "",
            node_runtime_mirrors_builtin(),
        ),
        mk(
            GIT_RUNTIME_TOOL_ID,
            "Git 下载源",
            "st",
            "runtime_download",
            "",
            git_runtime_mirrors_builtin(),
        ),
        mk(
            MAVEN_RUNTIME_TOOL_ID,
            "Maven 下载源",
            "mv2",
            "runtime_download",
            "",
            maven_runtime_mirrors_builtin(),
        ),
        mk(
            GRADLE_RUNTIME_TOOL_ID,
            "Gradle 下载源",
            "gr",
            "runtime_download",
            "",
            gradle_runtime_mirrors_builtin(),
        ),
        mk(
            GO_RUNTIME_TOOL_ID,
            "Go 下载源",
            "go",
            "runtime_download",
            "",
            go_runtime_mirrors_builtin(),
        ),
        mk(
            RUST_RUNTIME_TOOL_ID,
            "Rust 工具链下载源",
            "rs",
            "runtime_download",
            "",
            rust_runtime_mirrors_builtin(),
        ),
        mk(
            "pip",
            "pip",
            "py",
            "pip_ini",
            "pip,python,python3,py",
            vec![
                m(
                    "official",
                    "官方 PyPI",
                    "https://pypi.org/simple",
                    "pypi.org",
                ),
                m(
                    "tsinghua",
                    "清华大学",
                    "https://pypi.tuna.tsinghua.edu.cn/simple",
                    "pypi.tuna.tsinghua.edu.cn",
                ),
                m(
                    "aliyun",
                    "阿里云",
                    "https://mirrors.aliyun.com/pypi/simple/",
                    "mirrors.aliyun.com",
                ),
                m(
                    "ustc",
                    "中科大",
                    "https://pypi.mirrors.ustc.edu.cn/simple/",
                    "pypi.mirrors.ustc.edu.cn",
                ),
                m(
                    "tencent",
                    "腾讯云",
                    "https://mirrors.cloud.tencent.com/pypi/simple",
                    "mirrors.cloud.tencent.com",
                ),
            ],
        ),
        mk(
            "npm",
            "npm / pnpm",
            "npm",
            "npmrc",
            "npm",
            vec![
                m(
                    "official",
                    "官方 registry",
                    "https://registry.npmjs.org/",
                    "registry.npmjs.org",
                ),
                m(
                    "npmmirror",
                    "npmmirror (淘宝)",
                    "https://registry.npmmirror.com/",
                    "registry.npmmirror.com",
                ),
                m(
                    "tencent",
                    "腾讯云",
                    "https://mirrors.cloud.tencent.com/npm/",
                    "mirrors.cloud.tencent.com",
                ),
            ],
        ),
        mk(
            "yarn",
            "yarn",
            "yn",
            "yarnrc",
            "yarn",
            vec![
                m(
                    "official",
                    "官方 registry",
                    "https://registry.yarnpkg.com/",
                    "registry.yarnpkg.com",
                ),
                m(
                    "npmmirror",
                    "npmmirror (淘宝)",
                    "https://registry.npmmirror.com/",
                    "registry.npmmirror.com",
                ),
            ],
        ),
        mk(
            "go",
            "Go (GOPROXY)",
            "go",
            "go_env",
            "go",
            vec![
                m(
                    "official",
                    "官方 proxy",
                    "https://proxy.golang.org,direct",
                    "",
                ),
                m(
                    "goproxyio",
                    "goproxy.io",
                    "https://goproxy.io,direct",
                    "goproxy.io",
                ),
                m(
                    "goproxycn",
                    "goproxy.cn",
                    "https://goproxy.cn,direct",
                    "goproxy.cn",
                ),
                m(
                    "aliyun",
                    "阿里云",
                    "https://mirrors.aliyun.com/goproxy/,direct",
                    "mirrors.aliyun.com",
                ),
                m(
                    "tencent",
                    "腾讯云",
                    "https://mirrors.tencent.com/go/,direct",
                    "mirrors.tencent.com",
                ),
            ],
        ),
        mk(
            "maven",
            "Maven",
            "mv2",
            "maven_settings",
            "mvn",
            vec![
                m("official", "官方 Central", "", "repo.maven.apache.org"),
                m(
                    "maven-central-repo1",
                    "Maven Central (repo1)",
                    "https://repo1.maven.org/maven2/",
                    "repo1.maven.org",
                ),
                m(
                    "tencent",
                    "腾讯云",
                    "https://mirrors.cloud.tencent.com/nexus/repository/maven-public/",
                    "mirrors.cloud.tencent.com",
                ),
                m(
                    "aliyun",
                    "阿里云",
                    "https://maven.aliyun.com/repository/public",
                    "maven.aliyun.com",
                ),
                m(
                    "huawei",
                    "华为云",
                    "https://repo.huaweicloud.com/repository/maven/",
                    "repo.huaweicloud.com",
                ),
            ],
        ),
        mk(
            "gradle",
            "Gradle",
            "gr",
            "gradle_init",
            "gradle",
            vec![
                m("official", "官方", "", "repo.maven.apache.org"),
                m(
                    "maven-central-repo1",
                    "Maven Central (repo1)",
                    "https://repo1.maven.org/maven2/",
                    "repo1.maven.org",
                ),
                m(
                    "tencent",
                    "腾讯云",
                    "https://mirrors.cloud.tencent.com/nexus/repository/maven-public/",
                    "mirrors.cloud.tencent.com",
                ),
                m(
                    "aliyun",
                    "阿里云",
                    "https://maven.aliyun.com/repository/public",
                    "maven.aliyun.com",
                ),
                m(
                    "huawei",
                    "华为云",
                    "https://repo.huaweicloud.com/repository/maven/",
                    "repo.huaweicloud.com",
                ),
            ],
        ),
        mk(
            "conda",
            "conda",
            "cd",
            "condarc",
            "conda",
            vec![
                m("official", "官方 defaults", "", ""),
                m(
                    "tsinghua",
                    "清华大学",
                    "https://mirrors.tuna.tsinghua.edu.cn/anaconda",
                    "mirrors.tuna.tsinghua.edu.cn",
                ),
                m(
                    "ustc",
                    "中科大",
                    "https://mirrors.ustc.edu.cn/anaconda",
                    "mirrors.ustc.edu.cn",
                ),
            ],
        ),
        mk(
            "cargo",
            "Cargo (Rust)",
            "rs",
            "cargo_config",
            "cargo",
            vec![
                m("official", "官方 crates.io", "", ""),
                m(
                    "rsproxy",
                    "rsproxy.cn",
                    "sparse+https://rsproxy.cn/index/",
                    "rsproxy.cn",
                ),
                m(
                    "ustc",
                    "中科大",
                    "sparse+https://mirrors.ustc.edu.cn/crates.io-index/",
                    "mirrors.ustc.edu.cn",
                ),
                m(
                    "tuna",
                    "清华大学",
                    "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/",
                    "mirrors.tuna.tsinghua.edu.cn",
                ),
            ],
        ),
    ]
}

// ── 路径 ──
fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}
fn appdata() -> PathBuf {
    dirs::config_dir().unwrap_or_default()
} // %APPDATA%\Roaming
pub fn pip_path() -> PathBuf {
    appdata().join("pip").join("pip.ini")
}
pub fn npmrc_path() -> PathBuf {
    home().join(".npmrc")
}
pub fn yarnrc_path() -> PathBuf {
    home().join(".yarnrc")
}
fn condarc_path() -> PathBuf {
    home().join(".condarc")
}
pub fn cargo_path() -> PathBuf {
    let cargo_home = winenv::get_raw_in(winenv::Hive::User, "CARGO_HOME")
        .or_else(|| winenv::get_raw_in(winenv::Hive::System, "CARGO_HOME"))
        .or_else(|| std::env::var("CARGO_HOME").ok())
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cargo"));
    cargo_home.join("config.toml")
}
pub fn maven_path() -> PathBuf {
    home().join(".m2").join("settings.xml")
}
pub fn gradle_path() -> PathBuf {
    home().join(".gradle").join("init.gradle")
}

fn config_display(handler: &str) -> String {
    match handler {
        "pip_ini" => pip_path().to_string_lossy().into(),
        "npmrc" => npmrc_path().to_string_lossy().into(),
        "yarnrc" => yarnrc_path().to_string_lossy().into(),
        "go_env" => "环境变量 GOPROXY".into(),
        "condarc" => condarc_path().to_string_lossy().into(),
        "cargo_config" => cargo_path().to_string_lossy().into(),
        "maven_settings" => maven_path().to_string_lossy().into(),
        "gradle_init" => gradle_path().to_string_lossy().into(),
        "runtime_download" => "Stacker 内部下载源".into(),
        _ => String::new(),
    }
}

// ── 工具助手 ──
fn read_text(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}
fn write_text(p: &Path, s: &str) -> Result<(), String> {
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, s).map_err(|e| e.to_string())
}
fn strip_url_creds(u: &str) -> String {
    for scheme in ["sparse+https://", "sparse+http://", "https://", "http://"] {
        if let Some(rest) = u.strip_prefix(scheme) {
            let slash = rest.find('/').unwrap_or(rest.len());
            let host_part = &rest[..slash];
            if let Some(at) = host_part.rfind('@') {
                return format!("{scheme}{}{}", &host_part[at + 1..], &rest[slash..]);
            }
            return u.to_string();
        }
    }
    u.to_string()
}

fn norm(u: &str) -> String {
    strip_url_creds(u.trim())
        .trim_end_matches('/')
        .to_lowercase()
}

// probe 可为逗号分隔的多个候选命令，任一找得到即视为已安装。
// 搜索目录：注册表最新 PATH（不受 app 启动时的旧快照影响）＋ app 进程 PATH 兜底
// ＋ fnm 默认 Node 安装目录（fnm 的 node/npm 只在终端启动时按钩子注入 PATH，
//   不在任何持久化 PATH 上，否则 npm/pnpm 永远「未检测到」）。
fn cmd_on_path(probe: &str) -> bool {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    #[cfg(windows)]
    if let Some(nd) = crate::fnm::default_node_dir() {
        dirs.push(nd);
    }
    for cmd in probe.split(',') {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            continue;
        }
        for dir in &dirs {
            // 跳过 Microsoft Store 的应用执行别名（WindowsApps 里有假的 python.exe / python3.exe 占位，
            // 点了只会弹商店、不是真 Python）——否则干净机会把 pip 误判为"已安装"。
            if dir
                .to_string_lossy()
                .to_lowercase()
                .contains("\\windowsapps")
            {
                continue;
            }
            for ext in ["exe", "cmd", "bat", ""] {
                let name = if ext.is_empty() {
                    cmd.to_string()
                } else {
                    format!("{cmd}.{ext}")
                };
                if dir.join(&name).is_file() {
                    return true;
                }
            }
        }
    }
    false
}

fn read_line_key(p: &Path, key: &str, quoted: bool) -> Option<String> {
    let text = read_text(p)?;
    for line in text.lines() {
        let t = line.trim();
        if quoted {
            if let Some(rest) = t.strip_prefix(&format!("{key} ")) {
                return Some(rest.trim().trim_matches('"').to_string());
            }
        } else if let Some(rest) = t.strip_prefix(&format!("{key}=")) {
            return Some(rest.trim().to_string());
        }
    }
    None
}
fn write_line_key(p: &Path, key: &str, value: &str, quoted: bool) -> Result<(), String> {
    let text = read_text(p).unwrap_or_default();
    let newline = if quoted {
        format!("{key} \"{value}\"")
    } else {
        format!("{key}={value}")
    };
    let mut out = String::new();
    let mut replaced = false;
    for line in text.lines() {
        let t = line.trim_start();
        let is = if quoted {
            t.starts_with(&format!("{key} "))
        } else {
            t.starts_with(&format!("{key}="))
        };
        if is {
            out.push_str(&newline);
            out.push('\n');
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&newline);
        out.push('\n');
    }
    write_text(p, &out)
}

// ── 模板 ──
fn cargo_template(url: &str) -> String {
    format!(
        "[source.crates-io]\nreplace-with = \"mirror\"\n\n[source.mirror]\nregistry = \"{url}\"\n"
    )
}
fn condarc_template(base: &str) -> String {
    format!("channels:\n  - defaults\nshow_channel_urls: true\ndefault_channels:\n  - {base}/pkgs/main\n  - {base}/pkgs/r\n  - {base}/pkgs/msys2\ncustom_channels:\n  conda-forge: {base}/cloud\n  pytorch: {base}/cloud\n")
}
fn gradle_repo_urls(url: &str) -> Vec<String> {
    let mut urls = vec![url.to_string()];
    if url
        .trim_end_matches('/')
        .eq_ignore_ascii_case("https://maven.aliyun.com/repository/public")
    {
        urls.push("https://maven.aliyun.com/repository/google".into());
        urls.push("https://maven.aliyun.com/repository/gradle-plugin".into());
    }
    urls
}

fn groovy_list(values: &[String]) -> String {
    values
        .iter()
        .map(|v| format!("'{}'", groovy_single_escape(v)))
        .collect::<Vec<_>>()
        .join(",\n    ")
}

fn gradle_template(url: &str) -> String {
    let urls = gradle_repo_urls(url);
    let urls = groovy_list(&urls);
    format!(
        "def stackerRepoUrls = [\n\
    {urls}\n\
]\n\
def stackerApplyRepos = {{ settings, repositories, includePluginPortal ->\n\
    repositories.clear()\n\
    stackerRepoUrls.each {{ repoUrl -> repositories.maven {{ url = settings.uri(repoUrl) }} }}\n\
    repositories.google()\n\
    repositories.mavenCentral()\n\
    if (includePluginPortal) {{ repositories.gradlePluginPortal() }}\n\
}}\n\
def stackerSettingsReposApplied = false\n\
settingsEvaluated {{ settings ->\n\
    try {{\n\
        settings.pluginManagement {{ repositories {{ stackerApplyRepos(settings, delegate, true) }} }}\n\
    }} catch (Throwable ignored) {{}}\n\
    try {{\n\
        settings.dependencyResolutionManagement {{ repositories {{ stackerApplyRepos(settings, delegate, false) }} }}\n\
        stackerSettingsReposApplied = true\n\
    }} catch (Throwable ignored) {{}}\n\
}}\n\
gradle.projectsLoaded {{ gradle ->\n\
    if (!stackerSettingsReposApplied) {{\n\
        gradle.rootProject.allprojects {{ project ->\n\
            project.repositories.clear()\n\
            stackerRepoUrls.each {{ repoUrl -> project.repositories.maven {{ url = project.uri(repoUrl) }} }}\n\
            project.repositories.google()\n\
            project.repositories.mavenCentral()\n\
        }}\n\
    }}\n\
}}\n"
    )
}

struct ToolProxy {
    host: String,
    port: u16,
}

fn no_proxy_hosts(mirror: &Mirror) -> String {
    let mut hosts = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    for h in domestic_hosts() {
        if !hosts.contains(&h) {
            hosts.push(h);
        }
    }
    if !mirror.host.trim().is_empty() && !hosts.contains(&mirror.host) {
        hosts.push(mirror.host.clone());
    }
    hosts.join("|")
}

fn maven_settings_template(mirror: &Mirror, proxy: Option<&ToolProxy>) -> String {
    let mut out = String::from("<settings>\n");
    if mirror.id != "official" && !mirror.url.trim().is_empty() {
        out.push_str(&format!(
            "  <mirrors>\n    <mirror>\n      <id>stacker-mirror</id>\n      <mirrorOf>central</mirrorOf>\n      <name>{}</name>\n      <url>{}</url>\n    </mirror>\n  </mirrors>\n",
            mirror.name, mirror.url
        ));
    }
    if let Some(proxy) = proxy {
        let non = no_proxy_hosts(mirror);
        out.push_str(&format!(
            "  <proxies>\n    <proxy>\n      <id>stacker-http</id>\n      <active>true</active>\n      <protocol>http</protocol>\n      <host>{}</host>\n      <port>{}</port>\n      <nonProxyHosts>{}</nonProxyHosts>\n    </proxy>\n    <proxy>\n      <id>stacker-https</id>\n      <active>true</active>\n      <protocol>https</protocol>\n      <host>{}</host>\n      <port>{}</port>\n      <nonProxyHosts>{}</nonProxyHosts>\n    </proxy>\n  </proxies>\n",
            proxy.host, proxy.port, non, proxy.host, proxy.port, non
        ));
    }
    out.push_str("</settings>\n");
    out
}

const MAVEN_PROXY_FLAGS: [&str; 6] = [
    "-Dhttp.proxyHost",
    "-Dhttp.proxyPort",
    "-Dhttps.proxyHost",
    "-Dhttps.proxyPort",
    "-Dhttp.nonProxyHosts",
    "-Dhttps.nonProxyHosts",
];

pub(crate) fn strip_maven_legacy_proxy_opts(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|t| !MAVEN_PROXY_FLAGS.iter().any(|p| t.starts_with(p)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_maven_legacy_proxy_opts(raw: &str) -> bool {
    MAVEN_PROXY_FLAGS.iter().any(|p| raw.contains(p))
}

pub(crate) fn clear_maven_legacy_proxy_opts() -> Result<bool, String> {
    let user_raw = winenv::get_user_raw("MAVEN_OPTS").unwrap_or_default();
    let process_raw = std::env::var("MAVEN_OPTS").unwrap_or_default();
    let user_has = has_maven_legacy_proxy_opts(&user_raw);
    let process_has = has_maven_legacy_proxy_opts(&process_raw);
    if !user_has && !process_has {
        return Ok(false);
    }

    if process_has {
        let keep = strip_maven_legacy_proxy_opts(&process_raw);
        if keep.trim().is_empty() {
            std::env::remove_var("MAVEN_OPTS");
        } else {
            std::env::set_var("MAVEN_OPTS", keep);
        }
    }

    if !user_has {
        return Ok(true);
    }

    if user_raw.trim().is_empty() {
        return Ok(true);
    }
    backup::backup_env(winenv::Hive::User, "maven-proxy", &["MAVEN_OPTS"]);
    let keep = strip_maven_legacy_proxy_opts(&user_raw);
    if keep.trim().is_empty() {
        winenv::remove_user("MAVEN_OPTS")?;
    } else {
        winenv::set_user("MAVEN_OPTS", &keep)?;
    }
    Ok(true)
}

fn maven_apply(
    path: PathBuf,
    mirror: &Mirror,
    proxy: Option<&ToolProxy>,
    _proxy_requested: bool,
) -> Result<(), String> {
    clear_maven_legacy_proxy_opts()?;
    backup::backup_file(&path);
    if mirror.id == "official" && proxy.is_none() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    write_text(&path, &maven_settings_template(mirror, proxy))
}

fn maven_proxy_has_at(path: &Path) -> bool {
    read_text(path)
        .map(|text| {
            let low = text.to_lowercase();
            low.contains("<proxies>") && low.contains("<active>true</active>")
        })
        .unwrap_or(false)
}

fn gradle_proxy_has_at(init_path: &Path) -> bool {
    read_text(init_path)
        .map(|text| {
            text.lines().any(|l| {
                l.trim_start()
                    .starts_with("System.setProperty('http.proxyHost'")
            })
        })
        .unwrap_or(false)
}

fn groovy_single_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn gradle_init_template(mirror: &Mirror, proxy: Option<&ToolProxy>) -> String {
    let mut out = String::new();
    if let Some(proxy) = proxy {
        let host = groovy_single_escape(&proxy.host);
        let port = proxy.port.to_string();
        let non = groovy_single_escape(&no_proxy_hosts(mirror));
        for (k, v) in [
            ("http.proxyHost", host.clone()),
            ("http.proxyPort", port.clone()),
            ("https.proxyHost", host),
            ("https.proxyPort", port),
            ("http.nonProxyHosts", non.clone()),
            ("https.nonProxyHosts", non),
        ] {
            out.push_str(&format!("System.setProperty('{k}', '{v}')\n"));
        }
        out.push('\n');
    }
    if mirror.id != "official" && !mirror.url.trim().is_empty() {
        out.push_str(&gradle_template(&mirror.url));
    }
    out
}

fn gradle_apply(
    init_path: PathBuf,
    mirror: &Mirror,
    proxy: Option<&ToolProxy>,
    proxy_requested: bool,
) -> Result<(), String> {
    if !proxy_requested {
        return managed_apply(init_path, mirror, gradle_template);
    }
    backup::backup_file(&init_path);
    if mirror.id == "official" && proxy.is_none() {
        if init_path.exists() {
            std::fs::remove_file(&init_path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    write_text(&init_path, &gradle_init_template(mirror, proxy))
}

fn parse_proxy(
    proxy_enabled: Option<bool>,
    proxy_host: Option<String>,
    proxy_port: Option<u16>,
) -> Option<ToolProxy> {
    if proxy_enabled != Some(true) {
        return None;
    }
    let host = proxy_host
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "127.0.0.1".into());
    Some(ToolProxy {
        host,
        port: proxy_port.unwrap_or(7890),
    })
}

fn match_url(tool: &Tool, cur: &str) -> Option<String> {
    if cur.trim().is_empty() {
        return tool
            .mirrors
            .iter()
            .find(|m| m.id == "official")
            .map(|m| m.id.clone());
    }
    let n = norm(cur);
    tool.mirrors
        .iter()
        .find(|m| !m.url.is_empty() && norm(&m.url) == n)
        .map(|m| m.id.clone())
}

fn managed_detect_contains(tool: &Tool, path: &Path, by_host: bool) -> Option<String> {
    let Some(text) = read_text(path) else {
        return Some("official".into());
    };
    let low = text.to_lowercase();
    for m in &tool.mirrors {
        let needle = if by_host { &m.host } else { &m.url };
        if !needle.is_empty() && (text.contains(needle) || low.contains(&norm(needle))) {
            return Some(m.id.clone());
        }
    }
    Some("official".into())
}

// ── 检测当前源 ──
pub fn detect(tool: &Tool) -> Option<String> {
    match tool.handler.as_str() {
        "pip_ini" => {
            let p = pip_path();
            if !p.exists() {
                return Some("official".into());
            }
            let conf = Ini::load_from_file(&p).ok()?;
            let url = conf.get_from(Some("global"), "index-url").unwrap_or("");
            match_url(tool, url)
        }
        "npmrc" => match_url(
            tool,
            &read_line_key(&npmrc_path(), "registry", false).unwrap_or_default(),
        ),
        "yarnrc" => match_url(
            tool,
            &read_line_key(&yarnrc_path(), "registry", true).unwrap_or_default(),
        ),
        "go_env" => {
            let cur = winenv::get_user_raw("GOPROXY")
                .or_else(|| winenv::get_raw_in(winenv::Hive::System, "GOPROXY"))
                .unwrap_or_default();
            match_url(tool, &cur)
        }
        "cargo_config" => managed_detect_contains(tool, &cargo_path(), false),
        "maven_settings" => managed_detect_contains(tool, &maven_path(), false),
        "gradle_init" => managed_detect_contains(tool, &gradle_path(), false),
        "condarc" => managed_detect_contains(tool, &condarc_path(), true),
        // 下载源选择保存在各生态页面的本地设置中；后端无法可靠判断当前项，
        // 因此源目录不伪造“官方源为当前源”的状态。
        "runtime_download" => None,
        _ => None,
    }
}

fn managed_apply(path: PathBuf, mirror: &Mirror, tmpl: fn(&str) -> String) -> Result<(), String> {
    backup::backup_file(&path);
    if mirror.id == "official" {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    } else {
        write_text(&path, &tmpl(&mirror.url))
    }
}

fn pip_mirror_id_at(path: &Path, tool: &Tool) -> Option<String> {
    if !path.exists() {
        return Some("official".into());
    }
    let conf = Ini::load_from_file(path).ok()?;
    let url = conf.get_from(Some("global"), "index-url").unwrap_or("");
    match_url(tool, url)
}

fn pip_configured_at(path: &Path) -> bool {
    Ini::load_from_file(path)
        .ok()
        .and_then(|conf| {
            conf.get_from(Some("global"), "index-url")
                .map(|s| !s.trim().is_empty())
        })
        .unwrap_or(false)
}

pub fn write_pip_source_to_path(
    path: &Path,
    mirror: &Mirror,
    backup_first: bool,
) -> Result<(), String> {
    if backup_first {
        backup::backup_file(path);
    }
    if let Some(par) = path.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    let mut conf = Ini::load_from_file(path).unwrap_or_else(|_| Ini::new());
    conf.with_section(Some("global"))
        .set("index-url", mirror.url.as_str());
    if let Some(sec) = conf.section_mut(Some("install")) {
        sec.remove("trusted-host");
    }
    if mirror.id != "official" && !mirror.host.is_empty() {
        conf.with_section(Some("install"))
            .set("trusted-host", mirror.host.as_str());
    }
    conf.write_to_file(path).map_err(|e| e.to_string())
}

pub fn clear_pip_source_at(path: &Path, backup_first: bool) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if backup_first {
        backup::backup_file(path);
    }
    let mut conf = Ini::load_from_file(path).unwrap_or_else(|_| Ini::new());
    if let Some(sec) = conf.section_mut(Some("global")) {
        sec.remove("index-url");
    }
    if let Some(sec) = conf.section_mut(Some("install")) {
        sec.remove("trusted-host");
    }
    conf.write_to_file(path).map_err(|e| e.to_string())
}

// ── 应用切换 ──
pub fn apply(tool: &Tool, mirror: &Mirror) -> Result<(), String> {
    match tool.handler.as_str() {
        "pip_ini" => write_pip_source_to_path(&pip_path(), mirror, true),
        "npmrc" => {
            let p = npmrc_path();
            backup::backup_file(&p);
            write_line_key(&p, "registry", &mirror.url, false)
        }
        "yarnrc" => {
            let p = yarnrc_path();
            backup::backup_file(&p);
            write_line_key(&p, "registry", &mirror.url, true)
        }
        "go_env" => {
            backup::backup_env(winenv::Hive::User, "go-source", &["GOPROXY"]);
            winenv::set_user("GOPROXY", &mirror.url)
        }
        "cargo_config" => managed_apply(cargo_path(), mirror, cargo_template),
        "condarc" => managed_apply(condarc_path(), mirror, condarc_template),
        "maven_settings" => maven_apply(maven_path(), mirror, None, false),
        "gradle_init" => gradle_apply(gradle_path(), mirror, None, false),
        "runtime_download" => Ok(()),
        _ => Err(format!("未知 handler: {}", tool.handler)),
    }
}

/// 当前各工具所选镜像源的域名（用于终端代理的 NO_PROXY 白名单）。
pub fn domestic_hosts() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tools() {
        if let Some(id) = detect(&t) {
            if id != "official" {
                if let Some(mir) = t.mirrors.iter().find(|m| m.id == id) {
                    if !mir.host.is_empty() && !out.contains(&mir.host) {
                        out.push(mir.host.clone());
                    }
                }
            }
        }
    }
    out
}

// ── Tauri 命令 ──
#[tauri::command]
pub fn list_sources() -> Vec<ToolState> {
    tools()
        .into_iter()
        .map(|t| {
            let current = detect(&t);
            let current_label = current
                .as_ref()
                .and_then(|id| t.mirrors.iter().find(|m| &m.id == id))
                .map(|m| m.name.clone())
                .unwrap_or_else(|| "未识别".into());
            ToolState {
                installed: t.handler == "runtime_download" || cmd_on_path(&t.probe),
                config: config_display(&t.handler),
                current,
                current_label,
                id: t.id,
                name: t.name,
                icon: t.icon,
                mirrors: t.mirrors,
            }
        })
        .collect()
}

#[tauri::command]
pub fn apply_source(
    tool_id: String,
    mirror_id: String,
    proxy_enabled: Option<bool>,
    proxy_host: Option<String>,
    proxy_port: Option<u16>,
) -> Result<(), String> {
    let tools = tools();
    let tool = tools.iter().find(|t| t.id == tool_id).ok_or("未知工具")?;
    let mirror = tool
        .mirrors
        .iter()
        .find(|m| m.id == mirror_id)
        .ok_or("未知镜像")?;
    let proxy = parse_proxy(proxy_enabled, proxy_host, proxy_port);
    match tool.handler.as_str() {
        "maven_settings" if proxy_enabled.is_some() => {
            maven_apply(maven_path(), mirror, proxy.as_ref(), true)?
        }
        "gradle_init" if proxy_enabled.is_some() => {
            gradle_apply(gradle_path(), mirror, proxy.as_ref(), true)?
        }
        _ => apply(tool, mirror)?,
    }
    if mirror_id.starts_with("custom:") {
        crate::custom::apply_auth(&tool.handler, mirror)?;
    }
    Ok(())
}

#[tauri::command]
pub fn apply_source_scoped(
    tool_id: String,
    mirror_id: String,
    scope: Option<String>,
) -> Result<(), String> {
    let tools = tools();
    let tool = tools.iter().find(|t| t.id == tool_id).ok_or("未知工具")?;
    let mirror = tool
        .mirrors
        .iter()
        .find(|m| m.id == mirror_id)
        .ok_or("未知镜像")?;
    match (tool.handler.as_str(), scope.as_deref().unwrap_or("user")) {
        ("go_env", "user") => apply(tool, mirror),
        ("go_env", "system") => crate::winadmin::set_env_system(
            "go-source",
            vec![("GOPROXY".to_string(), mirror.url.clone())],
        ),
        (_, "user") => apply(tool, mirror),
        (_, "system") => Err("该工具的系统级换源暂不支持".into()),
        (_, other) => Err(format!("未知作用范围：{other}")),
    }
}

#[tauri::command]
pub fn apply_source_file(
    tool_id: String,
    path: String,
    mirror_id: String,
    proxy_enabled: Option<bool>,
    proxy_host: Option<String>,
    proxy_port: Option<u16>,
) -> Result<(), String> {
    let tools = tools();
    let tool = tools.iter().find(|t| t.id == tool_id).ok_or("未知工具")?;
    let mirror = tool
        .mirrors
        .iter()
        .find(|m| m.id == mirror_id)
        .ok_or("未知镜像")?;
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("请先选择配置文件".into());
    }
    let proxy = parse_proxy(proxy_enabled, proxy_host, proxy_port);
    match tool.handler.as_str() {
        "maven_settings" => maven_apply(target, mirror, proxy.as_ref(), proxy_enabled.is_some()),
        "gradle_init" => gradle_apply(target, mirror, proxy.as_ref(), proxy_enabled.is_some()),
        _ => Err("该工具暂不支持自选配置文件".into()),
    }
}

#[tauri::command]
pub fn clear_source_file(tool_id: String, path: String) -> Result<(), String> {
    let tools = tools();
    let tool = tools.iter().find(|t| t.id == tool_id).ok_or("未知工具")?;
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("请先选择配置文件".into());
    }
    let official = tool
        .mirrors
        .iter()
        .find(|m| m.id == "official")
        .ok_or("未找到官方源")?;
    match tool.handler.as_str() {
        "maven_settings" => maven_apply(target, official, None, true),
        "gradle_init" => gradle_apply(target, official, None, true),
        _ => Err("该工具暂不支持自选配置文件".into()),
    }
}

#[tauri::command]
pub fn source_proxy_state(tool_id: String, path: Option<String>) -> Result<bool, String> {
    let tools = tools();
    let tool = tools.iter().find(|t| t.id == tool_id).ok_or("未知工具")?;
    let target = path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    match tool.handler.as_str() {
        "maven_settings" => {
            let p = target.unwrap_or_else(maven_path);
            Ok(maven_proxy_has_at(&p))
        }
        "gradle_init" => {
            let p = target.unwrap_or_else(gradle_path);
            Ok(gradle_proxy_has_at(&p))
        }
        _ => Ok(false),
    }
}

#[derive(Serialize)]
pub struct SourceFileState {
    pub current: Option<String>,
    pub current_label: String,
}

#[tauri::command]
pub fn source_file_state(tool_id: String, path: String) -> Result<SourceFileState, String> {
    let tools = tools();
    let tool = tools.iter().find(|t| t.id == tool_id).ok_or("未知工具")?;
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Ok(SourceFileState {
            current: None,
            current_label: "未选择".into(),
        });
    }
    let current = match tool.handler.as_str() {
        "maven_settings" => managed_detect_contains(tool, &target, false),
        "gradle_init" => managed_detect_contains(tool, &target, false),
        _ => None,
    };
    let current_label = current
        .as_deref()
        .and_then(|id| {
            tool.mirrors
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.name.clone())
        })
        .unwrap_or_else(|| "未识别".into());
    Ok(SourceFileState {
        current,
        current_label,
    })
}

#[derive(Serialize, Clone)]
pub struct PipConfigScope {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub configured: bool,
    pub requires_admin: bool,
    pub current: Option<String>,
    pub current_label: String,
    pub effective: Option<String>,
    pub effective_label: String,
    pub overridden_by: Option<String>,
}

#[derive(Serialize)]
pub struct PipConfigState {
    pub mirrors: Vec<Mirror>,
    pub scopes: Vec<PipConfigScope>,
    pub env_overrides: Vec<String>,
    pub effective: Option<String>,
    pub effective_label: String,
}

fn pip_tool_from_list(list: &[Tool]) -> Result<Tool, String> {
    list.iter()
        .find(|t| t.id == "pip")
        .cloned()
        .ok_or_else(|| "未找到 pip 源配置".into())
}

fn mirror_label(tool: &Tool, id: Option<&str>) -> String {
    id.and_then(|id| tool.mirrors.iter().find(|m| m.id == id))
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "未识别".into())
}

fn pip_scope(
    tool: &Tool,
    kind: &str,
    name: &str,
    path: PathBuf,
    requires_admin: bool,
    base_effective: Option<&str>,
    override_label: Option<&str>,
) -> PipConfigScope {
    let configured = pip_configured_at(&path);
    let current = pip_mirror_id_at(&path, tool);
    let effective = if override_label.is_some() {
        None
    } else if configured {
        current.clone()
    } else {
        base_effective.map(str::to_string)
    };
    let overridden_by = if let Some(label) = override_label {
        Some(label.to_string())
    } else if configured {
        None
    } else if base_effective.is_some() {
        Some("上级配置".into())
    } else {
        None
    };
    PipConfigScope {
        id: if kind == "custom" {
            format!("custom:{}", path.display())
        } else {
            kind.to_string()
        },
        kind: kind.to_string(),
        name: name.to_string(),
        path: path.to_string_lossy().into_owned(),
        exists: path.exists(),
        configured,
        requires_admin,
        current_label: mirror_label(tool, current.as_deref()),
        current,
        effective_label: if let Some(label) = override_label {
            format!("{label}覆盖")
        } else {
            mirror_label(tool, effective.as_deref())
        },
        effective,
        overridden_by,
    }
}

fn pip_env_overrides() -> Vec<String> {
    let names = [
        "PIP_CONFIG_FILE",
        "PIP_INDEX_URL",
        "PIP_EXTRA_INDEX_URL",
        "PIP_TRUSTED_HOST",
        "PIP_NO_INDEX",
    ];
    let mut out = Vec::new();
    for name in names {
        if std::env::var_os(name).is_some()
            || winenv::get_raw_in(winenv::Hive::User, name).is_some()
            || winenv::get_raw_in(winenv::Hive::System, name).is_some()
        {
            out.push(name.to_string());
        }
    }
    out
}

fn pip_scope_path(kind: &str, path: Option<String>) -> Result<PathBuf, String> {
    match kind {
        "user" => Ok(pip_path()),
        "custom" => {
            let p = path.ok_or("请先选择 pip.ini 文件")?;
            let pip_ini = PathBuf::from(p);
            if !pip_ini
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("pip.ini"))
                .unwrap_or(false)
            {
                return Err("请选择名为 pip.ini 的文件".into());
            }
            Ok(pip_ini)
        }
        _ => Err("未知 pip 配置作用域".into()),
    }
}

#[tauri::command]
pub fn pip_config_state(custom_path: Option<String>) -> Result<PipConfigState, String> {
    let all = tools();
    let tool = pip_tool_from_list(&all)?;
    let env_overrides = pip_env_overrides();
    let env_overridden = !env_overrides.is_empty();

    let user_id = pip_mirror_id_at(&pip_path(), &tool);
    let user_configured = pip_configured_at(&pip_path());
    let effective = if env_overridden {
        None
    } else if user_configured {
        user_id.clone()
    } else {
        Some("official".into())
    };

    let mut scopes = vec![pip_scope(
        &tool,
        "user",
        "当前用户 pip.ini",
        pip_path(),
        false,
        Some("official"),
        if env_overridden {
            Some("环境变量")
        } else {
            None
        },
    )];

    if let Some(path) = custom_path
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        scopes.push(pip_scope(
            &tool,
            "custom",
            "自选 pip.ini",
            PathBuf::from(path),
            false,
            effective.as_deref(),
            if env_overridden {
                Some("环境变量")
            } else {
                None
            },
        ));
    }

    let effective_label = if env_overridden {
        "环境变量覆盖".into()
    } else {
        mirror_label(&tool, effective.as_deref())
    };

    Ok(PipConfigState {
        mirrors: tool.mirrors,
        scopes,
        env_overrides,
        effective_label,
        effective,
    })
}

fn find_pip_mirror(mirror_id: &str) -> Result<Mirror, String> {
    let all = tools();
    let tool = pip_tool_from_list(&all)?;
    tool.mirrors
        .into_iter()
        .find(|m| m.id == mirror_id)
        .ok_or_else(|| "未知 pip 镜像".into())
}

#[tauri::command]
pub fn pip_apply_source(
    scope: String,
    path: Option<String>,
    mirror_id: String,
) -> Result<(), String> {
    let mirror = find_pip_mirror(&mirror_id)?;
    let target = pip_scope_path(&scope, path)?;
    write_pip_source_to_path(&target, &mirror, true)
}

#[tauri::command]
pub fn pip_clear_source(scope: String, path: Option<String>) -> Result<(), String> {
    let target = pip_scope_path(&scope, path)?;
    clear_pip_source_at(&target, true)
}

// ── 软件源测速（按主机测 TCP 连接延迟，并行）──
#[derive(Serialize)]
pub struct HostPing {
    pub host: String,
    pub ms: Option<u64>, // None=不可达/超时
}

fn ping_host(host: &str) -> Option<u64> {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::{Duration, Instant};
    const SPEEDTEST_TIMEOUT_MS: u64 = 1500;
    let mut best: Option<u64> = None;
    let deadline = Instant::now() + Duration::from_millis(SPEEDTEST_TIMEOUT_MS);
    for port in [443u16, 80] {
        let Ok(mut addrs) = (host, port).to_socket_addrs() else {
            continue;
        };
        let Some(addr) = addrs.next() else { continue };
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let timeout = deadline.saturating_duration_since(now);
        let t = Instant::now();
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            let ms = t.elapsed().as_millis() as u64;
            best = Some(best.map_or(ms, |b| b.min(ms)));
        }
        if best.is_some() {
            break;
        } // 443 通就不再试 80
    }
    best
}

/// 并行测一批主机的连接延迟（毫秒）。host 为空的跳过。
#[tauri::command]
pub async fn speedtest_hosts(hosts: Vec<String>) -> Vec<HostPing> {
    tauri::async_runtime::spawn_blocking(move || {
        let handles: Vec<_> = hosts
            .into_iter()
            .filter(|h| !h.trim().is_empty())
            .map(|h| {
                std::thread::spawn(move || HostPing {
                    ms: ping_host(&h),
                    host: h,
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn list_backups() -> Vec<backup::BackupEntry> {
    backup::list_backups()
}

#[tauri::command]
pub fn restore_backup(path: String, origin: String) -> Result<(), String> {
    backup::restore(&path, &origin)
}

#[tauri::command]
pub fn backup_detail(path: String, origin: String) -> Result<backup::BackupDetail, String> {
    backup::backup_detail(path, origin)
}

#[tauri::command]
pub fn delete_backup(path: String) -> Result<(), String> {
    backup::delete_backup(path)
}

#[tauri::command]
pub fn clear_backups() -> Result<usize, String> {
    backup::clear_backups()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_catalog_contains_all_managed_downloads() {
        let ids: Vec<String> = hardcoded()
            .into_iter()
            .filter(|tool| tool.handler == "runtime_download")
            .map(|tool| tool.id)
            .collect();
        for expected in [
            PYTHON_RUNTIME_TOOL_ID,
            NODE_RUNTIME_TOOL_ID,
            GIT_RUNTIME_TOOL_ID,
            MAVEN_RUNTIME_TOOL_ID,
            GRADLE_RUNTIME_TOOL_ID,
            GO_RUNTIME_TOOL_ID,
        ] {
            assert!(ids.iter().any(|id| id == expected), "missing {expected}");
        }
    }
}
