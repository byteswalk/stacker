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
        "disk_batch" => format!(
            "你是 Windows 开发机的磁盘清理顾问。下面是占用最大的几个目录（路径和大小）：\n{items}\n\n\
             逐个用一行说明：这个目录属于什么工具、存的是什么、能不能删（可以放心删 / 删前确认 / 不要删）。\
             格式：`路径 —— 说明 —— 建议`。只凭路径判断，看不出来的就写“看不出来”。",
            items = json(payload.get("items").unwrap_or(&Value::Null)),
        ),
        "gateway_error" => format!(
            "你是一个本机 AI 接口服务的排错助手。这个服务把本机已登录的 Codex / Claude 等命令行包装成 \
             OpenAI / Anthropic 风格的接口。记录里只有状态码，没有报错原文；这个服务的状态码含义如下：\n\
             {GATEWAY_STATUSES}\n\n下面是一条失败的请求记录（JSON）：\n{entry}\n\n\
             请根据状态码、接口和模型说明最可能的原因，以及调用方或用户应该怎么改。{STYLE}",
            entry = json(payload.get("entry").unwrap_or(&Value::Null)),
        ),
        "proxy" => format!(
            "你是 Windows 网络代理排错助手。下面是这台机器的代理状态和一次连通性测试（JSON）：\n{state}\n\n\
             请判断连不上卡在哪（系统代理没开、代理软件没监听、某个程序没走代理、目标被墙、证书等），\
             并给出排查顺序。{STYLE}",
            state = json(payload),
        ),
        _ => return Err("E_AI_KIND".into()),
    };
    Ok(text)
}

#[tauri::command]
pub async fn ai_ask(kind: String, payload: Value) -> Result<String, String> {
    let prompt = prompt(&kind, &payload)?;
    tauri::async_runtime::spawn_blocking(move || crate::ai_config::complete(&prompt))
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
        crate::ai_config::complete(&prompt)
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
            "disk_batch",
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
