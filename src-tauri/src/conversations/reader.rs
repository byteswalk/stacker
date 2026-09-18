use super::*;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read};

const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_LINE: u64 = 8 * 1024 * 1024;
pub const MAX_MESSAGES: usize = 100_000;

pub fn default_sources() -> Vec<Source> {
    let home = dirs::home_dir().unwrap_or_default();
    let configured = |name: &str, fallback: PathBuf| {
        crate::winenv::get_user_raw(name)
            .or_else(|| std::env::var(name).ok())
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or(fallback)
    };
    let mut result = vec![
        Source {
            id: "codex-local".into(),
            name: "Codex".into(),
            kind: Kind::Codex,
            root: display(&configured("CODEX_HOME", home.join(".codex"))),
            enabled: true,
        },
        Source {
            id: "claude-cli".into(),
            name: "Claude Code CLI".into(),
            kind: Kind::ClaudeCli,
            root: display(&configured("CLAUDE_CONFIG_DIR", home.join(".claude"))),
            enabled: true,
        },
    ];
    for (n, base) in [dirs::data_dir(), dirs::data_local_dir()]
        .into_iter()
        .flatten()
        .enumerate()
    {
        let root = base.join("Claude").join("local-agent-mode-sessions");
        if root.is_dir() {
            result.push(Source {
                id: format!("claude-desktop-{n}"),
                name: "Claude Desktop (local)".into(),
                kind: Kind::ClaudeDesktop,
                root: display(&root),
                enabled: true,
            });
        }
    }
    result
}

pub fn native_titles(source: &Source) -> BTreeMap<String, String> {
    let mut titles = BTreeMap::new();
    if source.kind != Kind::Codex {
        return titles;
    }
    let root = Path::new(&source.root);
    let Ok(entries) = fs::read_dir(root) else {
        return titles;
    };
    let mut databases: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("state_"))
                && p.extension().is_some_and(|e| e == "sqlite")
        })
        .collect();
    databases.sort();
    for path in databases {
        if checked_path(root, &path).is_err() {
            continue;
        }
        let Ok(db) = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) else {
            continue;
        };
        let _ = db.busy_timeout(Duration::from_secs(2));
        let Ok(mut stmt) = db.prepare("SELECT id,title FROM threads WHERE title IS NOT NULL")
        else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        else {
            continue;
        };
        for (id, title) in rows.flatten() {
            if !title.trim().is_empty() {
                titles.insert(id, title);
            }
        }
    }
    titles
}

pub fn display(p: &Path) -> String {
    p.to_string_lossy().trim_start_matches(r"\\?\").to_string()
}

pub fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

pub fn checked_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute() || !path.is_absolute() {
        return Err("E_PATH".into());
    }
    let relative = path.strip_prefix(root).map_err(|_| "E_PATH")?;
    if relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("E_PATH".into());
    }
    let mut current = root.to_path_buf();
    if is_link(&fs::symlink_metadata(&current).map_err(err)?) {
        return Err("E_LINK".into());
    }
    for c in relative.components() {
        current.push(c);
        if is_link(&fs::symlink_metadata(&current).map_err(err)?) {
            return Err("E_LINK".into());
        }
    }
    let real_root = fs::canonicalize(root).map_err(err)?;
    let real = fs::canonicalize(path).map_err(err)?;
    if !real.starts_with(real_root) {
        return Err("E_PATH".into());
    }
    Ok(real)
}

