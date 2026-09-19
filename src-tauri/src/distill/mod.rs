//! 提炼：从网页对话、本机会话与摘录里提炼可复用的经验问答、领域要求、提示词与 skill 草稿。
//! 结果与网页对话同库（`webchat.sqlite3`），本机会话与网页对话共用一个结果库。
//! 只写本机文件，绝不把 skill 安装到任何智能体目录。
pub mod prompts;
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

/// 提炼的数据目录：`<conversations>\distill`，随数据目录迁移。
pub fn root() -> PathBuf {
    crate::webchat::root().join("distill")
}

/// skill 草稿目录：只写文件，永不安装。
pub fn skills_in(root: &Path) -> PathBuf {
    root.join("distill").join("skills")
}

pub fn skills_root() -> PathBuf {
    skills_in(&crate::webchat::root())
}

/// 提炼结果的导出目录。
pub fn exports_in(root: &Path) -> PathBuf {
    root.join("exports").join("distill")
}

#[cfg(test)]
mod tests {
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
}
