//! Readable messages from Codex rollouts and Claude transcripts.
use super::model::Agent;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_LINE: u64 = 8 * 1024 * 1024;
const MAX_MESSAGES: usize = 100_000;
const MAX_TEXT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub line: usize,
    pub role: String,
    pub text: String,
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn content(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                if let Some(s) = p.as_str() {
                    return Some(s.to_string());
                }
                match str_at(p, "type") {
                    "text" | "input_text" | "output_text" => {
                        p.get("text").and_then(Value::as_str).map(str::to_owned)
                    }
                    "tool_result" => Some(content(&p["content"])),
                    "tool_use" => Some(format!("{}\n{}", str_at(p, "name"), p["input"])),
                    "image" | "input_image" => Some("[image attachment]".into()),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

enum Parsed {
    Message(String, String),
    /// Codex event copies, used only when a rollout has no response items.
    Fallback(String, String),
    Other,
}

fn parse(agent: Agent, v: &Value) -> Parsed {
    let kind = str_at(v, "type");
    match agent {
        Agent::Codex => {
            let p = &v["payload"];
            match (kind, str_at(p, "type")) {
                ("response_item", "message") => {
                    Parsed::Message(str_at(p, "role").to_string(), content(&p["content"]))
                }
                ("response_item", "function_call") => Parsed::Message(
                    "tool".into(),
                    format!("{}\n{}", str_at(p, "name"), str_at(p, "arguments")),
                ),
                ("response_item", "function_call_output") => {
                    Parsed::Message("tool".into(), content(&p["output"]))
                }
                ("event_msg", "user_message") => {
                    Parsed::Fallback("user".into(), str_at(p, "message").into())
                }
                ("event_msg", "agent_message") => {
                    Parsed::Fallback("assistant".into(), str_at(p, "message").into())
                }
                _ => Parsed::Other,
            }
        }
        Agent::Claude => match kind {
            "user" | "assistant" => {
                Parsed::Message(kind.to_string(), content(&v["message"]["content"]))
            }
            _ => Parsed::Other,
        },
        // CodeBuddy and WorkBuddy keep the role beside the content instead of nesting a
        // message object.
        Agent::CodeBuddy | Agent::WorkBuddy | Agent::WorkBuddyAi => match kind {
            "message" => Parsed::Message(str_at(v, "role").to_string(), content(&v["content"])),
            _ => Parsed::Other,
        },
        // Read from their own stores instead; nothing reaches this parser.
        Agent::MiMo | Agent::Kimi => Parsed::Other,
    }
}

fn next_line(input: &mut impl BufRead, bytes: &mut Vec<u8>) -> Result<usize, String> {
    bytes.clear();
    input
        .take(MAX_LINE + 1)
        .read_until(b'\n', bytes)
        .map_err(super::err)
}

/// All readable messages; `complete` is false when limits were hit or lines were damaged.
pub fn read_session(session: &super::model::Session) -> Result<(Vec<Message>, bool), String> {
    let path = Path::new(&session.path);
    match session.agent {
        // MiMo keeps its turns in a SQLite file shared by every session.
        Agent::MiMo => super::mimo_catalog::read(path, &session.native_id, MAX_MESSAGES),
        // Kimi files a folder per session, with the conversation as a stream of events.
        Agent::Kimi => super::kimi_catalog::read(path, MAX_MESSAGES),
        agent => read(agent, path),
    }
}

/// Whether a session's readable text contains `needle`.
pub fn session_contains(session: &super::model::Session, needle: &str) -> bool {
    let path = Path::new(&session.path);
    match session.agent {
        Agent::MiMo => super::mimo_catalog::contains(path, &session.native_id, needle),
        Agent::Kimi => super::kimi_catalog::contains(path, needle),
        agent => contains(agent, path, needle),
    }
}

pub fn read(agent: Agent, path: &Path) -> Result<(Vec<Message>, bool), String> {
    let file = fs::File::open(path).map_err(|_| "E_NOT_FOUND".to_string())?;
    let mut complete = file.metadata().map(|m| m.len() <= MAX_FILE).unwrap_or(true);
    let mut input = BufReader::new(file.take(MAX_FILE));
    let mut messages = Vec::new();
    let mut fallback = Vec::new();
    let mut text_bytes = 0usize;
    let mut line = 0;
    let mut bytes = Vec::new();
    loop {
        let n = next_line(&mut input, &mut bytes)?;
        if n == 0 {
            break;
        }
        line += 1;
        if n as u64 > MAX_LINE {
            complete = false;
            break;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
            complete = false;
            continue;
        };
        let (role, mut text, target) = match parse(agent, &v) {
            Parsed::Message(role, text) => (role, text, &mut messages),
            Parsed::Fallback(role, text) => (role, text, &mut fallback),
            Parsed::Other => continue,
        };
        if text.len() > 262_144 {
            text = text.chars().take(65_536).collect();
            complete = false;
        }
        if text.trim().is_empty() {
            continue;
        }
        text_bytes += text.len();
        target.push(Message { line, role, text });
        if messages.len() > MAX_MESSAGES || text_bytes > MAX_TEXT {
            complete = false;
            break;
        }
    }
    if messages.is_empty() {
        messages = fallback;
    }
    Ok((messages, complete))
}

/// Streaming, case-insensitive search of the readable text; stops at the first hit.
pub fn contains(agent: Agent, path: &Path, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return false;
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut input = BufReader::new(file.take(MAX_FILE));
    let mut bytes = Vec::new();
    while let Ok(n) = next_line(&mut input, &mut bytes) {
        if n == 0 || n as u64 > MAX_LINE {
            break;
        }
        // Cheap raw check first; most lines never need parsing.
        if !String::from_utf8_lossy(&bytes)
            .to_lowercase()
            .contains(&needle)
        {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let text = match parse(agent, &v) {
            Parsed::Message(_, text) | Parsed::Fallback(_, text) => text,
            Parsed::Other => continue,
        };
        if text.to_lowercase().contains(&needle) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_messages_do_not_duplicate_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"abc\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"hello\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello\"}]}}\n"
            ),
        )
        .unwrap();
        let (messages, complete) = read(Agent::Codex, &path).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(complete);
        assert!(contains(Agent::Codex, &path, "HELLO"));
        assert!(!contains(Agent::Codex, &path, "absent"));
        assert!(!contains(Agent::Codex, &path, "session_meta"));
    }

    #[test]
    fn claude_partial_tail_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"user\",\"sessionId\":\"a\",\"message\":{\"content\":\"question\"}}\n{bad",
        )
        .unwrap();
        let (messages, complete) = read(Agent::Claude, &path).unwrap();
        assert_eq!(messages[0].text, "question");
        assert!(!complete);
    }
}
