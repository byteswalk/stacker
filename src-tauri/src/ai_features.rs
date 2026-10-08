//! What each page asks the AI source, and how its answer is shaped.
//!
//! Every feature here explains or suggests; none of them acts. The prompt is built from data
//! the page already shows — a log, a checkup result, a path and a size — never from anything
//! it would have to read off disk on its own, and each input is capped so a runaway log cannot
//! turn into a runaway bill.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Longest single block of text any prompt carries.
const MAX_BLOCK: usize = 8_000;

/// The same for every answer: plain Chinese, short, with the next step spelt out.
const STYLE: &str = "用简体中文回答，不用 Markdown 标题，不超过 250 字。先一句话给结论，再分点给具体做法（能给命令就给命令）。\
没把握的不要编，直接说“看不出来”。";

fn tail(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let kept: String = chars[chars.len() - max..].iter().collect();
    format!("…（前面省略 {} 字）\n{kept}", chars.len() - max)
}

fn field<'a>(payload: &'a Value, key: &str) -> &'a str {
    payload.get(key).and_then(Value::as_str).unwrap_or("")
}

/// What each status means in Stacker's own API service, taken from its server code, so the
/// answer reasons from facts rather than from what the codes mean elsewhere.
const GATEWAY_STATUSES: &str = "400 请求格式不对：不是 JSON、缺字段，或附件该模型读不了（codex 读不了 PDF，只有 claude/* 和 codex/* 能读图）；\n\
401 密钥错误或没带，或者这个智能体的命令行没登录；\n\
403 请求来自浏览器网页（一律拒绝），或者这个智能体在接口服务里被关掉了；\n\
404 接口路径不对，只有 /v1/chat/completions、/v1/messages、/v1/models；\n\
429 同时排队的请求太多；\n\
502 智能体运行失败；\n\
503 这台电脑上没装这个智能体的命令行；\n\
504 5 分钟内智能体没有回复。";

