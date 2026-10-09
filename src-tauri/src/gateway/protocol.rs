//! OpenAI Chat Completions and Anthropic Messages over a stateless runner: text, images, PDF and
//! plain-text documents; no tools.
use crate::runner::attachment::{decode_base64, encode_base64, sniff_image};
use crate::runner::{Attachment, AttachmentKind};
use serde_json::{json, Value};
use std::time::Duration;

/// Largest single image or document, decoded or downloaded.
pub const MAX_ATTACHMENT: usize = 20 * 1024 * 1024;
/// Attachments (and downloads) one request may carry: a body full of URLs to large files
/// would otherwise be fetched and held in memory one after another.
pub const MAX_ATTACHMENTS: usize = 20;

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
    /// Each turn's text; attachments appear in it as `[attachment N: …]`.
    pub turns: Vec<(Role, String)>,
    /// Images and PDFs of the whole conversation, numbered from 1 in order.
    pub attachments: Vec<Attachment>,
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

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "invalid_request_error", message)
}

fn unsupported(what: &str) -> ApiError {
    invalid(format!("Stacker gateway does not support {what}."))
}

fn no_tools() -> ApiError {
    unsupported("tool / function calling; the agents run without tools")
}

/// Downloads an http(s) URL: its content type and body.
pub type Fetch<'a> = &'a dyn Fn(&str) -> Result<(String, Vec<u8>), ApiError>;

/// Downloads an image or document URL a request names, capped at `MAX_ATTACHMENT`.
pub fn download(url: &str) -> Result<(String, Vec<u8>), ApiError> {
    use std::io::Read;
    // Only the public internet: a caller with the key must not make the gateway read this
    // machine's or the LAN's own services (a proxy's controller, a router page) and get them
    // back through the model.
    if !public_host(url) {
        return Err(invalid(
            "Attachment URLs must point to a public internet address.",
        ));
    }
    let failed = || invalid(format!("Could not download {url}."));
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        // A redirect could lead from a public address to a private one.
        .redirects(0);
    if let Some(proxy) = crate::agents::net::stacker_proxy() {
        if let Ok(proxy) = ureq::Proxy::new(&proxy) {
            builder = builder.proxy(proxy);
        }
    }
    let response = builder
        .build()
        .get(url)
        .set("User-Agent", "Stacker")
        .call()
        // No reason given: refused, timed out and HTTP errors told apart are a port scan.
        .map_err(|_| failed())?;
    if !(200..300).contains(&response.status()) {
        return Err(failed());
    }
    let too_large = || {
        invalid(format!(
            "{url} is larger than {} MB.",
            MAX_ATTACHMENT / 1024 / 1024
        ))
    };
    let length = response
        .header("Content-Length")
        .and_then(|l| l.trim().parse::<usize>().ok());
    if length.is_some_and(|l| l > MAX_ATTACHMENT) {
        return Err(too_large());
    }
    let content_type = response.content_type().to_string();
    let mut body = Vec::new();
    response
        .into_reader()
        .take(MAX_ATTACHMENT as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|_| failed())?;
    if body.len() > MAX_ATTACHMENT {
        return Err(too_large());
    }
    Ok((content_type, body))
}

/// Whether every address the URL's host resolves to is a public one.
fn public_host(url: &str) -> bool {
    use std::net::{IpAddr, ToSocketAddrs};
    let Some(rest) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = if let Some(v6) = host_port.strip_prefix('[') {
        let Some((host, after)) = v6.split_once(']') else {
            return false;
        };
        (
            host.to_string(),
            after.strip_prefix(':').and_then(|p| p.parse().ok()),
        )
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port.parse().ok()),
            None => (host_port.to_string(), None),
        }
    };
    let port = port.unwrap_or(if url.to_ascii_lowercase().starts_with("https") {
        443
    } else {
        80
    });
    let Ok(addresses) = (host.as_str(), port).to_socket_addrs() else {
        return false;
    };
    let public = |ip: IpAddr| match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1])
                || v4.octets()[0] == 0)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| !v4.is_global_like()))
        }
    };
    let mut any = false;
    for address in addresses {
        any = true;
        if !public(address.ip()) {
            return false;
        }
    }
    any
}

