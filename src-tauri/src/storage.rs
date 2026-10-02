use serde::Serialize;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tauri::Emitter;

use crate::{backup, sources, winenv};
mod maven;

#[derive(Clone, Serialize)]
pub struct StorageLocation {
    pub id: String,
    pub ecosystem: String,
    pub label: String,
    pub description: String,
    pub path: String,
    pub default_path: String,
    pub source: String,
    pub size_bytes: u64,
    pub advanced: bool,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn local() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| home().join("AppData").join("Local"))
}

fn clean_value(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

fn env_value(name: &str) -> Option<String> {
    #[cfg(windows)]
    let value = winenv::get_raw_in(winenv::Hive::User, name)
        .or_else(|| winenv::get_raw_in(winenv::Hive::System, name));
    #[cfg(not(windows))]
    let value = std::env::var(name).ok();
    // Storage settings describe future tool launches, not Stacker's inherited snapshot.
    clean_value(value)
}

fn read_key(path: &Path, key: &str, ini_section: Option<&str>) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let mut in_section = ini_section.is_none();
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(section) = ini_section {
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_section = trimmed[1..trimmed.len() - 1].eq_ignore_ascii_case(section);
                continue;
            }
        }
        if !in_section || trimmed.starts_with(['#', ';']) {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once('=') {
            if name.trim().eq_ignore_ascii_case(key) {
                return clean_value(Some(value.to_string()));
            }
        }
    }
    None
}

fn write_flat_key(path: &Path, key: &str, value: Option<&str>) -> Result<(), String> {
    let raw = read_config(path, "")?;
    let mut output = Vec::new();
    let mut replaced = false;
    for line in raw.lines() {
        if line
            .split_once('=')
            .map(|(name, _)| name.trim().eq_ignore_ascii_case(key))
            .unwrap_or(false)
        {
            if let Some(value) = value {
                output.push(format!("{key}={value}"));
            }
            replaced = true;
        } else {
            output.push(line.to_string());
        }
    }
    if !replaced {
        if let Some(value) = value {
            output.push(format!("{key}={value}"));
        }
    }
    save_config(
        path,
        &if output.is_empty() {
            String::new()
        } else {
            output.join("\n") + "\n"
        },
    )
}

fn write_pip_cache(path: &Path, value: Option<&str>) -> Result<(), String> {
    let raw = read_config(path, "")?;
    let mut lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let mut global_start = None;
    let mut global_end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("[global]") {
            global_start = Some(index);
        } else if global_start.is_some() && trimmed.starts_with('[') && trimmed.ends_with(']') {
            global_end = index;
            break;
        }
    }
    if global_start.is_none() && value.is_some() {
        if !lines.is_empty() && !lines.last().map(|v| v.is_empty()).unwrap_or(false) {
            lines.push(String::new());
        }
        lines.push("[global]".into());
        global_start = Some(lines.len() - 1);
        global_end = lines.len();
    }
    if let Some(start) = global_start {
        let existing = (start + 1..global_end).find(|index| {
            lines[*index]
                .split_once('=')
                .map(|(name, _)| name.trim().eq_ignore_ascii_case("cache-dir"))
                .unwrap_or(false)
        });
        match (existing, value) {
            (Some(index), Some(value)) => lines[index] = format!("cache-dir = {value}"),
            (Some(index), None) => {
                lines.remove(index);
            }
            (None, Some(value)) => lines.insert(start + 1, format!("cache-dir = {value}")),
            (None, None) => {}
        }
    }
    save_config(
        path,
        &if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        },
    )
}

fn maven_local_repository() -> Option<String> {
    let raw = fs::read_to_string(sources::maven_path()).ok()?;
    maven::local_repository(&raw).ok().flatten()
}

fn read_config(path: &Path, default: &str) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default.into()),
        Err(error) => Err(format!(
            "读取配置文件失败，未修改原文件：{}：{error}",
            path.display()
        )),
    }
}

