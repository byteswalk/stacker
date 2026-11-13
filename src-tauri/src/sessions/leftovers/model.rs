use super::settings::Settings;
use crate::sessions::model::Agent;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftoverKind {
    /// Build outputs, dependencies and tool caches in the project: made again by a build.
    Build,
    /// Parts of a Rust `target` that have not been built for a while, in a project in use.
    Stale,
    /// Cargo's incremental cache in a project in use: the next build is slower without it,
    /// and an agent waiting on a slow build may time out and try again, so it is not ticked.
    Incremental,
    /// A virtual environment: rebuilt only by downloading every package again.
    Environment,
    /// The scratch folders Claude made for the project's sessions.
    Temp,
    /// Claude's file snapshots for rewinding the project's old sessions.
    Checkpoints,
    /// Screenshots inside the transcripts of the project's old sessions.
    Images,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverItem {
    pub id: String,
    pub kind: LeftoverKind,
    pub label: String,
    pub explain: String,
    /// What the page shows for where it is.
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    /// Ticked by default, and what the automatic cleanup takes.
    pub recommended: bool,
    /// Why it cannot be cleaned now.
    pub blocked: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLeftovers {
    pub key: String,
    pub name: String,
    pub path: String,
    pub agents: Vec<Agent>,
    pub sessions: usize,
    /// The newest of the project's sessions and of its build outputs (Unix seconds).
    pub last_activity: u64,
    pub active: bool,
    /// Why the project's own folder was not looked into.
    pub skipped: Option<String>,
    pub items: Vec<LeftoverItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverReport {
    pub projects: Vec<ProjectLeftovers>,
    pub total: u64,
    pub recommended: u64,
    pub scanned_at: u64,
    pub settings: Settings,
}

/// What cleaning one item does; kept on the backend, the page only names the item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    /// Every path must stay inside this folder.
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    /// Rewrite the transcripts without their screenshots instead of deleting anything.
    pub slim: bool,
}

pub struct Scan {
    pub report: LeftoverReport,
    pub operations: std::collections::HashMap<String, Operation>,
}
