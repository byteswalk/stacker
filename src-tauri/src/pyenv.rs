//! Python 版本管理：pyenv-win 接管。检测 / 列版本 / 设全局默认 / 装卸 / 一键安装。
//! pyenv-win 是 pyenv.bat（非 exe），Windows 下 Command::new 跑不了 .bat，故经 cmd /c 调。

use crate::sources::Mirror;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;

/// pyenv.bat 全路径（按注册表最新 PATH 解析）。
fn pyenv_bat() -> Option<String> {
    crate::env::resolve_fresh("pyenv.bat").map(|p| p.to_string_lossy().into_owned())
}
/// PYENV 根目录（从 pyenv.bat 路径上推：...\pyenv-win\bin\pyenv.bat → ...\pyenv-win\）。
fn pyenv_root() -> Option<String> {
    let bat = pyenv_bat()?;
    std::path::Path::new(&bat)
        .parent()?
        .parent()
        .map(|p| format!("{}\\", p.to_string_lossy()))
}

/// 经 cmd /c 跑 pyenv.bat。带上 PYENV/PYENV_HOME/PYENV_ROOT 环境变量，
/// 否则 pyenv-win 会往输出里打 "PYENV variable is not set" 警告，污染 --version 等结果。
fn run_pyenv(args: &[&str]) -> Result<String, String> {
    let bat = pyenv_bat().ok_or("未找到 pyenv（未安装或 PATH 未刷新）")?;
    run_pyenv_bat(&bat, pyenv_root().as_deref(), args)
}

fn run_pyenv_at(root: &str, args: &[&str]) -> Result<String, String> {
    let bat = Path::new(root).join("bin").join("pyenv.bat");
    if !bat.is_file() {
        return Err(format!("未找到 pyenv.bat：{}", bat.display()));
    }
    run_pyenv_bat(&bat.to_string_lossy(), Some(root), args)
}

fn run_pyenv_bat(bat: &str, root: Option<&str>, args: &[&str]) -> Result<String, String> {
    let mut full: Vec<&str> = vec!["/c", &bat];
    full.extend_from_slice(args);
    let mut c = std::process::Command::new("cmd");
    c.args(&full);
    if let Some(r) = root {
        c.env("PYENV", r).env("PYENV_HOME", r).env("PYENV_ROOT", r);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let out = c.output().map_err(|e| format!("pyenv 执行失败：{e}"))?;
    let so = String::from_utf8_lossy(&out.stdout).into_owned();
    let se = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok(so)
    } else {
        Err(if se.trim().is_empty() { so } else { se })
    }
}

fn parse_pyenv_global_output(output: &str) -> Option<String> {
    let value = output.trim();
    if value.is_empty()
        || value.eq_ignore_ascii_case("system")
        || value
            .to_ascii_lowercase()
            .contains("no global version configured")
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn pyenv_global_version() -> Option<String> {
    run_pyenv(&["global"])
        .ok()
        .and_then(|output| parse_pyenv_global_output(&output))
}

fn pyenv_global_version_at(root: &str) -> Option<String> {
    run_pyenv_at(root, &["global"])
        .ok()
        .and_then(|output| parse_pyenv_global_output(&output))
}

fn has_conda() -> bool {
    crate::env::resolve_fresh("conda.exe").is_some()
        || crate::env::resolve_fresh("conda.bat").is_some()
}

const PYTHON_SPEEDTEST_VERSION: &str = "3.12.10";
const PYENV_GITHUB_URL: &str =
    "https://github.com/pyenv-win/pyenv-win/archive/refs/heads/master.zip";
const BUNDLED_PYENV_COMMIT: &str = "067b0829665744483802c19a19376f409b3a81d8";
const BUNDLED_PYENV_ZIP: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/tools/pyenv-win-067b082.zip"
));

#[derive(Clone, Copy)]
enum PythonInstallerKind {
    Exe,
    Msi,
}

struct PythonInstallerArtifact {
    filename: String,
    url: String,
    kind: PythonInstallerKind,
}

struct PythonInstallerDownload {
    path: PathBuf,
    kind: PythonInstallerKind,
}

const PYTHON_REQUIRED_MSI_COMPONENTS: &[&str] = &["core", "exe", "lib"];
const PYTHON_OPTIONAL_MSI_COMPONENTS: &[&str] = &["dev", "tcltk"];

struct InstallLog {
    path: PathBuf,
}

impl InstallLog {
    fn new(version: &str, source: &str) -> Result<Self, String> {
        let dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Stacker")
            .join("logs");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = dir.join(format!("python-install-session-{version}-{stamp}.log"));
        let log = Self { path };
        log.line(format!(
            "START python install session version={version} source={source}"
        ));
        Ok(log)
    }

    fn line(&self, msg: impl AsRef<str>) {
        use std::io::Write;
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(file, "[{ts}] {}", msg.as_ref());
        }
    }

    fn error(&self, err: impl AsRef<str>) -> String {
        let err = err.as_ref();
        self.line(format!("ERROR {err}"));
        format!("{err}\n诊断日志：{}", self.path.display())
    }

    fn sidecar(&self, suffix: &str) -> PathBuf {
        let clean: String = suffix
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        let stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("python-install-session");
        self.path.with_file_name(format!("{stem}-{clean}.log"))
    }
}

fn pyenv_pypi_simple(id: &str) -> Option<&'static str> {
    match id {
        "tuna" => Some("https://pypi.tuna.tsinghua.edu.cn/simple/pyenv-win/"),
        "aliyun" => Some("https://mirrors.aliyun.com/pypi/simple/pyenv-win/"),
        "huawei" => Some("https://repo.huaweicloud.com/repository/pypi/simple/pyenv-win/"),
        "ustc" => Some("https://pypi.mirrors.ustc.edu.cn/simple/pyenv-win/"),
        "bfsu" => Some("https://mirrors.bfsu.edu.cn/pypi/web/simple/pyenv-win/"),
        "nju" => Some("https://mirror.nju.edu.cn/pypi/web/simple/pyenv-win/"),
        _ => None,
    }
}

fn python_runtime_mirror(id: &str) -> Option<Mirror> {
    crate::sources::python_runtime_mirrors()
        .into_iter()
        .find(|m| m.id == id)
}

#[derive(Serialize)]
pub struct PyVer {
    pub version: String,
    pub is_default: bool,
}
#[derive(Serialize, Default)]
pub struct PyenvStatus {
    pub installed: bool,
    pub pyenv_version: Option<String>,
    pub versions: Vec<PyVer>,
    pub default: Option<String>,
    pub has_conda: bool,
}

// 异步：内含多次 cmd /c pyenv.bat 子进程调用，放后台线程，避免阻塞主线程让窗口"未响应"。
#[tauri::command]
pub async fn pyenv_status() -> PyenvStatus {
    tauri::async_runtime::spawn_blocking(pyenv_status_impl)
        .await
        .unwrap_or_default()
}
fn pyenv_status_impl() -> PyenvStatus {
    let pyenv_version = run_pyenv(&["--version"]).ok().map(|s| s.trim().to_string());
    let installed = pyenv_version.is_some();
    let mut versions = Vec::new();
    let mut default = None;
    if installed {
        let global = pyenv_global_version();
        default = global.clone();
        if let Ok(list) = run_pyenv(&["versions", "--bare"]) {
            let root_path = pyenv_root().map(PathBuf::from);
            for line in list.lines() {
                let v = line.trim().to_string();
                if v.is_empty() {
                    continue;
                }
                if let Some(root) = root_path.as_ref() {
                    if !installed_python_ready(&root.join("versions").join(&v), &v) {
                        continue;
                    }
                }
                let is_default = Some(&v) == global.as_ref();
                versions.push(PyVer {
                    version: v,
                    is_default,
                });
            }
        }
    }
    PyenvStatus {
        installed,
        pyenv_version,
        versions,
        default,
        has_conda: has_conda(),
    }
}

pub(crate) fn pyenv_status_snapshot() -> PyenvStatus {
    pyenv_status_impl()
}

pub(crate) fn pyenv_python_exe(version: &str) -> Option<PathBuf> {
    let root = pyenv_root()?;
    let version_dir = PathBuf::from(root).join("versions").join(version);
    if installed_python_ready(&version_dir, version) {
        Some(version_dir.join("python.exe"))
    } else {
        None
    }
}

pub(crate) fn pyenv_integration_ready() -> bool {
    let Some(root) = pyenv_root() else {
        return false;
    };
    let bin = format!("{root}bin").to_lowercase();
    let shims = format!("{root}shims").to_lowercase();
    let has = |needle: &str| {
        crate::env::fresh_path_dirs().iter().any(|dir| {
            dir.to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase()
                == needle.trim_end_matches(['\\', '/'])
        })
    };
    if !(has(&bin) && has(&shims)) {
        return false;
    }
    let default = pyenv_global_version();
    let Some(default) = default else { return true };
    let Some(expected) = pyenv_python_exe(&default) else {
        return false;
    };
    let Some(actual) = crate::env::resolve_fresh("python.exe") else {
        return false;
    };
    same_existing_path(&actual, &expected)
}