pub fn files(source: &Source, cancel: &AtomicBool) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let root = Path::new(&source.root);
    let mut stack = match source.kind {
        Kind::Codex => vec![root.join("sessions"), root.join("archived_sessions")],
        Kind::ClaudeCli => vec![root.join("projects")],
        _ => vec![root.to_path_buf()],
    };
    if !root.is_dir() {
        return Ok((vec![], vec![format!("{}: E_SOURCE_MISSING", source.name)]));
    }
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut dirs = 0;
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err("E_CANCELLED".into());
        }
        dirs += 1;
        if dirs > 50_000 || out.len() > 50_000 {
            warnings.push(format!("{}: E_SCAN_LIMIT", source.name));
            break;
        }
        if !dir.exists() {
            continue;
        }
        if checked_path(root, &dir).is_err() {
            warnings.push(format!("{}: E_LINK", display(&dir)));
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => {
                warnings.push(format!("{}: E_ACCESS", display(&dir)));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if is_link(&meta) {
                warnings.push(format!("{}: E_LINK", display(&path)));
                continue;
            }
            if meta.is_dir() {
                if path
                    .strip_prefix(root)
                    .map(|p| p.components().count())
                    .unwrap_or(100)
                    < 32
                {
                    stack.push(path);
                } else {
                    warnings.push(format!("{}: E_SCAN_LIMIT", display(&path)));
                }
            } else if path.file_name().is_some_and(|n| n != "manifest.json")
                && path.extension().is_some_and(|e| {
                    e == if source.kind == Kind::Import {
                        "json"
                    } else {
                        "jsonl"
                    }
                })
            {
                out.push(path);
            }
        }
    }
    Ok((out, warnings))
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn content(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                if let Some(s) = p.as_str() {
                    return Some(s.to_string());
                }
                match str_at(p, "type") {
                    "text" | "input_text" | "output_text" => {
                        p.get("text").and_then(Value::as_str).map(str::to_owned)
                    }
                    "tool_result" => Some(content(&p["content"])),
                    "tool_use" => Some(format!("{}\n{}", str_at(p, "name"), p["input"])),
                    "image" | "input_image" => Some("[image attachment]".into()),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub fn quick_fingerprint(path: &Path) -> Result<String, String> {
    let m = fs::metadata(path).map_err(err)?;
    let modified = m
        .modified()
        .map_err(err)?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_nanos();
    Ok(format!("{}:{modified}", m.len()))
}

pub fn read(
    source: &Source,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<(Conversation, Vec<Message>), String> {
    let real = checked_path(Path::new(&source.root), path)?;
    let before = quick_fingerprint(&real)?;
    let meta = fs::metadata(&real).map_err(err)?;
    if source.kind == Kind::Import && meta.len() > MAX_FILE {
        return Err("E_FILE_LIMIT".into());
    }
    if source.kind == Kind::Import {
        let bundle: Bundle =
            serde_json::from_reader(fs::File::open(&real).map_err(err)?).map_err(|_| "E_FORMAT")?;
        if bundle.schema_version != 1 || bundle.messages.len() > MAX_MESSAGES {
            return Err("E_FORMAT".into());
        }
        let mut c = bundle.conversation;
        if !valid_native_id(&c.native_id) {
            return Err("E_FORMAT".into());
        }
        let summary_current = c.summary_fingerprint == c.fingerprint;
        c.id = format!("{}:{}", source.id, c.native_id);
        c.source_id = source.id.clone();
        c.path = display(path);
        c.client = format!("Import / {}", c.client);
        c.fingerprint = before;
        c.bytes = meta.len();
        if summary_current {
            c.summary_fingerprint = c.fingerprint.clone();
        }
        return Ok((c, bundle.messages));
    }
    let mut c = Conversation {
        source_id: source.id.clone(),
        path: display(path),
        bytes: meta.len(),
        complete: true,
        fingerprint: before.clone(),
        modified: meta
            .modified()
            .map_err(err)?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?
            .as_secs(),
        archived: path
            .strip_prefix(&source.root)
            .ok()
            .and_then(|p| p.components().next())
            .is_some_and(|p| p.as_os_str() == "archived_sessions"),
        client: source.name.clone(),
        ..Default::default()
    };
    if meta.len() > MAX_FILE {
        c.complete = false;
        c.warning = "E_FILE_LIMIT".into();
    }
    let mut input = BufReader::new(fs::File::open(real).map_err(err)?.take(MAX_FILE));
    let mut messages = Vec::new();
    let mut text_bytes = 0usize;
    let mut fallback = Vec::new();
    let mut line = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("E_CANCELLED".into());
        }
        let mut bytes = Vec::new();
        let n = (&mut input)
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(err)?;
        if n == 0 {
            break;
        }
        line += 1;
        if n as u64 > MAX_LINE {
            c.complete = false;
            c.warning = "E_LINE_LIMIT".into();
            break;
        }
        let v: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                c.complete = false;
                c.warning = "E_PARTIAL".into();
                continue;
            }
        };
        let kind = str_at(&v, "type");
        let mut message = None;
        if source.kind == Kind::Codex {
            let p = &v["payload"];
            if kind == "session_meta" {
                c.native_id = str_at(p, "id").into();
                c.project = str_at(p, "cwd").into();
                c.parent_id = p
                    .pointer("/source/subagent/thread_spawn/parent_thread_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into();
                c.client = match p.get("source").and_then(Value::as_str) {
                    Some("cli") => "Codex CLI",
                    Some("vscode") => "Codex VS Code",
                    Some("appServer") => "Codex App Server",
                    _ => "Codex",
                }
                .into();
            } else if kind == "response_item" {
                match str_at(p, "type") {
                    "message" => {
                        message = Some((str_at(p, "role").to_string(), content(&p["content"])))
                    }
                    "function_call" => {
                        message = Some((
                            "tool".into(),
                            format!("{}\n{}", str_at(p, "name"), str_at(p, "arguments")),
                        ))
                    }
                    "function_call_output" => {
                        message = Some(("tool".into(), content(&p["output"])))
                    }
                    _ => {}
                }
            } else if kind == "event_msg"
                && ["user_message", "agent_message"].contains(&str_at(p, "type"))
            {
                fallback.push(Message {
                    line,
                    role: if str_at(p, "type") == "user_message" {
                        "user"
                    } else {
                        "assistant"
                    }
                    .into(),
                    text: str_at(p, "message").into(),
                });
                text_bytes += str_at(p, "message").len();
            }
        } else {
            if !str_at(&v, "sessionId").is_empty() {
                c.native_id = str_at(&v, "sessionId").into();
            }
            if !str_at(&v, "cwd").is_empty() {
                c.project = str_at(&v, "cwd").into();
            }
            if kind == "custom-title" {
                c.title = str_at(&v, "customTitle").into();
            }
            if ["user", "assistant"].contains(&kind) {
                message = Some((kind.to_string(), content(&v["message"]["content"])));
            }
        }
        if let Some((role, mut text)) = message {
            if text.len() > 262_144 {
                text = text.chars().take(65_536).collect();
                c.complete = false;
                c.warning = "E_MESSAGE_LIMIT".into();
            }
            if !text.trim().is_empty() {
                text_bytes += text.len();
                messages.push(Message { line, role, text });
            }
        }
        if messages.len() > MAX_MESSAGES || text_bytes > 8 * 1024 * 1024 {
            c.complete = false;
            c.warning = "E_MESSAGE_LIMIT".into();
            break;
        }
    }
    if messages.is_empty() {
        messages = fallback;
    }
    if c.native_id.is_empty() {
        return Err("E_FORMAT".into());
    }
    if !valid_native_id(&c.native_id) {
        return Err("E_FORMAT".into());
    }
    // Sidecar agents must not overwrite their parent session's row.
    let sidecar = path.components().any(|p| p.as_os_str() == "subagents")
        || (source.kind != Kind::Codex
            && path
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().starts_with("agent-")));
    let suffix = if sidecar {
        format!(
            ":{}",
            path.file_stem().unwrap_or_default().to_string_lossy()
        )
    } else {
        String::new()
    };
    c.id = format!("{}:{}{suffix}", source.id, c.native_id);
    if sidecar {
        c.parent_id = c.native_id.clone();
    }
    if c.title.is_empty() {
        c.title = messages
            .iter()
            .find(|m| {
                m.role == "user"
                    && !m.text.starts_with("<environment_context>")
                    && !m.text.starts_with("# AGENTS.md")
            })
            .map(|m| {
                m.text
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect()
            })
            .unwrap_or_else(|| c.native_id.clone());
    }
    c.message_count = messages.len();
    if before != quick_fingerprint(path)? {
        c.complete = false;
        c.warning = "E_CHANGED".into();
    }
    Ok((c, messages))
}

