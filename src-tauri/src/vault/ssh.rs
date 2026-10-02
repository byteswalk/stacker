//! SSH 私钥：识别算法、是否加密、指纹与公钥，标出弱密钥；导出时把权限收紧为仅当前用户。

use super::errors::{FILE_EXISTS, HOST, HOST_EXISTS, IO, KEY_FORMAT, NAME, PASSPHRASE};
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::Zeroizing;

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
    let bits = if rsa && !encrypted {
        pkcs1_modulus_bits(text)
    } else {
        None
    };
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
    SshInfo {
        algorithm,
        bits,
        encrypted,
        fingerprint,
        public_key,
        risks,
    }
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
        len_bytes
            .iter()
            .fold(0usize, |acc, b| (acc << 8) | *b as usize)
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

/// A new key pair in OpenSSH form: the private key as it goes into the vault, the public
/// key as a server's `authorized_keys` takes it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyPair {
    pub private_key: String,
    pub public_key: String,
}

/// `ed25519` (the default everywhere it is supported) or `rsa` at 4096 bits for servers
/// too old for it. A non-empty passphrase encrypts the private key the OpenSSH way.
pub(crate) fn generate(
    algorithm: &str,
    comment: &str,
    passphrase: &str,
) -> Result<KeyPair, String> {
    use ssh_key::private::{KeypairData, RsaKeypair};
    use ssh_key::rand_core::OsRng;
    use ssh_key::{Algorithm, LineEnding, PrivateKey};

    // The comment ends up inside a quoted shell command, so only plain characters stay.
    let comment: String = comment
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "@._-+ ".contains(*c))
        .take(120)
        .collect();
    let mut key = match algorithm {
        "ed25519" => PrivateKey::random(&mut OsRng, Algorithm::Ed25519),
        "rsa" => RsaKeypair::random(&mut OsRng, 4096)
            .and_then(|pair| PrivateKey::new(KeypairData::from(pair), "")),
        _ => return Err(IO.into()),
    }
    .map_err(|_| IO.to_string())?;
    key.set_comment(comment);
    let public_key = key.public_key().to_openssh().map_err(|_| IO.to_string())?;
    if !passphrase.is_empty() {
        key = key
            .encrypt(&mut OsRng, passphrase)
            .map_err(|_| IO.to_string())?;
    }
    let private_key: Zeroizing<String> =
        key.to_openssh(LineEnding::LF).map_err(|_| IO.to_string())?;
    Ok(KeyPair {
        private_key: private_key.to_string(),
        public_key,
    })
}

/// The same key under another passphrase: `old` opens it when it is encrypted, an empty
/// `new` leaves it unencrypted. Only keys in OpenSSH's own format can be rewritten.
pub(crate) fn set_passphrase(
    text: &str,
    old: &str,
    new: &str,
) -> Result<Zeroizing<String>, String> {
    use ssh_key::rand_core::OsRng;
    use ssh_key::{LineEnding, PrivateKey};

    let key = PrivateKey::from_openssh(text.trim()).map_err(|_| KEY_FORMAT.to_string())?;
    let key = if key.is_encrypted() {
        key.decrypt(old).map_err(|_| PASSPHRASE.to_string())?
    } else {
        key
    };
    let key = if new.is_empty() {
        key
    } else {
        key.encrypt(&mut OsRng, new).map_err(|_| IO.to_string())?
    };
    key.to_openssh(LineEnding::LF).map_err(|_| IO.to_string())
}

/// The `Host` block to add to `~/.ssh/config`, so `ssh <alias>` uses the key.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalHost {
    pub alias: String,
    pub host: String,
    pub user: String,
    pub port: Option<u16>,
}

fn plain(text: &str, extra: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// A file name of the key's own: no path, no dot file, none of the names ssh itself keeps.
fn key_file_name(name: &str) -> bool {
    plain(name, "._-")
        && !name.starts_with('.')
        && !name.ends_with(".pub")
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "config" | "known_hosts" | "authorized_keys"
        )
}

fn has_host(config: &str, alias: &str) -> bool {
    config.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next().is_some_and(|w| w.eq_ignore_ascii_case("host"))
            && words.any(|w| w.eq_ignore_ascii_case(alias))
    })
}