fn save_config(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "配置文件缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    if path.exists() && backup::backup_file(path).is_none() {
        return Err("配置备份失败，未修改原文件".into());
    }
    // Write beside the original so disk-full failures cannot truncate it.
    let mut pending = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    pending
        .write_all(content.as_bytes())
        .map_err(|error| error.to_string())?;
    pending
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    pending.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

fn write_maven_local_repository(value: Option<&str>) -> Result<(), String> {
    let path = sources::maven_path();
    let raw = read_config(&path, "<settings>\n</settings>\n")?;
    let raw =
        maven::update(&raw, value).map_err(|error| format!("Maven settings.xml 无效：{error}"))?;
    save_config(&path, &raw)
}

fn location(
    metadata: [&str; 4],
    path: PathBuf,
    default_path: PathBuf,
    state: (&str, bool),
) -> StorageLocation {
    let [id, ecosystem, label, description] = metadata;
    let (source, advanced) = state;
    StorageLocation {
        id: id.into(),
        ecosystem: ecosystem.into(),
        label: label.into(),
        description: description.into(),
        path: path.to_string_lossy().into_owned(),
        default_path: default_path.to_string_lossy().into_owned(),
        source: source.into(),
        // Directory usage is intentionally not traversed on page entry. Large caches are
        // measured by the dedicated disk analysis workflow instead.
        size_bytes: 0,
        advanced,
    }
}