pub fn valid_native_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

pub fn digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(err)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(text: &str, kind: Kind) -> (tempfile::TempDir, Source, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("test.jsonl");
        fs::write(&p, text).unwrap();
        let s = Source {
            id: "s".into(),
            name: "test".into(),
            kind,
            root: display(d.path()),
            enabled: true,
        };
        (d, s, p)
    }
    #[test]
    fn codex_does_not_duplicate_events() {
        let (_d,s,p)=fixture("{\"type\":\"session_meta\",\"payload\":{\"id\":\"abc\",\"cwd\":\"D:/project\",\"source\":\"cli\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"hello\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello\"}]}}\n",Kind::Codex);
        let (c, m) = read(&s, &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(c.title, "hello");
        assert!(c.complete);
    }
    #[test]
    fn claude_and_partial_tail() {
        let (_d,s,p)=fixture("{\"type\":\"user\",\"sessionId\":\"abc\",\"cwd\":\"D:/p\",\"message\":{\"content\":\"question\"}}\n{bad",Kind::ClaudeCli);
        let (c, m) = read(&s, &p, &AtomicBool::new(false)).unwrap();
        assert!(!c.complete);
        assert_eq!(m[0].text, "question");
    }
    #[test]
    fn traversal_rejected() {
        let d = tempfile::tempdir().unwrap();
        assert!(checked_path(d.path(), &d.path().join("../x")).is_err());
    }
    #[test]
    fn cancellation() {
        let (_d, s, p) = fixture("", Kind::Codex);
        assert_eq!(
            read(&s, &p, &AtomicBool::new(true)).err().unwrap(),
            "E_CANCELLED"
        );
    }
    #[test]
    fn untrusted_import_id_cannot_escape_export_folder() {
        assert!(!valid_native_id("a/../../outside"));
        assert!(!valid_native_id("..\\outside"));
        assert!(!valid_native_id(""));
        assert!(valid_native_id("session-abc_12"));
    }
    #[test]
    #[ignore = "Read-only local compatibility probe, reports counts only"]
    fn local_source_compatibility() {
        let cancel = AtomicBool::new(false);
        for source in default_sources() {
            let (files, warnings) = files(&source, &cancel).unwrap();
            let mut readable = 0;
            let mut partial = 0;
            let count = files.len();
            for path in files.iter().take(12) {
                match read(&source, path, &cancel) {
                    Ok((c, _)) => {
                        readable += 1;
                        if !c.complete {
                            partial += 1;
                        }
                    }
                    Err(code) => println!("adapter={:?} code={code}", source.kind),
                }
            }
            println!("adapter={:?} discovered={count} sampled={} readable={readable} partial={partial} warnings={}",source.kind,count.min(12),warnings.len());
            if count > 0 {
                assert!(readable > 0);
            }
        }
    }
}
