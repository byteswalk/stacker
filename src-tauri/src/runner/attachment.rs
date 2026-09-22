//! Images and PDF documents sent along with a prompt, kept as base64 the way both APIs carry them.
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Pdf,
}

#[derive(Clone, PartialEq)]
pub struct Attachment {
    pub kind: AttachmentKind,
    /// `image/png`, `image/jpeg`, `image/gif`, `image/webp` or `application/pdf`.
    pub media_type: String,
    /// Standard base64 without line breaks.
    pub data: String,
    pub name: Option<String>,
}

// Never prints the data: it is large and may be private.
impl fmt::Debug for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachment")
            .field("kind", &self.kind)
            .field("media_type", &self.media_type)
            .field("bytes", &(self.data.len() / 4 * 3))
            .field("name", &self.name)
            .finish()
    }
}

impl Attachment {
    /// File extension for the media type, used when a CLI takes the attachment as a file.
    pub fn extension(&self) -> &'static str {
        match self.media_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "application/pdf" => "pdf",
            _ => "bin",
        }
    }

    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        decode_base64(&self.data).ok_or_else(|| "E_ATTACHMENT_UNSUPPORTED".into())
    }
}

/// The image type named by the file's first bytes.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard or URL-safe base64; whitespace is skipped and padding is optional.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut padding = false;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                padding = true;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if padding {
            return None;
        }
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // A lone leftover character cannot encode a byte.
    (bits < 6).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for text in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let encoded = encode_base64(text.as_bytes());
            assert_eq!(decode_base64(&encoded).unwrap(), text.as_bytes());
        }
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(decode_base64("Zm8").unwrap(), b"fo");
        assert_eq!(decode_base64("Zm9v\r\nYmFy").unwrap(), b"foobar");
        assert!(decode_base64("Zm9v!").is_none());
        assert!(decode_base64("Z").is_none());
    }

    #[test]
    fn images_are_sniffed_and_debug_hides_data() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"%PDF-1.7"), None);
        let a = Attachment {
            kind: AttachmentKind::Image,
            media_type: "image/jpeg".into(),
            data: "c2VjcmV0".into(),
            name: None,
        };
        assert_eq!(a.extension(), "jpg");
        assert!(!format!("{a:?}").contains("c2VjcmV0"));
    }
}
