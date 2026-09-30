//! Tauri commands for the 网页对话 tab and the 浏览器插件 settings block.
use super::host::{self, Browser, UserRegistry};
use super::protocol::WebMessage;
use super::store::{self, WebChatRow, WebPage, WebQuery};
use crate::runner::CancelFlag;
use crate::sessions::commands::{blocking, explorer};
use crate::sessions::summary::{self, RunnerChoice};
use crate::sessions::summary_job::{live_runner, Runner};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebchatStatus {
    pub extension_id: String,
    pub extension_dir: String,
    pub extension_found: bool,
    pub data_dir: String,
    pub export_dir: String,
    pub browsers: Vec<host::BrowserStatus>,
    pub last_hello_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    pub counts: store::Counts,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebChatDetail {
    pub chat: WebChatRow,
    pub messages: Vec<WebMessage>,
    /// Characters a summary would send.
    pub chars: usize,
    /// Who would write a summary; absent while no AI source is set.
    pub runner: Option<RunnerChoice>,
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|_| "E_PATH".to_string())
}

fn status_now() -> Result<WebchatStatus, String> {
    let root = super::root();
    let conn = store::open(&root)?;
    let (dir, found) = host::extension_dir();
    Ok(WebchatStatus {
        extension_id: super::extension_id().to_string(),
        extension_dir: dir.to_string_lossy().into_owned(),
        extension_found: found,
        data_dir: root.to_string_lossy().into_owned(),
        export_dir: super::export_dir()
            .join("web")
            .to_string_lossy()
            .into_owned(),
        browsers: host::status(&UserRegistry, &root, &exe()?),
        last_hello_at: store::meta(&conn, "last_hello_at"),
        last_sync_at: store::meta(&conn, "last_sync_at"),
        counts: store::counts(&conn)?,
    })
}

#[tauri::command]
pub async fn webchat_status() -> Result<WebchatStatus, String> {
    blocking(status_now).await
}

/// Writes the manifest and the browser's HKCU key; only ever called from the user's click.
#[tauri::command]
pub async fn webchat_connect(browser: String) -> Result<WebchatStatus, String> {
    blocking(move || {
        let browser = Browser::parse(&browser)?;
        host::connect(
            &UserRegistry,
            browser,
            &super::root(),
            &exe()?,
            super::extension_id(),
        )?;
        status_now()
    })
    .await
}

#[tauri::command]
pub async fn webchat_disconnect(browser: String) -> Result<WebchatStatus, String> {
    blocking(move || {
        host::disconnect(&UserRegistry, Browser::parse(&browser)?, &super::root())?;
        status_now()
    })
    .await
}

/// `target`: `extension` (the unpacked extension folder) or `exports` (web chat exports).
#[tauri::command]
pub async fn webchat_open(target: String) -> Result<(), String> {
    blocking(move || match target.as_str() {
        "extension" => {
            let (dir, found) = host::extension_dir();
            if !found {
                return Err("E_NO_EXTENSION".into());
            }
            explorer(dir)
        }
        "exports" => {
            let dir = super::export_dir().join("web");
            std::fs::create_dir_all(&dir).map_err(|_| "E_STORAGE".to_string())?;
            explorer(dir)
        }
        _ => Err("E_REQUEST".into()),
    })
    .await
}

#[tauri::command]
pub async fn webchat_list(query: WebQuery) -> Result<WebPage, String> {
    blocking(move || {
        let root = super::root();
        store::list(&store::open(&root)?, &root, &query)
    })
    .await
}

/// Same heading shape as local sessions ("\n### "), so long chats split into parts the same way.
pub fn body_markdown(title: &str, messages: &[WebMessage]) -> String {
    let mut out = format!("# {title}\n\n");
    for m in messages {
        let heading = match m.role.as_str() {
            "user" => "用户",
            "assistant" => "助手",
            _ => "工具",
        };
        out.push_str(&format!("### {heading}\n\n{}\n\n", m.text));
    }
    out
}

#[tauri::command]
pub async fn webchat_read(key: String) -> Result<WebChatDetail, String> {
    blocking(move || {
        let root = super::root();
        let conn = store::open(&root)?;
        let chat = store::chat(&conn, &key)?;
        let messages = if chat.body_fetched_at.is_some() {
            store::body(&root, &chat)?.messages
        } else {
            Vec::new()
        };
        let chars = if messages.is_empty() {
            0
        } else {
            body_markdown(&chat.title, &messages).chars().count()
        };
        Ok(WebChatDetail {
            // Nothing configured is not an error here: the chat still reads; only
            // summarising asks for a source.
            runner: crate::ai_config::runner_choice().ok(),
            chat,
            messages,
            chars,
        })
    })
    .await
}

