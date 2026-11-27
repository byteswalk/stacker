//! What Windows itself is set to, read and never written.
//!
//! Windows keeps the system proxy twice: the loose `ProxyEnable` / `ProxyServer` values, and
//! the per-connection record in `Connections\DefaultConnectionSettings`. A freshly started
//! program — every Chromium-based one included — sees a proxy only when the two agree, so a
//! leftover `ProxyServer` next to a record that says "direct" is not a proxy that is on. The
//! service proxy (WinHTTP) is a third, separate setting that system services read.
//!
//! A Mac keeps one setting per network service; `scutil --proxy` prints the one in effect
//! for the current network, which is what programs that follow the system use.
//!
//! Stacker only reports these. Turning the system proxy on or off is the proxy software's
//! job, and this module never writes.

use serde::Serialize;

/// What the two copies of the system proxy setting add up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemState {
    /// Both copies agree that a proxy is in use.
    On,
    /// Both copies agree there is none.
    Off,
    /// They disagree: a leftover address that programs do not actually use.
    Stale,
    /// Not Windows, or the settings could not be read.
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemProxy {
    pub state: SystemState,
    /// `host:port` when one is in use, else empty.
    pub server: String,
    /// The address the loose values carry even when the record disagrees.
    pub recorded: String,
    /// Hosts Windows is told to reach directly.
    pub bypass: String,
}

/// WinHTTP: what system services use. Read from `netsh`, which needs no elevation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceProxy {
    /// `host:port`, or empty when services go direct.
    pub server: String,
    pub bypass: String,
    /// False when the setting could not be read at all.
    pub known: bool,
}

/// The part of `DefaultConnectionSettings` that says whether a proxy is in use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionRecord {
    pub flags: u32,
    pub proxy: String,
}

/// Layout: version, a change counter, flags, then a length-prefixed proxy string, all
/// little-endian.
pub fn parse_connection_record(bytes: &[u8]) -> Option<ConnectionRecord> {
    let u32_at = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let flags = u32_at(8)?;
    let length = u32_at(12)? as usize;
    let proxy = bytes.get(16..16 + length)?;
    Some(ConnectionRecord {
        flags,
        proxy: String::from_utf8_lossy(proxy).into_owned(),
    })
}

const PROXY_TYPE_PROXY: u32 = 0x2;

/// Whether the record says the same as the loose values do.
pub fn record_agrees(record: &ConnectionRecord, enabled: bool, server: &str) -> bool {
    if enabled {
        record.flags & PROXY_TYPE_PROXY != 0 && record.proxy == server
    } else {
        record.flags & PROXY_TYPE_PROXY == 0
    }
}

/// The first usable `host:port` out of a `ProxyServer` value, which may name one address for
/// everything or one per protocol (`http=127.0.0.1:1080;socks=…`).
pub fn first_endpoint(server: &str) -> String {
    for part in server.split(';') {
        let value = part
            .split_once('=')
            .map(|(_, value)| value)
            .unwrap_or(part)
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        if value.contains(':') && !value.is_empty() {
            return value.to_string();
        }
    }
    String::new()
}

/// `netsh winhttp show proxy` output, in either language Windows prints it in.
pub fn parse_winhttp(text: &str) -> ServiceProxy {
    let direct = ["direct access", "直接访问", "no proxy server"];
    let lower = text.to_lowercase();
    if direct.iter().any(|needle| lower.contains(needle)) {
        return ServiceProxy {
            server: String::new(),
            bypass: String::new(),
            known: true,
        };
    }
    let after = |line: &str| {
        line.split_once(':')
            .map(|(_, value)| value.trim().to_string())
            .unwrap_or_default()
    };
    let mut proxy = ServiceProxy {
        server: String::new(),
        bypass: String::new(),
        known: false,
    };
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("proxy server") || lower.contains("代理服务器") {
            proxy.server = first_endpoint(&after(line));
            proxy.known = true;
        } else if lower.contains("bypass") || lower.contains("绕过") {
            proxy.bypass = after(line);
        }
    }
    proxy
}

