//! Claude invocation: print mode, no persistence, no tools, no MCP servers.
use super::attachment::{Attachment, AttachmentKind};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn claude_args(model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--no-session-persistence",
        "--tools",
        "",
        "--strict-mcp-config",
        "--output-format",
        "json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(model) = model {
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(effort) = effort {
        args.push("--effort".into());
        args.push(effort.into());
    }
    args
}

/// Claude's print-mode flags with one stream-json message in and stream-json events out, so
/// images and PDFs can be sent and the answer arrives token by token (verified on 2.1.268).
pub fn claude_stream_args(model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut args = claude_args(model, effort);
    let at = args
        .iter()
        .position(|a| a == "--output-format")
        .unwrap_or(args.len());
    args.splice(
        at..(at + 2).min(args.len()),
        [
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
        ]
        .map(String::from),
    );
    args
}

/// The one user message written to stdin: attachments first, then the prompt.
pub fn stream_input(prompt: &str, attachments: &[Attachment]) -> String {
    let mut content: Vec<Value> = attachments
        .iter()
        .map(|a| {
            let kind = match a.kind {
                AttachmentKind::Image => "image",
                AttachmentKind::Pdf => "document",
            };
            json!({ "type": kind, "source": { "type": "base64", "media_type": a.media_type, "data": a.data } })
        })
        .collect();
    content.push(json!({ "type": "text", "text": prompt }));
    let message = json!({ "type": "user", "message": { "role": "user", "content": content } });
    format!("{message}\n")
}

/// Reads stream-json output one line at a time.
#[derive(Debug, Default)]
pub struct StreamState {
    /// Text deltas so far.
    pub text: String,
    /// The final `result` event, once seen.
    pub result: Option<Result<String, String>>,
}

impl StreamState {
    /// Takes one stdout line; returns the text it adds to the answer, if any.
    pub fn line(&mut self, line: &str) -> Option<String> {
        let value: Value = serde_json::from_str(line.trim()).ok()?;
        match value.get("type").and_then(Value::as_str)? {
            "stream_event" => {
                let event = value.get("event")?;
                let delta = event.get("delta")?;
                if event.get("type").and_then(Value::as_str) != Some("content_block_delta")
                    || delta.get("type").and_then(Value::as_str) != Some("text_delta")
                {
                    return None;
                }
                let text = delta.get("text").and_then(Value::as_str)?;
                self.text.push_str(text);
                Some(text.to_string()).filter(|t| !t.is_empty())
            }
            "result" => {
                self.result = Some(result_of(&value));
                None
            }
            _ => None,
        }
    }

    /// The `result` text, or the streamed text when the result carried none.
    pub fn answer(self) -> Result<String, String> {
        match self.result {
            Some(Err(code)) => Err(code),
            Some(Ok(text)) if !text.trim().is_empty() => Ok(text),
            _ if !self.text.trim().is_empty() => Ok(self.text),
            _ => Err("E_RUNNER_EMPTY".into()),
        }
    }
}

/// A `result` event's text, or the error code it stands for.
fn result_of(value: &Value) -> Result<String, String> {
    let text = value.get("result").and_then(Value::as_str);
    if value.get("is_error").and_then(Value::as_bool) == Some(true) {
        let lower = text.unwrap_or("").to_lowercase();
        if lower.contains("login") || lower.contains("log in") || lower.contains("401") {
            return Err("E_RUNNER_AUTH".into());
        }
        return Err("E_RUNNER_FAILED".into());
    }
    Ok(text.unwrap_or("").to_string())
}

/// Claude names project folders by replacing every non-alphanumeric character with `-`.
pub fn claude_project_slug(dir: &Path) -> String {
    dir.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let path = e.path();
            if path.is_dir() {
                has_files(&path)
            } else {
                true
            }
        })
    })
}

/// Removes the empty `projects\<slug>` folder print mode leaves behind.
pub fn remove_project_leftover(cwd: &Path) {
    let root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")));
    let Some(root) = root else { return };
    remove_leftover_in(&root, cwd);
}

