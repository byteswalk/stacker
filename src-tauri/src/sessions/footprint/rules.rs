//! Static rules that say what each agent folder is and whether it can go.
use super::model::{FootprintKind, Owner};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: u64 = 86_400;
const LOG_KEEP_DAYS: u64 = 7;

pub struct Context<'a> {
    /// Every known session id (Codex thread ids and Claude session ids).
    pub session_ids: &'a HashSet<String>,
    pub running: &'a [PathBuf],
    pub now: u64,
}

impl Context<'_> {
    fn images(&self) -> impl Iterator<Item = String> + '_ {
        self.running
            .iter()
            .map(|p| p.to_string_lossy().to_lowercase())
    }

    fn codex_running(&self) -> bool {
        self.images().any(|p| {
            let name = p.rsplit('\\').next().unwrap_or("");
            name.starts_with("codex") || p.contains("\\openai.codex_")
        })
    }

    fn claude_app_running(&self) -> bool {
        self.images()
            .any(|p| p.contains("\\windowsapps\\claude_") || p.contains("\\anthropicclaude\\"))
    }

    fn claude_cli_running(&self) -> bool {
        self.images()
            .any(|p| p.rsplit('\\').next() == Some("claude.exe"))
    }

    fn in_use(&self, dir: &Path) -> bool {
        let dir = dir.to_string_lossy().to_lowercase();
        let prefix = format!("{}\\", dir.trim_end_matches('\\'));
        self.images().any(|p| p.starts_with(&prefix))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootKind {
    CodexHome,
    CodexApp,
    ClaudeHome,
    ClaudeApp,
    /// Any other agent's folder: read by what the folder names say, and nothing is
    /// suggested for deletion unless it is provably spent.
    Generic,
}

#[derive(Clone, Debug)]
pub struct Draft {
    pub rule: &'static str,
    pub kind: FootprintKind,
    pub owner: Owner,
    pub label: String,
    pub explain: String,
    pub paths: Vec<PathBuf>,
    pub blocked: Option<String>,
}

fn draft(
    rule: &'static str,
    kind: FootprintKind,
    owner: Owner,
    label: &str,
    explain: &str,
    paths: Vec<PathBuf>,
) -> Draft {
    Draft {
        rule,
        kind,
        owner,
        label: label.into(),
        explain: explain.into(),
        paths,
        blocked: None,
    }
}

fn blocked_if(mut d: Draft, running: bool) -> Draft {
    if running {
        d.blocked = Some("E_APP_RUNNING".into());
    }
    d
}

/// "0.146.0-alpha.9.2" → [0, 146, 0, 9, 2]; "2.1.275" → [2, 1, 275].
pub fn parse_version(s: &str) -> Option<Vec<u64>> {
    let parts: Vec<u64> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    (!parts.is_empty() && s.starts_with(|c: char| c.is_ascii_digit())).then_some(parts)
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = fs::read_dir(dir)
        .map(|e| e.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    list.sort();
    list
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn age_days(path: &Path, now: u64) -> u64 {
    let modified = fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(now);
    now.saturating_sub(modified) / DAY
}

/// Collects drafts for one root; everything unmatched ends in a single "other" Keep draft.
struct Builder {
    owner: Owner,
    drafts: Vec<Draft>,
    other: Vec<PathBuf>,
}

impl Builder {
    fn new(owner: Owner) -> Self {
        Self {
            owner,
            drafts: Vec::new(),
            other: Vec::new(),
        }
    }

    fn push(&mut self, d: Draft) {
        if !d.paths.is_empty() {
            self.drafts.push(d);
        }
    }

    fn add(
        &mut self,
        rule: &'static str,
        kind: FootprintKind,
        label: &str,
        explain: &str,
        paths: Vec<PathBuf>,
        running: bool,
    ) {
        self.push(blocked_if(
            draft(rule, kind, self.owner, label, explain, paths),
            running && matches!(kind, FootprintKind::Reclaimable | FootprintKind::Review),
        ));
    }

    fn finish(mut self) -> Vec<Draft> {
        let other = std::mem::take(&mut self.other);
        self.push(draft(
            "other",
            FootprintKind::Keep,
            self.owner,
            "其他数据",
            "智能体的配置、索引和运行所需文件。",
            other,
        ));
        self.drafts
    }
}

pub fn classify_root(kind: RootKind, root: &Path, cx: &Context) -> Vec<Draft> {
    match kind {
        RootKind::CodexHome => codex_home(root, cx),
        RootKind::CodexApp => codex_app(root),
        RootKind::ClaudeHome => claude_home(root, cx),
        RootKind::ClaudeApp => claude_app(root, cx),
        RootKind::Generic => generic(root, cx),
    }
}

/// Folder names that mean the same thing across agents. Only the first group is ever
/// suggested for deletion: logs the agent has moved on from, crash reports, and update
/// packages it already installed. Caches and work products are shown and left alone,
/// and anything unrecognised is kept without comment.
fn generic_kind(name: &str) -> Option<(&'static str, FootprintKind, &'static str, &'static str)> {
    use FootprintKind::*;
    let is = |list: &[&str]| list.contains(&name);
    if is(&["logs", "log", "traces", "trace", "audit-log", "observe"]) {
        return Some((
            "agent-logs",
            Reclaimable,
            "日志",
            "智能体的运行日志，只用于排查问题，删除后会重新生成。",
        ));
    }
    if is(&[
        "crashpad",
        "crashes",
        "crash dumps",
        "crashdumps",
        "pending-telemetry",
        "sentry",
    ]) {
        return Some((
            "agent-crash",
            Reclaimable,
            "崩溃与遥测数据",
            "崩溃转储和待上传的遥测数据，删除不影响使用。",
        ));
    }
    if name.ends_with("-updater") || is(&["updates", "update-cache", "pending-updates"]) {
        return Some((
            "agent-updates",
            Reclaimable,
            "更新包缓存",
            "已下载的安装包，装完就不再需要，下次更新会重新下载。",
        ));
    }
    if is(&[
        "cache",
        "caches",
        ".cache",
        "code cache",
        "gpucache",
        "dawncache",
        "dawngraphitecache",
        "dawnwebgpucache",
        "blob_storage",
        "tmp",
        "temp",
        ".tmp",
    ]) {
        return Some((
            "agent-cache",
            Review,
            "缓存与临时文件",
            "删除后智能体会重建，但下次启动会慢一些；正在运行时不要删。",
        ));
    }
    if is(&[
        "projects",
        "sessions",
        "conversations",
        "threads",
        "history",
        "chats",
    ]) {
        return Some((
            "agent-sessions",
            Sessions,
            "会话记录",
            "对话的完整记录，请在「会话」标签中按会话删除。",
        ));
    }
    if is(&[
        "binaries",
        "app",
        "vendor",
        "plugins",
        "extensions",
        "blobs",
        "snapshots",
        "buddy-snapshots",
        "file-history",
        "artifact-index",
        "workspace",
        "storage",
    ]) {
        return Some((
            "agent-work",
            Review,
            "程序文件与工作产物",
            "智能体自带的程序、插件和它干活留下的产物，删除前请确认不再需要。",
        ));
    }
    None
}

/// An agent folder Stacker has no special knowledge of.
fn generic(root: &Path, cx: &Context) -> Vec<Draft> {
    use FootprintKind::*;
    let running = cx.in_use(root);
    let mut b = Builder::new(Owner::Shared);
    let mut grouped: Vec<(
        &'static str,
        FootprintKind,
        &'static str,
        &'static str,
        Vec<PathBuf>,
    )> = Vec::new();
    let mut old_logs = Vec::new();
    for path in entries(root) {
        let n = name(&path);
        // A log file the agent has moved past, wherever it keeps it.
        if n.ends_with(".log") || n.ends_with(".log.old") {
            if age_days(&path, cx.now) > LOG_KEEP_DAYS {
                old_logs.push(path);
            } else {
                b.other.push(path);
            }
            continue;
        }
        match generic_kind(&n) {
            Some((rule, kind, label, explain)) => {
                match grouped.iter_mut().find(|(r, ..)| *r == rule) {
                    Some((.., paths)) => paths.push(path),
                    None => grouped.push((rule, kind, label, explain, vec![path])),
                }
            }
            None => b.other.push(path),
        }
    }
    for (rule, kind, label, explain, paths) in grouped {
        b.add(rule, kind, label, explain, paths, running);
    }
    b.add(
        "agent-old-logs",
        Reclaimable,
        "7 天前的日志文件",
        "智能体留下的旧日志文件。",
        old_logs,
        false,
    );
    b.finish()
}

fn codex_home(root: &Path, cx: &Context) -> Vec<Draft> {
    use FootprintKind::*;
    let running = cx.codex_running();
    let mut b = Builder::new(Owner::Shared);
    let mut sessions = Vec::new();
    let (mut logs_db, mut old_logs, mut state, mut history) = (vec![], vec![], vec![], vec![]);
    for path in entries(root) {
        let n = name(&path);
        match n.as_str() {
            "sessions" | "archived_sessions" => sessions.push(path),
            "plugins" => b.add(
                "codex-plugins",
                Keep,
                "插件",
                "已安装的 Codex 插件。",
                vec![path],
                false,
            ),
            "skills" => b.add(
                "codex-skills",
                Keep,
                "技能",
                "已安装的 Codex 技能。",
                vec![path],
                false,
            ),
            ".tmp" => b.add(
                "codex-tmp",
                Reclaimable,
                "临时文件",
                "Codex 解包插件市场等产生的临时文件，退出后可安全删除。",
                vec![path],
                running,
            ),
            "cache" => b.add(
                "codex-cache",
                Reclaimable,
                "缓存",
                "Codex 的下载与模型缓存，删除后按需重新下载。",
                vec![path],
                running,
            ),
            ".sandbox-bin" => codex_sandbox(&mut b, &path),
            "tmp" => {
                for child in entries(&path) {
                    let label = format!(
                        "工作临时目录 {}",
                        child.file_name().unwrap_or_default().to_string_lossy()
                    );
                    b.add(
                        "codex-work-tmp",
                        Review,
                        &label,
                        "智能体执行任务时创建的临时工作目录，可能含未保存到项目里的产物。",
                        vec![child],
                        false,
                    );
                }
            }
            "target" => b.add(
                "codex-target",
                Review,
                "编译产物 target",
                "在 Codex 数据目录里执行编译留下的产物，通常可删除，需要时重新编译。",
                vec![path],
                false,
            ),
            "generated_images" => b.add(
                "codex-images",
                Review,
                "生成的图片",
                "Codex 生成的图片，删除前请确认已保存需要的图片。",
                vec![path],
                false,
            ),
            "attachments" => b.add(
                "codex-attachments",
                Review,
                "附件",
                "发送给 Codex 的附件副本，删除后历史会话中无法再查看这些附件。",
                vec![path],
                false,
            ),
            "visualizations" => b.add(
                "codex-visualizations",
                Review,
                "可视化产物",
                "Codex 生成的图表和页面，删除前请确认已不需要。",
                vec![path],
                false,
            ),
            _ if n.starts_with("logs_") && n.contains(".sqlite") => logs_db.push(path),
            _ if n.starts_with("state_") && n.contains(".sqlite") => state.push(path),
            _ if n.starts_with("thread_history_") && n.contains(".sqlite") => history.push(path),
            _ if n.ends_with(".log") && age_days(&path, cx.now) > LOG_KEEP_DAYS => {
                old_logs.push(path)
            }
            _ => b.other.push(path),
        }
    }
    b.add(
        "codex-sessions",
        Sessions,
        "会话记录",
        "Codex 会话的完整记录，请在「会话」标签中按会话删除。",
        sessions,
        false,
    );
    b.add(
        "codex-state",
        Keep,
        "会话状态数据库",
        "Codex 的会话列表与项目信息，删除会丢失全部会话索引。",
        state,
        false,
    );
    b.add(
        "codex-history",
        Keep,
        "线程历史数据库",
        "Codex 的会话历史投影，用于快速打开会话。",
        history,
        false,
    );
    b.add(
        "codex-logs-db",
        Review,
        "运行日志数据库",
        "Codex 的运行日志，只用于排查问题；删除后 Codex 会重建，需要先退出 Codex。",
        logs_db,
        running,
    );
    b.add(
        "codex-old-logs",
        Reclaimable,
        "7 天前的日志文件",
        "旧的沙箱日志文件。",
        old_logs,
        false,
    );
    b.finish()
}

fn codex_sandbox(b: &mut Builder, dir: &Path) {
    let mut runners: Vec<(Vec<u64>, PathBuf)> = Vec::new();
    for path in entries(dir) {
        let n = name(&path);
        match n
            .strip_prefix("codex-command-runner-")
            .and_then(|v| v.strip_suffix(".exe"))
            .and_then(parse_version)
        {
            Some(version) => runners.push((version, path)),
            None => b.other.push(path),
        }
    }
    runners.sort();
    if let Some((_, newest)) = runners.pop() {
        b.other.push(newest);
    }
    b.add(
        "codex-old-runners",
        FootprintKind::Reclaimable,
        "旧版命令执行器",
        "Codex 升级后留下的旧版 codex-command-runner，只保留最新版本。",
        runners.into_iter().map(|(_, p)| p).collect(),
        false,
    );
}

fn codex_app(root: &Path) -> Vec<Draft> {
    let mut b = Builder::new(Owner::DesktopApp);
    for path in entries(root) {
        match name(&path).as_str() {
            "bin" => b.add(
                "codex-app-bin",
                FootprintKind::Keep,
                "桌面端组件",
                "Codex 桌面端运行所需的程序组件（codex、node、rg 等），按内容存放，不是旧版本。",
                vec![path],
                false,
            ),
            "runtimes" => b.add(
                "codex-app-runtimes",
                FootprintKind::Keep,
                "运行时",
                "Codex 桌面端的电脑操控等功能所需的运行时。",
                vec![path],
                false,
            ),
            _ => b.other.push(path),
        }
    }
    b.finish()
}

fn claude_home(root: &Path, cx: &Context) -> Vec<Draft> {
    use FootprintKind::*;
    let running = cx.claude_cli_running();
    let mut b = Builder::new(Owner::Shared);
    let mut caches = Vec::new();
    for path in entries(root) {
        match name(&path).as_str() {
            "projects" => b.add(
                "claude-sessions",
                Sessions,
                "会话记录",
                "Claude 会话的完整记录，请在「会话」标签中按会话删除。",
                vec![path],
                false,
            ),
            "plugins" => b.add(
                "claude-plugins",
                Keep,
                "插件",
                "已安装的 Claude 插件与市场。",
                vec![path],
                false,
            ),
            "file-history" | "session-env" => {
                let (orphans, kept): (Vec<PathBuf>, Vec<PathBuf>) =
                    entries(&path).into_iter().partition(|p| {
                        let id = p
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        !cx.session_ids.contains(&id) && age_days(p, cx.now) >= 1
                    });
                b.other.extend(kept);
                if name(&path) == "file-history" {
                    b.add(
                        "claude-orphan-history",
                        Reclaimable,
                        "无对应会话的编辑检查点",
                        "会话已删除后留下的文件编辑检查点。",
                        orphans,
                        false,
                    );
                } else {
                    b.add(
                        "claude-orphan-env",
                        Reclaimable,
                        "无对应会话的环境快照",
                        "会话已删除后留下的会话环境快照。",
                        orphans,
                        false,
                    );
                }
            }
            "shell-snapshots" => {
                let (old, recent): (Vec<PathBuf>, Vec<PathBuf>) = entries(&path)
                    .into_iter()
                    .partition(|p| age_days(p, cx.now) > LOG_KEEP_DAYS);
                b.other.extend(recent);
                b.add(
                    "claude-shell-snapshots",
                    Reclaimable,
                    "7 天前的终端快照",
                    "Claude 执行命令前保存的终端环境快照，只在当次会话中使用。",
                    old,
                    false,
                );
            }
            "cache" | "telemetry" => caches.push(path),
            _ => b.other.push(path),
        }
    }
    b.add(
        "claude-cache",
        Reclaimable,
        "缓存与遥测",
        "Claude Code 的缓存和待上传的遥测数据，删除后自动重建。",
        caches,
        running,
    );
    b.finish()
}

fn claude_app(root: &Path, cx: &Context) -> Vec<Draft> {
    use FootprintKind::*;
    let running = cx.claude_app_running();
    let mut b = Builder::new(Owner::DesktopApp);
    let (mut caches, mut old_logs) = (vec![], vec![]);
    for path in entries(root) {
        match name(&path).as_str() {
            "claude-code" => claude_code_versions(&mut b, &path, cx),
            "cache" | "code cache" | "gpucache" | "dawngraphitecache" | "dawnwebgpucache" => {
                caches.push(path)
            }
            "logs" => {
                for log in entries(&path) {
                    if age_days(&log, cx.now) > LOG_KEEP_DAYS {
                        old_logs.push(log);
                    } else {
                        b.other.push(log);
                    }
                }
            }
            _ => b.other.push(path),
        }
    }
    b.add(
        "claude-app-cache",
        Reclaimable,
        "桌面端浏览器缓存",
        "Claude 桌面端的网页与显卡缓存，删除后自动重建；需要先退出 Claude 桌面端。",
        caches,
        running,
    );
    b.add(
        "claude-app-logs",
        Reclaimable,
        "7 天前的桌面端日志",
        "Claude 桌面端的旧日志文件。",
        old_logs,
        false,
    );
    b.finish()
}

fn claude_code_versions(b: &mut Builder, dir: &Path, cx: &Context) {
    let mut versions: Vec<(Vec<u64>, PathBuf)> = Vec::new();
    for path in entries(dir) {
        match parse_version(&name(&path)).filter(|_| path.is_dir()) {
            Some(v) => versions.push((v, path)),
            None => b.other.push(path),
        }
    }
    versions.sort();
    let newest = versions.last().map(|(v, _)| v.clone());
    let (current, old): (Vec<_>, Vec<_>) = versions
        .into_iter()
        .partition(|(v, p)| Some(v) == newest.as_ref() || cx.in_use(p));
    b.add(
        "claude-code-current",
        FootprintKind::Keep,
        "当前 Claude Code",
        "桌面端正在使用的 Claude Code 版本。",
        current.into_iter().map(|(_, p)| p).collect(),
        false,
    );
    b.add(
        "claude-code-old",
        FootprintKind::Reclaimable,
        "旧版 Claude Code",
        "桌面端自动更新后留下的旧版本，没有程序在使用。",
        old.into_iter().map(|(_, p)| p).collect(),
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"x").unwrap();
    }

    fn age(path: &Path, days: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(days * DAY))
            .unwrap();
    }

    fn rules(drafts: &[Draft]) -> Vec<&'static str> {
        drafts.iter().map(|d| d.rule).collect()
    }

    fn find<'a>(drafts: &'a [Draft], rule: &str) -> &'a Draft {
        drafts.iter().find(|d| d.rule == rule).unwrap()
    }

    fn cx<'a>(ids: &'a HashSet<String>, running: &'a [PathBuf]) -> Context<'a> {
        Context {
            session_ids: ids,
            running,
            now: SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(
            parse_version("0.146.0-alpha.9.2"),
            Some(vec![0, 146, 0, 9, 2])
        );
        assert!(parse_version("0.153.4").unwrap() > parse_version("0.146.0-alpha.9.2").unwrap());
        assert_eq!(parse_version("abc"), None);
    }

    #[test]
    fn codex_home_is_classified() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for p in [
            "sessions/2026/a.jsonl",
            ".sandbox-bin/codex-command-runner-0.146.0-alpha.9.2.exe",
            ".sandbox-bin/codex-command-runner-0.153.4.exe",
            ".sandbox-bin/codex.exe",
            ".tmp/x",
            "tmp/job/out.txt",
            "target/debug/a",
            "logs_2.sqlite",
            "logs_2.sqlite-wal",
            "state_5.sqlite",
            "config.toml",
            "sandbox.old.log",
            "sandbox.new.log",
        ] {
            touch(&r.join(p));
        }
        age(&r.join("sandbox.old.log"), 30);
        let ids = HashSet::new();
        let drafts = codex_home(r, &cx(&ids, &[]));
        let runners = find(&drafts, "codex-old-runners");
        assert_eq!(runners.paths.len(), 1);
        assert!(runners.paths[0].ends_with("codex-command-runner-0.146.0-alpha.9.2.exe"));
        assert_eq!(find(&drafts, "codex-logs-db").paths.len(), 2);
        assert_eq!(find(&drafts, "codex-old-logs").paths.len(), 1);
        assert_eq!(find(&drafts, "codex-work-tmp").kind, FootprintKind::Review);
        assert_eq!(
            find(&drafts, "codex-sessions").kind,
            FootprintKind::Sessions
        );
        let other = find(&drafts, "other");
        assert!(other.paths.iter().any(|p| p.ends_with("config.toml")));
        assert!(other
            .paths
            .iter()
            .any(|p| p.ends_with("codex-command-runner-0.153.4.exe")));
        assert!(other.paths.iter().any(|p| p.ends_with("sandbox.new.log")));
        assert!(find(&drafts, "codex-tmp").blocked.is_none());

        let running = vec![PathBuf::from(r"C:\x\codex.exe")];
        let drafts = codex_home(r, &cx(&ids, &running));
        assert_eq!(
            find(&drafts, "codex-tmp").blocked.as_deref(),
            Some("E_APP_RUNNING")
        );
        assert!(find(&drafts, "codex-old-runners").blocked.is_none());
    }

    #[test]
    fn claude_home_finds_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for p in [
            "projects/p/live.jsonl",
            "session-env/live/x",
            "session-env/gone/x",
            "file-history/gone/v1",
            "shell-snapshots/old.sh",
            "settings.json",
        ] {
            touch(&r.join(p));
        }
        for p in ["session-env/live", "session-env/gone", "file-history/gone"] {
            filetime_dir(&r.join(p), 3);
        }
        age(&r.join("shell-snapshots/old.sh"), 30);
        let ids: HashSet<String> = ["live".to_string()].into();
        let drafts = claude_home(r, &cx(&ids, &[]));
        let env = find(&drafts, "claude-orphan-env");
        assert_eq!(env.paths.len(), 1);
        assert!(env.paths[0].ends_with("gone"));
        assert_eq!(find(&drafts, "claude-orphan-history").paths.len(), 1);
        assert_eq!(find(&drafts, "claude-shell-snapshots").paths.len(), 1);
        assert!(find(&drafts, "other")
            .paths
            .iter()
            .any(|p| p.ends_with("live")));
    }

    /// Directory mtimes cannot be set through std on stable; age via a PowerShell call.
    fn filetime_dir(path: &Path, days: u64) {
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command"])
            .arg(format!(
                "(Get-Item -LiteralPath '{}').LastWriteTime = (Get-Date).AddDays(-{days})",
                path.display()
            ))
            .output();
    }

    #[test]
    fn claude_app_keeps_newest_and_running_versions() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for p in [
            "claude-code/2.1.270/claude.exe",
            "claude-code/2.1.274/claude.exe",
            "claude-code/2.1.275/claude.exe",
            "Cache/data",
            "Code Cache/js",
            "IndexedDB/x",
            "Logs/main.log",
        ] {
            touch(&r.join(p));
        }
        let ids = HashSet::new();
        let running = vec![r.join("claude-code").join("2.1.270").join("claude.exe")];
        let drafts = claude_app(r, &cx(&ids, &running));
        let old = find(&drafts, "claude-code-old");
        assert_eq!(old.paths.len(), 1);
        assert!(old.paths[0].ends_with("2.1.274"));
        assert_eq!(find(&drafts, "claude-code-current").paths.len(), 2);
        assert_eq!(find(&drafts, "claude-app-cache").paths.len(), 2);
        assert!(rules(&drafts).contains(&"other"));
        assert!(!rules(&drafts).contains(&"claude-app-logs"));

        let app = vec![PathBuf::from(
            r"C:\Program Files\WindowsApps\Claude_2.0_x64__abc\app\Claude.exe",
        )];
        let drafts = claude_app(r, &cx(&ids, &app));
        assert_eq!(
            find(&drafts, "claude-app-cache").blocked.as_deref(),
            Some("E_APP_RUNNING")
        );
        assert!(find(&drafts, "claude-code-old").blocked.is_none());
    }
}
