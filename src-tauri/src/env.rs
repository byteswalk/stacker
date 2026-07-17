//! 默认环境：管理 JDK/Python/Node/Go 的“默认版本”，靠改用户级 JAVA_HOME/GOROOT/PATH。
//! 扫描用 jwalk 并行遍历选定磁盘/目录（零管理员），严格过滤掉 JRE/捆绑/缓存。

use crate::{backup, winenv};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone)]
pub struct SdkVersion {
    pub kind: String,
    pub version: String,
    pub vendor: String,
    pub path: String,
    pub current: bool,
    pub arch: String,   // x64 | x86 | ARM64 | ""（仅 Java 现填）
    pub origin: String, // managed | external | tool-bundled | project | unknown
    pub can_delete: bool,
}

#[derive(Serialize)]
pub struct SdkGroup {
    pub kind: String,
    pub label: String,
    pub current_desc: String,
    pub versions: Vec<SdkVersion>,
}

#[derive(Serialize, Default)]
pub struct ScanResult {
    pub java: Vec<SdkVersion>,
    pub python: Vec<SdkVersion>,
    pub node: Vec<SdkVersion>,
    pub go: Vec<SdkVersion>,
    pub maven: Vec<SdkVersion>,
    pub gradle: Vec<SdkVersion>,
}

#[derive(Serialize)]
pub struct DriveInfo {
    pub letter: String,
    pub fixed: bool,
}

fn label_for(kind: &str) -> String {
    match kind {
        "java" => "Java",
        "python" => "Python",
        "node" => "Node.js",
        "go" => "Go",
        "maven" => "Maven",
        "gradle" => "Gradle",
        _ => kind,
    }
    .to_string()
}
fn version_desc(kind: &str, v: &SdkVersion) -> String {
    match kind {
        "java" => format!("JDK {} · {}", v.version, v.vendor),
        "python" => format!("Python {}", v.version),
        "node" => format!("Node {}", v.version),
        "go" => format!("Go {}", v.version),
        "maven" => format!("Maven {}", v.version),
        "gradle" => format!("Gradle {}", v.version),
        _ => v.version.clone(),
    }
}

fn extract_version(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            return Some(s[start..i].trim_end_matches('.').to_string());
        }
        i += 1;
    }
    None
}

fn map_java_vendor(imp: &str, home: &str) -> String {
    let s = imp.to_lowercase();
    let h = home.to_lowercase();
    if s.contains("azul") || h.contains("zulu") {
        return "Zulu".into();
    }
    if s.contains("temurin") || s.contains("adoptium") || h.contains("adoptium") {
        return "Temurin".into();
    }
    if s.contains("dragonwell") || s.contains("alibaba") || h.contains("dragonwell") {
        return "Dragonwell".into();
    }
    if s.contains("oracle") {
        return "Oracle".into();
    }
    if s.contains("microsoft") || h.contains("microsoft") {
        return "Microsoft".into();
    }
    if s.contains("amazon") || h.contains("corretto") {
        return "Corretto".into();
    }
    if s.contains("bellsoft") || h.contains("liberica") {
        return "Liberica".into();
    }
    if h.contains("j2sdk") || h.contains("com.sun") || s.contains("sun microsystems") {
        return "Sun".into();
    }
    if !imp.is_empty() {
        return imp.to_string();
    }
    "Unknown".into()
}

/// 从目录名取版本：优先「最长且含点的数字串」(1.6.0 / 25.0.1 / 1.6.0.013)，
/// 避免被路径里的 T420 / x86 之类数字污染（老 JDK 没 release 文件时用）。
fn version_from_dirname(name: &str) -> Option<String> {
    let b = name.as_bytes();
    let mut best: Option<&str> = None;
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let s = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let seg = name[s..i].trim_end_matches('.');
            if seg.contains('.') && best.map_or(true, |bb| seg.len() > bb.len()) {
                best = Some(seg);
            }
        } else {
            i += 1;
        }
    }
    best.map(|s| s.to_string())
        .or_else(|| extract_version(name))
}

fn java_info(home: &Path) -> (String, String) {
    let mut version = String::new();
    let mut imp = String::new();
    if let Ok(text) = std::fs::read_to_string(home.join("release")) {
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("JAVA_VERSION=") {
                version = v.trim_matches('"').to_string();
            } else if let Some(v) = line.strip_prefix("IMPLEMENTOR=") {
                imp = v.trim_matches('"').to_string();
            }
        }
    }
    if version.is_empty() {
        // 老 JDK 无 release 文件：从安装目录名提取，而非整条路径
        let fname = home
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        version = version_from_dirname(&fname).unwrap_or_else(|| "?".into());
    }
    (version, map_java_vendor(&imp, &home.to_string_lossy()))
}

