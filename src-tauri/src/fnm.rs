//! Node 版本管理：fnm 接管。检测 / 列版本 / 设默认 / 装卸 / 注入 shell 集成（PS + Git Bash / cmd）。
//! 改 shell profile 前先 backup::backup_file（可在「历史」还原）。

use crate::sources::Mirror;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::Emitter;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// fnm 可执行全路径：按注册表最新 PATH 解析（winget 装好后进程旧 PATH 找不到，故不能直接用 "fnm"）。
fn fnm_exe() -> String {
    crate::env::resolve_fresh("fnm.exe")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fnm".into())
}

/// 跑命令、捕获输出、Windows 下不弹控制台窗口。
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    run_env(program, args, &[])
}

/// 同 run，但可附加环境变量（如 FNM_NODE_DIST_MIRROR，让 Node 安装走镜像）。
fn run_env(program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<String, String> {
    let mut c = std::process::Command::new(program);
    c.args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let out = c
        .output()
        .map_err(|e| format!("{program} 未找到或执行失败：{e}"))?;
    let so = String::from_utf8_lossy(&out.stdout).into_owned();
    let se = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok(so)
    } else {
        Err(if se.trim().is_empty() { so } else { se })
    }
}

const NODE_SPEEDTEST_VERSION: &str = "v24.1.0";

fn node_runtime_mirror(id: &str) -> Option<Mirror> {
    crate::sources::node_runtime_mirrors()
        .into_iter()
        .find(|m| m.id == id)
}

fn node_dist_mirror(source: &str) -> Result<String, String> {
    node_runtime_mirror(source)
        .map(|m| m.url.trim_end_matches('/').to_string())
        .ok_or_else(|| "未知 Node 下载源".into())
}

fn node_index_url(source: &str) -> Result<String, String> {
    Ok(format!("{}/index.json", node_dist_mirror(source)?))
}

#[derive(Deserialize)]
struct NodeIndexEntry {
    version: String,
    #[serde(default)]
    lts: serde_json::Value,
    #[serde(default)]
    files: Vec<String>,
}

fn node_lts(e: &NodeIndexEntry) -> bool {
    !matches!(
        e.lts,
        serde_json::Value::Bool(false) | serde_json::Value::Null
    )
}

fn node_has_windows_zip(e: &NodeIndexEntry) -> bool {
    e.files.iter().any(|f| f == "win-x64-zip") || e.files.is_empty()
}

