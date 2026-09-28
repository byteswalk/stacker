//! 内置镜像清单的远程更新：从用户配置的 GitHub URL 拉取 mirrors.json，覆盖兜底清单。
//! raw.githubusercontent.com 在部分网络环境下不可达，自动追加 jsDelivr 兜底。
//! 本地缓存 %APPDATA%\stacker\mirrors.json；地址存 config.json（运行时可配，不写死仓库）。

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use serde::{de, Deserialize, Deserializer, Serialize};
use tauri::{Emitter, Manager};

use crate::sources::{self, Mirror};

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteTool {
    pub id: String,
    pub mirrors: Vec<Mirror>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteList {
    #[serde(
        default = "default_catalog_version",
        deserialize_with = "de_version_string"
    )]
    pub version: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub tools: Vec<RemoteTool>,
}

#[derive(Serialize, Deserialize, Default)]
struct Cfg {
    #[serde(default)]
    mirror_list_url: String,
}

const DEFAULT_MIRROR_LIST_URL: &str =
    "https://raw.githubusercontent.com/byteswalk/stacker/main/resources/mirrors.json";
const GITEE_MIRROR_LIST_URL: &str =
    "https://gitee.com/shaxiong/stacker/raw/main/resources/mirrors.json?download=1";
const DEFAULT_LATEST_URL: &str =
    "https://raw.githubusercontent.com/byteswalk/stacker/main/resources/latest.json";
const GITEE_LATEST_URL: &str =
    "https://gitee.com/shaxiong/stacker/raw/main/resources/latest.json?download=1";
const GITEE_APP_REPO: &str = "shaxiong/stacker";

fn official_mirror_urls_for_locale(locale: &str) -> Vec<&'static str> {
    if locale.eq_ignore_ascii_case("en-US") {
        vec![DEFAULT_MIRROR_LIST_URL, GITEE_MIRROR_LIST_URL]
    } else {
        vec![GITEE_MIRROR_LIST_URL, DEFAULT_MIRROR_LIST_URL]
    }
}

fn github_first_for_locale(locale: &str) -> bool {
    locale.eq_ignore_ascii_case("en-US")
}

fn official_mirror_urls() -> Vec<String> {
    official_mirror_urls_for_locale(&crate::settings::load().locale)
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn is_official_mirror_url(url: &str) -> bool {
    matches!(url.trim(), DEFAULT_MIRROR_LIST_URL | GITEE_MIRROR_LIST_URL)
}

fn default_catalog_version() -> String {
    "197001010000".into()
}

fn de_version_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        _ => Err(de::Error::custom("version 必须是字符串或数字")),
    }
}

fn configured_mirror_url() -> String {
    let cfg = load_cfg();
    if cfg.mirror_list_url.trim().is_empty() || is_official_mirror_url(&cfg.mirror_list_url) {
        official_mirror_urls()
            .into_iter()
            .next()
            .unwrap_or_else(|| DEFAULT_MIRROR_LIST_URL.into())
    } else {
        cfg.mirror_list_url
    }
}

fn configured_mirror_urls() -> Vec<String> {
    let cfg = load_cfg();
    if cfg.mirror_list_url.trim().is_empty() || is_official_mirror_url(&cfg.mirror_list_url) {
        official_mirror_urls()
    } else {
        vec![cfg.mirror_list_url]
    }
}

fn requested_mirror_urls(url: Option<String>) -> Vec<String> {
    match url.filter(|value| !value.trim().is_empty()) {
        Some(value) if is_official_mirror_url(&value) => {
            let mut urls = vec![value.trim().to_string()];
            urls.extend(official_mirror_urls());
            urls.dedup();
            urls
        }
        Some(value) => vec![value],
        None => configured_mirror_urls(),
    }
}

fn dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
}
fn list_path() -> PathBuf {
    dir().join("mirrors.json")
}
fn cfg_path() -> PathBuf {
    dir().join("config.json")
}