/// The IPv4 checks above, for an address mapped into IPv6.
trait GlobalLike {
    fn is_global_like(&self) -> bool;
}
impl GlobalLike for std::net::Ipv4Addr {
    fn is_global_like(&self) -> bool {
        !(self.is_loopback()
            || self.is_private()
            || self.is_link_local()
            || self.is_unspecified()
            || self.is_broadcast())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Image,
    Document,
}

/// Media types read as text and inlined into the prompt.
fn is_text(media_type: &str, name: Option<&str>) -> bool {
    let media_type = media_type.to_ascii_lowercase();
    if media_type.starts_with("text/")
        || [
            "application/json",
            "application/xml",
            "application/yaml",
            "application/x-yaml",
            "application/toml",
            "application/javascript",
            "application/x-sh",
            "application/sql",
        ]
        .contains(&media_type.as_str())
        || media_type.ends_with("+json")
        || media_type.ends_with("+xml")
    {
        return true;
    }
    let ext = name
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase());
    matches!(media_type.as_str(), "" | "application/octet-stream")
        && ext.is_some_and(|e| {
            [
                "txt", "md", "markdown", "csv", "tsv", "json", "jsonl", "xml", "yaml", "yml",
                "toml", "html", "htm", "log", "ini",
            ]
            .contains(&e.as_str())
        })
}

/// `data:<type>[;…];base64,<data>`: the media type and the decoded bytes.
fn data_url(url: &str) -> Result<(String, Vec<u8>), ApiError> {
    let rest = &url[5..];
    let (meta, data) = rest
        .split_once(',')
        .ok_or_else(|| invalid("Malformed data: URL."))?;
    let mut fields = meta.split(';');
    let media_type = fields.next().unwrap_or("").trim().to_string();
    if !fields.any(|f| f.trim().eq_ignore_ascii_case("base64")) {
        return Err(invalid("data: URLs must be base64 encoded."));
    }
    Ok((media_type, base64(data)?))
}

fn base64(data: &str) -> Result<Vec<u8>, ApiError> {
    if data.len() / 4 * 3 > MAX_ATTACHMENT + 3 {
        return Err(invalid(format!(
            "Attachments may be at most {} MB each.",
            MAX_ATTACHMENT / 1024 / 1024
        )));
    }
    decode_base64(data).ok_or_else(|| invalid("Attachment data is not valid base64."))
}

/// Collects one conversation's attachments while its turns are read.
struct Parts<'a> {
    attachments: Vec<Attachment>,
    fetch: Fetch<'a>,
    downloads: std::cell::Cell<usize>,
}

