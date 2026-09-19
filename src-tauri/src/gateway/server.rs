//! The 127.0.0.1-only HTTP server.
use super::protocol::{self, ApiError, ChatRequest};
use crate::runner::{CancelFlag, RunOutput, RunRequest, DEFAULT_TIMEOUT};
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;
use tiny_http::{Header, Method, Request, Response, Server};

pub const RUNNING: usize = 2;
pub const WAITING: usize = 8;
const MAX_BODY: u64 = 8 * 1024 * 1024;
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

/// Binds 127.0.0.1 only. Port 0 picks a free port (tests).
pub fn start(port: u16, shared: Arc<Shared>) -> Result<Running, String> {
    let server = Server::http(("127.0.0.1", port)).map_err(|_| "E_PORT".to_string())?;
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

fn runner_error(code: &str) -> ApiError {
    match code {
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

/// `codex/<slug>` from Codex's model cache and `claude/<alias>`.
fn known_models() -> Vec<String> {
    let codex_home = crate::sessions::roots::resolve(&Default::default()).codex;
    crate::runner::options::options(std::path::Path::new(&codex_home))
        .into_iter()
        .flat_map(|o| {
            let agent = o.agent.as_str();
            o.models
                .into_iter()
                .map(move |m| format!("{agent}/{}", m.id))
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
    let respond = |req: Request, resp: Response<std::io::Cursor<Vec<u8>>>, model: &str| {
        let status = resp.status_code().0;
        let _ = req.respond(resp);
        shared.log(LogEntry {
            at: now(),
            endpoint: path.clone(),
            model: model.to_string(),
            status,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
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
            let body = protocol::models_list(now(), &known_models());
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
            shared.acquire();
            let result = run_chat(&chat, shared);
            shared.release();
            shared.in_flight.fetch_sub(1, Ordering::SeqCst);
            let resp = match result {
                Ok((text, prompt)) => success(style, &chat, &text, &prompt),
                Err(code) => error(style, &runner_error(&code)),
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

fn run_chat(chat: &ChatRequest, shared: &Shared) -> Result<(String, String), String> {
    let (model, effort) = (shared.defaults)(chat);
    let prompt = protocol::render_prompt(chat);
    let req = RunRequest {
        agent: chat.model.agent,
        model,
        effort,
        prompt: prompt.clone(),
        timeout: DEFAULT_TIMEOUT,
    };
    (shared.runner)(&req, &CancelFlag::default()).map(|o| (o.text, prompt))
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
    use std::io::Write;
    use std::net::TcpStream;

    fn fake() -> Arc<Shared> {
        let runner: Runner = Arc::new(|req: &RunRequest, _: &CancelFlag| {
            Ok(RunOutput {
                text: format!(
                    "echo:{}:{}",
                    req.agent.as_str(),
                    req.effort.clone().unwrap_or_default()
                ),
            })
        });
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
    fn serves_both_styles_with_auth() {
        let server = start(0, fake()).unwrap();
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
        assert_eq!(v["choices"][0]["message"]["content"], "echo:claude:low");

        let (status, body) = post(
            port,
            "/v1/messages",
            "x-api-key: sk-test\r\n",
            "",
            r#"{"model":"codex","max_tokens":5,"messages":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(status, 200);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["content"][0]["text"], "echo:codex:low");

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

    /// Live, uses the signed-in CLIs: `cargo test --lib live_gateway -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_gateway() {
        let runner: Runner = Arc::new(crate::runner::run);
        let defaults: Defaults = Arc::new(|c: &ChatRequest| {
            let settings = crate::sessions::summary::SummarySettings::default();
            let base = crate::sessions::summary::choice_for(&settings, c.model.agent);
            (
                c.model.model.clone().or(base.model),
                c.effort.clone().or(base.effort),
            )
        });
        let server = start(0, Shared::new("sk-live".into(), runner, defaults)).unwrap();
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
    fn binds_loopback_only() {
        let server = start(0, fake()).unwrap();
        let addr = server.server.server_addr().to_ip().unwrap();
        assert!(addr.ip().is_loopback());
        server.stop();
    }
}
