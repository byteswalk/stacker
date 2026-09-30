//! Programs whose proxy lives in a file of their own, described as data rather than code.
//!
//! The eight places in `proxy_ledger` each needed their own reader and writer. Most tools do
//! not: they keep a key in a file, and the only things that differ are which file, which keys,
//! and how the file is shaped. Describing them as data means a preset and something the user
//! adds are the same thing, and the list can grow without new Rust for every tool.
//!
//! Nothing is written to a program that is not installed: creating a config folder for a tool
//! the user does not have leaves litter behind and can confuse that tool's own first run.
//!
//! The shape follows the one in the user's nexa project, minus the writer that shells out:
//! a target the user typed should never be able to run a command.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How to tell whether the program is here at all.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Detect {
    /// A file or folder the program keeps.
    PathExists { path: String },
    /// A command the program puts on PATH.
    CommandOnPath { command: String },
}

/// How the proxy is written into that program's configuration. Each writer edits exactly the
/// keys it is given and leaves the rest of the file — its comments, its order, its unrelated
/// settings — as it found them.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum Writer {
    /// `key=value` lines: `.env`, `.npmrc`, `.curlrc`.
    KeyValue { path: String, entries: Vec<Entry> },
    /// Sectioned `key = value`: `pip.ini`, and the corner of TOML that looks the same
    /// (`[http]` + `proxy = "…"` in Cargo's config).
    Ini { path: String, entries: Vec<Entry> },
    /// `key` is a JSON Pointer, e.g. `/env/HTTPS_PROXY`.
    Json { path: String, entries: Vec<Entry> },
}

/// One key and the value written into it. `{proxy}` becomes `host:port`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Entry {
    /// Only the `ini` writer uses this.
    pub section: String,
    pub key: String,
    pub value: String,
}

/// One program whose configuration gets the proxy written into it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub icon: String,
    /// Shipped with Stacker; the page does not offer to delete it.
    #[serde(default)]
    pub builtin: bool,
    pub detect: Detect,
    pub writer: Writer,
}

fn entry(key: &str, value: &str) -> Entry {
    Entry {
        section: String::new(),
        key: key.into(),
        value: value.into(),
    }
}

fn ini_entry(section: &str, key: &str, value: &str) -> Entry {
    Entry {
        section: section.into(),
        key: key.into(),
        value: value.into(),
    }
}

/// The tools Stacker knows about, beyond the eight it reads and writes itself.
pub fn builtin() -> Vec<Target> {
    vec![
        Target {
            id: "pip".into(),
            name: "pip".into(),
            detail: "pip.ini 的 [global] proxy".into(),
            icon: "ti-brand-python".into(),
            builtin: true,
            detect: Detect::CommandOnPath {
                command: "pip".into(),
            },
            writer: Writer::Ini {
                path: "%APPDATA%/pip/pip.ini".into(),
                entries: vec![ini_entry("global", "proxy", "http://{proxy}")],
            },
        },
        Target {
            id: "cargo".into(),
            name: "Cargo".into(),
            detail: "~/.cargo/config.toml 的 [http] proxy".into(),
            icon: "ti-brand-rust".into(),
            builtin: true,
            detect: Detect::PathExists {
                path: "~/.cargo".into(),
            },
            writer: Writer::Ini {
                path: "~/.cargo/config.toml".into(),
                entries: vec![ini_entry("http", "proxy", "\"http://{proxy}\"")],
            },
        },
        Target {
            id: "docker".into(),
            name: "Docker CLI".into(),
            detail: "~/.docker/config.json 的 proxies.default".into(),
            icon: "ti-brand-docker".into(),
            builtin: true,
            detect: Detect::PathExists {
                path: "~/.docker".into(),
            },
            writer: Writer::Json {
                path: "~/.docker/config.json".into(),
                entries: vec![
                    entry("/proxies/default/httpProxy", "http://{proxy}"),
                    entry("/proxies/default/httpsProxy", "http://{proxy}"),
                ],
            },
        },
        Target {
            id: "vscode".into(),
            name: "VS Code".into(),
            detail: "用户 settings.json 的 http.proxy".into(),
            icon: "ti-code".into(),
            builtin: true,
            detect: Detect::PathExists {
                path: "%APPDATA%/Code/User".into(),
            },
            writer: Writer::Json {
                path: "%APPDATA%/Code/User/settings.json".into(),
                entries: vec![entry("/http.proxy", "http://{proxy}")],
            },
        },
        Target {
            id: "claude_code".into(),
            name: "Claude Code".into(),
            detail: "~/.claude/settings.json 注入的 env".into(),
            icon: "ti-message-chatbot".into(),
            builtin: true,
            detect: Detect::PathExists {
                path: "~/.claude".into(),
            },
            writer: Writer::Json {
                path: "~/.claude/settings.json".into(),
                entries: vec![
                    entry("/env/HTTP_PROXY", "http://{proxy}"),
                    entry("/env/HTTPS_PROXY", "http://{proxy}"),
                ],
            },
        },
        Target {
            id: "codex".into(),
            name: "Codex".into(),
            detail: "~/.codex/.env 的 HTTP_PROXY / HTTPS_PROXY".into(),
            icon: "ti-terminal".into(),
            builtin: true,
            detect: Detect::PathExists {
                path: "~/.codex".into(),
            },
            writer: Writer::KeyValue {
                path: "~/.codex/.env".into(),
                entries: vec![
                    entry("HTTP_PROXY", "http://{proxy}"),
                    entry("HTTPS_PROXY", "http://{proxy}"),
                ],
            },
        },
        Target {
            id: "curl".into(),
            name: "curl".into(),
            detail: "~/.curlrc 的 proxy".into(),
            icon: "ti-world-download".into(),
            builtin: true,
            detect: Detect::CommandOnPath {
                command: "curl".into(),
            },
            writer: Writer::KeyValue {
                path: "~/.curlrc".into(),
                entries: vec![entry("proxy", "http://{proxy}")],
            },
        },
    ]
}