impl Parts<'_> {
    /// Bytes of a `data:` URL or a downloaded http(s) URL.
    fn url(&self, url: &str) -> Result<(String, Vec<u8>), ApiError> {
        let url = url.trim();
        let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
        match scheme.as_deref() {
            Some("data") => data_url(url),
            Some("http") | Some("https") => {
                if self.downloads.get() >= MAX_ATTACHMENTS {
                    return Err(invalid(format!(
                        "A request may download at most {MAX_ATTACHMENTS} attachments."
                    )));
                }
                self.downloads.set(self.downloads.get() + 1);
                (self.fetch)(url)
            }
            _ => Err(invalid(
                "Image and file URLs must be data: URLs or http(s) URLs.",
            )),
        }
    }

    /// Adds an image or PDF and returns its marker, or returns a text document inlined.
    fn add(
        &mut self,
        media_type: &str,
        bytes: Vec<u8>,
        name: Option<String>,
        expect: Expect,
    ) -> Result<String, ApiError> {
        if bytes.len() > MAX_ATTACHMENT {
            return Err(invalid(format!(
                "Attachments may be at most {} MB each.",
                MAX_ATTACHMENT / 1024 / 1024
            )));
        }
        if self.attachments.len() >= MAX_ATTACHMENTS {
            return Err(invalid(format!(
                "A request may carry at most {MAX_ATTACHMENTS} attachments."
            )));
        }
        let media_type = media_type.trim().to_ascii_lowercase();
        let n = self.attachments.len() + 1;
        let (kind, media_type, marker) = if let Some(image) = sniff_image(&bytes) {
            // The bytes decide: clients often label a JPEG as PNG and the model rejects that.
            (
                AttachmentKind::Image,
                image.to_string(),
                format!("[attachment {n}: image]"),
            )
        } else if bytes.starts_with(b"%PDF-") && expect == Expect::Document {
            let marker = match &name {
                Some(name) => format!("[attachment {n}: PDF document \"{name}\"]"),
                None => format!("[attachment {n}: PDF document]"),
            };
            (AttachmentKind::Pdf, "application/pdf".to_string(), marker)
        } else if expect == Expect::Image {
            return Err(invalid(format!(
                "Unsupported image type \"{media_type}\"; send PNG, JPEG, GIF or WebP."
            )));
        } else if media_type == "application/pdf" {
            return Err(invalid("The PDF document is not a valid PDF file."));
        } else if is_text(&media_type, name.as_deref()) {
            let text = String::from_utf8_lossy(&bytes);
            return Ok(text_document(name.as_deref(), &text));
        } else {
            return Err(invalid(format!(
                "Unsupported document type \"{media_type}\"; send a PDF or a plain-text document."
            )));
        };
        self.attachments.push(Attachment {
            kind,
            media_type,
            data: encode_base64(&bytes),
            name,
        });
        Ok(marker)
    }

    /// Text of a content value: a string or an array of parts, with attachments collected.
    fn content(&mut self, content: &Value) -> Result<String, ApiError> {
        match content {
            Value::Null => Ok(String::new()),
            Value::String(s) => Ok(s.clone()),
            Value::Array(parts) => {
                let mut out = Vec::new();
                for part in parts {
                    if let Some(text) = self.part(part)? {
                        out.push(text);
                    }
                }
                Ok(out.join("\n"))
            }
            _ => Err(unsupported("this content")),
        }
    }

    /// One content part of either API.
    fn part(&mut self, part: &Value) -> Result<Option<String>, ApiError> {
        let str_of = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
        let Some(kind) = part.get("type").and_then(Value::as_str) else {
            return Err(unsupported("untyped content"));
        };
        match kind {
            "text" | "input_text" => Ok(Some(str_of(part, "text").unwrap_or_default())),
            "refusal" => Ok(str_of(part, "refusal")),
            // Earlier reasoning a client sends back carries nothing the agent can use.
            "thinking" | "redacted_thinking" => Ok(None),
            // OpenAI: `image_url` is `{ url }`, or a plain string in the Responses shape.
            "image_url" | "input_image" => {
                let image = part.get("image_url");
                let url = image
                    .and_then(|i| i.get("url").and_then(Value::as_str).or(i.as_str()))
                    .ok_or_else(|| {
                        if part.get("file_id").is_some() {
                            unsupported("file ids; send the image as a data: URL")
                        } else {
                            invalid("image_url.url is required")
                        }
                    })?;
                let (media_type, bytes) = self.url(url)?;
                self.add(&media_type, bytes, None, Expect::Image).map(Some)
            }
            // OpenAI: `{ type: "file", file: { file_data, filename } }` or the flat `input_file`.
            "file" | "input_file" => {
                let file = part.get("file").unwrap_or(part);
                let name = str_of(file, "filename");
                let (media_type, bytes) = if let Some(data) = str_of(file, "file_data") {
                    if data.trim_start().starts_with("data:") {
                        data_url(data.trim())?
                    } else {
                        (String::new(), base64(&data)?)
                    }
                } else if let Some(url) = str_of(file, "file_url") {
                    self.url(&url)?
                } else if file.get("file_id").is_some() {
                    return Err(unsupported("file ids; send the file as file_data"));
                } else {
                    return Err(invalid("file.file_data is required"));
                };
                self.add(&media_type, bytes, name, Expect::Document)
                    .map(Some)
            }
            // Anthropic: `{ type: "image" | "document", source: { … } }`.
            "image" | "document" => {
                let expect = if kind == "image" {
                    Expect::Image
                } else {
                    Expect::Document
                };
                let source = part
                    .get("source")
                    .ok_or_else(|| invalid(format!("{kind}.source is required")))?;
                let name = str_of(part, "title");
                match source.get("type").and_then(Value::as_str).unwrap_or("") {
                    "base64" => {
                        let data = str_of(source, "data").unwrap_or_default();
                        let media_type = str_of(source, "media_type").unwrap_or_default();
                        self.add(&media_type, base64(&data)?, name, expect)
                            .map(Some)
                    }
                    "url" => {
                        let url = str_of(source, "url").unwrap_or_default();
                        let (media_type, bytes) = self.url(&url)?;
                        self.add(&media_type, bytes, name, expect).map(Some)
                    }
                    "text" if expect == Expect::Document => Ok(Some(text_document(
                        name.as_deref(),
                        &str_of(source, "data").unwrap_or_default(),
                    ))),
                    "file" => Err(unsupported("file ids; send the data as base64")),
                    other => Err(unsupported(&format!("{kind} source type \"{other}\""))),
                }
            }
            "tool_use"
            | "tool_result"
            | "server_tool_use"
            | "web_search_tool_result"
            | "tool_call"
            | "function_call"
            | "function_call_output" => Err(no_tools()),
            other => Err(unsupported(&format!("content type \"{other}\""))),
        }
    }
}

