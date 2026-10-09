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

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: String::new(),
            disabled_agents: Vec::new(),
            log_enabled: true,
            log_retention_days: default_retention(),
            lan_access: false,
        }
    }
}

/// 32 bytes from the system's secure random source, as hex. (A hash of the clock is not a
/// secret; this key is all that stands between the LAN and the user's agent accounts.)
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    if getrandom::getrandom(&mut bytes).is_err() {
        // No secure randomness: a key nobody can guess is better than a weak one.
        use std::hash::{BuildHasher, Hasher};
        for chunk in bytes.chunks_mut(8) {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
            );
            chunk.copy_from_slice(&h.finish().to_le_bytes()[..chunk.len()]);
        }
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("sk-stacker-{hex}")
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

/// A request runs with the model and reasoning level it names; what it leaves out, the CLI
/// decides, as it would in a terminal.
pub(crate) fn live_defaults() -> Defaults {
    Arc::new(|chat: &protocol::ChatRequest| (chat.model.model.clone(), chat.effort.clone()))
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
    /// Every address the service can be reached at, the likeliest first.
    pub addresses: Vec<LanAddress>,
}

/// This machine's addresses on the networks it is attached to. Windows is asked for every
/// adapter that is up, because the one a caller needs is often not the one carrying the
/// default route: a virtual machine reaches its host through the host-only adapter, and a
/// second network card is just as valid an answer.
pub fn lan_addresses() -> Vec<String> {
    let mut found = adapter_addresses();
    // Whatever the adapter list missed, the interface the default route uses still answers.
    for probe in ["223.5.5.5:53", "8.8.8.8:53"] {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if socket.connect(probe).is_ok() {
                if let Ok(local) = socket.local_addr() {
                    let ip = local.ip().to_string();
                    if usable(&ip) && !found.contains(&ip) {
                        found.insert(0, ip);
                    }
                }
            }
        }
    }
    // Ordinary home and office networks first: they are what a caller on another machine
    // will recognise, ahead of a VPN's or a hypervisor's own range.
    found.sort_by_key(|ip| u8::from(!is_private_lan(ip)));
    let name = hostname();
    if !name.is_empty() {
        found.push(name);
    }
    found
}

/// One way another machine can reach the service, with the adapter it belongs to.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LanAddress {
    /// An IPv4 address or a host name.
    pub host: String,
    /// The adapter's name as Windows shows it; empty for the host name.
    pub adapter: String,
    /// `lan` (a real network card), `hostname`, `tailscale`, `vm` (a hypervisor's own
    /// network), `vpn` (any other virtual adapter) or `other`.
    pub kind: String,
}

/// What an adapter is, from its name and the driver's description. A hardware card is the
/// ordinary network; the rest are told apart by the software that made them.
pub fn adapter_kind(ip: &str, alias: &str, description: &str, hardware: bool) -> &'static str {
    let text = format!("{alias} {description}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&["tailscale"]) || in_cgnat(ip) && has(&["tunnel", "wintun"]) {
        "tailscale"
    } else if has(&[
        "vmware",
        "virtualbox",
        "hyper-v",
        "vethernet",
        "wsl",
        "vmnet",
        "parallels",
    ]) {
        "vm"
    } else if hardware && !has(&["virtual", "vpn", "tap-", "wintun", "wireguard"]) {
        "lan"
    } else if has(&[
        "vpn",
        "tap",
        "tun",
        "wireguard",
        "zerotier",
        "openvpn",
        "sangfor",
        "atrust",
        "easyconnect",
        "fortinet",
        "forticlient",
        "cisco anyconnect",
        "globalprotect",
        "virtual",
        "vnic",
    ]) {
        "vpn"
    } else {
        "other"
    }
}

/// 100.64.0.0/10, the shared range Tailscale and carrier NAT use.
fn in_cgnat(ip: &str) -> bool {
    let mut parts = ip.split('.').filter_map(|p| p.parse::<u8>().ok());
    matches!((parts.next(), parts.next()), (Some(100), Some(b)) if (64..128).contains(&b))
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "lan" => 0,
        "hostname" => 1,
        "tailscale" => 2,
        "other" => 3,
        "vm" => 4,
        _ => 5,
    }
}

