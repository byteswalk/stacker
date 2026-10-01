//! 纯加密原语：Argon2id 派生密钥、XChaCha20-Poly1305 封装、恢复密钥编解码。不含 IO。

use super::errors::CORRUPT;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

pub(crate) type SecretKey = Zeroizing<[u8; 32]>;

const NONCE_LEN: usize = 24;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KdfParams {
    pub mem_kib: u32,
    pub iters: u32,
    pub lanes: u32,
}

impl KdfParams {
    pub(crate) const STANDARD: KdfParams = KdfParams {
        mem_kib: 64 * 1024,
        iters: 3,
        lanes: 1,
    };

    /// Bounds for parameters read from a file, so a tampered header cannot force a huge allocation.
    pub(crate) fn is_reasonable(&self) -> bool {
        (8..=1_048_576).contains(&self.mem_kib)
            && (1..=10).contains(&self.iters)
            && (1..=8).contains(&self.lanes)
            && self.mem_kib >= 8 * self.lanes
    }
    /// Cheap parameters so unit tests stay fast; never used outside tests.
    #[cfg(test)]
    pub(crate) const FAST: KdfParams = KdfParams {
        mem_kib: 256,
        iters: 1,
        lanes: 1,
    };
}

pub(crate) fn random<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::getrandom(&mut out).expect("the system random source is available");
    out
}

pub(crate) fn new_key() -> SecretKey {
    Zeroizing::new(random::<32>())
}

pub(crate) fn derive_key(
    secret: &[u8],
    salt: &[u8],
    params: KdfParams,
) -> Result<SecretKey, String> {
    let params = Params::new(params.mem_kib, params.iters, params.lanes, Some(32))
        .map_err(|_| CORRUPT.to_string())?;
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(secret, salt, out.as_mut())
        .map_err(|_| CORRUPT.to_string())?;
    Ok(out)
}

/// Returns nonce ‖ ciphertext; the AAD is authenticated but not stored.
pub(crate) fn seal(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let nonce = random::<NONCE_LEN>();
    let body = XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
        .expect("encrypting in memory cannot fail");
    let mut out = nonce.to_vec();
    out.extend(body);
    out
}

/// None when the key, the AAD or the bytes are wrong.
pub(crate) fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE_LEN {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(XNonce::from_slice(nonce), Payload { msg: body, aad })
        .ok()
        .map(Zeroizing::new)
}

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The fingerprint used to recognise a secret again: line endings and outer space don't count.
pub(crate) fn secret_digest(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let normalized = Zeroizing::new(value.replace("\r\n", "\n"));
    to_hex(&Sha256::digest(normalized.trim().as_bytes()))
}

pub(crate) fn new_recovery_key() -> Zeroizing<String> {
    let mut bytes = random::<20>();
    let key = encode_recovery(&bytes);
    bytes.zeroize();
    key
}

/// 160 bits → 32 Crockford Base32 characters in 8 groups of 4.
pub(crate) fn encode_recovery(bytes: &[u8; 20]) -> Zeroizing<String> {
    let mut chars = Zeroizing::new(String::with_capacity(32));
    let mut buffer: u64 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            chars.push(CROCKFORD[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1u64 << bits) - 1;
    }
    let groups: Vec<&str> = (0..8).map(|i| &chars[i * 4..i * 4 + 4]).collect();
    Zeroizing::new(groups.join("-"))
}

/// Accepts any case, spaces and dashes; reads I/L as 1 and O as 0. Anything else fails.
pub(crate) fn decode_recovery(input: &str) -> Option<Zeroizing<[u8; 20]>> {
    let mut out = Zeroizing::new([0u8; 20]);
    let mut buffer: u64 = 0;
    let mut bits = 0;
    let mut index = 0;
    let mut count = 0;
    for ch in input.chars() {
        if ch == '-' || ch.is_whitespace() {
            continue;
        }
        let value = match ch.to_ascii_uppercase() {
            'O' => 0,
            'I' | 'L' => 1,
            upper => CROCKFORD.iter().position(|&c| c as char == upper)? as u64,
        };
        count += 1;
        if count > 32 {
            return None;
        }
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out[index] = (buffer >> bits) as u8;
            index += 1;
            buffer &= (1u64 << bits) - 1;
        }
    }
    (count == 32).then_some(out)
}

