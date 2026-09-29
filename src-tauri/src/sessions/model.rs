use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Claude,
    CodeBuddy,
    #[serde(rename = "workbuddy")]
    WorkBuddy,
    #[serde(rename = "workbuddy-ai")]
    WorkBuddyAi,
    Qoder,
    #[serde(rename = "qoder-cn")]
    QoderCn,
    Antigravity,
    Trae,
    MiMo,
    Kimi,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Codex => "codex",
            Agent::Claude => "claude",
            Agent::CodeBuddy => "codebuddy",
            Agent::WorkBuddy => "workbuddy",
            Agent::WorkBuddyAi => "workbuddy-ai",
            Agent::Qoder => "qoder",
            Agent::QoderCn => "qoder-cn",
            Agent::Antigravity => "antigravity",
            Agent::Trae => "trae",
            Agent::MiMo => "mimo",
            Agent::Kimi => "kimi",
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
    /// Deleted inside the agent's own app, but its transcript is still on disk.
    Discarded,
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
    /// Older transcripts of the same session (Claude writes one per working directory
    /// when a session moves between worktrees); deleted together with `path`.
    #[serde(default)]
    pub copies: Vec<String>,
    /// The agent this conversation was imported from, set only when the original is gone;
    /// while the original is still listed, the import is folded into it instead.
    #[serde(default)]
    pub imported_from: Option<Agent>,
    /// Agents that imported a copy of this conversation into their own store. The copy is
    /// listed here so it is counted once, and it is never deleted with this session.
    #[serde(default)]
    pub imported_by: Vec<Agent>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionQuery {
    /// "" or an agent name.
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
    /// What each agent holds under the rest of the filters, for the source picker.
    pub agents: Vec<AgentCount>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCount {
    pub agent: Agent,
    pub sessions: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub project: ProjectRef,
    pub agents: Vec<Agent>,
    /// Conversations the sessions tab lists for this project.
    pub sessions: usize,
    pub orphans: usize,
    /// Conversations deleted inside the agent's own app, whose transcripts are still here.
    /// They are not listed as conversations, so they are counted apart from them.
    pub discarded: usize,
    pub bytes: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Roots {
    pub codex: String,
    pub claude: String,
    pub claude_desktop_index: String,
    #[serde(default)]
    pub codebuddy: String,
    #[serde(default)]
    pub mimo: String,
    #[serde(default)]
    pub kimi: String,
    #[serde(default)]
    pub workbuddy: String,
    #[serde(default)]
    pub workbuddy_ai: String,
    #[serde(default)]
    pub qoder: String,
    #[serde(default)]
    pub qoder_cn: String,
    #[serde(default)]
    pub antigravity: String,
    #[serde(default)]
    pub trae: String,
}
