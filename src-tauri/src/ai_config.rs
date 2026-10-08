//! Where Stacker's own AI features get a model from.
//!
//! One setting for the whole app: an agent already signed in on this machine (the same ones the
//! API service offers, run directly, so the service does not have to be switched on), or an
//! external API in the OpenAI or Anthropic shape. Features ask `complete` for text and never
//! care which it is.
//!
//! The file lives beside the settings rather than inside them: settings travel in the backup
//! and export, and an API key must not. The key itself is sealed with the user's Windows
//! credentials (DPAPI), so the file is useless copied to another account or machine.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// How long one answer may take before it is given up on.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AiConfig {
    /// `none`, `local` or `external`.
    pub kind: String,
    /// `local`: a name the API service offers, e.g. `claude/sonnet` or `codex`.
    pub local_model: String,
    /// `external`: `openai` or `anthropic`.
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    /// Hex of the DPAPI-sealed key; never sent to the page.
    pub api_key_sealed: String,
    /// `low`, `medium` or `high`; empty leaves it to the model. A local agent gets it as its
    /// `--effort`, OpenAI as `reasoning_effort`, Anthropic as an extended-thinking budget.
    #[serde(default)]
    pub effort: String,
}

/// What the page is shown: everything but the key, which it only learns exists.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiView {
    pub kind: String,
    pub local_model: String,
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
    pub effort: String,
}

/// What the page sends. An empty key keeps the one already saved.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiUpdate {
    pub kind: String,
    pub local_model: String,
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub clear_key: bool,
    pub effort: String,
}

fn file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Stacker")
        .join("ai.json")
}