/// The prompt for one kind of question, from what the page sent.
pub fn prompt(kind: &str, payload: &Value) -> Result<String, String> {
    let json = |v: &Value| {
        tail(
            &serde_json::to_string_pretty(v).unwrap_or_default(),
            MAX_BLOCK,
        )
    };
    let text = match kind {
        "install_failure" => format!(
            "你是 Windows 开发机上的软件安装排错助手。下面是一次{action}失败的任务日志。\n\
             产品：{product}（{surface}）\n最后的错误：{message}\n日志：\n{log}\n\n\
             请判断失败原因（网络/代理、权限、文件被占用、安装包损坏、版本冲突等），并给出修复步骤。{STYLE}",
            action = field(payload, "action"),
            product = field(payload, "product"),
            surface = field(payload, "surface"),
            message = field(payload, "message"),
            log = tail(field(payload, "log"), MAX_BLOCK),
        ),
        "checkup" => format!(
            "你是 Windows 开发环境体检顾问。下面是体检中没有通过的项目（JSON）：\n{items}\n\n\
             请按影响从大到小排出先修哪几项、为什么，并给出每项的修法。{STYLE}",
            items = json(payload.get("items").unwrap_or(&Value::Null)),
        ),
        "toolchain" => format!(
            "你是 Windows 开发环境顾问。下面是「{page}」这一页检测到的状态（JSON）：\n{items}\n\n\
             请解释其中的冲突或异常（例如 PATH 里有多个版本、环境变量与默认版本不一致），\
             说明实际会用到哪一个、为什么，以及怎么改。状态都正常就直说正常。{STYLE}",
            page = field(payload, "page"),
            items = json(payload.get("items").unwrap_or(&Value::Null)),
        ),
        "disk_directory" => format!(
            "你是 Windows 开发机的磁盘空间顾问。下面是目录 {path}（实际占用 {size}）和它下一层占用最大的子目录\
             （名称、实际占用、下面还有几个子目录；JSON）：\n{children}\n\n\
             请给一个综合解读，用简体中文、不用 Markdown 标题、不超过 450 字，分四段：\n\
             1. 一句话说这个目录整体是什么、属于哪个程序或工具；\n\
             2. 空间主要花在哪几个子目录上，各自存的是什么；\n\
             3. 哪些可以放心清理（缓存、日志、临时文件、构建产物、旧版本、安装包等），怎么清最好\
             （有工具自带的清理命令就给命令，比如 npm cache clean --force、cargo clean）；\n\
             4. 哪些删前要确认或不要删，为什么。\n\
             只凭路径、名称和大小判断，看不出来的就直说“看不出来”，不要编。",
            path = field(payload, "path"),
            size = field(payload, "size"),
            children = json(payload.get("children").unwrap_or(&Value::Null)),
        ),
        "skipped_path" => format!(
            "你是 Windows 开发机的磁盘空间顾问。磁盘扫描跳过了下面这个路径，没有计入空间统计。\n\
             路径：{path}\n跳过原因（扫描器给出的）：{reason}\n同一路径被跳过的次数：{count}\n\
             扫描器的规则：硬链接（同一个文件 ID 已在别处计入）只算一次；符号链接、目录联接和其他\
             重解析点只记录不进入，避免重复统计或绕到别的盘；无权访问、扫描中消失、无法读取的都跳过。\n\n\
             请说明：1) 这个路径大概属于什么程序或工具，为什么会出现这种情况（例如 pnpm 用硬链接共享包、\
             OneDrive 占位文件、系统保护目录）；2) 跳过它会不会让统计结果明显偏小；3) 需不需要用户做什么，\
             要的话怎么做（例如以管理员身份重新扫描）。{STYLE}",
            path = field(payload, "path"),
            reason = field(payload, "reason"),
            count = field(payload, "count"),
        ),
        "gateway_error" => format!(
            "你是一个本机 AI 接口服务的排错助手。这个服务把本机已登录的 Codex / Claude 等命令行包装成 \
             OpenAI / Anthropic 风格的接口。记录里有状态码，常常还有服务给出的报错原文（error 字段）和调用方式（是否流式、推理强度、客户端），但没有请求内容；这个服务的状态码含义如下：\n\
             {GATEWAY_STATUSES}\n\n下面是一条失败的请求记录（JSON）：\n{entry}\n\n\
             请根据报错原文、状态码、接口和模型说明最可能的原因，以及调用方或用户应该怎么改。{STYLE}",
            entry = json(payload.get("entry").unwrap_or(&Value::Null)),
        ),
        "proxy" => format!(
            "你是 Windows 网络代理排错助手。下面是这台机器的代理状态和一次连通性测试（JSON）：\n{state}\n\n\
             请判断连不上卡在哪（系统代理没开、代理软件没监听、某个程序没走代理、目标被墙、证书等），\
             并给出排查顺序。{STYLE}",
            state = json(payload),
        ),
        "lan_address" => format!(
            "你是网络顾问。这台 Windows 电脑在局域网里开了一个 HTTP 接口服务，端口 {port}。\
             下面是它的全部地址和对应网卡（JSON）：\n{addresses}\n\n\
             请逐个说明每个地址来自什么网卡、哪种设备能用它访问（同一路由器下的手机电脑、本机虚拟机、\
             同一 Tailscale 网络的设备等），以及哪些基本用不上；最后给出一般情况下首选哪个。{STYLE}",
            port = field(payload, "port"),
            addresses = json(payload.get("addresses").unwrap_or(&Value::Null)),
        ),
        _ => return Err("E_AI_KIND".into()),
    };
    Ok(text)
}

#[tauri::command]
pub async fn ai_ask(kind: String, payload: Value) -> Result<String, String> {
    let prompt = prompt(&kind, &payload)?;
    tauri::async_runtime::spawn_blocking(move || crate::ai_config::complete_answer(&prompt))
        .await
        .map_err(|e| e.to_string())?
}

/// A search typed in plain words, turned into the session list's own filters.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionFilter {
    pub agent: String,
    pub project: String,
    pub search: String,
    pub days: u32,
    pub favorites_only: bool,
}

