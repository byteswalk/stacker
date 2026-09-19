//! Conversation bodies as gzip JSON: `<root>/webchat/bodies/<site>/<account>/<id>.json.gz`.
use super::protocol::StoredBody;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 8 hex characters of a stable hash of `raw`, so two different ids that clean to the same safe
/// string (e.g. "a/b" and "a_b") never collide on disk.
fn short_hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest[..4].iter().map(|b| format!("{b:02x}")).collect()
}

/// One safe path component: ASCII letters, digits, `.`, `_`, `-`; never empty, `.`, `..` or a device
/// name. An id that had to be cleaned or cut keeps a `-<hash>` suffix of the original so it can't
/// collide with a different id that cleans to the same string; an id that passes through unchanged
/// keeps today's plain name.
pub fn component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .take(120)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_end_matches('.');
    let base = if trimmed.is_empty() {
        "_".to_string()
    } else {
        trimmed.to_string()
    };
    let stem = base.split('.').next().unwrap_or("").to_ascii_uppercase();
    let safe = if RESERVED.contains(&stem.as_str()) {
        format!("_{base}")
    } else {
        base
    };
    if safe == raw {
        safe
    } else {
        format!("{safe}-{}", short_hash(raw))
    }
}

pub fn body_path(root: &Path, site: &str, account: &str, id: &str) -> PathBuf {
    let remote = account.split_once(':').map_or(account, |(_, r)| r);
    root.join("webchat")
        .join("bodies")
        .join(component(site))
        .join(component(remote))
        .join(format!("{}.json.gz", component(id)))
}

/// Writes to a temporary file first so a crash never leaves half a body.
pub fn write_body(path: &Path, body: &StoredBody) -> Result<(), String> {
    let storage = |_| "E_STORAGE".to_string();
    let dir = path.parent().ok_or("E_STORAGE")?;
    std::fs::create_dir_all(dir).map_err(storage)?;
    let json = serde_json::to_vec(body).map_err(|_| "E_STORAGE".to_string())?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&json).map_err(storage)?;
    let bytes = encoder.finish().map_err(storage)?;
    let tmp = path.with_extension("gz.tmp");
    std::fs::write(&tmp, bytes).map_err(storage)?;
    std::fs::rename(&tmp, path).map_err(storage)
}

pub fn read_body(path: &Path) -> Result<StoredBody, String> {
    let file = std::fs::File::open(path).map_err(|_| "E_NOT_FOUND".to_string())?;
    let mut json = Vec::new();
    GzDecoder::new(file)
        .read_to_end(&mut json)
        .map_err(|_| "E_STORAGE".to_string())?;
    serde_json::from_slice(&json).map_err(|_| "E_STORAGE".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_components_are_safe() {
        // Already-safe ids pass through unchanged.
        assert_eq!(component("abc-DEF_1.2"), "abc-DEF_1.2");
        // Anything that had to be cleaned or cut keeps a `-<8 hex chars>` suffix of the original.
        assert!(component("..").starts_with("_-"));
        assert!(component("").starts_with("_-"));
        assert!(component(".").starts_with("_-"));
        assert!(component("a/b\\c:d").starts_with("a_b_c_d-"));
        assert!(component("CON").starts_with("_CON-"));
        assert!(component("con.txt").starts_with("_con.txt-"));
        let long = component(&"x".repeat(500));
        assert!(long.starts_with(&"x".repeat(120)));
        assert_eq!(long.len(), 120 + 1 + 8);
    }

    #[test]
    fn cleaned_ids_get_a_hash_suffix_so_distinct_ids_cannot_collide() {
        // Before the fix both of these cleaned to the plain string "a_b", colliding on disk.
        let cleaned = component("a/b");
        let already_safe = component("a_b");
        assert_eq!(
            already_safe, "a_b",
            "an id that needs no cleaning keeps its plain name"
        );
        assert_ne!(cleaned, already_safe);
        assert!(cleaned.starts_with("a_b-"));
        assert_eq!(cleaned.len(), "a_b-".len() + 8);
        // The hash is stable for the same input.
        assert_eq!(component("a/b"), cleaned);
        // And different inputs that clean the same way get different hashes.
        assert_ne!(component("a/b"), component("a\\b"));
    }

    #[test]
    fn bodies_live_under_site_and_account_without_escaping() {
        let root = Path::new("C:/data");
        let p = body_path(root, "chatgpt", "chatgpt:u1", "abc");
        assert!(p.ends_with(Path::new("webchat/bodies/chatgpt/u1/abc.json.gz")));
        let hostile = body_path(root, "..", "chatgpt:../..", "../x");
        assert!(hostile.starts_with(root.join("webchat").join("bodies")));
        assert!(!hostile
            .components()
            .any(|c| c == std::path::Component::ParentDir));
    }

    #[test]
    fn a_body_round_trips_through_gzip() {
        let dir = tempfile::tempdir().unwrap();
        let path = body_path(dir.path(), "claude", "claude:o1", "c1");
        let body = StoredBody {
            key: "claude:c1".into(),
            site: "claude".into(),
            account: "claude:o1".into(),
            id: "c1".into(),
            title: "Rust".into(),
            updated_at: 5,
            fetched_at: 6,
            messages: vec![super::super::protocol::WebMessage {
                role: "user".into(),
                text: "hello".repeat(1000),
                at: None,
                attachments: vec![],
            }],
        };
        write_body(&path, &body).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[..2], &[0x1f, 0x8b], "gzip magic");
        assert!(raw.len() < 1000, "repetitive text compresses");
        assert_eq!(read_body(&path).unwrap(), body);
        assert_eq!(
            read_body(&dir.path().join("nope.json.gz")).unwrap_err(),
            "E_NOT_FOUND"
        );
    }
}