pub fn load() -> AiConfig {
    std::fs::read(file())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(config: &AiConfig) -> Result<(), String> {
    let path = file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

pub fn view(config: &AiConfig) -> AiView {
    AiView {
        kind: if config.kind.is_empty() {
            "none".into()
        } else {
            config.kind.clone()
        },
        local_model: config.local_model.clone(),
        protocol: if config.protocol.is_empty() {
            "openai".into()
        } else {
            config.protocol.clone()
        },
        base_url: config.base_url.clone(),
        model: config.model.clone(),
        has_key: !config.api_key_sealed.is_empty(),
        effort: config.effort.clone(),
    }
}

pub fn apply(update: AiUpdate) -> Result<AiView, String> {
    let kind = match update.kind.as_str() {
        "none" | "local" | "external" => update.kind.clone(),
        _ => return Err("E_AI_KIND".into()),
    };
    let protocol = match update.protocol.as_str() {
        "" | "openai" => "openai".to_string(),
        "anthropic" => "anthropic".to_string(),
        _ => return Err("E_AI_PROTOCOL".into()),
    };
    // Each source takes its own set: an external API its specification's values, a local
    // agent the levels its CLI lists (checked again when a request is made).
    let allowed: &[&str] = match (kind.as_str(), protocol.as_str()) {
        ("external", "anthropic") => crate::gateway::protocol::ANTHROPIC_EFFORTS,
        ("external", _) => crate::gateway::protocol::OPENAI_EFFORTS,
        _ => &["low", "medium", "high", "xhigh", "max"],
    };
    let effort = update.effort.trim().to_string();
    if !effort.is_empty() && !allowed.contains(&effort.as_str()) {
        return Err("E_AI_EFFORT".into());
    }
    let mut config = load();
    config.effort = effort;
    config.kind = kind;
    config.local_model = update.local_model.trim().to_string();
    config.protocol = protocol;
    config.base_url = update.base_url.trim().trim_end_matches('/').to_string();
    config.model = update.model.trim().to_string();
    if update.clear_key {
        config.api_key_sealed.clear();
    } else if !update.api_key.trim().is_empty() {
        config.api_key_sealed = hex(&seal(update.api_key.trim().as_bytes())?);
    }
    save(&config)?;
    Ok(view(&config))
}

/// Makes a name from the API service, with the reasoning level picked beside it, Stacker's AI
/// in one step; no level means the agent decides.
pub fn use_local(model: &str, effort: Option<&str>) -> Result<AiView, String> {
    let mut config = load();
    config.kind = "local".into();
    config.local_model = model.trim().to_string();
    config.effort = effort.map(str::trim).unwrap_or_default().to_string();
    save(&config)?;
    Ok(view(&config))
}

/// Text for a prompt, from whichever model is configured.
pub fn complete(prompt: &str) -> Result<String, String> {
    complete_with(&load(), prompt)
}

/// The prompts are written in Chinese and ask for Chinese; with the interface in English the
/// answer is asked for in English instead. Not for replies a program reads (filters).
pub fn in_ui_language(prompt: &str, locale: &str) -> String {
    if locale == "en-US" {
        format!("{prompt}\n\nWrite the whole answer in English, whatever language is asked for above; keep commands, paths and names as they are.")
    } else {
        prompt.to_string()
    }
}

/// `complete`, for an answer a person reads: in the interface's language.
pub fn complete_answer(prompt: &str) -> Result<String, String> {
    complete(&in_ui_language(prompt, &crate::settings::load().locale))
}

/// The configured source as the runner describes one, for features that go through a job:
/// a local agent names its backend, an external API leaves the backend empty.
pub fn runner_choice() -> Result<crate::sessions::summary::RunnerChoice, String> {
    let config = load();
    match config.kind.as_str() {
        "local" => {
            let spec = crate::gateway::protocol::ModelSpec::parse(&config.local_model)
                .ok_or("E_AI_MODEL")?;
            Ok(crate::sessions::summary::RunnerChoice {
                effort: local_effort(&spec.backend, &config.effort),
                backend: spec.backend,
                model: spec.model,
            })
        }
        "external" => {
            if config.base_url.is_empty() || config.model.is_empty() {
                return Err("E_AI_FIELDS".into());
            }
            Ok(crate::sessions::summary::RunnerChoice {
                backend: String::new(),
                model: Some(config.model.clone()),
                effort: None,
            })
        }
        _ => Err("E_AI_NONE".into()),
    }
}

/// The production run function behind every job: a request naming a backend goes to that
/// local agent's CLI, one naming none goes to the external API.
pub fn run_request(
    req: &crate::runner::RunRequest,
    cancel: &crate::runner::CancelFlag,
) -> Result<crate::runner::RunOutput, String> {
    let started = std::time::Instant::now();
    if !req.backend.is_empty() {
        let result = crate::runner::run(req, cancel);
        let label = match &req.model {
            Some(model) => format!("{}/{model}", req.backend),
            None => req.backend.clone(),
        };
        let logged = result
            .as_ref()
            .map(|output| output.text.clone())
            .map_err(Clone::clone);
        crate::gateway::requests::record_internal(&label, &req.prompt, &logged, started);
        return result;
    }
    if cancel.is_cancelled() {
        return Err("E_CANCELLED".into());
    }
    let config = load();
    let result = complete_external(&config, &req.prompt, req.timeout);
    crate::gateway::requests::record_internal(
        &external_label(&config),
        &req.prompt,
        &result,
        started,
    );
    result.map(|text| crate::runner::RunOutput { text })
}

/// How an external model shows in the request log: `api/<model>`, so the filter groups them.
fn external_label(config: &AiConfig) -> String {
    format!("api/{}", config.model)
}

fn complete_with(config: &AiConfig, prompt: &str) -> Result<String, String> {
    let started = std::time::Instant::now();
    let (label, result) = match config.kind.as_str() {
        "local" => (
            config.local_model.clone(),
            complete_local(&config.local_model, &config.effort, prompt),
        ),
        "external" => (
            external_label(config),
            complete_external(config, prompt, ANSWER_TIMEOUT),
        ),
        _ => return Err("E_AI_NONE".into()),
    };
    // Stacker's own questions show in the request log beside the service's, marked as its own.
    crate::gateway::requests::record_internal(&label, prompt, &result, started);
    result
}

/// A level the agent's CLI lists, or none: a CLI given one it does not know refuses to run.
fn local_effort(backend: &str, effort: &str) -> Option<String> {
    if effort.is_empty() {
        return None;
    }
    let backend = crate::runner::backends::get(backend)?;
    (backend.efforts)()
        .iter()
        .any(|level| level == effort)
        .then(|| effort.to_string())
}

/// The request body for an external API, with the reasoning level where one is set.
fn external_body(protocol: &str, model: &str, effort: &str, prompt: &str) -> serde_json::Value {
    // Each API's own field: Anthropic's `output_config.effort` (a thinking token budget is
    // refused by current Claude models), OpenAI's `reasoning_effort`.
    let messages = serde_json::json!([{ "role": "user", "content": prompt }]);
    let mut body = if protocol == "anthropic" {
        serde_json::json!({ "model": model, "max_tokens": 16000, "messages": messages })
    } else {
        serde_json::json!({ "model": model, "messages": messages })
    };
    if !effort.is_empty() {
        if protocol == "anthropic" {
            body["output_config"] = serde_json::json!({ "effort": effort });
        } else {
            body["reasoning_effort"] = serde_json::json!(effort);
        }
    }
    body
}

/// The answer text; Anthropic puts thinking blocks ahead of it when thinking is on.
fn external_text(protocol: &str, body: &serde_json::Value) -> Option<String> {
    if protocol == "anthropic" {
        body["content"]
            .as_array()?
            .iter()
            .find(|block| block["type"] == "text")
            .and_then(|block| block["text"].as_str())
            .map(str::to_string)
    } else {
        body["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
    }
}

fn complete_local(name: &str, effort: &str, prompt: &str) -> Result<String, String> {
    let spec = crate::gateway::protocol::ModelSpec::parse(name).ok_or("E_AI_MODEL")?;
    let request = crate::runner::RunRequest {
        effort: local_effort(&spec.backend, effort),
        backend: spec.backend,
        model: spec.model,
        prompt: prompt.to_string(),
        timeout: ANSWER_TIMEOUT,
        attachments: Vec::new(),
        on_delta: None,
    };
    crate::runner::run(&request, &crate::runner::CancelFlag::default()).map(|output| output.text)
}

fn agent(timeout: Duration) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new().timeout(timeout);
    if let Some(proxy) =
        crate::agents::net::stacker_proxy().and_then(|url| ureq::Proxy::new(url).ok())
    {
        builder = builder.proxy(proxy);
    }
    builder.build()
}

/// The endpoint for a base URL as people usually paste it, with or without the version.
pub fn endpoint(protocol: &str, base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    match protocol {
        "anthropic" if base.ends_with("/v1") => format!("{base}/messages"),
        "anthropic" => format!("{base}/v1/messages"),
        _ if base.ends_with("/chat/completions") => base.to_string(),
        _ => format!("{base}/chat/completions"),
    }
}

fn complete_external(config: &AiConfig, prompt: &str, timeout: Duration) -> Result<String, String> {
    if config.base_url.is_empty() || config.model.is_empty() {
        return Err("E_AI_FIELDS".into());
    }
    let key = if config.api_key_sealed.is_empty() {
        String::new()
    } else {
        String::from_utf8(unseal(&unhex(&config.api_key_sealed)?)?).map_err(|_| "E_AI_KEY")?
    };
    let url = endpoint(&config.protocol, &config.base_url);
    // ureq is built without its JSON feature here, so the body goes as text.
    let request = if config.protocol == "anthropic" {
        agent(timeout)
            .post(&url)
            .set("x-api-key", &key)
            .set("anthropic-version", "2023-06-01")
    } else {
        agent(timeout)
            .post(&url)
            .set("Authorization", &format!("Bearer {key}"))
    };
    let body = external_body(&config.protocol, &config.model, &config.effort, prompt);
    let response = request
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    let body: serde_json::Value = match response {
        Ok(response) => {
            let text = response.into_string().map_err(|e| e.to_string())?;
            serde_json::from_str(&text).map_err(|_| "E_AI_REPLY".to_string())?
        }
        Err(ureq::Error::Status(status, response)) => {
            let text = response.into_string().unwrap_or_default();
            // A model without reasoning levels rejects the field; say which setting to change.
            let hint = if status == 400 && !config.effort.is_empty() {
                "（该模型可能不支持推理强度，可在偏好设置里改回“默认”）"
            } else {
                ""
            };
            return Err(format!(
                "HTTP {status}{hint}: {}",
                text.chars().take(300).collect::<String>()
            ));
        }
        Err(error) => return Err(error.to_string()),
    };
    external_text(&config.protocol, &body).ok_or_else(|| "E_AI_REPLY".into())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    (0..text.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(text.get(at..at + 2).unwrap_or(""), 16)
                .map_err(|_| "E_AI_KEY".to_string())
        })
        .collect()
}

#[cfg(windows)]
fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(plain, true)
}

