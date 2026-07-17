use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct GradleWrapperState {
    pub path: String,
    pub exists: bool,
    pub distribution_url: String,
    pub version: String,
    pub package_type: String,
    pub source_id: String,
    pub source_label: String,
}

#[derive(Clone, Copy)]
struct GradleDistSource {
    id: &'static str,
    label: &'static str,
    base: &'static str,
    aliyun_layout: bool,
}

const DIST_SOURCES: &[GradleDistSource] = &[
    GradleDistSource {
        id: "official",
        label: "官方 Gradle",
        base: "https://services.gradle.org/distributions",
        aliyun_layout: false,
    },
    GradleDistSource {
        id: "tencent",
        label: "腾讯云",
        base: "https://mirrors.cloud.tencent.com/gradle",
        aliyun_layout: false,
    },
    GradleDistSource {
        id: "aliyun",
        label: "阿里云",
        base: "https://mirrors.aliyun.com/gradle/distributions",
        aliyun_layout: true,
    },
    GradleDistSource {
        id: "huawei",
        label: "华为云",
        base: "https://repo.huaweicloud.com/gradle",
        aliyun_layout: false,
    },
];

fn source_by_id(id: &str) -> Option<GradleDistSource> {
    DIST_SOURCES.iter().copied().find(|s| s.id == id)
}

fn decode_property_url(value: &str) -> String {
    value.trim().replace("\\:", ":").replace("\\/", "/")
}

fn encode_property_url(value: &str) -> String {
    value.replacen("://", "\\://", 1)
}

fn dist_file_name(url: &str) -> Option<String> {
    let url = decode_property_url(url);
    url.split(['?', '#'])
        .next()
        .unwrap_or(&url)
        .rsplit('/')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_dist_file(file: &str) -> (String, String) {
    let Some(rest) = file.strip_prefix("gradle-") else {
        return (String::new(), String::new());
    };
    for suffix in ["-bin.zip", "-all.zip", "-src.zip"] {
        if let Some(version) = rest.strip_suffix(suffix) {
            return (
                version.to_string(),
                suffix
                    .trim_start_matches('-')
                    .trim_end_matches(".zip")
                    .into(),
            );
        }
    }
    (String::new(), String::new())
}

fn source_of(url: &str) -> GradleDistSource {
    let decoded = decode_property_url(url)
        .trim_end_matches('/')
        .to_ascii_lowercase();
    DIST_SOURCES
        .iter()
        .copied()
        .find(|s| decoded.starts_with(&s.base.to_ascii_lowercase()))
        .unwrap_or(GradleDistSource {
            id: "custom",
            label: "自定义",
            base: "",
            aliyun_layout: false,
        })
}

fn wrapper_url(source: GradleDistSource, version: &str, file: &str) -> Result<String, String> {
    if source.aliyun_layout {
        if version.trim().is_empty() {
            return Err("无法识别 Gradle 版本，不能生成阿里云下载地址".into());
        }
        Ok(format!("{}/v{}/{}", source.base, version.trim(), file))
    } else {
        Ok(format!("{}/{}", source.base, file))
    }
}

fn distribution_line(text: &str) -> Option<(usize, String)> {
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("distributionUrl=") {
            let value = trimmed.split_once('=')?.1.trim().to_string();
            return Some((idx, value));
        }
    }
    None
}

fn state_from_text(path: &Path, text: &str) -> GradleWrapperState {
    let distribution_url = distribution_line(text)
        .map(|(_, value)| decode_property_url(&value))
        .unwrap_or_default();
    let file = dist_file_name(&distribution_url).unwrap_or_default();
    let (version, package_type) = parse_dist_file(&file);
    let source = source_of(&distribution_url);
    GradleWrapperState {
        path: path.to_string_lossy().to_string(),
        exists: path.is_file(),
        distribution_url,
        version,
        package_type,
        source_id: source.id.into(),
        source_label: source.label.into(),
    }
}

fn read_wrapper(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("读取 Gradle Wrapper 配置失败：{e}"))
}

fn skip_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        ".git" | ".gradle" | ".idea" | "build" | "target" | "node_modules" | ".venv" | "venv"
    )
}

#[tauri::command]
pub fn gradle_wrapper_state(path: String) -> Result<GradleWrapperState, String> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Ok(GradleWrapperState {
            path: path.to_string_lossy().to_string(),
            exists: false,
            distribution_url: String::new(),
            version: String::new(),
            package_type: String::new(),
            source_id: "missing".into(),
            source_label: "未选择".into(),
        });
    }
    let text = read_wrapper(&path)?;
    Ok(state_from_text(&path, &text))
}

#[tauri::command]
pub fn gradle_wrapper_scan(root: String) -> Result<Vec<GradleWrapperState>, String> {
    let root = PathBuf::from(root);
    if !root.is_dir() {
        return Err("请选择项目目录或包含项目的父目录".into());
    }
    let mut out = Vec::new();
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > 20_000 || out.len() >= 100 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !skip_dir(&path) {
                    stack.push(path);
                }
                continue;
            }
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("gradle-wrapper.properties"))
                .unwrap_or(false)
            {
                if let Ok(text) = read_wrapper(&path) {
                    out.push(state_from_text(&path, &text));
                }
            }
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn gradle_wrapper_apply(path: String, source_id: String) -> Result<GradleWrapperState, String> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("请选择有效的 gradle-wrapper.properties 文件".into());
    }
    let source = source_by_id(&source_id).ok_or_else(|| "未知 Gradle 下载源".to_string())?;
    let text = read_wrapper(&path)?;
    let (line_idx, current) =
        distribution_line(&text).ok_or_else(|| "未找到 distributionUrl 配置".to_string())?;
    let file =
        dist_file_name(&current).ok_or_else(|| "无法识别 Gradle 发行包文件名".to_string())?;
    if !file.starts_with("gradle-") || !file.ends_with(".zip") {
        return Err("distributionUrl 不是标准 Gradle 发行包地址".into());
    }
    let (version, _) = parse_dist_file(&file);
    let next_url = wrapper_url(source, &version, &file)?;
    let next_line = format!("distributionUrl={}", encode_property_url(&next_url));

    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if line_idx >= lines.len() {
        return Err("无法定位 distributionUrl 配置行".into());
    }
    lines[line_idx] = next_line;
    let mut next = lines.join("\n");
    if text.ends_with('\n') {
        next.push('\n');
    }
    crate::backup::backup_file(&path);
    std::fs::write(&path, next).map_err(|e| format!("写入 Gradle Wrapper 配置失败：{e}"))?;
    let text = read_wrapper(&path)?;
    Ok(state_from_text(&path, &text))
}
