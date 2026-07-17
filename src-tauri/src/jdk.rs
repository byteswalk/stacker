//! JDK 下载源解析：从清华 TUNA 的 Adoptium 镜像，解析某大版本的完整 Windows x64
//! zip 文件名与 URL。Adoptium 镜像按大版本分目录，文件名含完整补丁号（如
//! OpenJDK21U-jdk_x64_windows_hotspot_21.0.11_10.zip），故需运行时解析而非硬编码。

use std::time::Duration;

use serde::Serialize;

const BASE: &str = "https://mirrors.tuna.tsinghua.edu.cn/Adoptium";

#[derive(Serialize)]
pub struct JdkAsset {
    pub version: String, // 完整版本（21.0.11_10 / 8u432b06）
    pub filename: String,
    pub url: String,
}

/// 抽出文件名里所有数字组，作为版本比较键（避免 21.0.9 字典序大于 21.0.11 的坑）。
fn ver_key(name: &str) -> Vec<u64> {
    let b = name.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            out.push(name[s..i].parse().unwrap_or(0));
        } else {
            i += 1;
        }
    }
    out
}

/// arch: "x64"（64 位）或 "x32"（32 位，仅 8/11/17 有）。
fn resolve_impl(major: &str, arch: &str) -> Result<JdkAsset, String> {
    let dir = format!("{BASE}/{major}/jdk/{arch}/windows/");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build();
    let body = agent
        .get(&dir)
        .call()
        .map_err(|e| format!("访问清华镜像失败（该版本可能无此位数构建）：{e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;

    // 文件名 32 位是 x86-32、64 位是 x64，前缀统一 OpenJDK{m}U-，目录已按位数隔离
    let prefix = format!("OpenJDK{major}U-");
    let mut best: Option<String> = None;
    for part in body.split("href=\"").skip(1) {
        let Some(end) = part.find('"') else { continue };
        let name = &part[..end];
        if name.starts_with(&prefix)
            && name.ends_with(".zip")
            && name.contains("windows")
            && !name.contains("debugimage")
        {
            let take = match &best {
                None => true,
                Some(b) => ver_key(name) > ver_key(b),
            };
            if take {
                best = Some(name.to_string());
            }
        }
    }

    let filename = best.ok_or("未在清华镜像找到该版本/位数的 Windows zip")?;
    let version = filename
        .split("_hotspot_")
        .nth(1)
        .and_then(|s| s.strip_suffix(".zip"))
        .unwrap_or("")
        .to_string();
    Ok(JdkAsset {
        url: format!("{dir}{filename}"),
        version,
        filename,
    })
}

#[tauri::command]
pub async fn jdk_resolve(major: String, arch: String) -> Result<JdkAsset, String> {
    tauri::async_runtime::spawn_blocking(move || resolve_impl(&major, &arch))
        .await
        .map_err(|e| e.to_string())?
}

// ── 阿里 Dragonwell：从 release 元数据解析 Windows x64 资产，实际下载走官方 OSS 镜像 ──
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()
}

fn fetch_text(url: &str, candidates: &[String]) -> Result<String, String> {
    let a = agent();
    let mut tried = vec![url.to_string()];
    tried.extend_from_slice(candidates);
    let mut last = String::new();
    for u in tried {
        match a.get(&u).set("User-Agent", "Stacker").call() {
            Ok(r) => match r.into_string() {
                Ok(b) => return Ok(b),
                Err(e) => last = e.to_string(),
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn dragonwell_version_from_filename(filename: &str) -> Option<String> {
    filename
        .trim_end_matches(".zip")
        .split('_')
        .find(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()) && s.contains('.'))
        .map(|s| s.to_string())
}

fn dragonwell_oss_url(major: &str, filename: &str, version: &str) -> String {
    let dir = if matches!(major, "17" | "21") {
        match version.rsplit_once('.') {
            Some((base, build)) => format!("{base}%2B{build}"),
            None => version.to_string(),
        }
    } else {
        version.to_string()
    };
    format!("https://dragonwell.oss-cn-shanghai.aliyuncs.com/{dir}/{filename}")
}

fn dragonwell_impl(major: &str) -> Result<JdkAsset, String> {
    let api = format!(
        "https://api.github.com/repos/dragonwell-project/dragonwell{major}/releases/latest"
    );
    let body = fetch_text(&api, &[]).map_err(|e| format!("获取 Dragonwell 发布信息失败：{e}"))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let assets = v["assets"].as_array().ok_or("无法解析 release 资产列表")?;
    // 只取 Windows x64 zip；优先 Standard 版（无则退而取任意，如个别版本只发 Extended）
    let wins: Vec<&serde_json::Value> = assets
        .iter()
        .filter(|a| {
            a["name"]
                .as_str()
                .is_some_and(|n| n.ends_with("_x64_windows.zip"))
        })
        .collect();
    let asset = wins
        .iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n.contains("Standard")))
        .or_else(|| wins.first())
        .ok_or("该版本无 Windows x64 构建")?;
    let filename = asset["name"].as_str().unwrap_or("").to_string();
    let version =
        dragonwell_version_from_filename(&filename).ok_or("无法解析 Dragonwell 版本号")?;
    let url = dragonwell_oss_url(major, &filename, &version);
    Ok(JdkAsset {
        url,
        version,
        filename,
    })
}

#[tauri::command]
pub async fn dragonwell_resolve(major: String) -> Result<JdkAsset, String> {
    tauri::async_runtime::spawn_blocking(move || dragonwell_impl(&major))
        .await
        .map_err(|e| e.to_string())?
}

// ── Azul Zulu：Azul 元数据 API 查 Windows zip，从 cdn.azul.com 下载（无国产镜像）──
fn zulu_impl(major: &str, bitness: &str) -> Result<JdkAsset, String> {
    let api = format!(
        "https://api.azul.com/metadata/v1/zulu/packages/?java_version={major}&os=windows&arch=x86&hw_bitness={bitness}&archive_type=zip&java_package_type=jdk&javafx_bundled=false&release_status=ga&latest=true&availability_types=CA&page_size=20"
    );
    let body = fetch_text(&api, &[]).map_err(|e| format!("访问 Azul API 失败：{e}"))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let arr = v.as_array().ok_or("Azul 返回格式异常")?;
    // 选标准版（排除 crac / javafx 变体）
    let pkg = arr
        .iter()
        .find(|p| {
            p["name"]
                .as_str()
                .is_some_and(|n| !n.contains("crac") && !n.contains("-fx"))
        })
        .or_else(|| arr.first())
        .ok_or("该版本/位数无 Windows 构建")?;
    let url = pkg["download_url"]
        .as_str()
        .ok_or("无下载地址")?
        .to_string();
    let filename = pkg["name"].as_str().unwrap_or("").to_string();
    let version = pkg["java_version"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64())
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(".")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| major.to_string());
    Ok(JdkAsset {
        url,
        version,
        filename,
    })
}

#[tauri::command]
pub async fn zulu_resolve(major: String, arch: String) -> Result<JdkAsset, String> {
    let bitness = if arch == "x32" { "32" } else { "64" };
    tauri::async_runtime::spawn_blocking(move || zulu_impl(&major, bitness))
        .await
        .map_err(|e| e.to_string())?
}
