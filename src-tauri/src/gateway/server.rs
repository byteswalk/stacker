//! The HTTP server: on 127.0.0.1, or on every interface when the user opens it to the
//! local network.
use super::protocol::{self, ApiError, ChatRequest, SseStream};
use crate::runner::{CancelFlag, DeltaSink, RunOutput, RunRequest, DEFAULT_TIMEOUT};
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Instant;
use tiny_http::{HTTPVersion, Header, Method, Request, Response, Server};

pub const RUNNING: usize = 2;
pub const WAITING: usize = 8;
/// Room for base64 images and PDFs; each attachment is capped separately.
const MAX_BODY: u64 = 64 * 1024 * 1024;
const RECENT: usize = 50;

pub type Runner = Arc<dyn Fn(&RunRequest, &CancelFlag) -> Result<RunOutput, String> + Send + Sync>;
/// Fills in the model and effort a bare `codex` / `claude` request uses.
pub type Defaults = Arc<dyn Fn(&ChatRequest) -> (Option<String>, Option<String>) + Send + Sync>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub at: u64,
    pub endpoint: String,
    pub model: String,
    pub status: u16,
    pub elapsed_ms: u64,
}

pub struct Shared {
    pub token: String,
    /// Agents the user allows through the gateway; changed live from the page.
    pub enabled: Mutex<std::collections::HashSet<String>>,
    pub runner: Runner,
    pub defaults: Defaults,
    pub recent: Mutex<VecDeque<LogEntry>>,
    in_flight: AtomicUsize,
    slots: (Mutex<usize>, Condvar),
}

impl Shared {
    pub fn new(token: String, runner: Runner, defaults: Defaults) -> Arc<Self> {
        Arc::new(Self {
            token,
            enabled: Mutex::new(
                crate::runner::backends::all()
                    .iter()
                    .map(|b| b.id.to_string())
                    .collect(),
            ),
            runner,
            defaults,
            recent: Mutex::new(VecDeque::new()),
            in_flight: AtomicUsize::new(0),
            slots: (Mutex::new(0), Condvar::new()),
        })
    }

    fn acquire(&self) {
        let (lock, cv) = &self.slots;
        let mut running = lock.lock().unwrap_or_else(|e| e.into_inner());
        while *running >= RUNNING {
            running = cv.wait(running).unwrap_or_else(|e| e.into_inner());
        }
        *running += 1;
    }

    fn release(&self) {
        let (lock, cv) = &self.slots;
        let mut running = lock.lock().unwrap_or_else(|e| e.into_inner());
        *running = running.saturating_sub(1);
        cv.notify_one();
    }

    fn log(&self, entry: LogEntry) {
        // Kept on disk: the page searches it, removes rows and forgets old ones on a
        // schedule, none of which survives a ring buffer in memory.
        super::requests::record(
            &entry.endpoint,
            &entry.model,
            entry.status,
            entry.elapsed_ms,
        );
        if let Ok(mut recent) = self.recent.lock() {
            recent.push_front(entry);
            recent.truncate(RECENT);
        }
    }
}

pub struct Running {
    pub server: Arc<Server>,
    pub port: u16,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Running {
    pub fn stop(mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Binds 127.0.0.1, or 0.0.0.0 when the user opened the service to the local network.
/// Port 0 picks a free port (tests).
pub fn start(port: u16, lan: bool, shared: Arc<Shared>) -> Result<Running, String> {
    let host = if lan { "0.0.0.0" } else { "127.0.0.1" };
    let server = Server::http((host, port)).map_err(|_| "E_PORT".to_string())?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(port);
    let server = Arc::new(server);
    let accept = server.clone();
    let thread = std::thread::spawn(move || {
        for request in accept.incoming_requests() {
            let shared = shared.clone();
            std::thread::spawn(move || handle(request, &shared));
        }
    });
    Ok(Running {
        server,
        port,
        thread: Some(thread),
    })
}

fn header(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

fn authorized(req: &Request, token: &str) -> bool {
    let bearer = header(req, "authorization")
        .and_then(|v| v.strip_prefix("Bearer ").map(|t| t.trim().to_string()));
    let key = header(req, "x-api-key").map(|v| v.trim().to_string());
    bearer.as_deref() == Some(token) || key.as_deref() == Some(token)
}

fn json_response(status: u16, body: &Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.to_string().into_bytes())
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
}

fn sse_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.into_bytes())
        .with_header(Header::from_bytes("Content-Type", "text/event-stream").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap())
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    OpenAi,
    Anthropic,
}

fn error(style: Style, e: &ApiError) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = match style {
        Style::OpenAi => e.openai_body(),
        Style::Anthropic => e.anthropic_body(),
    };
    json_response(e.status, &body)
}

