//! Which proxy settings Stacker wrote, and the only ones it may change on its own.
//!
//! In hands-off mode nothing is changed automatically. In follow-system and manual mode,
//! `reconcile` updates a location only while its current value still equals what Stacker
//! last wrote there; anything changed by the user or another tool is forgotten and left alone.
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Env,
    Git,
    Npm,
    Yarn,
    Maven,
    MavenOpts,
    Gradle,
    GradleProps,
}

pub const LOCATIONS: [Location; 8] = [
    Location::Env,
    Location::Git,
    Location::Npm,
    Location::Yarn,
    Location::Maven,
    Location::MavenOpts,
    Location::Gradle,
    Location::GradleProps,
];

impl Location {
    pub fn id(self) -> &'static str {
        match self {
            Location::Env => "env",
            Location::Git => "git",
            Location::Npm => "npm",
            Location::Yarn => "yarn",
            Location::Maven => "maven",
            Location::MavenOpts => "maven_opts",
            Location::Gradle => "gradle",
            Location::GradleProps => "gradle_props",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        LOCATIONS.iter().copied().find(|l| l.id() == id)
    }

    /// Current proxy at this location, as written there.
    pub fn read(self) -> Option<String> {
        match self {
            Location::Env => crate::proxy::env_proxy(),
            Location::Git => crate::git::proxy_get(),
            Location::Npm => crate::sources::npm_proxy(),
            Location::Yarn => crate::sources::yarn_proxy(),
            Location::Maven => crate::sources::maven_proxy(),
            Location::MavenOpts => crate::proxy::maven_opts_proxy(),
            Location::Gradle => crate::sources::gradle_proxy(),
            Location::GradleProps => crate::proxy::gradle_props_proxy(),
        }
    }

    fn write(self, host: &str, port: u16) -> Result<(), String> {
        match self {
            Location::Env => {
                crate::proxy::enable(host, port, false, crate::settings::proxy_manual())
            }
            Location::Git => crate::git::proxy_set(&format!("http://{host}:{port}")),
            Location::Npm => crate::sources::set_npm_proxy(Some(&format!("http://{host}:{port}"))),
            Location::Yarn => {
                crate::sources::set_yarn_proxy(Some(&format!("http://{host}:{port}")))
            }
            Location::Maven => crate::sources::set_maven_proxy(Some((host, port))),
            Location::MavenOpts => crate::proxy::set_maven_opts_proxy(host, port),
            Location::Gradle => crate::sources::set_gradle_proxy(Some((host, port))),
            Location::GradleProps => crate::proxy::set_gradle_props_proxy(host, port),
        }
    }

    fn clear(self) -> Result<(), String> {
        match self {
            Location::Env => crate::proxy::disable(false),
            Location::Git => crate::git::proxy_clear(),
            Location::Npm => crate::sources::set_npm_proxy(None),
            Location::Yarn => crate::sources::set_yarn_proxy(None),
            Location::Maven => crate::sources::set_maven_proxy(None),
            Location::MavenOpts => crate::proxy::clear_maven_opts_proxy(),
            Location::Gradle => crate::sources::set_gradle_proxy(None),
            Location::GradleProps => crate::proxy::clear_gradle_props_proxy(),
        }
    }
}

/// `http://user@Host:7890/` → `host:7890`; empty for nothing.
pub fn normalize(value: Option<&str>) -> String {
    let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return String::new();
    };
    let rest = raw.split_once("://").map(|(_, r)| r).unwrap_or(raw);
    let rest = rest.trim_end_matches('/');
    let rest = rest.rsplit('@').next().unwrap_or(rest);
    rest.to_ascii_lowercase()
}

/// The address a "write" puts there: what Windows itself is set to, and nothing else.
fn write_address() -> Option<(String, u16)> {
    crate::settings::detected_proxy_addr()
}

/// A described target, written and cleared through its own file writer.
pub fn write_target(target: &crate::proxy_targets::Target) -> Result<(), String> {
    let (host, port) = write_address().ok_or("E_PROXY_ADDR")?;
    target.write(&host, port)?;
    record_id(&target.id, Some(&format!("{host}:{port}")))
}

pub fn clear_target(target: &crate::proxy_targets::Target) -> Result<(), String> {
    target.clear()?;
    record_id(&target.id, None)
}

