//! 提炼的材料：网页对话正文、本机会话原文、摘录。
//! 本机会话由调用方先读好再传进来，单元测试因此不会去读用户的真实会话目录。
use super::pipeline::SourceText;
use super::DistillSource;
use crate::sessions::model::Session;
use crate::webchat::store;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 界面传来的来源选择。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceRef {
    /// "web" | "session" | "excerpt"
    pub kind: String,
    /// 网页对话 `<site>:<id>`；本机会话 `Session::id`；摘录的 id。
    pub key: String,
}

/// 开始对话框里的候选来源。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub kind: String,
    pub key: String,
    pub title: String,
    pub subtitle: String,
    /// 网页对话还没有同步正文时为 false：不能提炼。
    pub available: bool,
}

/// 每一类候选最多返回多少条。
pub const MAX_CANDIDATES: usize = 50;
/// 摘录合并成的那份材料，标题最多这么长。
const UNIT_TITLE_CHARS: usize = 120;

pub fn source_key(kind: &str, key: &str) -> String {
    format!("{kind}:{key}")
}

fn excerpt_unit(parts: Vec<(DistillSource, String)>) -> SourceText {
    let title: String = parts
        .iter()
        .map(|(s, _)| s.title.clone())
        .collect::<Vec<_>>()
        .join(" / ")
        .chars()
        .take(UNIT_TITLE_CHARS)
        .collect();
    let markdown = format!(
        "# {title}\n\n{}",
        parts
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    );
    SourceText {
        title,
        markdown,
        sources: parts.into_iter().map(|(s, _)| s).collect(),
    }
}

/// 把选中的来源读成若干份材料：每条网页对话、每个本机会话各一份，全部摘录合成一份。
/// 读不出任何内容时返回 `E_NO_BODY`；读不出的单条静默跳过。
pub fn gather(
    root: &Path,
    sessions: &[Session],
    refs: &[SourceRef],
) -> Result<Vec<SourceText>, String> {
    let conn = store::open(root)?;
    let all_excerpts = if refs.iter().any(|r| r.kind == "excerpt") {
        store::excerpts(&conn)?
    } else {
        Vec::new()
    };
    let mut units: Vec<SourceText> = Vec::new();
    let mut excerpts: Vec<(DistillSource, String)> = Vec::new();
    for r in refs {
        match r.kind.as_str() {
            "web" => {
                let Ok(chat) = store::chat(&conn, &r.key) else {
                    continue;
                };
                if chat.body_fetched_at.is_none() {
                    continue;
                }
                let Ok(body) = store::body(root, &chat) else {
                    continue;
                };
                let markdown = crate::webchat::commands::body_markdown(&chat.title, &body.messages);
                units.push(SourceText {
                    title: chat.title.clone(),
                    markdown,
                    sources: vec![DistillSource {
                        key: source_key("web", &r.key),
                        kind: "web".into(),
                        title: chat.title,
                        link: String::new(),
                    }],
                });
            }
            "session" => {
                let Some(session) = sessions.iter().find(|s| s.id == r.key) else {
                    continue;
                };
                let Ok(markdown) = crate::sessions::summary::transcript_markdown(session) else {
                    continue;
                };
                units.push(SourceText {
                    title: session.title.clone(),
                    markdown,
                    sources: vec![DistillSource {
                        key: source_key("session", &r.key),
                        kind: "session".into(),
                        title: session.title.clone(),
                        link: session.path.clone(),
                    }],
                });
            }
            "excerpt" => {
                let Some(e) = all_excerpts.iter().find(|e| e.id == r.key) else {
                    continue;
                };
                let title = if e.page_title.is_empty() {
                    e.id.clone()
                } else {
                    e.page_title.clone()
                };
                excerpts.push((
                    DistillSource {
                        key: source_key("excerpt", &e.id),
                        kind: "excerpt".into(),
                        title: title.clone(),
                        link: e.url.clone(),
                    },
                    format!("### {title}\n\n{}\n\n{}\n", e.text, e.note),
                ));
            }
            _ => continue,
        }
    }
    if !excerpts.is_empty() {
        units.push(excerpt_unit(excerpts));
    }
    if units.is_empty() {
        return Err("E_NO_BODY".into());
    }
    Ok(units)
}