fn make_version(kind: &str, home: &Path, current: Option<&str>) -> SdkVersion {
    let path = home.to_string_lossy().to_string();
    let is_current = current
        .map(|c| {
            c.trim_end_matches(['\\', '/'])
                .eq_ignore_ascii_case(path.trim_end_matches(['\\', '/']))
        })
        .unwrap_or(false);
    let (version, vendor) = match kind {
        "java" => java_info(home),
        "maven" | "gradle" => {
            // Maven/Gradle 安装目录里含版本（apache-maven-3.9.9 / gradle-8.5），
            // 用末段目录名提取，避免被路径中其它数字（如 x86）干扰。
            let fname = home
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            (
                extract_version(&fname)
                    .or_else(|| extract_version(&path))
                    .unwrap_or_else(|| "?".into()),
                String::new(),
            )
        }
        _ => (
            extract_version(&path).unwrap_or_else(|| "?".into()),
            String::new(),
        ),
    };
    let arch = if kind == "java" {
        arch_of(home)
    } else {
        String::new()
    };
    SdkVersion {
        kind: kind.into(),
        version,
        vendor,
        path,
        current: is_current,
        arch,
        origin: origin_for(kind, home),
        can_delete: is_managed_install(kind, home),
    }
}

/// 读 PE 头判断 java.exe 位数：0x8664=x64，0x14c=x86，0xAA64=ARM64。
fn pe_machine(exe: &Path) -> Option<u16> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(exe).ok()?;
    let mut mz = [0u8; 2];
    f.read_exact(&mut mz).ok()?;
    if &mz != b"MZ" {
        return None;
    }
    f.seek(SeekFrom::Start(0x3C)).ok()?;
    let mut lfa = [0u8; 4];
    f.read_exact(&mut lfa).ok()?;
    f.seek(SeekFrom::Start(u32::from_le_bytes(lfa) as u64))
        .ok()?;
    let mut sig = [0u8; 4];
    f.read_exact(&mut sig).ok()?;
    if &sig != b"PE\0\0" {
        return None;
    }
    let mut m = [0u8; 2];
    f.read_exact(&mut m).ok()?;
    Some(u16::from_le_bytes(m))
}
fn arch_of(home: &Path) -> String {
    match pe_machine(&home.join("bin").join("java.exe")) {
        Some(0x8664) => "x64".into(),
        Some(0x014c) => "x86".into(),
        Some(0xAA64) => "ARM64".into(),
        _ => String::new(),
    }
}

// ── 当前默认（来自环境变量 / PATH）──
fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}
fn first_real(exe: &str, skip_substr: &str) -> Option<String> {
    for dir in path_dirs() {
        if dir.join(exe).is_file() {
            let low = dir.to_string_lossy().to_lowercase();
            if skip_substr.is_empty() || !low.contains(skip_substr) {
                return Some(dir.to_string_lossy().to_string());
            }
        }
    }
    None
}
// 读环境变量：注册表（用户→系统）优先于进程旧快照，避免本会话内改了变量仍显示旧值。
fn reg_var(name: &str) -> Option<String> {
    winenv::get_raw_in(winenv::Hive::User, name)
        .or_else(|| winenv::get_raw_in(winenv::Hive::System, name))
        .or_else(|| std::env::var(name).ok())
        .filter(|s| !s.trim().is_empty())
}

fn current_home(kind: &str) -> Option<String> {
    match kind {
        "java" => reg_var("JAVA_HOME"),
        "python" => first_real("python.exe", "windowsapps"),
        "node" => first_real("node.exe", ""),
        "go" => reg_var("GOROOT").or_else(|| {
            first_real("go.exe", "").and_then(|d| {
                Path::new(&d)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
            })
        }),
        "maven" => reg_var("MAVEN_HOME")
            .or_else(|| reg_var("M2_HOME"))
            .or_else(|| {
                first_real("mvn.cmd", "").and_then(|d| {
                    Path::new(&d)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                })
            }),
        "gradle" => reg_var("GRADLE_HOME").or_else(|| {
            first_real("gradle.bat", "").and_then(|d| {
                Path::new(&d)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
            })
        }),
        _ => None,
    }
}

