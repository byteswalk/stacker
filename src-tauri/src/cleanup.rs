//! Legacy cleanup commands backed by the centralized known-space scanner.
//! Deletion only accepts freshly discovered known candidates.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::space_analysis::known::{known_candidates, scan_known_candidates, CleanupKind};
use crate::space_analysis::model::KnownSpaceItem;
use crate::space_analysis::walker::CancellationToken;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct CacheItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,        // bytes
    pub category: String, // "safe" | "cautious" | "history" | "temp"
    pub icon: String,
    pub av: String,
}

impl From<KnownSpaceItem> for CacheItem {
    fn from(item: KnownSpaceItem) -> Self {
        let (icon, av) = legacy_visuals(&item);
        let name = legacy_name(&item);
        let category = legacy_category(&item);
        Self {
            id: item.id.clone(),
            name,
            path: item.path,
            size: item.bytes,
            category,
            icon: icon.into(),
            av: av.into(),
        }
    }
}

fn legacy_name(item: &KnownSpaceItem) -> String {
    if let Some(name) = item.id.strip_prefix("jetbrains-history:") {
        let directory = name.split_once(':').map(|(_, name)| name).unwrap_or(name);
        return format!("JetBrains 历史版本 · {directory}");
    }

    match item.id.as_str() {
        "gradle" => "Gradle caches",
        "gomod" => "Go 模块缓存",
        "pnpm" => "pnpm store",
        "npm" => "npm 缓存",
        "cargo" => "Cargo registry 缓存",
        "pip" => "pip 缓存",
        "electron" => "Electron 下载缓存",
        "playwright" => "Playwright 浏览器",
        "hf" => "HuggingFace 模型缓存",
        "m2repo" => "Maven 本地仓库",
        "windows-temp" => "Windows 临时目录",
        "user-temp" => "用户临时目录",
        _ => item.name_key.as_str(),
    }
    .into()
}

fn legacy_category(item: &KnownSpaceItem) -> String {
    if item.id.starts_with("jetbrains-history:") {
        "history"
    } else if matches!(item.id.as_str(), "windows-temp" | "user-temp") {
        "temp"
    } else if item.safety == "safe" {
        "safe"
    } else {
        "cautious"
    }
    .into()
}

fn legacy_visuals(item: &KnownSpaceItem) -> (&'static str, &'static str) {
    match item.id.as_str() {
        "gradle" => ("ti-box", "gr"),
        "gomod" => ("ti-brand-golang", "go"),
        "pnpm" | "npm" => ("ti-brand-npm", "npm"),
        "cargo" => ("ti-brand-rust", "rs"),
        "pip" => ("ti-brand-python", "py"),
        "electron" => ("ti-bolt", "el"),
        "playwright" => ("ti-theater", "el"),
        "hf" => ("ti-robot", "hf"),
        "m2repo" => ("ti-feather", "mv2"),
        id if id.starts_with("jetbrains-history:") => ("ti-code", "st"),
        "windows-temp" | "user-temp" => ("ti-trash", "st"),
        _ => ("ti-box", "st"),
    }
}

fn is_link_or_reparse_point(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

#[tauri::command]
pub fn cleanup_scan() -> Vec<CacheItem> {
    scan_known_candidates(&CancellationToken::default(), |_| {})
        .map(|result| result.items.into_iter().map(CacheItem::from).collect())
        .unwrap_or_default()
}

fn delete_contents(path: &Path) -> u64 {
    let mut freed = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            // Never recurse through links or Windows junctions beyond the validated root.
            if is_link_or_reparse_point(&entry_path) {
                continue;
            }
            if entry_path.is_dir() {
                freed += delete_contents(&entry_path);
                let _ = fs::remove_dir(&entry_path);
            } else if let Ok(metadata) = entry_path.metadata() {
                let bytes = metadata.len();
                if fs::remove_file(&entry_path).is_ok() {
                    freed += bytes;
                }
            }
        }
    }
    freed
}

#[derive(Clone)]
struct FreshKnownPath {
    cleanup_kind: CleanupKind,
    canonical_path: PathBuf,
}

fn fresh_known_paths() -> HashMap<String, FreshKnownPath> {
    known_candidates()
        .into_iter()
        .filter_map(|candidate| {
            let canonical_path = fs::canonicalize(&candidate.path).ok()?;
            Some((
                candidate.path.to_string_lossy().into_owned(),
                FreshKnownPath {
                    cleanup_kind: candidate.cleanup_kind,
                    canonical_path,
                },
            ))
        })
        .collect()
}

