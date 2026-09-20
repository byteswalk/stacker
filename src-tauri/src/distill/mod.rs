//! 提炼：从网页对话、本机会话与摘录里提炼可复用的经验问答、领域要求、提示词与 skill 草稿。
//! 结果与网页对话同库（`webchat.sqlite3`），本机会话与网页对话共用一个结果库。
//! 只写本机文件，绝不把 skill 安装到任何智能体目录。
pub mod commands;
pub mod job;
pub mod pipeline;
pub mod prompts;
pub mod skills;
pub mod sources;
pub mod store;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 产出类型；顺序即界面顺序。
pub const KINDS: [&str; 4] = ["qa", "requirement", "prompt", "skill"];

pub fn is_kind(kind: &str) -> bool {
    KINDS.contains(&kind)
}

/// 结果状态：草稿或已采用。
pub fn is_state(state: &str) -> bool {
    state == "draft" || state == "adopted"
}

/// 一条结果的来源。`key` 同时是 `distill_sources` 的查询键：
/// 网页对话 `web:<site>:<id>`、本机会话 `session:<agent>:<nativeId>`、摘录 `excerpt:<id>`。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DistillSource {
    pub key: String,
    /// "web" | "session" | "excerpt"
    pub kind: String,
    pub title: String,
    /// 原文链接：摘录用保存时记下的地址，本机会话用原始记录路径；
    /// 网页对话留空，由前端按站点拼（Stacker 侧不枚举站点）。
    pub link: String,
}

/// skill 草稿目录：只写文件，永不安装。`root` 是网页对话的数据目录（`crate::webchat::root()`）。
pub fn skills_in(root: &Path) -> PathBuf {
    root.join("distill").join("skills")
}

/// 提炼结果的导出目录。
pub fn exports_in(root: &Path) -> PathBuf {
    root.join("exports").join("distill")
}

/// 发给插件的只读结果：最多 20 条、正文截到 4000 字，远低于桥接一帧 1 MiB 的上限。
pub const BRIDGE_MAX_ITEMS: usize = 20;
pub const BRIDGE_BODY_CHARS: usize = 4_000;

/// 桥接一帧标题的上限：和落库时用的 `pipeline::TITLE_CHARS` 一致，编辑框改出的标题
/// 理论上已经被 `commands::distill_save` 截过一次，这里再兜底一次，来源标题同理。
fn bridge_title(title: &str) -> String {
    title.chars().take(pipeline::TITLE_CHARS).collect()
}

pub fn bridge_items(items: Vec<store::DistillResult>) -> Vec<serde_json::Value> {
    items
        .into_iter()
        .take(BRIDGE_MAX_ITEMS)
        .map(|r| {
            let mut body: String = r.body.chars().take(BRIDGE_BODY_CHARS).collect();
            if r.body.chars().count() > BRIDGE_BODY_CHARS {
                body.push('…');
            }
            serde_json::json!({
                "id": r.id,
                "kind": r.kind,
                "title": bridge_title(&r.title),
                "body": body,
                "state": r.state,
                "updatedAt": r.updated_at,
                "sources": r.sources.iter().map(|s| bridge_title(&s.title)).collect::<Vec<_>>(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distill::store::DistillResult;
    use crate::distill::DistillSource;

    #[test]
    fn kinds_and_states_are_checked() {
        assert!(super::KINDS.iter().all(|k| super::is_kind(k)));
        assert!(!super::is_kind("poem") && !super::is_kind(""));
        assert!(super::is_state("draft") && super::is_state("adopted"));
        assert!(!super::is_state("published"));
    }

    #[test]
    fn skill_drafts_live_under_the_data_folder() {
        let root = std::path::Path::new("C:/data");
        assert!(super::skills_in(root).ends_with(std::path::Path::new("distill/skills")));
        assert!(super::skills_in(root).starts_with(root));
        assert!(super::exports_in(root).ends_with(std::path::Path::new("exports/distill")));
    }

    #[test]
    fn bridge_items_cap_title_and_source_titles_as_well_as_the_body() {
        let long_title: String = "t".repeat(pipeline::TITLE_CHARS + 50);
        let long_source_title: String = "s".repeat(pipeline::TITLE_CHARS + 50);
        let result = DistillResult {
            id: "r1".into(),
            kind: "qa".into(),
            title: long_title.clone(),
            body: "b".repeat(BRIDGE_BODY_CHARS + 50),
            sources: vec![DistillSource {
                key: "web:chatgpt:a".into(),
                kind: "web".into(),
                title: long_source_title.clone(),
                link: String::new(),
            }],
            state: "draft".into(),
            by: "claude / sonnet / low".into(),
            folder: String::new(),
            created_at: 1,
            updated_at: 2,
        };
        let items = bridge_items(vec![result]);
        let item = &items[0];
        assert_eq!(
            item["title"].as_str().unwrap().chars().count(),
            pipeline::TITLE_CHARS,
            "an edited title cannot grow the bridge frame without bound"
        );
        assert_eq!(
            item["sources"][0].as_str().unwrap().chars().count(),
            pipeline::TITLE_CHARS,
            "a source title is capped the same way"
        );
        let body = item["body"].as_str().unwrap();
        assert!(body.ends_with('…'));
        assert_eq!(body.chars().count(), BRIDGE_BODY_CHARS + 1);
    }
}