// ── 用「注册表最新 PATH」解析命令真实指向（不受 app 进程旧快照影响）──
fn expand_vars(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find('%') {
        out.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        if let Some(q) = after.find('%') {
            let name = &after[..q];
            out.push_str(&reg_var(name).unwrap_or_else(|| format!("%{name}%")));
            rest = &after[q + 1..];
        } else {
            out.push('%');
            rest = after;
            break;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(windows)]
pub fn fresh_path_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for h in [winenv::Hive::System, winenv::Hive::User] {
        for e in winenv::get_path_in(h) {
            v.push(PathBuf::from(expand_vars(&e)));
        }
    }
    v
}
#[cfg(not(windows))]
pub fn fresh_path_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// 按「注册表最新 PATH」找可执行文件全路径（不受 app 进程旧快照影响）。
pub fn resolve_fresh(exe: &str) -> Option<PathBuf> {
    fresh_path_dirs()
        .into_iter()
        .map(|d| d.join(exe))
        .find(|p| p.is_file())
}

/// 跑 `<exe> -version` 解析 (完整版本, 主版本)。1.8.0_x→8；21.0.3→21。
fn java_run(exe: &str) -> Option<(String, String)> {
    let mut c = std::process::Command::new(exe);
    c.arg("-version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let out = c.output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let q = text.find('"')?;
    let rest = &text[q + 1..];
    let e = rest.find('"')?;
    let ver = rest[..e].to_string();
    let major = if ver.starts_with("1.") {
        ver.split('.').nth(1)?.to_string()
    } else {
        ver.split('.').next()?.to_string()
    };
    Some((ver, major))
}

pub fn java_home_reg() -> Option<String> {
    reg_var("JAVA_HOME")
}

/// 命令 `java` 的真实指向（按最新 PATH 解析）。
pub fn java_cmd() -> Option<(String, String)> {
    let exe = resolve_fresh("java.exe")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "java".into());
    java_run(&exe)
}

/// JAVA_HOME 指向的 java 版本。
pub fn java_of_home(home: &str) -> Option<(String, String)> {
    java_run(&format!(
        "{}\\bin\\java.exe",
        home.trim_end_matches(['\\', '/'])
    ))
}

#[derive(Serialize, Default)]
pub struct JavaEffective {
    pub cmd_version: Option<String>,
    pub cmd_major: Option<String>,
    pub home_path: Option<String>,
    pub home_version: Option<String>,
    pub home_major: Option<String>,
    pub split: bool, // 命令 java 与 JAVA_HOME 主版本不一致
}

#[tauri::command]
pub async fn env_java_effective() -> JavaEffective {
    tauri::async_runtime::spawn_blocking(|| {
        let cmd = java_cmd();
        let home = java_home_reg();
        let home_v = home.as_deref().and_then(java_of_home);
        let split = matches!((&cmd, &home_v), (Some((_, c)), Some((_, h))) if c != h);
        JavaEffective {
            cmd_version: cmd.as_ref().map(|(v, _)| v.clone()),
            cmd_major: cmd.as_ref().map(|(_, m)| m.clone()),
            home_path: home,
            home_version: home_v.as_ref().map(|(v, _)| v.clone()),
            home_major: home_v.as_ref().map(|(_, m)| m.clone()),
            split,
        }
    })
    .await
    .unwrap_or_default()
}

// ── 磁盘列举 ──
#[cfg(windows)]
#[tauri::command]
pub fn list_drives() -> Vec<DriveInfo> {
    use winapi::um::fileapi::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) != 0 {
            let letter = (b'A' + i as u8) as char;
            let root = format!("{letter}:\\");
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            let dt = unsafe { GetDriveTypeW(wide.as_ptr()) };
            out.push(DriveInfo {
                letter: format!("{letter}:"),
                fixed: dt == 3,
            }); // DRIVE_FIXED
        }
    }
    out
}
#[cfg(not(windows))]
#[tauri::command]
pub fn list_drives() -> Vec<DriveInfo> {
    vec![]
}

// ── 扫描（jwalk 并行遍历 + 严格过滤）──
fn is_noise(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "node_modules"
            | ".git"
            | "$recycle.bin"
            | "system volume information"
            | "windows"
            | "winsxs"
            | "found.000"
            | ".cargo"
            | ".gradle"
            | ".npm"
            | ".cache"
            | "__pycache__"
            | "cache"
            | "caches"
            | "temp"
            | "tmp"
    ) || n.starts_with("$windows.")
}

fn java_home_from_javac(javac: &Path) -> Option<PathBuf> {
    let bin = javac.parent()?;
    if bin.file_name()?.to_str()?.eq_ignore_ascii_case("bin") && bin.join("java.exe").is_file() {
        return Some(bin.parent()?.to_path_buf());
    }
    None
}
fn python_home(py: &Path) -> Option<PathBuf> {
    let dir = py.parent()?;
    let dn = dir.file_name()?.to_str()?.to_lowercase();
    if dn == "scripts" {
        return None;
    }
    if dir.join("pyvenv.cfg").is_file() {
        return None;
    }
    let low = dir.to_string_lossy().to_lowercase();
    if low.contains("windowsapps") || low.contains("\\node_modules\\") {
        return None;
    }
    Some(dir.to_path_buf())
}
fn node_home(node: &Path) -> Option<PathBuf> {
    let dir = node.parent()?;
    let low = dir.to_string_lossy().to_lowercase();
    if low.contains("\\node_modules\\") {
        return None;
    }
    Some(dir.to_path_buf())
}
fn go_home(go: &Path) -> Option<PathBuf> {
    let bin = go.parent()?;
    if bin.file_name()?.to_str()?.eq_ignore_ascii_case("bin") {
        return Some(bin.parent()?.to_path_buf());
    }
    None
}
/// exe 位于 <home>\bin\ 时返回 home（用于 mvn.cmd / gradle.bat）。
fn bin_parent(exe: &Path) -> Option<PathBuf> {
    let bin = exe.parent()?;
    if bin.file_name()?.to_str()?.eq_ignore_ascii_case("bin") {
        return Some(bin.parent()?.to_path_buf());
    }
    None
}

