//! What a low-space cleanup works through and what it writes down.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The order space is freed in: what costs least to make again goes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    /// Build output of projects nobody has built or worked in lately.
    IdleBuild,
    /// Build output of projects in use, the least recently built first; never one being
    /// written right now or one a running program was started from.
    Build,
    /// Package managers' download caches, then the dependency folders of idle projects.
    Caches,
    /// Temporary files and the caches of the system and the user's programs.
    System,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::IdleBuild, Step::Build, Step::Caches, Step::System];

    /// Idle build output is worth nothing and goes whole; from the next step on the cleanup
    /// stops as soon as the disk has room again.
    pub fn stops_at_goal(self) -> bool {
        self != Step::IdleBuild
    }
}

/// How a candidate is removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    /// The folder or file itself.
    Whole,
    /// What is inside the folder; the folder stays, as the program that owns it expects.
    Contents,
    /// The entries of a temporary folder nothing has written to for this many seconds.
    OldEntries(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub step: Step,
    pub path: PathBuf,
    pub removal: Removal,
    pub bytes: u64,
    /// Chinese text, or a `spaceAnalysis.known.*` key the page names.
    pub label: String,
    /// When anything inside was last written (Unix seconds): the oldest go first.
    pub newest: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRecord {
    pub path: String,
    pub label: String,
    pub freed: u64,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    pub step: Step,
    pub freed: u64,
    pub items: Vec<ItemRecord>,
    /// Items cleaned but not listed, so a record stays small.
    #[serde(default)]
    pub more: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    /// The disk's root, `D:\` or `/`.
    pub drive: String,
    pub at: u64,
    pub finished_at: u64,
    pub total: u64,
    pub free_before: u64,
    pub free_after: u64,
    pub freed: u64,
    /// Started from the page rather than by the disk running low on its own.
    pub manual: bool,
    pub steps: Vec<StepRecord>,
    /// What was left alone and why, in a sentence each.
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub drive: String,
    pub step: Option<Step>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub threshold_percent: u8,
    pub running: Option<Progress>,
    pub runs: Vec<RunRecord>,
    /// The local disks and their space now, so the page can say how each low one fared.
    pub disks: Vec<DiskSpace>,
}

/// What "check now" found: the disks it is cleaning and every disk's share of free space.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutcome {
    pub cleaning: Vec<String>,
    pub disks: Vec<DiskSpace>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub root: String,
    pub total: u64,
    pub free: u64,
}