#[cfg(windows)]
fn unseal(sealed: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(sealed, false)
}

/// The user's own Windows credentials protect the key: another account, or the same file on
/// another machine, cannot open it.
#[cfg(windows)]
fn dpapi(input: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let data = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &data,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &data,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
    };
    if ok == 0 || out.pbData.is_null() {
        return Err("E_AI_KEY".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(out.pbData as _);
    }
    Ok(bytes)
}

#[cfg(not(windows))]
fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn unseal(sealed: &[u8]) -> Result<Vec<u8>, String> {
    Ok(sealed.to_vec())
}

#[tauri::command]
pub fn ai_config_get() -> AiView {
    view(&load())
}

#[tauri::command]
pub fn ai_config_set(update: AiUpdate) -> Result<AiView, String> {
    apply(update)
}

#[tauri::command]
pub fn ai_config_use_local(model: String, effort: Option<String>) -> Result<AiView, String> {
    use_local(&model, effort.as_deref())
}

/// One short question, to prove the configuration answers.
#[tauri::command]
pub async fn ai_config_test() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| complete("Reply with exactly: OK"))
        .await
        .map_err(|e| e.to_string())?
}

/// What a folder on disk is, and whether deleting it does harm — asked of the configured AI.
#[tauri::command]
pub async fn ai_explain_path(path: String, bytes: u64, kind: String) -> Result<String, String> {
    let prompt = explain_prompt(&path, bytes, &kind);
    tauri::async_runtime::spawn_blocking(move || complete_answer(&prompt))
        .await
        .map_err(|e| e.to_string())?
}