#[tauri::command]
pub async fn pyenv_set_global(version: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_pyenv(&["global", &version])?;
        let _ = run_pyenv(&["rehash"]);
        let root = pyenv_root().ok_or("未找到 pyenv-win 安装目录")?;
        write_pyenv_integration_for_root(&root)?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 重写 pyenv 的「集成」：把 PYENV/PYENV_HOME/PYENV_ROOT 变量 + bin/shims 重新写进用户级 PATH。
/// pyenv 靠 PATH 生效（不用 shell 钩子），这个用于修复终端里 pyenv/python 找不到（PATH 被改乱）的情况。
#[tauri::command]
pub fn pyenv_write_integration() -> Result<(), String> {
    let root = pyenv_root().ok_or("未找到 pyenv（未安装或 PATH 未刷新）")?;
    write_pyenv_integration_for_root(&root)
}

fn write_pyenv_integration_for_root(root: &str) -> Result<(), String> {
    crate::backup::backup_env(
        crate::winenv::Hive::User,
        "pyenv",
        &["PYENV", "PYENV_HOME", "PYENV_ROOT"],
    );
    crate::winenv::set_user("PYENV", root)?;
    crate::winenv::set_user("PYENV_HOME", root)?;
    crate::winenv::set_user("PYENV_ROOT", root)?;
    crate::winenv::prepend_path_in(crate::winenv::Hive::User, &format!("{root}shims"))?;
    crate::winenv::prepend_path_in(crate::winenv::Hive::User, &format!("{root}bin"))?;
    if let Some(default) = pyenv_global_version_at(root) {
        sync_default_python_paths(root, &default)?;
    }
    Ok(())
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    normalized_path(&left) == normalized_path(&right)
}

fn sync_default_python_paths(root: &str, version: &str) -> Result<(), String> {
    let version_dir = PathBuf::from(root).join("versions").join(version);
    if !installed_python_ready(&version_dir, version) {
        return Err(format!("默认 Python {version} 的安装目录不存在或不完整"));
    }

    let managed_root = format!("{}\\versions\\", normalized_path(Path::new(root)));
    for entry in crate::winenv::get_path_in(crate::winenv::Hive::User) {
        if normalized_path(Path::new(&entry)).starts_with(&managed_root) {
            crate::winenv::remove_path_in(crate::winenv::Hive::User, &entry)?;
        }
    }

    let scripts = version_dir.join("Scripts");
    crate::winenv::prepend_path_in(crate::winenv::Hive::User, &scripts.to_string_lossy())?;
    crate::winenv::prepend_path_in(crate::winenv::Hive::User, &version_dir.to_string_lossy())?;
    Ok(())
}

#[tauri::command]
pub fn pyenv_root_dir() -> Option<String> {
    pyenv_root()
}

fn normalize_pyenv_root(root: &str) -> String {
    let mut s = root.trim().trim_matches('"').to_string();
    if !s.ends_with(['\\', '/']) {
        s.push('\\');
    }
    s
}

fn ensure_pyenv_root(root: &str) -> Result<(), String> {
    let target = Path::new(root);
    if target.join("bin").join("pyenv.bat").is_file() {
        write_pyenv_integration_for_root(root)?;
        return Ok(());
    }
    let current = pyenv_root().ok_or("未找到 pyenv-win，无法初始化自定义安装位置")?;
    let current_path = Path::new(&current);
    if !current_path.join("bin").join("pyenv.bat").is_file() {
        return Err("当前 pyenv-win 目录不完整，请先重新安装 pyenv-win".into());
    }
    copy_dir_all(current_path, target)?;
    write_pyenv_integration_for_root(root)
}

// pyenv-win 自己从 python.org 下载，干净机没梯子会失败。这里由 Stacker 先从用户选择的下载源
// 下载 Windows 安装包，再本地静默安装到 pyenv-win 的 versions 目录。
fn python_installer_url(version: &str, source: &str, filename: &str) -> Result<String, String> {
    let mirror = python_runtime_mirror(source).ok_or("未知 Python 下载源")?;
    let url = mirror.url.trim();
    if url.is_empty() {
        return Err(format!("{} 未配置下载地址", mirror.name));
    }
    let release_dir = python_release_dir(version);
    if url.contains("{version}") || url.contains("{filename}") {
        return Ok(url
            .replace("{version}", &release_dir)
            .replace("{filename}", filename));
    }
    Ok(format!(
        "{}/{}/{}",
        url.trim_end_matches('/'),
        release_dir,
        filename
    ))
}

fn python_release_dir(version: &str) -> String {
    let mut dots = 0usize;
    let mut end = version.len();
    for (idx, ch) in version.char_indices() {
        if ch == '.' {
            dots += 1;
            continue;
        }
        if dots >= 2 && !ch.is_ascii_digit() {
            end = idx;
            break;
        }
    }
    version[..end].to_string()
}

fn python_installer_kind(filename: &str) -> Option<PythonInstallerKind> {
    if filename.ends_with("-amd64.exe") {
        Some(PythonInstallerKind::Exe)
    } else if filename.ends_with(".amd64.msi") {
        Some(PythonInstallerKind::Msi)
    } else {
        None
    }
}

fn python_installer_filenames(version: &str) -> Vec<String> {
    let major = version.split('.').next().unwrap_or_default();
    if major == "2" {
        vec![format!("python-{version}.amd64.msi")]
    } else {
        vec![format!("python-{version}-amd64.exe")]
    }
}

fn python_msi_arch_dir(version: &str) -> String {
    let release_dir = python_release_dir(version);
    let suffix = version.strip_prefix(&release_dir).unwrap_or_default();
    if suffix.is_empty() {
        "amd64".into()
    } else {
        format!("amd64{suffix}")
    }
}

fn python_component_msi_url(
    version: &str,
    source: &str,
    component: &str,
) -> Result<String, String> {
    if !source_has_release_dirs(source) {
        return Err(format!(
            "{} 只提供扁平安装包，不提供 Python 组件 MSI",
            python_source_label(source)
        ));
    }
    let base = python_versions_index_url(source)?;
    Ok(format!(
        "{}/{}/{}/{}.msi",
        base.trim_end_matches('/'),
        python_release_dir(version),
        python_msi_arch_dir(version),
        component
    ))
}

fn python_component_cache_dir(version: &str) -> Result<PathBuf, String> {
    let root = pyenv_root().ok_or("未找到 pyenv（未安装或 PATH 未刷新）")?;
    Ok(Path::new(&root)
        .join("install_cache")
        .join(format!("{version}-components")))
}

fn resolve_python_installer(
    version: &str,
    source: &str,
) -> Result<PythonInstallerArtifact, String> {
    for filename in python_installer_filenames(version) {
        let Some(kind) = python_installer_kind(&filename) else {
            continue;
        };
        let url = python_installer_url(version, source, &filename)?;
        if quick_head(&url, Duration::from_secs(5)).is_ok() {
            return Ok(PythonInstallerArtifact {
                filename,
                url,
                kind,
            });
        }
    }
    Err(format!(
        "{} 中未找到 Python {version} 的 64 位 Windows 安装包",
        python_source_label(source)
    ))
}

fn python_versions_index_url(source: &str) -> Result<String, String> {
    let mirror = python_runtime_mirror(source).ok_or("未知 Python 下载源")?;
    let url = mirror.url.trim();
    if url.is_empty() {
        return Err(format!("{} 未配置下载地址", mirror.name));
    }
    if source == "aliyun" {
        return Ok("https://mirrors.aliyun.com/python-release/windows/".into());
    }
    if let Some(pos) = url.find("{version}") {
        return Ok(url[..pos].trim_end_matches('/').to_string() + "/");
    }
    Ok(url.trim_end_matches('/').to_string() + "/")
}

fn is_valid_python_version_token(token: &str) -> bool {
    let mut dot_count = 0usize;
    let mut chars = token.chars().peekable();
    for idx in 0..3 {
        let mut saw_digit = false;
        while matches!(chars.peek(), Some(ch) if ch.is_ascii_digit()) {
            saw_digit = true;
            chars.next();
        }
        if !saw_digit {
            return false;
        }
        if idx < 2 {
            if chars.next() != Some('.') {
                return false;
            }
            dot_count += 1;
        }
    }
    if dot_count != 2 {
        return false;
    }
    let suffix: String = chars.collect();
    suffix.is_empty()
        || suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

fn add_version(versions: &mut Vec<String>, version: &str) {
    if is_valid_python_version_token(version) && !versions.iter().any(|v| v == version) {
        versions.push(version.to_string());
    }
}

fn is_stable_python_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn parse_python_file_versions(body: &str) -> Vec<String> {
    let mut versions = Vec::<String>::new();
    for part in body.split("python-").skip(1) {
        if let Some(pos) = part.find("-amd64.exe") {
            add_version(&mut versions, &part[..pos]);
        }
        if let Some(pos) = part.find(".amd64.msi") {
            add_version(&mut versions, &part[..pos]);
        }
    }
    versions.sort_by(|a, b| ver_cmp(b, a));
    versions
}

fn parse_python_release_dirs(body: &str) -> Vec<String> {
    let mut versions = Vec::<String>::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        let token = &body[start..i];
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            let major = parts[0].parse::<u32>().unwrap_or(0);
            let minor = parts[1].parse::<u32>().unwrap_or(999);
            let patch = parts[2].parse::<u32>().unwrap_or(999);
            if (major == 2 || major == 3) && minor < 100 && patch < 1000 {
                add_version(&mut versions, &format!("{major}.{minor}.{patch}"));
            }
        }
    }
    versions.sort_by(|a, b| ver_cmp(b, a));
    versions
}

fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn key(v: &str) -> [u32; 5] {
        let mut nums = [0u32; 5];
        let parts: Vec<&str> = v.split('.').collect();
        for (i, slot) in nums.iter_mut().enumerate().take(2) {
            *slot = parts.get(i).and_then(|p| p.parse().ok()).unwrap_or(0);
        }
        let patch_part = parts.get(2).copied().unwrap_or_default();
        let digit_len = patch_part
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .map(char::len_utf8)
            .sum();
        nums[2] = patch_part[..digit_len].parse().unwrap_or(0);
        let suffix = &patch_part[digit_len..].to_ascii_lowercase();
        nums[3] = if suffix.is_empty() {
            50
        } else if suffix.starts_with("rc") {
            40
        } else if suffix.starts_with('b') {
            30
        } else if suffix.starts_with('a') {
            20
        } else if suffix.starts_with("dev") {
            10
        } else {
            1
        };
        nums[4] = suffix
            .chars()
            .filter(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        nums
    }
    let ka = key(a);
    let kb = key(b);
    for i in 0..ka.len() {
        let d = ka[i].cmp(&kb[i]);
        if d != std::cmp::Ordering::Equal {
            return d;
        }
    }
    std::cmp::Ordering::Equal
}

