//! Whether Stacker frees space by itself when a disk runs low, and the record of what it did,
//! kept beside Stacker's other files.

use super::model::RunRecord;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How many runs are remembered; older ones fall off the end.
const MAX_RUNS: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// On unless the user turns it off.
    pub enabled: bool,
    /// A disk with this share of its space free, or less, is cleaned.
    pub threshold_percent: u8,
    /// Newest first.
    pub runs: Vec<RunRecord>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_percent: 5,
            runs: Vec::new(),
        }
    }
}

/// What the page may change; the runs are Stacker's own record.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub enabled: bool,
    pub threshold_percent: u8,
}

#[cfg(test)]
fn file() -> PathBuf {
    std::env::temp_dir()
        .join(format!("stacker-test-{}", std::process::id()))
        .join("low-space.json")
}

#[cfg(not(test))]
fn file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("low-space.json")
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
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// 1–50 %: above half free nothing is "low", and 0 would never clean.
pub fn apply(update: SettingsUpdate) -> Result<Settings, String> {
    if !(1..=50).contains(&update.threshold_percent) {
        return Err("E_REQUEST".into());
    }
    let mut settings = load();
    settings.enabled = update.enabled;
    settings.threshold_percent = update.threshold_percent;
    save(&settings)?;
    Ok(settings)
}

/// A finished run joins the record, newest first.
pub fn remember(run: RunRecord) {
    let mut settings = load();
    settings.runs.insert(0, run);
    settings.runs.truncate(MAX_RUNS);
    if let Err(error) = save(&settings) {
        log::warn!("failed to record the low-space cleanup: {error}");
    }
}
