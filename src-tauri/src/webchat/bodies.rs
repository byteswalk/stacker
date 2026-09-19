//! Conversation bodies as gzip JSON: `<root>/webchat/bodies/<site>/<account>/<id>.json.gz`.
use super::protocol::StoredBody;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// One safe path component: ASCII letters, digits, `.`, `_`, `-`; never empty, `.`, `..` or a device name.
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
    if trimmed.is_empty() {
        return "_".into();
    }
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
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
        assert_eq!(component(".."), "_");
        assert_eq!(component(""), "_");
        assert_eq!(component("."), "_");
        assert_eq!(component("a/b\\c:d"), "a_b_c_d");
        assert_eq!(component("CON"), "_CON");
        assert_eq!(component("con.txt"), "_con.txt");
        assert_eq!(component("abc-DEF_1.2"), "abc-DEF_1.2");
        assert_eq!(component(&"x".repeat(500)).len(), 120);
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
