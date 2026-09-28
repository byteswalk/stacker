//! Local-only OpenAI / Anthropic style gateway over the agent runner.
pub mod agents;
pub mod protocol;
pub mod requests;
pub mod server;

use serde::{Deserialize, Serialize};
use server::{Defaults, LogEntry, Runner, Running, Shared};
use std::sync::{Arc, Mutex};

pub const DEFAULT_PORT: u16 = 8765;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewayConfig {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    /// Agents turned off on the page (missing = on).
    pub disabled_agents: Vec<String>,
    /// What a request that names only the agent runs with. Without an entry the CLI decides,
    /// which is a value Stacker cannot read or show, so the page lets the user pick one.
    #[serde(default)]
    pub agent_defaults: Vec<AgentDefault>,
    /// Whether requests are written to the log at all.
    #[serde(default = "yes")]
    pub log_enabled: bool,
    /// Days of history kept; 0 keeps everything until the user clears it.
    #[serde(default = "default_retention")]
    pub log_retention_days: u32,
    /// Whether the service answers other machines on the network as well as this one.
    #[serde(default)]
    pub lan_access: bool,
}

fn yes() -> bool {
    true
}

fn default_retention() -> u32 {
    7
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentDefault {
    pub agent: String,
    /// The model id as the CLI names it, without the `<agent>/` prefix.
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: String::new(),
            disabled_agents: Vec::new(),
            agent_defaults: Vec::new(),
            log_enabled: true,
            log_retention_days: default_retention(),
            lan_access: false,
        }
    }
}

fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let part = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.finish()
    };
    format!("sk-stacker-{:016x}{:016x}", part(), part())
}

pub(crate) fn load() -> GatewayConfig {
    let mut config: GatewayConfig = crate::sessions::annotations::connect()
        .ok()
        .and_then(|c| crate::sessions::annotations::setting(&c, "gateway"))
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    if config.token.is_empty() {
        config.token = new_token();
        let _ = save(&config);
    }
    if config.port == 0 {
        config.port = DEFAULT_PORT;
    }
    config
}

pub(crate) fn save(config: &GatewayConfig) -> Result<(), String> {
    let conn = crate::sessions::annotations::connect()?;
    crate::sessions::annotations::set_setting(
        &conn,
        "gateway",
        &serde_json::to_string(config).map_err(crate::sessions::err)?,
    )
}

pub(crate) struct State {
    pub(crate) running: Option<(Running, Arc<Shared>)>,
    error: String,
}

pub(crate) static STATE: Mutex<State> = Mutex::new(State {
    running: None,
    error: String::new(),
});

fn live_runner() -> Runner {
    Arc::new(crate::runner::run)
}

/// Codex and Claude fall back to the summary settings' model and effort; other backends
/// to the CLI's own defaults.
pub(crate) fn live_defaults() -> Defaults {
    Arc::new(|chat: &protocol::ChatRequest| {
        // What the user picked on the page wins; Codex and Claude otherwise follow the
        // summary settings, and anything else leaves the choice to the CLI.
        let picked = load()
            .agent_defaults
            .into_iter()
            .find(|d| d.agent == chat.model.backend);
        if let Some(picked) = picked {
            if picked.model.is_some() || picked.effort.is_some() {
                return (
                    chat.model.model.clone().or(picked.model),
                    chat.effort.clone().or(picked.effort),
                );
            }
        }
        let agent = match chat.model.backend.as_str() {
            "codex" => Some(crate::sessions::model::Agent::Codex),
            "claude" => Some(crate::sessions::model::Agent::Claude),
            _ => None,
        };
        let (model, effort) = match agent {
            Some(agent) => {
                let settings = crate::sessions::annotations::connect()
                    .map(|c| crate::sessions::summary::load_settings(&c))
                    .unwrap_or_default();
                let base = crate::sessions::summary::choice_for(&settings, agent);
                (base.model, base.effort)
            }
            None => (None, None),
        };
        (
            chat.model.model.clone().or(model),
            chat.effort.clone().or(effort),
        )
    })
}

fn stop_locked(state: &mut State) {
    if let Some((running, _)) = state.running.take() {
        running.stop();
    }
}