/// User action: write the current address here and manage it from now on.
pub fn write_location(location: Location) -> Result<(), String> {
    let (host, port) = write_address().ok_or("E_PROXY_ADDR")?;
    location.write(&host, port)?;
    record(location, Some(&format!("{host}:{port}")))
}

/// User action: clear this location and stop managing it.
pub fn clear_location(location: Location) -> Result<(), String> {
    location.clear()?;
    record(location, None)
}

/// Records a write (Some) or forgets a location (None) after Stacker changed it elsewhere,
/// e.g. the Git page or a mirror switch with a proxy.
pub fn record(location: Location, written: Option<&str>) -> Result<(), String> {
    record_id(location.id(), written)
}

fn record_id(id: &str, written: Option<&str>) -> Result<(), String> {
    let mut managed = crate::settings::proxy_managed();
    match written {
        Some(value) => {
            managed.insert(id.to_string(), normalize(Some(value)));
        }
        None => {
            managed.remove(id);
        }
    }
    crate::settings::save_proxy_managed(managed)
}

#[derive(Clone, Debug, Serialize)]
pub struct LocationRow {
    pub id: String,
    pub value: Option<String>,
    /// managed | external | none
    pub owner: String,
    /// The described targets carry their own label; the eight built-in ones are named by
    /// the page, which has always known them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<crate::proxy_targets::Target>,
    /// Whether the program is on this machine at all.
    #[serde(default = "yes")]
    pub installed: bool,
}

#[allow(dead_code)]
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    pub windows: Option<String>,
    pub write_address: Option<String>,
    pub locations: Vec<LocationRow>,
}

pub fn owner(managed: &BTreeMap<String, String>, id: &str, value: Option<&str>) -> &'static str {
    let current = normalize(value);
    match managed.get(id) {
        Some(recorded) if *recorded == current && !current.is_empty() => "managed",
        _ if current.is_empty() => "none",
        _ => "external",
    }
}

pub fn overview() -> Overview {
    let settings = crate::settings::load();
    let managed = settings.proxy_managed.clone();
    let locations = LOCATIONS
        .iter()
        .map(|l| {
            let value = l.read();
            LocationRow {
                id: l.id().into(),
                owner: owner(&managed, l.id(), value.as_deref()).into(),
                value,
                target: None,
                installed: true,
            }
        })
        .collect::<Vec<_>>();
    // Everything described as data, presets first, the user's own after.
    let mut locations = locations;
    for target in crate::proxy_targets::all() {
        let value = target.read();
        locations.push(LocationRow {
            id: target.id.clone(),
            owner: owner(&managed, &target.id, value.as_deref()).into(),
            value,
            installed: target.installed(),
            target: Some(target),
        });
    }
    Overview {
        windows: crate::settings::detected_proxy_addr().map(|(h, p)| format!("{h}:{p}")),
        write_address: write_address().map(|(h, p)| format!("{h}:{p}")),
        locations,
    }
}

/// One place whose proxy no longer matches what Windows is set to.
#[derive(Clone, Debug, Serialize)]
pub struct SyncRow {
    pub id: String,
    pub value: String,
    /// missing | elsewhere | leftover
    pub issue: String,
    /// Whether Stacker wrote what is there now, and may change it without asking.
    pub ours: bool,
}

/// The system's setting, the service setting, and everywhere that disagrees with them.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub system: crate::proxy_system::SystemProxy,
    pub service: crate::proxy_system::ServiceProxy,
    pub rows: Vec<SyncRow>,
}