/// Puts a vault key where ssh finds it: the private key as `<dir>/<name>` (never over an
/// existing file, readable by the current user only), the public key beside it, and, when
/// asked, a `Host` block appended to `<dir>/config`. Everything that can be refused is
/// checked before the first file is written; `backup` sees the config before it changes.
pub(crate) fn install_local(
    dir: &Path,
    name: &str,
    private: &str,
    public: Option<&str>,
    host: Option<&LocalHost>,
    backup: impl FnOnce(&Path),
) -> Result<String, String> {
    if !key_file_name(name) {
        return Err(NAME.into());
    }
    let config_path = dir.join("config");
    let config = std::fs::read_to_string(&config_path).unwrap_or_default();
    if let Some(host) = host {
        if !plain(&host.alias, "._-") || !plain(&host.host, ".:_-") || !plain(&host.user, "._-") {
            return Err(HOST.into());
        }
        if has_host(&config, &host.alias) {
            return Err(HOST_EXISTS.into());
        }
    }
    std::fs::create_dir_all(dir).map_err(|_| IO.to_string())?;
    let dest = dir.join(name);
    export_private(private, &dest)?;
    if let Some(public) = public.map(str::trim).filter(|text| !text.is_empty()) {
        let beside = dir.join(format!("{name}.pub"));
        if !beside.exists() {
            let _ = std::fs::write(beside, format!("{public}\n"));
        }
    }
    if let Some(host) = host {
        backup(&config_path);
        let mut next = config;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        if !next.is_empty() {
            next.push('\n');
        }
        next.push_str(&format!(
            "Host {}\n    HostName {}\n    User {}\n",
            host.alias, host.host, host.user
        ));
        if let Some(port) = host.port.filter(|port| *port != 22) {
            next.push_str(&format!("    Port {port}\n"));
        }
        next.push_str(&format!(
            "    IdentityFile ~/.ssh/{name}\n    IdentitiesOnly yes\n"
        ));
        std::fs::write(&config_path, next).map_err(|_| IO.to_string())?;
    }
    Ok(dest.display().to_string())
}

/// Never overwrites. OpenSSH refuses a private key others can read, so the empty file is
/// locked down to the current user before any secret goes into it; if anything fails after
/// the file was created, it is removed again so a retry starts clean.
pub(crate) fn export_private(text: &str, dest: &Path) -> Result<(), String> {
    export_with(text, dest, restrict_to_current_user)
}

fn export_with(
    text: &str,
    dest: &Path,
    restrict: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(dest).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => FILE_EXISTS.to_string(),
        _ => IO.to_string(),
    })?;
    let written = restrict(dest).and_then(|()| {
        let mut content = zeroize::Zeroizing::new(text.replace("\r\n", "\n"));
        if !content.ends_with('\n') {
            content.push('\n');
        }
        file.write_all(content.as_bytes())
            .map_err(|_| IO.to_string())
    });
    drop(file);
    if written.is_err() {
        // We created it just now, so removing it cannot touch anyone else's file.
        let _ = std::fs::remove_file(dest);
        return Err(IO.to_string());
    }
    Ok(())
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
    if status.success() {
        Ok(())
    } else {
        Err(IO.into())
    }
}