/// The presets plus whatever the user added, the user's own last.
pub fn all() -> Vec<Target> {
    let mut targets = builtin();
    targets.extend(crate::settings::load().proxy_targets);
    targets
}

pub fn find(id: &str) -> Option<Target> {
    all().into_iter().find(|target| target.id == id)
}

/// `~` and `%VAR%` as the shell would read them.
pub fn expand(path: &str) -> PathBuf {
    let mut text = path.replace('/', "\\");
    if let Some(rest) = text.strip_prefix("~\\") {
        let home = dirs::home_dir().unwrap_or_default();
        text = home.join(rest).to_string_lossy().into_owned();
    }
    while let Some(start) = text.find('%') {
        let Some(end) = text[start + 1..].find('%').map(|at| start + 1 + at) else {
            break;
        };
        let name = &text[start + 1..end];
        let value = std::env::var(name).unwrap_or_default();
        if value.is_empty() {
            break;
        }
        text = format!("{}{}{}", &text[..start], value, &text[end + 1..]);
    }
    PathBuf::from(text)
}

impl Target {
    pub fn path(&self) -> PathBuf {
        expand(match &self.writer {
            Writer::KeyValue { path, .. }
            | Writer::Ini { path, .. }
            | Writer::Json { path, .. } => path,
        })
    }

    fn entries(&self) -> &[Entry] {
        match &self.writer {
            Writer::KeyValue { entries, .. }
            | Writer::Ini { entries, .. }
            | Writer::Json { entries, .. } => entries,
        }
    }

    /// Whether the program is on this machine. Nothing is written to one that is not.
    pub fn installed(&self) -> bool {
        match &self.detect {
            Detect::PathExists { path } => expand(path).exists(),
            Detect::CommandOnPath { command } => {
                crate::agents::process::resolve_command(&[command.as_str()]).is_some()
            }
        }
    }

    /// What the first key currently holds, as a proxy value.
    pub fn read(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.path()).ok()?;
        let first = self.entries().first()?;
        let value = match &self.writer {
            Writer::KeyValue { .. } => read_key_value(&text, &first.key),
            Writer::Ini { .. } => read_ini(&text, &first.section, &first.key),
            Writer::Json { .. } => read_json(&text, &first.key),
        }?;
        let value = value.trim().trim_matches('"').to_string();
        (!value.is_empty()).then_some(value)
    }

    pub fn write(&self, host: &str, port: u16) -> Result<(), String> {
        if !self.installed() {
            return Err("E_NOT_INSTALLED".into());
        }
        self.apply(Some(&format!("{host}:{port}")))
    }

    pub fn clear(&self) -> Result<(), String> {
        self.apply(None)
    }

    fn apply(&self, proxy: Option<&str>) -> Result<(), String> {
        let path = self.path();
        if proxy.is_none() && !path.exists() {
            return Ok(());
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut next = text.clone();
        for entry in self.entries() {
            let value = proxy.map(|proxy| entry.value.replace("{proxy}", proxy));
            next = match &self.writer {
                Writer::KeyValue { .. } => set_key_value(&next, &entry.key, value.as_deref()),
                Writer::Ini { .. } => set_ini(&next, &entry.section, &entry.key, value.as_deref()),
                Writer::Json { .. } => set_json(&next, &entry.key, value.as_deref())?,
            };
        }
        if next == text {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, next).map_err(|error| error.to_string())
    }
}

