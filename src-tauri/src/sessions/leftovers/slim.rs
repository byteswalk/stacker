//! Takes the screenshots out of a finished session's transcript and keeps everything else.
//!
//! Each embedded image (Codex's `data:image/<type>;base64,…`, Claude's `"media_type":
//! "image/<type>","data":"…"`) becomes a 1×1 picture of the same type, so every line is still
//! the same JSON, the session still opens and resumes, and a resumed model is sent less, not
//! more. Only the base64 text changes; nothing is parsed and written back, so key order,
//! spacing and every other byte stay as the client wrote them.

use regex::bytes::{Captures, Regex};
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

/// Shorter images are left alone: icons cost nothing and a short run may not be an image.
const MIN_IMAGE: usize = 2048;

/// The smallest valid picture of each type, as the client would have stored it.
fn tiny(kind: &[u8]) -> Option<&'static str> {
    Some(match kind.to_ascii_lowercase().as_slice() {
        b"png" => "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
        b"jpeg" | b"jpg" => "/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=",
        b"gif" => "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7",
        b"webp" => "UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==",
        _ => return None,
    })
}

fn data_url() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"data:image/([A-Za-z0-9.+-]{1,16});base64,([A-Za-z0-9+/=]{64,})").unwrap()
    })
}

fn media_then_data() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""media_type":"image/([A-Za-z0-9.+-]{1,16})","data":"([A-Za-z0-9+/=]{64,})""#)
            .unwrap()
    })
}

fn data_then_media() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""data":"([A-Za-z0-9+/=]{64,})","media_type":"image/([A-Za-z0-9.+-]{1,16})""#)
            .unwrap()
    })
}

/// What slimming would save in one line: bytes and images.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saving {
    pub bytes: u64,
    pub images: u64,
}

impl Saving {
    fn add(&mut self, other: Saving) {
        self.bytes += other.bytes;
        self.images += other.images;
    }
}

