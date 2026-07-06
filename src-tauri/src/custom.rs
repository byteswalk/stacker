//! 自定义私有源：用户自建镜像（可含鉴权），密码用 DPAPI 加密落盘。
//! 自定义源会合并进对应工具的镜像列表（merge_into），detect/apply/方案/代理白名单
//! 全部自动复用内置换源逻辑；apply 时若带凭据，再为 npm/pip 注入鉴权。
//! 存储 %APPDATA%\stacker\custom_sources.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{backup, dpapi, sources, winenv};

#[derive(Serialize, Deserialize, Clone)]
struct CustomSource {
    id: String,   // "custom:<毫秒时间戳>"
    tool: String, // 内置工具 id（pip/npm/go/...）
    name: String,
    url: String,
    username: String, // 可空
    #[serde(default)]
    enc_password: Vec<u8>, // DPAPI 密文，空=无密码
}

/// 给前端的 DTO（不含密文，只标注是否有密码）；也用作导出/导入格式。
#[derive(Serialize, Deserialize, Clone)]
pub struct CustomDTO {
    pub id: String,
    pub tool: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub has_password: bool,
}

fn store_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("custom_sources.json")
}

fn load() -> Vec<CustomSource> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_all(list: &[CustomSource]) -> Result<(), String> {
    let p = store_path();
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())
}

fn host_of(url: &str) -> String {
    let after = url.split("://").nth(1).unwrap_or(url);
    let host = after.split('/').next().unwrap_or("");
    host.split('@').last().unwrap_or(host).to_string()
}

/// 把自定义源作为镜像合并进对应工具（供 sources::tools() 调用）。
pub fn merge_into(tools: &mut [sources::Tool]) {
    for c in load() {
        if let Some(t) = tools.iter_mut().find(|t| t.id == c.tool) {
            t.mirrors.push(sources::Mirror {
                id: c.id.clone(),
                name: format!("{}（自定义）", c.name),
                url: c.url.clone(),
                host: host_of(&c.url),
            });
        }
    }
}

// ── DTO 转换 ──
fn dto(c: &CustomSource) -> CustomDTO {
    CustomDTO {
        id: c.id.clone(),
        tool: c.tool.clone(),
        name: c.name.clone(),
        url: c.url.clone(),
        username: c.username.clone(),
        has_password: !c.enc_password.is_empty(),
    }
}

#[tauri::command]
pub fn custom_list() -> Vec<CustomDTO> {
    load().iter().map(dto).collect()
}