fn text_document(name: Option<&str>, text: &str) -> String {
    let title = name.map(|n| format!(" name=\"{n}\"")).unwrap_or_default();
    format!("<document{title}>\n{}\n</document>", text.trim_end())
}

/// Text only: the system prompt carries no attachments.
fn system_text(content: &Value) -> Result<String, ApiError> {
    let mut parts = Parts {
        attachments: Vec::new(),
        downloads: std::cell::Cell::new(0),
        fetch: &|_: &str| Err(unsupported("attachments in the system prompt")),
    };
    let text = parts.content(content)?;
    if parts.attachments.is_empty() {
        Ok(text)
    } else {
        Err(unsupported("attachments in the system prompt"))
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
    parse_openai_with(v, &download)
}

pub fn parse_openai_with(v: &Value, fetch: Fetch) -> Result<ChatRequest, ApiError> {
    let (model_name, model) = model_of(v)?;
    if has_tools(v) {
        return Err(no_tools());
    }
    let messages = v
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("messages is required"))?;
    let mut parts = Parts {
        attachments: Vec::new(),
        downloads: std::cell::Cell::new(0),
        fetch,
    };
    let mut system = Vec::new();
    let mut turns = Vec::new();
    for m in messages {
        let content = m.get("content").unwrap_or(&Value::Null);
        match m.get("role").and_then(Value::as_str).unwrap_or("") {
            "system" | "developer" => system.push(system_text(content)?),
            "user" => turns.push((Role::User, parts.content(content)?)),
            "assistant"
                if m.get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty()) =>
            {
                return Err(no_tools())
            }
            "assistant" => turns.push((Role::Assistant, parts.content(content)?)),
            "tool" | "function" => return Err(no_tools()),
            other => return Err(unsupported(&format!("role \"{other}\""))),
        }
    }
    let effort = effort_field(
        v.get("reasoning_effort"),
        OPENAI_EFFORTS,
        "reasoning_effort",
    )?;
    finish(
        model_name,
        model,
        system.join("\n\n"),
        turns,
        parts,
        v,
        effort,
    )
}

pub fn parse_anthropic(v: &Value) -> Result<ChatRequest, ApiError> {
    parse_anthropic_with(v, &download)
}

pub fn parse_anthropic_with(v: &Value, fetch: Fetch) -> Result<ChatRequest, ApiError> {
    let (model_name, model) = model_of(v)?;
    if has_tools(v) {
        return Err(no_tools());
    }
    let system = system_text(v.get("system").unwrap_or(&Value::Null))?;
    let messages = v
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("messages is required"))?;
    let mut parts = Parts {
        attachments: Vec::new(),
        downloads: std::cell::Cell::new(0),
        fetch,
    };
    let mut turns = Vec::new();
    for m in messages {
        let content = m.get("content").unwrap_or(&Value::Null);
        match m.get("role").and_then(Value::as_str).unwrap_or("") {
            "user" => turns.push((Role::User, parts.content(content)?)),
            "assistant" => turns.push((Role::Assistant, parts.content(content)?)),
            other => return Err(unsupported(&format!("role \"{other}\""))),
        }
    }
    let effort = effort_field(
        v.get("output_config").and_then(|c| c.get("effort")),
        ANTHROPIC_EFFORTS,
        "output_config.effort",
    )?;
    finish(model_name, model, system, turns, parts, v, effort)
}

