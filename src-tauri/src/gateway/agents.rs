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
        stream: false,
        effort: None,
    };
    live_defaults()(&chat)
}

/// Every agent CLI Stacker knows, with what the API service can do with it.
pub fn agent_cards() -> Vec<AgentCard> {
    let config = load();
    let mut seen = std::collections::HashSet::new();
    let mut cards = Vec::new();
    for tool in crate::agents::last_scan_or_scan() {
        let Some(cli_id) = tool.cli_id.clone() else {
            continue;
        };
        if !seen.insert(cli_id.clone()) {
            continue;
        }
        let backend = backends::get(&cli_id);
        let installed = tool.cli.installed;
        let mut card = AgentCard {
            id: cli_id.clone(),
            name: tool.cli.label.clone(),
            installed,
            version: tool.cli.version.clone(),
            supported: false,
            reason: String::new(),
            login: None,
            enabled: false,
            default_model: None,
            default_effort: None,
            efforts: Vec::new(),
            models: Vec::new(),
        };
        match (backend, installed) {
            (_, false) => card.reason = "未安装".into(),
            (None, true) => {
                card.reason = "尚未验证能否在不留会话、不开放工具的前提下调用，暂未接入".into()
            }
            (Some(b), true) => {
                let login = (b.login)();
                if login.state == "logged_out" {
                    card.reason = "未登录：请在终端运行该智能体并完成登录".into();
                }
                card.supported = login.state != "logged_out";
                card.login = Some(login);
                card.enabled = card.supported && !config.disabled_agents.contains(&cli_id);
                let (model, effort) = defaults_for(b.id);
                card.default_model = model;
                card.default_effort = effort;
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
        let backend = backends::get(&agent).ok_or("E_REQUEST")?;
        let chat = protocol::ChatRequest {
            model_name: backend.id.into(),
            model: protocol::ModelSpec {
                backend: backend.id.into(),
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
                backend: backend.id.into(),
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
