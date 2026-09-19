use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Claude,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Codex => "codex",
            Agent::Claude => "claude",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientTag {
    Desktop,
    Terminal,
    Ide,
    Automation,
    Sdk,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Archived,
    Orphaned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    Client,
    Custom,
    Summary,
    FirstMessage,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub key: String,
    pub name: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub bytes: u64,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub native_id: String,
    pub title: String,
    pub title_source: TitleSource,
    pub project: ProjectRef,
    pub client: ClientTag,
    pub created_at: u64,
    pub updated_at: u64,
    pub archived: bool,
    pub pinned: bool,
    pub status: SessionStatus,
    /// Sub-agent and review runs attached to this session.
    pub children: Vec<ChildSummary>,
    /// Transcript + children + related directories.
    pub bytes: u64,
    /// Main transcript file.
    pub path: String,
    /// Present in the Claude desktop sidebar (Claude only).
    pub in_desktop_index: bool,
    /// A sub-agent whose parent session no longer exists.
    pub parent_missing: bool,
    pub favorite: bool,
    pub summary: Option<String>,
    pub summary_stale: bool,
    /// "codex / <model> / <effort>" of the run that wrote the summary.
    pub summary_by: String,
    pub summary_at: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionQuery {
    /// "" | "codex" | "claude"
    pub agent: String,
    /// Project key.
    pub project: String,
    /// "" | "active" | "archived" | "orphaned"
    pub status: String,
    /// "" | client tag name
    pub client: String,
    pub search: String,
    pub full_text: bool,
    pub include_automation: bool,
    pub favorites_only: bool,
    pub updated_after: u64,
    /// "" (most recent first) | "bytes" (largest first)
    pub sort: String,
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    pub items: Vec<Session>,
    pub total: usize,
    pub ids: Vec<String>,
    pub total_bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub project: ProjectRef,
    pub agents: Vec<Agent>,
    pub sessions: usize,
    pub orphans: usize,
    pub bytes: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Roots {
    pub codex: String,
    pub claude: String,
    pub claude_desktop_index: String,
}