/// The values OpenAI's Chat Completions API takes for `reasoning_effort`.
pub const OPENAI_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
/// The values Anthropic's Messages API takes for `output_config.effort`.
pub const ANTHROPIC_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// A reasoning level as each API defines it: absent, or one of that API's own values. The
/// other API's field is not read: each endpoint follows its own specification.
fn effort_field(
    value: Option<&Value>,
    allowed: &[&str],
    field: &str,
) -> Result<Option<String>, ApiError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(level)) if allowed.contains(&level.as_str()) => Ok(Some(level.clone())),
        Some(other) => Err(invalid(format!(
            "Invalid value for '{field}': {other}. Supported values are: {}.",
            allowed
                .iter()
                .map(|level| format!("'{level}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn finish(
    model_name: String,
    model: ModelSpec,
    system: String,
    turns: Vec<(Role, String)>,
    parts: Parts,
    v: &Value,
    effort: Option<String>,
) -> Result<ChatRequest, ApiError> {
    if !turns.iter().any(|(r, _)| *r == Role::User) {
        return Err(invalid("at least one user message is required"));
    }
    Ok(ChatRequest {
        model_name,
        model,
        system,
        turns,
        attachments: parts.attachments,
        stream: v.get("stream").and_then(Value::as_bool).unwrap_or(false),
        effort,
    })
}

/// The whole conversation as one prompt for a stateless runner.
pub fn render_prompt(req: &ChatRequest) -> String {
    let mut out = String::from(
        "You are answering as the assistant in the conversation below. Reply only with the assistant's next message, without any preamble or role label.\n",
    );
    if !req.attachments.is_empty() {
        out.push_str(
            "The files marked [attachment N] in the conversation are attached to this message, in the same order.\n",
        );
    }
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

/// Server-sent events of one streamed answer, in either API's shape.
pub struct SseStream {
    pub anthropic: bool,
    pub id: String,
    pub created: u64,
    pub model: String,
    pub prompt: String,
}

impl SseStream {
    fn chunk(&self, delta: Value, finish: Value) -> String {
        let chunk = json!({ "id": self.id, "object": "chat.completion.chunk", "created": self.created,
            "model": self.model, "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }] });
        format!("data: {chunk}\n\n")
    }

    fn event(name: &str, data: Value) -> String {
        format!("event: {name}\ndata: {data}\n\n")
    }

    /// Opens the answer: the assistant role, or `message_start` and the text block.
    pub fn start(&self) -> String {
        if !self.anthropic {
            return self.chunk(json!({ "role": "assistant" }), Value::Null);
        }
        let mut out = Self::event(
            "message_start",
            json!({ "type": "message_start", "message": {
            "id": self.id, "type": "message", "role": "assistant", "model": self.model, "content": [],
            "stop_reason": null, "stop_sequence": null,
            "usage": { "input_tokens": estimate_tokens(&self.prompt), "output_tokens": 0 } } }),
        );
        out.push_str(&Self::event("content_block_start", json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } })));
        out
    }

    pub fn delta(&self, text: &str) -> String {
        if self.anthropic {
            Self::event(
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } }),
            )
        } else {
            self.chunk(json!({ "content": text }), Value::Null)
        }
    }

    /// Closes the answer; `text` is everything sent, for the usage count.
    pub fn end(&self, text: &str) -> String {
        if !self.anthropic {
            return format!("{}data: [DONE]\n\n", self.chunk(json!({}), json!("stop")));
        }
        let mut out = Self::event(
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        );
        out.push_str(&Self::event("message_delta", json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": estimate_tokens(text) } })));
        out.push_str(&Self::event(
            "message_stop",
            json!({ "type": "message_stop" }),
        ));
        out
    }

    /// Ends a stream that failed after it started.
    pub fn error(&self, e: &ApiError) -> String {
        if self.anthropic {
            Self::event("error", e.anthropic_body())
        } else {
            format!("data: {}\n\ndata: [DONE]\n\n", e.openai_body())
        }
    }

    /// A whole answer as one burst, for agents that cannot stream.
    pub fn whole(&self, text: &str) -> String {
        format!("{}{}{}", self.start(), self.delta(text), self.end(text))
    }
}

pub fn openai_sse(id: &str, created: u64, model: &str, text: &str) -> String {
    SseStream {
        anthropic: false,
        id: id.into(),
        created,
        model: model.into(),
        prompt: String::new(),
    }
    .whole(text)
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
    SseStream {
        anthropic: true,
        id: id.into(),
        created: 0,
        model: model.into(),
        prompt: prompt.into(),
    }
    .whole(text)
}

pub fn models_list(created: u64, ids: &[String]) -> Value {
    json!({ "object": "list", "data": ids.iter().map(|id| json!({ "id": id, "object": "model", "created": created, "owned_by": "stacker" })).collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_urls_may_only_reach_the_public_internet() {
        for url in [
            "http://127.0.0.1:9090/configs",
            "http://localhost/",
            "http://169.254.169.254/latest/meta-data",
            "http://10.0.0.1/",
            "http://192.168.1.1/admin",
            "http://[::1]:8080/",
            "http://user@127.0.0.1/",
            "file:///C:/Windows/win.ini",
        ] {
            assert!(!public_host(url), "{url}");
        }
        assert!(public_host("https://1.1.1.1/image.png"));
    }

    /// 1×1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    const PDF: &[u8] = b"%PDF-1.4\n%fake\n";

    fn no_fetch(url: &str) -> Result<(String, Vec<u8>), ApiError> {
        panic!("unexpected download of {url}")
    }

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
        assert!(!prompt.contains("[attachment"));
    }

    #[test]
    fn anthropic_requests_parse_system_blocks() {
        let v = json!({ "model": "codex", "system": [{ "type": "text", "text": "S" }], "max_tokens": 10,
                        "messages": [{ "role": "user", "content": [{ "type": "text", "text": "Q" }] }] });
        let req = parse_anthropic(&v).unwrap();
        assert_eq!(req.system, "S");
        assert_eq!(req.turns, vec![(Role::User, "Q".to_string())]);
        assert!(req.attachments.is_empty());
        assert!(!req.stream);
    }

    #[test]
    fn tools_and_unknown_models_are_rejected() {
        let tools = json!({ "model": "claude", "tools": [{ "type": "function" }], "messages": [{ "role": "user", "content": "x" }] });
        let e = parse_openai(&tools).unwrap_err();
        assert_eq!(e.status, 400);
        assert!(e.message.contains("tool") && !e.message.contains("text chat only"));
        let tool_turn = json!({ "model": "claude", "messages": [{ "role": "user", "content": "x" }, { "role": "tool", "content": "y" }] });
        assert_eq!(parse_openai(&tool_turn).unwrap_err().status, 400);
        let tool_result = json!({ "model": "claude", "messages": [{ "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t" }] }] });
        assert!(parse_anthropic(&tool_result)
            .unwrap_err()
            .message
            .contains("tool"));
        let audio = json!({ "model": "claude", "messages": [{ "role": "user", "content": [{ "type": "input_audio" }] }] });
        assert!(parse_openai(&audio)
            .unwrap_err()
            .message
            .contains("input_audio"));
        let unknown =
            json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "x" }] });
        assert_eq!(parse_openai(&unknown).unwrap_err().status, 404);
        let no_user =
            json!({ "model": "claude", "messages": [{ "role": "system", "content": "x" }] });
        assert_eq!(parse_openai(&no_user).unwrap_err().status, 400);
    }

    #[test]
    fn openai_images_and_files_become_attachments() {
        let pdf = format!("data:application/pdf;base64,{}", encode_base64(PDF));
        let notes = format!(
            "data:text/markdown;base64,{}",
            encode_base64(b"# Notes\nbuy milk")
        );
        let v = json!({ "model": "claude", "messages": [
            { "role": "user", "content": [
                { "type": "text", "text": "Compare these." },
                { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{PNG}") } },
                { "type": "file", "file": { "file_data": pdf, "filename": "report.pdf" } },
                { "type": "file", "file": { "file_data": notes, "filename": "notes.md" } }
            ]},
            { "role": "assistant", "content": "Done." },
            { "role": "user", "content": [
                { "type": "input_image", "image_url": format!("data:image/jpeg;base64,{PNG}") },
                { "type": "input_file", "file_data": encode_base64(b"a,b\n1,2"), "filename": "t.csv" }
            ]}
        ]});
        let req = parse_openai_with(&v, &no_fetch).unwrap();
        assert_eq!(req.attachments.len(), 3);
        assert_eq!(req.attachments[0].kind, AttachmentKind::Image);
        assert_eq!(req.attachments[0].media_type, "image/png");
        assert_eq!(req.attachments[1].kind, AttachmentKind::Pdf);
        assert_eq!(req.attachments[1].name.as_deref(), Some("report.pdf"));
        assert_eq!(req.attachments[1].data, encode_base64(PDF));
        assert_eq!(
            req.attachments[2].media_type, "image/png",
            "the bytes decide the image type"
        );
        let first = &req.turns[0].1;
        assert!(first.contains(
            "Compare these.\n[attachment 1: image]\n[attachment 2: PDF document \"report.pdf\"]"
        ));
        assert!(first.contains("<document name=\"notes.md\">\n# Notes\nbuy milk\n</document>"));
        let last = &req.turns[2].1;
        assert!(last.contains("[attachment 3: image]"));
        assert!(last.contains("<document name=\"t.csv\">\na,b\n1,2\n</document>"));
        let prompt = render_prompt(&req);
        assert!(prompt.contains("marked [attachment N]"));
    }

    #[test]
    fn anthropic_images_and_documents_become_attachments() {
        let v = json!({ "model": "claude", "max_tokens": 10, "messages": [{ "role": "user", "content": [
            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": PNG } },
            { "type": "document", "title": "spec", "source": { "type": "base64", "media_type": "application/pdf", "data": encode_base64(PDF) } },
            { "type": "document", "source": { "type": "base64", "media_type": "text/plain", "data": encode_base64(b"plain words") } },
            { "type": "document", "title": "inline", "source": { "type": "text", "media_type": "text/plain", "data": "inline words" } },
            { "type": "image", "source": { "type": "url", "url": "https://example.com/cat.png" } },
            { "type": "text", "text": "What is this?" }
        ]}, { "role": "assistant", "content": [{ "type": "thinking", "thinking": "…" }, { "type": "text", "text": "A cat." }] },
            { "role": "user", "content": "Sure?" }] });
        let fetch = |url: &str| -> Result<(String, Vec<u8>), ApiError> {
            assert_eq!(url, "https://example.com/cat.png");
            Ok(("image/png".into(), decode_base64(PNG).unwrap()))
        };
        let req = parse_anthropic_with(&v, &fetch).unwrap();
        let kinds: Vec<_> = req.attachments.iter().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            vec![
                AttachmentKind::Image,
                AttachmentKind::Pdf,
                AttachmentKind::Image
            ]
        );
        let text = &req.turns[0].1;
        assert!(text.starts_with(
            "[attachment 1: image]\n[attachment 2: PDF document \"spec\"]\n<document>\nplain words\n</document>\n<document name=\"inline\">\ninline words\n</document>\n[attachment 3: image]\nWhat is this?"
        ), "{text}");
        assert_eq!(req.turns[1].1, "A cat.");
    }

    #[test]
    fn unsupported_attachments_are_rejected() {
        let user = |part: Value| json!({ "model": "claude", "messages": [{ "role": "user", "content": [part] }] });
        let cases = [
            (
                json!({ "type": "image_url", "image_url": { "url": format!("data:image/bmp;base64,{}", encode_base64(b"BM....")) } }),
                "Unsupported image type",
            ),
            (
                json!({ "type": "image_url", "image_url": { "url": "file:///C:/x.png" } }),
                "data: URLs or http(s)",
            ),
            (
                json!({ "type": "image_url", "image_url": { "url": "data:image/png,rawbytes" } }),
                "base64",
            ),
            (
                json!({ "type": "image_url", "image_url": { "url": "data:image/png;base64,@@@" } }),
                "not valid base64",
            ),
            (
                json!({ "type": "file", "file": { "file_data": format!("data:application/zip;base64,{}", encode_base64(b"PK\x03\x04")), "filename": "a.zip" } }),
                "Unsupported document type \"application/zip\"",
            ),
            (
                json!({ "type": "file", "file": { "file_data": format!("data:application/pdf;base64,{}", encode_base64(b"nope")) } }),
                "not a valid PDF",
            ),
            (
                json!({ "type": "file", "file": { "file_id": "file-1" } }),
                "file ids",
            ),
        ];
        for (part, needle) in cases {
            let e = parse_openai_with(&user(part), &no_fetch).unwrap_err();
            assert_eq!(e.status, 400);
            assert!(e.message.contains(needle), "{} / {needle}", e.message);
        }
        let fetch_fail =
            |_: &str| -> Result<(String, Vec<u8>), ApiError> { Err(invalid("Could not download")) };
        let remote =
            user(json!({ "type": "image_url", "image_url": { "url": "https://x/y.png" } }));
        assert!(parse_openai_with(&remote, &fetch_fail)
            .unwrap_err()
            .message
            .contains("Could not download"));
        let system_image = json!({ "model": "claude", "messages": [
            { "role": "system", "content": [{ "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{PNG}") } }] },
            { "role": "user", "content": "x" }] });
        assert!(parse_openai_with(&system_image, &no_fetch)
            .unwrap_err()
            .message
            .contains("system prompt"));
        let pdf_as_image = json!({ "model": "claude", "max_tokens": 5, "messages": [{ "role": "user", "content": [
            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": encode_base64(PDF) } }] }] });
        assert!(parse_anthropic_with(&pdf_as_image, &no_fetch)
            .unwrap_err()
            .message
            .contains("Unsupported image type"));
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

    #[test]
    fn incremental_stream_events() {
        let stream = |anthropic| SseStream {
            anthropic,
            id: "x1".into(),
            created: 7,
            model: "claude".into(),
            prompt: "prompt".into(),
        };
        let o = stream(false);
        let body = [o.start(), o.delta("Hel"), o.delta("lo"), o.end("Hello")].concat();
        let data: Vec<&str> = body
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .collect();
        assert_eq!(data.len(), 5);
        assert_eq!(data[4], "[DONE]");
        let chunks: Vec<Value> = data[..4]
            .iter()
            .map(|d| serde_json::from_str(d).unwrap())
            .collect();
        assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(chunks[1]["choices"][0]["delta"]["content"], "Hel");
        assert_eq!(chunks[2]["choices"][0]["delta"]["content"], "lo");
        assert_eq!(chunks[3]["choices"][0]["finish_reason"], "stop");
        assert!(chunks
            .iter()
            .all(|c| c["object"] == "chat.completion.chunk" && c["id"] == "x1"));
        let err = o.error(&ApiError::new(504, "timeout_error", "late"));
        assert!(err.starts_with("data: {\"error\"") && err.ends_with("data: [DONE]\n\n"));

        let a = stream(true);
        let body = [a.start(), a.delta("Hel"), a.delta("lo"), a.end("Hello")].concat();
        let events: Vec<&str> = body
            .lines()
            .filter_map(|l| l.strip_prefix("event: "))
            .collect();
        assert_eq!(
            events,
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop"
            ]
        );
        for l in body.lines().filter_map(|l| l.strip_prefix("data: ")) {
            serde_json::from_str::<Value>(l).unwrap();
        }
        assert!(a
            .error(&ApiError::new(502, "api_error", "boom"))
            .starts_with("event: error\ndata: {"));
    }
}