fn load_cfg() -> Cfg {
    std::fs::read_to_string(cfg_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
fn save_cfg(c: &Cfg) -> Result<(), String> {
    let p = cfg_path();
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(c).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn load_list() -> Option<RemoteList> {
    std::fs::read_to_string(list_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn remote_snapshot() -> Option<RemoteList> {
    load_list()
}

pub fn save_remote_snapshot(list: &RemoteList) -> Result<(), String> {
    let p = list_path();
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 用远程/缓存清单全量替换兜底工具的镜像列表。
/// 远程清单是服务器维护的内置源真相：未出现在清单里的内置工具会变成空镜像列表。
pub fn overlay(tools: &mut [sources::Tool]) {
    let Some(list) = load_list() else { return };
    let list_version = list.version.clone();
    let mut by_tool = std::collections::HashMap::new();
    for rt in list.tools {
        by_tool.insert(rt.id, rt.mirrors);
    }
    for t in tools {
        if let Some(mirrors) = by_tool.remove(&t.id) {
            t.mirrors = mirrors;
            patch_legacy_catalog_mirrors(&mut t.mirrors, &t.id, &list_version);
        } else if keep_builtin_for_legacy_manifest(&t.id, &list_version) {
            continue;
        } else {
            t.mirrors.clear();
        }
    }
}

fn keep_builtin_for_legacy_manifest(tool_id: &str, list_version: &str) -> bool {
    (tool_id == sources::GIT_RUNTIME_TOOL_ID && list_version < "202607111317")
        || (matches!(
            tool_id,
            sources::MAVEN_RUNTIME_TOOL_ID
                | sources::GRADLE_RUNTIME_TOOL_ID
                | sources::GO_RUNTIME_TOOL_ID
                | sources::RUST_RUNTIME_TOOL_ID
        ) && list_version < "202607131700")
        || (matches!(tool_id, sources::PHP_RUNTIME_TOOL_ID | "composer")
            && list_version < "202608301100")
}

fn patch_legacy_catalog_mirrors(mirrors: &mut Vec<Mirror>, tool_id: &str, list_version: &str) {
    if list_version >= "202607141100" {
        return;
    }
    match tool_id {
        sources::MAVEN_RUNTIME_TOOL_ID => push_missing_mirror(
            mirrors,
            "apache-cdn",
            "Apache CDN",
            "https://dlcdn.apache.org/maven",
            "dlcdn.apache.org",
        ),
        "go" => push_missing_mirror(
            mirrors,
            "goproxyio",
            "goproxy.io",
            "https://goproxy.io,direct",
            "goproxy.io",
        ),
        "maven" | "gradle" => push_missing_mirror(
            mirrors,
            "maven-central-repo1",
            "Maven Central (repo1)",
            "https://repo1.maven.org/maven2/",
            "repo1.maven.org",
        ),
        _ => {}
    }
}

fn push_missing_mirror(mirrors: &mut Vec<Mirror>, id: &str, name: &str, url: &str, host: &str) {
    if mirrors.iter().any(|mirror| mirror.id == id) {
        return;
    }
    mirrors.push(Mirror {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        host: host.into(),
    });
}

// ── 拉取（带 CDN 兜底）──
fn candidates(url: &str) -> Vec<String> {
    let mut v = vec![url.to_string()];
    if let Some(rest) = url.strip_prefix("https://raw.githubusercontent.com/") {
        let p: Vec<&str> = rest.splitn(4, '/').collect();
        if p.len() == 4 {
            // OWNER/REPO/BRANCH/PATH → jsDelivr CDN
            v.push(format!(
                "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
                p[0], p[1], p[2], p[3]
            ));
        }
    }
    v
}

fn fetch_first(urls: &[String]) -> Result<(String, String), String> {
    let mut errors = Vec::new();
    for url in urls {
        match fetch(url) {
            Ok(body) => return Ok((url.clone(), body)),
            Err(error) => errors.push(format!("{url}: {error}")),
        }
    }
    Err(errors.join("; "))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(30))
        .build()
}

fn fetch(url: &str) -> Result<String, String> {
    let agent = agent();
    let mut last = String::new();
    for u in candidates(url) {
        match agent.get(&u).call() {
            Ok(resp) => match resp.into_string() {
                Ok(body) => return Ok(body),
                Err(e) => last = e.to_string(),
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("拉取失败（已试直连/jsDelivr）：{last}"))
}

/// 工具自身更新信息（fnm/pyenv 用）。
#[derive(Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    pub release_url: Option<String>,
    pub installer_url: Option<String>,
    pub portable_url: Option<String>,
    /// 安装包与免安装包的 SHA-256（小写十六进制）。没有校验值就不会自动安装。
    pub installer_sha256: Option<String>,
    pub portable_sha256: Option<String>,
    /// 安装包的 minisign 签名（.minisig 全文）。没有签名同样不会自动安装。
    pub installer_signature: Option<String>,
    pub published_at: Option<String>,
    pub notes: Vec<String>,
}

// Stacker 自身的发布仓库（owner/repo）。发布到 GitHub Releases 后填上即可启用「检查更新」。
const APP_REPO: &str = "byteswalk/stacker";

/// 发布签名的公钥（minisign 格式）。私钥只在发版那台机器上，不进仓库、不进 CI。
/// 换密钥对就换这一行：cargo run --example release-key -- keygen <私钥路径>
const RELEASE_PUBLIC_KEY: &str = "RWRFeBheM9RE7hJRWngACNiP4WwTCpVuaU+wHV0QyARJPayvTHGs1Qmv";

/// 发布清单里的校验值，用来补上 Releases 接口没取到的那一份
/// （发布页的 SHA256SUMS.txt 在部分网络下拉不到，清单走 raw/jsDelivr/Gitee，更容易通）。
/// 只有清单描述的正是同一个版本时才采用。
fn fill_checksums_from_manifest(info: &mut UpdateInfo) {
    fill_checksums_with(info, fetch);
}

fn fill_checksums_with(info: &mut UpdateInfo, read: impl Fn(&str) -> Result<String, String>) {
    if !info.has_update || (info.installer_sha256.is_some() && info.installer_signature.is_some()) {
        return;
    }
    for url in [DEFAULT_LATEST_URL, GITEE_LATEST_URL] {
        let Ok(body) = read(url) else { continue };
        let Ok(file) = serde_json::from_str::<LatestFile>(&body) else {
            continue;
        };
        if file.version.trim().trim_start_matches('v') != info.latest {
            continue;
        }
        let hash = file
            .installer_sha256
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| is_sha256_hex(h));
        if hash.is_some() {
            if info.installer_sha256.is_none() {
                info.installer_sha256 = hash;
                info.portable_sha256 = file
                    .portable_sha256
                    .map(|h| h.trim().to_ascii_lowercase())
                    .filter(|h| is_sha256_hex(h));
            }
            info.installer_signature = file.installer_signature;
            return;
        }
    }
}

/// 检查更新的四个来源：两个发布接口 + 两份发布清单。
#[derive(Clone, Copy)]
enum Source {
    GiteeRelease,
    GiteeManifest,
    GitHubRelease,
    GitHubManifest,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::GiteeRelease => "Gitee Releases",
            Source::GiteeManifest => "Gitee manifest",
            Source::GitHubRelease => "GitHub Releases",
            Source::GitHubManifest => "GitHub manifest",
        }
    }

    fn check(self, current: &str) -> Result<UpdateInfo, String> {
        match self {
            Source::GiteeRelease => gitee_latest_release(GITEE_APP_REPO, current),
            Source::GiteeManifest => latest_json_update_from(GITEE_LATEST_URL, current),
            Source::GitHubRelease => github_latest_release(APP_REPO, current),
            Source::GitHubManifest => latest_json_update_from(DEFAULT_LATEST_URL, current),
        }
    }
}

/// 按界面语言选择 GitHub/Gitee 的检查顺序。
pub fn check_update_for(current: &str, english: bool) -> Result<UpdateInfo, String> {
    let github = [Source::GitHubRelease, Source::GitHubManifest];
    let gitee = [Source::GiteeRelease, Source::GiteeManifest];
    let order: [Source; 4] = if english {
        [github[0], github[1], gitee[0], gitee[1]]
    } else {
        [gitee[0], gitee[1], github[0], github[1]]
    };

    let mut errors = Vec::new();
    for source in order {
        match source.check(current) {
            Ok(mut info) => {
                fill_checksums_from_manifest(&mut info);
                return Ok(info);
            }
            Err(error) => errors.push(format!("{}: {error}", source.name())),
        }
    }
    Err(format!("检查更新失败：{}", errors.join("；")))
}

#[tauri::command]
pub async fn app_check_update() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let current = env!("CARGO_PKG_VERSION").to_string();
        let english = github_first_for_locale(&crate::settings::load().locale);
        check_update_for(&current, english)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 流式计算文件的 SHA-256，避免把整个安装包读进内存。
pub fn sha256_of_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| format!("无法读取更新包：{e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("无法读取更新包：{e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 用内置公钥验证安装包的 minisign 签名。清单被篡改也没用：签的是安装包的字节，
/// 私钥只在发版那台机器上。
pub fn verify_release_signature(bytes: &[u8], signature: &str) -> Result<(), String> {
    let key = minisign_verify::PublicKey::from_base64(RELEASE_PUBLIC_KEY)
        .map_err(|e| format!("内置公钥无效：{e}"))?;
    let signature = minisign_verify::Signature::decode(signature.trim())
        .map_err(|e| format!("签名格式不对：{e}"))?;
    key.verify(bytes, &signature, false)
        .map_err(|e| format!("签名验证不通过：{e}"))
}

#[tauri::command]
pub async fn app_download_update(
    window: tauri::Window,
    url: String,
    version: String,
    sha256: String,
    signature: String,
) -> Result<String, String> {
    if !url.trim().starts_with("https://") {
        return Err("更新包必须使用 HTTPS 地址".into());
    }
    // 校验值和签名都得有才装。校验值挡下载损坏和上传错文件，签名挡发布清单被篡改
    // （地址和校验值都来自清单，只有签名的私钥不在那里）。
    let expected = sha256.trim().to_ascii_lowercase();
    if !is_sha256_hex(&expected) {
        return Err("这个版本没有提供校验值，请到发布页手动下载安装".into());
    }
    let signature = signature.trim().to_string();
    if signature.is_empty() {
        return Err("这个版本没有提供签名，请到发布页手动下载安装".into());
    }
    crate::installer::op_reset();
    let target = std::env::temp_dir().join(format!(
        "stacker-update-{}.exe",
        version
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '.' || *ch == '-')
            .collect::<String>()
    ));
    let download_window = window.clone();
    let download_url = url.trim().to_string();
    let download_target = target.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .timeout_read(Duration::from_secs(30))
            .timeout_write(Duration::from_secs(30))
            .build();
        crate::installer::download_file_candidates_with_agent(
            &agent,
            &[download_url],
            &download_target,
            512 * 1024,
            |message| {
                let _ = download_window.emit("app-update-progress", message);
            },
        )?;
        let actual = sha256_of_file(&download_target).inspect_err(|_| {
            let _ = std::fs::remove_file(&download_target);
        })?;
        if actual != expected {
            let _ = std::fs::remove_file(&download_target);
            log::error!(
                target: "stacker::update",
                "update package rejected: expected sha256 {expected}, got {actual}"
            );
            return Err("更新包校验不通过，已删除；请到发布页手动下载安装".into());
        }
        let bytes = std::fs::read(&download_target).map_err(|error| {
            let _ = std::fs::remove_file(&download_target);
            format!("无法读取更新包：{error}")
        })?;
        if !bytes.starts_with(b"MZ") {
            let _ = std::fs::remove_file(&download_target);
            return Err("下载内容不是有效的 Windows 安装程序".into());
        }
        if let Err(error) = verify_release_signature(&bytes, &signature) {
            let _ = std::fs::remove_file(&download_target);
            log::error!(target: "stacker::update", "update package rejected: {error}");
            return Err("更新包签名验证不通过，已删除；请到发布页手动下载安装".into());
        }
        let _ = download_window.emit("app-update-progress", "更新包已下载，正在启动安装程序…");
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| error.to_string())??;

    let current_pid = std::process::id();
    let launch_script = format!(
        "$p=Get-Process -Id {current_pid} -ErrorAction SilentlyContinue; if($p){{Wait-Process -Id {current_pid} -Timeout 30 -ErrorAction SilentlyContinue}}; Start-Process -FilePath '{}' -ArgumentList '/S'",
        target.to_string_lossy().replace('\'', "''")
    );
    let mut launcher = std::process::Command::new("powershell.exe");
    launcher.args([
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-Command",
        &launch_script,
    ]);
    // -WindowStyle reaches the host only once it has started; the console it is given has to
    // be refused separately, or it flashes on screen first.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        launcher.creation_flags(0x08000000);
    }
    launcher
        .spawn()
        .map_err(|error| format!("启动安装程序失败：{error}"))?;
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        app.exit(0);
    });
    Ok("更新包已下载，安装程序即将接管升级".into())
}