/// Compares every place Stacker knows against the system setting. Nothing is changed here;
/// the page decides what to offer, and the user decides what to do.
pub fn sync_report() -> SyncReport {
    let system = crate::proxy_system::system();
    let service = crate::proxy_system::service();
    let on = system.state == crate::proxy_system::SystemState::On;
    let wanted = crate::proxy_system::first_endpoint(&system.server);
    let overview = overview();
    let mut rows = Vec::new();
    // A tool that is not installed here follows nothing and is out of step with nothing.
    for row in overview.locations.iter().filter(|row| row.installed) {
        let ours = row.owner == "managed";
        let current = row.value.clone().unwrap_or_default();
        let endpoint = endpoint(&current).map(|(h, p)| format!("{h}:{p}"));
        let issue = match (on, current.is_empty()) {
            // The system routes through a proxy and this place does not know about it.
            (true, true) => "missing",
            (true, false) if endpoint.as_deref() != Some(wanted.as_str()) => "elsewhere",
            (true, false) => continue,
            // The system goes direct, so a proxy left here sends traffic nowhere.
            (false, false) => "leftover",
            (false, true) => continue,
        };
        rows.push(SyncRow {
            id: row.id.clone(),
            value: current,
            issue: issue.into(),
            ours,
        });
    }
    // The service proxy is Windows' own setting, changed with an elevated netsh; it is
    // reported the same way so nothing about it is a surprise.
    if service.known {
        let issue = match (on, service.server.is_empty()) {
            (true, true) => Some("missing"),
            (true, false) if service.server != wanted => Some("elsewhere"),
            (false, false) => Some("leftover"),
            _ => None,
        };
        if let Some(issue) = issue {
            rows.push(SyncRow {
                id: "winhttp".into(),
                value: service.server.clone(),
                issue: issue.into(),
                ours: false,
            });
        }
    }
    SyncReport {
        system,
        service,
        rows,
    }
}

/// Writes or clears the service proxy through the elevated helper. Windows asks the user to
/// approve it; a refusal comes back as an error, not a silent no-op.
#[tauri::command]
pub async fn proxy_service_set(address: Option<String>) -> Result<SyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::winadmin::set_service_proxy(address.as_deref())?;
        Ok(sync_report())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Everywhere at once. What Windows is set to is what gets written, so when the system has
/// no proxy the only honest thing left is to take it back out of every place that has one.
#[tauri::command]
pub async fn proxy_follow_system(release: bool) -> Result<SyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || follow_system(release))
        .await
        .map_err(|e| e.to_string())?
}

pub(crate) fn follow_system(release: bool) -> Result<SyncReport, String> {
    let system = crate::proxy_system::system();
    let wanted = crate::proxy_system::first_endpoint(&system.server);
    if !release && wanted.is_empty() {
        return Err("E_PROXY_ADDR".to_string());
    }
    let mut failed = Vec::new();
    for location in LOCATIONS {
        let current = normalize(location.read().as_deref());
        let result = if release {
            if current.is_empty() {
                continue;
            }
            clear_location(location)
        } else {
            if endpoint(&current)
                .map(|(h, p)| format!("{h}:{p}"))
                .as_deref()
                == Some(&wanted)
            {
                continue;
            }
            write_location(location)
        };
        if let Err(error) = result {
            failed.push(format!("{}：{error}", location.id()));
        }
    }
    // The service proxy is Windows' own, so it takes the elevated path and one prompt.
    let service = crate::proxy_system::service();
    let service_wanted = if release { None } else { Some(wanted.as_str()) };
    if service.known && service.server != service_wanted.unwrap_or_default() {
        if let Err(error) = crate::winadmin::set_service_proxy(service_wanted) {
            failed.push(format!("winhttp：{error}"));
        }
    }
    if failed.is_empty() {
        Ok(sync_report())
    } else {
        Err(failed.join("；"))
    }
}

#[tauri::command]
pub async fn proxy_sync_report() -> SyncReport {
    tauri::async_runtime::spawn_blocking(sync_report)
        .await
        .unwrap_or_else(|_| SyncReport {
            system: crate::proxy_system::system(),
            service: crate::proxy_system::service(),
            rows: Vec::new(),
        })
}

#[tauri::command]
pub async fn proxy_overview() -> Overview {
    tauri::async_runtime::spawn_blocking(overview)
        .await
        .unwrap_or_else(|_| Overview {
            windows: None,
            write_address: None,
            locations: Vec::new(),
        })
}

/// `host:port` from a stored proxy value.
pub fn endpoint(value: &str) -> Option<(String, u16)> {
    let normalized = normalize(Some(value));
    let (host, port) = normalized.rsplit_once(':')?;
    let port = port.parse().ok()?;
    (!host.is_empty()).then(|| (host.to_string(), port))
}

