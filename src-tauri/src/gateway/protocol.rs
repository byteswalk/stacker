//! OpenAI Chat Completions and Anthropic Messages, text only, over a stateless runner.
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct ModelSpec {
    /// Runner backend id (`codex`, `claude`, `pi`, …).
    pub backend: String,
    pub model: Option<String>,
}

impl ModelSpec {
    /// `<backend>` or `<backend>/<model>`; the model part may itself contain `/`.
    pub fn parse(name: &str) -> Option<Self> {
        let (backend, model) = match name.trim().split_once('/') {
            Some((a, m)) => (a, Some(m.trim().to_string()).filter(|m| !m.is_empty())),
            None => (name.trim(), None),
        };
        let backend = crate::runner::backends::get(backend)?.id.to_string();
        Some(Self { backend, model })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChatRequest {
    pub model_name: String,
    pub model: ModelSpec,
    pub system: String,
    pub turns: Vec<(Role, String)>,
    pub stream: bool,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub kind: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: u16, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            kind,
            message: message.into(),
        }
    }

    pub fn openai_body(&self) -> Value {
        json!({ "error": { "message": self.message, "type": self.kind, "code": self.kind } })
    }

    pub fn anthropic_body(&self) -> Value {
        json!({ "type": "error", "error": { "type": self.kind, "message": self.message } })
    }
}

fn unsupported(what: &str) -> ApiError {
    ApiError::new(
        400,
        "invalid_request_error",
        format!("Stacker gateway supports text chat only; {what} is not supported."),
    )
}

/// Text of a content value: a string, or an array of text parts. Anything else is unsupported.
fn text_of(content: &Value) -> Result<String, ApiError> {
    match content {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s.clone()),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") | Some("input_text") => out.push(
                        part.get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    ),
                    Some(other) => return Err(unsupported(&format!("content type \"{other}\""))),
                    None => return Err(unsupported("untyped content")),
                }
            }
            Ok(out.join("\n"))
        }
        _ => Err(unsupported("this content")),
    }
}

fn model_of(v: &Value) -> Result<(String, ModelSpec), ApiError> {
    let name = v
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let spec = ModelSpec::parse(&name).ok_or_else(|| {
        ApiError::new(
            404,
            "not_found_error",
            format!(
                "Unknown model \"{name}\". Use <agent> or <agent>/<model>, e.g. codex or claude/sonnet."
            ),
        )
    })?;
    Ok((name, spec))
}

fn has_tools(v: &Value) -> bool {
    ["tools", "functions"].iter().any(|k| {
        v.get(k)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    })
}

pub fn parse_openai(v: &Value) -> Result<ChatRequest, ApiError> {
    let (model_name, model) = model_of(v)?;
    if has_tools(v) {
        return Err(unsupported("tool / function calling"));
    }
    let messages = v
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::new(400, "invalid_request_error", "messages is required"))?;
    let mut system = Vec::new();
    let mut turns = Vec::new();
    for m in messages {
        let text = text_of(m.get("content").unwrap_or(&Value::Null))?;
        match m.get("role").and_then(Value::as_str).unwrap_or("") {
            "system" | "developer" => system.push(text),
            "user" => turns.push((Role::User, text)),
            "assistant" => turns.push((Role::Assistant, text)),
            other => return Err(unsupported(&format!("role \"{other}\""))),
        }
    }
    finish(model_name, model, system.join("\n\n"), turns, v)
}

pub fn parse_anthropic(v: &Value) -> Result<ChatRequest, ApiError> {
    let (model_name, model) = model_of(v)?;
    if has_tools(v) {
        return Err(unsupported("tool use"));
    }
    let system = text_of(v.get("system").unwrap_or(&Value::Null))?;
    let messages = v
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::new(400, "invalid_request_error", "messages is required"))?;
    let mut turns = Vec::new();
    for m in messages {
        let text = text_of(m.get("content").unwrap_or(&Value::Null))?;
        match m.get("role").and_then(Value::as_str).unwrap_or("") {
            "user" => turns.push((Role::User, text)),
            "assistant" => turns.push((Role::Assistant, text)),
            other => return Err(unsupported(&format!("role \"{other}\""))),
        }
    }
    finish(model_name, model, system, turns, v)
}

