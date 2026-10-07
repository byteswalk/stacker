//! WinGet's own download setting. By default WinGet hands large installers to Windows'
//! Delivery Optimization, which can crawl and shows no progress; `"network": {"downloader":
//! "wininet"}` in WinGet's settings file makes WinGet download them itself, through the
//! system proxy. Stacker leaves the file alone unless the user picks that, and a change backs
//! the file up first.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

/// WinGet's default: Delivery Optimization for large files.
pub const SYSTEM: &str = "default";
/// WinGet downloads itself.
pub const WININET: &str = "wininet";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetDownloader {
    /// `default`, `wininet` or `do`, as the file says; `default` when it says nothing.
    pub value: String,
    pub path: String,
    /// False when WinGet is not installed here.
    pub available: bool,
    /// After a change: the file there before was copied to the config backups.
    pub backed_up: bool,
}

/// The settings file WinGet reads: the packaged App Installer's, else the unpackaged one.
fn settings_path() -> Option<PathBuf> {
    let local = dirs::data_local_dir()?;
    let packaged = local.join("Packages\\Microsoft.DesktopAppInstaller_8wekyb3d8bbwe");
    if packaged.is_dir() {
        return Some(packaged.join("LocalState\\settings.json"));
    }
    crate::agents::install::winget::winget_command()
        .map(|_| local.join("Microsoft\\WinGet\\Settings\\settings.json"))
}

/// The file is JSON with comments and trailing commas; this leaves plain JSON.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for next in chars.by_ref() {
                    if last == '*' && next == '/' {
                        break;
                    }
                    last = next;
                }
            }
            _ => out.push(c),
        }
    }
    // A comma right before a closing bracket.
    let mut plain = String::with_capacity(out.len());
    let mut in_string = false;
    let mut escaped = false;
    let chars: Vec<char> = out.chars().collect();
    for (index, &c) in chars.iter().enumerate() {
        if in_string {
            in_string = !(c == '"' && !escaped);
            escaped = c == '\\' && !escaped;
        } else if c == '"' {
            in_string = true;
        } else if c == ',' {
            let next = chars[index + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        plain.push(c);
    }
    plain
}

fn parse(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&strip_jsonc(text)) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("E_WINGET_SETTINGS".into()),
        Err(_) => Err("E_WINGET_SETTINGS".into()),
    }
}

pub fn downloader_of(text: &str) -> String {
    parse(text)
        .ok()
        .and_then(|map| {
            map.get("network")?
                .get("downloader")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| SYSTEM.into())
}

/// The file with the downloader set, or taken out for `default` (an emptied `network` goes too).
pub fn with_downloader(text: &str, value: &str) -> Result<String, String> {
    let mut map = parse(text)?;
    if value == SYSTEM {
        if let Some(Value::Object(network)) = map.get_mut("network") {
            network.remove("downloader");
            if network.is_empty() {
                map.remove("network");
            }
        }
    } else {
        let network = map
            .entry("network")
            .or_insert_with(|| Value::Object(Map::new()));
        if !network.is_object() {
            *network = Value::Object(Map::new());
        }
        network
            .as_object_mut()
            .expect("an object")
            .insert("downloader".into(), Value::String(value.into()));
    }
    if !map.contains_key("$schema") {
        map.insert(
            "$schema".into(),
            Value::String("https://aka.ms/winget-settings.schema.json".into()),
        );
    }
    // `$schema` first, as WinGet writes it.
    let mut ordered = Map::new();
    if let Some(schema) = map.remove("$schema") {
        ordered.insert("$schema".into(), schema);
    }
    ordered.extend(map);
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    Value::Object(ordered)
        .serialize(&mut serializer)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "// For documentation on these settings, see: https://aka.ms/winget-settings\n{}\n",
        String::from_utf8_lossy(&out)
    ))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .map(|text| text.trim_start_matches('\u{feff}').to_string())
        .unwrap_or_default()
}

pub fn status() -> WingetDownloader {
    match settings_path() {
        Some(path) => WingetDownloader {
            value: downloader_of(&read(&path)),
            path: path.to_string_lossy().into_owned(),
            available: true,
            backed_up: false,
        },
        None => WingetDownloader {
            value: SYSTEM.into(),
            path: String::new(),
            available: false,
            backed_up: false,
        },
    }
}

pub fn set(value: &str) -> Result<WingetDownloader, String> {
    if ![SYSTEM, WININET, "do"].contains(&value) {
        return Err("E_WINGET_SETTINGS".into());
    }
    let path = settings_path().ok_or("E_NO_WINGET")?;
    let text = read(&path);
    if downloader_of(&text) == value {
        return Ok(status());
    }
    let next = with_downloader(&text, value)?;
    let backed_up = crate::backup::backup_file(&path).is_some();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, next).map_err(|e| e.to_string())?;
    Ok(WingetDownloader {
        backed_up,
        ..status()
    })
}

#[tauri::command]
pub async fn winget_downloader() -> WingetDownloader {
    tauri::async_runtime::spawn_blocking(status)
        .await
        .unwrap_or(WingetDownloader {
            value: SYSTEM.into(),
            path: String::new(),
            available: false,
            backed_up: false,
        })
}

#[tauri::command]
pub async fn winget_downloader_set(value: String) -> Result<WingetDownloader, String> {
    tauri::async_runtime::spawn_blocking(move || set(&value))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINGET_DEFAULT: &str = "{\n    \"$schema\": \"https://aka.ms/winget-settings.schema.json\",\n\n    // For documentation on these settings, see: https://aka.ms/winget-settings\n    // \"source\": {\n    //    \"autoUpdateIntervalInMinutes\": 5\n    // },\n}\n";

    #[test]
    fn the_file_winget_ships_with_says_nothing_about_downloads() {
        assert_eq!(downloader_of(WINGET_DEFAULT), SYSTEM);
        assert_eq!(downloader_of(""), SYSTEM);
        assert_eq!(downloader_of("not json"), SYSTEM);
    }

    #[test]
    fn choosing_wininet_and_going_back_leaves_the_rest_alone() {
        let own = "{\n  \"$schema\": \"x\", /* mine */\n  \"visual\": {\"progressBar\": \"rainbow\"},\n  \"network\": {\"doProgressTimeoutInSeconds\": 60,},\n  \"note\": \"a // in a string\",\n}";
        let on = with_downloader(own, WININET).unwrap();
        assert_eq!(downloader_of(&on), WININET);
        let map = parse(&on).unwrap();
        assert_eq!(map["visual"]["progressBar"], "rainbow");
        assert_eq!(map["network"]["doProgressTimeoutInSeconds"], 60);
        assert_eq!(map["note"], "a // in a string");
        assert!(on.contains("\"$schema\": \"x\""));

        let off = with_downloader(&on, SYSTEM).unwrap();
        assert_eq!(downloader_of(&off), SYSTEM);
        assert_eq!(
            parse(&off).unwrap()["network"]["doProgressTimeoutInSeconds"],
            60
        );

        let fresh = with_downloader(WINGET_DEFAULT, WININET).unwrap();
        let back = parse(&with_downloader(&fresh, SYSTEM).unwrap()).unwrap();
        assert!(!back.contains_key("network"), "an emptied network goes too");
        assert!(with_downloader("[1]", WININET).is_err());
    }
}
