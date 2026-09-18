//! Slim Markdown copies of sessions: readable text only, no snapshots or image data.
use super::model::*;
use super::transcript::{self, Message};
use std::path::{Path, PathBuf};

const TOOL_TEXT_LIMIT: usize = 2_000;

fn safe(name: &str, limit: usize) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if "\\/:*?\"<>|".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(limit)
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim().to_string();
    if trimmed.is_empty() {
        "session".into()
    } else {
        trimmed
    }
}

fn date(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

pub fn slim_markdown(session: &Session, messages: &[Message]) -> String {
    let mut out = format!(
        "# {}\n\n- 智能体：{}\n- 项目：{}\n- 创建：{}\n- 最后活动：{}\n- 会话 ID：{}\n- 原始记录：{}\n\n",
        session.title,
        session.agent.as_str(),
        session.project.path,
        date(session.created_at),
        date(session.updated_at),
        session.native_id,
        session.path
    );
    for m in messages {
        let heading = match m.role.as_str() {
            "user" => "用户",
            "assistant" => "助手",
            _ => "工具",
        };
        let mut text = m.text.clone();
        if heading == "工具" && text.chars().count() > TOOL_TEXT_LIMIT {
            text = text.chars().take(TOOL_TEXT_LIMIT).collect::<String>() + "\n…（已截断）";
        }
        out.push_str(&format!("### {heading}\n\n{text}\n\n"));
    }
    out
}

/// Writes `<dir>/<date>/<agent>/<project>/<title>-<id>.md`.
pub fn write_slim(session: &Session, dir: &Path) -> Result<PathBuf, String> {
    let (messages, _) = transcript::read(session.agent, Path::new(&session.path))?;
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    let folder = dir
        .join(day)
        .join(session.agent.as_str())
        .join(safe(&session.project.name, 60));
    std::fs::create_dir_all(&folder).map_err(|_| "E_STORAGE".to_string())?;
    let file = folder.join(format!(
        "{}-{}.md",
        safe(&session.title, 60),
        safe(&session.native_id, 80)
    ));
    std::fs::write(&file, slim_markdown(session, &messages))
        .map_err(|_| "E_STORAGE".to_string())?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slim_export_keeps_readable_text_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let image = "A".repeat(50_000);
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::json!({"type":"user","sessionId":"s","message":{"content":[{"type":"text","text":"question"},{"type":"image","source":{"data":image}}]}}),
                serde_json::json!({"type":"assistant","sessionId":"s","message":{"content":[{"type":"text","text":"answer"}]}})
            ),
        )
        .unwrap();
        let mut session = super::super::catalog::tests_support::session("claude:s");
        session.agent = Agent::Claude;
        session.native_id = "s".into();
        session.title = "a/b:c".into();
        session.path = path.to_string_lossy().into_owned();
        let out = write_slim(&session, &dir.path().join("exports")).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("question") && text.contains("answer"));
        assert!(text.contains("[image attachment]"));
        assert!(text.len() < 2_000, "image data must not be exported");
        assert!(out
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("a_b_c-s"));
    }
}