/// 取 GitHub 仓库最新 release 的 tag（去掉前导 v）。仅走官方 GitHub API。
pub fn github_latest_tag(repo: &str) -> Result<String, String> {
    github_latest_release(repo, env!("CARGO_PKG_VERSION")).map(|info| info.latest)
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GiteeRelease {
    tag_name: String,
    body: Option<String>,
    created_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct LatestFile {
    version: String,
    #[serde(default)]
    release_url: Option<String>,
    #[serde(default)]
    installer_url: Option<String>,
    #[serde(default)]
    portable_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    released_at: Option<String>,
    #[serde(default)]
    installer_sha256: Option<String>,
    #[serde(default)]
    portable_sha256: Option<String>,
    #[serde(default)]
    installer_signature: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
}

fn notes_from_body(body: &str) -> Vec<String> {
    body.lines()
        .map(|line| {
            line.trim()
                .trim_start_matches(['-', '*', '•', ' '])
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(8)
        .collect()
}

fn pick_asset(assets: &[GitHubAsset], portable: bool) -> Option<String> {
    assets
        .iter()
        .find(|a| {
            let name = a.name.to_ascii_lowercase();
            if portable {
                name.ends_with(".zip") && (name.contains("portable") || name.contains("免安装"))
            } else {
                name.ends_with(".exe") && (name.contains("setup") || name.contains("install"))
            }
        })
        .or_else(|| {
            assets.iter().find(|a| {
                let name = a.name.to_ascii_lowercase();
                if portable {
                    name.ends_with(".zip")
                } else {
                    name.ends_with(".exe")
                }
            })
        })
        .map(|a| a.browser_download_url.clone())
}

/// 发布产物旁边的 SHA256SUMS.txt（release-windows.ps1 生成并随发布上传）。
fn pick_checksums(assets: &[GitHubAsset]) -> Option<String> {
    assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS.txt"))
        .map(|a| a.browser_download_url.clone())
}

/// `<hex> *<文件名>` 或 `<hex>  <文件名>`，取出某个文件名对应的校验值。
pub fn sha256_from_sums(text: &str, file_name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hex, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*').trim();
        if !name.eq_ignore_ascii_case(file_name) || !is_sha256_hex(hex) {
            return None;
        }
        Some(hex.to_ascii_lowercase())
    })
}

pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn file_name_of(url: &str) -> Option<&str> {
    url.rsplit('/').next().filter(|name| !name.is_empty())
}

/// 从发布产物里读出安装包与免安装包的校验值；读不到就返回两个 None，
/// 届时「立即更新」会拒绝自动安装，让用户去发布页手动下载。
fn checksums_of(
    assets: &[GitHubAsset],
    installer: &Option<String>,
    portable: &Option<String>,
) -> (Option<String>, Option<String>) {
    let Some(sums_url) = pick_checksums(assets) else {
        log::warn!(target: "stacker::update", "release has no SHA256SUMS.txt asset");
        return (None, None);
    };
    let text = match fetch(&sums_url) {
        Ok(text) => text,
        Err(error) => {
            log::warn!(target: "stacker::update", "cannot read {sums_url}: {error}");
            return (None, None);
        }
    };
    let of = |url: &Option<String>| {
        url.as_deref()
            .and_then(file_name_of)
            .and_then(|name| sha256_from_sums(&text, name))
    };
    (of(installer), of(portable))
}

fn github_latest_release(repo: &str, current: &str) -> Result<UpdateInfo, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let a = agent();
    let mut last = String::new();
    for u in [url.clone()] {
        match a.get(&u).set("User-Agent", "Stacker").call() {
            Ok(r) => match r.into_string() {
                Ok(b) => {
                    let release: GitHubRelease = serde_json::from_str(&b)
                        .map_err(|e| format!("GitHub Release 格式错误：{e}"))?;
                    let latest = release.tag_name.trim_start_matches('v').trim().to_string();
                    let installer_url = pick_asset(&release.assets, false);
                    let portable_url = pick_asset(&release.assets, true);
                    let (installer_sha256, portable_sha256) =
                        checksums_of(&release.assets, &installer_url, &portable_url);
                    return Ok(UpdateInfo {
                        has_update: ver_lt(current, &latest),
                        current: current.into(),
                        latest,
                        release_url: release.html_url,
                        installer_url,
                        portable_url,
                        installer_sha256,
                        portable_sha256,
                        installer_signature: None,
                        published_at: release.published_at,
                        notes: release
                            .body
                            .as_deref()
                            .map(notes_from_body)
                            .unwrap_or_default(),
                    });
                }
                Err(e) => last = e.to_string(),
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("获取最新版本失败：{last}"))
}

fn gitee_latest_release(repo: &str, current: &str) -> Result<UpdateInfo, String> {
    let url = format!("https://gitee.com/api/v5/repos/{repo}/releases/latest");
    let response = agent()
        .get(&url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|error| format!("获取最新版本失败：{error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("读取版本信息失败：{error}"))?;
    let release: GiteeRelease =
        serde_json::from_str(&body).map_err(|error| format!("Gitee Release 格式错误：{error}"))?;
    let latest = release.tag_name.trim_start_matches('v').trim().to_string();
    let installer_url = pick_asset(&release.assets, false);
    let portable_url = pick_asset(&release.assets, true);
    let (installer_sha256, portable_sha256) =
        checksums_of(&release.assets, &installer_url, &portable_url);
    Ok(UpdateInfo {
        has_update: ver_lt(current, &latest),
        current: current.into(),
        latest,
        release_url: Some(format!(
            "https://gitee.com/{repo}/releases/tag/{}",
            release.tag_name
        )),
        installer_url: installer_url.clone(),
        portable_url: portable_url.clone(),
        installer_sha256,
        portable_sha256,
        installer_signature: None,
        published_at: release.created_at,
        notes: release
            .body
            .as_deref()
            .map(notes_from_body)
            .unwrap_or_default(),
    })
}

fn latest_json_update_from(url: &str, current: &str) -> Result<UpdateInfo, String> {
    let body = fetch(url)?;
    let file: LatestFile =
        serde_json::from_str(&body).map_err(|e| format!("latest.json 格式错误：{e}"))?;
    let latest = file.version.trim_start_matches('v').trim().to_string();
    Ok(UpdateInfo {
        has_update: ver_lt(current, &latest),
        current: current.into(),
        latest,
        release_url: file.release_url,
        installer_url: file.installer_url,
        portable_url: file.portable_url,
        installer_sha256: file.installer_sha256.map(|h| h.trim().to_ascii_lowercase()),
        portable_sha256: file.portable_sha256.map(|h| h.trim().to_ascii_lowercase()),
        installer_signature: file.installer_signature,
        published_at: file.published_at.or(file.released_at),
        notes: file.notes,
    })
}

/// 简单版本比较：a < b 返回 true（按数字组）。
pub fn ver_lt(a: &str, b: &str) -> bool {
    fn key(s: &str) -> Vec<u64> {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let st = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                out.push(s[st..i].parse().unwrap_or(0));
            } else {
                i += 1;
            }
        }
        out
    }
    key(a) < key(b)
}

#[tauri::command]
pub fn app_open_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("只能打开 http(s) 链接".into());
    }
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = OsStr::new(trimmed).encode_wide().chain(Some(0)).collect();
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
            Err(format!("打开链接失败：ShellExecuteW 返回 {}", rc as isize))
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
        std::process::Command::new(opener)
            .arg(trimmed)
            .spawn()
            .map_err(|e| format!("打开链接失败：{e}"))?;
        Ok(())
    }
}

