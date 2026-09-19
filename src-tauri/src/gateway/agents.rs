//! Per-agent view of the API service: installed, signed in, models, efforts, on/off, test.
use super::{live_defaults, load, protocol, save, GatewayConfig, Shared, STATE};
use crate::runner::login::LoginStatus;
use crate::sessions::model::Agent;
use serde::Serialize;

/// Agents the service can run today (verified stateless and tool-less).
pub const SUPPORTED: [Agent; 2] = [Agent::Codex, Agent::Claude];

pub fn apply_enabled(shared: &Shared, config: &GatewayConfig) {
    if let Ok(mut enabled) = shared.enabled.lock() {
        *enabled = SUPPORTED
            .into_iter()
            .filter(|a| !config.disabled_agents.iter().any(|d| d == a.as_str()))
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
    pub models: Vec<AgentModel>,
}

/// Every agent CLI Stacker knows, with what the API service can do with it.
pub fn agent_cards() -> Vec<AgentCard> {
    let config = load();
    let settings = crate::sessions::annotations::connect()
        .map(|c| crate::sessions::summary::load_settings(&c))
        .unwrap_or_default();
    let codex_home = crate::sessions::roots::resolve(&Default::default()).codex;
    let options = crate::runner::options::options(std::path::Path::new(&codex_home));
    let mut seen = std::collections::HashSet::new();
    let mut cards = Vec::new();
    for tool in crate::agents::last_scan_or_scan() {
        let Some(cli_id) = tool.cli_id.clone() else {
            continue;
        };
        if !seen.insert(cli_id.clone()) {
            continue;
        }
        let agent = SUPPORTED.into_iter().find(|a| a.as_str() == cli_id);
        let installed = tool.cli.installed;
        let reason = match (agent, installed) {
            (_, false) => "未安装",
            (None, true) => "尚未验证能否在不留会话、不开放工具的前提下调用，暂未接入",
            (Some(_), true) => "",
        };
        let mut card = AgentCard {
            id: cli_id.clone(),
            name: tool.cli.label.clone(),
            installed,
            version: tool.cli.version.clone(),
            supported: agent.is_some() && installed,
            reason: reason.into(),
            login: None,
            enabled: false,
            default_model: None,
            default_effort: None,
            models: Vec::new(),
        };
        if let Some(agent) = agent.filter(|_| installed) {
            let choice = crate::sessions::summary::choice_for(&settings, agent);
            card.enabled = !config.disabled_agents.contains(&cli_id);
            card.login = Some(crate::runner::login::login_status(agent));
            card.default_model = choice.model;
            card.default_effort = choice.effort;
            card.models = options
                .iter()
                .find(|o| o.agent == agent)
                .map(|o| {
                    o.models
                        .iter()
                        .map(|m| AgentModel {
                            call: format!("{}/{}", agent.as_str(), m.id),
                            label: m.label.clone(),
                            efforts: m.efforts.clone(),
                            default_effort: m.default_effort.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        cards.push(card);
    }
    cards.sort_by_key(|c| (!c.supported, !c.installed));
    cards
}

#[tauri::command]
pub async fn gateway_agents() -> Vec<AgentCard> {
    tauri::async_runtime::spawn_blocking(agent_cards)
        .await
        .unwrap_or_default()
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
        let agent = SUPPORTED
            .into_iter()
            .find(|a| a.as_str() == agent)
            .ok_or("E_REQUEST")?;
        let chat = protocol::ChatRequest {
            model_name: agent.as_str().into(),
            model: protocol::ModelSpec {
                agent,
                model: model.filter(|m| !m.is_empty()),
            },
            system: String::new(),
            turns: vec![(protocol::Role::User, "Reply with exactly: OK".into())],
            stream: false,
            effort: None,
        };
        let (model, effort) = live_defaults()(&chat);
        let started = std::time::Instant::now();
        let result = crate::runner::run(
            &crate::runner::RunRequest {
                agent,
                model: model.clone(),
                effort: effort.clone(),
                prompt: protocol::render_prompt(&chat),
                timeout: crate::runner::DEFAULT_TIMEOUT,
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
