//! SSH 私钥：识别算法、是否加密、指纹与公钥，标出弱密钥；导出时把权限收紧为仅当前用户。

use super::errors::{FILE_EXISTS, IO};
use serde::Serialize;
use std::path::Path;

pub(crate) const RISK_UNENCRYPTED: &str = "unencrypted";
pub(crate) const RISK_DSA: &str = "dsa";
pub(crate) const RISK_RSA_SHORT: &str = "rsa_short";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SshInfo {
    pub algorithm: String,
    pub bits: Option<u32>,
    pub encrypted: bool,
    pub fingerprint: Option<String>,
    pub public_key: Option<String>,
    pub risks: Vec<&'static str>,
}

pub(crate) fn inspect(text: &str) -> Option<SshInfo> {
    let text = text.trim();
    if text.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
        inspect_openssh(text)
    } else {
        inspect_pem(text)
    }
}

fn inspect_openssh(text: &str) -> Option<SshInfo> {
    let key = ssh_key::PrivateKey::from_openssh(text).ok()?;
    let public = key.public_key();
    let bits = public
        .key_data()
        .rsa()
        .and_then(|rsa| rsa.n.as_positive_bytes())
        .map(bit_length);
    Some(finish(
        key.algorithm().as_str().to_string(),
        bits,
        key.is_encrypted(),
        Some(public.fingerprint(ssh_key::HashAlg::Sha256).to_string()),
        public.to_openssh().ok(),
    ))
}

fn inspect_pem(text: &str) -> Option<SshInfo> {
    let header = text.lines().next()?.trim();
    let (algorithm, rsa) = match header {
        "-----BEGIN RSA PRIVATE KEY-----" => ("ssh-rsa", true),
        "-----BEGIN DSA PRIVATE KEY-----" => ("ssh-dss", false),
        "-----BEGIN EC PRIVATE KEY-----" => ("ecdsa", false),
        "-----BEGIN PRIVATE KEY-----" | "-----BEGIN ENCRYPTED PRIVATE KEY-----" => ("pkcs8", false),
        _ => return None,
    };
    let encrypted = header.contains("ENCRYPTED") || text.contains("Proc-Type: 4,ENCRYPTED");
    let bits = if rsa && !encrypted { pkcs1_modulus_bits(text) } else { None };
    Some(finish(algorithm.to_string(), bits, encrypted, None, None))
}

fn finish(
    algorithm: String,
    bits: Option<u32>,
    encrypted: bool,
    fingerprint: Option<String>,
    public_key: Option<String>,
) -> SshInfo {
    let mut risks = Vec::new();
    if !encrypted {
        risks.push(RISK_UNENCRYPTED);
    }
    if algorithm == "ssh-dss" {
        risks.push(RISK_DSA);
    }
    if algorithm == "ssh-rsa" && bits.is_some_and(|bits| bits < 2048) {
        risks.push(RISK_RSA_SHORT);
    }
    SshInfo { algorithm, bits, encrypted, fingerprint, public_key, risks }
}

fn bit_length(bytes: &[u8]) -> u32 {
    let bytes = match bytes.iter().position(|b| *b != 0) {
        Some(start) => &bytes[start..],
        None => return 0,
    };
    (bytes.len() as u32 - 1) * 8 + (8 - bytes[0].leading_zeros())
}

/// One DER item: (tag, content, rest).
fn der_item(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, mut rest) = rest.split_first()?;
    let len = if first & 0x80 == 0 {
        first as usize
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (len_bytes, tail) = rest.split_at(count);
        rest = tail;
        len_bytes.iter().fold(0usize, |acc, b| (acc << 8) | *b as usize)
    };
    if rest.len() < len {
        return None;
    }
    let (content, tail) = rest.split_at(len);
    Some((tag, content, tail))
}

/// RSAPrivateKey ::= SEQUENCE { version INTEGER, modulus INTEGER, ... }
fn pkcs1_modulus_bits(text: &str) -> Option<u32> {
    use base64ct::{Base64, Encoding};
    let encoded: String = text
        .lines()
        .filter(|line| !line.starts_with("-----") && !line.contains(':'))
        .map(str::trim)
        .collect();
    let der = Base64::decode_vec(&encoded).ok()?;
    let (tag, sequence, _) = der_item(&der)?;
    if tag != 0x30 {
        return None;
    }
    let (tag, _, after_version) = der_item(sequence)?;
    if tag != 0x02 {
        return None;
    }
    let (tag, modulus, _) = der_item(after_version)?;
    (tag == 0x02).then(|| bit_length(modulus))
}

