pub mod annotations;
pub mod catalog;
pub mod claude_catalog;
pub mod codebuddy_catalog;
pub mod codex_catalog;
pub mod codex_rpc;
pub mod commands;
pub mod delete;
pub mod export;
pub mod footprint;
pub mod handoff;
pub mod kimi_catalog;
pub mod migration;
pub mod mimo_catalog;
pub mod model;
pub mod project;
pub mod roots;
pub mod summary;
pub mod summary_job;
pub mod transcript;
pub mod workbuddy_catalog;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
