//! 磁盘清理：扫描各开发生态的缓存目录占用，分"可安全清理 / 谨慎"，按需删除。
//! 删除只清目录"内容"、保留目录本身（工具会自动重建）；只允许删已知候选路径。

use std::collections::HashSet;
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
    pub category: String, // "safe" | "cautious"
    pub icon: String,
    pub av: String,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}
fn lad() -> PathBuf {
    dirs::data_local_dir().unwrap_or_default()
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

#[tauri::command]
pub fn cleanup_scan() -> Vec<CacheItem> {
    specs()
        .into_iter()
        .filter_map(|s| {
            let p = first_existing(&s.cands)?;
            let size = dir_size(&p);
            if size == 0 {
                return None;
            }
            Some(CacheItem {
                id: s.id.into(),
                name: s.name.into(),
                path: p.to_string_lossy().into_owned(),
                size,
                category: s.category.into(),
                icon: s.icon.into(),
                av: s.av.into(),
            })
        })
        .collect()
}

// 删除一批已知候选目录的内容（保留目录本身）。返回释放字节数。
fn delete_known_paths(paths: Vec<String>) -> Result<u64, String> {
    let known: HashSet<String> = specs()
        .iter()
        .flat_map(|s| s.cands.iter().map(|p| p.to_string_lossy().into_owned()))
        .collect();
    let mut freed = 0u64;
    for p in paths {
        if !known.contains(&p) {
            return Err(format!("拒绝删除未知路径：{p}"));
        }
        let path = PathBuf::from(&p);
        if !path.exists() {
            continue;
        }
        freed += dir_size(&path);
        if let Ok(rd) = fs::read_dir(&path) {
            for entry in rd.flatten() {
                let ep = entry.path();
                let _ = if ep.is_dir() {
                    fs::remove_dir_all(&ep)
                } else {
                    fs::remove_file(&ep)
                };
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
    specs()
        .iter()
        .any(|s| s.cands.iter().any(|p| p.to_string_lossy() == path))
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
