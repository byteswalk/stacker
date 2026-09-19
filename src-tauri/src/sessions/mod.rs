pub mod annotations;
pub mod catalog;
pub mod claude_catalog;
pub mod codex_catalog;
pub mod codex_rpc;
pub mod commands;
pub mod delete;
pub mod export;
pub mod footprint;
pub mod model;
pub mod project;
pub mod roots;
pub mod transcript;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