fn session_filter_prompt(query: &str, agents: &[String], projects: &[(String, String)]) -> String {
    let projects: Vec<String> = projects
        .iter()
        .take(200)
        .map(|(key, name)| format!("{key}\t{name}"))
        .collect();
    format!(
        "把用户找会话的一句话转换成筛选条件。只输出一个 JSON 对象，不要任何其他文字。\n\
         字段：agent（下面列表里的一个 id，不确定就空字符串）、project（下面列表里的 key，不确定就空）、\
         search（标题或摘要里的关键词，尽量短，没有就空）、days（最近多少天，没说就 0）、favoritesOnly（是否只看收藏）。\n\
         可选 agent：{agents}\n可选项目（key\\t名称）：\n{projects}\n\n用户的话：{query}",
        agents = agents.join(", "),
        projects = projects.join("\n"),
    )
}

/// The first JSON object in a reply, since a model sometimes wraps it in words or a fence.
fn first_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

pub fn parse_session_filter(
    text: &str,
    agents: &[String],
    projects: &[(String, String)],
) -> Result<SessionFilter, String> {
    let object = first_object(text).ok_or("E_AI_REPLY")?;
    let mut filter: SessionFilter = serde_json::from_str(object).map_err(|_| "E_AI_REPLY")?;
    // Only what the page can actually filter by survives: an invented agent or project is
    // dropped rather than producing a list that is mysteriously empty.
    if !agents.contains(&filter.agent) {
        filter.agent.clear();
    }
    if !projects.iter().any(|(key, _)| key == &filter.project) {
        filter.project.clear();
    }
    filter.search = filter.search.chars().take(60).collect();
    filter.days = filter.days.min(3650);
    Ok(filter)
}

#[tauri::command]
pub async fn ai_session_filter(
    query: String,
    agents: Vec<String>,
    projects: Vec<(String, String)>,
) -> Result<SessionFilter, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let reply = crate::ai_config::complete(&session_filter_prompt(&query, &agents, &projects))?;
        parse_session_filter(&reply, &agents, &projects)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A search of the vault typed in plain words, turned into its list's filters. Only the
/// words and the platform names go out — never a title, an account or a value.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct VaultFilter {
    pub search: String,
    /// "", "web", "general" or "ssh".
    pub kind: String,
    pub platform: String,
    /// "", "on" or "off".
    pub windows: String,
    pub soon_only: bool,
}

fn vault_filter_prompt(query: &str, platforms: &[String]) -> String {
    let platforms: Vec<&str> = platforms.iter().take(300).map(String::as_str).collect();
    format!(
        "把用户在密码保管库里找条目的一句话转换成筛选条件。只输出一个 JSON 对象，不要任何其他文字。\n\
         字段：search（标题、网址、账号、标签或备注里会出现的关键词，尽量短，没有就空）、\
         kind（web=网站密码，general=API 密钥、令牌等通用密钥，ssh=SSH 密钥，没说就空）、\
         platform（下面列表里的一个，不确定就空）、windows（on=已放进 Windows 凭据管理器，off=没放进，没说就空）、\
         soonOnly（是否只看 14 天内到期或已到期的）。\n\
         可选平台：\n{platforms}\n\n用户的话：{query}",
        platforms = platforms.join("\n"),
    )
}

pub fn parse_vault_filter(text: &str, platforms: &[String]) -> Result<VaultFilter, String> {
    let object = first_object(text).ok_or("E_AI_REPLY")?;
    let mut filter: VaultFilter = serde_json::from_str(object).map_err(|_| "E_AI_REPLY")?;
    if !["web", "general", "ssh"].contains(&filter.kind.as_str()) {
        filter.kind.clear();
    }
    if !platforms.contains(&filter.platform) {
        filter.platform.clear();
    }
    if !["on", "off"].contains(&filter.windows.as_str()) {
        filter.windows.clear();
    }
    filter.search = filter.search.chars().take(60).collect();
    Ok(filter)
}

