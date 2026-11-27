//! Generic passwords in the login keychain, through Apple's own `security` tool.
//!
//! An item a program writes itself is tied to that program's code; without an Apple team ID
//! every update counts as another program and macOS asks for the login password once per
//! item. Items `security` writes trust `security`, so every Stacker build reads them silently,
//! and so can other programs of the same user (the same reach DPAPI has on Windows). Secrets
//! go in on stdin as hex, never on a command line another process could see.

use std::io::Write;
use std::process::{Command, Stdio};
use zeroize::Zeroizing;

const TOOL: &str = "/usr/bin/security";
/// The account every Stacker item is filed under; the service is its name.
pub const ACCOUNT: &str = "Stacker";
/// The longest secret one item takes: `security -i` reads a line of about 4 KB, and the secret
/// goes in as hex, two characters a byte, beside the item's name.
pub const MAX_SECRET_BYTES: usize = 1800;

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn hex(bytes: &[u8]) -> Zeroizing<String> {
    Zeroizing::new(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Adds or replaces the item named `service`, then reads it back: `security -i` reports
/// failures only as text and may cut a very long line short.
pub fn write(service: &str, secret: &[u8]) -> Result<(), String> {
    add(service, secret, true)?;
    match read(service) {
        Lookup::Found(back) if back.as_slice() == secret => Ok(()),
        _ => Err("写入钥匙串后读回的内容不一致".into()),
    }
}

/// Adds the item only when there is none by that name yet; an existing one is an error.
pub fn create(service: &str, secret: &[u8]) -> Result<(), String> {
    add(service, secret, false)?;
    match read(service) {
        Lookup::Found(back) if back.as_slice() == secret => Ok(()),
        _ => Err("写入钥匙串后读回的内容不一致".into()),
    }
}

fn add(service: &str, secret: &[u8], replace: bool) -> Result<(), String> {
    if secret.len() > MAX_SECRET_BYTES || service.len() > 300 {
        return Err("这个值太长，放不进钥匙串".into());
    }
    let line = Zeroizing::new(format!(
        "add-generic-password {}-a {} -s {} -l {} -X {}\n",
        if replace { "-U " } else { "" },
        quoted(ACCOUNT),
        quoted(service),
        quoted(service),
        hex(secret).as_str()
    ));
    let mut child = Command::new(TOOL)
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法运行 security：{e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("写入钥匙串失败：{e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("写入钥匙串失败：{e}"))?;
    let error = String::from_utf8_lossy(&output.stderr);
    // `security -i` exits 0 even when the command inside it failed; the failure is on stderr.
    if output.status.success() && !error.contains("returned") && !error.contains("unknown command")
    {
        Ok(())
    } else {
        Err(format!("写入钥匙串失败：{}", error.trim()))
    }
}

/// What a lookup found. A keychain that is locked or refused is not the same as an item that
/// is not there: only the second may be created anew.
pub enum Lookup {
    Found(Zeroizing<Vec<u8>>),
    Missing,
    Failed,
}

impl Lookup {
    pub fn found(self) -> Option<Zeroizing<Vec<u8>>> {
        match self {
            Lookup::Found(secret) => Some(secret),
            _ => None,
        }
    }
}

/// `security` exits with 44 for errSecItemNotFound.
const NOT_FOUND: i32 = 44;

pub fn read(service: &str) -> Lookup {
    let Ok(output) = Command::new(TOOL)
        .args(["find-generic-password", "-a", ACCOUNT, "-s", service, "-g"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    else {
        return Lookup::Failed;
    };
    if output.status.code() == Some(NOT_FOUND) {
        return Lookup::Missing;
    }
    if !output.status.success() {
        return Lookup::Failed;
    }
    let text = Zeroizing::new(String::from_utf8_lossy(&output.stderr).into_owned());
    parse_password(&text).map_or(Lookup::Failed, Lookup::Found)
}

/// `-g` prints `password: "<text>"` for printable ASCII and `password: 0x<hex>  "…"` for
/// anything else.
fn parse_password(stderr: &str) -> Option<Zeroizing<Vec<u8>>> {
    let line = stderr.lines().find_map(|l| l.strip_prefix("password: "))?;
    if let Some(rest) = line.strip_prefix("0x") {
        let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        return unhex(&hex).map(Zeroizing::new);
    }
    let inner = line.strip_prefix('"')?.strip_suffix('"')?;
    Some(Zeroizing::new(inner.as_bytes().to_vec()))
}

pub fn delete(service: &str) {
    let _ = Command::new(TOOL)
        .args(["delete-generic-password", "-a", ACCOUNT, "-s", service])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The names of Stacker's items that start with `prefix`. Listing reads attributes only;
/// no secret leaves the keychain.
pub fn list(prefix: &str) -> Vec<String> {
    let Ok(output) = Command::new(TOOL)
        .arg("dump-keychain")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    parse_dump(&String::from_utf8_lossy(&output.stdout))
        .into_iter()
        .filter(|(account, service)| account == ACCOUNT && service.starts_with(prefix))
        .map(|(_, service)| service)
        .collect()
}

/// (account, service) of every generic password in a `dump-keychain` listing.
fn parse_dump(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut generic = false;
    let mut account = None;
    let mut service = None;
    let mut flush = |generic: bool, account: &mut Option<String>, service: &mut Option<String>| {
        if let (true, Some(a), Some(s)) = (generic, account.take(), service.take()) {
            out.push((a, s));
        }
        *account = None;
        *service = None;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("keychain:") {
            flush(generic, &mut account, &mut service);
            generic = false;
        } else if let Some(class) = line.strip_prefix("class: ") {
            generic = class.trim() == "\"genp\"";
        } else if let Some(value) = line.strip_prefix("\"acct\"<blob>=") {
            account = blob(value);
        } else if let Some(value) = line.strip_prefix("\"svce\"<blob>=") {
            service = blob(value);
        }
    }
    flush(generic, &mut account, &mut service);
    out
}

/// A `<blob>` value: `"text"`, or `0x<hex>  "…"` when it is not plain ASCII.
fn blob(value: &str) -> Option<String> {
    if let Some(rest) = value.strip_prefix("0x") {
        let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        return String::from_utf8(unhex(&hex)?).ok();
    }
    Some(value.strip_prefix('"')?.strip_suffix('"')?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_is_read_in_either_form() {
        assert_eq!(
            parse_password("keychain: \"x\"\npassword: \"sk-1\"\n")
                .unwrap()
                .as_slice(),
            b"sk-1"
        );
        let hex = hex("密钥".as_bytes());
        assert_eq!(
            parse_password(&format!("password: 0x{}  \"\\345\"\n", hex.to_uppercase()))
                .unwrap()
                .as_slice(),
            "密钥".as_bytes()
        );
        assert!(parse_password("no such line").is_none());
    }

    #[test]
    fn only_generic_passwords_are_listed_with_their_names() {
        let name = hex("Stacker:密钥:token".as_bytes());
        let dump = format!(
            "keychain: \"/k\"\nversion: 512\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"Stacker\"\n    \"svce\"<blob>=\"Stacker:a b:password\"\n\
             keychain: \"/k\"\nclass: \"inet\"\nattributes:\n    \"acct\"<blob>=\"Stacker\"\n    \"srvr\"<blob>=\"x\"\n\
             keychain: \"/k\"\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"Stacker\"\n    \"svce\"<blob>=0x{}  \"Stacker:\\345\"\n",
            name.as_str()
        );
        assert_eq!(
            parse_dump(&dump),
            vec![
                ("Stacker".into(), "Stacker:a b:password".into()),
                ("Stacker".into(), "Stacker:密钥:token".into()),
            ]
        );
    }

    #[test]
    #[ignore = "writes to the real login keychain; run by hand in the logged-in session"]
    fn an_item_round_trips_and_is_listed_by_prefix() {
        let name = "Stacker-selftest:a \"b\" (c):密钥";
        create(name, "sk-测试-1".as_bytes()).unwrap();
        assert!(create(name, b"other").is_err(), "create never replaces");
        assert_eq!(
            read(name).found().unwrap().as_slice(),
            "sk-测试-1".as_bytes()
        );
        write(name, b"second").unwrap();
        assert_eq!(read(name).found().unwrap().as_slice(), b"second");
        let long = vec![b'k'; MAX_SECRET_BYTES];
        write(name, &long).unwrap();
        assert_eq!(read(name).found().unwrap().as_slice(), long.as_slice());
        assert!(write(name, &vec![b'k'; MAX_SECRET_BYTES + 1]).is_err());
        assert_eq!(
            read(name).found().unwrap().as_slice(),
            long.as_slice(),
            "kept as it was"
        );
        assert!(list("Stacker-selftest:").contains(&name.to_string()));
        delete(name);
        assert!(matches!(read(name), Lookup::Missing));
        assert!(list("Stacker-selftest:").is_empty());
    }

    #[test]
    fn a_name_is_quoted_for_the_interactive_tool() {
        assert_eq!(quoted(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}
