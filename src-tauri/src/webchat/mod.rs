//! Web chats from the Stacker browser extension: native-messaging bridge, storage and sync.
pub mod bodies;
pub mod bridge;
pub mod export;
pub mod framing;
pub mod host;
pub mod protocol;
pub mod store;

use std::path::PathBuf;

/// Where web chats live: the same folder as `sessions.sqlite3`.
/// Debug builds honour `STACKER_WEBCHAT_DIR` so the bridge can be probed against a scratch folder.
pub fn root() -> PathBuf {
    if cfg!(debug_assertions) {
        if let Some(dir) = std::env::var_os("STACKER_WEBCHAT_DIR").filter(|d| !d.is_empty()) {
            return PathBuf::from(dir);
        }
    }
    crate::sessions::annotations::root()
}

/// Stacker's export folder; web chat exports go into its `web` subfolder.
pub fn export_dir() -> PathBuf {
    root().join("exports")
}

/// The extension's fixed ID (derived from the manifest key), recorded in `extension/EXTENSION_ID`.
pub fn extension_id() -> &'static str {
    include_str!("../../../extension/EXTENSION_ID").trim()
}

/// The only caller the bridge and the host manifest accept.
pub fn allowed_origin() -> String {
    format!("chrome-extension://{}/", extension_id())
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn origin_uses_the_recorded_extension_id() {
        let id = super::extension_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| ('a'..='p').contains(&c)));
        assert_eq!(super::allowed_origin(), format!("chrome-extension://{id}/"));
    }
}