#[cfg(not(windows))]
fn restrict_to_current_user(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| IO.to_string())
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
    fn a_generated_key_reads_back_as_what_was_asked_for() {
        let pair = generate("ed25519", "me@box 'quoted'", "").unwrap();
        assert!(pair.public_key.starts_with("ssh-ed25519 "));
        assert!(
            pair.public_key.ends_with(" me@box quoted"),
            "{}",
            pair.public_key
        );
        let info = inspect(&pair.private_key).unwrap();
        assert_eq!(info.algorithm, "ssh-ed25519");
        assert!(!info.encrypted);
        assert_eq!(info.public_key.unwrap(), pair.public_key);

        let locked = generate("ed25519", "", "a passphrase").unwrap();
        assert!(inspect(&locked.private_key).unwrap().encrypted);
        assert!(generate("dsa", "", "").is_err());
    }

    #[test]
    fn a_passphrase_is_set_changed_and_removed_without_changing_the_key() {
        let pair = generate("ed25519", "check", "").unwrap();
        // An encrypted key keeps its comment inside the encrypted part, so what is compared
        // is the key itself.
        let same_key = |text: &str| {
            let public = inspect(text).unwrap().public_key.unwrap();
            pair.public_key.starts_with(public.trim())
        };
        let locked = set_passphrase(&pair.private_key, "", "first").unwrap();
        assert!(inspect(&locked).unwrap().encrypted);
        assert!(same_key(&locked));

        assert_eq!(
            set_passphrase(&locked, "wrong", "second").err().unwrap(),
            PASSPHRASE
        );
        let relocked = set_passphrase(&locked, "first", "second").unwrap();
        assert_eq!(
            set_passphrase(&relocked, "first", "").err().unwrap(),
            PASSPHRASE
        );
        let open = set_passphrase(&relocked, "second", "").unwrap();
        assert!(!inspect(&open).unwrap().encrypted);
        assert!(same_key(&open));
        assert_eq!(inspect(&open).unwrap().public_key.unwrap(), pair.public_key);

        assert_eq!(
            set_passphrase(
                "-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----",
                "",
                "x"
            )
            .err()
            .unwrap(),
            KEY_FORMAT
        );
    }

    /// The system's own ssh-keygen reads the key we made and derives the same public key.
    #[test]
    fn openssh_itself_accepts_a_generated_key() {
        let dir = tempfile::tempdir().unwrap();
        let pair = generate("ed25519", "check", "").unwrap();
        let path = dir.path().join("ed25519");
        export_private(&pair.private_key, &path).unwrap();
        let Ok(out) = std::process::Command::new("ssh-keygen")
            .arg("-y")
            .arg("-f")
            .arg(&path)
            .output()
        else {
            eprintln!("ssh-keygen not found; skipped");
            return;
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let derived = String::from_utf8_lossy(&out.stdout);
        let mut words = pair.public_key.split(' ');
        let (kind, body) = (words.next().unwrap(), words.next().unwrap());
        assert!(
            derived.trim().starts_with(&format!("{kind} {body}")),
            "{derived}"
        );
    }

    #[test]
    fn installing_a_key_locally_writes_the_files_and_one_host_block() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        let pair = generate("ed25519", "me@box", "").unwrap();
        let host = LocalHost {
            alias: "vps1".into(),
            host: "203.0.113.7".into(),
            user: "root".into(),
            port: Some(2222),
        };
        let backed_up = std::cell::Cell::new(false);
        let path = install_local(
            &ssh,
            "vps1_ed25519",
            &pair.private_key,
            Some(&pair.public_key),
            Some(&host),
            |_| backed_up.set(true),
        )
        .unwrap();
        assert!(path.ends_with("vps1_ed25519") && backed_up.get());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), pair.private_key);
        assert_eq!(
            std::fs::read_to_string(ssh.join("vps1_ed25519.pub"))
                .unwrap()
                .trim(),
            pair.public_key
        );
        let config = std::fs::read_to_string(ssh.join("config")).unwrap();
        assert_eq!(
            config,
            "Host vps1\n    HostName 203.0.113.7\n    User root\n    Port 2222\n    IdentityFile ~/.ssh/vps1_ed25519\n    IdentitiesOnly yes\n"
        );

        // The same alias again is refused before any file is written.
        assert_eq!(
            install_local(&ssh, "other", &pair.private_key, None, Some(&host), |_| ())
                .err()
                .unwrap(),
            HOST_EXISTS
        );
        assert!(!ssh.join("other").exists());
        // An existing key file is never replaced; an existing config is added to, not rewritten.
        assert_eq!(
            install_local(&ssh, "vps1_ed25519", &pair.private_key, None, None, |_| ())
                .err()
                .unwrap(),
            FILE_EXISTS
        );
        let second = LocalHost {
            alias: "vps2".into(),
            port: None,
            ..host.clone()
        };
        install_local(&ssh, "vps2", &pair.private_key, None, Some(&second), |_| ()).unwrap();
        let config = std::fs::read_to_string(ssh.join("config")).unwrap();
        assert!(
            config.starts_with("Host vps1\n")
                && config.contains("\n\nHost vps2\n")
                && !config.contains("Port 22\n")
        );
    }

    #[test]
    fn local_install_refuses_names_and_hosts_that_are_not_plain() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "",
            "config",
            "known_hosts",
            "../up",
            "a b",
            ".hidden",
            "key.pub",
            "a/b",
        ] {
            assert_eq!(
                install_local(dir.path(), name, "x", None, None, |_| ())
                    .err()
                    .unwrap(),
                NAME,
                "{name}"
            );
        }
        let bad = LocalHost {
            alias: "a b".into(),
            host: "h".into(),
            user: "u".into(),
            port: None,
        };
        assert_eq!(
            install_local(dir.path(), "k", "x", None, Some(&bad), |_| ())
                .err()
                .unwrap(),
            HOST
        );
        let bad = LocalHost {
            alias: "a".into(),
            host: "h\nProxyCommand x".into(),
            user: "u".into(),
            port: None,
        };
        assert_eq!(
            install_local(dir.path(), "k", "x", None, Some(&bad), |_| ())
                .err()
                .unwrap(),
            HOST
        );
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
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

        let locked = keygen(
            dir.path(),
            "locked",
            &["-t", "ed25519", "-N", "fixture-pass"],
        )
        .unwrap();
        let info = inspect(&locked).unwrap();
        assert!(info.encrypted);
        assert!(info.risks.is_empty());
        assert!(
            info.public_key.is_some(),
            "OpenSSH files carry the public key in the clear"
        );
    }

    #[test]
    fn openssh_ecdsa_keys_are_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let Some(plain) = keygen(dir.path(), "ec", &["-t", "ecdsa", "-b", "256", "-N", ""]) else {
            eprintln!("ssh-keygen not found; skipped");
            return;
        };
        let info = inspect(&plain).unwrap();
        assert_eq!(info.algorithm, "ecdsa-sha2-nistp256");
        assert!(info.fingerprint.unwrap().starts_with("SHA256:"));
        assert_eq!(info.risks, vec![RISK_UNENCRYPTED]);

        let locked = keygen(
            dir.path(),
            "ec_locked",
            &["-t", "ecdsa", "-b", "256", "-N", "fixture-pass"],
        )
        .unwrap();
        let info = inspect(&locked).unwrap();
        assert_eq!(info.algorithm, "ecdsa-sha2-nistp256");
        assert!(info.encrypted);
        assert!(info.fingerprint.unwrap().starts_with("SHA256:"));
        assert!(info.risks.is_empty());
    }

    #[test]
    fn short_rsa_is_flagged_in_both_formats() {
        let dir = tempfile::tempdir().unwrap();
        let Some(openssh) = keygen(dir.path(), "rsa", &["-t", "rsa", "-b", "1024", "-N", ""])
        else {
            eprintln!("ssh-keygen not found; skipped");
            return;
        };
        let info = inspect(&openssh).unwrap();
        assert_eq!(info.bits, Some(1024));
        assert!(info.risks.contains(&RISK_RSA_SHORT));

        let pem = keygen(
            dir.path(),
            "rsa_pem",
            &["-t", "rsa", "-b", "1024", "-m", "PEM", "-N", ""],
        )
        .unwrap();
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
        let encrypted =
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n";
        assert!(inspect(encrypted).unwrap().encrypted);
        assert!(inspect("just some text").is_none());
    }

    #[test]
    fn export_writes_once_with_unix_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("id_restored");
        export_private("line one\r\nline two", &dest).unwrap();
        assert_eq!(
            std::fs::read_to_string(&dest).unwrap(),
            "line one\nline two\n"
        );
        assert_eq!(export_private("again", &dest).err().unwrap(), FILE_EXISTS);
    }

    #[cfg(windows)]
    #[test]
    fn exported_key_is_readable_only_by_the_current_user() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("id_locked_down");
        export_private("secret", &dest).unwrap();
        let output = std::process::Command::new("icacls")
            .arg(&dest)
            .output()
            .unwrap();
        let listing =
            String::from_utf8_lossy(&output.stdout).replace(dest.to_string_lossy().as_ref(), "");
        let user = std::env::var("USERNAME").unwrap();
        assert!(
            listing.contains(&user),
            "current user missing from ACL: {listing}"
        );
        for other in [
            "Everyone",
            "BUILTIN\\Users",
            "Authenticated Users",
            "Administrators",
            "SYSTEM",
        ] {
            assert!(
                !listing.contains(other),
                "{other} still has access: {listing}"
            );
        }
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "secret\n");
    }

    #[cfg(unix)]
    #[test]
    fn exported_key_is_mode_600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("id_locked_down");
        export_private("secret", &dest).unwrap();
        assert_eq!(
            std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn access_is_restricted_before_the_secret_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("id_ordered");
        export_with("secret", &dest, |path| {
            assert_eq!(
                std::fs::metadata(path).unwrap().len(),
                0,
                "secret written before restriction"
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "secret\n");
    }

    #[test]
    fn a_failed_export_leaves_no_file_and_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("id_failed");
        let error = export_with("secret", &dest, |_| Err(IO.to_string()))
            .err()
            .unwrap();
        assert_eq!(error, IO);
        assert!(
            !dest.exists(),
            "a failed export must not leave a file behind"
        );
        export_private("secret", &dest).unwrap();
    }
}