#[tauri::command]
pub async fn ai_vault_filter(query: String, platforms: Vec<String>) -> Result<VaultFilter, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let reply = crate::ai_config::complete(&vault_filter_prompt(&query, &platforms))?;
        parse_vault_filter(&reply, &platforms)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One filter a list offers, as the page describes it to the AI.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterField {
    pub name: String,
    /// What it means, in words the model reads.
    pub meaning: String,
    /// "text", "number", "bool" or "choice".
    pub kind: String,
    #[serde(default)]
    pub options: Vec<String>,
}

fn list_filter_prompt(list: &str, fields: &[FilterField], query: &str) -> String {
    let lines: Vec<String> = fields
        .iter()
        .take(12)
        .map(|field| {
            let options: Vec<&str> = field.options.iter().take(200).map(String::as_str).collect();
            match field.kind.as_str() {
                "choice" => format!(
                    "- {}：{}；只能是下列之一，不确定就空字符串：{}",
                    field.name,
                    field.meaning,
                    options.join(" | ")
                ),
                "number" => format!("- {}：{}；数字，没说就 0", field.name, field.meaning),
                "bool" => format!(
                    "- {}：{}；true 或 false，没说就 false",
                    field.name, field.meaning
                ),
                _ => format!(
                    "- {}：{}；尽量短的关键词，没有就空字符串",
                    field.name, field.meaning
                ),
            }
        })
        .collect();
    format!(
        "把用户在「{list}」里找东西的一句话转换成筛选条件。只输出一个 JSON 对象，不要任何其他文字。\n\
         字段：\n{fields}\n\n用户的话：{query}",
        fields = lines.join("\n"),
    )
}