#[derive(Serialize)]
pub struct MirrorsStatus {
    pub url: String,
    pub local_version: Option<String>,
    pub tools: usize,
}

#[tauri::command]
pub fn mirrors_status() -> MirrorsStatus {
    let url = configured_mirror_url();
    let list = load_list();
    MirrorsStatus {
        url,
        local_version: list.as_ref().map(|l| l.version.clone()),
        tools: list.map(|l| l.tools.len()).unwrap_or(0),
    }
}

#[derive(Serialize)]
pub struct MirrorsUpdateCheck {
    pub url: String,
    pub local_version: Option<String>,
    pub remote_version: String,
    pub has_update: bool,
    pub tools: usize,
}

fn version_gt(remote: &str, local: Option<&str>) -> bool {
    let remote = remote.trim();
    if remote.is_empty() {
        return false;
    }
    match local.map(str::trim).filter(|s| !s.is_empty()) {
        Some(local) => remote > local,
        None => true,
    }
}

fn parse_remote_list_body(body: &str) -> Result<RemoteList, String> {
    let list: RemoteList = serde_json::from_str(body).map_err(|error| error.to_string())?;
    if list.tools.is_empty() {
        return Err("The remote source manifest is empty.".into());
    }
    Ok(list)
}

/// 只检查远程清单版本，不写本地缓存。
#[tauri::command]
pub async fn mirrors_check_update(url: Option<String>) -> Result<MirrorsUpdateCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let urls = requested_mirror_urls(url);
        let (url, body) = fetch_first(&urls)?;
        let remote = parse_remote_list_body(&body)?;
        let local = load_list();
        let local_version = local.as_ref().map(|l| l.version.clone());
        Ok(MirrorsUpdateCheck {
            url,
            has_update: version_gt(&remote.version, local_version.as_deref()),
            remote_version: remote.version,
            local_version,
            tools: remote.tools.len(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 拉取并应用远程清单。url 为空则用已存配置；成功后记住该地址。
#[tauri::command]
pub async fn mirrors_update(url: Option<String>) -> Result<MirrorsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let urls = requested_mirror_urls(url);
        let (url, body) = fetch_first(&urls)?;
        let list: RemoteList =
            serde_json::from_str(&body).map_err(|e| format!("清单格式错误：{e}"))?;
        if list.tools.is_empty() {
            return Err("服务器清单为空，已拒绝应用".into());
        }
        let p = list_path();
        if let Some(par) = p.parent() {
            std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, &body).map_err(|e| e.to_string())?;
        save_cfg(&Cfg {
            mirror_list_url: url.trim().to_string(),
        })?;
        Ok(MirrorsStatus {
            url: url.trim().to_string(),
            local_version: Some(list.version.clone()),
            tools: list.tools.len(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {

    use super::{is_sha256_hex, sha256_from_sums, sha256_of_file};

    const SUMS: &str = "810b9f5b3a491506e2e7f8341f4450bab5193b0b1d0eb805905330acc0f67e0c *Stacker-0.3.3-setup-windows-x64.exe
        9230453791e126e4b957580ccebb92fd5111f21ff34e016b9b2aac1e05be54b4 *Stacker-0.3.3-portable-windows-x64.zip
";

    /// 发布前自检：本机那把私钥签出来的东西，程序里内置的公钥确实认。
    /// 需要本机有私钥，默认不跑：
    ///   cargo test -- --ignored the_local_release_key_matches_the_built_in_one
    #[test]
    #[ignore]
    fn the_local_release_key_matches_the_built_in_one() {
        let path = std::env::var("STACKER_SIGNING_KEY").unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap()
                .join(".stacker")
                .join("release-signing.key")
                .to_string_lossy()
                .into_owned()
        });
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read the signing key {path}: {e}"));
        let secret = minisign::SecretKeyBox::from_string(&text)
            .unwrap()
            .into_secret_key(std::env::var("STACKER_SIGNING_PASSWORD").ok())
            .unwrap();
        let payload = b"MZ pretend installer";
        let signature = minisign::sign(None, &secret, &payload[..], Some("test"), Some("test"))
            .unwrap()
            .to_string();
        super::verify_release_signature(payload, &signature)
            .expect("the local signing key does not match RELEASE_PUBLIC_KEY");
    }

    /// 发布纪律：最新发布必须带 SHA256SUMS.txt，否则走 Releases 接口的用户拿不到校验值。
    /// 需要联网，默认不跑：cargo test -- --ignored live_release_publishes_checksums
    #[test]
    #[ignore]
    fn live_release_publishes_checksums() {
        let body =
            super::fetch("https://api.github.com/repos/byteswalk/stacker/releases/latest").unwrap();
        let release: super::GitHubRelease = serde_json::from_str(&body).unwrap();
        let names: Vec<&str> = release.assets.iter().map(|a| a.name.as_str()).collect();
        assert!(
            names
                .iter()
                .any(|n| n.eq_ignore_ascii_case("SHA256SUMS.txt")),
            "assets: {names:?}"
        );
    }

    fn info_needing_a_checksum() -> super::UpdateInfo {
        super::UpdateInfo {
            current: "0.3.3".into(),
            latest: "0.3.4".into(),
            has_update: true,
            release_url: None,
            installer_url: Some(
                "https://example.invalid/Stacker-0.3.4-setup-windows-x64.exe".into(),
            ),
            portable_url: None,
            installer_sha256: None,
            portable_sha256: None,
            installer_signature: None,
            published_at: None,
            notes: Vec::new(),
        }
    }

    fn manifest(version: &str, installer: &str) -> String {
        format!(
            r#"{{"version":"{version}","installer_sha256":"{installer}","portable_sha256":"{}"}}"#,
            "b".repeat(64)
        )
    }

    #[test]
    fn a_checksum_is_taken_from_the_manifest_for_the_same_version() {
        let mut info = info_needing_a_checksum();
        let hex = "a".repeat(64);
        super::fill_checksums_with(&mut info, |_| Ok(manifest("v0.3.4", &hex)));
        assert_eq!(info.installer_sha256.as_deref(), Some(hex.as_str()));
        assert_eq!(
            info.portable_sha256.as_deref(),
            Some("b".repeat(64).as_str())
        );
    }

    #[test]
    fn a_manifest_for_another_version_or_a_bad_digest_is_not_used() {
        for body in [
            manifest("v0.3.3", &"a".repeat(64)),
            manifest("v0.3.4", "not-a-digest"),
            manifest("v0.3.4", &"a".repeat(63)),
            "{".into(),
        ] {
            let mut info = info_needing_a_checksum();
            super::fill_checksums_with(&mut info, |_| Ok(body.clone()));
            assert_eq!(info.installer_sha256, None, "body {body}");
        }
        // 取不到清单时保持原样，不会报错。
        let mut info = info_needing_a_checksum();
        super::fill_checksums_with(&mut info, |_| Err("offline".into()));
        assert_eq!(info.installer_sha256, None);
    }

    #[test]
    fn a_checksum_that_came_with_the_release_is_kept_while_the_signature_is_fetched() {
        let mut info = info_needing_a_checksum();
        info.installer_sha256 = Some("c".repeat(64));
        super::fill_checksums_with(&mut info, |_| {
            Ok(format!(
                r#"{{"version":"0.3.4","installer_sha256":"{}","installer_signature":"sig"}}"#,
                "a".repeat(64)
            ))
        });
        // 发布接口给的校验值不被清单覆盖，只是顺带把清单里的签名带回来。
        assert_eq!(
            info.installer_sha256.as_deref(),
            Some("c".repeat(64).as_str())
        );
        assert_eq!(info.installer_signature.as_deref(), Some("sig"));
    }

    #[test]
    fn nothing_is_fetched_once_both_the_checksum_and_the_signature_are_in() {
        let mut info = info_needing_a_checksum();
        info.installer_sha256 = Some("c".repeat(64));
        info.installer_signature = Some("sig".into());
        super::fill_checksums_with(&mut info, |_| panic!("must not read the manifest"));
        assert_eq!(info.installer_signature.as_deref(), Some("sig"));
    }

    #[test]
    fn checksums_are_read_per_file_name() {
        assert_eq!(
            sha256_from_sums(SUMS, "Stacker-0.3.3-setup-windows-x64.exe").as_deref(),
            Some("810b9f5b3a491506e2e7f8341f4450bab5193b0b1d0eb805905330acc0f67e0c")
        );
        assert_eq!(
            sha256_from_sums(SUMS, "stacker-0.3.3-PORTABLE-windows-x64.zip").as_deref(),
            Some("9230453791e126e4b957580ccebb92fd5111f21ff34e016b9b2aac1e05be54b4")
        );
        assert_eq!(
            sha256_from_sums(SUMS, "Stacker-0.3.4-setup-windows-x64.exe"),
            None
        );
        // 双空格分隔（sha256sum 的默认文本模式）同样认。
        assert_eq!(
            sha256_from_sums(
                "810b9f5b3a491506e2e7f8341f4450bab5193b0b1d0eb805905330acc0f67e0c  a.exe",
                "a.exe"
            )
            .as_deref(),
            Some("810b9f5b3a491506e2e7f8341f4450bab5193b0b1d0eb805905330acc0f67e0c")
        );
    }

    #[test]
    fn a_line_without_a_real_checksum_is_ignored() {
        for line in [
            "not-a-hash *a.exe",
            "810b9f5b3a491506e2e7f8341f4450bab5193b0b1d0eb805905330acc0f67e0 *a.exe",
            "810b9f5b3a491506e2e7f8341f4450bab5193b0b1d0eb805905330acc0f67e0cc *a.exe",
            "a.exe",
        ] {
            assert_eq!(sha256_from_sums(line, "a.exe"), None, "line {line:?}");
        }
    }

    #[test]
    fn only_a_full_hex_digest_counts_as_a_checksum() {
        assert!(is_sha256_hex(&"0".repeat(64)));
        assert!(is_sha256_hex(&"F".repeat(64)));
        assert!(!is_sha256_hex(&"0".repeat(63)));
        assert!(!is_sha256_hex(&"0".repeat(65)));
        assert!(!is_sha256_hex(""));
        assert!(!is_sha256_hex(&format!("{}g", "0".repeat(63))));
    }

    /// 用发布工具的同一套 minisign API 现签一份，确认内置公钥能验过、改一个字节就验不过。
    fn sign_for_test(bytes: &[u8], pair: &minisign::KeyPair) -> String {
        minisign::sign(None, &pair.sk, bytes, Some("test"), Some("test"))
            .unwrap()
            .to_string()
    }

    #[test]
    fn only_the_release_key_can_sign_an_update() {
        let installer = b"MZ fake installer bytes".to_vec();

        // 换一把钥匙签的，验不过。
        let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let forged = sign_for_test(&installer, &other);
        assert!(super::verify_release_signature(&installer, &forged).is_err());

        // 签名本身是乱的，也验不过，而且不会 panic。
        for junk in ["", "not a signature", "untrusted comment: x"] {
            assert!(
                super::verify_release_signature(&installer, junk).is_err(),
                "{junk}"
            );
        }
    }

    #[test]
    fn a_signature_covers_the_exact_bytes() {
        // 这里用内置公钥对应的那把私钥是拿不到的，所以换一个自带的公钥来验同一套逻辑：
        // 签名、验证、篡改后失败，三件事在 minisign 侧是同一条路径。
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let bytes = b"MZ installer".to_vec();
        let signature = sign_for_test(&bytes, &pair);

        let key = minisign_verify::PublicKey::from_base64(&pair.pk.to_base64()).unwrap();
        let parsed = minisign_verify::Signature::decode(&signature).unwrap();
        assert!(key.verify(&bytes, &parsed, false).is_ok());

        let mut tampered = bytes.clone();
        tampered.push(b'!');
        assert!(key.verify(&tampered, &parsed, false).is_err());
    }

    #[test]
    fn the_built_in_release_key_is_usable() {
        assert!(
            minisign_verify::PublicKey::from_base64(super::RELEASE_PUBLIC_KEY).is_ok(),
            "RELEASE_PUBLIC_KEY must hold one minisign public key"
        );
    }

    #[test]
    fn a_files_digest_matches_the_published_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("payload.bin");
        // 跨过 256 KiB 的读取块，确保分块累加没写错。
        let bytes: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();

        let expected = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(&bytes))
        };
        assert_eq!(sha256_of_file(&path).unwrap(), expected);

        std::fs::write(&path, b"tampered").unwrap();
        assert_ne!(sha256_of_file(&path).unwrap(), expected);
    }
    use super::*;

    #[test]
    fn english_prefers_github() {
        assert!(github_first_for_locale("en-US"));
        assert_eq!(
            official_mirror_urls_for_locale("en-US"),
            vec![DEFAULT_MIRROR_LIST_URL, GITEE_MIRROR_LIST_URL]
        );
    }

    #[test]
    fn chinese_prefers_gitee() {
        assert!(!github_first_for_locale("zh-CN"));
        assert_eq!(
            official_mirror_urls_for_locale("zh-CN"),
            vec![GITEE_MIRROR_LIST_URL, DEFAULT_MIRROR_LIST_URL]
        );
    }

    #[test]
    fn legacy_manifest_keeps_new_php_catalogs() {
        assert!(keep_builtin_for_legacy_manifest(
            sources::PHP_RUNTIME_TOOL_ID,
            "202607141100"
        ));
        assert!(keep_builtin_for_legacy_manifest("composer", "202607141100"));
        assert!(!keep_builtin_for_legacy_manifest(
            sources::PHP_RUNTIME_TOOL_ID,
            "202608301100"
        ));
        assert!(!keep_builtin_for_legacy_manifest(
            "composer",
            "202608301100"
        ));
    }
}