/// Locations whose proxy points at a port nobody listens on, as (Stacker's, the user's).
pub fn stale_with(
    rows: &[LocationRow],
    listening: impl Fn(&str, u16) -> bool,
) -> (Vec<LocationRow>, Vec<LocationRow>) {
    let mut checked: BTreeMap<(String, u16), bool> = BTreeMap::new();
    let (mut ours, mut theirs) = (Vec::new(), Vec::new());
    for row in rows {
        let Some((host, port)) = row.value.as_deref().and_then(endpoint) else {
            continue;
        };
        let up = *checked
            .entry((host.clone(), port))
            .or_insert_with(|| listening(&host, port));
        if up {
            continue;
        }
        match row.owner.as_str() {
            "managed" => ours.push(row.clone()),
            "external" => theirs.push(row.clone()),
            _ => {}
        }
    }
    (ours, theirs)
}

/// Clears only the dead proxies Stacker wrote and nobody changed since; the rest is untouched.
pub fn clear_stale_managed(listening: impl Fn(&str, u16) -> bool) -> Result<usize, String> {
    let (ours, _) = stale_with(&overview().locations, listening);
    for row in &ours {
        if let Some(location) = Location::from_id(&row.id) {
            clear_location(location)?;
        }
    }
    Ok(ours.len())
}

#[tauri::command]
pub async fn proxy_clear_stale() -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(|| clear_stale_managed(crate::checkup::port_listening))
        .await
        .map_err(|e| e.to_string())?
}

/// The described targets: the presets Stacker ships and the ones the user added.
#[tauri::command]
pub async fn proxy_targets_list() -> Vec<crate::proxy_targets::Target> {
    crate::proxy_targets::all()
}

#[tauri::command]
pub async fn proxy_target_save(target: crate::proxy_targets::Target) -> Result<(), String> {
    crate::proxy_targets::save_custom(target)
}

#[tauri::command]
pub async fn proxy_target_remove(id: String) -> Result<(), String> {
    crate::proxy_targets::remove_custom(&id)
}

#[tauri::command]
pub async fn proxy_location_write(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || match Location::from_id(&id) {
        Some(location) => write_location(location),
        None => write_target(&crate::proxy_targets::find(&id).ok_or("E_REQUEST")?),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn proxy_location_clear(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || match Location::from_id(&id) {
        Some(location) => clear_location(location),
        None => clear_target(&crate::proxy_targets::find(&id).ok_or("E_REQUEST")?),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_splits_by_owner_and_skips_live_ports() {
        let row = |id: &str, value: &str, owner: &str| LocationRow {
            id: id.into(),
            value: Some(value.into()),
            owner: owner.into(),
            target: None,
            installed: true,
        };
        let rows = vec![
            row("env", "http://127.0.0.1:7890", "managed"),
            row("git", "http://127.0.0.1:6789", "external"),
            row("npm", "http://127.0.0.1:1080", "managed"),
            row("yarn", "not a proxy", "external"),
        ];
        let (ours, theirs) = stale_with(&rows, |_, port| port == 1080);
        assert_eq!(
            ours.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["env"]
        );
        assert_eq!(
            theirs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["git"]
        );
        assert_eq!(endpoint("http://u@Host:80/"), Some(("host".into(), 80)));
        assert_eq!(endpoint("host"), None);
    }

    #[test]
    fn values_normalize() {
        assert_eq!(
            normalize(Some("http://User@Host.Local:7890/")),
            "host.local:7890"
        );
        assert_eq!(normalize(Some("socks5://127.0.0.1:7891")), "127.0.0.1:7891");
        assert_eq!(normalize(Some("127.0.0.1:7890")), "127.0.0.1:7890");
        assert_eq!(normalize(None), "");
        assert_eq!(normalize(Some("  ")), "");
    }

    /// Live, read-only: `cargo test --lib live_proxy_overview -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_proxy_overview() {
        let o = overview();
        println!("windows={:?} write={:?}", o.windows, o.write_address);
        for l in o.locations {
            println!("  {:<12} {:<9} {:?}", l.id, l.owner, l.value);
        }
    }

    #[test]
    fn owners_are_reported() {
        let managed = BTreeMap::from([("git".to_string(), "127.0.0.1:7890".to_string())]);
        assert_eq!(
            owner(&managed, "git", Some("http://127.0.0.1:7890")),
            "managed"
        );
        assert_eq!(
            owner(&managed, "git", Some("http://127.0.0.1:1080")),
            "external"
        );
        assert_eq!(
            owner(&managed, "npm", Some("http://127.0.0.1:7890")),
            "external"
        );
        assert_eq!(owner(&managed, "npm", None), "none");
    }
}
