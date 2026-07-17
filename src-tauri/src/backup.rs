//! 改配置/环境前自动备份到 %APPDATA%\stacker\backups\<sanitized>\...

use std::path::{Path, PathBuf};

use crate::winenv;

pub fn backup_root() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("stacker").join("backups")
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S_%3f").to_string()
}

fn sanitize_raw(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            ':' | '\\' | '/' | '%' => out.push_str(&format!("%{:02X}", ch as u32)),
            _ => out.push(ch),
        }
    }
    out
}

fn sanitize(path: &Path) -> String {
    sanitize_raw(&path.to_string_lossy())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OriginMeta {
    origin: String,
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join(".origin.json")
}

fn write_origin(dir: &Path, origin: &str) {
    let meta = OriginMeta {
        origin: origin.to_string(),
    };
    if let Ok(s) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(meta_path(dir), s);
    }
}

fn read_origin(dir: &Path) -> Option<String> {
    std::fs::read_to_string(meta_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<OriginMeta>(&s).ok())
        .map(|m| m.origin)
}

/// 备份一个文件（若存在）。返回备份文件路径；原文件不存在则返回 None。
pub fn backup_file(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    let dir = backup_root().join(sanitize(path));
    std::fs::create_dir_all(&dir).ok()?;
    write_origin(&dir, &path.to_string_lossy());
    let name = path.file_name()?.to_string_lossy().to_string();
    let dst = dir.join(format!("{name}.{}.bak", stamp()));
    std::fs::copy(path, &dst).ok()?;
    Some(dst)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EnvBackup {
    kind: String,
    hive: String,
    vars: Vec<(String, Option<String>)>,
    path: Vec<String>,
    created: String,
}

fn hive_name(hive: winenv::Hive) -> &'static str {
    match hive {
        winenv::Hive::User => "user",
        winenv::Hive::System => "system",
    }
}

fn hive_from_name(name: &str) -> Result<winenv::Hive, String> {
    match name {
        "user" => Ok(winenv::Hive::User),
        "system" => Ok(winenv::Hive::System),
        _ => Err(format!("未知环境范围：{name}")),
    }
}