#[derive(Default)]
struct Homes {
    java: Vec<PathBuf>,
    python: Vec<PathBuf>,
    node: Vec<PathBuf>,
    go: Vec<PathBuf>,
    maven: Vec<PathBuf>,
    gradle: Vec<PathBuf>,
}

fn build(kind: &str, mut homes: Vec<PathBuf>) -> Vec<SdkVersion> {
    if let Some(c) = current_home(kind) {
        let p = PathBuf::from(&c);
        if p.exists() {
            homes.push(p);
        }
    }
    homes.retain(|p| p.exists()); // 失效路径（被删/移动）自动剔除
    homes.sort();
    homes.dedup_by(|a, b| {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    });
    let cur = current_home(kind);
    let mut v: Vec<SdkVersion> = homes
        .iter()
        .map(|h| make_version(kind, h, cur.as_deref()))
        .collect();
    v.sort_by(|a, b| b.current.cmp(&a.current).then(b.version.cmp(&a.version)));
    v
}

// ── 扫描结果缓存：静态 + 落盘，切页/重启都不丢（按 kind 存安装根路径）──
fn scanned() -> &'static Mutex<HashMap<String, Vec<PathBuf>>> {
    static S: OnceLock<Mutex<HashMap<String, Vec<PathBuf>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(load_scan_cache()))
}
fn managed_installs() -> &'static Mutex<HashMap<String, Vec<PathBuf>>> {
    static MANAGED: OnceLock<Mutex<HashMap<String, Vec<PathBuf>>>> = OnceLock::new();
    MANAGED.get_or_init(|| Mutex::new(load_path_map(&managed_installs_path())))
}
fn scan_cache_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("scan_cache.json")
}
fn managed_installs_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("managed_installs.json")
}
fn load_path_map(path: &Path) -> HashMap<String, Vec<PathBuf>> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<HashMap<String, Vec<String>>>(&text).ok())
        .map(|items| {
            items
                .into_iter()
                .map(|(kind, paths)| (kind, paths.into_iter().map(PathBuf::from).collect()))
                .collect()
        })
        .unwrap_or_default()
}
fn save_path_map(path: &Path, values: &HashMap<String, Vec<PathBuf>>) {
    let serializable: HashMap<String, Vec<String>> = values
        .iter()
        .map(|(kind, paths)| {
            (
                kind.clone(),
                paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect(),
            )
        })
        .collect();
    if let Ok(text) = serde_json::to_string_pretty(&serializable) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }
}
fn is_managed_path(path: &Path) -> bool {
    managed_installs()
        .lock()
        .map(|items| {
            items.values().flatten().any(|managed| {
                managed
                    .to_string_lossy()
                    .trim_end_matches(['\\', '/'])
                    .eq_ignore_ascii_case(path.to_string_lossy().trim_end_matches(['\\', '/']))
            })
        })
        .unwrap_or(false)
}

fn legacy_managed_root(kind: &str) -> Option<PathBuf> {
    let folder = match kind {
        "java" => "jdk",
        "maven" => "maven",
        "gradle" => "gradle",
        "go" => "go",
        _ => return None,
    };
    let base = PathBuf::from(crate::installer::app_dir());
    (!base.as_os_str().is_empty()).then(|| base.join(folder))
}

fn has_runtime_marker(kind: &str, path: &Path) -> bool {
    match kind {
        "java" => path.join(r"bin\java.exe").is_file(),
        "maven" => path.join(r"bin\mvn.cmd").is_file(),
        "gradle" => path.join(r"bin\gradle.bat").is_file(),
        "go" => path.join(r"bin\go.exe").is_file(),
        _ => false,
    }
}

