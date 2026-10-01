//! 保管库文件：魔数 "SKV1"、格式版本（u16 LE）、文件头长度（u32 LE）、JSON 文件头、正文密文。
//! 文件头整体作为正文的附加认证数据，被改动则正文无法解开。

use super::crypto::{self, KdfParams, SecretKey};
use super::errors::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub(crate) const MAGIC: &[u8; 4] = b"SKV1";
pub(crate) const FORMAT_VERSION: u16 = 1;
const PREFIX_LEN: usize = 10;
const WRAP_AAD: &[u8] = b"stacker-vault-key-wrap";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Wrap {
    pub salt: String,
    pub sealed: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Header {
    pub kdf: KdfParams,
    pub password: Wrap,
    pub recovery: Wrap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub len: u64,
    pub digest: [u8; 32],
}

pub(crate) struct Loaded {
    pub header: Header,
    pub aad: Vec<u8>,
    pub body: Vec<u8>,
    pub snapshot: Snapshot,
}

impl Wrap {
    pub(crate) fn new(secret: &[u8], dek: &[u8; 32], kdf: KdfParams) -> Result<Wrap, String> {
        let salt = crypto::random::<16>();
        let kek = crypto::derive_key(secret, &salt, kdf)?;
        Ok(Wrap {
            salt: crypto::to_hex(&salt),
            sealed: crypto::to_hex(&crypto::seal(&kek, WRAP_AAD, dek)),
        })
    }

    /// Ok(None): this secret does not open the wrap.
    pub(crate) fn unwrap_key(&self, secret: &[u8], kdf: KdfParams) -> Result<Option<SecretKey>, String> {
        let salt = crypto::from_hex(&self.salt).ok_or(CORRUPT)?;
        let sealed = crypto::from_hex(&self.sealed).ok_or(CORRUPT)?;
        let kek = crypto::derive_key(secret, &salt, kdf)?;
        let Some(plain) = crypto::open(&kek, WRAP_AAD, &sealed) else {
            return Ok(None);
        };
        let key: [u8; 32] = plain.as_slice().try_into().map_err(|_| CORRUPT.to_string())?;
        Ok(Some(Zeroizing::new(key)))
    }
}

fn snapshot_of(bytes: &[u8]) -> Snapshot {
    use sha2::{Digest, Sha256};
    Snapshot { len: bytes.len() as u64, digest: Sha256::digest(bytes).into() }
}

pub(crate) fn current_snapshot(path: &Path) -> Option<Snapshot> {
    std::fs::read(path).ok().map(|bytes| snapshot_of(&bytes))
}

pub(crate) fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("skv.bak")
}

pub(crate) fn encode(header: &Header, dek: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let header_json = serde_json::to_vec(header).map_err(|_| IO.to_string())?;
    let mut out = Vec::with_capacity(PREFIX_LEN + header_json.len() + plain.len() + 40);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(header_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&header_json);
    let sealed = crypto::seal(dek, &out, plain);
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Loaded, String> {
    if bytes.len() < PREFIX_LEN || &bytes[..4] != MAGIC {
        return Err(CORRUPT.into());
    }
    match u16::from_le_bytes([bytes[4], bytes[5]]) {
        0 => return Err(CORRUPT.into()),
        version if version > FORMAT_VERSION => return Err(NEWER.into()),
        _ => {}
    }
    let header_len = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]) as usize;
    let header_end = PREFIX_LEN
        .checked_add(header_len)
        .filter(|end| *end <= bytes.len())
        .ok_or(CORRUPT)?;
    let header: Header =
        serde_json::from_slice(&bytes[PREFIX_LEN..header_end]).map_err(|_| CORRUPT.to_string())?;
    if !header.kdf.is_reasonable() {
        return Err(CORRUPT.into());
    }
    Ok(Loaded {
        header,
        aad: bytes[..header_end].to_vec(),
        body: bytes[header_end..].to_vec(),
        snapshot: snapshot_of(bytes),
    })
}

pub(crate) fn read(path: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => MISSING.to_string(),
        _ => IO.to_string(),
    })?;
    parse(&bytes)
}

pub(crate) fn decrypt_body(loaded: &Loaded, dek: &[u8; 32]) -> Result<Zeroizing<Vec<u8>>, String> {
    crypto::open(dek, &loaded.aad, &loaded.body).ok_or_else(|| CORRUPT.to_string())
}

/// Writes a temporary file, flushes it, keeps the current file as `.bak`, then renames over it.
/// `expected` is what was read last; anything else on disk is someone else's change.
pub(crate) fn write(path: &Path, bytes: &[u8], expected: Option<&Snapshot>) -> Result<Snapshot, String> {
    if let Some(expected) = expected {
        if current_snapshot(path).as_ref() != Some(expected) {
            return Err(CHANGED.into());
        }
    }
    let dir = path.parent().ok_or(IO)?;
    std::fs::create_dir_all(dir).map_err(|_| IO.to_string())?;
    let tmp = path.with_extension("skv.tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp).map_err(|_| IO.to_string())?;
        file.write_all(bytes).map_err(|_| IO.to_string())?;
        file.sync_all().map_err(|_| IO.to_string())?;
    }
    if path.exists() {
        std::fs::copy(path, backup_path(path)).map_err(|_| IO.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|_| IO.to_string())?;
    Ok(snapshot_of(bytes))
}

/// A write that changes who can open the vault (new password or recovery key): the backup must
/// hold the new bytes too, or the retired credential would still open the vault through it.
///
/// Any Err leaves the vault file itself untouched: both temporary files are staged first, the
/// backup is replaced next, and the vault file is renamed into place last.
pub(crate) fn write_rekeyed(path: &Path, bytes: &[u8], expected: Option<&Snapshot>) -> Result<Snapshot, String> {
    if let Some(expected) = expected {
        if current_snapshot(path).as_ref() != Some(expected) {
            return Err(CHANGED.into());
        }
    }
    let dir = path.parent().ok_or(IO)?;
    std::fs::create_dir_all(dir).map_err(|_| IO.to_string())?;
    let tmp = path.with_extension("skv.tmp");
    let backup_tmp = path.with_extension("skv.bak.tmp");
    let staged = stage(&tmp, bytes)
        .and_then(|_| stage(&backup_tmp, bytes))
        .and_then(|_| std::fs::rename(&backup_tmp, backup_path(path)).map_err(|_| IO.to_string()))
        .and_then(|_| std::fs::rename(&tmp, path).map_err(|_| IO.to_string()));
    if let Err(error) = staged {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&backup_tmp);
        return Err(error);
    }
    Ok(snapshot_of(bytes))
}

