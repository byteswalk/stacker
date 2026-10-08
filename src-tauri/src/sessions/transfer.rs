//! 会话迁移：把一批 Codex / Claude Code 对话（可选连同工程目录）打成一个迁移包，在另一台电脑
//! 导入后用 `codex resume` / `claude --resume` 接着聊。
//!
//! - Codex keeps its list in `state_<n>.sqlite`; a rollout file alone is not seen, so the import
//!   writes the rollout and the thread's rows, only the columns the other machine's database
//!   has, after backing that database up. It refuses while Codex runs, and never overwrites a
//!   thread id that is already there.
//! - Claude Code files each session under `projects/<encoded working directory>/`, so the
//!   transcript goes where the new working directory says.
//! - Paths of the old machine (each project, Codex's own folder) are rewritten wherever they
//!   appear, so the conversation and the tools it ran point at the new places.
//! - Reasoning that the service encrypted (Codex) or signed (Claude thinking blocks) belongs to
//!   the account that produced it. When the other machine signs in with another account it is
//!   left out; with the same account everything goes over as it is.

use super::model::{Agent, Roots, Session};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const FORMAT: &str = "stacker-session-transfer";
const VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";

/// What a project carries that is rebuilt anyway; left out of a package.
const REBUILT: &[&str] = &[
    "node_modules",
    "target",
    ".gradle",
    "__pycache__",
    ".venv",
    "venv",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    ".pytest_cache",
    ".mypy_cache",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub created_at: u64,
    pub machine: String,
    /// Each agent's account on the packing machine, as a one-way fingerprint.
    #[serde(default)]
    pub accounts: BTreeMap<String, String>,
    /// The agents' own folders there, so paths into them can be moved too.
    #[serde(default)]
    pub roots: BTreeMap<String, String>,
    pub sessions: Vec<PackedSession>,
    pub projects: Vec<PackedProject>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedSession {
    pub agent: Agent,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub updated_at: u64,
    pub files: Vec<PackedFile>,
    /// Codex: the thread's rows (and its sub-agents'), column by column.
    #[serde(default)]
    pub threads: Vec<Map<String, Value>>,
    /// Codex: rows of other tables keyed by thread id.
    #[serde(default)]
    pub tables: BTreeMap<String, Vec<Map<String, Value>>>,
    /// Codex: (parent, child, status) of sub-agent threads.
    #[serde(default)]
    pub edges: Vec<(String, String, String)>,
    /// Codex: its lines of `session_index.jsonl`.
    #[serde(default)]
    pub index_lines: Vec<String>,
}

/// One file in the package: `entry` in the zip, `role` says where it goes, `relative` where
/// under that place.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedFile {
    pub entry: String,
    /// `rollout` / `artifact` (under the Codex folder), `transcript` / `side` (Claude).
    pub role: String,
    pub relative: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedProject {
    pub path: String,
    pub name: String,
    pub included: bool,
    pub files: u64,
    pub bytes: u64,
    pub entry_prefix: String,
}

// ---------- accounts ----------

fn fingerprint(parts: &[&str]) -> String {
    let digest = Sha256::digest(parts.join("\n").as_bytes());
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// Who Codex is signed in as: the ChatGPT account, or an API key.
pub fn codex_account(root: &Path) -> Option<String> {
    let auth: Value = serde_json::from_slice(&std::fs::read(root.join("auth.json")).ok()?).ok()?;
    if let Some(account) = auth.pointer("/tokens/account_id").and_then(Value::as_str) {
        return Some(fingerprint(&["codex", "chatgpt", account]));
    }
    auth.get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
        .map(|key| fingerprint(&["codex", "apikey", key]))
}

/// Who Claude Code is signed in as: its organization and account.
pub fn claude_account(root: &Path) -> Option<String> {
    // `.claude.json` sits beside the folder by default, inside it when CLAUDE_CONFIG_DIR moved it.
    let candidates = [
        root.join(".claude.json"),
        root.parent()
            .map(|p| p.join(".claude.json"))
            .unwrap_or_default(),
    ];
    let config: Value = candidates
        .iter()
        .find_map(|path| serde_json::from_slice(&std::fs::read(path).ok()?).ok())?;
    let account = config.pointer("/oauthAccount/accountUuid")?.as_str()?;
    let org = config
        .pointer("/oauthAccount/organizationUuid")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(fingerprint(&["claude", org, account]))
}

fn agent_key(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "codex",
        Agent::Claude => "claude",
        _ => "other",
    }
}

pub fn supported(agent: Agent) -> bool {
    matches!(agent, Agent::Codex | Agent::Claude)
}

// ---------- paths ----------

fn plain(path: &str) -> String {
    path.trim_start_matches(r"\\?\").to_string()
}

/// Replaces one directory by another wherever it starts a path in `text`, in its `\\?\`,
/// backslash and forward-slash spellings. A match must end the path or be followed by a
/// separator, so `E:\app` does not touch `E:\app2`.
pub fn remap_text(text: &str, moves: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (from, to) in moves {
        let forms = [
            (format!(r"\\?\{from}"), to.clone()),
            // Escaped once or twice more inside text that is itself escaped (a command's JSON
            // output, a tool call quoting one).
            (from.replace('\\', r"\\\\"), to.replace('\\', r"\\\\")),
            (from.replace('\\', r"\\"), to.replace('\\', r"\\")),
            (from.clone(), to.clone()),
            (from.replace('\\', "/"), to.replace('\\', "/")),
            // In a file URL, where what is not plain ASCII is percent-encoded.
            (url_path(from), url_path(to)),
        ];
        for (old, new) in forms {
            if old.is_empty() || !out.contains(&old) {
                continue;
            }
            let mut result = String::with_capacity(out.len());
            let mut rest = out.as_str();
            while let Some(at) = rest.find(&old) {
                let after = &rest[at + old.len()..];
                let boundary = !matches!(after.chars().next(), Some(c) if c.is_alphanumeric() || "-_.~".contains(c));
                result.push_str(&rest[..at]);
                result.push_str(if boundary { &new } else { &old });
                rest = after;
            }
            result.push_str(rest);
            out = result;
        }
    }
    out
}

/// A path as a `file:///` URL spells it: forward slashes, and every byte that is not an
/// unreserved ASCII character percent-encoded.
fn url_path(path: &str) -> String {
    path.replace('\\', "/")
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn remap_value(value: &mut Value, moves: &[(String, String)]) {
    match value {
        Value::String(text) => *text = remap_text(text, moves),
        Value::Array(items) => items.iter_mut().for_each(|item| remap_value(item, moves)),
        Value::Object(map) => map.values_mut().for_each(|item| remap_value(item, moves)),
        _ => {}
    }
}

/// The folder Claude Code files a session under for `cwd`: every character that is not an
/// ASCII letter or digit becomes `-` (one per UTF-16 unit, as its JavaScript does).
pub fn claude_project_dir(cwd: &str) -> String {
    cwd.encode_utf16()
        .map(|unit| match char::from_u32(unit as u32) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect()
}

// ---------- what goes over, line by line ----------

/// A Codex rollout for the new machine: paths moved, and with another account the encrypted
/// reasoning (and encrypted compaction) left out. Returns the lines and how many were dropped.
pub fn rewrite_codex(text: &str, moves: &[(String, String)], strip: bool) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    let mut dropped = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(mut entry) = serde_json::from_str::<Value>(line) else {
            lines.push(line.to_string());
            continue;
        };
        if strip {
            let encrypted = entry
                .get("payload")
                .and_then(Value::as_object)
                .is_some_and(|payload| payload.contains_key("encrypted_content"));
            if entry.get("type").and_then(Value::as_str) == Some("response_item") && encrypted {
                dropped += 1;
                continue;
            }
            if let Some(history) = entry
                .pointer_mut("/payload/replacement_history")
                .and_then(Value::as_array_mut)
            {
                let before = history.len();
                history.retain(|item| item.get("encrypted_content").is_none());
                dropped += before - history.len();
            }
        }
        remap_value(&mut entry, moves);
        lines.push(entry.to_string());
    }
    (lines, dropped)
}

const LINKS: &[&str] = &[
    "parentUuid",
    "logicalParentUuid",
    "leafUuid",
    "sourceToolAssistantUUID",
];

/// A Claude Code transcript for the new machine: paths moved, and with another account the
/// signed thinking blocks left out. A line left with nothing is dropped, and the lines that
/// pointed at it point at its parent instead, so the conversation stays one chain.
pub fn rewrite_claude(text: &str, moves: &[(String, String)], strip: bool) -> (Vec<String>, usize) {
    let mut entries: Vec<Option<Value>> = Vec::new();
    let mut raw: Vec<Option<String>> = Vec::new();
    let mut replaced: HashMap<String, Option<String>> = HashMap::new();
    let mut dropped = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(mut entry) = serde_json::from_str::<Value>(line) else {
            entries.push(None);
            raw.push(Some(line.to_string()));
            continue;
        };
        if strip {
            if let Some(content) = entry
                .pointer_mut("/message/content")
                .and_then(Value::as_array_mut)
            {
                let before = content.len();
                content.retain(|block| {
                    !matches!(
                        block.get("type").and_then(Value::as_str),
                        Some("thinking") | Some("redacted_thinking")
                    )
                });
                dropped += before - content.len();
                if before > 0 && content.is_empty() {
                    let uuid = entry
                        .get("uuid")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let parent = entry
                        .get("parentUuid")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    if let Some(uuid) = uuid {
                        replaced.insert(uuid, parent);
                    }
                    continue;
                }
            }
        }
        remap_value(&mut entry, moves);
        entries.push(Some(entry));
        raw.push(None);
    }
    let resolve = |mut id: String| -> Option<String> {
        let mut seen = HashSet::new();
        while let Some(next) = replaced.get(&id) {
            if !seen.insert(id.clone()) {
                return None;
            }
            id = next.clone()?;
        }
        Some(id)
    };
    let lines = entries
        .into_iter()
        .zip(raw)
        .map(|(entry, raw)| match entry {
            None => raw.unwrap_or_default(),
            Some(mut entry) => {
                if let Some(map) = entry.as_object_mut() {
                    for key in LINKS {
                        if let Some(Value::String(id)) = map.get(*key) {
                            if replaced.contains_key(id) {
                                let next = resolve(id.clone())
                                    .map(Value::String)
                                    .unwrap_or(Value::Null);
                                map.insert((*key).to_string(), next);
                            }
                        }
                    }
                }
                entry.to_string()
            }
        })
        .collect();
    (lines, dropped)
}

// ---------- packing ----------

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn files_under(dir: &Path, skip_rebuilt: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if !(skip_rebuilt && REBUILT.contains(&name.as_str())) {
                    stack.push(path);
                }
            } else if kind.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Files and bytes a project would add to a package.
pub fn project_size(path: &Path) -> (u64, u64) {
    let files = files_under(path, true);
    let bytes = files
        .iter()
        .filter_map(|file| std::fs::metadata(file).ok())
        .map(|meta| meta.len())
        .sum();
    (files.len() as u64, bytes)
}

fn relative(path: &Path, base: &Path) -> Option<String> {
    let path = plain(&path.to_string_lossy());
    let base = plain(&base.to_string_lossy());
    let lower = path.to_lowercase();
    let prefix = format!("{}\\", base.trim_end_matches('\\').to_lowercase());
    lower
        .starts_with(&prefix)
        .then(|| path[prefix.len()..].replace('\\', "/"))
}

/// The newest `state_<n>.sqlite` in a Codex folder.
pub fn codex_db(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let number: u32 = name
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse()
                .ok()?;
            Some((number, entry.path()))
        })
        .max_by_key(|(number, _)| *number)
        .map(|(_, path)| path)
}