/// Compatibility for releases that installed runtimes beside Stacker before the
/// managed-install manifest was introduced. Only complete runtimes below the
/// ecosystem's dedicated Stacker directory qualify.
fn is_legacy_managed_path(kind: &str, path: &Path) -> bool {
    if !path.is_dir() || !has_runtime_marker(kind, path) {
        return false;
    }
    let Some(root) = legacy_managed_root(kind) else {
        return false;
    };
    let Ok(target) = path.canonicalize() else {
        return false;
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    target != root && target.starts_with(root)
}

fn is_managed_install(kind: &str, path: &Path) -> bool {
    is_managed_path(path) || is_legacy_managed_path(kind, path)
}
fn is_tool_bundled(path: &Path) -> bool {
    let value = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    [
        "\\jetbrains\\",
        "\\android studio\\",
        "\\androidstudio",
        "\\microsoft visual studio\\",
        "\\.vscode\\extensions\\",
        "\\eclipse\\plugins\\",
        "\\myeclipse\\",
        "\\adobe\\",
    ]
    .iter()
    .any(|part| value.contains(part))
}
fn is_project_bundled(path: &Path) -> bool {
    let value = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    value.contains("\\.m2\\wrapper\\")
        || value.contains("\\.gradle\\wrapper\\")
        || value.contains("\\gradle\\wrapper\\dists\\")
}
fn origin_for(kind: &str, path: &Path) -> String {
    if is_managed_install(kind, path) {
        "managed"
    } else if is_tool_bundled(path) {
        "tool-bundled"
    } else if is_project_bundled(path) {
        "project"
    } else if path.is_absolute() {
        "external"
    } else {
        "unknown"
    }
    .into()
}
fn load_scan_cache() -> HashMap<String, Vec<PathBuf>> {
    std::fs::read_to_string(scan_cache_path())
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, Vec<String>>>(&s).ok())
        .map(|m| {
            m.into_iter()
                .map(|(k, v)| (k, v.into_iter().map(PathBuf::from).collect()))
                .collect()
        })
        .unwrap_or_default()
}
fn save_scan_cache(c: &HashMap<String, Vec<PathBuf>>) {
    let m: HashMap<String, Vec<String>> = c
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.iter().map(|p| p.to_string_lossy().to_string()).collect(),
            )
        })
        .collect();
    if let Ok(s) = serde_json::to_string_pretty(&m) {
        let p = scan_cache_path();
        if let Some(par) = p.parent() {
            let _ = std::fs::create_dir_all(par);
        }
        let _ = std::fs::write(p, s);
    }
}
fn cache_scan(homes: &Homes, kinds: &std::collections::HashSet<String>) {
    let mut c = scanned().lock().unwrap();
    let all = kinds.is_empty();
    if all || kinds.contains("java") {
        c.insert("java".into(), homes.java.clone());
    }
    if all || kinds.contains("python") {
        c.insert("python".into(), homes.python.clone());
    }
    if all || kinds.contains("node") {
        c.insert("node".into(), homes.node.clone());
    }
    if all || kinds.contains("go") {
        c.insert("go".into(), homes.go.clone());
    }
    if all || kinds.contains("maven") {
        c.insert("maven".into(), homes.maven.clone());
    }
    if all || kinds.contains("gradle") {
        c.insert("gradle".into(), homes.gradle.clone());
    }
    save_scan_cache(&c);
}

// IDE / 工具自带的 JDK（JetBrains jbr、Android Studio、MyEclipse 等）——通常不作为独立 JDK 使用，可选过滤。
fn is_ide_jdk(path: &Path) -> bool {
    let p = path.to_string_lossy().to_lowercase();
    [
        "\\jbr",
        "jbrsdk",
        "jetbrains",
        "android studio",
        "myeclipse",
        "\\.p2\\",
    ]
    .iter()
    .any(|m| p.contains(m))
}

#[tauri::command]
pub async fn env_scan(
    window: tauri::Window,
    roots: Vec<String>,
    exclude_ide_jdk: Option<bool>,
    exclude_tool_bundled: Option<bool>,
    kinds: Option<Vec<String>>,
) -> ScanResult {
    // 放到后台阻塞线程，避免堵住主线程导致界面卡死
    tauri::async_runtime::spawn_blocking(move || {
        scan_impl(
            window,
            roots,
            exclude_ide_jdk.unwrap_or(false),
            exclude_tool_bundled.unwrap_or(false),
            kinds.unwrap_or_default(),
        )
    })
    .await
    .unwrap_or_default()
}

fn scan_impl(
    window: tauri::Window,
    roots: Vec<String>,
    exclude_ide_jdk: bool,
    exclude_tool_bundled: bool,
    kinds: Vec<String>,
) -> ScanResult {
    use tauri::Emitter;
    CANCEL.store(false, Ordering::SeqCst);
    let mut homes = Homes::default();
    let kinds = kinds
        .into_iter()
        .map(|kind| kind.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let wants = |kind: &str| kinds.is_empty() || kinds.contains(kind);
    let count = AtomicUsize::new(0);

    'outer: for root in &roots {
        if root.trim().is_empty() {
            continue;
        }
        let walker = jwalk::WalkDir::new(root)
            .skip_hidden(false)
            .process_read_dir(|_, _, _, children| {
                children.retain(|c| {
                    if let Ok(e) = c {
                        if e.file_type.is_dir() {
                            if let Some(name) = e.file_name.to_str() {
                                return !is_noise(name);
                            }
                        }
                    }
                    true
                });
            });
        for entry in walker {
            if CANCEL.load(Ordering::SeqCst) {
                break 'outer;
            }
            let Ok(e) = entry else { continue };
            if e.file_type.is_dir() {
                let c = count.fetch_add(1, Ordering::Relaxed);
                if c % 400 == 0 {
                    let _ =
                        window.emit("env-scan-progress", e.path().to_string_lossy().to_string());
                }
                continue;
            }
            let path = e.path();
            match e.file_name.to_string_lossy().to_lowercase().as_str() {
                "javac.exe" if wants("java") => {
                    if !(exclude_ide_jdk && is_ide_jdk(&path))
                        && !(exclude_tool_bundled && is_tool_bundled(&path))
                    {
                        if let Some(h) = java_home_from_javac(&path) {
                            homes.java.push(h);
                        }
                    }
                }
                "python.exe" if wants("python") => {
                    if !(exclude_tool_bundled && is_tool_bundled(&path)) {
                        if let Some(h) = python_home(&path) {
                            homes.python.push(h);
                        }
                    }
                }
                "node.exe" if wants("node") => {
                    if !(exclude_tool_bundled && is_tool_bundled(&path)) {
                        if let Some(h) = node_home(&path) {
                            homes.node.push(h);
                        }
                    }
                }
                "go.exe" if wants("go") => {
                    if !(exclude_tool_bundled && is_tool_bundled(&path)) {
                        if let Some(h) = go_home(&path) {
                            homes.go.push(h);
                        }
                    }
                }
                "mvn.cmd" if wants("maven") => {
                    if !(exclude_tool_bundled && is_tool_bundled(&path)) {
                        if let Some(h) = bin_parent(&path) {
                            homes.maven.push(h);
                        }
                    }
                }
                "gradle.bat"
                    if wants("gradle") && !(exclude_tool_bundled && is_tool_bundled(&path)) =>
                {
                    if let Some(h) = bin_parent(&path) {
                        homes.gradle.push(h);
                    }
                }
                _ => {}
            }
        }
    }
    let cancelled = CANCEL.load(Ordering::SeqCst);
    let _ = window.emit("env-scan-progress", "__done__".to_string());

    // 取消时不落盘：否则把"扫了一半"的不完整结果持久化，下次 env_state 合并后会显示半截列表。
    if !cancelled {
        cache_scan(&homes, &kinds);
    }

    ScanResult {
        java: build("java", homes.java),
        python: build("python", homes.python),
        node: build("node", homes.node),
        go: build("go", homes.go),
        maven: build("maven", homes.maven),
        gradle: build("gradle", homes.gradle),
    }
}