/// Only what the list can filter by survives: a field it does not have, a choice it does not
/// offer, or a value of the wrong kind is dropped rather than giving a mysteriously empty list.
pub fn parse_list_filter(text: &str, fields: &[FilterField]) -> Result<Value, String> {
    let object = first_object(text).ok_or("E_AI_REPLY")?;
    let reply: Value = serde_json::from_str(object).map_err(|_| "E_AI_REPLY")?;
    let mut out = serde_json::Map::new();
    for field in fields {
        let value = reply.get(&field.name);
        let kept = match field.kind.as_str() {
            "choice" => value
                .and_then(Value::as_str)
                .filter(|v| field.options.iter().any(|o| o == v))
                .map(|v| Value::from(v.to_string()))
                .unwrap_or_else(|| Value::from("")),
            "number" => value
                .and_then(|v| {
                    v.as_f64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(Value::from)
                .unwrap_or_else(|| Value::from(0)),
            "bool" => Value::from(value.and_then(Value::as_bool).unwrap_or(false)),
            _ => Value::from(
                value
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(60)
                    .collect::<String>(),
            ),
        };
        out.insert(field.name.clone(), kept);
    }
    Ok(Value::Object(out))
}

#[tauri::command]
pub async fn ai_list_filter(
    list: String,
    fields: Vec<FilterField>,
    query: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let reply = crate::ai_config::complete(&list_filter_prompt(&list, &fields, &query))?;
        parse_list_filter(&reply, &fields)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── What changed in an update, from the real release notes only ─────────────────────────

fn agent(timeout: Duration) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new().timeout(timeout);
    if let Some(proxy) =
        crate::agents::net::stacker_proxy().and_then(|url| ureq::Proxy::new(url).ok())
    {
        builder = builder.proxy(proxy);
    }
    builder.build()
}

fn get_json(url: &str) -> Option<Value> {
    let mut request = agent(Duration::from_secs(15))
        .get(url)
        .set("User-Agent", "Stacker");
    // GitHub's media type only for GitHub: the npm registry answers it with nothing usable.
    if url.starts_with("https://api.github.com/") {
        request = request.set("Accept", "application/vnd.github+json");
    }
    let text = request.call().ok()?.into_string().ok()?;
    serde_json::from_str(&text).ok()
}

/// `owner/repo` out of whatever an npm package's `repository` field holds.
pub fn github_repo(url: &str) -> Option<String> {
    let rest = url.split("github.com").nth(1)?;
    let path = rest.trim_start_matches([':', '/']).trim_end_matches(".git");
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    Some(format!("{}/{}", parts.next()?, parts.next()?))
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// Release notes between the installed version (exclusive) and the latest (inclusive).
fn github_notes(repo: &str, current: &str, latest: &str) -> Option<String> {
    let releases = get_json(&format!(
        "https://api.github.com/repos/{repo}/releases?per_page=15"
    ))?;
    let (low, high) = (version_key(current), version_key(latest));
    let mut notes = Vec::new();
    for release in releases.as_array()? {
        // A pre-release is not what an update installs.
        if release["prerelease"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag = release["tag_name"].as_str().unwrap_or("");
        let key = version_key(tag);
        if key.is_empty() || key <= low || (!high.is_empty() && key > high) {
            continue;
        }
        let body = release["body"].as_str().unwrap_or("").trim();
        if !body.is_empty() {
            notes.push(format!("## {tag}\n{body}"));
        }
    }
    (!notes.is_empty()).then(|| notes.join("\n\n"))
}

/// Products whose notes live in a CHANGELOG.md and whose npm package does not say where.
const CHANGELOGS: &[(&str, &str)] = &[(
    "claude",
    "https://raw.githubusercontent.com/anthropics/claude-code/HEAD/CHANGELOG.md",
)];

fn get_text(url: &str) -> Option<String> {
    agent(Duration::from_secs(15))
        .get(url)
        .set("User-Agent", "Stacker")
        .call()
        .ok()?
        .into_string()
        .ok()
}

/// The sections of a CHANGELOG.md whose `## <version>` heading lies in the update's range.
pub fn changelog_sections(markdown: &str, current: &str, latest: &str) -> Option<String> {
    let (low, high) = (version_key(current), version_key(latest));
    let mut kept = Vec::new();
    for section in markdown.split("\n## ").skip(1) {
        let heading = section.lines().next().unwrap_or("");
        let key = version_key(heading);
        if key.is_empty() || key <= low || (!high.is_empty() && key > high) {
            continue;
        }
        kept.push(format!("## {}", section.trim_end()));
    }
    (!kept.is_empty()).then(|| kept.join("\n\n"))
}

/// Where the notes come from: a known CHANGELOG.md, else the npm package's GitHub releases,
/// else that repository's CHANGELOG.md.
fn release_notes(
    tool_id: &str,
    npm_package: Option<&str>,
    current: &str,
    latest: &str,
) -> Option<String> {
    if let Some((_, url)) = CHANGELOGS.iter().find(|(id, _)| *id == tool_id) {
        return changelog_sections(&get_text(url)?, current, latest);
    }
    // The `latest` document is a few KB; the full one runs to megabytes.
    let meta = get_json(&format!(
        "https://registry.npmjs.org/{}/latest",
        npm_package?
    ))?;
    let repo_url = meta["repository"]["url"]
        .as_str()
        .or_else(|| meta["repository"].as_str())?;
    let repo = github_repo(repo_url)?;
    github_notes(&repo, current, latest).or_else(|| {
        let text = get_text(&format!(
            "https://raw.githubusercontent.com/{repo}/HEAD/CHANGELOG.md"
        ))?;
        changelog_sections(&text, current, latest)
    })
}

#[tauri::command]
pub async fn ai_update_notes(
    tool_id: String,
    product: String,
    current: String,
    latest: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let npm_package = crate::agents::registry::cli_by_id(&tool_id).and_then(|spec| spec.npm_package);
        // Without the real notes the model would be guessing, and a guess about what an
        // update changes is worse than no answer.
        let notes = release_notes(&tool_id, npm_package, &current, &latest).ok_or("E_AI_NO_NOTES")?;
        let prompt = format!(
            "下面是 {product} 从 {current} 升到 {latest} 之间的官方更新日志。\n{notes}\n\n\
             请用简体中文、不超过 200 字总结：主要改了什么（挑用户能感知的）、有没有破坏性变化或需要注意的地方，\
             最后一句话给出建议：建议升级 / 可以等等 / 有风险先别升。只根据日志内容，不要补充日志里没有的东西。",
            notes = tail(&notes, 12_000),
        );
        crate::ai_config::complete_answer(&prompt)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── A connectivity probe, so the proxy diagnosis has facts to reason from ────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub url: String,
    /// Straight out, ignoring every proxy.
    pub direct: String,
    /// Through the Windows system proxy, when there is one.
    pub via_proxy: Option<String>,
    pub proxy: Option<String>,
}

fn probe_once(url: &str, proxy: Option<&str>) -> String {
    let mut builder = ureq::AgentBuilder::new().timeout(Duration::from_secs(8));
    if let Some(proxy) = proxy.and_then(|p| ureq::Proxy::new(format!("http://{p}")).ok()) {
        builder = builder.proxy(proxy);
    }
    let started = std::time::Instant::now();
    match builder.build().head(url).call() {
        Ok(response) => format!(
            "HTTP {} ({} ms)",
            response.status(),
            started.elapsed().as_millis()
        ),
        Err(ureq::Error::Status(status, _)) => {
            format!("HTTP {status} ({} ms)", started.elapsed().as_millis())
        }
        Err(error) => format!("失败：{error}"),
    }
}

#[tauri::command]
pub async fn proxy_probe(url: String) -> Result<ProbeResult, String> {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("E_PROBE_URL".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let proxy = crate::settings::detected_proxy_addr().map(|(h, p)| format!("{h}:{p}"));
        Ok(ProbeResult {
            direct: probe_once(&url, None),
            via_proxy: proxy.as_deref().map(|p| probe_once(&url, Some(p))),
            proxy,
            url,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_prompt_and_an_unknown_one_is_refused() {
        for kind in [
            "install_failure",
            "checkup",
            "toolchain",
            "disk_directory",
            "skipped_path",
            "gateway_error",
            "proxy",
        ] {
            let text = prompt(
                kind,
                &serde_json::json!({ "log": "boom", "items": [1], "entry": {} }),
            )
            .unwrap();
            assert!(!text.is_empty(), "{kind}");
        }
        assert_eq!(prompt("nonsense", &Value::Null).unwrap_err(), "E_AI_KIND");
    }

    #[test]
    fn a_long_log_keeps_its_end() {
        let log = format!("{}THE-ERROR", "x".repeat(MAX_BLOCK * 2));
        let text = prompt("install_failure", &serde_json::json!({ "log": log })).unwrap();
        assert!(text.contains("THE-ERROR"));
        assert!(text.contains("前面省略"));
        assert!(text.chars().count() < MAX_BLOCK + 1_000);
    }

    #[test]
    fn a_filter_keeps_only_what_the_list_can_filter_by() {
        let agents = vec!["codex".to_string(), "claude".to_string()];
        let projects = vec![("p1".to_string(), "Stacker".to_string())];
        let reply = "好的：```json\n{\"agent\":\"claude\",\"project\":\"p1\",\"search\":\"代理\",\"days\":7}\n```";
        let filter = parse_session_filter(reply, &agents, &projects).unwrap();
        assert_eq!(filter.agent, "claude");
        assert_eq!(filter.project, "p1");
        assert_eq!(filter.search, "代理");
        assert_eq!(filter.days, 7);
        // An agent or project the model made up is dropped, not passed on.
        let invented =
            parse_session_filter(r#"{"agent":"gemini","project":"nope"}"#, &agents, &projects)
                .unwrap();
        assert!(invented.agent.is_empty() && invented.project.is_empty());
        assert_eq!(
            parse_session_filter("no json here", &agents, &projects).unwrap_err(),
            "E_AI_REPLY"
        );
    }

    #[test]
    fn a_vault_filter_keeps_only_what_the_list_can_filter_by() {
        let platforms = vec!["GitHub".to_string(), "阿里云".to_string()];
        let reply = r#"```json
{"search":"root","kind":"web","platform":"阿里云","windows":"on","soonOnly":true}
```"#;
        let filter = parse_vault_filter(reply, &platforms).unwrap();
        assert_eq!(filter.search, "root");
        assert_eq!(filter.kind, "web");
        assert_eq!(filter.platform, "阿里云");
        assert_eq!(filter.windows, "on");
        assert!(filter.soon_only);
        let invented = parse_vault_filter(
            r#"{"kind":"database","platform":"AWS","windows":"maybe"}"#,
            &platforms,
        )
        .unwrap();
        assert!(
            invented.kind.is_empty() && invented.platform.is_empty() && invented.windows.is_empty()
        );
        // What goes out is the words and the platform names, nothing else.
        let prompt = vault_filter_prompt("阿里云的 root", &platforms);
        assert!(prompt.contains("阿里云的 root") && prompt.contains("GitHub"));
    }

    #[test]
    fn a_list_filter_keeps_only_the_fields_choices_and_kinds_the_list_has() {
        let field = |name: &str, kind: &str, options: &[&str]| FilterField {
            name: name.into(),
            meaning: "m".into(),
            kind: kind.into(),
            options: options.iter().map(|o| o.to_string()).collect(),
        };
        let fields = vec![
            field("search", "text", &[]),
            field("outcome", "choice", &["ok", "error"]),
            field("hours", "number", &[]),
            field("git", "bool", &[]),
        ];
        let reply = "```json\n{\"search\":\"502\",\"outcome\":\"error\",\"hours\":\"24\",\"git\":true,\"extra\":1}\n```";
        let got = parse_list_filter(reply, &fields).unwrap();
        assert_eq!(
            got,
            serde_json::json!({"search":"502","outcome":"error","hours":24.0,"git":true})
        );
        let invented = parse_list_filter(r#"{"outcome":"maybe","hours":-3}"#, &fields).unwrap();
        assert_eq!(invented["outcome"], "");
        assert_eq!(invented["hours"], 0);
        assert_eq!(invented["search"], "");
        let prompt = list_filter_prompt("请求记录", &fields, "昨天失败的");
        assert!(prompt.contains("ok | error") && prompt.contains("昨天失败的"));
    }

    #[test]
    fn a_repository_url_names_its_github_repo() {
        assert_eq!(
            github_repo("git+https://github.com/openai/codex.git").as_deref(),
            Some("openai/codex")
        );
        assert_eq!(
            github_repo("git@github.com:anthropics/claude-code.git").as_deref(),
            Some("anthropics/claude-code")
        );
        assert!(github_repo("https://gitlab.com/a/b").is_none());
    }

    /// Live, read-only: `cargo test --lib live_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_probe() {
        let proxy = crate::settings::detected_proxy_addr().map(|(h, p)| format!("{h}:{p}"));
        println!("proxy {proxy:?}");
        println!("direct    {}", probe_once("https://api.openai.com", None));
        println!(
            "via proxy {}",
            probe_once("https://api.openai.com", proxy.as_deref())
        );
    }

    /// Live, read-only: `cargo test --lib live_release_notes -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_release_notes() {
        for (id, package, current, latest) in [
            ("codex", Some("@openai/codex"), "0.158.0", "0.159.2"),
            (
                "claude",
                Some("@anthropic-ai/claude-code"),
                "2.1.282",
                "2.1.285",
            ),
        ] {
            let notes = release_notes(id, package, current, latest);
            println!(
                "== {id}: {:?} chars
{}
",
                notes.as_ref().map(|n| n.chars().count()),
                notes
                    .as_deref()
                    .unwrap_or("(none)")
                    .chars()
                    .take(300)
                    .collect::<String>()
            );
        }
    }

    #[test]
    fn a_changelog_gives_only_the_sections_in_the_update() {
        let md = "# Changelog\n\n## 2.1.285\n- newest\n\n## 2.1.284\n- middle\n\n## 2.1.283\n- installed\n";
        let notes = changelog_sections(md, "2.1.283", "2.1.285").unwrap();
        assert!(notes.contains("newest") && notes.contains("middle"));
        assert!(!notes.contains("installed"));
        assert!(changelog_sections(md, "2.1.285", "2.1.285").is_none());
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(version_key("v2.1.10") > version_key("2.1.9"));
        assert!(version_key("rust-v0.158.0") > version_key("0.155.1"));
        assert_eq!(version_key("1.0.0"), vec![1, 0, 0]);
    }
}
