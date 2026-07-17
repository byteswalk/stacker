//! 磁盘清理：扫描各开发生态的缓存目录占用，分"可安全清理 / 谨慎"，按需删除。
//! 删除只允许处理已知候选路径；缓存默认清目录内容，历史版本目录可整目录删除。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct CacheItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,        // 字节
    pub category: String, // "safe" | "cautious" | "history" | "temp"
    pub icon: String,
    pub av: String,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}
fn lad() -> PathBuf {
    dirs::data_local_dir().unwrap_or_default()
}
fn rad() -> PathBuf {
    dirs::data_dir().unwrap_or_default()
}
fn envp(k: &str) -> Option<PathBuf> {
    std::env::var(k)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
fn first_existing(cands: &[PathBuf]) -> Option<PathBuf> {
    cands.iter().find(|p| p.exists()).cloned()
}

fn dir_size(p: &Path) -> u64 {
    jwalk::WalkDir::new(p)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_type.is_dir())
        .filter_map(|e| e.path().metadata().ok())
        .map(|m| m.len())
        .sum()
}

struct Spec {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    icon: &'static str,
    av: &'static str,
    cands: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
enum DeleteMode {
    Contents,
    WholeDir,
}

struct ScanEntry {
    item: CacheItem,
    mode: DeleteMode,
}

fn specs() -> Vec<Spec> {
    let h = home();
    let l = lad();
    let go_cache = vec![
        envp("GOMODCACHE"),
        envp("GOPATH").map(|g| g.join("pkg").join("mod")),
        Some(h.join("go").join("pkg").join("mod")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let cargo_cache = vec![
        envp("CARGO_HOME").map(|c| c.join("registry").join("cache")),
        Some(h.join(".cargo").join("registry").join("cache")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    vec![
        Spec {
            id: "gradle",
            name: "Gradle caches",
            category: "safe",
            icon: "ti-box",
            av: "gr",
            cands: vec![h.join(".gradle").join("caches")],
        },
        Spec {
            id: "gomod",
            name: "Go 模块缓存",
            category: "safe",
            icon: "ti-brand-golang",
            av: "go",
            cands: go_cache,
        },
        Spec {
            id: "pnpm",
            name: "pnpm store",
            category: "safe",
            icon: "ti-brand-npm",
            av: "npm",
            cands: vec![l.join("pnpm").join("store"), h.join(".pnpm-store")],
        },
        Spec {
            id: "npm",
            name: "npm 缓存",
            category: "safe",
            icon: "ti-brand-npm",
            av: "npm",
            cands: vec![l.join("npm-cache"), h.join(".npm").join("_cacache")],
        },
        Spec {
            id: "cargo",
            name: "Cargo registry 缓存",
            category: "safe",
            icon: "ti-brand-rust",
            av: "rs",
            cands: cargo_cache,
        },
        Spec {
            id: "pip",
            name: "pip 缓存",
            category: "safe",
            icon: "ti-brand-python",
            av: "py",
            cands: vec![l.join("pip").join("Cache")],
        },
        Spec {
            id: "electron",
            name: "Electron 下载缓存",
            category: "safe",
            icon: "ti-bolt",
            av: "el",
            cands: vec![l.join("electron").join("Cache")],
        },
        Spec {
            id: "playwright",
            name: "Playwright 浏览器",
            category: "cautious",
            icon: "ti-theater",
            av: "el",
            cands: vec![l.join("ms-playwright")],
        },
        Spec {
            id: "hf",
            name: "HuggingFace 模型缓存",
            category: "cautious",
            icon: "ti-robot",
            av: "hf",
            cands: vec![h.join(".cache").join("huggingface").join("hub")],
        },
        Spec {
            id: "m2repo",
            name: "Maven 本地仓库",
            category: "cautious",
            icon: "ti-feather",
            av: "mv2",
            cands: vec![h.join(".m2").join("repository")],
        },
    ]
}

fn spec_entries() -> Vec<ScanEntry> {
    specs()
        .into_iter()
        .filter_map(|s| {
            let p = first_existing(&s.cands)?;
            let size = dir_size(&p);
            if size == 0 {
                return None;
            }
            Some(ScanEntry {
                item: CacheItem {
                    id: s.id.into(),
                    name: s.name.into(),
                    path: p.to_string_lossy().into_owned(),
                    size,
                    category: s.category.into(),
                    icon: s.icon.into(),
                    av: s.av.into(),
                },
                mode: DeleteMode::Contents,
            })
        })
        .collect()
}

fn version_key(s: &str) -> Vec<u32> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<u32>().ok())
        .collect()
}

fn split_versioned_dir(name: &str) -> Option<(String, String)> {
    let idx = name.find(|c: char| c.is_ascii_digit())?;
    if idx == 0 || idx >= name.len() {
        return None;
    }
    let product = name[..idx].trim_matches(['-', '_', '.', ' ']).to_string();
    let version = name[idx..].trim().to_string();
    if product.is_empty() || version_key(&version).is_empty() {
        return None;
    }
    Some((product, version))
}

type VersionedDirectory = (PathBuf, Vec<u32>, String, String);

fn jetbrains_history_entries() -> Vec<ScanEntry> {
    let roots = [
        ("Local", lad().join("JetBrains")),
        ("Roaming", rad().join("JetBrains")),
    ];
    let mut out = Vec::new();
    for (scope, root) in roots {
        let Ok(rd) = fs::read_dir(&root) else {
            continue;
        };
        let mut groups: HashMap<String, Vec<VersionedDirectory>> = HashMap::new();
        for ent in rd.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            let Some((product, version)) = split_versioned_dir(&name) else {
                continue;
            };
            groups
                .entry(product)
                .or_default()
                .push((path, version_key(&version), version, name));
        }
        for (product, mut dirs) in groups {
            if dirs.len() <= 1 {
                continue;
            }
            dirs.sort_by(|a, b| a.1.cmp(&b.1).then(a.3.cmp(&b.3)));
            let keep = dirs.last().map(|d| d.3.clone()).unwrap_or_default();
            for (path, _, version, name) in dirs.into_iter().filter(|d| d.3 != keep) {
                let size = dir_size(&path);
                if size == 0 {
                    continue;
                }
                out.push(ScanEntry {
                    item: CacheItem {
                        id: format!("jetbrains-history:{scope}:{name}"),
                        name: format!("JetBrains 历史版本 · {product} {version}"),
                        path: path.to_string_lossy().into_owned(),
                        size,
                        category: "history".into(),
                        icon: "ti-code".into(),
                        av: "st".into(),
                    },
                    mode: DeleteMode::WholeDir,
                });
            }
        }
    }
    out
}

fn temp_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(t) = envp("TEMP").or_else(|| envp("TMP")) {
        out.push(t);
    }
    out.push(lad().join("Temp"));
    if let Some(system_root) = envp("SystemRoot").or_else(|| envp("WINDIR")) {
        out.push(system_root.join("Temp"));
    }
    let mut seen = HashSet::new();
    out.into_iter()
        .filter(|p| seen.insert(p.to_string_lossy().to_ascii_lowercase()))
        .collect()
}

fn temp_entries() -> Vec<ScanEntry> {
    const GB: u64 = 1024 * 1024 * 1024;
    temp_candidates()
        .into_iter()
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let size = dir_size(&p);
            if size <= GB {
                return None;
            }
            let is_system = p
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("\\windows\\temp");
            Some(ScanEntry {
                item: CacheItem {
                    id: if is_system {
                        "windows-temp".into()
                    } else {
                        "user-temp".into()
                    },
                    name: if is_system {
                        "Windows 临时目录".into()
                    } else {
                        "用户临时目录".into()
                    },
                    path: p.to_string_lossy().into_owned(),
                    size,
                    category: "temp".into(),
                    icon: "ti-trash".into(),
                    av: "st".into(),
                },
                mode: DeleteMode::Contents,
            })
        })
        .collect()
}