fn source_has_release_dirs(source: &str) -> bool {
    python_runtime_mirror(source)
        .map(|m| m.url.contains("{version}"))
        .unwrap_or(false)
}

fn python_release_dir_url(source: &str, version: &str) -> Result<String, String> {
    let base = python_versions_index_url(source)?;
    Ok(format!("{}/", base.trim_end_matches('/')) + &python_release_dir(version) + "/")
}

fn add_versions(dst: &mut Vec<String>, src: Vec<String>) {
    for version in src {
        add_version(dst, &version);
    }
}

fn parse_recent_release_dir_file_versions(source: &str, dirs: &[String]) -> Vec<String> {
    use std::sync::mpsc;
    const INSPECT_DIR_LIMIT: usize = 80;
    let inspect: Vec<String> = dirs.iter().take(INSPECT_DIR_LIMIT).cloned().collect();
    let (tx, rx) = mpsc::channel();
    for dir_version in inspect {
        let tx = tx.clone();
        let source = source.to_string();
        std::thread::spawn(move || {
            let versions = python_release_dir_url(&source, &dir_version)
                .and_then(|url| {
                    agent_with_timeout(Duration::from_secs(5))
                        .get(&url)
                        .call()
                        .map_err(|e| e.to_string())?
                        .into_string()
                        .map_err(|e| e.to_string())
                })
                .map(|body| parse_python_file_versions(&body))
                .unwrap_or_default();
            let _ = tx.send(versions);
        });
    }
    drop(tx);
    let mut versions = Vec::new();
    for rows in rx {
        add_versions(&mut versions, rows);
    }
    versions.sort_by(|a, b| ver_cmp(b, a));
    versions
}

fn python_versions_from_source(
    source: &str,
    include_prerelease: bool,
) -> Result<Vec<String>, String> {
    let mirror = python_runtime_mirror(source).ok_or("未知 Python 下载源")?;
    let url = python_versions_index_url(source)?;
    let body = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .timeout(Duration::from_secs(30))
        .build()
        .get(&url)
        .call()
        .map_err(|e| format!("读取{}版本列表失败：{e}", mirror.name))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let mut versions = parse_python_file_versions(&body);
    if !include_prerelease {
        versions.retain(|v| is_stable_python_version(v));
    }
    let release_dirs = parse_python_release_dirs(&body);
    add_versions(
        &mut versions,
        filter_installable_python_versions(source, release_dirs.clone()),
    );
    if include_prerelease && source_has_release_dirs(source) {
        add_versions(
            &mut versions,
            parse_recent_release_dir_file_versions(source, &release_dirs),
        );
    }
    versions.sort_by(|a, b| ver_cmp(b, a));
    if versions.is_empty() {
        Err(format!(
            "{}版本列表中没有找到可下载的 64 位 Windows Python 安装器",
            mirror.name
        ))
    } else {
        Ok(versions)
    }
}

fn filter_installable_python_versions(source: &str, versions: Vec<String>) -> Vec<String> {
    use std::sync::mpsc;
    let candidates: Vec<String> = versions;
    let (tx, rx) = mpsc::channel();
    for (idx, version) in candidates.iter().cloned().enumerate() {
        let tx = tx.clone();
        let source = source.to_string();
        std::thread::spawn(move || {
            let ok = resolve_python_installer(&version, &source).is_ok();
            let _ = tx.send((idx, version, ok));
        });
    }
    drop(tx);
    let mut rows: Vec<(usize, String, bool)> = rx.into_iter().collect();
    rows.sort_by_key(|r| r.0);
    rows.into_iter()
        .filter_map(|(_, version, ok)| ok.then_some(version))
        .collect()
}

fn python_source_label(source: &str) -> String {
    python_runtime_mirror(source)
        .map(|s| s.name)
        .unwrap_or_else(|| "未知源".into())
}