fn explain_prompt(path: &str, bytes: u64, kind: &str) -> String {
    let size = if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64)
    };
    format!(
        "你是 Windows 开发机的磁盘清理顾问。用简体中文回答，不超过 150 字，不要用 Markdown 标题。\n\
         路径：{path}\n大小：{size}\n类型：{kind}\n\
         请说明：1) 这个目录属于哪个工具、存的是什么；2) 删除后会发生什么（是否会自动重新生成、是否丢数据）；\
         3) 一句话建议：可以放心删 / 删前确认 / 不要删。只凭路径判断，不要编造没把握的细节。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_english_interface_asks_for_an_english_answer() {
        assert_eq!(in_ui_language("用简体中文回答", "zh-CN"), "用简体中文回答");
        assert!(in_ui_language("用简体中文回答", "en-US")
            .ends_with("keep commands, paths and names as they are."));
    }

    #[test]
    fn the_reasoning_level_reaches_each_protocol_in_its_own_form() {
        let openai = external_body("openai", "o4", "high", "hi");
        assert_eq!(openai["reasoning_effort"], "high");
        assert!(external_body("openai", "gpt", "", "hi")
            .get("reasoning_effort")
            .is_none());
        let anthropic = external_body("anthropic", "claude", "xhigh", "hi");
        assert_eq!(anthropic["output_config"]["effort"], "xhigh");
        assert!(anthropic.get("thinking").is_none());
        assert!(anthropic.get("reasoning_effort").is_none());
        assert!(external_body("anthropic", "claude", "", "hi")
            .get("output_config")
            .is_none());
    }

    #[test]
    fn the_answer_is_the_text_block_after_any_thinking() {
        let body = serde_json::json!({ "content": [
            { "type": "thinking", "thinking": "..." },
            { "type": "text", "text": "OK" },
        ]});
        assert_eq!(external_text("anthropic", &body).as_deref(), Some("OK"));
    }

    #[test]
    fn an_unknown_reasoning_level_is_refused() {
        let update = AiUpdate {
            kind: "none".into(),
            effort: "turbo".into(),
            ..Default::default()
        };
        assert_eq!(apply(update).unwrap_err(), "E_AI_EFFORT");
        // "none" is OpenAI's value, not Anthropic's.
        let update = AiUpdate {
            kind: "external".into(),
            protocol: "anthropic".into(),
            effort: "none".into(),
            ..Default::default()
        };
        assert_eq!(apply(update).unwrap_err(), "E_AI_EFFORT");
    }

    #[test]
    fn an_endpoint_is_found_from_a_base_url_as_people_paste_it() {
        assert_eq!(
            endpoint("openai", "https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            endpoint("openai", "https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            endpoint("openai", "https://x/v1/chat/completions"),
            "https://x/v1/chat/completions"
        );
        assert_eq!(
            endpoint("anthropic", "https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            endpoint("anthropic", "https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn nothing_configured_is_said_plainly() {
        let config = AiConfig::default();
        assert_eq!(complete_with(&config, "hi").unwrap_err(), "E_AI_NONE");
        let external = AiConfig {
            kind: "external".into(),
            ..AiConfig::default()
        };
        assert_eq!(complete_with(&external, "hi").unwrap_err(), "E_AI_FIELDS");
    }

    #[test]
    fn a_key_survives_sealing_and_is_never_shown() {
        let sealed = seal(b"sk-test-123").unwrap();
        assert_ne!(sealed, b"sk-test-123");
        assert_eq!(
            unseal(&unhex(&hex(&sealed)).unwrap()).unwrap(),
            b"sk-test-123"
        );
        let config = AiConfig {
            api_key_sealed: hex(&sealed),
            ..AiConfig::default()
        };
        let shown = serde_json::to_string(&view(&config)).unwrap();
        assert!(!shown.contains("sk-test"));
        assert!(view(&config).has_key);
    }

    #[test]
    fn the_explanation_prompt_carries_the_path_and_size() {
        let prompt = explain_prompt("C:\\Users\\me\\.cargo\\registry", 3 << 30, "cache");
        assert!(prompt.contains("C:\\Users\\me\\.cargo\\registry"));
        assert!(prompt.contains("3.0 GB"));
    }
}