/// Never overwrites; Windows OpenSSH refuses a private key others can read, so access is
/// narrowed to the current user right after writing.
pub(crate) fn export_private(text: &str, dest: &Path) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => FILE_EXISTS.to_string(),
            _ => IO.to_string(),
        })?;
    let mut content = zeroize::Zeroizing::new(text.replace("\r\n", "\n"));
    if !content.ends_with('\n') {
        content.push('\n');
    }
    file.write_all(content.as_bytes()).map_err(|_| IO.to_string())?;
    drop(file);
    restrict_to_current_user(dest)
}

#[cfg(windows)]
fn restrict_to_current_user(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let user = std::env::var("USERNAME").map_err(|_| IO.to_string())?;
    let account = match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    };
    let status = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("{account}:F"))
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| IO.to_string())?;
    if status.success() { Ok(()) } else { Err(IO.into()) }
}

#[cfg(not(windows))]
fn restrict_to_current_user(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|_| IO.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Generates a throwaway key with the system ssh-keygen; None when it is not installed.
    fn keygen(dir: &Path, name: &str, args: &[&str]) -> Option<String> {
        let path = dir.join(name);
        let status = std::process::Command::new("ssh-keygen")
            .args(["-q", "-C", "fixture", "-f"])
            .arg(&path)
            .args(args)
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        std::fs::read_to_string(&path).ok()
    }

    #[test]
    fn openssh_keys_report_fingerprint_public_key_and_risks() {
        let dir = tempfile::tempdir().unwrap();
        let Some(plain) = keygen(dir.path(), "plain", &["-t", "ed25519", "-N", ""]) else {
            eprintln!("ssh-keygen not found; skipped");
            return;
        };
        let info = inspect(&plain).unwrap();
        assert_eq!(info.algorithm, "ssh-ed25519");
        assert!(!info.encrypted);
        assert_eq!(info.risks, vec![RISK_UNENCRYPTED]);
        assert!(info.fingerprint.unwrap().starts_with("SHA256:"));
        assert!(info.public_key.unwrap().starts_with("ssh-ed25519 "));

        let locked = keygen(dir.path(), "locked", &["-t", "ed25519", "-N", "fixture-pass"]).unwrap();
        let info = inspect(&locked).unwrap();
        assert!(info.encrypted);
        assert!(info.risks.is_empty());
        assert!(info.public_key.is_some(), "OpenSSH files carry the public key in the clear");
    }

    #[test]
    fn short_rsa_is_flagged_in_both_formats() {
        let dir = tempfile::tempdir().unwrap();
        let Some(openssh) = keygen(dir.path(), "rsa", &["-t", "rsa", "-b", "1024", "-N", ""]) else {
            eprintln!("ssh-keygen not found; skipped");
            return;
        };
        let info = inspect(&openssh).unwrap();
        assert_eq!(info.bits, Some(1024));
        assert!(info.risks.contains(&RISK_RSA_SHORT));

        let pem = keygen(dir.path(), "rsa_pem", &["-t", "rsa", "-b", "1024", "-m", "PEM", "-N", ""]).unwrap();
        let info = inspect(&pem).unwrap();
        assert_eq!(info.algorithm, "ssh-rsa");
        assert_eq!(info.bits, Some(1024));
        assert!(info.fingerprint.is_none());
        assert!(info.risks.contains(&RISK_RSA_SHORT) && info.risks.contains(&RISK_UNENCRYPTED));
    }

    #[test]
    fn pem_headers_are_recognised_without_parsing_the_body() {
        let dsa = "-----BEGIN DSA PRIVATE KEY-----\nAAAA\n-----END DSA PRIVATE KEY-----\n";
        let info = inspect(dsa).unwrap();
        assert!(info.risks.contains(&RISK_DSA));
        let encrypted = "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n";
        assert!(inspect(encrypted).unwrap().encrypted);
        assert!(inspect("just some text").is_none());
    }

    #[test]
    fn export_writes_once_with_unix_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("id_restored");
        export_private("line one\r\nline two", &dest).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "line one\nline two\n");
        assert_eq!(export_private("again", &dest).err().unwrap(), FILE_EXISTS);
    }
}
