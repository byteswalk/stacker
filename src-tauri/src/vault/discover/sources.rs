//! 发现的四类来源：~/.ssh、云与包管理配置文件、环境变量、项目 .env。只读。

use super::rules;
use crate::vault::model::Kind;
use crate::vault::ssh;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use zeroize::Zeroizing;

pub(crate) const MAX_FILE_BYTES: u64 = 1_048_576;
pub(crate) const MAX_FILES: usize = 50_000;
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".idea",
    ".vs",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    Ssh,
    Config,
    Env,
    Dotenv,
}

#[derive(Clone)]
pub(crate) struct RawField {
    pub name: String,
    pub value: Zeroizing<String>,
    pub secret: bool,
}

impl std::fmt::Debug for RawField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawField")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("secret", &self.secret)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Raw {
    pub source: Source,
    pub location: String,
    pub name: String,
    pub platform: String,
    pub kind: Kind,
    pub fields: Vec<RawField>,
    pub risks: Vec<&'static str>,
}

impl Raw {
    pub(crate) fn primary(&self) -> &str {
        self.fields
            .iter()
            .find(|field| field.secret)
            .map(|field| field.value.as_str())
            .unwrap_or("")
    }
}

fn field(name: &str, value: String, secret: bool) -> RawField {
    RawField {
        name: name.to_string(),
        value: Zeroizing::new(value),
        secret,
    }
}

pub(crate) struct Walk<'a> {
    pub cancel: &'a AtomicBool,
    pub files: &'a AtomicUsize,
    pub truncated: &'a AtomicBool,
}

impl Walk<'_> {
    /// Files under `root` at most `depth` levels down (1 = only `root` itself), never through
    /// links or junctions, skipping build and dependency folders.
    pub(crate) fn files(
        &self,
        root: &Path,
        depth: usize,
        keep: &dyn Fn(&str) -> bool,
    ) -> Vec<PathBuf> {
        let mut out = Vec::new();
        // A root that is itself a link (or junction) is skipped, like any link met on the way down.
        if std::fs::symlink_metadata(root).is_ok_and(|meta| !is_link(&meta)) {
            self.visit(root, depth, keep, &mut out);
        }
        out
    }

    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.truncated.load(Ordering::Relaxed)
    }

    fn visit(&self, dir: &Path, depth: usize, keep: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
        if self.stopped() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if self.stopped() {
                return;
            }
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if is_link(&meta) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if meta.is_dir() {
                if depth > 1 && !SKIP_DIRS.contains(&name.as_str()) {
                    self.visit(&path, depth - 1, keep, out);
                }
                continue;
            }
            if self.files.fetch_add(1, Ordering::Relaxed) >= MAX_FILES {
                self.truncated.store(true, Ordering::Relaxed);
                return;
            }
            if meta.len() <= MAX_FILE_BYTES && keep(&name) {
                out.push(path);
            }
        }
    }
}

fn is_link(meta: &std::fs::Metadata) -> bool {
    // On Windows std reports both symlinks and junctions as symlinks, while cloud
    // placeholders (OneDrive Files-On-Demand) stay readable regular files.
    meta.file_type().is_symlink()
}

fn read_small(path: &Path) -> Option<Zeroizing<String>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = Zeroizing::new(std::fs::read(path).ok()?);
    decode_text(&bytes).map(Zeroizing::new)
}

/// UTF-8, or UTF-16LE when the file starts with its byte-order mark (what PowerShell 5.1's `>` writes).
fn decode_text(bytes: &[u8]) -> Option<String> {
    match bytes.strip_prefix(&[0xFF, 0xFE]) {
        Some(rest) => {
            let units: Zeroizing<Vec<u16>> = Zeroizing::new(
                rest.chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect(),
            );
            String::from_utf16(&units).ok()
        }
        None => String::from_utf8(bytes.to_vec()).ok(),
    }
}