fn read_key_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case(key)
                .then(|| value.trim().to_string())
        })
}

fn set_key_value(text: &str, key: &str, value: Option<&str>) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in text.lines() {
        let matches = line
            .split_once('=')
            .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(key));
        if !matches {
            lines.push(line.to_string());
            continue;
        }
        match value {
            Some(value) if !replaced => {
                lines.push(format!("{key}={value}"));
                replaced = true;
            }
            _ => {}
        }
    }
    if let Some(value) = value {
        if !replaced {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut out = lines.join("\r\n");
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    out
}

fn read_ini(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        if !current.eq_ignore_ascii_case(section) || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once('=') {
            if name.trim().eq_ignore_ascii_case(key) {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn set_ini(text: &str, section: &str, key: &str, value: Option<&str>) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut written = false;
    let mut section_end: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if current.eq_ignore_ascii_case(section) && section_end.is_none() {
                section_end = Some(lines.len());
            }
            current = trimmed[1..trimmed.len() - 1].trim().to_string();
            lines.push(line.to_string());
            continue;
        }
        let matches = current.eq_ignore_ascii_case(section)
            && trimmed
                .split_once('=')
                .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(key));
        if !matches {
            lines.push(line.to_string());
            continue;
        }
        match value {
            Some(value) if !written => {
                lines.push(format!("{key} = {value}"));
                written = true;
            }
            _ => {}
        }
    }
    if current.eq_ignore_ascii_case(section) && section_end.is_none() {
        section_end = Some(lines.len());
    }
    if let Some(value) = value {
        if !written {
            match section_end {
                Some(at) => lines.insert(at, format!("{key} = {value}")),
                None => {
                    if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty())
                    {
                        lines.push(String::new());
                    }
                    lines.push(format!("[{section}]"));
                    lines.push(format!("{key} = {value}"));
                }
            }
        }
    }
    let mut out = lines.join("\r\n");
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    out
}

/// A JSON Pointer, except that a key containing a dot is taken as one key: VS Code's settings
/// are flat with dotted names, and `/http.proxy` means that key, not two levels.
fn read_json(text: &str, pointer: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut current = &value;
    for part in pointer.trim_start_matches('/').split('/') {
        current = current.get(part)?;
    }
    current.as_str().map(str::to_string)
}

fn set_json(text: &str, pointer: &str, value: Option<&str>) -> Result<String, String> {
    let mut root: serde_json::Value = if text.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(text).map_err(|_| "E_BROKEN_JSON".to_string())?
    };
    let parts: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
    let Some((last, parents)) = parts.split_last() else {
        return Ok(text.to_string());
    };
    let mut current = &mut root;
    for part in parents {
        if !current.is_object() {
            *current = serde_json::Value::Object(serde_json::Map::new());
        }
        let map = current.as_object_mut().ok_or("E_BROKEN_JSON")?;
        current = map
            .entry((*part).to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    }
    if !current.is_object() {
        *current = serde_json::Value::Object(serde_json::Map::new());
    }
    let map = current.as_object_mut().ok_or("E_BROKEN_JSON")?;
    match value {
        Some(value) => {
            map.insert((*last).to_string(), serde_json::Value::String(value.into()));
        }
        None => {
            map.remove(*last);
        }
    }
    serde_json::to_string_pretty(&root).map_err(|error| error.to_string())
}

/// A target the user described. Built-in ids cannot be taken over, and nothing here runs.
pub fn save_custom(target: Target) -> Result<(), String> {
    let id = target.id.trim().to_string();
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("E_TARGET_ID".into());
    }
    if builtin().iter().any(|preset| preset.id == id) {
        return Err("E_TARGET_BUILTIN".into());
    }
    if target.name.trim().is_empty() || target.path().as_os_str().is_empty() {
        return Err("E_TARGET_FIELDS".into());
    }
    // A description may only reach the user's own profile: nothing here should be able to
    // point Stacker at a system file.
    if !inside_home(&target.path()) {
        return Err("E_TARGET_PATH".into());
    }
    let mut settings = crate::settings::load();
    let mut target = target;
    target.id = id.clone();
    target.builtin = false;
    match settings.proxy_targets.iter_mut().find(|t| t.id == id) {
        Some(existing) => *existing = target,
        None => settings.proxy_targets.push(target),
    }
    crate::settings::save_settings(&settings)
}

pub fn remove_custom(id: &str) -> Result<(), String> {
    let mut settings = crate::settings::load();
    settings.proxy_targets.retain(|target| target.id != id);
    crate::settings::save_settings(&settings)
}