fn row_json(row: &rusqlite::Row, names: &[String]) -> Map<String, Value> {
    use rusqlite::types::ValueRef;
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let value = match row.get_ref(i) {
                Ok(ValueRef::Integer(v)) => Value::from(v),
                Ok(ValueRef::Real(v)) => Value::from(v),
                Ok(ValueRef::Text(v)) => Value::from(String::from_utf8_lossy(v).into_owned()),
                _ => Value::Null,
            };
            (name.clone(), value)
        })
        .collect()
}

fn select_rows(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    ids: &[String],
) -> Vec<Map<String, Value>> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |_| Ok(()),
        )
        .is_ok();
    if !exists || ids.is_empty() {
        return Vec::new();
    }
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!("SELECT * FROM \"{table}\" WHERE \"{column}\" IN ({marks})");
    let Ok(mut statement) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|n| n.to_string())
        .collect();
    statement
        .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
            Ok(row_json(row, &names))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

/// Where a Codex rollout lives under the Codex folder: as it is there, or by the date in its name.
fn rollout_relative(path: &Path, root: &Path) -> String {
    if let Some(rel) = relative(path, root) {
        return rel;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let date = name.strip_prefix("rollout-").unwrap_or_default();
    let (y, m, d) = (date.get(0..4), date.get(5..7), date.get(8..10));
    match (y, m, d) {
        (Some(y), Some(m), Some(d)) => format!("sessions/{y}/{m}/{d}/{name}"),
        _ => format!("sessions/imported/{name}"),
    }
}

/// Codex desktop's own output for a thread: `visualizations/<y>/<m>/<d>/<thread id>/`.
fn codex_artifacts(root: &Path, thread: &str) -> Vec<PathBuf> {
    let base = root.join("visualizations");
    let mut out = Vec::new();
    for year in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        for month in std::fs::read_dir(year.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            for day in std::fs::read_dir(month.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let folder = day.path().join(thread);
                if folder.is_dir() {
                    out.extend(files_under(&folder, false));
                }
            }
        }
    }
    out
}