fn all_locations(ecosystem: Option<&str>) -> Vec<StorageLocation> {
    let home = home();
    let local = local();
    let npmrc = sources::npmrc_path();
    let pip_ini = sources::pip_path();
    let maven_default = home.join(".m2").join("repository");
    let gradle_default = home.join(".gradle");
    let npm_cache_default = local.join("npm-cache");
    let npm_prefix_default = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        .join("npm");
    let pnpm_default = local.join("pnpm").join("store");
    let pip_default = local.join("pip").join("Cache");
    let composer_home_default = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        .join("Composer");
    let composer_cache_default = local.join("Composer");
    let go_path = env_value("GOPATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("go"));
    let go_mod_default = go_path.join("pkg").join("mod");
    let go_build_default = local.join("go-build");
    let cargo_default = home.join(".cargo");
    let rustup_default = home.join(".rustup");

    let maven = maven_local_repository();
    let gradle = env_value("GRADLE_USER_HOME");
    let npm_cache = read_key(&npmrc, "cache", None);
    let npm_prefix = read_key(&npmrc, "prefix", None);
    let pnpm = read_key(&npmrc, "store-dir", None);
    let pip = read_key(&pip_ini, "cache-dir", Some("global"));
    let composer_home = env_value("COMPOSER_HOME");
    let composer_cache = env_value("COMPOSER_CACHE_DIR");
    let go_mod = env_value("GOMODCACHE");
    let hf_home = env_value("HF_HOME");
    let hf_default = home.join(".cache").join("huggingface");
    let go_build = env_value("GOCACHE");
    let cargo = env_value("CARGO_HOME");
    let rustup = env_value("RUSTUP_HOME");

    let wanted = |name: &str| ecosystem.map(|value| value == name).unwrap_or(true);
    let mut result = Vec::new();
    if wanted("maven") {
        result.push(location(
            [
                "maven-local-repository",
                "maven",
                "Maven 本地仓库",
                "依赖、插件与本地安装构件",
            ],
            maven
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| maven_default.clone()),
            maven_default,
            (
                if maven.is_some() {
                    "user_config"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
    }
    if wanted("gradle") {
        result.push(location(
            [
                "gradle-user-home",
                "gradle",
                "Gradle 用户目录",
                "依赖缓存、Wrapper 发行包、守护进程与全局配置",
            ],
            gradle
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| gradle_default.clone()),
            gradle_default,
            (
                if gradle.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
    }
    if wanted("node") {
        result.push(location(
            [
                "npm-cache",
                "node",
                "npm 下载缓存",
                "npm 下载包与元数据缓存",
            ],
            npm_cache
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| npm_cache_default.clone()),
            npm_cache_default,
            (
                if npm_cache.is_some() {
                    "user_config"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
        result.push(location(
            [
                "npm-prefix",
                "node",
                "npm 全局包目录",
                "全局 npm 包与命令入口；更改时会同步用户 PATH",
            ],
            npm_prefix
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| npm_prefix_default.clone()),
            npm_prefix_default,
            (
                if npm_prefix.is_some() {
                    "user_config"
                } else {
                    "tool_default"
                },
                true,
            ),
        ));
        result.push(location(
            ["pnpm-store", "node", "pnpm Store", "pnpm 内容寻址包存储"],
            pnpm.as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| pnpm_default.clone()),
            pnpm_default,
            (
                if pnpm.is_some() {
                    "user_config"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
    }
    if wanted("python") {
        result.push(location(
            [
                "pip-cache",
                "python",
                "pip 下载缓存",
                "Python 包下载与构建缓存，不改变包的安装环境",
            ],
            pip.as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| pip_default.clone()),
            pip_default,
            (
                if pip.is_some() {
                    "user_config"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
        result.push(location(
            [
                "hf-home",
                "python",
                "Hugging Face 模型缓存",
                "transformers、diffusers 等通过 huggingface_hub 下载的模型和数据集（HF_HOME），体积常达几十 GB",
            ],
            hf_home
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| hf_default.clone()),
            hf_default,
            (
                if hf_home.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
    }
    if wanted("php") {
        result.push(location(
            [
                "composer-home",
                "php",
                "Composer 用户目录",
                "Composer 全局包、命令入口与用户配置；更改时会同步当前用户 PATH",
            ],
            composer_home
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| composer_home_default.clone()),
            composer_home_default,
            (
                if composer_home.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                true,
            ),
        ));
        result.push(location(
            [
                "composer-cache",
                "php",
                "Composer 下载缓存",
                "Composer 下载的软件包与仓库元数据缓存",
            ],
            composer_cache
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| composer_cache_default.clone()),
            composer_cache_default,
            (
                if composer_cache.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
    }
    if wanted("go") {
        result.push(location(
            [
                "go-module-cache",
                "go",
                "Go 模块缓存",
                "下载的 Go 模块源代码",
            ],
            go_mod
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| go_mod_default.clone()),
            go_mod_default,
            (
                if go_mod.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
        result.push(location(
            [
                "go-build-cache",
                "go",
                "Go 构建缓存",
                "Go 编译产生的可复用构建缓存",
            ],
            go_build
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| go_build_default.clone()),
            go_build_default,
            (
                if go_build.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
    }
    if wanted("rust") {
        result.push(location(
            [
                "cargo-home",
                "rust",
                "Cargo 用户目录",
                "Cargo 命令、注册表缓存、Git 依赖缓存与用户配置",
            ],
            cargo
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| cargo_default.clone()),
            cargo_default,
            (
                if cargo.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                true,
            ),
        ));
        result.push(location(
            [
                "rustup-home",
                "rust",
                "Rustup 工具链目录",
                "rustup 下载并管理的 Rust 工具链、组件与编译目标",
            ],
            rustup
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| rustup_default.clone()),
            rustup_default,
            (
                if rustup.is_some() {
                    "env"
                } else {
                    "tool_default"
                },
                false,
            ),
        ));
    }
    #[cfg(windows)]
    for row in &mut result {
        let variable = match row.id.as_str() {
            "gradle-user-home" => "GRADLE_USER_HOME",
            "composer-home" => "COMPOSER_HOME",
            "composer-cache" => "COMPOSER_CACHE_DIR",
            "go-module-cache" => "GOMODCACHE",
            "go-build-cache" => "GOCACHE",
            "cargo-home" => "CARGO_HOME",
            "rustup-home" => "RUSTUP_HOME",
            _ => continue,
        };
        if let Some(system) = clean_value(winenv::get_raw_in(winenv::Hive::System, variable)) {
            row.default_path = system;
            if clean_value(winenv::get_raw_in(winenv::Hive::User, variable)).is_none() {
                row.source = "system_env".into();
            }
        }
    }
    result
}

fn storage_by_id(id: &str) -> Result<StorageLocation, String> {
    let ecosystem = match id {
        "maven-local-repository" => "maven",
        "gradle-user-home" => "gradle",
        "npm-cache" | "npm-prefix" | "pnpm-store" => "node",
        "pip-cache" | "hf-home" => "python",
        "composer-home" | "composer-cache" => "php",
        "go-module-cache" | "go-build-cache" => "go",
        "cargo-home" | "rustup-home" => "rust",
        _ => return Err(format!("未知存储位置：{id}")),
    };
    all_locations(Some(ecosystem))
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("未知存储位置：{id}"))
}

pub(crate) fn effective_path(id: &str) -> Option<PathBuf> {
    storage_by_id(id)
        .ok()
        .map(|location| PathBuf::from(location.path))
}

fn validate_target(id: &str, target: &Path) -> Result<(), String> {
    if !target.is_absolute() {
        return Err("请选择绝对路径".into());
    }
    if !target
        .components()
        .any(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return Err("不能把磁盘根目录作为工具存储位置".into());
    }
    if target
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("存储位置不能包含上级目录跳转".into());
    }
    let lower = normalized_path(target);
    if lower.contains("\\windows\\")
        || lower.ends_with("\\windows")
        || lower.contains("\\program files\\stacker")
    {
        return Err("不能使用 Windows 系统目录或 Stacker 安装目录".into());
    }
    for item in all_locations(None) {
        if item.id == id {
            continue;
        }
        if normalized_path(Path::new(&item.path)) == lower {
            return Err(format!(
                "该目录已由“{}”使用，请为不同工具选择独立目录",
                item.label
            ));
        }
    }
    fs::create_dir_all(target).map_err(|e| format!("无法创建目标目录：{e}"))?;
    let resolved = target.canonicalize().map_err(|e| e.to_string())?;
    if normalized_path(&resolved) != lower {
        return Err("目标目录经过链接或重定向，请选择实际目录".into());
    }
    let mut probe = tempfile::Builder::new()
        .prefix(".stacker-write-test-")
        .tempfile_in(target)
        .map_err(|e| format!("目标目录不可写：{e}"))?;
    probe
        .write_all(b"ok")
        .map_err(|e| format!("目标目录不可写：{e}"))?;
    Ok(())
}

fn normalized_path(path: &Path) -> String {
    let text = path
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    if let Some(unc) = text.strip_prefix("\\\\?\\unc\\") {
        format!("\\\\{unc}")
    } else {
        text.strip_prefix("\\\\?\\").unwrap_or(&text).to_string()
    }
}

fn validate_migration(source: &Path, target: &Path) -> Result<(), String> {
    let source = match source.canonicalize() {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("读取旧目录失败：{error}")),
    };
    let source = normalized_path(&source);
    let resolved_target = target
        .canonicalize()
        .map_err(|e| format!("无法读取目标目录：{e}"))?;
    let target_text = normalized_path(&resolved_target);
    if source == target_text {
        return Err("新位置与当前目录相同，无需迁移".into());
    }
    if target_text.starts_with(&(source.clone() + "\\"))
        || source.starts_with(&(target_text.clone() + "\\"))
    {
        return Err("新位置不能位于当前目录内部，也不能包含当前目录".into());
    }
    if fs::read_dir(target)
        .map_err(|e| format!("无法读取目标目录：{e}"))?
        .next()
        .is_some()
    {
        return Err("复制现有内容时，请选择一个空目录，避免覆盖已有文件".into());
    }
    Ok(())
}

trait PathEq {
    fn eq_ignore_ascii_case(&self, other: &Path) -> bool;
}

impl PathEq for Path {
    fn eq_ignore_ascii_case(&self, other: &Path) -> bool {
        self.to_string_lossy()
            .eq_ignore_ascii_case(&other.to_string_lossy())
    }
}

fn copy_contents<F>(source: &Path, target: &Path, progress: &mut F) -> Result<(), String>
where
    F: FnMut(&Path),
{
    if crate::installer::op_cancelled() {
        return Err("已取消更改存储位置".into());
    }
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("读取旧目录失败：{error}")),
    };
    if crate::space_analysis::walker::is_link_or_reparse_point(&metadata) {
        return Err(format!(
            "迁移遇到链接或重解析点，未切换配置：{}",
            source.display()
        ));
    }
    if source.eq_ignore_ascii_case(target) {
        return Ok(());
    }
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| format!("读取旧目录失败：{e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        let metadata = fs::symlink_metadata(&from).map_err(|e| e.to_string())?;
        if crate::space_analysis::walker::is_link_or_reparse_point(&metadata) {
            return Err(format!(
                "迁移遇到链接或重解析点，未切换配置：{}",
                from.display()
            ));
        }
        if file_type.is_dir() {
            copy_contents(&from, &to, progress)?;
        } else if file_type.is_file() {
            if crate::installer::op_cancelled() {
                return Err("已取消更改存储位置".into());
            }
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut input = fs::File::open(&from).map_err(|e| e.to_string())?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&to)
                .map_err(|e| format!("复制 {} 失败：{e}", from.display()))?;
            let mut buffer = vec![0_u8; 1024 * 1024];
            loop {
                if crate::installer::op_cancelled() {
                    return Err("已取消更改存储位置".into());
                }
                let bytes = input
                    .read(&mut buffer)
                    .map_err(|e| format!("复制 {} 失败：{e}", from.display()))?;
                if bytes == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..bytes])
                    .map_err(|e| format!("复制 {} 失败：{e}", from.display()))?;
            }
            let after = input.metadata().map_err(|e| e.to_string())?;
            if metadata.len() != after.len() || metadata.modified().ok() != after.modified().ok() {
                return Err(format!(
                    "迁移期间文件发生变化，请停止使用该工具后重试：{}",
                    from.display()
                ));
            }
            fs::set_permissions(&to, metadata.permissions()).map_err(|e| e.to_string())?;
            progress(&from);
        }
    }
    Ok(())
}

fn apply_config(id: &str, value: Option<&str>, old_path: Option<&str>) -> Result<(), String> {
    match id {
        "maven-local-repository" => write_maven_local_repository(value),
        "gradle-user-home" => {
            backup::backup_env(winenv::Hive::User, "gradle-storage", &["GRADLE_USER_HOME"]);
            match value {
                Some(v) => winenv::set_user("GRADLE_USER_HOME", v),
                None => winenv::remove_user("GRADLE_USER_HOME"),
            }
        }
        "npm-cache" => write_flat_key(&sources::npmrc_path(), "cache", value),
        "npm-prefix" => {
            backup::backup_env(winenv::Hive::User, "npm-prefix", &["Path"]);
            write_flat_key(&sources::npmrc_path(), "prefix", value)?;
            if let Some(old) = old_path {
                winenv::remove_path_in(winenv::Hive::User, old)?;
            }
            let active = value.map(PathBuf::from).unwrap_or_else(|| {
                all_locations(Some("node"))
                    .into_iter()
                    .find(|item| item.id == "npm-prefix")
                    .map(|item| PathBuf::from(item.default_path))
                    .unwrap_or_default()
            });
            if !active.as_os_str().is_empty() {
                winenv::prepend_path_in(winenv::Hive::User, &active.to_string_lossy())?;
            }
            Ok(())
        }
        "pnpm-store" => write_flat_key(&sources::npmrc_path(), "store-dir", value),
        "pip-cache" => write_pip_cache(&sources::pip_path(), value),
        "hf-home" => {
            backup::backup_env(winenv::Hive::User, "hf-home", &["HF_HOME"]);
            match value {
                Some(v) => winenv::set_user("HF_HOME", v),
                None => winenv::remove_user("HF_HOME"),
            }
        }
        "composer-home" => {
            backup::backup_env(
                winenv::Hive::User,
                "composer-home",
                &["COMPOSER_HOME", "Path"],
            );
            let old_bin = old_path.map(|path| Path::new(path).join("bin"));
            if let Some(old_bin) = old_bin.as_deref() {
                winenv::remove_path_in(winenv::Hive::User, &old_bin.to_string_lossy())?;
            }
            match value {
                Some(v) => winenv::set_user("COMPOSER_HOME", v)?,
                None => winenv::remove_user("COMPOSER_HOME")?,
            }
            let composer_home = PathBuf::from(storage_by_id("composer-home")?.path);
            winenv::prepend_path_in(
                winenv::Hive::User,
                &composer_home.join("bin").to_string_lossy(),
            )
        }
        "composer-cache" => {
            backup::backup_env(
                winenv::Hive::User,
                "composer-cache",
                &["COMPOSER_CACHE_DIR"],
            );
            match value {
                Some(v) => winenv::set_user("COMPOSER_CACHE_DIR", v),
                None => winenv::remove_user("COMPOSER_CACHE_DIR"),
            }
        }
        "go-module-cache" => {
            backup::backup_env(winenv::Hive::User, "go-module-cache", &["GOMODCACHE"]);
            match value {
                Some(v) => winenv::set_user("GOMODCACHE", v),
                None => winenv::remove_user("GOMODCACHE"),
            }
        }
        "go-build-cache" => {
            backup::backup_env(winenv::Hive::User, "go-build-cache", &["GOCACHE"]);
            match value {
                Some(v) => winenv::set_user("GOCACHE", v),
                None => winenv::remove_user("GOCACHE"),
            }
        }
        "cargo-home" => {
            backup::backup_env(winenv::Hive::User, "cargo-home", &["CARGO_HOME", "Path"]);
            let old_bin = old_path.map(|path| Path::new(path).join("bin"));
            if let Some(old_bin) = old_bin.as_deref() {
                winenv::remove_path_in(winenv::Hive::User, &old_bin.to_string_lossy())?;
            }
            match value {
                Some(v) => winenv::set_user("CARGO_HOME", v)?,
                None => winenv::remove_user("CARGO_HOME")?,
            }
            let cargo_home = PathBuf::from(storage_by_id("cargo-home")?.path);
            winenv::prepend_path_in(
                winenv::Hive::User,
                &cargo_home.join("bin").to_string_lossy(),
            )
        }
        "rustup-home" => {
            backup::backup_env(winenv::Hive::User, "rustup-home", &["RUSTUP_HOME"]);
            match value {
                Some(v) => winenv::set_user("RUSTUP_HOME", v),
                None => winenv::remove_user("RUSTUP_HOME"),
            }
        }
        _ => Err(format!("未知存储位置：{id}")),
    }
}

#[tauri::command]
pub fn storage_locations(ecosystem: Option<String>) -> Vec<StorageLocation> {
    all_locations(ecosystem.as_deref())
}

#[tauri::command]
pub async fn storage_apply(
    window: tauri::Window,
    id: String,
    path: String,
    migrate: bool,
) -> Result<StorageLocation, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::installer::op_reset();
        let current = storage_by_id(&id)?;
        let target = PathBuf::from(path.trim().trim_matches('"'));
        validate_target(&id, &target)?;
        if normalized_path(Path::new(&current.path)) == normalized_path(&target) {
            let _ = window.emit("storage-progress", "__done__".to_string());
            return Ok(current);
        }
        if migrate {
            validate_migration(Path::new(&current.path), &target)?;
            let _ = window.emit("storage-progress", "正在复制现有内容…".to_string());
            let mut copied = 0_u64;
            copy_contents(Path::new(&current.path), &target, &mut |source| {
                copied += 1;
                if copied == 1 || copied % 25 == 0 {
                    let name = source
                        .file_name()
                        .map(|value| value.to_string_lossy())
                        .unwrap_or_default();
                    let _ = window.emit(
                        "storage-progress",
                        format!("已复制 {copied} 个文件 · {name}"),
                    );
                }
            })?;
        }
        if crate::installer::op_cancelled() {
            return Err("已取消更改存储位置".into());
        }
        let _ = window.emit("storage-progress", "正在写入工具配置…".to_string());
        apply_config(&id, Some(&target.to_string_lossy()), Some(&current.path))?;
        let _ = window.emit("storage-progress", "__done__".to_string());
        storage_by_id(&id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn storage_reset(id: String) -> Result<StorageLocation, String> {
    let current = storage_by_id(&id)?;
    apply_config(&id, None, Some(&current.path))?;
    storage_by_id(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_drive_root() {
        assert!(validate_target("npm-cache", Path::new("C:\\")).is_err());
    }

    #[test]
    fn normalizes_windows_extended_paths_for_comparison() {
        assert_eq!(
            normalized_path(Path::new(r"\\?\D:\Cache")),
            normalized_path(Path::new("D:/cache/"))
        );
        assert_eq!(
            normalized_path(Path::new(r"\\?\UNC\host\share\Cache")),
            normalized_path(Path::new(r"\\host\share\cache"))
        );
    }

    #[test]
    #[cfg(windows)]
    fn storage_does_not_fall_back_to_a_stale_process_override() {
        const NAME: &str = "STACKER_STORAGE_TEST_STALE_ENV";
        std::env::set_var(NAME, "stale-process-value");
        let result = env_value(NAME);
        std::env::remove_var(NAME);
        assert_eq!(result, None);
    }

    #[test]
    fn refuses_unreadable_configs_without_replacing_them() {
        let root = tempdir().unwrap();
        let path = root.path().join(".npmrc");
        fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
        assert!(write_flat_key(&path, "cache", Some("new")).is_err());
        assert_eq!(fs::read(&path).unwrap(), vec![0xff, 0xfe, 0xfd]);
    }

    #[test]
    fn target_probe_never_overwrites_an_existing_file() {
        let root = tempdir().unwrap();
        let path = root.path().join("cache");
        fs::create_dir(&path).unwrap();
        fs::write(path.join(".stacker-write-test"), b"keep").unwrap();
        validate_target("npm-cache", &path).unwrap();
        assert_eq!(fs::read(path.join(".stacker-write-test")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(path).unwrap().count(), 1);
    }

    #[test]
    fn copy_never_overwrites_files_created_after_validation() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("cache"), b"new").unwrap();
        fs::write(target.join("cache"), b"keep").unwrap();
        assert!(copy_contents(&source, &target, &mut |_| {}).is_err());
        assert_eq!(fs::read(target.join("cache")).unwrap(), b"keep");
    }

    #[test]
    fn migration_rejects_overlapping_and_non_empty_targets() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("cache.bin"), b"data").unwrap();

        assert!(validate_migration(&source, &source.join("nested")).is_err());
        assert!(validate_migration(&source.join("nested"), &source).is_err());

        let target = root.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("existing.bin"), b"data").unwrap();
        assert!(validate_migration(&source, &target).is_err());
    }

    #[test]
    fn flat_config_updates_one_key_without_losing_other_settings() {
        let root = tempdir().unwrap();
        let path = root.path().join(".npmrc");
        fs::write(&path, "registry=https://registry.example/\ncache=old\n").unwrap();

        backup::with_test_root(&root.path().join("backups"), || {
            write_flat_key(&path, "cache", Some("D:\\npm-cache")).unwrap();
        });
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("registry=https://registry.example/"));
        assert!(content.contains("cache=D:\\npm-cache"));
    }

    #[test]
    fn pip_cache_update_preserves_other_sections() {
        let root = tempdir().unwrap();
        let path = root.path().join("pip.ini");
        fs::write(
            &path,
            "[global]\nindex-url = https://pypi.example/simple\n\n[install]\ntrusted-host = pypi.example\n",
        )
        .unwrap();

        backup::with_test_root(&root.path().join("backups"), || {
            write_pip_cache(&path, Some("D:\\pip-cache")).unwrap();
        });
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("index-url = https://pypi.example/simple"));
        assert!(content.contains("cache-dir = D:\\pip-cache"));
        assert!(content.contains("[install]"));
        assert!(content.contains("trusted-host = pypi.example"));
    }
}
