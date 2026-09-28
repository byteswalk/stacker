//! Per-agent view of the API service: installed, signed in, models, efforts, on/off, test.
use super::{live_defaults, load, protocol, save, GatewayConfig, Shared, STATE};
use crate::runner::backends;
use crate::runner::login::LoginStatus;
use serde::Serialize;

pub fn apply_enabled(shared: &Shared, config: &GatewayConfig) {
    if let Ok(mut enabled) = shared.enabled.lock() {
        *enabled = backends::all()
            .iter()
            .filter(|b| !config.disabled_agents.iter().any(|d| d == b.id))
            .map(|b| b.id.to_string())
            .collect();
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModel {
    /// What to put in the request's `model` field.
    pub call: String,
    pub label: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub id: String,
    pub name: String,
    /// Who makes it, for the line under the name.
    pub vendor: String,
    pub installed: bool,
    pub version: Option<String>,
    pub supported: bool,
    /// Why an agent cannot be used through the service.
    pub reason: String,
    pub login: Option<LoginStatus>,
    pub enabled: bool,
    /// Used when a request names only the agent.
    pub default_model: Option<String>,
    pub default_effort: Option<String>,
    /// Whether that default is the user's own choice rather than the CLI's.
    pub default_is_chosen: bool,
    /// Reasoning levels usable without naming a model.
    pub efforts: Vec<String>,
    pub models: Vec<AgentModel>,
}

fn defaults_for(id: &str) -> (Option<String>, Option<String>) {
    let chat = protocol::ChatRequest {
        model_name: id.into(),
        model: protocol::ModelSpec {
            backend: id.into(),
            model: None,
        },
        system: String::new(),
        turns: Vec::new(),
        attachments: Vec::new(),
        stream: false,
        effort: None,
    };
    live_defaults()(&chat)
}

/// Every agent CLI Stacker knows, with what the API service can do with it.
pub fn agent_cards() -> Vec<AgentCard> {
    let config = load();
    let mut seen = std::collections::HashSet::new();
    let tools: Vec<_> = crate::agents::last_scan_or_local_scan()
        .into_iter()
        .filter_map(|tool| Some((tool.cli_id.clone()?, tool)))
        .filter(|(cli_id, _)| seen.insert(cli_id.clone()))
        .collect();
    // Each agent's sign-in and model checks start its CLI, so the agents are checked at once.
    let mut cards: Vec<AgentCard> = std::thread::scope(|scope| {
        let handles: Vec<_> = tools
            .iter()
            .map(|(cli_id, tool)| scope.spawn(|| card_for(&config, cli_id, tool)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    cards.sort_by_key(|c| (!c.supported, !c.installed));
    cards
}

/// Why an installed CLI has no API backend. The API service only drives agents that sign in
/// with the vendor's own account, so the ones that need a pay-per-token key from an open
/// platform say so instead of looking unfinished.
fn not_wired(cli_id: &str) -> &'static str {
    match cli_id {
        "dsh" | "hermes" | "opencode" | "openclaw" | "pi" => {
            "需要自备第三方 API key，接口服务只接入用厂商账号登录的智能体"
        }
        "traecli" => "官方仅向 TRAE 企业版旗舰套餐开放，暂不接入",
        _ => "尚未验证能否在不留会话、不开放工具的前提下调用，暂未接入",
    }
}

fn card_for(config: &GatewayConfig, cli_id: &str, tool: &crate::agents::VibeTool) -> AgentCard {
    let backend = backends::get(cli_id);
    let installed = tool.cli.installed;
    let mut card = AgentCard {
        id: cli_id.to_string(),
        name: tool.cli.label.clone(),
        vendor: crate::agents::cli_vendor_label(cli_id).to_string(),
        installed,
        version: tool.cli.version.clone(),
        supported: false,
        reason: String::new(),
        login: None,
        enabled: false,
        default_model: None,
        default_effort: None,
        default_is_chosen: false,
        efforts: Vec::new(),
        models: Vec::new(),
    };
    match (backend, installed) {
        (_, false) => card.reason = "未安装".into(),
        (None, true) => card.reason = not_wired(cli_id).into(),
        (Some(b), true) => {
            let login = (b.login)();
            if login.state == "logged_out" {
                card.reason = "未登录：请在终端运行该智能体并完成登录".into();
            }
            card.supported = login.state != "logged_out";
            card.login = Some(login);
            card.enabled = card.supported && !config.disabled_agents.iter().any(|d| d == cli_id);
            let (model, effort) = defaults_for(b.id);
            card.default_model = model;
            card.default_effort = effort;
            card.default_is_chosen = config
                .agent_defaults
                .iter()
                .any(|d| d.agent == cli_id && (d.model.is_some() || d.effort.is_some()));
            card.efforts = (b.efforts)();
            card.models = (b.models)()
                .into_iter()
                .map(|m| AgentModel {
                    call: format!("{}/{}", b.id, m.id),
                    label: m.label,
                    efforts: m.efforts,
                    default_effort: m.default_effort,
                })
                .collect();
        }
    }
    card
}

#[tauri::command]
pub async fn gateway_agents() -> Vec<AgentCard> {
    tauri::async_runtime::spawn_blocking(agent_cards)
        .await
        .unwrap_or_default()
}

/// Saves what a request that names only this agent should run with.
#[tauri::command]
pub fn gateway_set_agent_default(
    agent: String,
    model: Option<String>,
    effort: Option<String>,
) -> Result<(), String> {
    let mut config = load();
    config.agent_defaults.retain(|d| d.agent != agent);
    let clean = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    let (model, effort) = (clean(model), clean(effort));
    if model.is_some() || effort.is_some() {
        config.agent_defaults.push(super::AgentDefault {
            agent,
            model,
            effort,
        });
    }
    save(&config)
}

/// The request log: what it holds, what a filter matches, and how long rows are kept.
#[tauri::command]
pub async fn gateway_log(
    query: super::requests::LogQuery,
) -> Result<super::requests::LogPage, String> {
    tauri::async_runtime::spawn_blocking(move || super::requests::list(&query))
        .await
        .map_err(crate::sessions::err)?
}

#[tauri::command]
pub fn gateway_log_remove(ids: Vec<i64>) -> Result<usize, String> {
    super::requests::remove(&ids)
}

/// Removes everything the filter matches; an empty filter empties the log.
#[tauri::command]
pub fn gateway_log_clear(query: super::requests::LogQuery) -> Result<usize, String> {
    super::requests::clear(&query)
}

#[tauri::command]
pub fn gateway_set_log(enabled: bool, retention_days: u32) -> Result<(), String> {
    let mut config = load();
    config.log_enabled = enabled;
    config.log_retention_days = retention_days;
    save(&config)?;
    if let Ok(conn) = crate::sessions::annotations::connect() {
        super::requests::prune(&conn, retention_days);
    }
    Ok(())
}

#[tauri::command]
pub fn gateway_set_agent(agent: String, enabled: bool) -> Result<(), String> {
    let mut config = load();
    config.disabled_agents.retain(|a| a != &agent);
    if !enabled {
        config.disabled_agents.push(agent);
    }
    save(&config)?;
    let state = STATE.lock().map_err(crate::sessions::err)?;
    if let Some((_, shared)) = state.running.as_ref() {
        apply_enabled(shared, &config);
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub reply: String,
    pub error: String,
    pub elapsed_ms: u64,
    pub model: String,
    pub effort: String,
}

/// Sends one short message through the same runner and defaults the service uses.
#[tauri::command]
pub async fn gateway_test(agent: String, model: Option<String>) -> Result<TestResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let backend = backends::get(&agent).ok_or("E_REQUEST")?;
        let chat = protocol::ChatRequest {
            model_name: backend.id.into(),
            model: protocol::ModelSpec {
                backend: backend.id.into(),
                model: model.filter(|m| !m.is_empty()),
            },
            system: String::new(),
            turns: vec![(protocol::Role::User, "Reply with exactly: OK".into())],
            attachments: Vec::new(),
            stream: false,
            effort: None,
        };
        let (model, effort) = live_defaults()(&chat);
        let started = std::time::Instant::now();
        let result = crate::runner::run(
            &crate::runner::RunRequest {
                backend: backend.id.into(),
                model: model.clone(),
                effort: effort.clone(),
                prompt: protocol::render_prompt(&chat),
                timeout: crate::runner::DEFAULT_TIMEOUT,
                attachments: Vec::new(),
                on_delta: None,
            },
            &crate::runner::CancelFlag::default(),
        );
        Ok(TestResult {
            ok: result.is_ok(),
            reply: result.as_ref().map(|o| o.text.clone()).unwrap_or_default(),
            error: result.err().unwrap_or_default(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            model: model.unwrap_or_default(),
            effort: effort.unwrap_or_default(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