#[tauri::command]
pub fn env_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

// ── 设默认（配套全切）──
fn trim(p: &str) -> &str {
    p.trim_end_matches(['\\', '/'])
}

fn is_related_path(kind: &str, entry: &str, siblings: &[String]) -> bool {
    let raw = entry.trim();
    let low = raw.replace('/', "\\").to_lowercase();
    if siblings.iter().any(|s| {
        let bin = format!("{}\\bin", trim(s))
            .replace('/', "\\")
            .to_lowercase();
        low.eq_ignore_ascii_case(&bin)
    }) {
        return true;
    }
    match kind {
        "java" => low == "%java_home%\\bin" || low.contains("\\jdk") && low.ends_with("\\bin"),
        "go" => {
            low == "%goroot%\\bin"
                || low.ends_with("\\go\\bin")
                || (low.contains("\\go\\go") && low.ends_with("\\bin"))
                || (low.contains("\\golang\\") && low.ends_with("\\bin"))
        }
        "maven" => {
            low == "%maven_home%\\bin"
                || low == "%m2_home%\\bin"
                || (low.contains("apache-maven") && low.ends_with("\\bin"))
        }
        "gradle" => {
            low == "%gradle_home%\\bin" || (low.contains("\\gradle") && low.ends_with("\\bin"))
        }
        "python" => siblings.iter().any(|s| {
            let base = trim(s).replace('/', "\\").to_lowercase();
            low.eq_ignore_ascii_case(&base) || low.eq_ignore_ascii_case(&format!("{base}\\scripts"))
        }),
        "node" => siblings.iter().any(|s| {
            let base = trim(s).replace('/', "\\").to_lowercase();
            low.eq_ignore_ascii_case(&base)
        }),
        _ => false,
    }
}

fn remove_related_path_entries(
    hive: winenv::Hive,
    kind: &str,
    siblings: &[String],
) -> Result<(), String> {
    let before = winenv::get_path_in(hive);
    let after: Vec<String> = before
        .iter()
        .filter(|entry| !is_related_path(kind, entry, siblings))
        .cloned()
        .collect();
    if after.len() != before.len() {
        winenv::set_path_in(hive, &after)?;
    }
    Ok(())
}