/// 候选来源：网页对话、本机会话、摘录各最多 50 条，按这个顺序返回。
pub fn candidates(
    root: &Path,
    sessions: &[Session],
    search: &str,
) -> Result<Vec<Candidate>, String> {
    let needle = search.trim().to_lowercase();
    let conn = store::open(root)?;
    let query = store::WebQuery {
        search: search.trim().to_string(),
        ..Default::default()
    };
    let mut out: Vec<Candidate> = store::list(&conn, root, &query)?
        .items
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|c| Candidate {
            kind: "web".into(),
            key: c.key,
            title: c.title,
            subtitle: format!("{} · {}", c.site, c.account_name),
            available: c.body_fetched_at.is_some(),
        })
        .collect();
    out.extend(
        sessions
            .iter()
            .filter(|s| !crate::sessions::catalog::is_automation(s))
            .filter(|s| {
                needle.is_empty()
                    || s.title.to_lowercase().contains(&needle)
                    || s.project.name.to_lowercase().contains(&needle)
            })
            .take(MAX_CANDIDATES)
            .map(|s| Candidate {
                kind: "session".into(),
                key: s.id.clone(),
                title: s.title.clone(),
                subtitle: format!("{} · {}", s.agent.as_str(), s.project.name),
                available: true,
            }),
    );
    out.extend(
        store::excerpts(&conn)?
            .into_iter()
            .filter(|e| {
                needle.is_empty()
                    || e.text.to_lowercase().contains(&needle)
                    || e.page_title.to_lowercase().contains(&needle)
            })
            .take(MAX_CANDIDATES)
            .map(|e| Candidate {
                kind: "excerpt".into(),
                title: if e.page_title.is_empty() {
                    e.id.clone()
                } else {
                    e.page_title.clone()
                },
                subtitle: e.text.chars().take(60).collect(),
                key: e.id,
                available: true,
            }),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webchat::protocol::{BodyChunk, WebConversation, WebExcerpt, WebMessage};
    use crate::webchat::store;

    fn seed(root: &Path) {
        let conn = store::open(root).unwrap();
        let conversation = |id: &str, title: &str| WebConversation {
            key: format!("chatgpt:{id}"),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: id.into(),
            title: title.into(),
            updated_at: 5,
            listed_at: 1,
            ..Default::default()
        };
        store::upsert_conversations(
            &conn,
            &[
                conversation("a", "Trip plan"),
                conversation("nobody", "No body yet"),
            ],
        )
        .unwrap();
        store::put_body_chunk(
            &conn,
            root,
            &BodyChunk {
                key: "chatgpt:a".into(),
                site: "chatgpt".into(),
                account: "chatgpt:u1".into(),
                id: "a".into(),
                title: "Trip plan".into(),
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
            },
        )
        .unwrap();
        store::upsert_excerpts(
            &conn,
            &[
                WebExcerpt {
                    id: "e1".into(),
                    site: "chatgpt".into(),
                    conversation_id: Some("a".into()),
                    url: "https://chatgpt.com/c/a".into(),
                    page_title: "Trip plan".into(),
                    text: "Kyoto in spring".into(),
                    note: "remember".into(),
                    created_at: 1,
                    local_updated_at: 1,
                },
                WebExcerpt {
                    id: "e2".into(),
                    site: "chatgpt".into(),
                    conversation_id: None,
                    url: String::new(),
                    page_title: "Budget".into(),
                    text: "Book early".into(),
                    note: String::new(),
                    created_at: 2,
                    local_updated_at: 2,
                },
            ],
        )
        .unwrap();
    }

    fn refs(pairs: &[(&str, &str)]) -> Vec<SourceRef> {
        pairs
            .iter()
            .map(|(kind, key)| SourceRef {
                kind: (*kind).into(),
                key: (*key).into(),
            })
            .collect()
    }

    #[test]
    fn a_web_chat_becomes_one_unit_with_its_body() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        let units = gather(dir.path(), &[], &refs(&[("web", "chatgpt:a")])).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].title, "Trip plan");
        assert!(units[0].markdown.contains("Where should we go?"));
        assert_eq!(units[0].sources[0].key, "web:chatgpt:a");
        assert_eq!(units[0].sources[0].kind, "web");
    }

    #[test]
    fn excerpts_are_gathered_into_one_unit_and_keep_every_source() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        let units = gather(
            dir.path(),
            &[],
            &refs(&[("excerpt", "e1"), ("excerpt", "e2")]),
        )
        .unwrap();
        assert_eq!(units.len(), 1);
        assert!(
            units[0].markdown.contains("Kyoto in spring")
                && units[0].markdown.contains("Book early")
        );
        assert_eq!(
            units[0]
                .sources
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>(),
            vec!["excerpt:e1", "excerpt:e2"]
        );
        assert_eq!(units[0].sources[0].link, "https://chatgpt.com/c/a");
    }

    #[test]
    fn missing_and_body_less_sources_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        let mixed = refs(&[
            ("web", "chatgpt:a"),
            ("web", "chatgpt:nobody"),
            ("web", "chatgpt:gone"),
            ("nonsense", "x"),
        ]);
        assert_eq!(gather(dir.path(), &[], &mixed).unwrap().len(), 1);
        assert_eq!(
            gather(dir.path(), &[], &refs(&[("web", "chatgpt:nobody")])).unwrap_err(),
            "E_NO_BODY"
        );
        assert_eq!(gather(dir.path(), &[], &[]).unwrap_err(), "E_NO_BODY");
    }

    #[test]
    fn candidates_flag_a_chat_without_a_body_and_match_the_search() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        let all = candidates(dir.path(), &[], "").unwrap();
        let web: Vec<&Candidate> = all.iter().filter(|c| c.kind == "web").collect();
        assert_eq!(web.len(), 2);
        assert!(web.iter().any(|c| c.key == "chatgpt:a" && c.available));
        assert!(web
            .iter()
            .any(|c| c.key == "chatgpt:nobody" && !c.available));
        assert_eq!(all.iter().filter(|c| c.kind == "excerpt").count(), 2);
        let found = candidates(dir.path(), &[], "kyoto").unwrap();
        assert_eq!(
            found.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            vec!["e1"],
            "excerpts are searched by body; web chats by title, note, tags and summary"
        );
    }
}