pub(crate) fn last_group(key: &str) -> &str {
    key.rsplit('-').next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kdf_bounds_are_enforced() {
        let p = |mem_kib, iters, lanes| {
            KdfParams {
                mem_kib,
                iters,
                lanes,
            }
            .is_reasonable()
        };
        assert!(KdfParams::STANDARD.is_reasonable());
        assert!(KdfParams::FAST.is_reasonable());
        // (mem_kib, iters, lanes, accepted)
        let table = [
            (8, 1, 1, true),
            (7, 1, 1, false),
            (0, 1, 1, false),
            (1_048_576, 1, 1, true),
            (1_048_577, 1, 1, false),
            (256, 1, 1, true),
            (256, 0, 1, false),
            (256, 10, 1, true),
            (256, 11, 1, false),
            (256, 1, 0, false),
            (256, 1, 8, true),
            (256, 1, 9, false),
            (8, 1, 8, false),
            (63, 1, 8, false),
            (64, 1, 8, true),
        ];
        for (mem_kib, iters, lanes, accepted) in table {
            assert_eq!(
                p(mem_kib, iters, lanes),
                accepted,
                "mem={mem_kib} iters={iters} lanes={lanes}"
            );
        }
    }

    #[test]
    fn derive_is_deterministic_and_salt_sensitive() {
        let a = derive_key(b"correct horse", &[1u8; 16], KdfParams::FAST).unwrap();
        let b = derive_key(b"correct horse", &[1u8; 16], KdfParams::FAST).unwrap();
        let c = derive_key(b"correct horse", &[2u8; 16], KdfParams::FAST).unwrap();
        assert_eq!(*a, *b);
        assert_ne!(*a, *c);
    }

    #[test]
    fn seal_round_trips_and_rejects_tampering() {
        let key = new_key();
        let sealed = seal(&key, b"aad", b"secret text");
        assert_eq!(
            open(&key, b"aad", &sealed).unwrap().as_slice(),
            b"secret text"
        );
        assert!(open(&key, b"other aad", &sealed).is_none());
        let mut flipped = sealed.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert!(open(&key, b"aad", &flipped).is_none());
        assert!(open(&new_key(), b"aad", &sealed).is_none());
        assert!(open(&key, b"aad", &sealed[..10]).is_none());
        assert_ne!(
            seal(&key, b"aad", b"x"),
            seal(&key, b"aad", b"x"),
            "fresh nonce each time"
        );
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(to_hex(&[0, 15, 255]), "000fff");
        assert_eq!(from_hex("000fff").unwrap(), vec![0, 15, 255]);
        assert!(from_hex("abc").is_none());
        assert!(from_hex("zz").is_none());
    }

    #[test]
    fn digest_ignores_line_endings_and_outer_space() {
        assert_eq!(secret_digest("a\r\nb\r\n"), secret_digest("  a\nb"));
        assert_ne!(secret_digest("a"), secret_digest("b"));
    }

    #[test]
    fn recovery_key_shape_and_round_trip() {
        let key = new_recovery_key();
        let groups: Vec<&str> = key.split('-').collect();
        assert_eq!(groups.len(), 8);
        assert!(groups.iter().all(|g| g.len() == 4));
        assert!(!key.contains(['I', 'L', 'O', 'U']));
        let bytes = [7u8; 20];
        let encoded = encode_recovery(&bytes);
        assert_eq!(*decode_recovery(&encoded).unwrap(), bytes);
    }

    #[test]
    fn recovery_encoding_matches_fixed_vectors() {
        assert_eq!(
            encode_recovery(&[0u8; 20]).as_str(),
            "0000-0000-0000-0000-0000-0000-0000-0000"
        );
        // Bytes 1..=20, worked out independently of this code from the Crockford alphabet.
        let counting: [u8; 20] = std::array::from_fn(|i| i as u8 + 1);
        let expected = "0410-6105-0R3G-G28A-1C60-T3GF-208H-44RM";
        assert_eq!(encode_recovery(&counting).as_str(), expected);
        assert_eq!(*decode_recovery(expected).unwrap(), counting);
    }

    #[test]
    fn recovery_decoding_is_forgiving_about_format_only() {
        let bytes = [0x5au8; 20];
        let encoded = encode_recovery(&bytes);
        let loose = encoded.to_ascii_lowercase().replace('-', " ");
        assert_eq!(*decode_recovery(&loose).unwrap(), bytes);
        let squashed = encoded.replace('-', "");
        assert_eq!(*decode_recovery(&squashed).unwrap(), bytes);
        // I/L read as 1 and O as 0, the way Crockford intends.
        assert_eq!(
            decode_recovery(&"O".repeat(32)).unwrap().as_slice(),
            &[0u8; 20]
        );
        assert!(decode_recovery(&"U".repeat(32)).is_none());
        assert!(decode_recovery(&encoded[..30]).is_none());
        assert!(decode_recovery(&format!("{}0", encoded.as_str())).is_none());
    }

    #[test]
    fn last_group_is_the_final_four() {
        assert_eq!(last_group("AAAA-BBBB-CCCC"), "CCCC");
    }
}