fn runner_error(code: &str, backend: &str) -> ApiError {
    match code {
        "E_ATTACHMENT_UNSUPPORTED" if backend == "codex" => ApiError::new(
            400,
            "invalid_request_error",
            "Codex cannot read PDF documents. Send the PDF to a claude/* model, e.g. claude/sonnet.",
        ),
        "E_ATTACHMENT_UNSUPPORTED" => ApiError::new(
            400,
            "invalid_request_error",
            format!(
                "{backend} cannot read images or documents through Stacker. Images work with claude/* and codex/* models; PDF documents with claude/* models."
            ),
        ),
        "E_RUNNER_MISSING" => ApiError::new(
            503,
            "api_error",
            "The agent CLI is not installed on this computer.",
        ),
        "E_RUNNER_AUTH" => ApiError::new(
            401,
            "authentication_error",
            "The agent CLI is not signed in. Run it in a terminal and sign in.",
        ),
        "E_RUNNER_TIMEOUT" => ApiError::new(
            504,
            "timeout_error",
            "The agent did not answer within 5 minutes.",
        ),
        _ => ApiError::new(502, "api_error", format!("The agent run failed ({code}).")),
    }
}

/// Each enabled backend's bare id followed by its `<backend>/<model>` ids.
fn known_models(enabled: &std::collections::HashSet<String>) -> Vec<String> {
    crate::runner::backends::all()
        .iter()
        .filter(|b| enabled.contains(b.id))
        .flat_map(|b| {
            let mut ids = vec![b.id.to_string()];
            ids.extend(
                (b.models)()
                    .into_iter()
                    .map(|m| format!("{}/{}", b.id, m.id)),
            );
            ids
        })
        .collect()
}

fn now() -> u64 {
    crate::sessions::now()
}

fn id(prefix: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    format!("{prefix}{:016x}", h.finish())
}