/// 新建/编辑自定义源。
/// - id 为空 => 新建；非空 => 按 id 覆盖。
/// - password: None=不动原密码；Some("")=清除；Some(x)=设置（DPAPI 加密）。
#[tauri::command]
pub fn custom_save(
    id: Option<String>,
    tool: String,
    name: String,
    url: String,
    username: String,
    password: Option<String>,
) -> Result<CustomDTO, String> {
    let name = name.trim().to_string();
    let url = url.trim().to_string();
    if name.is_empty() {
        return Err("名称不能为空".into());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with("sparse+") {
        return Err("地址需以 http(s):// 开头".into());
    }
    if !sources::tools_builtin_ids().contains(&tool.as_str()) {
        return Err(format!("未知工具：{tool}"));
    }

    let mut list = load();
    let editing_id = id.filter(|s| !s.is_empty());

    // 计算密文：编辑时 None 表示沿用原密文
    let prev_enc = editing_id
        .as_ref()
        .and_then(|eid| list.iter().find(|c| &c.id == eid))
        .map(|c| c.enc_password.clone())
        .unwrap_or_default();
    let enc_password = match password {
        None => prev_enc,
        Some(p) if p.is_empty() => Vec::new(),
        Some(p) => dpapi::encrypt(&p)?,
    };

    let rec = CustomSource {
        id: editing_id
            .clone()
            .unwrap_or_else(|| format!("custom:{}", chrono::Local::now().timestamp_millis())),
        tool,
        name,
        url,
        username: username.trim().to_string(),
        enc_password,
    };

    match editing_id {
        Some(eid) => {
            let slot = list.iter_mut().find(|c| c.id == eid).ok_or("源不存在")?;
            *slot = rec.clone();
        }
        None => list.push(rec.clone()),
    }
    save_all(&list)?;
    Ok(dto(&rec))
}

#[tauri::command]
pub fn custom_delete(id: String) -> Result<(), String> {
    let mut list = load();
    let before = list.len();
    list.retain(|c| c.id != id);
    if list.len() == before {
        return Err("源不存在".into());
    }
    save_all(&list)
}

/// 导出自定义源定义（不含密码——DPAPI 密文跨机无意义）。
pub fn export_all() -> Vec<CustomDTO> {
    load().iter().map(dto).collect()
}

/// 导入自定义源（按 id 或同名去重）。密码需在新机重新填写。返回写入数。
pub fn import_merge(incoming: Vec<CustomDTO>) -> Result<usize, String> {
    let mut list = load();
    let mut n = 0;
    for it in incoming {
        list.retain(|c| c.id != it.id && c.name != it.name);
        list.push(CustomSource {
            id: if it.id.is_empty() {
                format!(
                    "custom:{}",
                    chrono::Local::now().timestamp_millis() + n as i64
                )
            } else {
                it.id
            },
            tool: it.tool,
            name: it.name,
            url: it.url,
            username: it.username,
            enc_password: Vec::new(),
        });
        n += 1;
    }
    save_all(&list)?;
    Ok(n)
}

#[derive(Serialize)]
pub struct CustomImportSummary {
    pub added: usize,
    pub skipped: usize,
}

/// 导入本地自定义源，但不覆盖用户已有记录。按 id / 名称 / 工具+地址判重。
pub fn import_preserve(incoming: Vec<CustomDTO>) -> Result<CustomImportSummary, String> {
    let mut list = load();
    let mut added = 0usize;
    let mut skipped = 0usize;
    for it in incoming {
        if !sources::tools_builtin_ids().contains(&it.tool.as_str()) {
            skipped += 1;
            continue;
        }
        let url = it.url.trim().to_string();
        let name = it.name.trim().to_string();
        if name.is_empty()
            || (!url.starts_with("http://")
                && !url.starts_with("https://")
                && !url.starts_with("sparse+"))
        {
            skipped += 1;
            continue;
        }
        let exists = list.iter().any(|c| {
            (!it.id.is_empty() && c.id == it.id)
                || c.name == name
                || (c.tool == it.tool && c.url.trim_end_matches('/') == url.trim_end_matches('/'))
        });
        if exists {
            skipped += 1;
            continue;
        }
        list.push(CustomSource {
            id: if it.id.is_empty() {
                format!(
                    "custom:{}",
                    chrono::Local::now().timestamp_millis() + added as i64
                )
            } else {
                it.id
            },
            tool: it.tool,
            name,
            url,
            username: it.username.trim().to_string(),
            enc_password: Vec::new(),
        });
        added += 1;
    }
    save_all(&list)?;
    Ok(CustomImportSummary { added, skipped })
}

// ── 鉴权注入（apply_source 在写完 URL 后调用）──

fn b64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(T[(b[0] >> 2) as usize] as char);
        out.push(T[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(b[2] & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn npmrc_set(key: &str, value: &str) -> Result<(), String> {
    let p = sources::npmrc_path();
    backup::backup_file(&p);
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let mut out = String::new();
    let mut done = false;
    for line in text.lines() {
        if line.trim_start().starts_with(&format!("{key}=")) {
            out.push_str(&format!("{key}={value}\n"));
            done = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !done {
        out.push_str(&format!("{key}={value}\n"));
    }
    std::fs::write(&p, out).map_err(|e| e.to_string())
}

/// 把 user:pass 凭据嵌进 URL 的 scheme 之后（用于 pip / go）。
fn embed_creds(url: &str, creds: &str) -> String {
    if creds.is_empty() {
        return url.to_string();
    }
    for scheme in ["sparse+https://", "sparse+http://", "https://", "http://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            return format!("{scheme}{creds}@{rest}");
        }
    }
    url.to_string()
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn groovy_esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}
fn toml_esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_file(p: &std::path::Path, content: &str) -> Result<(), String> {
    backup::backup_file(p);
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, content).map_err(|e| e.to_string())
}

/// 若该自定义源带凭据，按 handler 注入鉴权。无凭据则不动。
/// npm/yarn=_authToken|_auth、pip/go=URL 内嵌、maven=settings.xml<server>、
/// gradle=init.gradle credentials{}、cargo=registries+credentials.toml。
pub fn apply_auth(handler: &str, mirror: &sources::Mirror) -> Result<(), String> {
    let list = load();
    let Some(c) = list.iter().find(|c| c.id == mirror.id) else {
        return Ok(());
    };
    if c.enc_password.is_empty() && c.username.is_empty() {
        return Ok(());
    }
    let pass = if c.enc_password.is_empty() {
        String::new()
    } else {
        dpapi::decrypt(&c.enc_password)?
    };
    let host = host_of(&c.url);

    match handler {
        "npmrc" | "yarnrc" => {
            if c.username.is_empty() {
                // 仅密码 => 当作 token
                if !pass.is_empty() {
                    npmrc_set(&format!("//{host}/:_authToken"), &pass)?;
                }
            } else {
                // 用户名+密码 => Basic
                let auth = b64(format!("{}:{}", c.username, pass).as_bytes());
                npmrc_set(&format!("//{host}/:_auth"), &auth)?;
                npmrc_set("always-auth", "true")?;
            }
            Ok(())
        }
        "go_env" => {
            // 凭据嵌进 GOPROXY（Go 支持 https://user:pass@host）
            let creds = if c.username.is_empty() {
                pass.clone()
            } else {
                format!("{}:{}", c.username, pass)
            };
            backup::backup_env(winenv::Hive::User, "go-source", &["GOPROXY"]);
            winenv::set_user("GOPROXY", &embed_creds(&c.url, &creds))
        }
        "maven_settings" => {
            // 覆盖 settings.xml：mirror + 同 id 的 server 凭据
            let xml = format!(
"<settings>\n  <servers>\n    <server>\n      <id>stacker-mirror</id>\n      <username>{u}</username>\n      <password>{p}</password>\n    </server>\n  </servers>\n  <mirrors>\n    <mirror>\n      <id>stacker-mirror</id>\n      <mirrorOf>central</mirrorOf>\n      <name>stacker</name>\n      <url>{url}</url>\n    </mirror>\n  </mirrors>\n</settings>\n",
                u = xml_esc(&c.username), p = xml_esc(&pass), url = xml_esc(&c.url));
            write_file(&sources::maven_path(), &xml)
        }
        "gradle_init" => {
            // 覆盖 init.gradle：repo 带 credentials{}
            let g = format!(
"allprojects {{\n    repositories {{\n        maven {{\n            url '{url}'\n            credentials {{\n                username '{u}'\n                password '{p}'\n            }}\n        }}\n        mavenCentral()\n    }}\n}}\n",
                url = groovy_esc(&c.url), u = groovy_esc(&c.username), p = groovy_esc(&pass));
            write_file(&sources::gradle_path(), &g)
        }
        "cargo_config" => {
            // config.toml 用具名 registry + 源替换；token 写 credentials.toml
            let cfg = format!(
"[registries.stacker]\nindex = \"{idx}\"\n\n[source.crates-io]\nreplace-with = \"stacker\"\n\n[source.stacker]\nregistry = \"{idx}\"\n",
                idx = toml_esc(&c.url));
            let cargo_cfg = sources::cargo_path();
            write_file(&cargo_cfg, &cfg)?;
            let token = if pass.is_empty() {
                c.username.clone()
            } else {
                pass.clone()
            };
            if !token.is_empty() {
                let cred = cargo_cfg
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("credentials.toml");
                write_file(
                    &cred,
                    &format!("[registries.stacker]\ntoken = \"{}\"\n", toml_esc(&token)),
                )?;
            }
            Ok(())
        }
        "pip_ini" => {
            // 把凭据嵌进 index-url
            let creds = if c.username.is_empty() {
                pass.clone()
            } else {
                format!("{}:{}", c.username, pass)
            };
            let withc = if let Some(rest) = c.url.strip_prefix("https://") {
                format!("https://{creds}@{rest}")
            } else if let Some(rest) = c.url.strip_prefix("http://") {
                format!("http://{creds}@{rest}")
            } else {
                c.url.clone()
            };
            let p = sources::pip_path();
            backup::backup_file(&p);
            let mut conf = ini::Ini::load_from_file(&p).unwrap_or_else(|_| ini::Ini::new());
            conf.with_section(Some("global"))
                .set("index-url", withc.as_str());
            if !host.is_empty() {
                conf.with_section(Some("install"))
                    .set("trusted-host", host.as_str());
            }
            conf.write_to_file(&p).map_err(|e| e.to_string())
        }
        // go/cargo/maven/gradle 私有源的鉴权格式各异，暂仅 URL（待确认后再接）
        _ => Ok(()),
    }
}