fn start_locked(state: &mut State, config: &GatewayConfig) {
    stop_locked(state);
    state.error.clear();
    let shared = Shared::new(config.token.clone(), live_runner(), live_defaults());
    agents::apply_enabled(&shared, config);
    match server::start(config.port, config.lan_access, shared.clone()) {
        Ok(running) => {
            log::info!(target: "stacker::gateway", "listening on 127.0.0.1:{}", running.port);
            state.running = Some((running, shared));
        }
        Err(code) => state.error = code,
    }
}

/// Startup: resume the gateway if it was on.
pub fn restore() {
    let config = load();
    if config.enabled {
        if let Ok(mut state) = STATE.lock() {
            start_locked(&mut state, &config);
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub token: String,
    pub error: String,
    pub recent: Vec<LogEntry>,
    /// Whether requests are written to the log, and for how long they are kept.
    pub log_enabled: bool,
    pub log_retention_days: u32,
    /// Whether the service answers the rest of the network.
    pub lan_access: bool,
    /// Every address the service can be reached at, this machine's first.
    pub addresses: Vec<String>,
}

/// This machine's addresses on the networks it is attached to. Asking a UDP socket where it
/// would send from names the interface that actually carries traffic, without sending
/// anything; the rest come from the adapters Windows lists.
pub fn lan_addresses() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut add = |address: String| {
        if !address.is_empty() && !found.contains(&address) {
            found.push(address);
        }
    };
    for probe in ["223.5.5.5:53", "8.8.8.8:53"] {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if socket.connect(probe).is_ok() {
                if let Ok(local) = socket.local_addr() {
                    let ip = local.ip().to_string();
                    if !ip.starts_with("127.") && ip != "0.0.0.0" {
                        add(ip);
                    }
                }
            }
        }
    }
    add(hostname());
    found
}

/// The name other machines can use instead of the address, when Windows gives one.
fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .map(|name| name.trim().to_lowercase())
        .map(|name| {
            if name.is_empty() {
                name
            } else {
                format!("{name}.local")
            }
        })
        .unwrap_or_default()
}

pub fn status() -> GatewayStatus {
    let config = load();
    let state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    GatewayStatus {
        enabled: config.enabled,
        running: state.running.is_some(),
        port: state
            .running
            .as_ref()
            .map(|(r, _)| r.port)
            .unwrap_or(config.port),
        log_enabled: config.log_enabled,
        log_retention_days: config.log_retention_days,
        lan_access: config.lan_access,
        addresses: if config.lan_access {
            lan_addresses()
        } else {
            Vec::new()
        },
        token: config.token,
        error: state.error.clone(),
        recent: state
            .running
            .as_ref()
            .and_then(|(_, s)| s.recent.lock().ok().map(|r| r.iter().cloned().collect()))
            .unwrap_or_default(),
    }
}

#[tauri::command]
pub fn gateway_status() -> GatewayStatus {
    status()
}

#[tauri::command]
pub async fn gateway_set(enabled: bool, port: u16) -> Result<GatewayStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if port < 1024 {
            return Err("E_PORT".to_string());
        }
        let mut config = load();
        config.enabled = enabled;
        config.port = port;
        save(&config)?;
        {
            let mut state = STATE.lock().map_err(crate::sessions::err)?;
            if enabled {
                start_locked(&mut state, &config);
            } else {
                stop_locked(&mut state);
                state.error.clear();
            }
        }
        Ok(status())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opening the service to the network restarts it on the other address; the token stays
/// the same, because it is what keeps the service the user's own.
#[tauri::command]
pub async fn gateway_set_lan(enabled: bool) -> Result<GatewayStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut config = load();
        config.lan_access = enabled;
        save(&config)?;
        {
            let mut state = STATE.lock().map_err(crate::sessions::err)?;
            if config.enabled {
                stop_locked(&mut state);
                start_locked(&mut state, &config);
            }
        }
        Ok(status())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn gateway_new_token() -> Result<GatewayStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut config = load();
        config.token = new_token();
        save(&config)?;
        let mut state = STATE.lock().map_err(crate::sessions::err)?;
        if state.running.is_some() {
            start_locked(&mut state, &config);
        }
        drop(state);
        Ok(status())
    })
    .await
    .map_err(|e| e.to_string())?
}