pub fn set_default(
    hive: winenv::Hive,
    kind: &str,
    path: &str,
    siblings: Vec<String>,
) -> Result<(), String> {
    let home = trim(path);
    let vars: &[&str] = match kind {
        "java" => &["JAVA_HOME"],
        "python" | "node" => &[],
        "go" => &["GOROOT"],
        "maven" => &["MAVEN_HOME", "M2_HOME"],
        "gradle" => &["GRADLE_HOME"],
        _ => return Err("未知 SDK".into()),
    };
    backup::backup_env(hive, kind, vars);
    match kind {
        "java" => {
            remove_related_path_entries(hive, kind, &siblings)?;
            for s in &siblings {
                winenv::remove_path_in(hive, &format!("{}\\bin", trim(s)))?;
            }
            winenv::remove_path_in(hive, "%JAVA_HOME%\\bin")?;
            winenv::set_in(hive, "JAVA_HOME", home)?;
            winenv::prepend_path_in(hive, "%JAVA_HOME%\\bin")?;
        }
        "python" => {
            remove_related_path_entries(hive, kind, &siblings)?;
            for s in &siblings {
                let s = trim(s);
                winenv::remove_path_in(hive, s)?;
                winenv::remove_path_in(hive, &format!("{s}\\Scripts"))?;
            }
            winenv::prepend_path_in(hive, &format!("{home}\\Scripts"))?;
            winenv::prepend_path_in(hive, home)?;
        }
        "node" => {
            remove_related_path_entries(hive, kind, &siblings)?;
            for s in &siblings {
                winenv::remove_path_in(hive, trim(s))?;
            }
            winenv::prepend_path_in(hive, home)?;
        }
        "go" => {
            remove_related_path_entries(hive, kind, &siblings)?;
            for s in &siblings {
                winenv::remove_path_in(hive, &format!("{}\\bin", trim(s)))?;
            }
            winenv::remove_path_in(hive, "%GOROOT%\\bin")?;
            winenv::set_in(hive, "GOROOT", home)?;
            winenv::prepend_path_in(hive, "%GOROOT%\\bin")?;
        }
        "maven" => {
            remove_related_path_entries(hive, kind, &siblings)?;
            for s in &siblings {
                winenv::remove_path_in(hive, &format!("{}\\bin", trim(s)))?;
            }
            winenv::remove_path_in(hive, "%MAVEN_HOME%\\bin")?;
            winenv::set_in(hive, "MAVEN_HOME", home)?;
            winenv::prepend_path_in(hive, "%MAVEN_HOME%\\bin")?;
        }
        "gradle" => {
            remove_related_path_entries(hive, kind, &siblings)?;
            for s in &siblings {
                winenv::remove_path_in(hive, &format!("{}\\bin", trim(s)))?;
            }
            winenv::remove_path_in(hive, "%GRADLE_HOME%\\bin")?;
            winenv::set_in(hive, "GRADLE_HOME", home)?;
            winenv::prepend_path_in(hive, "%GRADLE_HOME%\\bin")?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn clear_default(hive: winenv::Hive, kind: &str, siblings: &[String]) -> Result<(), String> {
    let vars = vars_for_kind(kind)?;
    backup::backup_env(hive, &format!("{kind}-clear"), vars);
    for var in vars {
        winenv::remove_in(hive, var)?;
    }
    remove_related_path_entries(hive, kind, siblings)?;
    Ok(())
}

fn vars_for_kind(kind: &str) -> Result<&'static [&'static str], String> {
    match kind {
        "java" => Ok(&["JAVA_HOME"]),
        "python" | "node" => Ok(&[]),
        "go" => Ok(&["GOROOT"]),
        "maven" => Ok(&["MAVEN_HOME", "M2_HOME"]),
        "gradle" => Ok(&["GRADLE_HOME"]),
        _ => Err("未知 SDK".into()),
    }
}

fn clear_user_shadow_for_system(kind: &str, siblings: &[String]) -> Result<(), String> {
    let vars = vars_for_kind(kind)?;
    backup::backup_env(winenv::Hive::User, &format!("{kind}-system-shadow"), vars);
    for var in vars {
        winenv::remove_in(winenv::Hive::User, var)?;
    }
    match kind {
        "java" => {
            remove_related_path_entries(winenv::Hive::User, kind, siblings)?;
        }
        "go" => {
            remove_related_path_entries(winenv::Hive::User, kind, siblings)?;
        }
        "maven" => {
            remove_related_path_entries(winenv::Hive::User, kind, siblings)?;
        }
        "gradle" => {
            remove_related_path_entries(winenv::Hive::User, kind, siblings)?;
        }
        _ => {}
    }
    Ok(())
}

// ── Tauri 命令 ──
#[tauri::command]
pub fn env_state() -> Vec<SdkGroup> {
    let mut cache = scanned().lock().unwrap();
    let mut cache_changed = false;
    for homes in cache.values_mut() {
        let before = homes.len();
        homes.retain(|path| path.exists());
        cache_changed |= homes.len() != before;
    }
    if cache_changed {
        save_scan_cache(&cache);
    }
    ["java", "python", "node", "go", "maven", "gradle"]
        .iter()
        .map(|k| {
            // 合并「上次扫描缓存的安装根」+ 当前默认；build 会补当前默认、剔失效、标生效中
            let homes = cache.get(*k).cloned().unwrap_or_default();
            let versions = build(k, homes);
            let desc = versions
                .iter()
                .find(|v| v.current)
                .or_else(|| versions.first())
                .map(|v| version_desc(k, v))
                .unwrap_or_else(|| "未设置".into());
            SdkGroup {
                kind: k.to_string(),
                label: label_for(k),
                current_desc: desc,
                versions,
            }
        })
        .collect()
}

#[tauri::command]
pub fn env_register_install(kind: String, path: String) -> Result<(), String> {
    if !matches!(kind.as_str(), "java" | "go" | "maven" | "gradle") {
        return Err("该生态不使用手动安装目录登记。".into());
    }
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err("安装目录必须是绝对路径。".into());
    }
    if !path.is_dir() {
        return Err(format!("安装目录不存在：{}", path.display()));
    }
    let key = path.to_string_lossy().to_lowercase();
    {
        let mut managed = managed_installs()
            .lock()
            .map_err(|_| "受管安装目录暂时不可用。")?;
        let paths = managed.entry(kind.clone()).or_default();
        if !paths
            .iter()
            .any(|item| item.to_string_lossy().to_lowercase() == key)
        {
            paths.push(path.clone());
            save_path_map(&managed_installs_path(), &managed);
        }
    }
    let mut cache = scanned().lock().map_err(|_| "安装目录缓存暂时不可用。")?;
    let homes = cache.entry(kind).or_default();
    if !homes
        .iter()
        .any(|item| item.to_string_lossy().to_lowercase() == key)
    {
        homes.push(path);
        save_scan_cache(&cache);
    }
    Ok(())
}

#[tauri::command]
pub fn env_remove_managed(kind: String, path: String) -> Result<(), String> {
    if !matches!(kind.as_str(), "java" | "go" | "maven" | "gradle") {
        return Err("该生态不支持通过此操作删除。".into());
    }
    let target = PathBuf::from(path.trim());
    if !target.is_absolute() || !target.is_dir() {
        return Err("目标安装目录不存在。请刷新状态后重试。".into());
    }
    if !is_managed_install(&kind, &target) {
        return Err(
            "该版本不是由 Stacker 安装，无法确认目录内是否包含其他文件。请使用原安装程序卸载。"
                .into(),
        );
    }
    if target.components().count() < 3 {
        return Err("为保护磁盘数据，拒绝删除过短的目录路径。".into());
    }

    let siblings = scanned()
        .lock()
        .map_err(|_| "安装目录缓存暂时不可用。")?
        .get(&kind)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let current = current_home(&kind).is_some_and(|value| {
        value
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(target.to_string_lossy().trim_end_matches(['\\', '/']))
    });
    if current {
        clear_default(winenv::Hive::User, &kind, &siblings)?;
        if env_system_info().get(&kind).copied().unwrap_or(false) {
            crate::winadmin::clear_default_system(&kind, siblings.clone())?;
        }
    }

    std::fs::remove_dir_all(&target).map_err(|error| format!("无法删除安装目录：{error}"))?;
    {
        let mut cache = scanned().lock().map_err(|_| "安装目录缓存暂时不可用。")?;
        if let Some(paths) = cache.get_mut(&kind) {
            paths.retain(|item| {
                !item
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&target.to_string_lossy())
            });
        }
        save_scan_cache(&cache);
    }
    {
        let mut managed = managed_installs()
            .lock()
            .map_err(|_| "受管安装目录暂时不可用。")?;
        if let Some(paths) = managed.get_mut(&kind) {
            paths.retain(|item| {
                !item
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&target.to_string_lossy())
            });
        }
        save_path_map(&managed_installs_path(), &managed);
    }
    Ok(())
}

