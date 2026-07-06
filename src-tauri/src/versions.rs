//! 实时拉取工具发行版版本号。Maven / Gradle 按当前下载源列该源实际存在的
//! Windows zip 版本，避免在 UI 里展示当前源没有的版本。

use std::time::Duration;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build()
}

fn fetch(url: &str) -> Result<String, String> {
    agent()
        .get(url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("拉取失败：{e}"))?
        .into_string()
        .map_err(|e| e.to_string())
}

fn nums(s: &str) -> Vec<u64> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let st = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            out.push(s[st..i].parse().unwrap_or(0));
        } else {
            i += 1;
        }
    }
    out
}

fn pre_rank(s: &str) -> u8 {
    let l = s.to_ascii_lowercase();
    if l.contains("rc") {
        2
    } else if l.contains("beta") || l.contains("milestone") {
        1
    } else if l.contains("alpha") {
        0
    } else {
        3
    }
}

fn version_parts(s: &str) -> (Vec<u64>, u8, u64) {
    let l = s.to_ascii_lowercase();
    let markers = ["alpha", "beta", "milestone", "rc"];
    let split_at = markers
        .iter()
        .filter_map(|m| l.find(m))
        .min()
        .unwrap_or(s.len());
    let main = &s[..split_at];
    let pre = &s[split_at..];
    let pre_num = nums(pre).last().copied().unwrap_or(0);
    (nums(main), pre_rank(s), pre_num)
}

fn version_cmp_desc(a: &str, b: &str) -> std::cmp::Ordering {
    let (an, ar, ap) = version_parts(a);
    let (bn, br, bp) = version_parts(b);
    bn.cmp(&an)
        .then_with(|| br.cmp(&ar))
        .then_with(|| bp.cmp(&ap))
        .then_with(|| b.cmp(a))
}

fn hrefs(body: &str) -> Vec<String> {
    body.split("href=\"")
        .skip(1)
        .filter_map(|part| {
            part.find('"')
                .map(|end| part[..end].trim_end_matches('/').to_string())
        })
        .collect()
}

fn maven_base(source: &str) -> Option<&'static str> {
    match source {
        "apache" | "official" => Some("https://archive.apache.org/dist/maven"),
        "tuna" => Some("https://mirrors.tuna.tsinghua.edu.cn/apache/maven"),
        "ustc" => Some("https://mirrors.ustc.edu.cn/apache/maven"),
        "aliyun" => Some("https://mirrors.aliyun.com/apache/maven"),
        "huawei" => Some("https://repo.huaweicloud.com/apache/maven"),
        "tencent" => Some("https://mirrors.cloud.tencent.com/apache/maven"),
        _ => None,
    }
}

fn gradle_base(source: &str) -> Option<&'static str> {
    match source {
        "official" => Some("https://services.gradle.org"),
        "tencent" => Some("https://mirrors.cloud.tencent.com/gradle"),
        "huawei" => Some("https://repo.huaweicloud.com/gradle"),
        _ => None,
    }
}

/// Maven 版本。按下载源目录列存在的版本；archive 源会列历史版本，其它镜像按实际同步内容列。
#[tauri::command]
pub async fn maven_versions(source: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "tuna".into());
        let base = maven_base(&source).ok_or("未知 Maven 下载源")?;
        let mut vs: Vec<String> = Vec::new();

        for track in ["maven-4", "maven-3", "maven-2"] {
            let url = format!("{base}/{track}/");
            let Ok(body) = fetch(&url) else { continue };
            for name in hrefs(&body) {
                if !name.contains('/')
                    && name.contains('.')
                    && name.chars().next().map_or(false, |c| c.is_ascii_digit())
                {
                    vs.push(name);
                }
            }
        }
        vs.sort_by(|a, b| version_cmp_desc(a, b));
        vs.dedup();
        vs.truncate(160);
        if vs.is_empty() {
            Err("未解析到 Maven 版本".into())
        } else {
            Ok(vs)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Gradle 版本。官方源读 services JSON；国内镜像读目录中实际存在的 bin zip。
#[tauri::command]
pub async fn gradle_versions(source: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "tencent".into());
        let base = gradle_base(&source).ok_or("未知 Gradle 下载源")?;
        let mut vs: Vec<String> = if source == "official" {
            let body = fetch("https://services.gradle.org/versions/all")?;
            let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            let arr = v.as_array().ok_or("Gradle 版本 JSON 格式异常")?;
            arr.iter()
                .filter(|e| {
                    e["snapshot"].as_bool() != Some(true)
                        && e["nightly"].as_bool() != Some(true)
                        && e["broken"].as_bool() != Some(true)
                })
                .filter_map(|e| e["version"].as_str().map(String::from))
                .collect()
        } else {
            let body = fetch(&format!("{base}/"))?;
            hrefs(&body)
                .into_iter()
                .filter_map(|name| {
                    name.strip_prefix("gradle-")
                        .and_then(|s| s.strip_suffix("-bin.zip"))
                        .map(String::from)
                })
                .collect()
        };
        vs.sort_by(|a, b| version_cmp_desc(a, b));
        vs.dedup();
        vs.truncate(160);
        if vs.is_empty() {
            Err("未解析到 Gradle 版本".into())
        } else {
            Ok(vs)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
