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

fn clean_source_url(source_url: Option<String>) -> Option<String> {
    source_url
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
}

fn source_base(
    source: &str,
    source_url: Option<String>,
    defaults: &[(&str, &str)],
) -> Option<String> {
    clean_source_url(source_url).or_else(|| {
        defaults
            .iter()
            .find(|(id, _)| *id == source)
            .map(|(_, url)| (*url).to_string())
    })
}

/// Maven 版本。按下载源目录列存在的版本；archive 源会列历史版本，其它镜像按实际同步内容列。
#[tauri::command]
pub async fn maven_versions(
    source: Option<String>,
    source_url: Option<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "official".into());
        let base = source_base(
            &source,
            source_url,
            &[
                ("official", "https://archive.apache.org/dist/maven"),
                ("apache-cdn", "https://dlcdn.apache.org/maven"),
                ("tuna", "https://mirrors.tuna.tsinghua.edu.cn/apache/maven"),
                ("ustc", "https://mirrors.ustc.edu.cn/apache/maven"),
                ("aliyun", "https://mirrors.aliyun.com/apache/maven"),
                ("huawei", "https://repo.huaweicloud.com/apache/maven"),
                ("tencent", "https://mirrors.cloud.tencent.com/apache/maven"),
            ],
        )
        .ok_or("Maven 下载源地址无效")?;
        let mut vs: Vec<String> = Vec::new();

        for track in ["maven-4", "maven-3", "maven-2"] {
            let url = format!("{base}/{track}/");
            let Ok(body) = fetch(&url) else { continue };
            for name in hrefs(&body) {
                if !name.contains('/')
                    && name.contains('.')
                    && name.chars().next().is_some_and(|c| c.is_ascii_digit())
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

/// Gradle 版本。官方源读 services JSON；镜像源读目录中实际存在的 bin zip。
#[tauri::command]
pub async fn gradle_versions(
    source: Option<String>,
    source_url: Option<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "official".into());
        let base = source_base(
            &source,
            source_url,
            &[
                ("official", "https://services.gradle.org/distributions"),
                ("tencent", "https://mirrors.cloud.tencent.com/gradle"),
                ("aliyun", "https://mirrors.aliyun.com/gradle/distributions"),
                ("huawei", "https://repo.huaweicloud.com/gradle"),
            ],
        )
        .ok_or("Gradle 下载源地址无效")?;
        let official = base.contains("services.gradle.org");
        let aliyun_layout = source == "aliyun" || base.contains("mirrors.aliyun.com/gradle");
        let mut vs: Vec<String> = if official {
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
        } else if aliyun_layout {
            let body = fetch(&format!("{base}/"))?;
            hrefs(&body)
                .into_iter()
                .filter_map(|name| {
                    name.strip_prefix('v')
                        .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
                        .map(String::from)
                })
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

/// Go 版本。官方源读取 go.dev 发布清单，镜像源只列实际存在的 Windows x64 zip。
#[tauri::command]
pub async fn go_versions(
    source: Option<String>,
    source_url: Option<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = source.unwrap_or_else(|| "official".into());
        let base = source_base(
            &source,
            source_url,
            &[
                ("official", "https://go.dev/dl"),
                ("aliyun", "https://mirrors.aliyun.com/golang"),
            ],
        )
        .ok_or("Go 下载源地址无效")?;
        let official = base.contains("go.dev/dl");
        let mut vs: Vec<String> = if official {
            let body = fetch("https://go.dev/dl/?mode=json&include=all")?;
            let releases: serde_json::Value =
                serde_json::from_str(&body).map_err(|e| format!("Go 版本清单格式异常：{e}"))?;
            releases
                .as_array()
                .ok_or("Go 版本清单格式异常")?
                .iter()
                .filter(|release| {
                    release["files"].as_array().is_some_and(|files| {
                        files.iter().any(|file| {
                            file["os"].as_str() == Some("windows")
                                && file["arch"].as_str() == Some("amd64")
                                && file["kind"].as_str() == Some("archive")
                                && file["filename"]
                                    .as_str()
                                    .is_some_and(|name| name.ends_with(".zip"))
                        })
                    })
                })
                .filter_map(|release| {
                    release["version"]
                        .as_str()
                        .and_then(|version| version.strip_prefix("go"))
                        .map(String::from)
                })
                .collect()
        } else {
            let body = fetch(&format!("{base}/"))?;
            hrefs(&body)
                .into_iter()
                .filter_map(|name| {
                    name.strip_prefix("go")
                        .and_then(|value| value.strip_suffix(".windows-amd64.zip"))
                        .filter(|version| {
                            version.chars().next().is_some_and(|c| c.is_ascii_digit())
                        })
                        .map(String::from)
                })
                .collect()
        };
        vs.sort_by(|a, b| version_cmp_desc(a, b));
        vs.dedup();
        vs.truncate(160);
        if vs.is_empty() {
            Err("当前下载源未提供 Windows 64 位 Go 发行包".into())
        } else {
            Ok(vs)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn href_parser_keeps_directory_and_file_names() {
        let body = r#"<a href="maven-3/">maven-3</a><a href="3.9.11/">3.9.11</a><a href="gradle-9.1-bin.zip">zip</a>"#;
        assert_eq!(hrefs(body), vec!["maven-3", "3.9.11", "gradle-9.1-bin.zip"]);
    }

    #[test]
    fn release_versions_sort_before_prereleases() {
        let mut versions = vec![
            "4.0.0-rc-2".to_string(),
            "3.9.11".to_string(),
            "4.0.0-beta-5".to_string(),
            "4.0.0".to_string(),
        ];
        versions.sort_by(|a, b| version_cmp_desc(a, b));
        assert_eq!(
            versions,
            vec!["4.0.0", "4.0.0-rc-2", "4.0.0-beta-5", "3.9.11"]
        );
    }

    #[test]
    fn configured_source_url_overrides_builtin_address() {
        let base = source_base(
            "official",
            Some("https://mirror.example.test/maven/".into()),
            &[("official", "https://archive.apache.org/dist/maven")],
        );
        assert_eq!(base.as_deref(), Some("https://mirror.example.test/maven"));
    }

    #[test]
    fn invalid_configured_source_falls_back_to_builtin_address() {
        let base = source_base(
            "official",
            Some("file:///tmp/maven".into()),
            &[("official", "https://archive.apache.org/dist/maven")],
        );
        assert_eq!(
            base.as_deref(),
            Some("https://archive.apache.org/dist/maven")
        );
    }
}