/// The adapters behind each IPv4 address, as `Get-NetAdapter` describes them.
fn adapter_details() -> Vec<(String, String, String, bool)> {
    let script = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | ForEach-Object { $a = Get-NetAdapter -InterfaceIndex $_.InterfaceIndex -ErrorAction SilentlyContinue; '{0}|{1}|{2}|{3}' -f $_.IPAddress, $_.InterfaceAlias, $a.InterfaceDescription, $a.HardwareInterface }";
    crate::agents::process::run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        "Get-NetIPAddress",
        std::time::Duration::from_secs(10),
    )
    .map(|text| {
        text.lines()
            .filter_map(|line| {
                let mut parts = line.trim().splitn(4, '|');
                Some((
                    parts.next()?.to_string(),
                    parts.next()?.to_string(),
                    parts.next().unwrap_or("").to_string(),
                    parts
                        .next()
                        .is_some_and(|h| h.trim().eq_ignore_ascii_case("true")),
                ))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The addresses with what each one is, ordered by how likely a caller can use it: the real
/// network first, a VPN's own range last.
pub fn lan_entries() -> Vec<LanAddress> {
    let details = adapter_details();
    let mut entries: Vec<LanAddress> = lan_addresses()
        .into_iter()
        .map(|host| {
            if host.parse::<std::net::Ipv4Addr>().is_err() {
                return LanAddress {
                    host,
                    adapter: String::new(),
                    kind: "hostname".into(),
                };
            }
            match details.iter().find(|(ip, ..)| *ip == host) {
                Some((ip, alias, description, hardware)) => LanAddress {
                    kind: adapter_kind(ip, alias, description, *hardware).into(),
                    adapter: if description.is_empty() || description == alias {
                        alias.clone()
                    } else {
                        format!("{alias} · {description}")
                    },
                    host,
                },
                None => LanAddress {
                    kind: "other".into(),
                    adapter: String::new(),
                    host,
                },
            }
        })
        .collect();
    entries.sort_by_key(|entry| kind_rank(&entry.kind));
    entries
}

/// The page asks for the status every few seconds; adapters change far less often than that,
/// and asking Windows about them takes about a second.
fn cached_lan_entries() -> Vec<LanAddress> {
    static CACHE: Mutex<Option<(std::time::Instant, Vec<LanAddress>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, entries)) = cache.as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(60) {
            return entries.clone();
        }
    }
    let entries = lan_entries();
    *cache = Some((std::time::Instant::now(), entries.clone()));
    entries
}

fn is_private_lan(ip: &str) -> bool {
    ip.starts_with("192.168.")
        || ip.starts_with("10.")
        || (172..=172).contains(&ip.split('.').next().unwrap_or("").parse().unwrap_or(0))
}

/// Addresses Stacker would hand to someone else: not loopback, not the "no address yet"
/// range Windows assigns when a network never came up.
fn usable(ip: &str) -> bool {
    !ip.starts_with("127.") && !ip.starts_with("169.254.") && ip != "0.0.0.0"
}

/// Every adapter that is up, as the operating system lists them.
fn adapter_addresses() -> Vec<String> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for interface in interfaces {
        if interface.is_loopback() {
            continue;
        }
        let std::net::IpAddr::V4(ip) = interface.ip() else {
            continue;
        };
        let ip = ip.to_string();
        if usable(&ip) && !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
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
            cached_lan_entries()
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

/// Whether the service runs, and on which port: what the tray shows, without looking up the
/// network addresses (a PowerShell call the first time).
pub(crate) fn running_port() -> (bool, u16) {
    let state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    match &state.running {
        Some((running, _)) => (true, running.port),
        None => (false, load().port),
    }
}

#[tauri::command]
pub fn gateway_status() -> GatewayStatus {
    status()
}

#[tauri::command]
pub async fn gateway_set(enabled: bool, port: u16) -> Result<GatewayStatus, String> {
    tauri::async_runtime::spawn_blocking(move || set(enabled, port))
        .await
        .map_err(|e| e.to_string())?
}

/// Turns the service on or off on `port` and remembers it for the next start.
pub(crate) fn set(enabled: bool, port: u16) -> Result<GatewayStatus, String> {
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
}

/// Opening the service to the network restarts it on the other address; the token stays
/// the same, because it is what keeps the service the user's own.
/// Adds the inbound rule for this build, so Windows stops asking about the firewall every
/// time the service starts listening on the network.
#[tauri::command]
pub async fn gateway_allow_firewall() -> Result<(), String> {
    let port = load().port;
    tauri::async_runtime::spawn_blocking(move || crate::winadmin::allow_firewall_port(port))
        .await
        .map_err(|e| e.to_string())?
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_and_closing_the_network_keeps_the_service_on_its_port() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut state = State {
            running: None,
            error: String::new(),
        };
        let mut config = GatewayConfig {
            enabled: true,
            port,
            ..GatewayConfig::default()
        };
        for lan in [false, true, false, true] {
            config.lan_access = lan;
            start_locked(&mut state, &config);
            assert!(state.error.is_empty(), "lan={lan}: {}", state.error);
            assert_eq!(state.running.as_ref().map(|(r, _)| r.port), Some(port));
            // Served a request: its connection lingers in TIME_WAIT on the port.
            use std::io::{Read, Write};
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            client
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            client
                .write_all(b"GET /v1/models HTTP/1.0\r\n\r\n")
                .unwrap();
            let mut answer = String::new();
            let _ = client.read_to_string(&mut answer);
            assert!(answer.starts_with("HTTP/1."), "{answer}");
        }
        stop_locked(&mut state);
    }

    #[test]
    fn each_address_is_told_by_its_adapter() {
        let wifi = "MediaTek Wi-Fi 7 MT7925 Wireless LAN Card";
        assert_eq!(adapter_kind("192.168.1.2", "WLAN", wifi, true), "lan");
        assert_eq!(
            adapter_kind("100.94.113.98", "Tailscale", "Tailscale Tunnel", false),
            "tailscale"
        );
        let vmnet = "VMware Virtual Ethernet Adapter for VMnet8";
        assert_eq!(
            adapter_kind(
                "192.168.139.1",
                "VMware Network Adapter VMnet8",
                vmnet,
                false
            ),
            "vm"
        );
        let hyperv = "Hyper-V Virtual Ethernet Adapter";
        assert_eq!(
            adapter_kind("172.20.0.1", "vEthernet (WSL)", hyperv, false),
            "vm"
        );
        assert_eq!(
            adapter_kind("2.0.0.1", "本地连接", "Sangfor aTrust VNIC", false),
            "vpn"
        );
        assert_eq!(
            adapter_kind("10.8.0.2", "以太网 2", "TAP-Windows Adapter V9", false),
            "vpn"
        );
    }

    #[test]
    #[ignore = "reads this machine's adapters"]
    fn this_machine_s_addresses() {
        for entry in lan_entries() {
            println!("{entry:?}");
        }
    }
}
