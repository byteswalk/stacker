//! How eager the leftover cleanup is, kept beside Stacker's other files.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// A project nobody worked in for this many days gives up its build outputs and temp files.
    pub inactive_days: u32,
    /// Claude's checkpoints of a session untouched this long are dropped (Claude's own default
    /// for command-line sessions is 30 days; desktop sessions it keeps forever).
    pub checkpoint_days: u32,
    /// Take the screenshots out of transcripts; off until the user turns it on.
    pub slim: bool,
    pub slim_days: u32,
    /// Clean what is ticked by default once a day without asking.
    pub auto: bool,
    pub auto_last_run: u64,
    pub auto_last_freed: u64,
    pub auto_last_error: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            inactive_days: 7,
            checkpoint_days: 30,
            slim: false,
            slim_days: 30,
            auto: false,
            auto_last_run: 0,
            auto_last_freed: 0,
            auto_last_error: String::new(),
        }
    }
}

/// What the page may change; the record of the last automatic run is Stacker's own.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub inactive_days: u32,
    pub checkpoint_days: u32,
    pub slim: bool,
    pub slim_days: u32,
    pub auto: bool,
}

fn file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("leftovers.json")
}

pub fn load() -> Settings {
    std::fs::read(file())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::sessions::err)?;
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(crate::sessions::err)?;
    std::fs::write(path, bytes).map_err(crate::sessions::err)
}

pub fn apply(update: SettingsUpdate) -> Result<Settings, String> {
    let days = |value: u32| {
        (1..=365)
            .contains(&value)
            .then_some(value)
            .ok_or("E_REQUEST")
    };
    let mut settings = load();
    settings.inactive_days = days(update.inactive_days)?;
    settings.checkpoint_days = days(update.checkpoint_days)?;
    settings.slim_days = days(update.slim_days)?;
    settings.slim = update.slim;
    settings.auto = update.auto;
    save(&settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_screenshots_and_ask_before_cleaning() {
        let settings = Settings::default();
        assert_eq!(
            (
                settings.inactive_days,
                settings.checkpoint_days,
                settings.slim_days
            ),
            (7, 30, 30)
        );
        assert!(!settings.slim && !settings.auto);
        let partial: Settings = serde_json::from_str(r#"{"inactiveDays":3}"#).unwrap();
        assert_eq!(partial.inactive_days, 3);
        assert_eq!(partial.checkpoint_days, 30);
    }
}