fn handle(mut req: Request, shared: &Shared) {
    let started = Instant::now();
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    let style = if path == "/v1/messages" {
        Style::Anthropic
    } else {
        Style::OpenAi
    };
    let log = |status: u16, model: &str| {
        shared.log(LogEntry {
            at: now(),
            endpoint: path.clone(),
            model: model.to_string(),
            status,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    };
    let respond = |req: Request, resp: Response<std::io::Cursor<Vec<u8>>>, model: &str| {
        let status = resp.status_code().0;
        let _ = req.respond(resp);
        log(status, model);
    };

    if method == Method::Get && path == "/health" {
        return respond(req, Response::from_string("ok"), "");
    }
    // Browsers send Origin; web pages must not reach the gateway through localhost.
    if header(&req, "origin").is_some() {
        return respond(
            req,
            error(
                style,
                &ApiError::new(
                    403,
                    "permission_error",
                    "Browser requests are not accepted.",
                ),
            ),
            "",
        );
    }
    if !authorized(&req, &shared.token) {
        return respond(
            req,
            error(
                style,
                &ApiError::new(401, "authentication_error", "Invalid or missing API key."),
            ),
            "",
        );
    }
    match (method, path.as_str()) {
        (Method::Get, "/v1/models") => {
            let enabled = shared.enabled.lock().map(|e| e.clone()).unwrap_or_default();
            let body = protocol::models_list(now(), &known_models(&enabled));
            respond(req, json_response(200, &body), "")
        }
        (Method::Post, "/v1/chat/completions") | (Method::Post, "/v1/messages") => {
            let mut body = String::new();
            if req
                .as_reader()
                .take(MAX_BODY)
                .read_to_string(&mut body)
                .is_err()
            {
                return respond(
                    req,
                    error(
                        style,
                        &ApiError::new(400, "invalid_request_error", "Body must be UTF-8 JSON."),
                    ),
                    "",
                );
            }
            let parsed = serde_json::from_str::<Value>(&body)
                .map_err(|_| ApiError::new(400, "invalid_request_error", "Body must be JSON."))
                .and_then(|v| match style {
                    Style::OpenAi => protocol::parse_openai(&v),
                    Style::Anthropic => protocol::parse_anthropic(&v),
                });
            let chat = match parsed {
                Ok(c) => c,
                Err(e) => return respond(req, error(style, &e), ""),
            };
            let model = chat.model_name.clone();
            let allowed = shared
                .enabled
                .lock()
                .map(|e| e.contains(&chat.model.backend))
                .unwrap_or(false);
            if !allowed {
                return respond(
                    req,
                    error(
                        style,
                        &ApiError::new(
                            403,
                            "permission_error",
                            format!(
                                "{} is turned off in Stacker's API service.",
                                chat.model.backend
                            ),
                        ),
                    ),
                    &model,
                );
            }
            if let Some(e) = unsupported_effort(&chat, style) {
                return respond(req, error(style, &e), &model);
            }
            if shared.in_flight.fetch_add(1, Ordering::SeqCst) >= RUNNING + WAITING {
                shared.in_flight.fetch_sub(1, Ordering::SeqCst);
                return respond(
                    req,
                    error(
                        style,
                        &ApiError::new(
                            429,
                            "rate_limit_error",
                            "Too many requests are waiting; try again shortly.",
                        ),
                    ),
                    &model,
                );
            }
            // HTTP/1.0 has no chunked encoding; such clients get the answer in one piece.
            if chat.stream && *req.http_version() != HTTPVersion(1, 0) {
                return stream_chat(req, style, &chat, shared, |status| log(status, &model));
            }
            shared.acquire();
            let result = run_chat(&chat, shared, None, &CancelFlag::default());
            shared.release();
            shared.in_flight.fetch_sub(1, Ordering::SeqCst);
            let resp = match result {
                Ok((text, prompt)) => success(style, &chat, &text, &prompt),
                Err(code) => error(style, &runner_error(&code, &chat.model.backend)),
            };
            respond(req, resp, &model)
        }
        _ => respond(
            req,
            error(
                style,
                &ApiError::new(404, "not_found_error", "Unknown endpoint."),
            ),
            "",
        ),
    }
}

/// A level the API allows that this model does not take, refused the way the APIs refuse it.
fn unsupported_effort(chat: &ChatRequest, style: Style) -> Option<ApiError> {
    let effort = chat.effort.as_deref()?;
    let supported =
        super::agents::supported_efforts(&chat.model.backend, chat.model.model.as_deref())?;
    if supported.iter().any(|level| level == effort) {
        return None;
    }
    let field = match style {
        Style::OpenAi => "reasoning_effort",
        Style::Anthropic => "output_config.effort",
    };
    Some(ApiError::new(
        400,
        "invalid_request_error",
        format!(
            "Unsupported value: '{field}' does not support '{effort}' with model '{}'. Supported values are: {}.",
            chat.model_name,
            supported
                .iter()
                .map(|level| format!("'{level}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ))
}

fn run_chat(
    chat: &ChatRequest,
    shared: &Shared,
    on_delta: Option<DeltaSink>,
    cancel: &CancelFlag,
) -> Result<(String, String), String> {
    let (model, effort) = (shared.defaults)(chat);
    let prompt = protocol::render_prompt(chat);
    let req = RunRequest {
        backend: chat.model.backend.clone(),
        model,
        effort,
        prompt: prompt.clone(),
        timeout: DEFAULT_TIMEOUT,
        attachments: chat.attachments.clone(),
        on_delta,
    };
    (shared.runner)(&req, cancel).map(|o| (o.text, prompt))
}

enum Event {
    Delta(String),
    /// The answer and the prompt, or a runner error code.
    Done(Result<(String, String), String>),
}

/// Runs a `stream: true` chat on a worker and forwards its text as it arrives. Until the first
/// piece arrives nothing is sent, so an agent that cannot stream (or fails first) is answered
/// exactly like before: one SSE burst, or a JSON error with its status.
fn stream_chat(req: Request, style: Style, chat: &ChatRequest, shared: &Shared, log: impl Fn(u16)) {
    let cancel = CancelFlag::default();
    let (tx, rx) = mpsc::channel::<Event>();
    std::thread::scope(|scope| {
        let deltas = tx.clone();
        let cancel = &cancel;
        scope.spawn(move || {
            let sink: DeltaSink = Arc::new(move |text: &str| {
                let _ = deltas.send(Event::Delta(text.to_string()));
            });
            shared.acquire();
            let result = run_chat(chat, shared, Some(sink), cancel);
            shared.release();
            shared.in_flight.fetch_sub(1, Ordering::SeqCst);
            let _ = tx.send(Event::Done(result));
        });
        let status = match rx.recv() {
            Ok(Event::Delta(first)) => {
                let format = SseStream {
                    anthropic: style == Style::Anthropic,
                    id: match style {
                        Style::OpenAi => id("chatcmpl-"),
                        Style::Anthropic => id("msg_"),
                    },
                    created: now(),
                    model: chat.model_name.clone(),
                    prompt: protocol::render_prompt(chat),
                };
                if write_stream(req.into_writer(), &format, first, &rx, &chat.model.backend)
                    .is_err()
                {
                    // The client went away: stop the agent instead of letting it finish.
                    cancel.cancel();
                }
                200
            }
            Ok(Event::Done(result)) => {
                let resp = match result {
                    Ok((text, prompt)) => success(style, chat, &text, &prompt),
                    Err(code) => error(style, &runner_error(&code, &chat.model.backend)),
                };
                let status = resp.status_code().0;
                let _ = req.respond(resp);
                status
            }
            Err(_) => {
                let e = runner_error("E_RUNNER_FAILED", &chat.model.backend);
                let _ = req.respond(error(style, &e));
                e.status
            }
        };
        log(status);
    });
}

/// One HTTP chunk per event, flushed at once so each piece reaches the client as it comes.
fn write_chunk(out: &mut dyn Write, text: &str) -> std::io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    write!(out, "{:x}\r\n{text}\r\n", text.len())?;
    out.flush()
}

fn write_stream(
    mut out: Box<dyn Write + Send>,
    format: &SseStream,
    first: String,
    events: &mpsc::Receiver<Event>,
    backend: &str,
) -> std::io::Result<()> {
    out.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    )?;
    write_chunk(&mut out, &(format.start() + &format.delta(&first)))?;
    let mut sent = first;
    for event in events.iter() {
        match event {
            Event::Delta(text) => {
                write_chunk(&mut out, &format.delta(&text))?;
                sent.push_str(&text);
            }
            Event::Done(Ok((text, _))) => {
                // The final answer may hold text the deltas did not.
                if let Some(rest) = text.strip_prefix(sent.as_str()).filter(|r| !r.is_empty()) {
                    write_chunk(&mut out, &format.delta(rest))?;
                }
                write_chunk(&mut out, &format.end(&text))?;
                break;
            }
            Event::Done(Err(code)) => {
                write_chunk(&mut out, &format.error(&runner_error(&code, backend)))?;
                break;
            }
        }
    }
    out.write_all(b"0\r\n\r\n")?;
    out.flush()
}

fn success(
    style: Style,
    chat: &ChatRequest,
    text: &str,
    prompt: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    match (style, chat.stream) {
        (Style::OpenAi, false) => json_response(
            200,
            &protocol::openai_response(&id("chatcmpl-"), now(), &chat.model_name, text, prompt),
        ),
        (Style::OpenAi, true) => sse_response(protocol::openai_sse(
            &id("chatcmpl-"),
            now(),
            &chat.model_name,
            text,
        )),
        (Style::Anthropic, false) => json_response(
            200,
            &protocol::anthropic_response(&id("msg_"), &chat.model_name, text, prompt),
        ),
        (Style::Anthropic, true) => sse_response(protocol::anthropic_sse(
            &id("msg_"),
            &chat.model_name,
            text,
            prompt,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::AttachmentKind;
    use std::net::TcpStream;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    /// Set when the `forever` fake run sees its cancel flag.
    static CANCELLED: AtomicBool = AtomicBool::new(false);

    /// Echoes the backend and effort; prompts naming a scenario stream or fail on purpose.
    fn fake_run(req: &RunRequest, cancel: &CancelFlag) -> Result<RunOutput, String> {
        let say = |text: &str| {
            if let Some(sink) = &req.on_delta {
                sink(text);
            }
        };
        let pause = || std::thread::sleep(Duration::from_millis(250));
        if req.backend == "codex"
            && req
                .attachments
                .iter()
                .any(|a| a.kind == AttachmentKind::Pdf)
        {
            return Err("E_ATTACHMENT_UNSUPPORTED".into());
        }
        if req.prompt.contains("stream-me") {
            for piece in ["one ", "two ", "three"] {
                say(piece);
                pause();
            }
            return Ok(RunOutput {
                text: "one two three!".into(),
            });
        }
        if req.prompt.contains("fail-late") {
            say("partial");
            return Err("E_RUNNER_TIMEOUT".into());
        }
        if req.prompt.contains("fail-early") {
            return Err("E_RUNNER_FAILED".into());
        }
        if req.prompt.contains("forever") {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(20) {
                if cancel.is_cancelled() {
                    CANCELLED.store(true, Ordering::SeqCst);
                    return Err("E_CANCELLED".into());
                }
                say("tick ");
                std::thread::sleep(Duration::from_millis(30));
            }
            return Err("E_RUNNER_TIMEOUT".into());
        }
        Ok(RunOutput {
            text: format!(
                "echo:{}:{}:{}",
                req.backend,
                req.effort.clone().unwrap_or_default(),
                req.attachments.len()
            ),
        })
    }

    fn fake() -> Arc<Shared> {
        let runner: Runner = Arc::new(fake_run);
        let defaults: Defaults = Arc::new(|c: &ChatRequest| {
            (
                c.model.model.clone(),
                c.effort.clone().or(Some("low".into())),
            )
        });
        Shared::new("sk-test".into(), runner, defaults)
    }

    fn http(port: u16, raw: &str) -> (u16, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let status = out.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = out
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or_default();
        (status, body)
    }

    fn post(port: u16, path: &str, auth: &str, extra: &str, body: &str) -> (u16, String) {
        http(port, &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\n{auth}{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ))
    }

    #[test]
    fn each_api_sets_reasoning_the_way_its_specification_does() {
        let server = start(0, false, fake()).unwrap();
        let port = server.port;
        let openai = |body: &str| {
            post(
                port,
                "/v1/chat/completions",
                "Authorization: Bearer sk-test\r\n",
                "",
                body,
            )
        };
        let anthropic = |body: &str| post(port, "/v1/messages", "x-api-key: sk-test\r\n", "", body);

        // OpenAI: reasoning_effort.
        let (status, body) = openai(
            r#"{"model":"claude","reasoning_effort":"high","messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("echo:claude:high:0"), "{body}");
        // Anthropic: output_config.effort; OpenAI's field means nothing there.
        let (status, body) = anthropic(
            r#"{"model":"claude","max_tokens":5,"output_config":{"effort":"max"},"messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("echo:claude:max:0"), "{body}");
        let (_, body) = anthropic(
            r#"{"model":"claude","max_tokens":5,"reasoning_effort":"high","messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert!(
            body.contains("echo:claude:low:0"),
            "the fake's own default, not high: {body}"
        );

        // A value outside the API's own set, and one the model does not take, are both 400.
        let (status, body) = anthropic(
            r#"{"model":"claude","max_tokens":5,"output_config":{"effort":"none"},"messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 400, "{body}");
        assert!(body.contains("output_config.effort"), "{body}");
        let (status, body) = openai(
            r#"{"model":"claude","reasoning_effort":"minimal","messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 400, "{body}");
        assert!(body.contains("Supported values are"), "{body}");
    }

    #[test]
    fn serves_both_styles_with_auth() {
        let server = start(0, false, fake()).unwrap();
        let port = server.port;
        let chat = r#"{"model":"claude","messages":[{"role":"user","content":"hi"}]}"#;

        let (status, body) = post(
            port,
            "/v1/chat/completions",
            "Authorization: Bearer sk-test\r\n",
            "",
            chat,
        );
        assert_eq!(status, 200, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["choices"][0]["message"]["content"], "echo:claude:low:0");

        let (status, body) = post(
            port,
            "/v1/messages",
            "x-api-key: sk-test\r\n",
            "",
            r#"{"model":"codex","max_tokens":5,"messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 200);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["content"][0]["text"], "echo:codex:low:0");

        let (status, body) = post(
            port,
            "/v1/chat/completions",
            "Authorization: Bearer sk-test\r\n",
            "",
            r#"{"model":"claude","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 200);
        assert!(body.contains("data: [DONE]"));

        let (status, _) = post(
            port,
            "/v1/chat/completions",
            "Authorization: Bearer wrong\r\n",
            "",
            chat,
        );
        assert_eq!(status, 401);
        let (status, _) = post(
            port,
            "/v1/chat/completions",
            "Authorization: Bearer sk-test\r\n",
            "Origin: https://evil.example\r\n",
            chat,
        );
        assert_eq!(status, 403, "browser pages are rejected even with the key");
        let (status, body) = post(
            port,
            "/v1/messages",
            "x-api-key: sk-test\r\n",
            "",
            r#"{"model":"claude","tools":[{"name":"x"}],"messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 400);
        assert!(body.contains("\"type\":\"error\""), "anthropic error shape");

        let (status, _) = http(
            port,
            "GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(status, 200);
        let (status, body) = http(port, "GET /v1/models HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer sk-test\r\nConnection: close\r\n\r\n");
        assert_eq!(status, 200);
        assert!(body.contains("\"claude\""));
        server.stop();
    }

    /// Sends a request and reads the raw response as it arrives: status, headers, and each
    /// decoded chunk with the time it was complete.
    fn stream_post(
        port: u16,
        path: &str,
        auth: &str,
        body: &str,
    ) -> (u16, String, Vec<(Duration, String)>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ).as_bytes()).unwrap();
        let started = Instant::now();
        let mut raw = Vec::new();
        let mut chunks = Vec::new();
        let mut buf = [0u8; 4096];
        let mut head_len = None;
        let mut pos = 0;
        loop {
            let n = s.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            if head_len.is_none() {
                head_len = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
                pos = head_len.unwrap_or(0);
            }
            // Decode every complete chunk received so far.
            while head_len.is_some() {
                let Some(eol) = raw[pos..].windows(2).position(|w| w == b"\r\n") else {
                    break;
                };
                let size =
                    usize::from_str_radix(std::str::from_utf8(&raw[pos..pos + eol]).unwrap(), 16)
                        .unwrap_or(0);
                if size == 0 || raw.len() < pos + eol + 2 + size + 2 {
                    break;
                }
                let data = &raw[pos + eol + 2..pos + eol + 2 + size];
                chunks.push((
                    started.elapsed(),
                    String::from_utf8_lossy(data).into_owned(),
                ));
                pos += eol + 2 + size + 2;
            }
        }
        let text = String::from_utf8_lossy(&raw).into_owned();
        let status = text.split_whitespace().nth(1).unwrap().parse().unwrap();
        let head = text.split("\r\n\r\n").next().unwrap_or("").to_string();
        (status, head, chunks)
    }

    #[test]
    fn streams_deltas_as_they_arrive() {
        let server = start(0, false, fake()).unwrap();
        let port = server.port;
        let (status, head, chunks) = stream_post(
            port,
            "/v1/chat/completions",
            "Authorization: Bearer sk-test\r\n",
            r#"{"model":"claude","stream":true,"messages":[{"role":"user","content":"stream-me"}]}"#,
        );
        assert_eq!(status, 200);
        assert!(head.contains("text/event-stream") && head.contains("chunked"));
        assert!(chunks.len() >= 4, "{chunks:?}");
        let (first_at, first) = &chunks[0];
        assert!(first.contains("\"role\":\"assistant\"") && first.contains("\"content\":\"one \""));
        let (last_at, last) = chunks.last().unwrap();
        assert!(
            *last_at > *first_at + Duration::from_millis(400),
            "the first piece arrived before the run ended"
        );
        let body: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        let contents: Vec<String> = body
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .filter(|d| *d != "[DONE]")
            .filter_map(|d| serde_json::from_str::<Value>(d).ok())
            .filter_map(|v| {
                v["choices"][0]["delta"]["content"]
                    .as_str()
                    .map(str::to_string)
            })
            .collect();
        assert_eq!(contents, vec!["one ", "two ", "three", "!"]);
        assert!(last.ends_with("data: [DONE]\n\n"));
        assert!(body.contains("\"finish_reason\":\"stop\""));

        let (status, _, chunks) = stream_post(
            port,
            "/v1/messages",
            "x-api-key: sk-test\r\n",
            r#"{"model":"claude","max_tokens":9,"stream":true,"messages":[{"role":"user","content":"stream-me"}]}"#,
        );
        assert_eq!(status, 200);
        let body: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        let events: Vec<&str> = body
            .lines()
            .filter_map(|l| l.strip_prefix("event: "))
            .collect();
        assert_eq!(events.first(), Some(&"message_start"));
        assert_eq!(
            events
                .iter()
                .filter(|e| **e == "content_block_delta")
                .count(),
            4
        );
        assert_eq!(events.last(), Some(&"message_stop"));
        server.stop();
    }

    #[test]
    fn stream_failures_and_non_streaming_agents() {
        let server = start(0, false, fake()).unwrap();
        let port = server.port;
        let auth = "Authorization: Bearer sk-test\r\n";
        // Failure before any text: a plain JSON error with the real status.
        let (status, body) = post(
            port,
            "/v1/chat/completions",
            auth,
            "",
            r#"{"model":"claude","stream":true,"messages":[{"role":"user","content":"fail-early"}]}"#,
        );
        assert_eq!(status, 502);
        assert!(body.contains("\"error\""));
        // Failure after text started: an error event, then the stream ends.
        let (status, _, chunks) = stream_post(
            port,
            "/v1/chat/completions",
            auth,
            r#"{"model":"claude","stream":true,"messages":[{"role":"user","content":"fail-late"}]}"#,
        );
        assert_eq!(status, 200);
        let body: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        assert!(body.contains("\"content\":\"partial\""));
        assert!(body.contains("data: {\"error\"") && body.contains("timeout_error"));
        assert!(body.ends_with("data: [DONE]\n\n"));
        let (_, _, chunks) = stream_post(
            port,
            "/v1/messages",
            "x-api-key: sk-test\r\n",
            r#"{"model":"claude","max_tokens":5,"stream":true,"messages":[{"role":"user","content":"fail-late"}]}"#,
        );
        let body: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        assert!(body.contains("event: error\n") && !body.contains("message_stop"));
        // An agent that never streams still gets one well-formed burst.
        let (status, body) = post(
            port,
            "/v1/messages",
            "x-api-key: sk-test\r\n",
            "",
            r#"{"model":"codex","max_tokens":5,"stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 200);
        assert!(body.contains("echo:codex:low:0") && body.contains("message_stop"));
        server.stop();
    }

    #[test]
    fn attachments_reach_the_runner_and_unsupported_ones_are_400() {
        let server = start(0, false, fake()).unwrap();
        let port = server.port;
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let body = format!(
            r#"{{"model":"codex","messages":[{{"role":"user","content":[{{"type":"image_url","image_url":{{"url":"data:image/png;base64,{png}"}}}},{{"type":"text","text":"what?"}}]}}]}}"#
        );
        let (status, body) = post(
            port,
            "/v1/chat/completions",
            "Authorization: Bearer sk-test\r\n",
            "",
            &body,
        );
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("echo:codex:low:1"));
        let pdf = crate::runner::attachment::encode_base64(b"%PDF-1.4\n");
        let body = format!(
            r#"{{"model":"codex","max_tokens":5,"messages":[{{"role":"user","content":[{{"type":"document","source":{{"type":"base64","media_type":"application/pdf","data":"{pdf}"}}}}]}}]}}"#
        );
        let (status, body) = post(port, "/v1/messages", "x-api-key: sk-test\r\n", "", &body);
        assert_eq!(status, 400);
        assert!(body.contains("Codex cannot read PDF"), "{body}");
        server.stop();
    }

    #[test]
    fn a_client_that_leaves_mid_stream_cancels_the_run() {
        let server = start(0, false, fake()).unwrap();
        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        let body =
            r#"{"model":"claude","stream":true,"messages":[{"role":"user","content":"forever"}]}"#;
        s.write_all(format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer sk-test\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ).as_bytes()).unwrap();
        let mut buf = [0u8; 256];
        assert!(s.read(&mut buf).unwrap() > 0);
        drop(s);
        let started = Instant::now();
        while !CANCELLED.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(CANCELLED.load(Ordering::SeqCst), "the run was cancelled");
        server.stop();
    }

    /// Live: `cargo test --lib live_gateway_attachments -- --ignored --nocapture`. Sends a
    /// generated red/blue PNG to Claude and Codex, and streams a Claude answer.
    #[test]
    #[ignore]
    fn live_gateway_attachments() {
        let runner: Runner = Arc::new(crate::runner::run);
        let defaults: Defaults = Arc::new(|c: &ChatRequest| {
            (
                c.model.model.clone(),
                c.effort.clone().or(Some("low".into())),
            )
        });
        let server = start(0, false, Shared::new("sk-live".into(), runner, defaults)).unwrap();
        let png = crate::runner::attachment::encode_base64(&two_color_png());
        let question = "The image is split into two halves of solid color. Name the color of the left half and the right half, in English, in the form: left=<color>, right=<color>.";
        let mut answers = Vec::new();
        for model in ["claude/sonnet", "codex"] {
            let body = format!(
                r#"{{"model":"{model}","messages":[{{"role":"user","content":[{{"type":"image_url","image_url":{{"url":"data:image/png;base64,{png}"}}}},{{"type":"text","text":"{question}"}}]}}]}}"#
            );
            let started = Instant::now();
            let (status, body) = post(
                server.port,
                "/v1/chat/completions",
                "Authorization: Bearer sk-live\r\n",
                "",
                &body,
            );
            println!("{model} image {status} {:?}\n{body}", started.elapsed());
            assert_eq!(status, 200);
            let v: Value = serde_json::from_str(&body).unwrap();
            let text = v["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_lowercase();
            answers.push((model, text));
        }
        let started = Instant::now();
        let (status, _, chunks) = stream_post(
            server.port,
            "/v1/messages",
            "x-api-key: sk-live\r\n",
            r#"{"model":"claude/sonnet","max_tokens":400,"stream":true,"messages":[{"role":"user","content":"Count from 1 to 40 in words, one per line."}]}"#,
        );
        let deltas = chunks
            .iter()
            .filter(|(_, c)| c.contains("content_block_delta"))
            .count();
        println!(
            "claude stream {status} {:?}: {} chunks, {deltas} with deltas, first at {:?}, last at {:?}",
            started.elapsed(),
            chunks.len(),
            chunks.first().map(|c| c.0),
            chunks.last().map(|c| c.0)
        );
        let pdf = crate::runner::attachment::encode_base64(&word_pdf("PELICAN"));
        let mut pdf_status = Vec::new();
        for model in ["claude/sonnet", "codex"] {
            let body = format!(
                r#"{{"model":"{model}","max_tokens":50,"messages":[{{"role":"user","content":[{{"type":"document","source":{{"type":"base64","media_type":"application/pdf","data":"{pdf}"}}}},{{"type":"text","text":"Which single word is printed in the PDF? Reply with the word only."}}]}}]}}"#
            );
            let (status, body) = post(
                server.port,
                "/v1/messages",
                "x-api-key: sk-live\r\n",
                "",
                &body,
            );
            println!("{model} pdf {status}\n{body}");
            pdf_status.push((status, body));
        }
        server.stop();
        assert_eq!(pdf_status[0].0, 200);
        assert!(pdf_status[0].1.to_uppercase().contains("PELICAN"));
        assert_eq!(pdf_status[1].0, 400, "codex has no document input");
        for (model, text) in &answers {
            assert!(
                text.contains("left=red") && text.contains("right=blue"),
                "{model}: {text}"
            );
        }
        assert_eq!(status, 200);
        assert!(deltas > 1, "the answer arrived in more than one piece");
    }

    /// A one-page PDF showing `word` in large Helvetica.
    fn word_pdf(word: &str) -> Vec<u8> {
        let content = format!("BT /F1 48 Tf 72 700 Td ({word}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// A 128×64 PNG: red left half, blue right half (stored, uncompressed deflate).
    fn two_color_png() -> Vec<u8> {
        fn crc(data: &[u8]) -> u32 {
            let mut c = 0xffff_ffffu32;
            for b in data {
                c ^= *b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        0xedb8_8320 ^ (c >> 1)
                    } else {
                        c >> 1
                    };
                }
            }
            !c
        }
        fn adler(data: &[u8]) -> u32 {
            let (mut a, mut b) = (1u32, 0u32);
            for x in data {
                a = (a + *x as u32) % 65521;
                b = (b + a) % 65521;
            }
            b << 16 | a
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc(&body).to_be_bytes());
        }
        let (w, h) = (128u32, 64u32);
        // Each row: filter byte 0, then RGB pixels.
        let row: Vec<u8> = std::iter::once(0)
            .chain((0..w).flat_map(|x| if x < w / 2 { [255, 0, 0] } else { [0, 0, 255] }))
            .collect();
        let raw = row.repeat(h as usize);
        let mut z = vec![0x78, 0x01, 1];
        z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        z.extend_from_slice(&raw);
        z.extend_from_slice(&adler(&raw).to_be_bytes());
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &z);
        chunk(&mut png, b"IEND", &[]);
        png
    }

    #[test]
    fn generated_png_is_recognised() {
        let png = two_color_png();
        assert_eq!(
            crate::runner::attachment::sniff_image(&png),
            Some("image/png")
        );
        // One stored deflate block holds at most 65535 bytes.
        assert!(png.len() < 65_535);
    }

    /// Live, uses the signed-in CLIs: `cargo test --lib live_gateway -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_gateway() {
        let runner: Runner = Arc::new(crate::runner::run);
        let defaults: Defaults = Arc::new(|c: &ChatRequest| {
            let fallback = if c.model.backend == "claude" {
                Some("sonnet".to_string())
            } else {
                None
            };
            (
                c.model.model.clone().or(fallback),
                c.effort.clone().or(Some("low".into())),
            )
        });
        let server = start(0, false, Shared::new("sk-live".into(), runner, defaults)).unwrap();
        let started = Instant::now();
        let (status, body) = post(
            server.port,
            "/v1/chat/completions",
            "Authorization: Bearer sk-live\r\n",
            "",
            r#"{"model":"claude/sonnet","messages":[{"role":"system","content":"Answer with digits only."},{"role":"user","content":"2+3?"},{"role":"assistant","content":"5"},{"role":"user","content":"and times 4?"}]}"#,
        );
        println!("openai→claude {status} {:?}\n{body}", started.elapsed());
        let started = Instant::now();
        let (status2, body2) = post(
            server.port,
            "/v1/messages",
            "x-api-key: sk-live\r\n",
            "",
            r#"{"model":"codex","max_tokens":50,"stream":true,"messages":[{"role":"user","content":"Reply with one word: the capital of France."}]}"#,
        );
        println!("anthropic→codex {status2} {:?}\n{body2}", started.elapsed());
        server.stop();
        assert_eq!((status, status2), (200, 200));
    }

    #[test]
    fn binds_loopback_until_the_network_is_asked_for() {
        let server = start(0, false, fake()).unwrap();
        let addr = server.server.server_addr().to_ip().unwrap();
        assert!(addr.ip().is_loopback());
        server.stop();

        // Opened to the network, it listens on every interface, and this machine's own
        // address reaches it.
        let server = start(0, true, fake()).unwrap();
        let addr = server.server.server_addr().to_ip().unwrap();
        assert!(addr.ip().is_unspecified(), "{addr}");
        let port = server.port;
        for address in crate::gateway::lan_addresses() {
            if address.parse::<std::net::Ipv4Addr>().is_err() {
                continue;
            }
            let reached = std::net::TcpStream::connect((address.as_str(), port));
            assert!(reached.is_ok(), "{address}:{port} refused the connection");
        }
        server.stop();
    }
}