fn node_versions_from_source(lts_only: bool, source: &str) -> Result<Vec<String>, String> {
    let url = node_index_url(source)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .timeout(Duration::from_secs(30))
        .build();
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| format!("读取 Node 版本列表失败：{e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let entries: Vec<NodeIndexEntry> =
        serde_json::from_str(&body).map_err(|e| format!("解析 Node 版本列表失败：{e}"))?;
    let mut versions: Vec<String> = entries
        .into_iter()
        .filter(|e| !lts_only || node_lts(e))
        .filter(node_has_windows_zip)
        .map(|e| e.version)
        .collect();
    versions.truncate(80);
    Ok(versions)
}

/// 从 `fnm list` 的一行里取出 vX.Y.Z。
fn parse_ver(line: &str) -> Option<String> {
    line.split_whitespace().find_map(|t| {
        let t = t.trim_start_matches('*').trim();
        if t.starts_with('v') && t.as_bytes().get(1).map_or(false, |b| b.is_ascii_digit()) {
            Some(t.to_string())
        } else {
            None
        }
    })
}

fn ps_profiles() -> Vec<PathBuf> {
    let docs = dirs::document_dir().unwrap_or_else(|| home().join("Documents"));
    vec![
        docs.join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
        docs.join("WindowsPowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
    ]
}
fn bashrc() -> PathBuf {
    home().join(".bashrc")
}
fn file_has_fnm(p: &Path) -> bool {
    std::fs::read_to_string(p)
        .map(|s| s.contains("fnm env"))
        .unwrap_or(false)
}

#[cfg(windows)]
fn cmd_autorun_has_fnm() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Command Processor")
        .and_then(|k| k.get_value::<String, _>("AutoRun"))
        .map(|v| v.to_lowercase().contains("fnm"))
        .unwrap_or(false)
}
#[cfg(not(windows))]
fn cmd_autorun_has_fnm() -> bool {
    false
}

#[derive(Serialize)]
pub struct NodeVer {
    pub version: String,
    pub is_default: bool,
    pub path: String,
}
#[derive(Serialize, Default)]
pub struct Shells {
    pub powershell: bool,
    pub gitbash: bool,
    pub cmd: bool,
}
#[derive(Serialize, Default)]
pub struct FnmStatus {
    pub installed: bool,
    pub fnm_version: Option<String>,
    pub versions: Vec<NodeVer>,
    pub default: Option<String>,
    pub shell: Shells,
    pub has_nvm: bool,
}

struct NodeInstallLog {
    path: PathBuf,
}

impl NodeInstallLog {
    fn new(version: &str, source: &str) -> Result<Self, String> {
        let dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Stacker")
            .join("logs");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let clean_version = clean_log_part(version);
        let clean_source = clean_log_part(source);
        let path = dir.join(format!(
            "node-install-session-{clean_version}-{clean_source}-{stamp}.log"
        ));
        let log = Self { path };
        log.line(format!(
            "START node install session version={version} source={source}"
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
}

fn clean_log_part(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

// 异步：内含 fnm 子进程调用，避免阻塞主线程。
#[tauri::command]
pub async fn fnm_status() -> FnmStatus {
    tauri::async_runtime::spawn_blocking(fnm_status_impl)
        .await
        .unwrap_or_default()
}
pub fn fnm_status_impl() -> FnmStatus {
    let exe = fnm_exe();
    let root = fnm_dir();
    let root_env = root.to_string_lossy().into_owned();
    let fnm_env = [("FNM_DIR", root_env.as_str())];
    let fnm_version = run_env(&exe, &["--version"], &fnm_env)
        .ok()
        .map(|s| s.trim().to_string());
    let installed = fnm_version.is_some();
    let mut versions = Vec::new();
    let mut default = None;
    if installed {
        if let Ok(list) = run_env(&exe, &["list"], &fnm_env) {
            for line in list.lines() {
                if let Some(v) = parse_ver(line) {
                    let is_default = line.contains("default");
                    if is_default {
                        default = Some(v.clone());
                    }
                    let path = node_installation_dir_at(&root, &v)
                        .to_string_lossy()
                        .into_owned();
                    versions.push(NodeVer {
                        version: v,
                        is_default,
                        path,
                    });
                }
            }
        }
    }
    FnmStatus {
        installed,
        fnm_version,
        versions,
        default,
        shell: Shells {
            powershell: ps_profiles().iter().any(|p| file_has_fnm(p)),
            gitbash: file_has_fnm(&bashrc()),
            cmd: cmd_autorun_has_fnm(),
        },
        has_nvm: std::env::var("NVM_HOME").is_ok(),
    }
}

#[tauri::command]
pub async fn fnm_set_default(
    version: String,
    scope: Option<String>,
    siblings: Option<Vec<String>>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        set_default_node_version(&version, scope.as_deref(), siblings.unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn set_default_node_version(
    version: &str,
    scope: Option<&str>,
    siblings: Vec<String>,
) -> Result<(), String> {
    let root = fnm_dir();
    set_default_node_version_at(&root, version, scope, siblings)
}

fn set_default_node_version_at(
    root: &Path,
    version: &str,
    scope: Option<&str>,
    siblings: Vec<String>,
) -> Result<(), String> {
    let root_env = root.to_string_lossy().into_owned();
    run_env(
        &fnm_exe(),
        &["default", version],
        &[("FNM_DIR", root_env.as_str())],
    )?;
    let node_dir = node_installation_dir_at(root, version);
    if !node_dir.join("node.exe").is_file() {
        return Err(format!(
            "Node {version} 的安装目录不完整，请重新安装该版本后再设为默认。目录：{}",
            node_dir.display()
        ));
    }
    match scope {
        Some("user") => crate::env::set_default(
            crate::winenv::Hive::User,
            "node",
            &node_dir.to_string_lossy(),
            siblings,
        )?,
        Some("system") => {
            crate::winadmin::set_default_system("node", &node_dir.to_string_lossy(), siblings)?
        }
        Some(other) => return Err(format!("未知默认范围：{other}")),
        None => {}
    }
    // cmd 集成是「直写默认版安装目录」，换默认后刷新那个 bat（仅当已写过 cmd 集成）
    #[cfg(windows)]
    if cmd_autorun_has_fnm() {
        let _ = write_cmd_autorun();
    }
    Ok(())
}

#[tauri::command]
pub fn fnm_root_dir() -> String {
    fnm_dir().to_string_lossy().into_owned()
}

#[tauri::command]
pub async fn fnm_uninstall_version(version: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || uninstall_version_impl(&version))
        .await
        .map_err(|e| e.to_string())?
}

fn current_default_version() -> Option<String> {
    let root = fnm_dir();
    let root_env = root.to_string_lossy().into_owned();
    let list = run_env(&fnm_exe(), &["list"], &[("FNM_DIR", root_env.as_str())]).ok()?;
    list.lines()
        .find(|l| l.contains("default"))
        .and_then(parse_ver)
}

#[cfg(windows)]
fn clear_default_alias() -> Result<(), String> {
    let alias = fnm_dir().join("aliases").join("default");
    if std::fs::symlink_metadata(&alias).is_err() {
        return Ok(());
    }
    let res = if alias.is_dir() {
        std::fs::remove_dir(&alias)
    } else {
        std::fs::remove_file(&alias)
    };
    res.map_err(|e| format!("清除默认版本入口失败：{e}"))
}
#[cfg(not(windows))]
fn clear_default_alias() -> Result<(), String> {
    Ok(())
}

fn uninstall_error(version: &str, err: String) -> String {
    let e = err.trim();
    let low = e.to_lowercase();
    if e.contains("拒绝访问") || e.contains("os error 5") || low.contains("access is denied") {
        #[cfg(windows)]
        {
            let dir = fnm_dir().join("node-versions").join(version);
            return format!(
                "Node {version} 未能删除：版本目录正在被终端、编辑器或其他进程占用。请关闭正在使用该 Node 的窗口后重试。目录：{}",
                dir.to_string_lossy()
            );
        }
    }
    format!("Node {version} 卸载失败：{e}")
}

fn uninstall_version_impl(version: &str) -> Result<(), String> {
    let was_default = current_default_version()
        .as_deref()
        .map(|v| v.eq_ignore_ascii_case(version))
        .unwrap_or(false);
    if was_default {
        clear_default_alias()
            .map_err(|e| format!("{e}。请关闭正在使用 Node 的终端或编辑器后重试。"))?;
    }
    let root = fnm_dir();
    let root_env = root.to_string_lossy().into_owned();
    run_env(
        &fnm_exe(),
        &["uninstall", version],
        &[("FNM_DIR", root_env.as_str())],
    )
    .map(|_| ())
    .map_err(|e| uninstall_error(version, e))?;
    if was_default {
        let _ = clear_default_alias();
        #[cfg(windows)]
        if cmd_autorun_has_fnm() {
            let _ = write_cmd_autorun();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn fnm_install_version(
    window: tauri::Window,
    version: String,
    source: Option<String>,
    set_default: Option<bool>,
    scope: Option<String>,
    siblings: Option<Vec<String>>,
    install_root: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        install_version_impl(
            &window,
            &version,
            source,
            set_default,
            scope,
            siblings.unwrap_or_default(),
            install_root,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
fn install_version_impl(
    window: &tauri::Window,
    version: &str,
    source: Option<String>,
    set_default: Option<bool>,
    scope: Option<String>,
    siblings: Vec<String>,
    install_root: Option<String>,
) -> Result<String, String> {
    let exe = fnm_exe();
    let source = source.unwrap_or_else(|| "official".into());
    let mirror = node_dist_mirror(&source)?;
    let log = NodeInstallLog::new(version, &source)?;
    let root = install_root
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(fnm_dir);
    let root_env = root.to_string_lossy().into_owned();
    let fnm_env = [("FNM_DIR", root_env.as_str())];
    if install_root
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
    {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        crate::backup::backup_env(crate::winenv::Hive::User, "fnm-dir", &["FNM_DIR"]);
        crate::winenv::set_user("FNM_DIR", &root_env)?;
    }
    log.line(format!("fnm_exe={exe}"));
    log.line(format!("node_dist_mirror={mirror}"));
    log.line(format!("fnm_root={}", root.display()));
    #[cfg(windows)]
    {
        log.line(format!(
            "FNM_DIR process={:?} user={:?} system={:?}",
            std::env::var("FNM_DIR").ok(),
            crate::winenv::get_raw_in(crate::winenv::Hive::User, "FNM_DIR"),
            crate::winenv::get_raw_in(crate::winenv::Hive::System, "FNM_DIR")
        ));
        log.line(format!("resolved_fnm_dir={}", root.display()));
        log.line(format!(
            "target_version_dir={}",
            node_version_dir_at(&root, version).display()
        ));
        log.line(format!(
            "target_installation_dir={}",
            node_installation_dir_at(&root, version).display()
        ));
        log.line(format!(
            "default_alias={}",
            root.join("aliases").join("default").display()
        ));
    }
    match run_env(&exe, &["list"], &fnm_env) {
        Ok(list) => log.line(format!("fnm list before\n{}", list.trim())),
        Err(e) => log.line(format!("fnm list before failed err={e}")),
    }
    // 安装前是否已有任何版本：只有「一个都没装过」才把这次的设为默认（否则不动用户已选的默认）
    let had_any = run_env(&exe, &["list"], &fnm_env)
        .map(|s| s.lines().any(|l| parse_ver(l).is_some()))
        .unwrap_or(false);
    let install_result = install_node_runtime_zip_at(window, &root, version, &mirror, &log);
    if let Err(e) = install_result {
        log_node_version_dir_snapshot_at(&log, &root, version, "after install failure");
        match run_env(&exe, &["list"], &fnm_env) {
            Ok(list) => log.line(format!("fnm list after failure\n{}", list.trim())),
            Err(err) => log.line(format!("fnm list after failure failed err={err}")),
        }
        return Err(log.error(format!("Node {version} 安装失败：{e}")));
    }
    if set_default.unwrap_or(!had_any) {
        let mut all_siblings = siblings;
        let install_path = node_installation_dir_at(&root, version)
            .to_string_lossy()
            .into_owned();
        if !all_siblings.iter().any(|s| s == &install_path) {
            all_siblings.push(install_path);
        }
        match set_default_node_version_at(&root, version, scope.as_deref(), all_siblings) {
            Ok(()) => log.line("fnm default ok"),
            Err(e) => {
                return Err(log.error(format!("Node {version} 已安装，但设置默认版本失败：{e}")))
            }
        }
    }
    match run_env(&exe, &["list"], &fnm_env) {
        Ok(list) => log.line(format!("fnm list after success\n{}", list.trim())),
        Err(e) => log.line(format!("fnm list after success failed err={e}")),
    }
    log.line("DONE node install session");
    Ok(version.to_string())
}

fn node_zip_url(mirror: &str, version: &str) -> String {
    format!(
        "{}/{}/node-{}-win-x64.zip",
        mirror.trim_end_matches('/'),
        version,
        version
    )
}

#[cfg(windows)]
fn node_version_dir_at(root: &Path, version: &str) -> PathBuf {
    root.join("node-versions").join(version)
}

#[cfg(windows)]
fn node_installation_dir_at(root: &Path, version: &str) -> PathBuf {
    node_version_dir_at(root, version).join("installation")
}

#[cfg(windows)]
fn log_node_version_dir_snapshot_at(log: &NodeInstallLog, root: &Path, version: &str, stage: &str) {
    let version_dir = node_version_dir_at(root, version);
    let install_dir = node_installation_dir_at(root, version);
    log.line(format!(
        "node dir snapshot stage={stage} version_dir={} exists={} installation={} installation_exists={}",
        version_dir.display(),
        version_dir.exists(),
        install_dir.display(),
        install_dir.exists()
    ));
    for rel in [
        "installation\\node.exe",
        "installation\\npm.cmd",
        "installation\\npx.cmd",
        "installation\\node_modules\\npm\\package.json",
    ] {
        let path = version_dir.join(rel);
        log.line(format!(
            "node key stage={stage} rel={rel} exists={} size={}",
            path.is_file(),
            path.metadata().map(|m| m.len()).unwrap_or(0)
        ));
    }
    if let Ok(entries) = std::fs::read_dir(&version_dir) {
        let names: Vec<String> = entries
            .flatten()
            .take(30)
            .map(|ent| {
                let ty = ent
                    .file_type()
                    .map(|ft| if ft.is_dir() { "dir" } else { "file" })
                    .unwrap_or("unknown");
                format!("{}:{ty}", ent.file_name().to_string_lossy())
            })
            .collect();
        log.line(format!(
            "node top stage={stage} entries={}",
            names.join(", ")
        ));
    }
}

#[cfg(windows)]
fn prepare_node_installation_dir_at(
    root: &Path,
    version: &str,
    log: &NodeInstallLog,
) -> Result<PathBuf, String> {
    let version_dir = node_version_dir_at(root, version);
    let install_dir = node_installation_dir_at(root, version);
    log_node_version_dir_snapshot_at(log, root, version, "before prepare");
    if install_dir.join("node.exe").is_file() && install_dir.join("npm.cmd").is_file() {
        log.line(format!(
            "node installation already ready dir={}",
            install_dir.display()
        ));
        return Ok(install_dir);
    }
    if version_dir.exists() {
        log.line(format!(
            "remove partial node version dir path={}",
            version_dir.display()
        ));
        std::fs::remove_dir_all(&version_dir).map_err(|e| {
            format!(
                "清理未完成的 Node 安装目录失败：{}。请关闭正在使用该版本的终端或编辑器后重试。",
                e
            )
        })?;
    }
    std::fs::create_dir_all(&install_dir).map_err(|e| {
        format!(
            "创建 Node 安装目录失败：{}。目录：{}",
            e,
            install_dir.display()
        )
    })?;
    Ok(install_dir)
}

fn install_node_runtime_zip_at(
    window: &tauri::Window,
    root: &Path,
    version: &str,
    mirror: &str,
    log: &NodeInstallLog,
) -> Result<(), String> {
    let install_dir = prepare_node_installation_dir_at(root, version, log)?;
    let url = node_zip_url(mirror, version);
    log.line(format!(
        "node zip install start url={url} dest={}",
        install_dir.display()
    ));
    crate::installer::download_impl_candidates(
        window.clone(),
        vec![url.clone()],
        install_dir.to_string_lossy().into_owned(),
        true,
    )
    .map_err(|e| {
        log.line(format!("node zip install failed url={url} err={e}"));
        e
    })?;
    log_node_version_dir_snapshot_at(log, root, version, "after zip extract");
    if install_dir.join("node.exe").is_file() && install_dir.join("npm.cmd").is_file() {
        log.line("node zip install ready");
        Ok(())
    } else {
        Err(format!(
            "Node 安装文件已下载并解压，但没有检测到 node.exe/npm.cmd。目录：{}",
            install_dir.display()
        ))
    }
}

// ── 从 nvm 迁移已装版本到 fnm（本地复制，免重新下载）──
#[derive(Serialize)]
pub struct MigrateResult {
    pub migrated: Vec<String>,
    pub skipped: Vec<String>,
}

#[cfg(windows)]
fn nvm_home() -> Option<PathBuf> {
    crate::winenv::get_raw_in(crate::winenv::Hive::User, "NVM_HOME")
        .or_else(|| crate::winenv::get_raw_in(crate::winenv::Hive::System, "NVM_HOME"))
        .or_else(|| std::env::var("NVM_HOME").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

#[cfg(windows)]
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
                std::fs::copy(&p, &dest).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn migrate_impl(window: &tauri::Window) -> Result<MigrateResult, String> {
    use tauri::Emitter;
    crate::installer::op_reset();
    let nvm = nvm_home().ok_or("未检测到 nvm（NVM_HOME 未设置）")?;
    // nvm 各版本目录：<NVM_HOME>\vX.Y.Z\node.exe
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for ent in std::fs::read_dir(&nvm)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let p = ent.path();
        if !p.is_dir() || !p.join("node.exe").is_file() {
            continue;
        }
        let ver = ent
            .file_name()
            .to_string_lossy()
            .trim_start_matches('v')
            .to_string();
        let parts: Vec<&str> = ver.split('.').collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        {
            found.push((ver, p));
        }
    }
    if found.is_empty() {
        return Err("nvm 里没有找到已安装的 Node 版本".into());
    }

    let nv_root = fnm_dir().join("node-versions");
    let (mut migrated, mut skipped) = (Vec::new(), Vec::new());
    for (ver, src) in &found {
        if crate::installer::op_cancelled() {
            return Err("已取消".into());
        }
        let dest = nv_root.join(format!("v{ver}")).join("installation");
        if dest.join("node.exe").is_file() {
            skipped.push(ver.clone());
            continue;
        } // fnm 已有
        let _ = window.emit("install-progress", format!("复制 v{ver}（本地，免下载）…"));
        if let Err(e) = copy_dir_all(src, &dest) {
            // 复制中途失败/取消：清掉残缺目录，避免 fnm 看到半成品
            let _ = std::fs::remove_dir_all(nv_root.join(format!("v{ver}")));
            return Err(e);
        }
        migrated.push(ver.clone());
    }
    Ok(MigrateResult { migrated, skipped })
}

/// 从 nvm 迁移已装 Node 版本到 fnm：本地复制 <NVM_HOME>\vX → fnm node-versions，免重新下载。
#[tauri::command]
pub async fn fnm_migrate_from_nvm(window: tauri::Window) -> Result<MigrateResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        {
            migrate_impl(&window)
        }
        #[cfg(not(windows))]
        {
            let _ = &window;
            Err("仅 Windows".to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn fnm_ls_remote(lts_only: bool, source: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "official".into());
        node_versions_from_source(lts_only, &source)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct NodeSourcePing {
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

fn remaining(start: Instant, total: Duration) -> Option<Duration> {
    total.checked_sub(start.elapsed())
}

fn quick_get(url: &str, timeout: Duration) -> Result<(), String> {
    let agent = agent_with_timeout(timeout);
    agent
        .get(url)
        .set("Range", "bytes=0-1023")
        .call()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn quick_head(url: &str, timeout: Duration) -> Result<(), String> {
    let agent = agent_with_timeout(timeout);
    agent
        .head(url)
        .call()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn speedtest_node_source(spec: Mirror) -> Option<u64> {
    let total = Duration::from_millis(1500);
    let start = Instant::now();
    let index = format!("{}/index.json", spec.url.trim_end_matches('/'));
    quick_get(&index, remaining(start, total)?).ok()?;
    let zip = format!(
        "{}/{}/node-{}-win-x64.zip",
        spec.url.trim_end_matches('/'),
        NODE_SPEEDTEST_VERSION,
        NODE_SPEEDTEST_VERSION
    );
    quick_head(&zip, remaining(start, total)?).ok()?;
    Some(start.elapsed().as_millis() as u64)
}

#[tauri::command]
pub async fn fnm_speedtest_sources(window: tauri::Window) -> Vec<NodeSourcePing> {
    tauri::async_runtime::spawn_blocking(move || {
        let sources = crate::sources::node_runtime_mirrors();
        let total = sources.len();
        let mut jobs = Vec::new();
        for spec in sources {
            let win = window.clone();
            jobs.push(std::thread::spawn(move || {
                let ms = speedtest_node_source(spec.clone());
                let _ = win.emit("node-source-speed-progress", spec.name.clone());
                NodeSourcePing {
                    id: spec.id,
                    name: spec.name,
                    ms,
                }
            }));
        }
        let mut out = Vec::new();
        for job in jobs {
            if let Ok(row) = job.join() {
                out.push(row);
            }
        }
        out.sort_by_key(|r| match r.ms {
            Some(ms) => (0, ms),
            None => (1, u64::MAX),
        });
        let _ = window.emit("node-source-speed-progress", "__done__".to_string());
        let _ = total;
        out
    })
    .await
    .unwrap_or_default()
}

// 集成块：fnm env 之后「显式激活默认版」——fnm 的 default 别名走二级 junction，部分 Windows
// 解析不了（fnm use default 直接报错），故先 fnm ls 解析出默认的具体版本号再 fnm use，单级 junction 可靠。
const SENTINEL: &str = "# >>> Stacker fnm >>>";
const SENTINEL_END: &str = "# <<< Stacker fnm <<<";
const PS_BLOCK: &str = "# >>> Stacker fnm >>>\nif (Get-Command fnm -ErrorAction SilentlyContinue) {\n  fnm env --use-on-cd | Out-String | Invoke-Expression\n  $__fnmDefault = (& fnm ls 2>$null | Select-String '\\bdefault\\b' | Select-Object -First 1 | ForEach-Object { if ($_ -match 'v\\d+\\.\\d+\\.\\d+') { $Matches[0] } })\n  if ($__fnmDefault) { & fnm use $__fnmDefault 2>$null | Out-Null }\n}\n# <<< Stacker fnm <<<";
const BASH_BLOCK: &str = "# >>> Stacker fnm >>>\nif command -v fnm >/dev/null 2>&1; then\n  eval \"$(fnm env --use-on-cd)\"\n  __fnm_default=$(fnm ls 2>/dev/null | grep default | grep -oE 'v[0-9]+\\.[0-9]+\\.[0-9]+' | head -1)\n  [ -n \"$__fnm_default\" ] && fnm use \"$__fnm_default\" >/dev/null 2>&1\nfi\n# <<< Stacker fnm <<<";

/// 写入/更新集成块：先删旧的（含旧版单行写法），再追加新块。
fn write_block(path: &Path, block: &str) -> Result<(), String> {
    if path.exists() {
        crate::backup::backup_file(path);
    } else if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    // 删掉旧的 Stacker/fnm 行（哨兵块内 + 旧版散行）
    let mut out = String::new();
    let mut in_block = false;
    for line in existing.lines() {
        let t = line.trim();
        if t == SENTINEL {
            in_block = true;
            continue;
        }
        if t == SENTINEL_END {
            in_block = false;
            continue;
        }
        if in_block {
            continue;
        }
        if t.contains("Stacker")
            || t.contains("fnm env")
            || t.contains("fnm use")
            || t.contains("__fnm")
            || t.contains("fnm ls")
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    let mut content = out.trim_end().to_string();
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str(block);
    content.push('\n');
    std::fs::write(path, content).map_err(|e| e.to_string())
}

// fnm 的版本目录（FNM_DIR，未设时 Windows 默认 %APPDATA%\fnm）。
#[cfg(windows)]
fn fnm_dir() -> PathBuf {
    if let Ok(d) = std::env::var("FNM_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    if let Some(d) = crate::winenv::get_raw_in(crate::winenv::Hive::User, "FNM_DIR")
        .or_else(|| crate::winenv::get_raw_in(crate::winenv::Hive::System, "FNM_DIR"))
        .filter(|d| !d.trim().is_empty())
    {
        return PathBuf::from(d);
    }
    dirs::data_dir()
        .map(|d| d.join("fnm"))
        .unwrap_or_else(|| home().join("AppData").join("Roaming").join("fnm"))
}
// 解析「默认版」的真实安装目录：<FNM_DIR>\node-versions\<vX.Y.Z>\installation。
// 直接指向安装目录、绕开 aliases\default junction（本机环境下该 junction 解析不了 → cmd 拿不到 fnm 的 node）。
#[cfg(windows)]
pub fn default_node_dir() -> Option<PathBuf> {
    let root = fnm_dir();
    let root_env = root.to_string_lossy().into_owned();
    let list = run_env(&fnm_exe(), &["list"], &[("FNM_DIR", root_env.as_str())]).ok()?;
    let default = list
        .lines()
        .find(|l| l.contains("default"))
        .and_then(parse_ver)?;
    let dir = node_installation_dir_at(&root, &default);
    if dir.join("node.exe").is_file() {
        Some(dir)
    } else {
        None
    }
}
// cmd 兜底批处理内容：把默认 Node 安装目录前插 PATH（绝不起子进程 → 不会触发 AutoRun 递归）。
#[cfg(windows)]
fn cmd_bat_content() -> String {
    let header = "@echo off\r\nrem Stacker: fnm cmd 兜底 —— 把 fnm 默认 Node 前插 PATH（直写安装目录，避开 default junction）\r\n";
    match default_node_dir() {
        Some(dir) => format!("{header}if exist \"{0}\\node.exe\" set \"PATH={0};%PATH%\"\r\n", dir.to_string_lossy()),
        // 还没有默认版/解析失败时退回 aliases\default（单级 junction）
        None => format!("{header}if exist \"%APPDATA%\\fnm\\aliases\\default\\node.exe\" set \"PATH=%APPDATA%\\fnm\\aliases\\default;%PATH%\"\r\n"),
    }
}

#[cfg(windows)]
fn write_cmd_autorun() -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let dir = dirs::config_dir().unwrap_or_default().join("stacker");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bat = dir.join("fnm-cmd.cmd");
    std::fs::write(&bat, cmd_bat_content()).map_err(|e| e.to_string())?;
    let call = format!("\"{}\"", bat.to_string_lossy());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("Software\\Microsoft\\Command Processor")
        .map_err(|e| e.to_string())?;
    let existing: String = key.get_value("AutoRun").unwrap_or_default();
    if existing.to_lowercase().contains("fnm-cmd") {
        return Ok(());
    }
    let newval = if existing.trim().is_empty() {
        call
    } else {
        format!("{existing}&{call}")
    };
    crate::backup::backup_user_reg_value(
        "fnm-cmd-autorun",
        "Software\\Microsoft\\Command Processor",
        "AutoRun",
    );
    key.set_value("AutoRun", &newval).map_err(|e| e.to_string())
}
#[cfg(not(windows))]
fn write_cmd_autorun() -> Result<(), String> {
    Ok(())
}

// 把 CurrentUser 执行策略设为 RemoteSigned——否则 Restricted 下 PowerShell profile（我们的钩子）跑不了。
#[cfg(windows)]
fn ensure_ps_execution_policy() {
    // 注册表设 PS5.1 的 CurrentUser 策略（Get-ExecutionPolicy -Scope CurrentUser 读这里）
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((k, _)) =
        hkcu.create_subkey("Software\\Microsoft\\PowerShell\\1\\ShellIds\\Microsoft.PowerShell")
    {
        let cur: String = k.get_value("ExecutionPolicy").unwrap_or_default();
        let c = cur.to_lowercase();
        if cur.trim().is_empty() || c == "restricted" || c == "allsigned" || c == "undefined" {
            crate::backup::backup_user_reg_value(
                "powershell-policy",
                "Software\\Microsoft\\PowerShell\\1\\ShellIds\\Microsoft.PowerShell",
                "ExecutionPolicy",
            );
            let _ = k.set_value("ExecutionPolicy", &"RemoteSigned".to_string());
        }
    }
    // PS7（pwsh）若装了，用 -Command 设（cmdlet 不受脚本策略限制）
    if let Some(pwsh) = crate::env::resolve_fresh("pwsh.exe") {
        let mut c = std::process::Command::new(pwsh);
        c.args([
            "-NoProfile",
            "-Command",
            "Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force",
        ]);
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
        let _ = c.output();
    }
}
#[cfg(not(windows))]
fn ensure_ps_execution_policy() {}

/// 注入 shell 集成。shells 取 "powershell" / "gitbash" / "cmd"。返回已写入的。
#[tauri::command]
pub fn fnm_write_integration(shells: Vec<String>) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for sh in &shells {
        match sh.as_str() {
            "powershell" => {
                ensure_ps_execution_policy(); // 否则 Restricted 下 profile 跑不了
                                              // 同时写 PS 5.1 与 PS 7 的 profile（用户用哪个都生效），不存在则创建
                for p in ps_profiles() {
                    write_block(&p, PS_BLOCK)?;
                }
                done.push("powershell".to_string());
            }
            "gitbash" => {
                // 本机没装 Git Bash 就别写 .bashrc（界面上它是灰的，集成时也跳过）
                if crate::installer::git_bash().is_some() {
                    write_block(&bashrc(), BASH_BLOCK)?;
                    done.push("gitbash".to_string());
                }
            }
            "cmd" => {
                write_cmd_autorun()?;
                done.push("cmd".to_string());
            }
            _ => {}
        }
    }
    Ok(done)
}

// fnm 装到「工具目录\fnm」（与 JDK/Maven/Go 等一致，整套可随 Stacker 目录一起拷走）。
fn app_fnm_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("fnm")))
        .unwrap_or_else(|| PathBuf::from("fnm"))
}
const FNM_URL: &str = "https://github.com/Schniz/fnm/releases/latest/download/fnm-windows.zip";
const BUNDLED_FNM_VERSION: &str = "1.39.0";
const BUNDLED_FNM_ZIP: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/tools/fnm-v1.39.0-windows.zip"
));
fn fnm_self_candidates() -> Vec<String> {
    vec![FNM_URL.to_string()]
}

/// 检查 Node 版本管理工具自身是否有新版（官方源）。
#[tauri::command]
pub async fn fnm_check_update() -> Result<crate::update::UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let current = run(&fnm_exe(), &["--version"])
            .map(|s| s.replace("fnm", "").trim().to_string())
            .unwrap_or_default();
        let latest = crate::update::github_latest_tag("Schniz/fnm")?;
        let has_update = !current.is_empty() && crate::update::ver_lt(&current, &latest);
        Ok(crate::update::UpdateInfo {
            current,
            latest,
            has_update,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 更新 fnm 自身：重新下最新 fnm.exe 覆盖到工具目录\fnm。
#[tauri::command]
pub async fn fnm_self_update(window: tauri::Window) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = app_fnm_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        crate::installer::download_impl_candidates(
            window,
            fnm_self_candidates(),
            dir.to_string_lossy().into_owned(),
            false,
        )?;
        crate::backup::backup_env(crate::winenv::Hive::User, "fnm", &["FNM_DIR"]);
        crate::winenv::prepend_path_in(crate::winenv::Hive::User, &dir.to_string_lossy())?;
        Ok("Node 版本管理工具已更新到最新版".into())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 安装 Node 版本管理工具：从官方源下载 fnm.exe（带超时）放工具目录\fnm 并加 PATH。
/// 不再走 winget——winget 在干净机上抓源常卡死且无超时，体验差且不可取消。
#[tauri::command]
pub async fn fnm_install_self(window: tauri::Window) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || install_self_impl(window))
        .await
        .map_err(|e| e.to_string())?
}
fn install_self_impl(window: tauri::Window) -> Result<String, String> {
    let dir = app_fnm_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::installer::extract_embedded_zip(
        window.clone(),
        BUNDLED_FNM_ZIP,
        &format!("fnm v{BUNDLED_FNM_VERSION}"),
        dir.to_string_lossy().into_owned(),
        false,
    )?;
    crate::backup::backup_env(crate::winenv::Hive::User, "fnm", &["FNM_DIR"]);
    crate::winenv::prepend_path_in(crate::winenv::Hive::User, &dir.to_string_lossy())?;
    let _ = window.emit("install-progress", "正在检测 fnm 安装状态…".to_string());
    if crate::env::resolve_fresh("fnm.exe").is_some() {
        Ok(format!("已安装内置 fnm v{BUNDLED_FNM_VERSION}"))
    } else {
        Err("Node 版本管理工具已下载，但 PATH 未即时刷新，请重启应用后重试".into())
    }
}