pub fn summarize_in(
    root: &Path,
    key: &str,
    choice: &RunnerChoice,
    locale: &str,
    cancel: &CancelFlag,
    run: &Runner,
) -> Result<WebChatRow, String> {
    let conn = store::open(root)?;
    let chat = store::chat(&conn, key)?;
    if chat.body_fetched_at.is_none() {
        return Err("E_NO_BODY".into());
    }
    let body = store::body(root, &chat)?;
    let markdown = body_markdown(&chat.title, &body.messages);
    let text = summary::summarize_text(&markdown, choice, locale, cancel, run.as_ref())?;
    store::save_summary(&conn, key, &text, &choice.label(), super::now_ms())?;
    store::chat(&conn, key)
}

static SUMMARY: Mutex<Option<CancelFlag>> = Mutex::new(None);

/// Clears the running-summary slot however the summary ends, including a panic.
struct Running;

impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(mut slot) = SUMMARY.lock() {
            *slot = None;
        }
    }
}

#[tauri::command]
pub async fn webchat_summarize(key: String, locale: String) -> Result<WebChatRow, String> {
    blocking(move || {
        let flag = CancelFlag::default();
        {
            let mut slot = SUMMARY.lock().map_err(|_| "E_SUMMARY_BUSY".to_string())?;
            if slot.is_some() {
                return Err("E_SUMMARY_BUSY".into());
            }
            *slot = Some(flag.clone());
        }
        let _running = Running;
        let choice = crate::ai_config::runner_choice()?;
        summarize_in(
            &super::root(),
            &key,
            &choice,
            &locale,
            &flag,
            &live_runner(),
        )
    })
    .await
}

#[tauri::command]
pub fn webchat_summary_cancel() {
    if let Ok(slot) = SUMMARY.lock() {
        if let Some(flag) = slot.as_ref() {
            flag.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RunOutput, RunRequest};
    use crate::webchat::protocol::{BodyChunk, WebConversation};
    use std::sync::Arc;

    #[test]
    fn markdown_has_one_heading_per_message() {
        let messages = vec![
            WebMessage {
                role: "user".into(),
                text: "Where should we go?".into(),
                at: None,
                attachments: vec![],
            },
            WebMessage {
                role: "assistant".into(),
                text: "Kyoto.".into(),
                at: None,
                attachments: vec![],
            },
        ];
        let md = body_markdown("Trip", &messages);
        assert!(md.starts_with("# Trip\n\n### "));
        assert_eq!(md.matches("\n### ").count(), 2);
        assert!(md.contains("Where should we go?") && md.contains("Kyoto."));
    }

    #[test]
    fn summarize_saves_the_summary_with_its_runner() {
        let dir = tempfile::tempdir().unwrap();
        let conn = store::open(dir.path()).unwrap();
        let conversation = |id: &str| WebConversation {
            key: format!("chatgpt:{id}"),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: id.into(),
            title: "Trip".into(),
            updated_at: 5,
            listed_at: 1,
            ..Default::default()
        };
        store::upsert_conversations(&conn, &[conversation("a"), conversation("nobody")]).unwrap();
        let body = BodyChunk {
            key: "chatgpt:a".into(),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: "a".into(),
            title: "Trip".into(),
            updated_at: 5,
            fetched_at: 6,
            chunk: 0,
            chunks: 1,
            messages: vec![WebMessage {
                role: "user".into(),
                text: "Where should we go?".into(),
                at: None,
                attachments: vec![],
            }],
        };
        store::put_body_chunk(&conn, dir.path(), &body).unwrap();
        let run: Runner = Arc::new(|req: &RunRequest, _: &CancelFlag| {
            assert!(req.prompt.contains("Where should we go?"));
            assert_eq!(req.backend, "claude");
            Ok::<_, String>(RunOutput {
                text: "Visit Kyoto".into(),
            })
        });
        let choice = RunnerChoice::local("claude", Some("sonnet"), Some("low"));
        let row = summarize_in(
            dir.path(),
            "chatgpt:a",
            &choice,
            "en",
            &CancelFlag::default(),
            &run,
        )
        .unwrap();
        assert_eq!(row.summary.as_deref(), Some("Visit Kyoto"));
        assert_eq!(row.summary_by, "claude / sonnet / low");
        let missing = summarize_in(
            dir.path(),
            "chatgpt:nobody",
            &choice,
            "en",
            &CancelFlag::default(),
            &run,
        );
        assert_eq!(missing.unwrap_err(), "E_NO_BODY");
    }
}