fn finish(
    model_name: String,
    model: ModelSpec,
    system: String,
    turns: Vec<(Role, String)>,
    v: &Value,
) -> Result<ChatRequest, ApiError> {
    if !turns.iter().any(|(r, _)| *r == Role::User) {
        return Err(ApiError::new(
            400,
            "invalid_request_error",
            "at least one user message is required",
        ));
    }
    Ok(ChatRequest {
        model_name,
        model,
        system,
        turns,
        stream: v.get("stream").and_then(Value::as_bool).unwrap_or(false),
        effort: v
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// The whole conversation as one prompt for a stateless runner.
pub fn render_prompt(req: &ChatRequest) -> String {
    let mut out = String::from(
        "You are answering as the assistant in the conversation below. Reply only with the assistant's next message, without any preamble or role label.\n",
    );
    if !req.system.trim().is_empty() {
        out.push_str(&format!("\n<system>\n{}\n</system>\n", req.system.trim()));
    }
    out.push_str("\n<conversation>\n");
    for (role, text) in &req.turns {
        let tag = match role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        out.push_str(&format!("<{tag}>\n{}\n</{tag}>\n", text.trim()));
    }
    out.push_str("</conversation>\n");
    out
}

/// Rough token estimate (≈4 characters per token) so clients get complete usage fields.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

pub fn openai_response(id: &str, created: u64, model: &str, text: &str, prompt: &str) -> Value {
    let (p, c) = (estimate_tokens(prompt), estimate_tokens(text));
    json!({
        "id": id, "object": "chat.completion", "created": created, "model": model,
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": p, "completion_tokens": c, "total_tokens": p + c }
    })
}

pub fn openai_sse(id: &str, created: u64, model: &str, text: &str) -> String {
    let chunk = |delta: Value, finish: Value| {
        json!({ "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }] })
    };
    format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        chunk(json!({ "role": "assistant" }), Value::Null),
        chunk(json!({ "content": text }), Value::Null),
        chunk(json!({}), json!("stop")),
    )
}

pub fn anthropic_response(id: &str, model: &str, text: &str, prompt: &str) -> Value {
    json!({
        "id": id, "type": "message", "role": "assistant", "model": model,
        "content": [{ "type": "text", "text": text }],
        "stop_reason": "end_turn", "stop_sequence": null,
        "usage": { "input_tokens": estimate_tokens(prompt), "output_tokens": estimate_tokens(text) }
    })
}

pub fn anthropic_sse(id: &str, model: &str, text: &str, prompt: &str) -> String {
    let event = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
    let mut out = String::new();
    out.push_str(&event(
        "message_start",
        json!({ "type": "message_start", "message": {
        "id": id, "type": "message", "role": "assistant", "model": model, "content": [],
        "stop_reason": null, "stop_sequence": null,
        "usage": { "input_tokens": estimate_tokens(prompt), "output_tokens": 0 } } }),
    ));
    out.push_str(&event("content_block_start", json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } })));
    out.push_str(&event("content_block_delta", json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } })));
    out.push_str(&event(
        "content_block_stop",
        json!({ "type": "content_block_stop", "index": 0 }),
    ));
    out.push_str(&event("message_delta", json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": estimate_tokens(text) } })));
    out.push_str(&event("message_stop", json!({ "type": "message_stop" })));
    out
}

pub fn models_list(created: u64, ids: &[String]) -> Value {
    json!({ "object": "list", "data": ids.iter().map(|id| json!({ "id": id, "object": "model", "created": created, "owned_by": "stacker" })).collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_parse() {
        assert_eq!(
            ModelSpec::parse("claude"),
            Some(ModelSpec {
                backend: "claude".into(),
                model: None
            })
        );
        assert_eq!(
            ModelSpec::parse("codex/gpt-5.6-sol"),
            Some(ModelSpec {
                backend: "codex".into(),
                model: Some("gpt-5.6-sol".into())
            })
        );
        assert_eq!(ModelSpec::parse("gpt-4o"), None);
    }

    #[test]
    fn openai_requests_render_the_whole_conversation() {
        let v = json!({ "model": "claude/sonnet", "stream": true, "reasoning_effort": "low", "messages": [
            { "role": "system", "content": "Be brief." },
            { "role": "user", "content": "Hi" },
            { "role": "assistant", "content": "Hello!" },
            { "role": "user", "content": [{ "type": "text", "text": "2+3?" }] }
        ]});
        let req = parse_openai(&v).unwrap();
        assert!(req.stream);
        assert_eq!(req.effort.as_deref(), Some("low"));
        assert_eq!(req.model.model.as_deref(), Some("sonnet"));
        let prompt = render_prompt(&req);
        assert!(prompt.contains("<system>\nBe brief.\n</system>"));
        assert!(prompt.find("<user>\nHi").unwrap() < prompt.find("<assistant>\nHello!").unwrap());
        assert!(prompt
            .trim_end()
            .ends_with("<user>\n2+3?\n</user>\n</conversation>"));
    }

    #[test]
    fn anthropic_requests_parse_system_blocks() {
        let v = json!({ "model": "codex", "system": [{ "type": "text", "text": "S" }], "max_tokens": 10,
                        "messages": [{ "role": "user", "content": [{ "type": "text", "text": "Q" }] }] });
        let req = parse_anthropic(&v).unwrap();
        assert_eq!(req.system, "S");
        assert_eq!(req.turns, vec![(Role::User, "Q".to_string())]);
        assert!(!req.stream);
    }

    #[test]
    fn tools_images_and_unknown_models_are_rejected() {
        let tools = json!({ "model": "claude", "tools": [{ "type": "function" }], "messages": [{ "role": "user", "content": "x" }] });
        assert_eq!(parse_openai(&tools).unwrap_err().status, 400);
        let image = json!({ "model": "claude", "messages": [{ "role": "user", "content": [{ "type": "image_url" }] }] });
        assert!(parse_openai(&image)
            .unwrap_err()
            .message
            .contains("image_url"));
        let unknown =
            json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "x" }] });
        assert_eq!(parse_openai(&unknown).unwrap_err().status, 404);
        let no_user =
            json!({ "model": "claude", "messages": [{ "role": "system", "content": "x" }] });
        assert_eq!(parse_openai(&no_user).unwrap_err().status, 400);
    }

    #[test]
    fn stream_bodies_are_well_formed() {
        let sse = openai_sse("id1", 1, "claude", "5");
        let lines: Vec<&str> = sse.lines().filter(|l| l.starts_with("data: ")).collect();
        assert_eq!(lines.last(), Some(&"data: [DONE]"));
        for l in &lines[..lines.len() - 1] {
            serde_json::from_str::<Value>(&l[6..]).unwrap();
        }
        assert!(sse.contains("\"content\":\"5\""));
        let a = anthropic_sse("m1", "claude", "5", "p");
        assert!(
            a.starts_with("event: message_start")
                && a.contains("\"text\":\"5\"")
                && a.trim_end().ends_with("{\"type\":\"message_stop\"}")
        );
        assert_eq!(
            openai_response("x", 1, "claude", "5", "abcd")["usage"]["prompt_tokens"],
            1
        );
    }
}