#[cfg(windows)]
fn read_system() -> Option<SystemProxy> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let user = RegKey::predef(HKEY_CURRENT_USER);
    let settings = user
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled = settings.get_value::<u32, _>("ProxyEnable").unwrap_or(0) != 0;
    let server = settings
        .get_value::<String, _>("ProxyServer")
        .unwrap_or_default();
    let bypass = settings
        .get_value::<String, _>("ProxyOverride")
        .unwrap_or_default();
    let record = user
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings\Connections")
        .ok()
        .and_then(|key| key.get_raw_value("DefaultConnectionSettings").ok())
        .and_then(|raw| parse_connection_record(&raw.bytes));
    let agrees = record
        .as_ref()
        .map(|record| record_agrees(record, enabled, &server));
    let state = match (enabled, agrees) {
        (true, Some(true)) => SystemState::On,
        (false, Some(true)) => SystemState::Off,
        // No record to check against: the loose values are all there is to go on.
        (true, None) => SystemState::On,
        (false, None) => SystemState::Off,
        _ => SystemState::Stale,
    };
    Some(SystemProxy {
        server: if state == SystemState::On {
            first_endpoint(&server)
        } else {
            String::new()
        },
        recorded: first_endpoint(&server),
        bypass,
        state,
    })
}

/// `scutil --proxy`: a dictionary printed one `Key : value` per line, with nested ones (the
/// exception list, and per-interface copies under `__SCOPED__`) indented inside `{ }`. Only
/// the top level is the setting in effect. The pages write `http://host:port`, so only the
/// HTTP and HTTPS proxies count; a SOCKS-only setting has no address to give them.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_scutil(text: &str) -> SystemProxy {
    let mut depth = 0usize;
    let mut top = std::collections::HashMap::new();
    let mut in_exceptions = false;
    let mut exceptions = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.ends_with('{') {
            if depth == 1 && line.starts_with("ExceptionsList") {
                in_exceptions = true;
            }
            depth += 1;
            continue;
        }
        if line == "}" {
            depth = depth.saturating_sub(1);
            if depth == 1 {
                in_exceptions = false;
            }
            continue;
        }
        let Some((key, value)) = line.split_once(" : ") else {
            continue;
        };
        if depth == 1 {
            top.insert(key.trim().to_string(), value.trim().to_string());
        } else if depth == 2 && in_exceptions {
            exceptions.push(value.trim().to_string());
        }
    }
    let address = |host: &str, port: &str| {
        let host = top.get(host).map(String::as_str).unwrap_or_default();
        let port = top.get(port).map(String::as_str).unwrap_or_default();
        (!host.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0))
            .then(|| format!("{host}:{port}"))
    };
    let enabled = |key: &str| top.get(key).map(String::as_str) == Some("1");
    let pairs = [
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        ("HTTPEnable", "HTTPProxy", "HTTPPort"),
    ];
    let server = pairs
        .iter()
        .find(|(on, _, _)| enabled(on))
        .and_then(|(_, host, port)| address(host, port))
        .unwrap_or_default();
    let recorded = pairs
        .iter()
        .find_map(|(_, host, port)| address(host, port))
        .unwrap_or_default();
    SystemProxy {
        state: if server.is_empty() {
            SystemState::Off
        } else {
            SystemState::On
        },
        server,
        recorded,
        bypass: exceptions.join(","),
    }
}

/// Read again at most every few seconds: the settings are loaded far more often than the
/// proxy changes, and each read starts a process.
#[cfg(target_os = "macos")]
fn read_system() -> Option<SystemProxy> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<(Instant, Option<SystemProxy>)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, proxy)) = last.as_ref() {
        if at.elapsed() < Duration::from_secs(3) {
            return proxy.clone();
        }
    }
    let proxy = crate::macos::scutil_proxy().map(|text| parse_scutil(&text));
    *last = Some((Instant::now(), proxy.clone()));
    proxy
}

#[cfg(not(any(windows, target_os = "macos")))]
fn read_system() -> Option<SystemProxy> {
    None
}

pub(crate) fn system() -> SystemProxy {
    read_system().unwrap_or(SystemProxy {
        state: SystemState::Unknown,
        server: String::new(),
        recorded: String::new(),
        bypass: String::new(),
    })
}