struct Packer<W: Write + std::io::Seek> {
    zip: zip::ZipWriter<W>,
    count: u64,
}

impl<W: Write + std::io::Seek> Packer<W> {
    fn options() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true)
    }
    fn file(&mut self, entry: &str, source: &Path) -> Result<(), String> {
        let mut input = File::open(source).map_err(|e| format!("{}：{e}", source.display()))?;
        self.zip
            .start_file(entry, Self::options())
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut input, &mut self.zip).map_err(|e| e.to_string())?;
        self.count += 1;
        Ok(())
    }
    fn bytes(&mut self, entry: &str, data: &[u8]) -> Result<(), String> {
        self.zip
            .start_file(entry, Self::options())
            .map_err(|e| e.to_string())?;
        self.zip.write_all(data).map_err(|e| e.to_string())?;
        self.count += 1;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub sessions: usize,
    pub skipped: usize,
    pub projects: usize,
    pub files: u64,
    pub bytes: u64,
}

/// Packs these sessions (Codex and Claude Code; others are counted as skipped) and the
/// projects in `include` into a zip at `dest`. `progress` hears `(what, done, total)`.
pub fn export(
    sessions: &[Session],
    roots: &Roots,
    include: &[String],
    dest: &Path,
    progress: &dyn Fn(&str, u64, u64),
) -> Result<ExportResult, String> {
    let codex_root = PathBuf::from(&roots.codex);
    let claude_root = PathBuf::from(&roots.claude);
    let partial = dest.with_extension("zip.part");
    let file = File::create(&partial).map_err(|e| e.to_string())?;
    let mut packer = Packer {
        zip: zip::ZipWriter::new(std::io::BufWriter::new(file)),
        count: 0,
    };
    let chosen: Vec<&Session> = sessions.iter().filter(|s| supported(s.agent)).collect();
    let codex_conn = codex_db(&codex_root).and_then(|db| {
        rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
    });
    let index_lines: Vec<String> = std::fs::read_to_string(codex_root.join("session_index.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let mut packed = Vec::new();
    for (n, session) in chosen.iter().enumerate() {
        progress("sessions", n as u64, chosen.len() as u64);
        let id = session.native_id.clone();
        let mut files = Vec::new();
        let mut item = PackedSession {
            agent: session.agent,
            id: id.clone(),
            title: session.title.clone(),
            cwd: plain(&session.project.path),
            updated_at: session.updated_at,
            files: Vec::new(),
            threads: Vec::new(),
            tables: BTreeMap::new(),
            edges: Vec::new(),
            index_lines: Vec::new(),
        };
        match session.agent {
            Agent::Codex => {
                let mut ids = vec![id.clone()];
                let mut rollouts = vec![PathBuf::from(&session.path)];
                for child in &session.children {
                    ids.push(child.id.trim_start_matches("codex:").to_string());
                    rollouts.push(PathBuf::from(&child.path));
                }
                for path in rollouts.iter().filter(|p| p.is_file()) {
                    let rel = rollout_relative(path, &codex_root);
                    let entry = format!("sessions/codex/{id}/{rel}");
                    packer.file(&entry, path)?;
                    files.push(PackedFile {
                        entry,
                        role: "rollout".into(),
                        relative: rel,
                    });
                }
                for path in codex_artifacts(&codex_root, &id) {
                    if let Some(rel) = relative(&path, &codex_root) {
                        let entry = format!("sessions/codex/{id}/{rel}");
                        packer.file(&entry, &path)?;
                        files.push(PackedFile {
                            entry,
                            role: "artifact".into(),
                            relative: rel,
                        });
                    }
                }
                if let Some(conn) = &codex_conn {
                    item.threads = select_rows(conn, "threads", "id", &ids);
                    for table in ["thread_dynamic_tools", "thread_attachments"] {
                        let rows = select_rows(conn, table, "thread_id", &ids);
                        if !rows.is_empty() {
                            item.tables.insert(table.to_string(), rows);
                        }
                    }
                    item.edges = select_rows(conn, "thread_spawn_edges", "parent_thread_id", &ids)
                        .into_iter()
                        .filter_map(|row| {
                            Some((
                                row.get("parent_thread_id")?.as_str()?.to_string(),
                                row.get("child_thread_id")?.as_str()?.to_string(),
                                row.get("status")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                            ))
                        })
                        .collect();
                }
                item.index_lines = index_lines
                    .iter()
                    .filter(|line| {
                        serde_json::from_str::<Value>(line)
                            .ok()
                            .and_then(|v| {
                                v.get("id")
                                    .and_then(Value::as_str)
                                    .map(|v| ids.iter().any(|id| id == v))
                            })
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect();
            }
            Agent::Claude => {
                let transcript = PathBuf::from(&session.path);
                let entry = format!("sessions/claude/{id}/{id}.jsonl");
                packer.file(&entry, &transcript)?;
                files.push(PackedFile {
                    entry,
                    role: "transcript".into(),
                    relative: format!("{id}.jsonl"),
                });
                if let Some(side) = transcript
                    .parent()
                    .map(|p| p.join(&id))
                    .filter(|p| p.is_dir())
                {
                    for path in files_under(&side, false) {
                        if let Some(rel) = relative(&path, &side) {
                            let entry = format!("sessions/claude/{id}/side/{rel}");
                            packer.file(&entry, &path)?;
                            files.push(PackedFile {
                                entry,
                                role: "side".into(),
                                relative: rel,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        item.files = files;
        packed.push(item);
    }
    let mut projects = Vec::new();
    let wanted: Vec<String> = include.iter().map(|p| plain(p)).collect();
    let all_paths: Vec<String> = {
        let mut seen = Vec::<String>::new();
        for session in &packed {
            if !session.cwd.is_empty() && !seen.iter().any(|p| p.eq_ignore_ascii_case(&session.cwd))
            {
                seen.push(session.cwd.clone());
            }
        }
        seen
    };
    for (n, path) in all_paths.iter().enumerate() {
        let included =
            wanted.iter().any(|w| w.eq_ignore_ascii_case(path)) && Path::new(path).is_dir();
        let prefix = format!("projects/{n}/");
        let (mut count, mut bytes) = (0, 0);
        if included {
            let files = files_under(Path::new(path), true);
            let total = files.len() as u64;
            for (i, file) in files.iter().enumerate() {
                if i % 50 == 0 {
                    progress("project", i as u64, total);
                }
                if let Some(rel) = relative(file, Path::new(path)) {
                    packer.file(&format!("{prefix}{rel}"), file)?;
                    count += 1;
                    bytes += std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                }
            }
        }
        projects.push(PackedProject {
            path: path.clone(),
            name: Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            included,
            files: count,
            bytes,
            entry_prefix: prefix,
        });
    }
    let mut accounts = BTreeMap::new();
    if let Some(account) = codex_account(&codex_root) {
        accounts.insert("codex".to_string(), account);
    }
    if let Some(account) = claude_account(&claude_root) {
        accounts.insert("claude".to_string(), account);
    }
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        created_at: now(),
        machine: std::env::var("COMPUTERNAME").unwrap_or_default(),
        accounts,
        roots: BTreeMap::from([
            ("codex".to_string(), plain(&roots.codex)),
            ("claude".to_string(), plain(&roots.claude)),
        ]),
        sessions: packed,
        projects,
    };
    let text = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    packer.bytes(MANIFEST, &text)?;
    let files = packer.count;
    packer.zip.finish().map_err(|e| e.to_string())?;
    if dest.exists() {
        std::fs::remove_file(dest).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&partial, dest).map_err(|e| e.to_string())?;
    Ok(ExportResult {
        sessions: manifest.sessions.len(),
        skipped: sessions.len() - manifest.sessions.len(),
        projects: manifest.projects.iter().filter(|p| p.included).count(),
        files,
        bytes: std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0),
    })
}

// ---------- unpacking ----------

fn open(path: &Path) -> Result<zip::ZipArchive<File>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(file).map_err(|_| "E_TRANSFER_FORMAT".to_string())
}

pub fn read_manifest(path: &Path) -> Result<Manifest, String> {
    let mut archive = open(path)?;
    let mut text = String::new();
    archive
        .by_name(MANIFEST)
        .map_err(|_| "E_TRANSFER_FORMAT".to_string())?
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|_| "E_TRANSFER_FORMAT".to_string())?;
    if manifest.format != FORMAT || manifest.version > VERSION {
        return Err("E_TRANSFER_FORMAT".into());
    }
    Ok(manifest)
}

fn entry_text(archive: &mut zip::ZipArchive<File>, entry: &str) -> Result<String, String> {
    let mut text = String::new();
    archive
        .by_name(entry)
        .map_err(|e| e.to_string())?
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

fn entry_to(archive: &mut zip::ZipArchive<File>, entry: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut input = archive.by_name(entry).map_err(|e| e.to_string())?;
    let mut output = File::create(dest).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    Ok(())
}

/// A relative path from a package, kept inside the folder it is meant for.
fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for part in relative.split(['/', '\\']).filter(|p| !p.is_empty()) {
        if part == ".." || part == "." || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCheck {
    pub agent: String,
    /// The package says who packed it, and this machine says who is signed in, and they agree.
    pub same: bool,
    pub packed: bool,
    pub here: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSession {
    pub agent: Agent,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub updated_at: u64,
    /// Already on this machine: it will be left as it is.
    pub present: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewProject {
    pub path: String,
    pub name: String,
    pub included: bool,
    pub files: u64,
    pub bytes: u64,
    /// The same path exists here.
    pub exists_here: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub machine: String,
    pub created_at: u64,
    pub sessions: Vec<PreviewSession>,
    pub projects: Vec<PreviewProject>,
    pub accounts: Vec<AccountCheck>,
    pub codex_running: bool,
    pub codex_ready: bool,
}

fn codex_present(conn: Option<&rusqlite::Connection>, id: &str) -> bool {
    conn.is_some_and(|conn| {
        conn.query_row("SELECT 1 FROM threads WHERE id=?1", [id], |_| Ok(()))
            .is_ok()
    })
}

pub fn preview(path: &Path, roots: &Roots, codex_running: bool) -> Result<Preview, String> {
    let manifest = read_manifest(path)?;
    let codex_root = PathBuf::from(&roots.codex);
    let claude_root = PathBuf::from(&roots.claude);
    let db = codex_db(&codex_root);
    let conn = db.as_ref().and_then(|db| {
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
    });
    let here = BTreeMap::from([
        ("codex", codex_account(&codex_root)),
        ("claude", claude_account(&claude_root)),
    ]);
    let mut agents: Vec<&str> = manifest
        .sessions
        .iter()
        .map(|s| agent_key(s.agent))
        .collect();
    agents.sort();
    agents.dedup();
    let accounts = agents
        .into_iter()
        .map(|agent| {
            let packed = manifest.accounts.get(agent);
            let mine = here.get(agent).cloned().flatten();
            AccountCheck {
                agent: agent.to_string(),
                same: packed.is_some() && packed == mine.as_ref(),
                packed: packed.is_some(),
                here: mine.is_some(),
            }
        })
        .collect();
    let sessions = manifest
        .sessions
        .iter()
        .map(|s| PreviewSession {
            agent: s.agent,
            id: s.id.clone(),
            title: s.title.clone(),
            cwd: s.cwd.clone(),
            updated_at: s.updated_at,
            present: match s.agent {
                Agent::Codex => codex_present(conn.as_ref(), &s.id),
                Agent::Claude => claude_present(&claude_root, &s.id),
                _ => false,
            },
        })
        .collect();
    let projects = manifest
        .projects
        .iter()
        .map(|p| PreviewProject {
            path: p.path.clone(),
            name: p.name.clone(),
            included: p.included,
            files: p.files,
            bytes: p.bytes,
            exists_here: Path::new(&p.path).is_dir(),
        })
        .collect();
    Ok(Preview {
        machine: manifest.machine,
        created_at: manifest.created_at,
        sessions,
        projects,
        accounts,
        codex_running,
        codex_ready: db.is_some(),
    })
}

fn claude_present(root: &Path, id: &str) -> bool {
    std::fs::read_dir(root.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|dir| dir.path().join(format!("{id}.jsonl")).is_file())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTarget {
    /// The project's path on the packing machine.
    pub from: String,
    /// Where it is (or goes) here.
    pub to: String,
    /// Unpack the packaged project files there (files already there are kept).
    pub extract: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub agent: Agent,
    pub id: String,
    pub title: String,
    pub cwd: String,
    /// The command that continues it.
    pub resume: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: Vec<Imported>,
    /// Titles already on this machine, left as they were.
    pub present: Vec<String>,
    /// Agents whose account-bound content was left out (another account here).
    pub stripped: Vec<String>,
    pub dropped: usize,
    pub extracted: u64,
    /// Project files that were already there and kept.
    pub kept: u64,
    /// Where Codex's database and index were copied before anything was written.
    pub backup: Option<String>,
}

/// Old machine's paths to this machine's: each project as mapped, and the agents' folders.
fn moves_for(
    manifest: &Manifest,
    targets: &[ProjectTarget],
    roots: &Roots,
) -> Vec<(String, String)> {
    let mut moves: Vec<(String, String)> = targets
        .iter()
        .filter(|t| !t.from.is_empty() && !t.to.is_empty() && !t.from.eq_ignore_ascii_case(&t.to))
        .map(|t| (plain(&t.from), plain(&t.to)))
        .collect();
    for (agent, here) in [
        ("codex", plain(&roots.codex)),
        ("claude", plain(&roots.claude)),
    ] {
        if let Some(there) = manifest.roots.get(agent) {
            if !there.is_empty() && !there.eq_ignore_ascii_case(&here) {
                moves.push((there.clone(), here));
            }
        }
    }
    // Longest first, so a project inside another is moved by its own mapping.
    moves.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    moves
}

fn sql_value(value: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as Sql;
    match value {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(Sql::Integer)
            .or_else(|| n.as_f64().map(Sql::Real))
            .unwrap_or(Sql::Null),
        Value::String(s) => Sql::Text(s.clone()),
        other => Sql::Text(other.to_string()),
    }
}

fn columns_of(conn: &rusqlite::Connection, table: &str) -> Vec<(String, bool, bool)> {
    let Ok(mut statement) = conn.prepare(&format!("PRAGMA table_info(\"{table}\")")) else {
        return Vec::new();
    };
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, i64>(3)? != 0,
                row.get::<_, Option<String>>(4)?.is_some(),
            ))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

fn insert(
    conn: &rusqlite::Connection,
    table: &str,
    row: &Map<String, Value>,
) -> Result<(), String> {
    let columns = columns_of(conn, table);
    if columns.is_empty() {
        return Ok(());
    }
    let fields: Vec<&String> = row
        .keys()
        .filter(|k| columns.iter().any(|(name, _, _)| name == *k))
        .collect();
    if let Some((name, _, _)) = columns
        .iter()
        .find(|(name, required, default)| *required && !*default && !fields.contains(&name))
    {
        return Err(format!("E_CODEX_SCHEMA:{name}"));
    }
    let sql = format!(
        "INSERT OR IGNORE INTO \"{table}\" ({}) VALUES ({})",
        fields
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(","),
        vec!["?"; fields.len()].join(",")
    );
    conn.execute(
        &sql,
        rusqlite::params_from_iter(fields.iter().map(|f| sql_value(&row[*f]))),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Brings the package into this machine's Codex and Claude Code. `backup_dir` receives a copy
/// of Codex's database and index before either is written.
pub fn import(
    path: &Path,
    targets: &[ProjectTarget],
    roots: &Roots,
    codex_running: bool,
    backup_dir: &Path,
    progress: &dyn Fn(&str, u64, u64),
) -> Result<ImportResult, String> {
    let manifest = read_manifest(path)?;
    let mut archive = open(path)?;
    let codex_root = PathBuf::from(plain(&roots.codex));
    let claude_root = PathBuf::from(plain(&roots.claude));
    let moves = moves_for(&manifest, targets, roots);
    let mut result = ImportResult::default();
    let codex: Vec<&PackedSession> = manifest
        .sessions
        .iter()
        .filter(|s| s.agent == Agent::Codex)
        .collect();
    // Nothing is written while Codex runs, or before it has made its database here.
    if !codex.is_empty() {
        if codex_running {
            return Err("E_CODEX_RUNNING".into());
        }
        codex_db(&codex_root).ok_or("E_CODEX_NOT_READY")?;
    }

    // Projects first, so the conversations land where their folders are.
    for target in targets.iter().filter(|t| t.extract) {
        let Some(project) = manifest
            .projects
            .iter()
            .find(|p| p.included && p.path.eq_ignore_ascii_case(&target.from))
        else {
            continue;
        };
        let to = PathBuf::from(plain(&target.to));
        let names: Vec<String> = archive
            .file_names()
            .filter(|name| name.starts_with(&project.entry_prefix))
            .map(str::to_string)
            .collect();
        for (i, name) in names.iter().enumerate() {
            if i % 50 == 0 {
                progress("project", i as u64, names.len() as u64);
            }
            let Some(dest) = safe_join(&to, &name[project.entry_prefix.len()..]) else {
                continue;
            };
            if dest.exists() {
                result.kept += 1;
                continue;
            }
            entry_to(&mut archive, name, &dest)?;
            result.extracted += 1;
        }
    }

    let here_codex = codex_account(&codex_root);
    let here_claude = claude_account(&claude_root);
    let strip_for = |agent: &str| {
        let mine = if agent == "codex" {
            &here_codex
        } else {
            &here_claude
        };
        let packed = manifest.accounts.get(agent);
        !(packed.is_some() && packed == mine.as_ref())
    };
    let mapped = |cwd: &str| remap_text(cwd, &moves);

    // Codex: refuse while it runs; back up; write rollouts, then rows, all or nothing per thread.
    if !codex.is_empty() {
        let db = codex_db(&codex_root).ok_or("E_CODEX_NOT_READY")?;
        let conn = rusqlite::Connection::open(&db).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(backup_dir).map_err(|e| e.to_string())?;
        let copy = backup_dir.join(db.file_name().unwrap_or_default());
        conn.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()])
            .map_err(|e| e.to_string())?;
        let index = codex_root.join("session_index.jsonl");
        if index.is_file() {
            std::fs::copy(&index, backup_dir.join("session_index.jsonl"))
                .map_err(|e| e.to_string())?;
        }
        result.backup = Some(backup_dir.to_string_lossy().into_owned());
        let strip = strip_for("codex");
        if strip {
            result.stripped.push("codex".into());
        }
        for (n, session) in codex.iter().enumerate() {
            progress("codex", n as u64, codex.len() as u64);
            if codex_present(Some(&conn), &session.id) {
                result.present.push(session.title.clone());
                continue;
            }
            let mut written = Vec::new();
            let outcome = (|| -> Result<(), String> {
                for file in &session.files {
                    let Some(dest) = safe_join(&codex_root, &file.relative) else {
                        continue;
                    };
                    if dest.exists() {
                        continue;
                    }
                    if file.role == "rollout" {
                        let (lines, dropped) =
                            rewrite_codex(&entry_text(&mut archive, &file.entry)?, &moves, strip);
                        result.dropped += dropped;
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(&dest, lines.join("\n") + "\n")
                            .map_err(|e| e.to_string())?;
                    } else {
                        entry_to(&mut archive, &file.entry, &dest)?;
                    }
                    written.push(dest);
                }
                conn.execute_batch("BEGIN IMMEDIATE")
                    .map_err(|e| e.to_string())?;
                let rows = (|| -> Result<(), String> {
                    for row in &session.threads {
                        let mut row = row.clone();
                        let id = row
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if let Some(file) = session
                            .files
                            .iter()
                            .find(|f| f.role == "rollout" && f.relative.contains(&id))
                        {
                            if let Some(dest) = safe_join(&codex_root, &file.relative) {
                                row.insert(
                                    "rollout_path".into(),
                                    Value::from(dest.to_string_lossy().into_owned()),
                                );
                            }
                        }
                        let mut value = Value::Object(row);
                        if let Some(map) = value.as_object_mut() {
                            if let Some(cwd) = map.get("cwd").and_then(Value::as_str).map(mapped) {
                                map.insert("cwd".into(), Value::from(cwd));
                            }
                            for key in [
                                "project_id",
                                "thread_section_id",
                                "section_position",
                                "section_entered_at_ms",
                            ] {
                                map.insert(key.into(), Value::Null);
                            }
                            if strip {
                                map.insert("creator_user_id".into(), Value::Null);
                                map.insert("creator_account_id".into(), Value::Null);
                            }
                            // The old machine's full-access settings do not come along.
                            if map.get("approval_mode").and_then(Value::as_str) == Some("never") {
                                map.insert("approval_mode".into(), Value::from("on-request"));
                            }
                            if map
                                .get("sandbox_policy")
                                .and_then(Value::as_str)
                                .is_some_and(|p| p.contains("danger-full-access"))
                            {
                                map.insert(
                                    "sandbox_policy".into(),
                                    Value::from(r#"{"type":"workspace-write"}"#),
                                );
                            }
                        }
                        insert(&conn, "threads", value.as_object().expect("an object"))?;
                    }
                    for (table, rows) in &session.tables {
                        for row in rows {
                            let mut value = Value::Object(row.clone());
                            remap_value(&mut value, &moves);
                            insert(&conn, table, value.as_object().expect("an object"))?;
                        }
                    }
                    for (parent, child, status) in &session.edges {
                        let mut row = Map::new();
                        row.insert("parent_thread_id".into(), Value::from(parent.clone()));
                        row.insert("child_thread_id".into(), Value::from(child.clone()));
                        row.insert("status".into(), Value::from(status.clone()));
                        insert(&conn, "thread_spawn_edges", &row)?;
                    }
                    Ok(())
                })();
                match rows {
                    Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e.to_string()),
                    Err(error) => {
                        let _ = conn.execute_batch("ROLLBACK");
                        Err(error)
                    }
                }
            })();
            if let Err(error) = outcome {
                for file in written {
                    let _ = std::fs::remove_file(file);
                }
                return Err(error);
            }
            if !session.index_lines.is_empty() {
                let mut index = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(codex_root.join("session_index.jsonl"))
                    .map_err(|e| e.to_string())?;
                for line in &session.index_lines {
                    writeln!(index, "{line}").map_err(|e| e.to_string())?;
                }
            }
            let cwd = mapped(&session.cwd);
            result.imported.push(Imported {
                agent: Agent::Codex,
                id: session.id.clone(),
                title: session.title.clone(),
                resume: format!("codex resume {} -C \"{cwd}\"", session.id),
                cwd,
            });
        }
    }

    // Claude Code: each transcript under the folder its new working directory names.
    let claude: Vec<&PackedSession> = manifest
        .sessions
        .iter()
        .filter(|s| s.agent == Agent::Claude)
        .collect();
    if !claude.is_empty() {
        let strip = strip_for("claude");
        if strip {
            result.stripped.push("claude".into());
        }
        for (n, session) in claude.iter().enumerate() {
            progress("claude", n as u64, claude.len() as u64);
            if claude_present(&claude_root, &session.id) {
                result.present.push(session.title.clone());
                continue;
            }
            let cwd = mapped(&session.cwd);
            let folder = claude_root.join("projects").join(claude_project_dir(&cwd));
            for file in &session.files {
                let base = if file.role == "side" {
                    folder.join(&session.id)
                } else {
                    folder.clone()
                };
                let Some(dest) = safe_join(&base, &file.relative) else {
                    continue;
                };
                if dest.exists() {
                    continue;
                }
                if file.relative.ends_with(".jsonl") {
                    let (lines, dropped) =
                        rewrite_claude(&entry_text(&mut archive, &file.entry)?, &moves, strip);
                    result.dropped += dropped;
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&dest, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
                } else {
                    entry_to(&mut archive, &file.entry, &dest)?;
                }
            }
            result.imported.push(Imported {
                agent: Agent::Claude,
                id: session.id.clone(),
                title: session.title.clone(),
                resume: format!("cd \"{cwd}\"; claude --resume {}", session.id),
                cwd,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn a_path_moves_in_every_spelling_but_never_into_a_longer_name() {
        let m = moves(&[(r"E:\app", r"D:\work\app")]);
        assert_eq!(remap_text(r"E:\app", &m), r"D:\work\app");
        assert_eq!(
            remap_text(r"\\?\E:\app\src\main.rs", &m),
            r"D:\work\app\src\main.rs"
        );
        assert_eq!(
            remap_text("cd E:/app/web && ls", &m),
            "cd D:/work/app/web && ls"
        );
        assert_eq!(
            remap_text(r"E:\app2\x and E:\app-old", &m),
            r"E:\app2\x and E:\app-old"
        );
        assert_eq!(remap_text(r"see E:\app.", &m), r"see E:\app.");
        let m = moves(&[(r"E:\Vibe\12音乐", r"D:\work\12音乐")]);
        assert_eq!(
            remap_text("file:///E:/Vibe/12%E9%9F%B3%E4%B9%90/src", &m),
            "file:///D:/work/12%E9%9F%B3%E4%B9%90/src"
        );
        assert_eq!(
            remap_text(r"out: E:\\Vibe\\12音乐\\x", &m),
            r"out: D:\\work\\12音乐\\x"
        );
    }

    #[test]
    fn claude_names_a_project_folder_after_its_path() {
        assert_eq!(
            claude_project_dir(r"D:\Projects\rust\envswitch"),
            "D--Projects-rust-envswitch"
        );
        assert_eq!(
            claude_project_dir(r"E:\VibeCoding\01音乐"),
            "E--VibeCoding-01--"
        );
    }

    #[test]
    fn codex_lines_move_and_lose_encrypted_reasoning_only_for_another_account() {
        let text = [
            r#"{"type":"session_meta","payload":{"id":"t1","cwd":"E:\\app"}}"#,
            r#"{"type":"turn_context","payload":{"cwd":"E:\\app","workspace_roots":["E:\\app"]}}"#,
            r#"{"type":"response_item","payload":{"type":"reasoning","encrypted_content":"x","summary":[]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done in E:\\app\\src"}]}}"#,
            r#"{"type":"compacted","payload":{"message":"","replacement_history":[{"type":"compaction","encrypted_content":"y"},{"type":"message"}]}}"#,
        ]
        .join("\n");
        let m = moves(&[(r"E:\app", r"D:\app")]);
        let (same, dropped) = rewrite_codex(&text, &m, false);
        assert_eq!((same.len(), dropped), (5, 0));
        assert!(same[0].contains(r#""cwd":"D:\\app""#));
        assert!(same[1].contains(r#""workspace_roots":["D:\\app"]"#));
        assert!(same[3].contains(r"D:\\app\\src"));
        let (other, dropped) = rewrite_codex(&text, &m, true);
        assert_eq!((other.len(), dropped), (4, 2));
        assert!(!other.iter().any(|line| line.contains("encrypted_content")));
    }

    #[test]
    fn claude_lines_lose_signed_thinking_and_stay_one_chain() {
        let text = [
            r#"{"type":"user","uuid":"u1","parentUuid":null,"cwd":"E:\\app","message":{"content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","cwd":"E:\\app","message":{"content":[{"type":"thinking","thinking":"..","signature":"s"}]}}"#,
            r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","cwd":"E:\\app","message":{"content":[{"type":"thinking","thinking":"..","signature":"s"},{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"last-prompt","leafUuid":"a2"}"#,
        ]
        .join("\n");
        let m = moves(&[(r"E:\app", r"D:\app")]);
        let (same, dropped) = rewrite_claude(&text, &m, false);
        assert_eq!((same.len(), dropped), (4, 0));
        assert!(same[1].contains("signature"));
        let (other, dropped) = rewrite_claude(&text, &m, true);
        assert_eq!((other.len(), dropped), (3, 2));
        assert!(!other.iter().any(|line| line.contains("signature")));
        let second: Value = serde_json::from_str(&other[1]).unwrap();
        assert_eq!(second["uuid"], "a2");
        assert_eq!(
            second["parentUuid"], "u1",
            "the dropped line's child points at its parent"
        );
        assert_eq!(second["cwd"], r"D:\app");
    }

    fn codex_fixture(root: &Path, id: &str, cwd: &str) -> PathBuf {
        let rollout = root.join(format!(
            "sessions/2026/10/02/rollout-2026-10-02T17-51-52-{id}.jsonl"
        ));
        std::fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        std::fs::write(
            &rollout,
            format!(
                "{}\n{}\n",
                serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":cwd}}),
                serde_json::json!({"type":"response_item","payload":{"type":"reasoning","encrypted_content":"x"}}),
            ),
        )
        .unwrap();
        let conn = rusqlite::Connection::open(root.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, cwd TEXT NOT NULL, title TEXT NOT NULL, approval_mode TEXT NOT NULL, sandbox_policy TEXT NOT NULL, project_id TEXT, creator_account_id TEXT, archived INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS thread_spawn_edges (parent_thread_id TEXT NOT NULL, child_thread_id TEXT NOT NULL PRIMARY KEY, status TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads (id, rollout_path, cwd, title, approval_mode, sandbox_policy, project_id, creator_account_id) VALUES (?1, ?2, ?3, 'Plan', 'never', '{\"type\":\"danger-full-access\"}', 'p1', 'acct')",
            rusqlite::params![id, rollout.to_string_lossy(), cwd],
        )
        .unwrap();
        rollout
    }

    fn session(agent: Agent, id: &str, path: &Path, cwd: &str) -> Session {
        serde_json::from_value(serde_json::json!({
            "id": format!("{}:{id}", agent_key(agent)), "agent": agent, "nativeId": id, "title": "Plan",
            "titleSource": "client", "project": {"key": cwd.to_lowercase(), "name": "app", "path": cwd, "exists": true},
            "client": "terminal", "createdAt": 1, "updatedAt": 2, "archived": false, "pinned": false, "status": "active",
            "children": [], "bytes": 10, "path": path.to_string_lossy(), "inDesktopIndex": false, "parentMissing": false,
            "favorite": false, "summary": null, "summaryStale": false, "summaryBy": "", "summaryAt": 0,
        }))
        .unwrap()
    }

    #[test]
    fn the_same_account_on_both_machines_takes_everything_as_it_is() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let (a_codex, b_codex) = (a.path().join(".codex"), b.path().join(".codex"));
        let rollout = codex_fixture(&a_codex, "t1", r"E:\app");
        codex_fixture(&b_codex, "other", r"C:\elsewhere");
        for root in [&a_codex, &b_codex] {
            std::fs::write(
                root.join("auth.json"),
                r#"{"tokens":{"account_id":"same"}}"#,
            )
            .unwrap();
        }
        let roots = |codex: &Path| Roots {
            codex: codex.to_string_lossy().into(),
            claude: String::new(),
            ..Roots::default()
        };
        let package = a.path().join("move.zip");
        export(
            &[session(Agent::Codex, "t1", &rollout, r"E:\app")],
            &roots(&a_codex),
            &[],
            &package,
            &|_, _, _| {},
        )
        .unwrap();
        assert!(preview(&package, &roots(&b_codex), false).unwrap().accounts[0].same);
        let done = import(
            &package,
            &[],
            &roots(&b_codex),
            false,
            &b.path().join("backups"),
            &|_, _, _| {},
        )
        .unwrap();
        assert!(done.stripped.is_empty());
        let conn = rusqlite::Connection::open(b_codex.join("state_5.sqlite")).unwrap();
        let path: String = conn
            .query_row("SELECT rollout_path FROM threads WHERE id='t1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(std::fs::read_to_string(path)
            .unwrap()
            .contains("encrypted_content"));
    }

    #[test]
    fn a_package_carries_sessions_and_a_project_to_another_machine() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let (a_codex, a_claude) = (a.path().join(".codex"), a.path().join(".claude"));
        let (b_codex, b_claude) = (b.path().join(".codex"), b.path().join(".claude"));
        let project = a.path().join("app");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("node_modules/x")).unwrap();
        std::fs::write(project.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(project.join("node_modules/x/big.js"), "x").unwrap();
        let cwd = project.to_string_lossy().into_owned();
        let rollout = codex_fixture(&a_codex, "t1", &cwd);
        let claude_dir = a_claude.join("projects").join(claude_project_dir(&cwd));
        std::fs::create_dir_all(claude_dir.join("c1/subagents")).unwrap();
        let transcript = claude_dir.join("c1.jsonl");
        std::fs::write(&transcript, format!("{}\n", serde_json::json!({"type":"user","uuid":"u1","parentUuid":null,"cwd":cwd,"message":{"content":[{"type":"text","text":"hi"}]}}))).unwrap();
        std::fs::write(claude_dir.join("c1/subagents/agent-1.jsonl"), "{}\n").unwrap();
        // Codex is signed in on both, with different accounts.
        std::fs::write(
            a_codex.join("auth.json"),
            r#"{"tokens":{"account_id":"one"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(&b_codex).unwrap();
        std::fs::write(
            b_codex.join("auth.json"),
            r#"{"tokens":{"account_id":"two"}}"#,
        )
        .unwrap();
        codex_fixture(&b_codex, "other", r"C:\elsewhere");

        let a_roots = Roots {
            codex: a_codex.to_string_lossy().into(),
            claude: a_claude.to_string_lossy().into(),
            ..Roots::default()
        };
        let b_roots = Roots {
            codex: b_codex.to_string_lossy().into(),
            claude: b_claude.to_string_lossy().into(),
            ..Roots::default()
        };
        let package = a.path().join("move.zip");
        let sessions = [
            session(Agent::Codex, "t1", &rollout, &cwd),
            session(Agent::Claude, "c1", &transcript, &cwd),
        ];
        let packed = export(
            &sessions,
            &a_roots,
            std::slice::from_ref(&cwd),
            &package,
            &|_, _, _| {},
        )
        .unwrap();
        assert_eq!((packed.sessions, packed.projects), (2, 1));

        let preview = preview(&package, &b_roots, false).unwrap();
        assert!(preview.sessions.iter().all(|s| !s.present));
        assert!(
            !preview
                .accounts
                .iter()
                .find(|a| a.agent == "codex")
                .unwrap()
                .same
        );
        assert_eq!(preview.projects[0].files, 1, "node_modules stays behind");

        let new_place = b.path().join("work").join("app");
        let targets = [ProjectTarget {
            from: cwd.clone(),
            to: new_place.to_string_lossy().into(),
            extract: true,
        }];
        let backups = b.path().join("backups");
        assert_eq!(
            import(&package, &targets, &b_roots, true, &backups, &|_, _, _| {}).unwrap_err(),
            "E_CODEX_RUNNING"
        );
        assert!(!new_place.exists(), "nothing is written while Codex runs");
        let done = import(&package, &targets, &b_roots, false, &backups, &|_, _, _| {}).unwrap();
        assert_eq!(done.imported.len(), 2);
        assert_eq!(done.extracted, 1);
        assert!(new_place.join("src/main.rs").is_file());
        assert!(done.stripped.contains(&"codex".to_string()));
        assert!(backups.join("state_5.sqlite").is_file());

        let conn = rusqlite::Connection::open(b_codex.join("state_5.sqlite")).unwrap();
        let (cwd_b, path_b, approval, sandbox, project_id, account): (String, String, String, String, Option<String>, Option<String>) = conn
            .query_row("SELECT cwd, rollout_path, approval_mode, sandbox_policy, project_id, creator_account_id FROM threads WHERE id='t1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .unwrap();
        assert_eq!(cwd_b, new_place.to_string_lossy());
        assert!(Path::new(&path_b).is_file());
        assert_eq!(
            (approval.as_str(), project_id, account),
            ("on-request", None, None)
        );
        assert!(sandbox.contains("workspace-write"));
        let written = std::fs::read_to_string(&path_b).unwrap();
        assert!(
            !written.contains("encrypted_content"),
            "another account: encrypted reasoning left out"
        );
        assert!(written.contains(
            &serde_json::to_string(&new_place.to_string_lossy()).unwrap()[1..]
                .trim_end_matches('"')
                .to_string()
        ));
        assert!(
            conn.query_row("SELECT 1 FROM threads WHERE id='other'", [], |_| Ok(()))
                .is_ok(),
            "the thread already there stays"
        );

        let b_transcript = b_claude
            .join("projects")
            .join(claude_project_dir(&new_place.to_string_lossy()))
            .join("c1.jsonl");
        assert!(b_transcript.is_file());
        assert!(b_transcript
            .parent()
            .unwrap()
            .join("c1/subagents/agent-1.jsonl")
            .is_file());

        // Imported again: everything is already there, nothing is written twice.
        let again = import(
            &package,
            &targets,
            &b_roots,
            false,
            &b.path().join("backups2"),
            &|_, _, _| {},
        )
        .unwrap();
        assert!(again.imported.is_empty());
        assert_eq!(again.present.len(), 2);
        assert_eq!(again.kept, 1);
    }
}