fn stage(tmp: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::File::create(tmp).map_err(|_| IO.to_string())?;
    file.write_all(bytes).map_err(|_| IO.to_string())?;
    file.sync_all().map_err(|_| IO.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::crypto::{self, KdfParams};

    fn header_for(dek: &[u8; 32]) -> Header {
        Header {
            kdf: KdfParams::FAST,
            password: Wrap::new(b"main password!", dek, KdfParams::FAST).unwrap(),
            recovery: Wrap::new(&[9u8; 20], dek, KdfParams::FAST).unwrap(),
        }
    }

    #[test]
    fn wraps_open_only_with_their_secret() {
        let dek = crypto::new_key();
        let header = header_for(&dek);
        assert_eq!(*header.password.unwrap_key(b"main password!", header.kdf).unwrap().unwrap(), *dek);
        assert!(header.password.unwrap_key(b"wrong", header.kdf).unwrap().is_none());
        assert_eq!(*header.recovery.unwrap_key(&[9u8; 20], header.kdf).unwrap().unwrap(), *dek);
    }

    #[test]
    fn encode_parse_decrypt_round_trip() {
        let dek = crypto::new_key();
        let bytes = encode(&header_for(&dek), &dek, b"{\"entries\":[]}").unwrap();
        let loaded = parse(&bytes).unwrap();
        assert_eq!(decrypt_body(&loaded, &dek).unwrap().as_slice(), b"{\"entries\":[]}");
    }

    #[test]
    fn a_touched_header_breaks_the_body() {
        let dek = crypto::new_key();
        let mut bytes = encode(&header_for(&dek), &dek, b"x").unwrap();
        // Change one hex digit of the password wrap's salt: the header still parses.
        let key = b"\"salt\":\"";
        let at = bytes.windows(key.len()).position(|w| w == key).unwrap() + key.len();
        bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
        let loaded = parse(&bytes).expect("a changed salt digit still parses");
        assert!(decrypt_body(&loaded, &dek).is_err());
    }

    #[test]
    fn newer_or_broken_files_are_refused() {
        let dek = crypto::new_key();
        let mut bytes = encode(&header_for(&dek), &dek, b"x").unwrap();
        bytes[4] = 2;
        assert_eq!(parse(&bytes).err().unwrap(), NEWER);
        bytes[4] = 0;
        assert_eq!(parse(&bytes).err().unwrap(), CORRUPT);
        assert_eq!(parse(b"SKV1").err().unwrap(), CORRUPT);
        assert_eq!(parse(b"NOPE000000").err().unwrap(), CORRUPT);
        let mut long = encode(&header_for(&dek), &dek, b"x").unwrap();
        long[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse(&long).err().unwrap(), CORRUPT);
    }

    #[test]
    fn an_unreasonable_kdf_in_the_header_is_refused() {
        let dek = crypto::new_key();
        let mut header = header_for(&dek);
        header.kdf = KdfParams { mem_kib: 4_194_304, iters: 3, lanes: 1 };
        let bytes = encode(&header, &dek, b"x").unwrap();
        assert_eq!(parse(&bytes).err().unwrap(), CORRUPT);
    }

    #[test]
    fn write_keeps_the_previous_file_and_refuses_outside_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.skv");
        let first = write(&path, b"one", None).unwrap();
        assert!(!backup_path(&path).exists());
        let second = write(&path, b"two", Some(&first)).unwrap();
        assert_eq!(std::fs::read(backup_path(&path)).unwrap(), b"one");
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        std::fs::write(&path, b"edited elsewhere").unwrap();
        assert_eq!(write(&path, b"three", Some(&second)).err().unwrap(), CHANGED);
        assert_eq!(std::fs::read(&path).unwrap(), b"edited elsewhere");
    }

    #[test]
    fn a_rekeyed_write_leaves_the_new_bytes_in_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.skv");
        let first = write(&path, b"one", None).unwrap();
        write_rekeyed(&path, b"two", Some(&first)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(std::fs::read(backup_path(&path)).unwrap(), b"two");
        assert!(!path.with_extension("skv.bak.tmp").exists());
    }

    #[test]
    fn a_failed_rekeyed_write_leaves_the_vault_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.skv");
        let first = write(&path, b"one", None).unwrap();
        // A directory in the backup's staging place makes the backup step fail.
        std::fs::create_dir(path.with_extension("skv.bak.tmp")).unwrap();
        assert_eq!(write_rekeyed(&path, b"two", Some(&first)).err().unwrap(), IO);
        assert_eq!(current_snapshot(&path), Some(first));
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        assert!(!path.with_extension("skv.tmp").exists());
    }

    #[test]
    fn reading_a_missing_file_says_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&dir.path().join("none.skv")).err().unwrap(), MISSING);
    }
}