#[cfg(windows)]
pub(crate) fn service() -> ServiceProxy {
    let mut command = std::process::Command::new("netsh");
    command.args(["winhttp", "show", "proxy"]);
    // Reading a setting is not something to flash a console window for.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().ok();
    match output {
        Some(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            parse_winhttp(&text)
        }
        None => ServiceProxy {
            server: String::new(),
            bypass: String::new(),
            known: false,
        },
    }
}

#[cfg(not(windows))]
pub(crate) fn service() -> ServiceProxy {
    ServiceProxy {
        server: String::new(),
        bypass: String::new(),
        known: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(flags: u32, proxy: &str) -> Vec<u8> {
        let mut bytes = vec![0u8; 16];
        bytes[8..12].copy_from_slice(&flags.to_le_bytes());
        bytes[12..16].copy_from_slice(&(proxy.len() as u32).to_le_bytes());
        bytes.extend_from_slice(proxy.as_bytes());
        bytes
    }

    #[test]
    fn a_leftover_address_is_not_a_proxy_that_is_on() {
        let on = parse_connection_record(&record(0x3, "127.0.0.1:7890")).unwrap();
        assert!(record_agrees(&on, true, "127.0.0.1:7890"));
        // The record says direct while the loose values still name an address: nothing
        // started after that point uses it.
        let direct = parse_connection_record(&record(0x1, "")).unwrap();
        assert!(!record_agrees(&direct, true, "127.0.0.1:7890"));
        assert!(record_agrees(&direct, false, ""));
        // A record that does not even hold its own length is no answer.
        assert!(parse_connection_record(&[0u8; 8]).is_none());
    }

    #[test]
    fn an_address_is_taken_out_of_whatever_shape_windows_wrote_it_in() {
        assert_eq!(first_endpoint("127.0.0.1:7890"), "127.0.0.1:7890");
        assert_eq!(
            first_endpoint("http=127.0.0.1:7890;https=127.0.0.1:7890"),
            "127.0.0.1:7890"
        );
        assert_eq!(first_endpoint("http://127.0.0.1:6789/"), "127.0.0.1:6789");
        assert_eq!(first_endpoint(""), "");
    }

    #[test]
    fn the_mac_setting_in_effect_is_the_top_level_one() {
        let text = "<dictionary> {
  ExceptionsList : <array> {
    0 : 127.0.0.1
    1 : localhost
  }
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7891
  HTTPSProxy : 127.0.0.1
  __SCOPED__ : <dictionary> {
    en0 : <dictionary> {
      HTTPEnable : 1
      HTTPPort : 1080
      HTTPProxy : 10.0.0.1
    }
  }
}
";
        let on = parse_scutil(text);
        assert_eq!(on.state, SystemState::On);
        assert_eq!(on.server, "127.0.0.1:7891");
        assert_eq!(on.bypass, "127.0.0.1,localhost");

        let off = parse_scutil(
            "<dictionary> {\n  HTTPEnable : 0\n  HTTPPort : 7890\n  HTTPProxy : 127.0.0.1\n  SOCKSEnable : 1\n  SOCKSPort : 7890\n  SOCKSProxy : 127.0.0.1\n}\n",
        );
        assert_eq!(off.state, SystemState::Off);
        assert_eq!(off.server, "");
        assert_eq!(off.recorded, "127.0.0.1:7890");
        assert_eq!(parse_scutil("<dictionary> {\n}\n").state, SystemState::Off);
    }

    #[test]
    fn the_service_proxy_is_read_in_either_language() {
        let direct = parse_winhttp(
            "Current WinHTTP proxy settings:\n\n    Direct access (no proxy server).\n",
        );
        assert!(direct.known && direct.server.is_empty());

        let set = parse_winhttp(
            "Current WinHTTP proxy settings:\n\n    Proxy Server(s) :  127.0.0.1:7890\n    Bypass List     :  <local>\n",
        );
        assert_eq!(set.server, "127.0.0.1:7890");
        assert_eq!(set.bypass, "<local>");

        let chinese = parse_winhttp(
            "    直接访问。
",
        );
        assert!(chinese.known && chinese.server.is_empty());

        // Nothing readable at all is "not known", not "no proxy".
        assert!(!parse_winhttp("").known);
    }
}
