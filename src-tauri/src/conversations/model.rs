use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Codex,
    ClaudeCli,
    ClaudeDesktop,
    Import,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub root: String,
    pub enabled: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub native_id: String,
    pub source_id: String,
    pub client: String,
    pub path: String,
    pub title: String,
    pub project: String,
    pub parent_id: String,
    pub modified: u64,
    pub bytes: u64,
    pub fingerprint: String,
    pub archived: bool,
    pub complete: bool,
    pub warning: String,
    pub message_count: usize,
    pub favorite: bool,
    pub hidden: bool,
    pub group_name: String,
    pub summary: String,
    pub summary_fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub line: usize,
    pub role: String,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Detail {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
    pub total: usize,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Query {
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub before: u64,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub full_text: bool,
}

#[derive(Serialize)]
pub struct Listing {
    pub items: Vec<Conversation>,
    pub total: usize,
    pub ids: Vec<String>,
    pub projects: Vec<String>,
    pub indexed: usize,
    pub synced_at: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ModelSettings {
    pub endpoint: String,
    pub model: String,
    #[serde(default)]
    pub key_cipher: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct JobItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub action: String,
    pub state: String,
    pub done: usize,
    pub total: usize,
    pub items: Vec<JobItem>,
    pub output: String,
    pub error: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Preview {
    pub token: String,
    pub action: String,
    pub selected: Vec<String>,
    pub affected: Vec<Conversation>,
    pub blocked: Vec<JobItem>,
    pub bytes: u64,
    pub created: u64,
    pub proofs: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub schema_version: u32,
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SummaryConsent {
    pub token: String,
    pub ids: Vec<String>,
    pub fingerprints: std::collections::BTreeMap<String, String>,
    pub endpoint: String,
    pub model: String,
    pub locale: String,
    pub created: u64,
}