pub(crate) fn ssh_keys(home: &Path, walk: &Walk) -> Vec<Raw> {
    let keep = |name: &str| {
        !name.ends_with(".pub")
            && !name.starts_with("known_hosts")
            && name != "config"
            && name != "authorized_keys"
    };
    walk.files(&home.join(".ssh"), 3, &keep)
        .into_iter()
        .filter_map(|path| {
            let text = read_small(&path)?;
            if !text.contains("PRIVATE KEY-----") {
                return None;
            }
            let info = ssh::inspect(&text);
            let public = info
                .as_ref()
                .and_then(|info| info.public_key.clone())
                .or_else(|| {
                    read_small(&PathBuf::from(format!("{}.pub", path.display())))
                        .map(|text| text.trim().to_string())
                });
            let mut fields = vec![field("私钥", text.to_string(), true)];
            if let Some(public) = public {
                fields.push(field("公钥", public, false));
            }
            Some(Raw {
                source: Source::Ssh,
                location: path.display().to_string(),
                name: path.file_name()?.to_string_lossy().to_string(),
                platform: String::new(),
                kind: Kind::SshKey,
                fields,
                risks: info.map(|info| info.risks).unwrap_or_default(),
            })
        })
        .collect()
}

type Section = (String, Vec<(String, String)>);

fn ini_sections(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = vec![(String::new(), Vec::new())];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            out.push((line[1..line.len() - 1].trim().to_string(), Vec::new()));
        } else if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().trim_matches('"').to_string();
            out.last_mut()
                .expect("starts with one section")
                .1
                .push((key.trim().to_string(), value));
        }
    }
    out
}

fn get(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.clone())
}

fn config_raw(path: &Path, name: String, platform: &str, kind: Kind, fields: Vec<RawField>) -> Raw {
    Raw {
        source: Source::Config,
        location: path.display().to_string(),
        name,
        platform: platform.to_string(),
        kind,
        fields,
        risks: Vec::new(),
    }
}

fn usable(value: &str) -> bool {
    !value.trim().is_empty() && !rules::is_placeholder(value)
}

fn aws(path: &Path, text: &str) -> Vec<Raw> {
    ini_sections(text)
        .into_iter()
        .filter_map(|(section, pairs)| {
            let id = get(&pairs, "aws_access_key_id")?;
            let secret = get(&pairs, "aws_secret_access_key").filter(|value| usable(value))?;
            Some(config_raw(
                path,
                format!("AWS {section}"),
                "AWS",
                Kind::AkSk,
                vec![
                    field("Access Key ID", id, false),
                    field("Secret Access Key", secret, true),
                ],
            ))
        })
        .collect()
}

/// Drops one pair of matching surrounding double or single quotes.
fn unquote(value: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|quote| {
            value
                .strip_prefix(*quote)
                .and_then(|rest| rest.strip_suffix(*quote))
        })
        .unwrap_or(value)
}

fn decoded(text: &str) -> String {
    percent_encoding::percent_decode_str(text)
        .decode_utf8_lossy()
        .into_owned()
}

fn npmrc(path: &Path, text: &str) -> Vec<Raw> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            let (key, value) = (key.trim(), unquote(value.trim()));
            let secret_key =
                key.ends_with("_authToken") || key.ends_with("_auth") || key.ends_with("_password");
            if !secret_key || !usable(value) {
                return None;
            }
            Some(config_raw(
                path,
                key.to_string(),
                "npm",
                Kind::Token,
                vec![field("Token", value.to_string(), true)],
            ))
        })
        .collect()
}

fn git_credentials(path: &Path, text: &str) -> Vec<Raw> {
    text.lines()
        .filter_map(|line| {
            let url = url::Url::parse(line.trim()).ok()?;
            let token = decoded(url.password()?);
            if !usable(&token) {
                return None;
            }
            let user = decoded(url.username());
            let host = url.host_str()?.to_string();
            Some(config_raw(
                path,
                format!("{user}@{host}"),
                &host,
                Kind::Token,
                vec![field("账号", user, false), field("Token", token, true)],
            ))
        })
        .collect()
}

fn pypirc(path: &Path, text: &str) -> Vec<Raw> {
    ini_sections(text)
        .into_iter()
        .filter_map(|(section, pairs)| {
            let password = get(&pairs, "password").filter(|value| usable(value))?;
            let user = get(&pairs, "username").unwrap_or_default();
            Some(config_raw(
                path,
                format!("PyPI {section}"),
                "PyPI",
                Kind::Token,
                vec![field("账号", user, false), field("Token", password, true)],
            ))
        })
        .collect()
}