/// One line with its images replaced, and what that saved; `None` when nothing changes.
pub fn slim_line(line: &[u8]) -> Option<(Vec<u8>, Saving)> {
    if !(memchr(line, b"base64,") || memchr(line, b"\"media_type\":\"image/")) {
        return None;
    }
    let mut saving = Saving::default();
    let mut out = data_url()
        .replace_all(line, |c: &Captures| {
            let (kind, run) = (&c[1], &c[2]);
            match tiny(kind).filter(|_| run.len() >= MIN_IMAGE) {
                Some(small) => {
                    saving.add(Saving {
                        bytes: (run.len() - small.len()) as u64,
                        images: 1,
                    });
                    let mut kept = b"data:image/".to_vec();
                    kept.extend_from_slice(kind);
                    kept.extend_from_slice(b";base64,");
                    kept.extend_from_slice(small.as_bytes());
                    kept
                }
                None => c[0].to_vec(),
            }
        })
        .into_owned();
    out = media_then_data()
        .replace_all(&out, |c: &Captures| {
            let (kind, run) = (&c[1], &c[2]);
            match tiny(kind).filter(|_| run.len() >= MIN_IMAGE) {
                Some(small) => {
                    saving.add(Saving {
                        bytes: (run.len() - small.len()) as u64,
                        images: 1,
                    });
                    let mut kept = br#""media_type":"image/"#.to_vec();
                    kept.extend_from_slice(kind);
                    kept.extend_from_slice(br#"","data":""#);
                    kept.extend_from_slice(small.as_bytes());
                    kept.push(b'"');
                    kept
                }
                None => c[0].to_vec(),
            }
        })
        .into_owned();
    out = data_then_media()
        .replace_all(&out, |c: &Captures| {
            let (run, kind) = (&c[1], &c[2]);
            match tiny(kind).filter(|_| run.len() >= MIN_IMAGE) {
                Some(small) => {
                    saving.add(Saving {
                        bytes: (run.len() - small.len()) as u64,
                        images: 1,
                    });
                    let mut kept = br#""data":""#.to_vec();
                    kept.extend_from_slice(small.as_bytes());
                    kept.extend_from_slice(br#"","media_type":"image/"#);
                    kept.extend_from_slice(kind);
                    kept.push(b'"');
                    kept
                }
                None => c[0].to_vec(),
            }
        })
        .into_owned();
    if saving.images == 0 {
        return None;
    }
    // A line the client wrote as JSON stays JSON; one that was not (a torn last line) is
    // left exactly as it was.
    let body = trim_line_end(line);
    if serde_json::from_slice::<serde::de::IgnoredAny>(body).is_err()
        || serde_json::from_slice::<serde::de::IgnoredAny>(trim_line_end(&out)).is_err()
    {
        return None;
    }
    Some((out, saving))
}

fn trim_line_end(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &line[..end]
}

fn memchr(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// What slimming one transcript would save, without changing it.
pub fn measure(path: &Path) -> std::io::Result<Saving> {
    let mut reader = BufReader::with_capacity(1 << 20, fs::File::open(path)?);
    let mut line = Vec::new();
    let mut total = Saving::default();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if let Some((_, saving)) = slim_line(&line) {
            total.add(saving);
        }
    }
    Ok(total)
}

fn stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Rewrites one transcript with its images replaced. The new file is written beside the old
/// one and swapped in only when the old one did not change meanwhile; it keeps the old write
/// time, so the session does not jump to the top of the client's list.
pub fn slim_file(path: &Path) -> Result<Saving, String> {
    let before = stamp(path).ok_or("E_PATH")?;
    let temp = PathBuf::from(format!("{}.stacker-slim", path.display()));
    let result = (|| -> Result<Saving, String> {
        let mut reader = BufReader::with_capacity(
            1 << 20,
            fs::File::open(path).map_err(|_| "E_IN_USE".to_string())?,
        );
        let mut writer = BufWriter::with_capacity(
            1 << 20,
            fs::File::create(&temp).map_err(|e| format!("E_DELETE: {e}"))?,
        );
        let mut total = Saving::default();
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader
                .read_until(b'\n', &mut line)
                .map_err(crate::sessions::err)?
                == 0
            {
                break;
            }
            match slim_line(&line) {
                Some((slimmed, saving)) => {
                    total.add(saving);
                    writer.write_all(&slimmed)
                }
                None => writer.write_all(&line),
            }
            .map_err(crate::sessions::err)?;
        }
        let file = writer.into_inner().map_err(crate::sessions::err)?;
        file.sync_all().map_err(crate::sessions::err)?;
        let _ = file.set_modified(before.1);
        drop(file);
        if total.images == 0 {
            return Ok(total);
        }
        // Written to while it was being read: the client is using it, try another time.
        if stamp(path) != Some(before) {
            return Err("E_IN_USE".into());
        }
        fs::rename(&temp, path).map_err(|_| "E_IN_USE".to_string())?;
        Ok(total)
    })();
    let _ = fs::remove_file(&temp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(n: usize) -> String {
        "QUJD".repeat(n / 4)
    }

    #[test]
    fn a_codex_screenshot_becomes_a_one_pixel_png() {
        let line = format!(
            r#"{{"type":"response_item","payload":{{"output":[{{"type":"input_image","image_url":"data:image/png;base64,{}","detail":"high"}}]}}}}"#,
            run(8000)
        );
        let (out, saving) = slim_line(line.as_bytes()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(&format!("data:image/png;base64,{}", tiny(b"png").unwrap())));
        assert!(text.ends_with(r#""detail":"high"}]}}"#));
        assert_eq!(saving.images, 1);
        assert_eq!(saving.bytes, (8000 - tiny(b"png").unwrap().len()) as u64);
        serde_json::from_str::<serde_json::Value>(&text).unwrap();
    }

    #[test]
    fn a_claude_screenshot_keeps_its_type_and_shape() {
        let line = format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","content":[{{"type":"image","source":{{"type":"base64","media_type":"image/jpeg","data":"{}"}}}}]}}]}}}}"#,
            run(4000)
        );
        let (out, _) = slim_line(line.as_bytes()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let source = &value["message"]["content"][0]["content"][0]["source"];
        assert_eq!(source["media_type"], "image/jpeg");
        assert_eq!(source["data"], tiny(b"jpeg").unwrap());
    }

    #[test]
    fn small_images_other_files_and_torn_lines_are_left_alone() {
        let icon = format!(r#"{{"u":"data:image/png;base64,{}"}}"#, run(400));
        assert!(slim_line(icon.as_bytes()).is_none());
        let pdf = format!(
            r#"{{"source":{{"type":"base64","media_type":"application/pdf","data":"{}"}}}}"#,
            run(8000)
        );
        assert!(slim_line(pdf.as_bytes()).is_none());
        let unknown = format!(r#"{{"u":"data:image/tiff;base64,{}"}}"#, run(8000));
        assert!(slim_line(unknown.as_bytes()).is_none());
        let torn = format!(r#"{{"u":"data:image/png;base64,{}"#, run(8000));
        assert!(slim_line(torn.as_bytes()).is_none());
    }

    #[test]
    fn a_transcript_is_rewritten_in_place_with_its_time_and_other_lines_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let image = format!(r#"{{"u":"data:image/png;base64,{}"}}"#, run(8000));
        let body = format!("{{\"a\":1}}\r\n{image}\n{{\"b\":\"text\"}}\n{{\"torn\":");
        fs::write(&path, &body).unwrap();
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(measure(&path).unwrap().images, 1);

        let saving = slim_file(&path).unwrap();
        assert_eq!(saving.images, 1);
        let after = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = after.split('\n').collect();
        assert_eq!(lines.len(), body.split('\n').count());
        assert_eq!(lines[0], "{\"a\":1}\r");
        assert_eq!(lines[2], "{\"b\":\"text\"}");
        assert_eq!(lines[3], "{\"torn\":");
        assert!(after.len() < body.len());
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), old);
        assert!(!dir.path().join("rollout.jsonl.stacker-slim").exists());
        assert_eq!(measure(&path).unwrap(), Saving::default());
    }
}