/// Whether a path is somewhere Stacker is willing to write: inside the user's own profile.
pub fn inside_home(path: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    path.starts_with(home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_replaced_where_it_is_and_added_when_it_is_missing() {
        let text = "# comment\r\nregistry=https://example\r\nproxy=http://old\r\n";
        let next = set_key_value(text, "proxy", Some("http://127.0.0.1:6789"));
        assert_eq!(
            next,
            "# comment\r\nregistry=https://example\r\nproxy=http://127.0.0.1:6789\r\n"
        );
        assert_eq!(
            read_key_value(&next, "proxy").unwrap(),
            "http://127.0.0.1:6789"
        );
        // Clearing takes the line out and leaves everything else alone.
        let cleared = set_key_value(&next, "proxy", None);
        assert_eq!(cleared, "# comment\r\nregistry=https://example\r\n");
        assert!(read_key_value(&cleared, "proxy").is_none());
        // A file without the key gains it.
        assert_eq!(set_key_value("", "proxy", Some("x")), "proxy=x\r\n");
    }

    #[test]
    fn an_ini_key_lands_in_its_own_section() {
        let text = "[install]\r\ntrusted-host = example\r\n";
        let next = set_ini(text, "global", "proxy", Some("http://p"));
        assert!(next.contains("[global]"));
        assert_eq!(read_ini(&next, "global", "proxy").unwrap(), "http://p");
        assert_eq!(
            read_ini(&next, "install", "trusted-host").unwrap(),
            "example"
        );
        // The same section is reused, not written twice.
        let again = set_ini(&next, "global", "proxy", Some("http://q"));
        assert_eq!(again.matches("[global]").count(), 1);
        assert_eq!(read_ini(&again, "global", "proxy").unwrap(), "http://q");
        let cleared = set_ini(&again, "global", "proxy", None);
        assert!(read_ini(&cleared, "global", "proxy").is_none());
        assert_eq!(
            read_ini(&cleared, "install", "trusted-host").unwrap(),
            "example"
        );
    }

    #[test]
    fn json_keeps_the_rest_of_the_file() {
        let text = r#"{"theme":"dark","env":{"KEEP":"1"}}"#;
        let next = set_json(text, "/env/HTTP_PROXY", Some("http://p")).unwrap();
        assert_eq!(read_json(&next, "/env/HTTP_PROXY").unwrap(), "http://p");
        assert_eq!(read_json(&next, "/env/KEEP").unwrap(), "1");
        assert_eq!(read_json(&next, "/theme").unwrap(), "dark");
        let cleared = set_json(&next, "/env/HTTP_PROXY", None).unwrap();
        assert!(read_json(&cleared, "/env/HTTP_PROXY").is_none());
        assert_eq!(read_json(&cleared, "/env/KEEP").unwrap(), "1");
        // A dotted name is one key, the way VS Code writes it.
        let flat = set_json("{}", "/http.proxy", Some("http://p")).unwrap();
        assert!(flat.contains("\"http.proxy\""));
        // A file that is not JSON is refused rather than overwritten.
        assert!(set_json("not json", "/a", Some("b")).is_err());
    }

    #[test]
    fn a_custom_target_needs_an_id_of_its_own() {
        let target = |id: &str| Target {
            id: id.into(),
            name: "Thing".into(),
            detail: String::new(),
            icon: "ti-point".into(),
            builtin: false,
            detect: Detect::PathExists {
                path: "~/.thing".into(),
            },
            writer: Writer::KeyValue {
                path: "~/.thing/config".into(),
                entries: vec![entry("proxy", "http://{proxy}")],
            },
        };
        assert_eq!(save_custom(target("pip")).unwrap_err(), "E_TARGET_BUILTIN");
        assert_eq!(save_custom(target("has space")).unwrap_err(), "E_TARGET_ID");
        // Only the user's own profile: a description cannot point at a system file.
        let mut outside = target("thing");
        outside.writer = Writer::KeyValue {
            path: "C:/Windows/System32/drivers/etc/hosts".into(),
            entries: vec![entry("proxy", "http://{proxy}")],
        };
        assert_eq!(save_custom(outside).unwrap_err(), "E_TARGET_PATH");
        assert_eq!(save_custom(target("")).unwrap_err(), "E_TARGET_ID");
    }

    #[test]
    fn a_path_is_expanded_the_way_the_shell_reads_it() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand("~/.curlrc"), home.join(".curlrc"));
        assert!(inside_home(&expand("~/.docker/config.json")));
        assert!(!inside_home(Path::new(
            "C:\\Windows\\System32\\drivers\\etc\\hosts"
        )));
    }
}
