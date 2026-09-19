//! Messages between the extension and the bridge: `{ id, type, payload }` → `{ id, ok, result | error }`.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
/// Records per sync message.
pub const MAX_BATCH: usize = 200;

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Response {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn success(id: String, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: String, code: &str) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(code.to_string()),
        }
    }
}

/// Browser times are JavaScript numbers in milliseconds, sometimes fractional.
fn millis<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.map_or(0, |v| v.round() as i64))
}

fn millis_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.map(|v| v.round() as i64))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebAccount {
    pub key: String,
    pub site: String,
    pub remote_id: String,
    pub name: String,
    pub alias: String,
    #[serde(deserialize_with = "millis")]
    pub last_seen: i64,
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebConversation {
    pub key: String,
    pub site: String,
    pub account: String,
    pub id: String,
    pub title: String,
    #[serde(deserialize_with = "millis")]
    pub created_at: i64,
    #[serde(deserialize_with = "millis")]
    pub updated_at: i64,
    pub archived: bool,
    #[serde(deserialize_with = "millis_opt")]
    pub removed_at: Option<i64>,
    /// When the site's own fields were last written; the newest listing wins.
    #[serde(deserialize_with = "millis")]
    pub listed_at: i64,
    pub folder_id: Option<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub note: String,
    /// When a local field last changed; the newer record wins.
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebFolder {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "millis")]
    pub created_at: i64,
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebExcerpt {
    pub id: String,
    pub site: String,
    pub conversation_id: Option<String>,
    pub url: String,
    pub page_title: String,
    pub text: String,
    pub note: String,
    #[serde(deserialize_with = "millis")]
    pub created_at: i64,
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebMessage {
    pub role: String,
    pub text: String,
    #[serde(deserialize_with = "millis_opt")]
    pub at: Option<i64>,
    pub attachments: Vec<String>,
}

/// One piece of a body read; a body over ~900 KB arrives in several, split by message index.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BodyChunk {
    pub key: String,
    pub site: String,
    pub account: String,
    pub id: String,
    pub title: String,
    #[serde(deserialize_with = "millis")]
    pub updated_at: i64,
    #[serde(deserialize_with = "millis")]
    pub fetched_at: i64,
    pub chunk: usize,
    pub chunks: usize,
    pub messages: Vec<WebMessage>,
}

/// A whole body as stored in `<id>.json.gz`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredBody {
    pub key: String,
    pub site: String,
    pub account: String,
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    pub fetched_at: i64,
    pub messages: Vec<WebMessage>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Removal {
    /// "folder" | "excerpt"
    pub kind: String,
    pub key: String,
    #[serde(deserialize_with = "millis")]
    pub at: i64,
}

#[derive(Debug, Deserialize)]
pub struct Items<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PullRequest {
    pub section: String,
    pub offset: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SaveExport {
    pub path: String,
    pub text: String,
    pub append: bool,
}

/// Site ids are plain lowercase words (`chatgpt`, `claude`, `gemini`, …); Stacker does not list them.
pub fn is_site(site: &str) -> bool {
    !site.is_empty()
        && site.len() <= 32
        && site
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Conversation keys are `<site>:<id>`, exactly as the extension builds them.
pub fn valid_key(site: &str, id: &str, key: &str) -> bool {
    is_site(site) && !id.is_empty() && id.len() <= 200 && key == format!("{site}:{id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn browser_numbers_become_whole_milliseconds() {
        let c: WebConversation = serde_json::from_value(json!({
            "key": "chatgpt:a", "site": "chatgpt", "account": "chatgpt:u1", "id": "a",
            "title": "Trip", "createdAt": 1.4, "updatedAt": 1_700_000_000_000.6_f64,
            "removedAt": null, "tags": ["x"], "favorite": true
        }))
        .unwrap();
        assert_eq!((c.created_at, c.updated_at), (1, 1_700_000_000_001));
        assert_eq!(c.removed_at, None);
        assert_eq!(
            (c.listed_at, c.local_updated_at),
            (0, 0),
            "missing times default to 0"
        );
        assert!(c.favorite && c.folder_id.is_none());
    }

    #[test]
    fn requests_may_omit_payload_and_responses_omit_empty_fields() {
        let r: Request = serde_json::from_str(r#"{"id":"1","type":"status"}"#).unwrap();
        assert_eq!((r.id.as_str(), r.kind.as_str()), ("1", "status"));
        assert!(r.payload.is_null());
        let ok = serde_json::to_value(Response::success("1".into(), json!({"a": 1}))).unwrap();
        assert_eq!(ok, json!({"id": "1", "ok": true, "result": {"a": 1}}));
        let err = serde_json::to_value(Response::failure("2".into(), "E_PATH")).unwrap();
        assert_eq!(err, json!({"id": "2", "ok": false, "error": "E_PATH"}));
    }

    #[test]
    fn site_ids_and_keys_are_checked() {
        assert!(is_site("chatgpt") && is_site("deepseek") && is_site("x-1"));
        assert!(!is_site("") && !is_site("Chat") && !is_site("a/b") && !is_site(&"a".repeat(33)));
        assert!(valid_key("claude", "c1", "claude:c1"));
        assert!(!valid_key("claude", "c1", "chatgpt:c1"));
        assert!(!valid_key("claude", "", "claude:"));
        assert!(!valid_key("Claude", "c1", "Claude:c1"));
    }
}
