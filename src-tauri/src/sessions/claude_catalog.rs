use super::model::*;
use super::project::project_ref;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const HEAD_LINES: usize = 256;
const HEAD_BYTES: u64 = 1024 * 1024;
/// Renames append `custom-title` records, so the end of the file is read as well.
const TAIL_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Default)]
pub struct DesktopEntry {
    pub title: String,
    pub archived: bool,
    pub created_ms: u64,
    pub last_activity_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Head {
    pub session_id: String,
    pub cwd: String,
    pub entrypoint: String,
    pub custom_title: Option<String>,
    pub summary: Option<String>,
    pub first_user_message: Option<String>,
    pub worktree_cwd: Option<String>,
}

pub(crate) fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 {
                    return true;
                }
            }
            m.file_type().is_symlink()
        })
        .unwrap_or(false)
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && !is_link(p))
                .collect()
        })
        .unwrap_or_default()
}

/// `<dir>/<account>/<org>/local_*.json`, keyed by `cliSessionId`.
pub fn read_desktop_index(dir: &Path) -> HashMap<String, DesktopEntry> {
    let mut entries = HashMap::new();
    for org in subdirs(dir).iter().flat_map(|account| subdirs(account)) {
        let Ok(files) = fs::read_dir(&org) else {
            continue;
        };
        for file in files.flatten().map(|e| e.path()) {
            let is_local = file
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("local_"))
                && file.extension().is_some_and(|e| e == "json");
            if !is_local || is_link(&file) {
                continue;
            }
            let Some(v) = fs::read(&file)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            else {
                continue;
            };
            let Some(id) = v.get("cliSessionId").and_then(Value::as_str) else {
                continue;
            };
            entries.insert(
                id.to_string(),
                DesktopEntry {
                    title: v
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    archived: v
                        .get("isArchived")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    created_ms: v.get("createdAt").and_then(Value::as_u64).unwrap_or(0),
                    last_activity_ms: v.get("lastActivityAt").and_then(Value::as_u64).unwrap_or(0),
                },
            );
        }
    }
    entries
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn is_context(text: &str) -> bool {
    let t = text.trim_start();
    t.is_empty() || t.starts_with('<') || t.starts_with("# AGENTS.md") || t.starts_with("Caveat:")
}

fn non_empty(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn absorb(head: &mut Head, v: &Value) {
    let field = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    if head.session_id.is_empty() {
        head.session_id = field("sessionId");
    }
    if head.cwd.is_empty() {
        head.cwd = field("cwd");
    }
    if head.entrypoint.is_empty() {
        head.entrypoint = field("entrypoint");
    }
    match v.get("type").and_then(Value::as_str).unwrap_or("") {
        // The latest rename wins.
        "custom-title" => {
            if let Some(title) = non_empty(v, "customTitle") {
                head.custom_title = Some(title);
            }
        }
        "summary" => {
            if head.summary.is_none() {
                head.summary = non_empty(v, "summary");
            }
        }
        "worktree-state" => {
            if let Some(original) = v
                .pointer("/worktreeSession/originalCwd")
                .and_then(Value::as_str)
            {
                head.worktree_cwd = Some(original.to_string());
            }
        }
        "relocated" => {
            if head.worktree_cwd.is_none() {
                head.worktree_cwd = non_empty(v, "relocatedCwd");
            }
        }
        "user" if head.first_user_message.is_none() => {
            let text = text_of(&v["message"]["content"]);
            if !is_context(&text) {
                let first = text
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .trim();
                head.first_user_message = Some(first.chars().take(80).collect());
            }
        }
        _ => {}
    }
}

/// Reads the first 256 lines (1 MB) and the last 256 KB of a transcript.
pub fn read_head(path: &Path) -> Result<Head, String> {
    let mut file = fs::File::open(path).map_err(super::err)?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut head = Head::default();
    let mut lines_read = 0;
    {
        let reader = BufReader::new((&mut file).take(HEAD_BYTES));
        for line in reader.lines().take(HEAD_LINES) {
            let Ok(line) = line else { break };
            lines_read += 1;
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                absorb(&mut head, &v);
            }
        }
    }
    if len > HEAD_BYTES || lines_read == HEAD_LINES {
        let start = len.saturating_sub(TAIL_BYTES);
        if file.seek(SeekFrom::Start(start)).is_ok() {
            let mut tail = Vec::new();
            let _ = file.take(TAIL_BYTES).read_to_end(&mut tail);
            let text = String::from_utf8_lossy(&tail);
            // Skip the first line when it is probably partial.
            for line in text.lines().skip(usize::from(start > 0)) {
                if line.contains("\"custom-title\"") || line.contains("\"summary\"") {
                    if let Ok(v) = serde_json::from_str::<Value>(line) {
                        absorb(&mut head, &v);
                    }
                }
            }
        }
    }
    Ok(head)
}