// 异步：写注册表后 broadcast_change 有最多 5s 的 SendMessageTimeout，放后台线程免得卡界面。
#[tauri::command]
pub async fn env_set_default(
    kind: String,
    path: String,
    siblings: Vec<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !Path::new(&path).exists() {
            return Err("该路径已不存在（可能被删除/移动），请重新扫描后再设默认".into());
        }
        set_default(winenv::Hive::User, &kind, &path, siblings)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 系统级切换：写请求文件 → 提权重启自身写 HKLM → 等待。
#[tauri::command]
pub fn env_set_default_system(
    kind: String,
    path: String,
    siblings: Vec<String>,
) -> Result<(), String> {
    if !Path::new(&path).exists() {
        return Err("该路径已不存在（可能被删除/移动），请重新扫描后再设默认".into());
    }
    crate::winadmin::set_default_system(&kind, &path, siblings.clone())?;
    clear_user_shadow_for_system(&kind, &siblings)?;
    Ok(())
}

/// 每个 SDK 是否存在系统级配置（用来提示需用系统级切换）。
#[tauri::command]
pub fn env_system_info() -> std::collections::HashMap<String, bool> {
    use winenv::Hive::System;
    let sp = winenv::get_path_in(System);
    let has = |needles: &[&str]| {
        sp.iter().any(|d| {
            let l = d.to_lowercase();
            needles.iter().any(|n| l.contains(n))
        })
    };
    let mut m = std::collections::HashMap::new();
    m.insert(
        "java".into(),
        winenv::get_raw_in(System, "JAVA_HOME").is_some() || has(&["\\jdk", "\\jre", "\\java\\"]),
    );
    m.insert(
        "go".into(),
        winenv::get_raw_in(System, "GOROOT").is_some() || has(&["\\go\\bin"]),
    );
    m.insert("python".into(), has(&["python"]));
    m.insert("node".into(), has(&["nodejs", "\\node\\"]));
    m.insert(
        "maven".into(),
        winenv::get_raw_in(System, "MAVEN_HOME").is_some()
            || winenv::get_raw_in(System, "M2_HOME").is_some()
            || has(&["%maven_home%\\bin", "\\maven\\bin", "\\apache-maven-"]),
    );
    m.insert(
        "gradle".into(),
        winenv::get_raw_in(System, "GRADLE_HOME").is_some()
            || has(&["%gradle_home%\\bin", "\\gradle\\bin", "\\gradle-"]),
    );
    m
}