fn scan_entries() -> Vec<ScanEntry> {
    let mut out = spec_entries();
    out.extend(jetbrains_history_entries());
    out.extend(temp_entries());
    out
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
    scan_entries().into_iter().map(|e| e.item).collect()
}

fn delete_contents(path: &Path) -> u64 {
    let mut freed = 0u64;
    if let Ok(rd) = fs::read_dir(path) {
        for entry in rd.flatten() {
            let ep = entry.path();
            // 缓存目录中可能包含符号链接或 Windows Junction。绝不沿链接递归，
            // 否则一次缓存清理可能越过已确认的根目录。
            if is_link_or_reparse_point(&ep) {
                continue;
            }
            if ep.is_dir() {
                freed += delete_contents(&ep);
                let _ = fs::remove_dir(&ep);
            } else if let Ok(meta) = ep.metadata() {
                let len = meta.len();
                if fs::remove_file(&ep).is_ok() {
                    freed += len;
                }
            }
        }
    }
    freed
}

// 删除一批当前扫描出来的目录。缓存/临时目录只清内容，历史版本目录整目录删除。
fn delete_known_paths(paths: Vec<String>) -> Result<u64, String> {
    let known: HashMap<String, DeleteMode> = scan_entries()
        .into_iter()
        .map(|entry| (entry.item.path, entry.mode))
        .collect();
    let mut freed = 0u64;
    for p in paths {
        let Some(mode) = known.get(&p).copied() else {
            return Err(format!("拒绝删除未知路径：{p}"));
        };
        let path = PathBuf::from(&p);
        if !path.exists() {
            continue;
        }
        if is_link_or_reparse_point(&path) {
            return Err(format!("拒绝清理链接或目录联接：{p}"));
        }
        match mode {
            DeleteMode::Contents => {
                freed += delete_contents(&path);
            }
            DeleteMode::WholeDir => {
                freed += delete_contents(&path);
                let _ = fs::remove_dir(&path);
            }
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
        .map_err(|e| e.to_string())?
}

/// 一键清理所有「可安全清理」(safe) 缓存：扫描 + 删除一气呵成（供概览一键修复用）。
#[tauri::command]
pub async fn cleanup_delete_safe() -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let paths: Vec<String> = cleanup_scan()
            .into_iter()
            .filter(|c| c.category == "safe")
            .map(|c| c.path)
            .collect();
        delete_known_paths(paths)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn is_known(path: &str) -> bool {
    scan_entries()
        .into_iter()
        .any(|entry| entry.item.path == path)
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
    for e in jwalk::WalkDir::new(&path)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.file_type.is_dir() {
            continue;
        }
        if let Ok(m) = e.path().metadata() {
            let last = m.accessed().or_else(|_| m.modified()).unwrap_or(now);
            if now
                .duration_since(last)
                .map(|a| a >= older)
                .unwrap_or(false)
            {
                count += 1;
                size += m.len();
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
    for e in jwalk::WalkDir::new(&path)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.file_type.is_dir() {
            continue;
        }
        let p = e.path();
        if is_link_or_reparse_point(&p) {
            continue;
        }
        if let Ok(m) = p.metadata() {
            let last = m.accessed().or_else(|_| m.modified()).unwrap_or(now);
            if now
                .duration_since(last)
                .map(|a| a >= older)
                .unwrap_or(false)
            {
                let len = m.len();
                if fs::remove_file(&p).is_ok() {
                    freed += len;
                }
            }
        }
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_jetbrains_product_and_version() {
        assert_eq!(
            split_versioned_dir("IntelliJIdea2026.1"),
            Some(("IntelliJIdea".into(), "2026.1".into()))
        );
        assert_eq!(
            split_versioned_dir("AndroidStudio2025.3.4"),
            Some(("AndroidStudio".into(), "2025.3.4".into()))
        );
    }

    #[test]
    fn ignores_directories_without_a_version() {
        assert_eq!(split_versioned_dir("SharedIndex"), None);
        assert_eq!(split_versioned_dir("2026.1"), None);
    }
}