pub fn remove_leftover_in(claude_root: &Path, cwd: &Path) {
    let mut candidates = vec![cwd.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(cwd) {
        candidates.push(real);
    }
    for dir in candidates {
        let project = claude_root.join("projects").join(claude_project_slug(&dir));
        if project.is_dir() && !has_files(&project) {
            let _ = std::fs::remove_dir_all(&project);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_disable_tools_and_persistence() {
        let args = claude_args(Some("sonnet"), Some("low"));
        assert_eq!(
            args,
            vec![
                "-p",
                "--no-session-persistence",
                "--tools",
                "",
                "--strict-mcp-config",
                "--output-format",
                "json",
                "--model",
                "sonnet",
                "--effort",
                "low"
            ]
        );
        assert_eq!(claude_args(None, None).len(), 7);
    }

    fn parse(lines: &[&str]) -> (Vec<String>, Result<String, String>) {
        let mut state = StreamState::default();
        let deltas = lines.iter().filter_map(|l| state.line(l)).collect();
        (deltas, state.answer())
    }

    #[test]
    fn stream_args_and_input() {
        let args = claude_stream_args(Some("sonnet"), Some("low"));
        let joined = args.join(" ");
        assert!(joined.starts_with("-p --no-session-persistence --tools  --strict-mcp-config --input-format stream-json --output-format stream-json --verbose --include-partial-messages --model sonnet --effort low"), "{joined}");
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--tools" && w[1].is_empty()));
        assert!(!args.contains(&"json".to_string()));

        let png = Attachment {
            kind: AttachmentKind::Image,
            media_type: "image/png".into(),
            data: "iVBO".into(),
            name: None,
        };
        let pdf = Attachment {
            kind: AttachmentKind::Pdf,
            media_type: "application/pdf".into(),
            data: "JVBE".into(),
            name: Some("a.pdf".into()),
        };
        let input = stream_input("Describe [attachment 1]", &[png, pdf]);
        assert_eq!(input.lines().count(), 1);
        let v: Value = serde_json::from_str(input.trim()).unwrap();
        let content = &v["message"]["content"];
        assert_eq!(v["type"], "user");
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["type"], "document");
        assert_eq!(content[1]["source"]["data"], "JVBE");
        assert_eq!(content[2]["text"], "Describe [attachment 1]");
    }

    #[test]
    fn stream_lines_parse() {
        let delta = |t: &str| {
            format!(
                r#"{{"type":"stream_event","event":{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{t}"}}}}}}"#
            )
        };
        let (a, b) = (delta("Hel"), delta("lo"));
        let (deltas, answer) = parse(&[
            r#"{"type":"system","subtype":"init","slash_commands":["login"]}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hm"}}}"#,
            &a,
            "not json",
            &b,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Hello!"}"#,
        ]);
        assert_eq!(deltas, vec!["Hel", "lo"]);
        assert_eq!(answer.unwrap(), "Hello!", "the result wins over the deltas");

        let (_, answer) = parse(&[&a, &b]);
        assert_eq!(
            answer.unwrap(),
            "Hello",
            "deltas stand in for a missing result"
        );
        let (_, answer) = parse(&[
            r#"{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}"#,
        ]);
        assert_eq!(answer.unwrap_err(), "E_RUNNER_AUTH");
        let (_, answer) = parse(&[
            &a,
            r#"{"type":"result","is_error":true,"result":"overloaded"}"#,
        ]);
        assert_eq!(answer.unwrap_err(), "E_RUNNER_FAILED");
        assert_eq!(parse(&["garbage"]).1.unwrap_err(), "E_RUNNER_EMPTY");
    }

    #[test]
    fn slug_matches_claude_naming() {
        assert_eq!(
            claude_project_slug(Path::new(
                r"C:\Users\simpl\AppData\Local\Temp\tmp.vBAHrRWx5Z"
            )),
            "C--Users-simpl-AppData-Local-Temp-tmp-vBAHrRWx5Z"
        );
    }

    #[test]
    fn only_empty_leftovers_are_removed() {
        let root = tempfile::tempdir().unwrap();
        let cwd = Path::new(r"C:\x\run.1");
        let empty = root.path().join("projects").join("C--x-run-1");
        std::fs::create_dir_all(empty.join("memory")).unwrap();
        remove_leftover_in(root.path(), cwd);
        assert!(!empty.exists());

        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("s.jsonl"), b"{}").unwrap();
        remove_leftover_in(root.path(), cwd);
        assert!(empty.exists(), "folders with files stay");
    }
}