fn preseed_python(
    window: &tauri::Window,
    version: &str,
    source: &str,
    log: &InstallLog,
) -> Result<PythonInstallerDownload, String> {
    use std::io::{Read, Write};
    const STALL_TIMEOUT_SECS: u64 = 30;
    let root = pyenv_root().ok_or("未找到 pyenv（未安装或 PATH 未刷新）")?;
    let artifact = resolve_python_installer(version, source)?;
    log.line(format!(
        "preseed full installer resolved filename={} url={} kind={}",
        artifact.filename,
        artifact.url,
        match artifact.kind {
            PythonInstallerKind::Exe => "exe",
            PythonInstallerKind::Msi => "msi",
        }
    ));
    let fname = artifact.filename;
    let cache = std::path::Path::new(&root).join("install_cache");
    let dest = cache.join(&fname);
    if dest.is_file() {
        log.line(format!(
            "preseed full installer cache hit path={} size={}",
            dest.display(),
            dest.metadata().map(|m| m.len()).unwrap_or(0)
        ));
        let _ = window.emit("install-progress", "安装文件已就绪".to_string());
        return Ok(PythonInstallerDownload {
            path: dest,
            kind: artifact.kind,
        });
    } // 已缓存
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let url = artifact.url;
    let label = python_source_label(source);
    let _ = window.emit(
        "install-progress",
        format!("正在通过「{label}」下载安装文件…"),
    );
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_write(Duration::from_secs(STALL_TIMEOUT_SECS))
        .build();
    let _ = window.emit("install-progress", "正在连接下载源…".to_string());
    let probe = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_write(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout(Duration::from_secs(STALL_TIMEOUT_SECS))
        .build();
    let probe_result = match probe.head(&url).call() {
        Ok(response) => Ok(response),
        Err(e) => {
            log.line(format!("preseed HEAD failed err={e}; trying range GET"));
            probe.get(&url).set("Range", "bytes=0-0").call()
        }
    };
    match probe_result {
        Ok(resp) => log.line(format!(
            "preseed probe ok status={} content_length={}",
            resp.status(),
            resp.header("Content-Length").unwrap_or("")
        )),
        Err(e) => {
            log.line(format!("preseed probe failed err={e}"));
            return Err(format!(
                "连接「{}」下载源失败：{}。请切换下载源后重试",
                label, e
            ));
        }
    }
    let resp = agent.get(&url).call().map_err(|e| {
        log.line(format!("preseed GET failed err={e}"));
        format!("连接「{}」下载源失败：{}。请切换下载源后重试", label, e)
    })?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let tmp = dest.with_extension("part");
    let result = (|| -> Result<(), String> {
        let mut reader = resp.into_reader();
        let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 16];
        let (mut got, mut last) = (0u64, 0u64);
        loop {
            if crate::installer::op_cancelled() {
                return Err("已取消".into());
            }
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            got += n as u64;
            if got - last > 2_000_000 {
                last = got;
                let msg = if total > 0 {
                    format!(
                        "下载安装文件 {:.0}% · {:.1}/{:.1} MB",
                        got as f64 * 100.0 / total as f64,
                        got as f64 / 1048576.0,
                        total as f64 / 1048576.0
                    )
                } else {
                    format!("下载安装文件 {:.1} MB", got as f64 / 1048576.0)
                };
                let _ = window.emit("install-progress", msg);
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
            log.line(format!(
                "preseed download complete path={} size={}",
                dest.display(),
                dest.metadata().map(|m| m.len()).unwrap_or(0)
            ));
            Ok(PythonInstallerDownload {
                path: dest,
                kind: artifact.kind,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            log.line(format!("preseed download failed err={e}"));
            Err(e)
        }
    }
}

fn download_file_to_cache(
    window: &tauri::Window,
    url: &str,
    dest: &Path,
    label: &str,
    log: &InstallLog,
) -> Result<(), String> {
    use std::io::{Read, Write};
    const STALL_TIMEOUT_SECS: u64 = 30;
    log.line(format!(
        "download request label={label} url={url} dest={}",
        dest.display()
    ));
    if dest.is_file() {
        log.line(format!(
            "download cache hit label={label} dest={} size={}",
            dest.display(),
            dest.metadata().map(|m| m.len()).unwrap_or(0)
        ));
        let _ = window.emit("install-progress", "安装文件已就绪".to_string());
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = window.emit("install-progress", "正在下载安装文件…".to_string());
    let probe = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_write(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout(Duration::from_secs(STALL_TIMEOUT_SECS))
        .build();
    let probe_result = match probe.head(url).call() {
        Ok(response) => Ok(response),
        Err(e) => {
            log.line(format!(
                "download HEAD failed label={label} err={e}; trying range GET"
            ));
            probe.get(url).set("Range", "bytes=0-0").call()
        }
    };
    match probe_result {
        Ok(resp) => log.line(format!(
            "download probe ok label={label} status={} content_length={}",
            resp.status(),
            resp.header("Content-Length").unwrap_or("")
        )),
        Err(e) => {
            log.line(format!("download probe failed label={label} err={e}"));
            return Err(format!("连接下载源失败：{e}"));
        }
    }

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(STALL_TIMEOUT_SECS))
        .timeout_write(Duration::from_secs(STALL_TIMEOUT_SECS))
        .build();
    let resp = agent.get(url).call().map_err(|e| {
        log.line(format!("download GET failed label={label} err={e}"));
        format!("下载安装文件失败：{e}")
    })?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let tmp = dest.with_extension("part");
    let result = (|| -> Result<(), String> {
        let mut reader = resp.into_reader();
        let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 16];
        let (mut got, mut last) = (0u64, 0u64);
        loop {
            if crate::installer::op_cancelled() {
                return Err("已取消".into());
            }
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            got += n as u64;
            if got - last > 1_000_000 {
                last = got;
                let msg = if total > 0 {
                    format!(
                        "下载安装文件 {:.0}% · {:.1}/{:.1} MB",
                        got as f64 * 100.0 / total as f64,
                        got as f64 / 1048576.0,
                        total as f64 / 1048576.0
                    )
                } else {
                    format!("下载安装文件 {:.1} MB", got as f64 / 1048576.0)
                };
                let _ = window.emit("install-progress", msg);
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
            log.line(format!(
                "download complete label={label} dest={} size={}",
                dest.display(),
                dest.metadata().map(|m| m.len()).unwrap_or(0)
            ));
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            log.line(format!("download failed label={label} err={e}"));
            Err(e)
        }
    }
}

fn installed_python_ready(version_dir: &std::path::Path, version: &str) -> bool {
    if !python_core_layout_ready(version_dir) {
        return false;
    }
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return true;
    };
    let Some(minor) = parts.next() else {
        return true;
    };
    version_dir.join(format!("python{major}.exe")).is_file()
        && (version_dir
            .join(format!("python{major}{minor}.exe"))
            .is_file()
            || version_dir
                .join(format!("python{major}.{minor}.exe"))
                .is_file())
}

fn copy_exe_alias(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if src.is_file() {
        std::fs::copy(src, dst).map_err(|e| format!("复制 {} 失败：{e}", dst.display()))?;
    }
    Ok(())
}

fn python_core_layout_ready(version_dir: &Path) -> bool {
    version_dir.join("python.exe").is_file()
        && version_dir.join("pythonw.exe").is_file()
        && version_dir.join("Lib").join("os.py").is_file()
}

fn python_major_minor(version: &str) -> Option<(&str, &str)> {
    let mut parts = version.split('.');
    let major = parts.next()?.trim();
    let minor = parts.next()?.trim();
    if major.is_empty() || minor.is_empty() {
        None
    } else {
        Some((major, minor))
    }
}

#[cfg(windows)]
fn default_python_install_dirs(version: &str) -> Vec<PathBuf> {
    let Some((major, minor)) = python_major_minor(version) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        dirs.push(
            local
                .join("Programs")
                .join("Python")
                .join(format!("Python{major}{minor}")),
        );
    }
    if major == "2" {
        dirs.push(PathBuf::from(format!(r"C:\Python{major}{minor}")));
    }
    dirs
}

#[cfg(not(windows))]
fn default_python_install_dirs(_version: &str) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn registered_python_install_dirs(version: &str) -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let Some((major, minor)) = python_major_minor(version) else {
        return Vec::new();
    };
    let subkey = format!(r"Software\Python\PythonCore\{major}.{minor}\InstallPath");
    let mut dirs = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let Ok(key) = RegKey::predef(hive).open_subkey(&subkey) else {
            continue;
        };
        if let Ok(path) = key.get_value::<String, _>("") {
            let path = path.trim();
            if !path.is_empty() {
                dirs.push(PathBuf::from(path));
            }
        }
        if let Ok(path) = key.get_value::<String, _>("InstallPath") {
            let path = path.trim();
            if !path.is_empty() {
                dirs.push(PathBuf::from(path));
            }
        }
    }
    dedupe_paths(dirs)
}

#[cfg(not(windows))]
fn registered_python_install_dirs(_version: &str) -> Vec<PathBuf> {
    Vec::new()
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        let key = path
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase();
        if !out.iter().any(|p: &PathBuf| {
            p.to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .eq_ignore_ascii_case(&key)
        }) {
            out.push(path);
        }
    }
    out
}

fn existing_python_install_dirs(version: &str) -> Vec<PathBuf> {
    let mut dirs = registered_python_install_dirs(version);
    dirs.extend(default_python_install_dirs(version));
    dedupe_paths(dirs)
}

fn path_starts_with_loose(path: &Path, root: &Path) -> bool {
    let p = path
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    let r = root
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    p == r || p.starts_with(&(r + "\\"))
}

fn is_stacker_managed_python_dir(path: &Path, version_dir: &Path) -> bool {
    if let Some(versions_dir) = version_dir.parent() {
        if path_starts_with_loose(path, versions_dir) {
            return true;
        }
    }
    pyenv_root()
        .map(PathBuf::from)
        .map(|root| path_starts_with_loose(path, &root))
        .unwrap_or(false)
}

fn registered_python_conflict(version: &str) -> Option<PathBuf> {
    registered_python_install_dirs(version)
        .into_iter()
        .find(|dir| {
            python_core_layout_ready(dir)
                && !python_exe_version_matches(&dir.join("python.exe"), version)
        })
}

fn has_stale_stacker_python_registration(version: &str, version_dir: &Path) -> bool {
    registered_python_install_dirs(version)
        .into_iter()
        .any(|dir| {
            !python_core_layout_ready(&dir)
                && (!dir.exists() || is_stacker_managed_python_dir(&dir, version_dir))
        })
}

#[cfg(windows)]
fn reg_string(key: &winreg::RegKey, name: &str) -> String {
    key.get_value::<String, _>(name).unwrap_or_default()
}

fn normalized_text(raw: &str) -> String {
    raw.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn text_mentions_path(text: &str, path: &Path) -> bool {
    let needle = normalized_text(&path.to_string_lossy());
    !needle.is_empty() && normalized_text(text).contains(&needle)
}

fn python_display_matches(version: &str, display_name: &str, display_version: &str) -> bool {
    let name = display_name.trim();
    let version_ok = display_version.trim().starts_with(version);
    name.eq_ignore_ascii_case(&format!("Python {version} (64-bit)"))
        || name.eq_ignore_ascii_case(&format!("Python {version}"))
        || (name.starts_with(&format!("Python {version} ")) && version_ok)
        || (name.starts_with("Python ") && version_ok)
}

#[cfg(windows)]
fn cleanup_python_core_registration(version: &str, version_dir: &Path) -> Result<usize, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let Some((major, minor)) = python_major_minor(version) else {
        return Ok(0);
    };
    let core_name = format!("{major}.{minor}");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let parent_path = r"Software\Python\PythonCore";
    let Ok(parent) = hkcu.open_subkey_with_flags(parent_path, KEY_READ | KEY_WRITE) else {
        return Ok(0);
    };
    let Ok(core) = parent.open_subkey_with_flags(&core_name, KEY_READ) else {
        return Ok(0);
    };
    let install_path = core
        .open_subkey_with_flags("InstallPath", KEY_READ)
        .ok()
        .map(|k| {
            let unnamed = reg_string(&k, "");
            if unnamed.trim().is_empty() {
                reg_string(&k, "InstallPath")
            } else {
                unnamed
            }
        })
        .unwrap_or_default();
    let install_dir = PathBuf::from(install_path.trim());
    let should_remove = !install_path.trim().is_empty()
        && (is_stacker_managed_python_dir(&install_dir, version_dir)
            || path_starts_with_loose(&install_dir, version_dir))
        && !python_core_layout_ready(&install_dir);
    if should_remove {
        parent
            .delete_subkey_all(&core_name)
            .map_err(|e| format!("清理 PythonCore 注册表失败：{e}"))?;
        Ok(1)
    } else {
        Ok(0)
    }
}

#[cfg(not(windows))]
fn cleanup_python_core_registration(_version: &str, _version_dir: &Path) -> Result<usize, String> {
    Ok(0)
}

#[cfg(windows)]
fn cleanup_python_uninstall_registration(
    version: &str,
    version_dir: &Path,
) -> Result<usize, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_path = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let Ok(uninstall) = hkcu.open_subkey_with_flags(uninstall_path, KEY_READ | KEY_WRITE) else {
        return Ok(0);
    };
    let versions_dir = version_dir.parent().map(Path::to_path_buf);
    let pyenv = pyenv_root().map(PathBuf::from);
    let app_pyenv = PathBuf::from(crate::installer::app_dir()).join("pyenv");
    let stale_stacker_registration = has_stale_stacker_python_registration(version, version_dir);
    let mut removed = 0usize;
    let names: Vec<String> = uninstall.enum_keys().flatten().collect();
    for name in names {
        let Ok(key) = uninstall.open_subkey_with_flags(&name, KEY_READ) else {
            continue;
        };
        let display_name = reg_string(&key, "DisplayName");
        let display_version = reg_string(&key, "DisplayVersion");
        let publisher = reg_string(&key, "Publisher");
        if !python_display_matches(version, &display_name, &display_version) {
            continue;
        }
        if !publisher.trim().is_empty()
            && !publisher
                .to_lowercase()
                .contains("python software foundation")
        {
            continue;
        }
        let install_location = reg_string(&key, "InstallLocation");
        let uninstall_string = reg_string(&key, "UninstallString");
        let quiet_uninstall_string = reg_string(&key, "QuietUninstallString");
        let modify_path = reg_string(&key, "ModifyPath");
        let bundle_cache_path = reg_string(&key, "BundleCachePath");
        let values = [
            install_location.as_str(),
            uninstall_string.as_str(),
            quiet_uninstall_string.as_str(),
            modify_path.as_str(),
            bundle_cache_path.as_str(),
        ]
        .join("\n");
        let managed_ref = text_mentions_path(&values, version_dir)
            || versions_dir
                .as_ref()
                .map(|p| text_mentions_path(&values, p))
                .unwrap_or(false)
            || pyenv
                .as_ref()
                .map(|p| text_mentions_path(&values, p))
                .unwrap_or(false)
            || text_mentions_path(&values, &app_pyenv);
        let install_missing =
            !install_location.trim().is_empty() && !PathBuf::from(install_location.trim()).exists();
        if managed_ref || install_missing || stale_stacker_registration {
            uninstall
                .delete_subkey_all(&name)
                .map_err(|e| format!("清理系统卸载登记失败：{e}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(not(windows))]
fn cleanup_python_uninstall_registration(
    _version: &str,
    _version_dir: &Path,
) -> Result<usize, String> {
    Ok(0)
}

fn cleanup_python_registry_for_version(version: &str, version_dir: &Path) -> Result<usize, String> {
    Ok(cleanup_python_uninstall_registration(version, version_dir)?
        + cleanup_python_core_registration(version, version_dir)?)
}

#[cfg(windows)]
fn parse_python_version_from_display(display_name: &str, display_version: &str) -> Option<String> {
    let from_name = display_name
        .strip_prefix("Python ")
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.trim_matches(|ch: char| ch == '(' || ch == ')'));
    if let Some(v) = from_name {
        if v.chars().filter(|ch| *ch == '.').count() >= 2 {
            return Some(v.to_string());
        }
    }
    let mut parts = display_version.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch_build = parts.next()?;
    if patch_build.len() < 3 {
        return None;
    }
    let patch = patch_build[..patch_build.len().saturating_sub(3)]
        .parse::<u32>()
        .ok()?;
    Some(format!("{major}.{minor}.{patch}"))
}

#[cfg(windows)]
fn cleanup_stale_python_registrations() -> Result<usize, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    let Some(root) = pyenv_root().map(PathBuf::from) else {
        return Ok(0);
    };
    let versions_dir = root.join("versions");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_path = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut versions = Vec::<String>::new();
    if let Ok(uninstall) = hkcu.open_subkey_with_flags(uninstall_path, KEY_READ) {
        for name in uninstall.enum_keys().flatten() {
            let Ok(key) = uninstall.open_subkey_with_flags(&name, KEY_READ) else {
                continue;
            };
            let display_name = reg_string(&key, "DisplayName");
            if !display_name.starts_with("Python ") {
                continue;
            }
            let display_version = reg_string(&key, "DisplayVersion");
            if let Some(version) =
                parse_python_version_from_display(&display_name, &display_version)
            {
                if !versions.iter().any(|v| v == &version) {
                    versions.push(version);
                }
            }
        }
    }
    if let Ok(core_parent) = hkcu.open_subkey_with_flags(r"Software\Python\PythonCore", KEY_READ) {
        for minor in core_parent.enum_keys().flatten() {
            let Ok(core) = core_parent.open_subkey_with_flags(&minor, KEY_READ) else {
                continue;
            };
            let install_path = core
                .open_subkey_with_flags("InstallPath", KEY_READ)
                .ok()
                .map(|k| reg_string(&k, ""))
                .unwrap_or_default();
            let install_dir = PathBuf::from(install_path.trim());
            if !install_path.trim().is_empty()
                && path_starts_with_loose(&install_dir, &versions_dir)
                && !python_core_layout_ready(&install_dir)
            {
                if let Some(name) = install_dir.file_name().and_then(|s| s.to_str()) {
                    if !versions.iter().any(|v| v == name) {
                        versions.push(name.to_string());
                    }
                }
            }
        }
    }
    let mut removed = 0usize;
    for version in versions {
        removed += cleanup_python_registry_for_version(&version, &versions_dir.join(&version))?;
    }
    Ok(removed)
}

#[cfg(not(windows))]
fn cleanup_stale_python_registrations() -> Result<usize, String> {
    Ok(0)
}

fn python_exe_version_matches(python: &Path, version: &str) -> bool {
    if !python.is_file() {
        return false;
    }
    let mut c = std::process::Command::new(python);
    c.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let Ok(out) = c.output() else {
        return false;
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    text.split_whitespace().any(|token| token == version)
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for ent in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        if crate::installer::op_cancelled() {
            return Err("已取消".into());
        }
        let p = ent.path();
        let dest = dst.join(ent.file_name());
        match ent.file_type() {
            Ok(ft) if ft.is_dir() => copy_dir_all(&p, &dest)?,
            Ok(_) => {
                std::fs::copy(&p, &dest)
                    .map_err(|e| format!("复制 {} 失败：{e}", dest.display()))?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

fn try_copy_existing_python_install(
    window: &tauri::Window,
    version: &str,
    version_dir: &Path,
) -> Result<(), String> {
    if python_core_layout_ready(version_dir) {
        return Ok(());
    }
    for source_dir in existing_python_install_dirs(version) {
        if !python_core_layout_ready(&source_dir)
            || !python_exe_version_matches(&source_dir.join("python.exe"), version)
        {
            continue;
        }
        let _ = window.emit("install-progress", "正在准备本地运行环境…".to_string());
        if version_dir.exists() {
            std::fs::remove_dir_all(version_dir).map_err(|e| format!("清理目标目录失败：{e}"))?;
        }
        return copy_dir_all(&source_dir, version_dir);
    }
    Ok(())
}

fn python_installer_log_path(version: &str) -> Result<PathBuf, String> {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Stacker")
        .join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    Ok(dir.join(format!("python-installer-{version}-{stamp}.log")))
}

fn installer_log_excerpt(log_path: &Path) -> String {
    let display = log_path.display();
    let Ok(text) = std::fs::read_to_string(log_path) else {
        return format!("\n安装日志：{display}（未生成或无法读取）");
    };
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(8)
        .map(ToString::to_string)
        .collect();
    lines.reverse();
    if lines.is_empty() {
        format!("\n安装日志：{display}（日志为空）")
    } else {
        format!("\n安装日志：{display}\n日志末尾：{}", lines.join(" | "))
    }
}

fn any_existing_python_ready(version: &str) -> bool {
    existing_python_install_dirs(version).iter().any(|dir| {
        python_core_layout_ready(dir)
            && python_exe_version_matches(&dir.join("python.exe"), version)
    })
}

fn finalize_python_layout(
    window: &tauri::Window,
    version_dir: &std::path::Path,
    version: &str,
) -> Result<(), String> {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or_default();
    let minor = parts.next().unwrap_or_default();
    if major.is_empty() || minor.is_empty() {
        return Err("Python 版本号格式异常".into());
    }

    let python = version_dir.join("python.exe");
    let pythonw = version_dir.join("pythonw.exe");
    if !python.is_file() {
        return Err(format!(
            "Python 安装未完成：目标目录缺少 python.exe（{}）。该版本安装文件可能尚未同步完整；请换一个补丁版本或切换下载源后重试",
            python.display()
        ));
    }
    if !pythonw.is_file() {
        return Err(format!(
            "Python 安装器已退出，但目标目录不完整：缺少 {}。请换一个补丁版本或切换下载源后重试",
            pythonw.display()
        ));
    }
    let stdlib = version_dir.join("Lib").join("os.py");
    if !stdlib.is_file() {
        return Err(format!(
            "Python 安装器已退出，但目标目录不完整：缺少标准库文件 {}。请换一个补丁版本或切换下载源后重试",
            stdlib.display()
        ));
    }

    let _ = window.emit("install-progress", "正在配置命令入口…".to_string());
    for suffix in [
        major.to_string(),
        format!("{major}{minor}"),
        format!("{major}.{minor}"),
    ] {
        copy_exe_alias(&python, &version_dir.join(format!("python{suffix}.exe")))?;
        copy_exe_alias(&pythonw, &version_dir.join(format!("pythonw{suffix}.exe")))?;
    }

    let venv = version_dir
        .join("Lib")
        .join("venv")
        .join("scripts")
        .join("nt");
    let venv_py = venv.join("python.exe");
    let venv_pyw = venv.join("pythonw.exe");
    if venv_py.is_file() {
        for suffix in [
            major.to_string(),
            format!("{major}{minor}"),
            format!("{major}.{minor}"),
        ] {
            copy_exe_alias(&venv_py, &venv.join(format!("python{suffix}.exe")))?;
        }
    }
    if venv_pyw.is_file() {
        for suffix in [
            major.to_string(),
            format!("{major}{minor}"),
            format!("{major}.{minor}"),
        ] {
            copy_exe_alias(&venv_pyw, &venv.join(format!("pythonw{suffix}.exe")))?;
        }
    }
    Ok(())
}

fn extract_msi_to_python_dir(
    window: &tauri::Window,
    msi: &Path,
    version_dir: &Path,
    label: &str,
    log: &InstallLog,
) -> Result<(), String> {
    if crate::installer::op_cancelled() {
        log.line(format!(
            "msiexec skipped because operation cancelled label={label}"
        ));
        return Err("已取消".into());
    }
    if let Some(parent) = version_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let target = version_dir.to_string_lossy().to_string();
    let msi_arg = msi.to_string_lossy().to_string();
    let msiexec_log = log.sidecar(&format!("msiexec-{label}"));
    let msiexec_log_arg = msiexec_log.to_string_lossy().to_string();
    let args = [
        "/a".to_string(),
        msi_arg,
        "/qn".to_string(),
        "/norestart".to_string(),
        "/L*v".to_string(),
        msiexec_log_arg,
        format!("TARGETDIR={target}"),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    log.line(format!(
        "msiexec extract start label={label} msi={} target={} log={}",
        msi.display(),
        version_dir.display(),
        msiexec_log.display()
    ));
    let started = std::time::Instant::now();
    let result = crate::installer::run_with_heartbeat(
        window,
        "msiexec.exe",
        &refs,
        &[],
        "正在安装 Python 运行时",
    );
    match &result {
        Ok(()) => log.line(format!(
            "msiexec extract ok label={label} elapsed_ms={}",
            started.elapsed().as_millis()
        )),
        Err(e) => log.line(format!(
            "msiexec extract failed label={label} elapsed_ms={} err={e}",
            started.elapsed().as_millis()
        )),
    }
    result
}

fn cleanup_extracted_msi_files(version_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(version_dir) {
        for ent in entries.flatten() {
            let path = ent.path();
            if path
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("msi"))
                .unwrap_or(false)
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn ensure_python_pip(
    window: &tauri::Window,
    version_dir: &Path,
    version: &str,
    log: &InstallLog,
) -> Result<(), String> {
    if version.split('.').next().unwrap_or_default() != "3" {
        log.line(format!(
            "ensurepip skipped for non-Python-3 version={version}"
        ));
        return Ok(());
    }
    let pip = version_dir.join("Scripts").join("pip.exe");
    if pip.is_file() {
        log.line(format!(
            "ensurepip skipped because pip exists path={}",
            pip.display()
        ));
        return Ok(());
    }
    let python = version_dir.join("python.exe");
    if !python.is_file() {
        log.line(format!(
            "ensurepip skipped because python.exe is missing path={}",
            python.display()
        ));
        return Ok(());
    }
    let program = python.to_string_lossy().to_string();
    let args = ["-m", "ensurepip", "--upgrade", "--default-pip"];
    let python_home = version_dir.to_string_lossy().to_string();
    let envs = [("PYTHONHOME", python_home.as_str())];
    log.line(format!(
        "ensurepip start version={version} program={} python_home={}",
        python.display(),
        version_dir.display()
    ));
    let started = std::time::Instant::now();
    let result = crate::installer::run_with_heartbeat(
        window,
        &program,
        &args,
        &envs,
        "正在配置 pip 与包管理工具",
    );
    match &result {
        Ok(()) => log.line(format!(
            "ensurepip ok elapsed_ms={} pip_exists={}",
            started.elapsed().as_millis(),
            pip.is_file()
        )),
        Err(e) => log.line(format!(
            "ensurepip failed elapsed_ms={} err={e} pip_exists={}",
            started.elapsed().as_millis(),
            pip.is_file()
        )),
    }
    log_version_dir_snapshot(log, version_dir, "after ensurepip");
    result
}

fn ensure_python_pip_best_effort(
    window: &tauri::Window,
    version_dir: &Path,
    version: &str,
    log: &InstallLog,
) -> Result<(), String> {
    if let Err(e) = ensure_python_pip(window, version_dir, version, log) {
        if crate::installer::op_cancelled() {
            return Err(e);
        }
        log.line(format!("ensurepip nonfatal err={e}; continuing"));
        let _ = window.emit(
            "install-progress",
            format!("Python 已安装，但 pip 配置未完成：{e}"),
        );
    }
    Ok(())
}

fn log_version_dir_snapshot(log: &InstallLog, version_dir: &Path, stage: &str) {
    let exists = version_dir.exists();
    let key_files = [
        "python.exe",
        "pythonw.exe",
        "Lib\\os.py",
        "Scripts\\pip.exe",
        "DLLs\\_socket.pyd",
    ];
    log.line(format!(
        "dir snapshot stage={stage} dir={} exists={exists}",
        version_dir.display()
    ));
    for rel in key_files {
        let path = version_dir.join(rel);
        log.line(format!(
            "dir key stage={stage} rel={rel} exists={} size={}",
            path.is_file(),
            path.metadata().map(|m| m.len()).unwrap_or(0)
        ));
    }
    if let Ok(entries) = std::fs::read_dir(version_dir) {
        let mut names = Vec::new();
        for ent in entries.flatten().take(30) {
            let ty = ent
                .file_type()
                .map(|ft| if ft.is_dir() { "dir" } else { "file" })
                .unwrap_or("unknown");
            names.push(format!("{}:{ty}", ent.file_name().to_string_lossy()));
        }
        log.line(format!(
            "dir top stage={stage} entries={}",
            names.join(", ")
        ));
    }
}

fn log_registered_python_state(log: &InstallLog, version: &str, version_dir: &Path) {
    log.line(format!(
        "registered/default python state version={version} target={}",
        version_dir.display()
    ));
    for dir in registered_python_install_dirs(version) {
        log.line(format!(
            "registered dir path={} exists={} core_ready={} version_match={} stacker_managed={}",
            dir.display(),
            dir.exists(),
            python_core_layout_ready(&dir),
            python_exe_version_matches(&dir.join("python.exe"), version),
            is_stacker_managed_python_dir(&dir, version_dir)
        ));
    }
    for dir in default_python_install_dirs(version) {
        log.line(format!(
            "default dir path={} exists={} core_ready={} version_match={}",
            dir.display(),
            dir.exists(),
            python_core_layout_ready(&dir),
            python_exe_version_matches(&dir.join("python.exe"), version)
        ));
    }
    log.line(format!(
        "registered conflict={:?}",
        registered_python_conflict(version).map(|p| p.to_string_lossy().to_string())
    ));
    log.line(format!(
        "stale stacker registration={}",
        has_stale_stacker_python_registration(version, version_dir)
    ));
}

fn install_python_with_component_msis(
    window: &tauri::Window,
    version: &str,
    source: &str,
    version_dir: &Path,
    log: &InstallLog,
) -> Result<bool, String> {
    log.line(format!(
        "component-msi install probe start version={version} source={source} version_dir={}",
        version_dir.display()
    ));
    let mut required_components = Vec::new();
    let mut optional_components = Vec::new();
    for component in PYTHON_REQUIRED_MSI_COMPONENTS {
        let url = match python_component_msi_url(version, source, component) {
            Ok(url) => url,
            Err(e) => {
                log.line(format!(
                    "component-msi required url failed component={component} err={e}"
                ));
                return Ok(false);
            }
        };
        log.line(format!(
            "component-msi probe required component={component} url={url}"
        ));
        match quick_head(&url, Duration::from_secs(5)) {
            Ok(()) => {
                log.line(format!(
                    "component-msi required available component={component}"
                ));
                required_components.push((component.to_string(), url));
            }
            Err(e) => {
                log.line(format!(
                    "component-msi required unavailable component={component} err={e}"
                ));
                return Ok(false);
            }
        }
    }
    for component in PYTHON_OPTIONAL_MSI_COMPONENTS {
        let Ok(url) = python_component_msi_url(version, source, component) else {
            log.line(format!(
                "component-msi optional url unavailable component={component}"
            ));
            continue;
        };
        if quick_head(&url, Duration::from_secs(5)).is_ok() {
            log.line(format!(
                "component-msi optional available component={component}"
            ));
            optional_components.push((component.to_string(), url));
        } else {
            log.line(format!(
                "component-msi optional unavailable component={component}"
            ));
        }
    }

    if version_dir.exists() {
        log.line(format!(
            "component-msi remove existing version_dir={}",
            version_dir.display()
        ));
        let _ = window.emit("install-progress", "正在清理上次未完成的安装…".to_string());
        std::fs::remove_dir_all(version_dir).map_err(|e| format!("清理未完成安装目录失败：{e}"))?;
    }
    std::fs::create_dir_all(version_dir).map_err(|e| e.to_string())?;
    let cache = python_component_cache_dir(version)?;
    let component_names = required_components
        .iter()
        .chain(optional_components.iter())
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    log.line(format!(
        "component-msi install start components={} cache={} target={}",
        component_names,
        cache.display(),
        version_dir.display()
    ));
    for (component, url) in required_components {
        let msi = cache.join(format!("{component}.msi"));
        download_file_to_cache(
            window,
            &url,
            &msi,
            &format!("Python {version} 组件 {component}.msi"),
            log,
        )?;
        extract_msi_to_python_dir(
            window,
            &msi,
            version_dir,
            &format!("Python {version} 组件 {component}.msi"),
            log,
        )?;
        log_version_dir_snapshot(log, version_dir, &format!("after component {component}"));
    }
    for (component, url) in optional_components {
        let result = (|| -> Result<(), String> {
            let msi = cache.join(format!("{component}.msi"));
            download_file_to_cache(
                window,
                &url,
                &msi,
                &format!("Python {version} 组件 {component}.msi"),
                log,
            )?;
            extract_msi_to_python_dir(
                window,
                &msi,
                version_dir,
                &format!("Python {version} 组件 {component}.msi"),
                log,
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                log.line(format!("component-msi optional ok component={component}"));
                log_version_dir_snapshot(log, version_dir, &format!("after optional {component}"));
            }
            Err(e) if crate::installer::op_cancelled() => return Err(e),
            Err(e) => {
                log.line(format!(
                    "component-msi optional failed component={component} err={e}; continuing"
                ));
                let _ = window.emit(
                    "install-progress",
                    "部分附加组件未完成，正在继续配置核心运行环境".to_string(),
                );
            }
        }
    }
    cleanup_extracted_msi_files(version_dir);
    log_version_dir_snapshot(log, version_dir, "after cleanup extracted msi files");
    finalize_python_layout(window, version_dir, version)?;
    ensure_python_pip_best_effort(window, version_dir, version, log)?;
    log_version_dir_snapshot(log, version_dir, "after finalize layout");
    Ok(true)
}

fn install_python_with_official_installer(
    window: &tauri::Window,
    version: &str,
    installer: &std::path::Path,
    version_dir: &std::path::Path,
    log: &InstallLog,
) -> Result<(), String> {
    log.line(format!(
        "full-installer fallback start version={version} installer={} target={}",
        installer.display(),
        version_dir.display()
    ));
    if installed_python_ready(version_dir, version) {
        log.line("full-installer skip because target already ready");
        let _ = window.emit(
            "install-progress",
            "安装文件已就绪，正在配置运行环境…".to_string(),
        );
        finalize_python_layout(window, version_dir, version)?;
        ensure_python_pip_best_effort(window, version_dir, version, log)?;
        return Ok(());
    }

    if version_dir.exists() {
        log.line(format!(
            "full-installer removing existing target={}",
            version_dir.display()
        ));
        let _ = window.emit("install-progress", "正在清理上次未完成的安装…".to_string());
        std::fs::remove_dir_all(version_dir).map_err(|e| format!("清理未完成安装目录失败：{e}"))?;
    }
    if let Some(parent) = version_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    try_copy_existing_python_install(window, version, version_dir)?;
    log_version_dir_snapshot(
        log,
        version_dir,
        "after try_copy_existing before full installer",
    );
    if installed_python_ready(version_dir, version) {
        log.line("full-installer satisfied by existing install copy");
        finalize_python_layout(window, version_dir, version)?;
        ensure_python_pip_best_effort(window, version_dir, version, log)?;
        return Ok(());
    }
    if let Some(conflict) = registered_python_conflict(version) {
        log.line(format!(
            "full-installer blocked by registered conflict path={}",
            conflict.display()
        ));
        return Err(format!(
            "已检测到系统中注册了同一 Python {} 系列：{}。Python 官方安装器按 major.minor 单实例注册，Stacker 不会自动修改这份系统 Python；请换一个未注册的系列，或先在系统中卸载/清理该 Python 后重试。",
            python_release_dir(version),
            conflict.display()
        ));
    }
    if has_stale_stacker_python_registration(version, version_dir) {
        log.line("full-installer blocked by stale Stacker registration");
        return Err(format!(
            "检测到上次 Python {} 安装留下了无效注册。当前下载源没有可用组件 MSI，只能走官方 full installer，而 full installer 会卡在这个注册状态；请切换到清华、北外、华为、中科大或南京大学等目录型 Python 下载源后重试。",
            python_release_dir(version)
        ));
    }

    let log_path = python_installer_log_path(version)?;
    let target = version_dir.to_string_lossy().to_string();
    let log_arg = log_path.to_string_lossy().to_string();
    let args = vec![
        "/quiet".to_string(),
        "/log".to_string(),
        log_arg,
        "InstallAllUsers=0".to_string(),
        "PrependPath=0".to_string(),
        "Include_launcher=0".to_string(),
        "Include_test=0".to_string(),
        "Include_pip=1".to_string(),
        "Include_exe=1".to_string(),
        "Include_lib=1".to_string(),
        "Include_dev=1".to_string(),
        "Include_tools=1".to_string(),
        "Include_tcltk=1".to_string(),
        format!("TargetDir={target}"),
        format!("DefaultJustForMeTargetDir={target}"),
        format!("DefaultCustomTargetDir={target}"),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let program = installer.to_string_lossy().to_string();
    log.line(format!(
        "full-installer run program={} args={}",
        program,
        args.join(" ")
    ));
    let success_probe =
        || python_core_layout_ready(version_dir) || any_existing_python_ready(version);
    let started = std::time::Instant::now();
    if let Err(e) = crate::installer::run_with_heartbeat_until(
        window,
        &program,
        &refs,
        &[],
        "正在安装 Python 运行时",
        &success_probe,
    ) {
        log.line(format!(
            "full-installer failed elapsed_ms={} err={e}",
            started.elapsed().as_millis()
        ));
        return Err(format!(
            "Python 安装器执行失败：{e}{}",
            installer_log_excerpt(&log_path)
        ));
    }
    log.line(format!(
        "full-installer returned ok elapsed_ms={}",
        started.elapsed().as_millis()
    ));
    try_copy_existing_python_install(window, version, version_dir)?;
    log_version_dir_snapshot(log, version_dir, "after full installer");
    if let Err(e) = finalize_python_layout(window, version_dir, version) {
        log.line(format!("full-installer finalize failed err={e}"));
        return Err(format!("{e}{}", installer_log_excerpt(&log_path)));
    }
    ensure_python_pip_best_effort(window, version_dir, version, log)?;
    if !installed_python_ready(version_dir, version) {
        log.line("full-installer target not ready after finalize");
        return Err(format!(
            "Python 安装器已退出，但版本目录不完整{}",
            installer_log_excerpt(&log_path)
        ));
    }
    Ok(())
}

fn install_python_with_msi_installer(
    window: &tauri::Window,
    version: &str,
    installer: &std::path::Path,
    version_dir: &std::path::Path,
    log: &InstallLog,
) -> Result<(), String> {
    log.line(format!(
        "legacy-msi install start version={version} installer={} target={}",
        installer.display(),
        version_dir.display()
    ));
    if installed_python_ready(version_dir, version) {
        log.line("legacy-msi skip because target already ready");
        let _ = window.emit(
            "install-progress",
            "安装文件已就绪，正在配置运行环境…".to_string(),
        );
        finalize_python_layout(window, version_dir, version)?;
        ensure_python_pip_best_effort(window, version_dir, version, log)?;
        return Ok(());
    }

    if version_dir.exists() {
        log.line(format!(
            "legacy-msi removing existing target={}",
            version_dir.display()
        ));
        let _ = window.emit("install-progress", "正在清理上次未完成的安装…".to_string());
        std::fs::remove_dir_all(version_dir).map_err(|e| format!("清理未完成安装目录失败：{e}"))?;
    }
    if let Some(parent) = version_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    extract_msi_to_python_dir(window, installer, version_dir, "Python 运行时", log)?;
    cleanup_extracted_msi_files(version_dir);
    log_version_dir_snapshot(log, version_dir, "after legacy msi extract");
    if let Err(e) = finalize_python_layout(window, version_dir, version) {
        log.line(format!("legacy-msi finalize failed err={e}"));
        return Err(e);
    }
    ensure_python_pip_best_effort(window, version_dir, version, log)?;
    if !installed_python_ready(version_dir, version) {
        log.line("legacy-msi target not ready after finalize");
        return Err("Python MSI 已解包，但版本目录不完整".into());
    }
    log.line("legacy-msi install ok");
    Ok(())
}

#[tauri::command]
pub async fn pyenv_install_version(
    window: tauri::Window,
    version: String,
    source: Option<String>,
    install_root: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "official".into());
        let log = InstallLog::new(&version, &source)?;
        let _ = window.emit("install-progress", "准备安装环境…".to_string());
        let result = (|| -> Result<String, String> {
            let pyenv_bat = pyenv_bat().ok_or("未找到 pyenv（未安装或 PATH 未刷新）")?;
            crate::installer::op_reset();
            let root = install_root
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(normalize_pyenv_root)
                .unwrap_or_else(|| pyenv_root().unwrap_or_default());
            if install_root
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some()
            {
                ensure_pyenv_root(&root)?;
            }
            let version_dir = std::path::Path::new(&root).join("versions").join(&version);
            log.line(format!("pyenv_bat={pyenv_bat}"));
            log.line(format!("pyenv_root={root}"));
            log.line(format!("version_dir={}", version_dir.display()));
            log.line(format!("source_label={}", python_source_label(&source)));
            log.line(format!(
                "source_mirror={:?}",
                python_runtime_mirror(&source).map(|m| m.url)
            ));
            log_registered_python_state(&log, &version, &version_dir);
            log_version_dir_snapshot(&log, &version_dir, "before install");

            if version.split('.').next().unwrap_or_default() != "2"
                && install_python_with_component_msis(
                    &window,
                    &version,
                    &source,
                    &version_dir,
                    &log,
                )?
            {
                log.line("component-msi path completed; running pyenv rehash");
                let _ = window.emit("install-progress", "正在刷新已安装版本…".to_string());
                let rehash = run_pyenv_at(&root, &["rehash"]);
                log.line(format!("pyenv rehash result={rehash:?}"));
                log_version_dir_snapshot(&log, &version_dir, "after successful component path");
                return Ok(version.clone());
            }

            log.line(
                "component-msi path unavailable or skipped; falling back to full installer path",
            );
            let installer = preseed_python(&window, &version, &source, &log)?;
            match installer.kind {
                PythonInstallerKind::Exe => install_python_with_official_installer(
                    &window,
                    &version,
                    &installer.path,
                    &version_dir,
                    &log,
                )?,
                PythonInstallerKind::Msi => install_python_with_msi_installer(
                    &window,
                    &version,
                    &installer.path,
                    &version_dir,
                    &log,
                )?,
            }
            log.line("fallback path completed; running pyenv rehash");
            let _ = window.emit("install-progress", "正在刷新已安装版本…".to_string());
            let rehash = run_pyenv_at(&root, &["rehash"]);
            log.line(format!("pyenv rehash result={rehash:?}"));
            log_version_dir_snapshot(&log, &version_dir, "after successful fallback path");
            Ok(version)
        })();
        match result {
            Ok(v) => {
                log.line(format!("SUCCESS installed version={v}"));
                Ok::<String, String>(v)
            }
            Err(e) => Err(log.error(e)),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn pyenv_uninstall_version(version: String) -> Result<(), String> {
    if version.is_empty()
        || version
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_'))
    {
        return Err("Python 版本号异常，已拒绝卸载".into());
    }
    let root = pyenv_root().ok_or("未找到 pyenv（未安装或 PATH 未刷新）")?;
    let versions_dir = Path::new(&root).join("versions");
    let target = versions_dir.join(&version);
    if !target.starts_with(&versions_dir) {
        return Err("Python 版本目录异常，已拒绝卸载".into());
    }
    if pyenv_global_version()
        .as_deref()
        .is_some_and(|default| default.eq_ignore_ascii_case(&version))
    {
        let _ = run_pyenv(&["global", "system"]);
    }
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("删除 Python 版本目录失败：{e}"))?;
    }
    cleanup_python_registry_for_version(&version, &target)?;
    let _ = run_pyenv(&["rehash"]);
    Ok(())
}

#[tauri::command]
pub fn pyenv_cleanup_stale_registrations() -> Result<usize, String> {
    cleanup_stale_python_registrations()
}

#[tauri::command]
pub async fn pyenv_install_list(
    source: Option<String>,
    include_prerelease: Option<bool>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "official".into());
        python_versions_from_source(&source, include_prerelease.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 检查 pyenv-win 自身是否有新版。官方源查 GitHub Release，其他镜像查对应 PyPI 索引。
#[tauri::command]
pub async fn pyenv_check_update(
    source: Option<String>,
) -> Result<crate::update::UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let current = run_pyenv(&["--version"])
            .map(|s| s.replace("pyenv", "").trim().to_string())
            .unwrap_or_default();
        let source = source.unwrap_or_else(|| "official".into());
        let latest = if source == "official" {
            crate::update::github_latest_tag("pyenv-win/pyenv-win")?
        } else {
            pyenv_latest_version(&source)?
        };
        let has_update = !current.is_empty() && crate::update::ver_lt(&current, &latest);
        Ok(crate::update::UpdateInfo {
            current,
            latest,
            has_update,
            release_url: None,
            installer_url: None,
            portable_url: None,
            published_at: None,
            notes: Vec::new(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 更新 pyenv-win 自身：重新下载 pyenv-win 覆盖。
#[tauri::command]
pub async fn pyenv_self_update(
    window: tauri::Window,
    source: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        install_self_online(window, source).map(|_| "pyenv-win 已更新到最新版".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn pypi_file_url(base_url: &str, href: &str) -> String {
    let clean = href.split('#').next().unwrap_or(href).trim_matches('"');
    if clean.starts_with("http://") || clean.starts_with("https://") {
        return clean.to_string();
    }
    let Some(scheme_pos) = base_url.find("://") else {
        return clean.to_string();
    };
    let after_scheme = scheme_pos + 3;
    let host_end = base_url[after_scheme..]
        .find('/')
        .map(|i| after_scheme + i)
        .unwrap_or(base_url.len());
    let origin = &base_url[..host_end];
    if clean.starts_with('/') {
        return format!("{origin}{clean}");
    }
    let base_path = base_url[host_end..].split('?').next().unwrap_or("/");
    let mut parts: Vec<&str> = base_path
        .trim_start_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    if !base_url.ends_with('/') {
        parts.pop();
    }
    for part in clean.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("{origin}/{}", parts.join("/"))
}

fn latest_pyenv_wheel_href(body: &str) -> Option<String> {
    let mut latest = None;
    for part in body.split("href=") {
        let Some(rest) = part.strip_prefix('"').or_else(|| part.strip_prefix('\'')) else {
            continue;
        };
        let quote = if part.starts_with('"') { '"' } else { '\'' };
        let Some(end) = rest.find(quote) else {
            continue;
        };
        let href = &rest[..end];
        if href.contains("pyenv_win-") && href.contains("-py3-none-any.whl") {
            latest = Some(href.to_string());
        }
    }
    latest
}

fn pyenv_wheel_url(source: &str) -> Result<String, String> {
    let spec = python_runtime_mirror(source).ok_or("未知 pyenv-win 下载源")?;
    let simple = pyenv_pypi_simple(source)
        .ok_or_else(|| format!("{} 未配置 pyenv-win PyPI 镜像地址", spec.name))?;
    let body = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .timeout(Duration::from_secs(30))
        .build()
        .get(simple)
        .call()
        .map_err(|e| format!("读取{} PyPI 镜像失败：{e}", spec.name))?
        .into_string()
        .map_err(|e| e.to_string())?;
    latest_pyenv_wheel_href(&body)
        .map(|href| pypi_file_url(simple, &href))
        .ok_or_else(|| format!("{} PyPI 镜像中未找到 pyenv-win wheel", spec.name))
}

fn pyenv_latest_version(source: &str) -> Result<String, String> {
    let url = pyenv_wheel_url(source)?;
    let filename = url.rsplit('/').next().unwrap_or("");
    filename
        .strip_prefix("pyenv_win-")
        .and_then(|s| s.split('-').next())
        .map(|s| s.replace('_', "."))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "无法从 pyenv-win wheel 文件名解析版本".into())
}

fn pyenv_download_candidates(source: &str) -> Result<Vec<String>, String> {
    match source {
        "official" => Ok(vec![PYENV_GITHUB_URL.to_string()]),
        _ => Ok(vec![pyenv_wheel_url(source)?]),
    }
}

#[derive(Serialize)]
pub struct PyenvSourcePing {
    pub id: String,
    pub name: String,
    pub ms: Option<u64>,
}

fn agent_with_timeout(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .timeout(timeout)
        .build()
}

fn remaining(start: std::time::Instant, total: Duration) -> Option<Duration> {
    total.checked_sub(start.elapsed()).filter(|d| !d.is_zero())
}

fn quick_head(url: &str, timeout: Duration) -> Result<(), String> {
    let agent = agent_with_timeout(timeout);
    match agent.head(url).call() {
        Ok(_) => Ok(()),
        Err(_) => agent
            .get(url)
            .set("Range", "bytes=0-0")
            .call()
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

fn speedtest_pyenv_source(spec: Mirror) -> Option<u64> {
    let total = Duration::from_millis(1500);
    let start = std::time::Instant::now();
    if spec.id == "official" {
        quick_head(PYENV_GITHUB_URL, remaining(start, total)?).ok()?;
    } else {
        let simple = pyenv_pypi_simple(&spec.id)?;
        let body = agent_with_timeout(remaining(start, total)?)
            .get(simple)
            .call()
            .ok()?
            .into_string()
            .ok()?;
        let wheel = pypi_file_url(simple, &latest_pyenv_wheel_href(&body)?);
        quick_head(&wheel, remaining(start, total)?).ok()?;
    }
    let filename = format!("python-{PYTHON_SPEEDTEST_VERSION}-amd64.exe");
    let installer = python_installer_url(PYTHON_SPEEDTEST_VERSION, &spec.id, &filename).ok()?;
    quick_head(&installer, remaining(start, total)?).ok()?;
    Some(start.elapsed().as_millis() as u64)
}

/// Python 下载源测速：官方源和镜像源都参与；1500ms 内 pyenv-win 与 Python 安装包链路都通过才算可用。
#[tauri::command]
pub async fn pyenv_speedtest_sources(window: tauri::Window) -> Vec<PyenvSourcePing> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::sync::mpsc;
        let sources: Vec<Mirror> = crate::sources::python_runtime_mirrors()
            .into_iter()
            .filter(|s| s.id == "official" || pyenv_pypi_simple(&s.id).is_some())
            .collect();
        let (tx, rx) = mpsc::channel();
        for spec in sources.iter().cloned() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let ms = speedtest_pyenv_source(spec.clone());
                let _ = tx.send(PyenvSourcePing {
                    id: spec.id,
                    name: spec.name,
                    ms,
                });
            });
        }
        drop(tx);
        let mut rows = Vec::new();
        for row in rx {
            let _ = window.emit(
                "pyenv-source-speed-progress",
                match row.ms {
                    Some(ms) => format!("{} · {}ms", row.name, ms),
                    None => format!("{} · 超时", row.name),
                },
            );
            rows.push(row);
        }
        rows.sort_by_key(|r| {
            sources
                .iter()
                .position(|s| s.id == r.id)
                .unwrap_or(usize::MAX)
        });
        let _ = window.emit("pyenv-source-speed-progress", "__done__".to_string());
        rows
    })
    .await
    .unwrap_or_default()
}

fn pyenv_strip_top(source: &str) -> Result<bool, String> {
    match source {
        "official" => Ok(true),
        _ if python_runtime_mirror(source).is_some() => Ok(false),
        _ => Err("未知 pyenv-win 下载源".into()),
    }
}

/// 一键安装 pyenv-win：下载 zip/wheel 解压到工具目录\pyenv，设 PYENV/PYENV_HOME/ROOT + PATH。
#[tauri::command]
pub async fn pyenv_install_self(
    window: tauri::Window,
    source: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || install_self_impl(window, source))
        .await
        .map_err(|e| e.to_string())?
}
fn install_self_impl(window: tauri::Window, source: Option<String>) -> Result<String, String> {
    // 装到「工具目录\pyenv」（与 JDK/Maven/fnm 等一致，整套随 Stacker 目录走）。
    let root = PathBuf::from(crate::installer::app_dir()).join("pyenv");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let _ = source;
    crate::installer::extract_embedded_zip(
        window.clone(),
        BUNDLED_PYENV_ZIP,
        &format!("pyenv-win {}", &BUNDLED_PYENV_COMMIT[..7]),
        root.to_string_lossy().into_owned(),
        true,
    )?;
    finish_pyenv_self_install(window, &root)?;
    Ok(format!(
        "已安装内置 pyenv-win {}",
        &BUNDLED_PYENV_COMMIT[..7]
    ))
}

fn install_self_online(window: tauri::Window, source: Option<String>) -> Result<String, String> {
    let root = PathBuf::from(crate::installer::app_dir()).join("pyenv");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let source = source.unwrap_or_else(|| "official".into());
    // GitHub master.zip 顶层是 pyenv-win-master/，需要 strip；
    // PyPI wheel 顶层已经是 pyenv-win/，不能 strip。
    let strip_top = pyenv_strip_top(&source)?;
    crate::installer::download_impl_candidates(
        window.clone(),
        pyenv_download_candidates(&source)?,
        root.to_string_lossy().into_owned(),
        strip_top,
    )?;
    finish_pyenv_self_install(window, &root)?;
    Ok("已安装 pyenv-win".into())
}

fn finish_pyenv_self_install(window: tauri::Window, root: &Path) -> Result<(), String> {
    let pd: PathBuf = root.join("pyenv-win");
    if !pd.join("bin").join("pyenv.bat").is_file() {
        return Err("pyenv-win 解压结构异常，请重试".into());
    }
    let base = format!("{}\\", pd.to_string_lossy());
    crate::backup::backup_env(
        crate::winenv::Hive::User,
        "pyenv",
        &["PYENV", "PYENV_HOME", "PYENV_ROOT"],
    );
    crate::winenv::set_user("PYENV", &base)?;
    crate::winenv::set_user("PYENV_HOME", &base)?;
    crate::winenv::set_user("PYENV_ROOT", &base)?;
    crate::winenv::prepend_path_in(crate::winenv::Hive::User, &format!("{base}shims"))?;
    crate::winenv::prepend_path_in(crate::winenv::Hive::User, &format!("{base}bin"))?;
    let _ = window.emit("install-progress", "正在检测 pyenv 安装状态…".to_string());
    Ok(())
}