pub(crate) fn tree_size(path: &Path) -> u64 {
    if is_link(path) {
        return 0;
    }
    match fs::metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        Ok(m) if m.is_dir() => fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| tree_size(&e.path())).sum())
            .unwrap_or(0),
        _ => 0,
    }
}

/// Everything a Claude session owns on disk; only existing, non-link paths.
pub fn related_paths(root: &Path, transcript: &Path, session_id: &str) -> Vec<PathBuf> {
    let mut paths = vec![transcript.to_path_buf()];
    if let Some(parent) = transcript.parent() {
        paths.push(parent.join(session_id));
    }
    paths.push(root.join("file-history").join(session_id));
    paths.push(root.join("session-env").join(session_id));
    paths
        .into_iter()
        .filter(|p| p.exists() && !is_link(p))
        .collect()
}

fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn children_of(side_dir: &Path, session_id: &str, agent: Agent) -> Vec<ChildSummary> {
    let Ok(entries) = fs::read_dir(side_dir.join("subagents")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl") && !is_link(p))
        .map(|p| ChildSummary {
            id: format!(
                "{}:{session_id}:{}",
                agent.as_str(),
                p.file_stem().unwrap_or_default().to_string_lossy()
            ),
            kind: "subagent".into(),
            path: p.to_string_lossy().into_owned(),
            title: read_head(&p)
                .ok()
                .and_then(|h| h.first_user_message)
                .unwrap_or_default(),
            bytes: tree_size(&p),
        })
        .collect()
}

fn session_from(
    root: &Path,
    project_dir: &Path,
    path: &Path,
    index: &HashMap<String, DesktopEntry>,
    agent: Agent,
) -> Option<Session> {
    let head = read_head(path).ok()?;
    let session_id = if head.session_id.is_empty() {
        path.file_stem()?.to_string_lossy().into_owned()
    } else {
        head.session_id.clone()
    };
    let side_dir = project_dir.join(&session_id);
    let custom_file = fs::read(side_dir.join("custom-title.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| non_empty(&v, "customTitle"));
    let desktop = index.get(&session_id);
    let (title, title_source) = if let Some(t) = desktop
        .map(|d| d.title.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        (t, TitleSource::Client)
    } else if let Some(t) = custom_file.or_else(|| head.custom_title.clone()) {
        (t, TitleSource::Custom)
    } else if let Some(t) = head.summary.clone() {
        (t, TitleSource::Summary)
    } else {
        (
            head.first_user_message
                .clone()
                .unwrap_or_else(|| session_id.clone()),
            TitleSource::FirstMessage,
        )
    };
    let client = match head.entrypoint.as_str() {
        "claude-desktop" => ClientTag::Desktop,
        "cli" => ClientTag::Terminal,
        e if e.contains("vscode") || e.contains("jetbrains") => ClientTag::Ide,
        e if e.starts_with("sdk") => ClientTag::Sdk,
        _ => ClientTag::Unknown,
    };
    let project = project_ref(head.worktree_cwd.as_deref().unwrap_or(&head.cwd), None);
    let archived = desktop.is_some_and(|d| d.archived);
    let status = if archived {
        SessionStatus::Archived
    } else if (client == ClientTag::Desktop && desktop.is_none()) || !project.exists {
        SessionStatus::Orphaned
    } else {
        SessionStatus::Active
    };
    let bytes = related_paths(root, path, &session_id)
        .iter()
        .map(|p| tree_size(p))
        .sum();
    let file_time = mtime(path);
    Some(Session {
        id: format!("{}:{session_id}", agent.as_str()),
        agent,
        children: children_of(&side_dir, &session_id, agent),
        native_id: session_id,
        title,
        title_source,
        project,
        client,
        created_at: desktop
            .map(|d| d.created_ms / 1000)
            .filter(|t| *t > 0)
            .unwrap_or(file_time),
        updated_at: desktop
            .map(|d| d.last_activity_ms / 1000)
            .filter(|t| *t > 0)
            .unwrap_or(file_time),
        archived,
        pinned: false,
        status,
        bytes,
        path: path.to_string_lossy().into_owned(),
        in_desktop_index: desktop.is_some(),
        parent_missing: false,
        favorite: false,
        summary: None,
        summary_stale: false,
        summary_by: String::new(),
        summary_at: 0,
        copies: Vec::new(),
        imported_from: None,
        imported_by: Vec::new(),
    })
}

pub fn load(root: &Path, desktop_index: &Path) -> Result<Vec<Session>, String> {
    load_as(root, Agent::Claude, Some(desktop_index))
}

/// The same store, read for whichever product wrote it. Qoder's CLI writes the same
/// transcripts under its own folder and keeps no desktop index.
pub fn load_as(
    root: &Path,
    agent: Agent,
    desktop_index: Option<&Path>,
) -> Result<Vec<Session>, String> {
    let projects = root.join("projects");
    if !projects.is_dir() {
        return Err("E_SOURCE_MISSING".into());
    }
    let index = desktop_index.map(read_desktop_index).unwrap_or_default();
    let mut sessions = Vec::new();
    for project_dir in subdirs(&projects) {
        let Ok(files) = fs::read_dir(&project_dir) else {
            continue;
        };
        for path in files.flatten().map(|e| e.path()) {
            if !path.extension().is_some_and(|e| e == "jsonl") || is_link(&path) {
                continue;
            }
            if let Some(session) = session_from(root, &project_dir, &path, &index, agent) {
                sessions.push(session);
            }
        }
    }
    Ok(merge_copies(sessions))
}

/// One session per id: the most recently written transcript is the session, the others
/// are copies left in other worktree folders. Sizes and sub-agents add up.
pub fn merge_copies(sessions: Vec<Session>) -> Vec<Session> {
    let mut by_id: HashMap<String, Vec<Session>> = HashMap::new();
    let mut order = Vec::new();
    for s in sessions {
        if !by_id.contains_key(&s.id) {
            order.push(s.id.clone());
        }
        by_id.entry(s.id.clone()).or_default().push(s);
    }
    order
        .into_iter()
        .filter_map(|id| {
            let mut group = by_id.remove(&id)?;
            group.sort_by_key(|s| {
                std::cmp::Reverse(fs::metadata(&s.path).and_then(|m| m.modified()).ok())
            });
            let mut primary = group.remove(0);
            for copy in group {
                primary.bytes += copy.bytes;
                primary.copies.push(copy.path);
                for child in copy.children {
                    if !primary
                        .children
                        .iter()
                        .any(|c| c.id == child.id && c.path == child.path)
                    {
                        primary.children.push(child);
                    }
                }
            }
            Some(primary)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(v: serde_json::Value) -> String {
        format!("{v}\n")
    }

    #[test]
    fn claude_sessions_follow_the_desktop_app() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".claude");
        let project = home.path().join("repo");
        fs::create_dir_all(&project).unwrap();
        let cwd = project.to_string_lossy().into_owned();
        let slug = root.join("projects").join("repo");
        fs::create_dir_all(slug.join("s-desktop").join("subagents")).unwrap();
        let user = |sid: &str, entry: &str, text: &str| {
            line(
                serde_json::json!({"type":"user","sessionId":sid,"cwd":cwd,"entrypoint":entry,"message":{"content":text}}),
            )
        };
        fs::write(
            slug.join("s-desktop.jsonl"),
            user("s-desktop", "claude-desktop", "hello"),
        )
        .unwrap();
        fs::write(
            slug.join("s-desktop")
                .join("subagents")
                .join("agent-1.jsonl"),
            user("s-desktop", "claude-desktop", "sub"),
        )
        .unwrap();
        fs::write(
            slug.join("s-desktop").join("custom-title.json"),
            r#"{"customTitle":"Custom"}"#,
        )
        .unwrap();
        fs::write(
            slug.join("s-orphan.jsonl"),
            user("s-orphan", "claude-desktop", "gone from sidebar"),
        )
        .unwrap();
        fs::write(
            slug.join("s-cli.jsonl"),
            format!(
                "{}{}",
                user("s-cli", "cli", "first question"),
                line(serde_json::json!({"type":"custom-title","customTitle":"CLI title","sessionId":"s-cli"}))
            ),
        )
        .unwrap();
        fs::create_dir_all(root.join("file-history").join("s-cli")).unwrap();
        fs::write(root.join("file-history").join("s-cli").join("a"), b"12345").unwrap();

        let index = home
            .path()
            .join("claude-code-sessions")
            .join("acct")
            .join("org");
        fs::create_dir_all(&index).unwrap();
        fs::write(
            index.join("local_1.json"),
            serde_json::json!({"cliSessionId":"s-desktop","title":"Desktop title","isArchived":true,"createdAt":1_000_000u64,"lastActivityAt":2_000_000u64}).to_string(),
        )
        .unwrap();

        let sessions = load(&root, &home.path().join("claude-code-sessions")).unwrap();
        assert_eq!(sessions.len(), 3, "sub-agents are children, not sessions");

        let desktop = sessions
            .iter()
            .find(|s| s.native_id == "s-desktop")
            .unwrap();
        assert_eq!(desktop.title, "Desktop title");
        assert_eq!(desktop.title_source, TitleSource::Client);
        assert_eq!(desktop.client, ClientTag::Desktop);
        assert!(desktop.in_desktop_index);
        assert_eq!(desktop.status, SessionStatus::Archived);
        assert_eq!(desktop.children.len(), 1);
        assert_eq!(desktop.updated_at, 2_000);

        let orphan = sessions.iter().find(|s| s.native_id == "s-orphan").unwrap();
        assert_eq!(orphan.status, SessionStatus::Orphaned);
        assert!(!orphan.in_desktop_index);

        let cli = sessions.iter().find(|s| s.native_id == "s-cli").unwrap();
        assert_eq!(cli.title, "CLI title");
        assert_eq!(cli.title_source, TitleSource::Custom);
        assert_eq!(cli.client, ClientTag::Terminal);
        assert_eq!(cli.status, SessionStatus::Active);
        let related = related_paths(&root, Path::new(&cli.path), "s-cli");
        assert!(related
            .iter()
            .any(|p| p.ends_with(Path::new("file-history").join("s-cli"))));
        assert!(cli.bytes >= 5);
    }

    #[test]
    fn worktree_copies_are_one_session() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".claude");
        let cwd = home.path().to_string_lossy().into_owned();
        let body = line(
            serde_json::json!({"type":"user","sessionId":"s1","cwd":cwd,"entrypoint":"cli","message":{"content":"hi"}}),
        );
        for (slug, size) in [("repo", 1usize), ("repo-worktree", 3usize)] {
            let dir = root.join("projects").join(slug);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("s1.jsonl"), body.repeat(size)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let sessions = load(&root, &home.path().join("none")).unwrap();
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert!(
            s.path.contains("repo-worktree"),
            "latest transcript is the session"
        );
        assert_eq!(s.copies.len(), 1);
        assert_eq!(s.bytes as usize, body.len() * 4);
    }

    #[test]
    fn first_message_fallback_skips_system_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        fs::write(
            &path,
            format!(
                "{}{}",
                line(serde_json::json!({"type":"user","sessionId":"s","message":{"content":"<environment_context>x</environment_context>"}})),
                line(serde_json::json!({"type":"user","sessionId":"s","message":{"content":[{"type":"text","text":"The real question\nsecond line"}]}}))
            ),
        )
        .unwrap();
        let head = read_head(&path).unwrap();
        assert_eq!(
            head.first_user_message.as_deref(),
            Some("The real question")
        );
    }

    #[test]
    fn worktree_sessions_use_the_original_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.jsonl");
        fs::write(
            &path,
            line(serde_json::json!({"type":"worktree-state","worktreeSession":{"originalCwd":"E:\\repo","worktreePath":"E:\\repo\\.claude\\worktrees\\x"},"sessionId":"w"})),
        )
        .unwrap();
        assert_eq!(
            read_head(&path).unwrap().worktree_cwd.as_deref(),
            Some("E:\\repo")
        );
    }

    #[test]
    fn late_renames_are_read_from_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jsonl");
        let filler =
            line(serde_json::json!({"type":"assistant","message":{"content":"x".repeat(4096)}}));
        let mut text =
            line(serde_json::json!({"type":"user","sessionId":"b","message":{"content":"hi"}}));
        for _ in 0..400 {
            text.push_str(&filler);
        }
        text.push_str(&line(
            serde_json::json!({"type":"custom-title","customTitle":"Renamed later","sessionId":"b"}),
        ));
        fs::write(&path, text).unwrap();
        assert_eq!(
            read_head(&path).unwrap().custom_title.as_deref(),
            Some("Renamed later")
        );
    }
}