fn cargo(path: &Path, text: &str) -> Vec<Raw> {
    ini_sections(text)
        .into_iter()
        .filter_map(|(section, pairs)| {
            let token = get(&pairs, "token").filter(|value| usable(value))?;
            let name = if section.is_empty() {
                "crates.io".to_string()
            } else {
                section
            };
            Some(config_raw(
                path,
                name,
                "crates.io",
                Kind::Token,
                vec![field("Token", token, true)],
            ))
        })
        .collect()
}

pub(crate) fn config_files(home: &Path) -> Vec<Raw> {
    type Parser = fn(&Path, &str) -> Vec<Raw>;
    let parsers: [(&str, Parser); 5] = [
        (".aws/credentials", aws),
        (".npmrc", npmrc),
        (".git-credentials", git_credentials),
        (".pypirc", pypirc),
        (".cargo/credentials.toml", cargo),
    ];
    parsers
        .iter()
        .flat_map(|(relative, parse)| {
            let path = home.join(relative);
            read_small(&path)
                .map(|text| parse(&path, &text))
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(windows)]
fn env_scopes() -> Vec<(&'static str, Vec<(String, String)>)> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let read = |root, path: &str| -> Vec<(String, String)> {
        let Ok(key) = RegKey::predef(root).open_subkey(path) else {
            return Vec::new();
        };
        key.enum_values()
            .flatten()
            .filter_map(|(name, _)| {
                key.get_value::<String, _>(&name)
                    .ok()
                    .map(|value| (name, value))
            })
            .collect()
    };
    vec![
        ("user", read(HKEY_CURRENT_USER, "Environment")),
        (
            "system",
            read(
                HKEY_LOCAL_MACHINE,
                r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            ),
        ),
    ]
}

#[cfg(not(windows))]
fn env_scopes() -> Vec<(&'static str, Vec<(String, String)>)> {
    Vec::new()
}

pub(crate) fn env_vars() -> Vec<Raw> {
    let mut out = Vec::new();
    for (scope, pairs) in env_scopes() {
        for (name, value) in pairs {
            let Some((platform, kind)) = rules::classify(&name, &value) else {
                continue;
            };
            out.push(Raw {
                source: Source::Env,
                location: scope.to_string(),
                name,
                platform,
                kind,
                fields: vec![field(rules::primary_field(kind), value, true)],
                risks: Vec::new(),
            });
        }
    }
    out
}

pub(crate) fn is_env_file(name: &str) -> bool {
    (name == ".env" || name.starts_with(".env."))
        && ![".example", ".sample", ".template"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

pub(crate) fn dotenv_files(dirs: &[PathBuf], walk: &Walk) -> Vec<Raw> {
    dirs.iter()
        .flat_map(|dir| walk.files(dir, 4, &is_env_file))
        .flat_map(|path| {
            let text = read_small(&path).unwrap_or_default();
            rules::parse_env(&text)
                .into_iter()
                .filter_map(|(name, value)| {
                    let (platform, kind) = rules::classify(&name, &value)?;
                    Some(Raw {
                        source: Source::Dotenv,
                        location: path.display().to_string(),
                        name,
                        platform,
                        kind,
                        fields: vec![field(rules::primary_field(kind), value, true)],
                        risks: Vec::new(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample() -> String {
        ["Zq81", "vKp3", "Lm0X", "w7Rt", "2YbN", "c4Hd"].concat()
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    struct Counters {
        cancel: AtomicBool,
        files: AtomicUsize,
        truncated: AtomicBool,
    }

    impl Counters {
        fn new() -> Self {
            Counters {
                cancel: AtomicBool::new(false),
                files: AtomicUsize::new(0),
                truncated: AtomicBool::new(false),
            }
        }
        fn walk(&self) -> Walk<'_> {
            Walk {
                cancel: &self.cancel,
                files: &self.files,
                truncated: &self.truncated,
            }
        }
    }

    const DSA: &str = "-----BEGIN DSA PRIVATE KEY-----\nAAAA\n-----END DSA PRIVATE KEY-----\n";

    #[test]
    fn ssh_keys_are_found_with_their_public_half_and_risks() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        put(&ssh.join("id_old"), DSA);
        put(&ssh.join("id_old.pub"), "ssh-dss AAAA fixture");
        put(&ssh.join("known_hosts"), "host ssh-ed25519 AAAA");
        put(&ssh.join("config"), "Host x");
        put(&ssh.join("a/b/c/too_deep"), DSA);
        let counters = Counters::new();
        let found = ssh_keys(home.path(), &counters.walk());
        assert_eq!(found.len(), 1);
        let raw = &found[0];
        assert_eq!(raw.kind, Kind::SshKey);
        assert_eq!(raw.name, "id_old");
        assert!(raw.risks.contains(&crate::vault::ssh::RISK_DSA));
        assert_eq!(
            raw.fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["私钥", "公钥"]
        );
        assert!(raw.primary().contains("DSA PRIVATE KEY"));
    }

    #[test]
    fn config_files_yield_tokens_and_key_pairs() {
        let home = tempfile::tempdir().unwrap();
        let token = sample();
        put(&home.path().join(".aws/credentials"), &format!(
            "[default]\naws_access_key_id = ID1\naws_secret_access_key = {token}\n[blank]\naws_access_key_id = ID2\naws_secret_access_key = your-secret-here\n"
        ));
        put(&home.path().join(".npmrc"), &format!("registry=https://registry.npmjs.org/\n//registry.npmjs.org/:_authToken={token}\n//x/:_authToken=${{NPM_TOKEN}}\n"));
        put(
            &home.path().join(".git-credentials"),
            &format!("https://alice:{token}@github.com\nnot a url\n"),
        );
        put(
            &home.path().join(".pypirc"),
            &format!("[pypi]\nusername = __token__\npassword = {token}\n"),
        );
        put(
            &home.path().join(".cargo/credentials.toml"),
            &format!("[registry]\ntoken = \"{token}\"\n"),
        );
        let found = config_files(home.path());
        let names: Vec<&str> = found.iter().map(|raw| raw.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "AWS default",
                "//registry.npmjs.org/:_authToken",
                "alice@github.com",
                "PyPI pypi",
                "registry"
            ]
        );
        let aws = &found[0];
        assert_eq!(aws.kind, Kind::AkSk);
        assert_eq!(aws.primary(), token);
        assert!(!aws.fields[0].secret && aws.fields[1].secret);
        assert_eq!(found[2].platform, "github.com");
        assert!(found.iter().all(|raw| raw.source == Source::Config));
    }

    #[test]
    fn dotenv_files_respect_names_depth_size_and_skipped_folders() {
        let root = tempfile::tempdir().unwrap();
        let line = format!("OPENAI_API_KEY={}\nNODE_ENV=development\n", sample());
        put(&root.path().join("proj/.env"), &line);
        put(&root.path().join("proj/.env.local"), &line);
        put(&root.path().join("proj/.env.example"), &line);
        put(&root.path().join("proj/node_modules/pkg/.env"), &line);
        put(&root.path().join("a/b/c/.env"), &line);
        put(&root.path().join("a/b/c/d/.env"), &line);
        put(
            &root.path().join("big/.env"),
            &format!("{line}{}", "#".repeat(1_100_000)),
        );
        let counters = Counters::new();
        let found = dotenv_files(&[root.path().to_path_buf()], &counters.walk());
        let mut places: Vec<String> = found
            .iter()
            .map(|raw| {
                Path::new(&raw.location)
                    .strip_prefix(root.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        places.sort();
        assert_eq!(places, vec!["a/b/c/.env", "proj/.env", "proj/.env.local"]);
        assert!(found
            .iter()
            .all(|raw| raw.name == "OPENAI_API_KEY" && raw.source == Source::Dotenv));
    }

    #[test]
    fn walking_stops_at_the_file_limit_and_on_cancel() {
        let root = tempfile::tempdir().unwrap();
        put(&root.path().join(".env"), &format!("A_KEY={}\n", sample()));
        let counters = Counters::new();
        counters.files.store(MAX_FILES, Ordering::Relaxed);
        assert!(dotenv_files(&[root.path().to_path_buf()], &counters.walk()).is_empty());
        assert!(counters.truncated.load(Ordering::Relaxed));
        let cancelled = Counters::new();
        cancelled.cancel.store(true, Ordering::Relaxed);
        assert!(dotenv_files(&[root.path().to_path_buf()], &cancelled.walk()).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        put(
            &outside.path().join(".env"),
            &format!("A_KEY={}\n", sample()),
        );
        let link = root.path().join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(made.status.success());
        let counters = Counters::new();
        assert!(dotenv_files(&[root.path().to_path_buf()], &counters.walk()).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn a_scan_root_that_is_a_junction_is_skipped() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        put(
            &outside.path().join(".env"),
            &format!("A_KEY={}\n", sample()),
        );
        let link = base.path().join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(made.status.success());
        let counters = Counters::new();
        assert!(dotenv_files(&[link], &counters.walk()).is_empty());
        // The same folder reached directly is still scanned.
        assert_eq!(
            dotenv_files(&[outside.path().to_path_buf()], &counters.walk()).len(),
            1
        );
    }

    #[test]
    fn utf16_files_with_a_byte_order_mark_are_decoded() {
        let root = tempfile::tempdir().unwrap();
        let text = format!("OPENAI_API_KEY={}\r\nNODE_ENV=development\r\n", sample());
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(root.path().join(".env"), &bytes).unwrap();
        let counters = Counters::new();
        let found = dotenv_files(&[root.path().to_path_buf()], &counters.walk());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].primary(), sample());
        // Without a BOM, bytes that are not valid UTF-8 are still left alone.
        assert!(decode_text(&[0x41, 0xFF, 0x42]).is_none());
        assert_eq!(decode_text(b"plain").as_deref(), Some("plain"));
    }

    #[test]
    fn git_credentials_are_percent_decoded() {
        let home = tempfile::tempdir().unwrap();
        let encoded = format!("{}%40{}", &sample()[..6], &sample()[6..]);
        let decoded = format!("{}@{}", &sample()[..6], &sample()[6..]);
        put(
            &home.path().join(".git-credentials"),
            &format!("https://al%40ice:{encoded}@github.com\n"),
        );
        let found = config_files(home.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].primary(), decoded);
        assert_eq!(found[0].fields[0].value.as_str(), "al@ice");
        assert_eq!(found[0].name, "al@ice@github.com");
    }

    #[test]
    fn debug_output_redacts_values() {
        let value = sample();
        let secret = field("Token", value.clone(), true);
        assert!(!format!("{secret:?}").contains(&value));
        let raw = Raw {
            source: Source::Env,
            location: "user".into(),
            name: "X".into(),
            platform: String::new(),
            kind: Kind::Token,
            fields: vec![secret],
            risks: Vec::new(),
        };
        assert!(!format!("{raw:?}").contains(&value));
    }

    #[test]
    fn read_small_reads_regular_files_only() {
        let dir = tempfile::tempdir().unwrap();
        put(&dir.path().join("f.txt"), "hello");
        assert_eq!(
            read_small(&dir.path().join("f.txt"))
                .as_deref()
                .map(String::as_str),
            Some("hello")
        );
        assert!(read_small(dir.path()).is_none());
    }

    #[test]
    fn npmrc_values_lose_one_pair_of_quotes() {
        let home = tempfile::tempdir().unwrap();
        let token = sample();
        put(
            &home.path().join(".npmrc"),
            &format!("//r/:_authToken=\"{token}\"\n//s/:_authToken='{token}'\n"),
        );
        let found = config_files(home.path());
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|raw| raw.primary() == token));
    }

    #[test]
    fn env_file_names() {
        for name in [".env", ".env.local", ".env.production"] {
            assert!(is_env_file(name), "{name}");
        }
        for name in [
            ".env.example",
            ".env.sample",
            ".env.template",
            "env",
            "app.env",
            ".envrc",
        ] {
            assert!(!is_env_file(name), "{name}");
        }
    }
}