/// 备份一个环境切换快照：当前 PATH + 相关 HOME 变量。
pub fn backup_env(hive: winenv::Hive, kind: &str, vars: &[&str]) -> Option<PathBuf> {
    let hive_s = hive_name(hive);
    let origin = format!("env://{hive_s}/{kind}");
    let dir = backup_root().join(sanitize_raw(&origin));
    std::fs::create_dir_all(&dir).ok()?;
    write_origin(&dir, &origin);
    let snap = EnvBackup {
        kind: kind.to_string(),
        hive: hive_s.to_string(),
        vars: vars
            .iter()
            .map(|name| (name.to_string(), winenv::get_raw_in(hive, name)))
            .collect(),
        path: winenv::get_path_in(hive),
        created: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    let dst = dir.join(format!("env-{hive_s}-{kind}.{}.json.bak", stamp()));
    let data = serde_json::to_string_pretty(&snap).ok()?;
    std::fs::write(&dst, data).ok()?;
    Some(dst)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RegistryBackup {
    kind: String,
    hive: String,
    subkey: String,
    value_name: String,
    value: Option<String>,
    created: String,
}

/// 备份 HKCU 下的字符串注册表值。用于 shell 集成这类非 Environment 设置。
#[cfg(windows)]
pub fn backup_user_reg_value(kind: &str, subkey: &str, value_name: &str) -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let origin = format!("reg://hkcu/{}/{}", subkey.replace('\\', "/"), value_name);
    let dir = backup_root().join(sanitize_raw(&origin));
    std::fs::create_dir_all(&dir).ok()?;
    write_origin(&dir, &origin);

    let value = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(subkey)
        .ok()
        .and_then(|key| key.get_value::<String, _>(value_name).ok());
    let snap = RegistryBackup {
        kind: kind.to_string(),
        hive: "hkcu".to_string(),
        subkey: subkey.to_string(),
        value_name: value_name.to_string(),
        value,
        created: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    let safe_name = value_name.replace(['\\', '/', ':'], "_");
    let dst = dir.join(format!("reg-{kind}-{safe_name}.{}.json.bak", stamp()));
    let data = serde_json::to_string_pretty(&snap).ok()?;
    std::fs::write(&dst, data).ok()?;
    Some(dst)
}

#[cfg(not(windows))]
pub fn backup_user_reg_value(_: &str, _: &str, _: &str) -> Option<PathBuf> {
    None
}

#[derive(serde::Serialize, Clone)]
pub struct BackupEntry {
    pub file: String,   // 原文件名
    pub path: String,   // 备份文件完整路径
    pub origin: String, // 原始路径
    pub time: String,   // 显示时间
}

#[derive(serde::Serialize)]
pub struct BackupDetailItem {
    pub label: String,
    pub value: String,
}

#[derive(serde::Serialize)]
pub struct BackupDetail {
    pub kind: String,
    pub title: String,
    pub created: String,
    pub origin: String,
    pub backup_path: String,
    pub restore_note: String,
    pub items: Vec<BackupDetailItem>,
    pub preview: Option<String>,
}

/// 列出所有备份（按时间倒序）。
pub fn list_backups() -> Vec<BackupEntry> {
    let mut out = Vec::new();
    let root = backup_root();
    let Ok(groups) = std::fs::read_dir(&root) else {
        return out;
    };
    for g in groups.flatten() {
        let gp = g.path();
        if !gp.is_dir() {
            continue;
        }
        let origin = read_origin(&gp).unwrap_or_else(|| {
            gp.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .replace('_', "\\")
        });
        if let Ok(files) = std::fs::read_dir(&gp) {
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) == Some("bak") {
                    let fname = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let time = f
                        .metadata()
                        .and_then(|m| m.modified())
                        .map(|t| {
                            let dt: chrono::DateTime<chrono::Local> = t.into();
                            dt.format("%Y-%m-%d %H:%M").to_string()
                        })
                        .unwrap_or_default();
                    out.push(BackupEntry {
                        file: fname,
                        path: p.to_string_lossy().to_string(),
                        origin: origin.clone(),
                        time,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| b.path.cmp(&a.path));
    out
}

fn ensure_backup_path(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    let root = backup_root()
        .canonicalize()
        .map_err(|e| format!("备份目录不存在：{e}"))?;
    let p = p
        .canonicalize()
        .map_err(|e| format!("备份文件不存在：{e}"))?;
    if !p.starts_with(&root) || p.extension().and_then(|e| e.to_str()) != Some("bak") {
        return Err("无效的备份文件路径".into());
    }
    Ok(p)
}

fn detail_item(label: impl Into<String>, value: impl Into<String>) -> BackupDetailItem {
    BackupDetailItem {
        label: label.into(),
        value: value.into(),
    }
}

fn opt_value(v: &Option<String>) -> String {
    v.as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("未设置")
        .to_string()
}

fn read_preview(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() {
        return Some("（空文件）".into());
    }
    let mut text = String::from_utf8_lossy(&bytes).to_string();
    const LIMIT: usize = 6000;
    if text.len() > LIMIT {
        text.truncate(LIMIT);
        text.push_str("\n…（内容较长，已截断预览）");
    }
    Some(text)
}

fn file_equal(a: &Path, b: &Path) -> Option<bool> {
    let left = std::fs::read(a).ok()?;
    let right = std::fs::read(b).ok()?;
    Some(left == right)
}

#[cfg(windows)]
fn current_registry_value(subkey: &str, value_name: &str) -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(subkey)
        .ok()
        .and_then(|key| key.get_value::<String, _>(value_name).ok())
}

#[cfg(not(windows))]
fn current_registry_value(_: &str, _: &str) -> Option<String> {
    None
}

/// 读取备份详情，并尽量对比当前状态，帮助用户判断这次还原会影响什么。
pub fn backup_detail(path: String, origin_hint: String) -> Result<BackupDetail, String> {
    let p = ensure_backup_path(&path)?;
    let origin = p.parent().and_then(read_origin).unwrap_or(origin_hint);
    let file_name = p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let backup_path = p.to_string_lossy().to_string();
    if origin.starts_with("env://") {
        let s = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let snap: EnvBackup =
            serde_json::from_str(&s).map_err(|e| format!("环境备份格式错误：{e}"))?;
        let hive = hive_from_name(&snap.hive)?;
        let mut items = vec![
            detail_item(
                "范围",
                if snap.hive == "system" {
                    "系统级"
                } else {
                    "用户级"
                },
            ),
            detail_item("备份类型", snap.kind.clone()),
        ];
        for (name, before) in &snap.vars {
            let current = winenv::get_raw_in(hive, name);
            items.push(detail_item(
                name,
                format!("备份：{}；当前：{}", opt_value(before), opt_value(&current)),
            ));
        }
        items.push(detail_item(
            "PATH",
            format!(
                "备份：{} 项；当前：{} 项",
                snap.path.len(),
                winenv::get_path_in(hive).len()
            ),
        ));
        return Ok(BackupDetail {
            kind: "env".into(),
            title: file_name,
            created: snap.created,
            origin,
            backup_path,
            restore_note: "还原会把上述变量和 PATH 恢复到备份时的状态；还原前会再次备份当前状态。"
                .into(),
            items,
            preview: None,
        });
    }

    if origin.starts_with("reg://") {
        let s = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let snap: RegistryBackup =
            serde_json::from_str(&s).map_err(|e| format!("注册表备份格式错误：{e}"))?;
        let current = current_registry_value(&snap.subkey, &snap.value_name);
        let items = vec![
            detail_item("范围", "当前用户注册表"),
            detail_item("键", snap.subkey.clone()),
            detail_item("值名", snap.value_name.clone()),
            detail_item(
                "值",
                format!(
                    "备份：{}；当前：{}",
                    opt_value(&snap.value),
                    opt_value(&current)
                ),
            ),
        ];
        return Ok(BackupDetail {
            kind: "registry".into(),
            title: file_name,
            created: snap.created,
            origin,
            backup_path,
            restore_note: "还原会把该注册表值恢复到备份时的状态；还原前会再次备份当前状态。".into(),
            items,
            preview: None,
        });
    }

    let origin_path = PathBuf::from(&origin);
    let backup_size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    let current_state = if origin_path.is_file() {
        match file_equal(&p, &origin_path) {
            Some(true) => "存在，内容与备份一致".to_string(),
            Some(false) => "存在，内容与备份不同".to_string(),
            None => "存在，无法比较内容".to_string(),
        }
    } else {
        "目标文件不存在".to_string()
    };
    let created = std::fs::metadata(&p)
        .and_then(|m| m.modified())
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d %H:%M:%S").to_string()
        })
        .unwrap_or_default();
    Ok(BackupDetail {
        kind: "file".into(),
        title: file_name,
        created,
        origin,
        backup_path,
        restore_note: "还原会用备份文件覆盖目标文件；还原前会再次备份当前目标文件。".into(),
        items: vec![
            detail_item("目标文件", origin_path.to_string_lossy().to_string()),
            detail_item("当前状态", current_state),
            detail_item("备份大小", format!("{:.1} KB", backup_size as f64 / 1024.0)),
        ],
        preview: read_preview(&p),
    })
}

/// 删除单条备份记录。
pub fn delete_backup(path: String) -> Result<(), String> {
    let p = ensure_backup_path(&path)?;
    std::fs::remove_file(&p).map_err(|e| e.to_string())?;
    if let Some(parent) = p.parent() {
        let is_empty = std::fs::read_dir(parent)
            .map(|mut it| it.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = std::fs::remove_dir(parent);
        }
    }
    Ok(())
}

/// 清空全部备份记录。
pub fn clear_backups() -> Result<usize, String> {
    let count = list_backups().len();
    let root = backup_root();
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
    }
    Ok(count)
}

fn restore_env(backup_path: &str) -> Result<(), String> {
    let s = std::fs::read_to_string(backup_path).map_err(|e| e.to_string())?;
    let snap: EnvBackup = serde_json::from_str(&s).map_err(|e| format!("环境备份格式错误：{e}"))?;
    let hive = hive_from_name(&snap.hive)?;
    let var_names: Vec<&str> = snap.vars.iter().map(|(name, _)| name.as_str()).collect();
    backup_env(hive, &snap.kind, &var_names);
    for (name, value) in &snap.vars {
        match value {
            Some(v) => winenv::set_in(hive, name, v)?,
            None => winenv::remove_in(hive, name)?,
        }
    }
    winenv::set_path_in(hive, &snap.path)?;
    Ok(())
}

#[cfg(windows)]
fn restore_registry(backup_path: &str) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let s = std::fs::read_to_string(backup_path).map_err(|e| e.to_string())?;
    let snap: RegistryBackup =
        serde_json::from_str(&s).map_err(|e| format!("注册表备份格式错误：{e}"))?;
    if snap.hive != "hkcu" {
        return Err(format!("暂不支持还原注册表范围：{}", snap.hive));
    }
    backup_user_reg_value(&snap.kind, &snap.subkey, &snap.value_name);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(&snap.subkey)
        .map_err(|e| e.to_string())?;
    match snap.value {
        Some(value) => key
            .set_value(&snap.value_name, &value)
            .map_err(|e| e.to_string())?,
        None => match key.delete_value(&snap.value_name) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        },
    }
    Ok(())
}

#[cfg(not(windows))]
fn restore_registry(_: &str) -> Result<(), String> {
    Err("仅支持 Windows".into())
}

/// 用某个备份还原。文件备份复制回原路径；环境备份恢复对应 PATH/HOME 变量。
pub fn restore(backup_path: &str, origin: &str) -> Result<(), String> {
    if origin.starts_with("env://") {
        return restore_env(backup_path);
    }
    if origin.starts_with("reg://") {
        return restore_registry(backup_path);
    }
    let origin_path = Path::new(origin);
    backup_file(origin_path);
    if let Some(parent) = origin_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(backup_path, origin).map_err(|e| e.to_string())?;
    Ok(())
}