// Delete only paths found by a fresh rule lookup; caller-provided item metadata is irrelevant.
fn delete_known_paths(paths: Vec<String>) -> Result<u64, String> {
    let known = fresh_known_paths();
    let mut freed = 0u64;
    for path_string in paths {
        let Some(known_path) = known.get(&path_string) else {
            return Err(format!("拒绝删除未知路径：{path_string}"));
        };
        if known_path.cleanup_kind == CleanupKind::None {
            return Err(format!("该路径不可清理：{path_string}"));
        }

        let path = PathBuf::from(&path_string);
        if !path.exists() {
            continue;
        }
        if is_link_or_reparse_point(&path) {
            return Err(format!("拒绝清理链接或目录联接：{path_string}"));
        }
        let canonical_path =
            fs::canonicalize(&path).map_err(|_| format!("无法验证清理路径：{path_string}"))?;
        if canonical_path != known_path.canonical_path {
            return Err(format!("清理路径在验证后发生变化：{path_string}"));
        }

        match known_path.cleanup_kind {
            CleanupKind::Contents => freed += delete_contents(&path),
            CleanupKind::WholeDirectory => {
                freed += delete_contents(&path);
                let _ = fs::remove_dir(&path);
            }
            CleanupKind::None => unreachable!(),
        }
    }
    Ok(freed)
}

/// 删除选中缓存（清目录内容、保留目录）。仅允许已知候选路径。返回释放字节数。
/// 异步：多 GB 删除放后台线程，避免阻塞主线程让界面卡死。
#[tauri::command]
pub async fn cleanup_delete(paths: Vec<String>) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || delete_known_paths(paths))
        .await
        .map_err(|error| error.to_string())?
}

/// 一键清理所有「可安全清理」缓存：重新扫描并在删除前再次验证候选路径。
#[tauri::command]
pub async fn cleanup_delete_safe() -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let paths = scan_known_candidates(&CancellationToken::default(), |_| {})
            .map_err(|error| error.to_string())?
            .items
            .into_iter()
            .filter(|item| item.safety == "safe")
            .map(|item| item.path)
            .collect();
        delete_known_paths(paths)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn is_known(path: &str) -> bool {
    known_candidates()
        .into_iter()
        .any(|candidate| candidate.path.to_string_lossy() == path)
}

#[derive(Serialize)]
pub struct AgedStats {
    pub count: u64,
    pub size: u64,
}

/// 统计 path 下"超过 days 天没被访问过"的文件数与总大小（用于谨慎项智能清理）。
#[tauri::command]
pub fn cleanup_aged_stats(path: String, days: u64) -> Result<AgedStats, String> {
    if !is_known(&path) {
        return Err(format!("未知路径：{path}"));
    }
    if is_link_or_reparse_point(Path::new(&path)) {
        return Err(format!("拒绝扫描链接或目录联接：{path}"));
    }
    let older = Duration::from_secs(days * 86400);
    let now = SystemTime::now();
    let (mut count, mut size) = (0u64, 0u64);
    for entry in jwalk::WalkDir::new(&path)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if entry.file_type.is_dir() {
            continue;
        }
        if let Ok(metadata) = entry.path().metadata() {
            let last = metadata
                .accessed()
                .or_else(|_| metadata.modified())
                .unwrap_or(now);
            if now
                .duration_since(last)
                .map(|age| age >= older)
                .unwrap_or(false)
            {
                count += 1;
                size += metadata.len();
            }
        }
    }
    Ok(AgedStats { count, size })
}

/// 删除 path 下超过 days 天未访问的文件，返回释放字节。
#[tauri::command]
pub fn cleanup_delete_aged(path: String, days: u64) -> Result<u64, String> {
    if !is_known(&path) {
        return Err(format!("未知路径：{path}"));
    }
    if is_link_or_reparse_point(Path::new(&path)) {
        return Err(format!("拒绝清理链接或目录联接：{path}"));
    }
    let older = Duration::from_secs(days * 86400);
    let now = SystemTime::now();
    let mut freed = 0u64;
    for entry in jwalk::WalkDir::new(&path)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if entry.file_type.is_dir() {
            continue;
        }
        let entry_path = entry.path();
        if is_link_or_reparse_point(&entry_path) {
            continue;
        }
        if let Ok(metadata) = entry_path.metadata() {
            let last = metadata
                .accessed()
                .or_else(|_| metadata.modified())
                .unwrap_or(now);
            if now
                .duration_since(last)
                .map(|age| age >= older)
                .unwrap_or(false)
            {
                let bytes = metadata.len();
                if fs::remove_file(&entry_path).is_ok() {
                    freed += bytes;
                }
            }
        }
    }
    Ok(freed)
}
