use crate::sessions::model::Agent;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FootprintKind {
    /// Session records: deleted from the 会话 tab, not here.
    Sessions,
    /// Provably unused; selected by default.
    Reclaimable,
    /// Agent work products the user decides about.
    Review,
    /// Needed by the agent; size only.
    Keep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// Shared by the agent's CLI and desktop app.
    Shared,
    /// Belongs to the desktop app.
    DesktopApp,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FootprintItem {
    pub id: String,
    pub agent: Agent,
    pub owner: Owner,
    pub kind: FootprintKind,
    pub label: String,
    pub explain: String,
    pub paths: Vec<String>,
    pub bytes: u64,
    pub files: u64,
    pub blocked: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFootprint {
    pub agent: Agent,
    pub total: u64,
    pub reclaimable: u64,
    pub items: Vec<FootprintItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FootprintReport {
    pub agents: Vec<AgentFootprint>,
    pub total: u64,
    pub reclaimable: u64,
    pub scanned_at: u64,
    pub warnings: Vec<String>,
}
