use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const REPORT_SCHEMA_VERSION: u16 = 1;
const MAX_REPORTS: usize = 100;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkEnvironmentItem {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub home_variable: Option<String>,
    pub home_path: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkEnvironmentContract {
    pub generated_at: String,
    pub fingerprint: String,
    pub items: Vec<WorkEnvironmentItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionLaunchRequest {
    pub agent_id: String,
    pub workspace: String,
    pub shell: String,
    pub enabled_items: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionLaunchResult {
    pub session_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub cli_name: String,
    pub command_path: String,
    pub workspace: String,
    pub shell: String,
    pub started_at: String,
    pub environment_fingerprint: String,
    #[serde(default)]
    pub environment: Vec<WorkEnvironmentItem>,
    #[serde(default = "default_session_mode")]
    pub mode: String,
    #[serde(default)]
    pub target_name: String,
    #[serde(default)]
    pub launch_kind: Option<String>,
    #[serde(default)]
    pub process: Option<WorkSessionDesktopProcess>,
}

fn default_session_mode() -> String {
    "cli".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionDesktopProcess {
    pub agent_id: String,
    pub agent_name: String,
    pub desktop_name: String,
    pub pid: u32,
    pub parent_pid: u32,
    pub process_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionDesktopRequest {
    pub agent_id: String,
    pub workspace: String,
    pub action: String,
    pub pid: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionDesktopCandidatesRequest {
    pub agent_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionDesktopProcessStatus {
    pub pid: u32,
    pub running: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionTrackingRoot {
    pub id: String,
    pub label: String,
    pub path: String,
    pub category: String,
    pub safety: String,
    pub default_enabled: bool,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionTrackingRequest {
    pub agent_id: String,
    pub workspace: String,
    pub enabled_items: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionReportInput {
    pub launch: WorkSessionLaunchResult,
    pub enabled_items: Vec<String>,
    pub tracking_roots: Vec<WorkSessionTrackingRoot>,
    pub monitor: crate::space_analysis::monitor::MonitorSnapshot,
    pub status: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionReport {
    pub id: String,
    pub ended_at: String,
    pub launch: WorkSessionLaunchResult,
    pub enabled_items: Vec<String>,
    pub tracking_roots: Vec<WorkSessionTrackingRoot>,
    pub monitor: crate::space_analysis::monitor::MonitorSnapshot,
    pub status: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredWorkSessionReport {
    schema_version: u16,
    report: WorkSessionReport,
}

#[derive(Clone, Copy)]
struct EnvironmentSpec {
    id: &'static str,
    label: &'static str,
    kind: &'static str,
    candidates: &'static [&'static str],
    version_args: &'static [&'static str],
    home_variable: Option<&'static str>,
    home_levels: usize,
}

const ENVIRONMENT_SPECS: &[EnvironmentSpec] = &[
    EnvironmentSpec {
        id: "git",
        label: "Git",
        kind: "source-control",
        candidates: &["git.exe", "git.cmd"],
        version_args: &["--version"],
        home_variable: None,
        home_levels: 0,
    },
    EnvironmentSpec {
        id: "node",
        label: "Node.js",
        kind: "runtime",
        candidates: &["node.exe"],
        version_args: &["--version"],
        home_variable: None,
        home_levels: 0,
    },
    EnvironmentSpec {
        id: "npm",
        label: "npm",
        kind: "package-manager",
        candidates: &["npm.cmd", "npm.exe", "npm.bat"],
        version_args: &["--version"],
        home_variable: None,
        home_levels: 0,
    },
    EnvironmentSpec {
        id: "python",
        label: "Python",
        kind: "runtime",
        candidates: &["python.exe"],
        version_args: &["--version"],
        home_variable: None,
        home_levels: 0,
    },
    EnvironmentSpec {
        id: "pip",
        label: "pip",
        kind: "package-manager",
        candidates: &["pip.exe", "pip.cmd"],
        version_args: &["--version"],
        home_variable: None,
        home_levels: 0,
    },
    EnvironmentSpec {
        id: "java",
        label: "Java",
        kind: "runtime",
        candidates: &["java.exe"],
        version_args: &["-version"],
        home_variable: Some("JAVA_HOME"),
        home_levels: 2,
    },
    EnvironmentSpec {
        id: "maven",
        label: "Maven",
        kind: "build-tool",
        candidates: &["mvn.cmd", "mvn.bat", "mvn.exe"],
        version_args: &["--version"],
        home_variable: Some("MAVEN_HOME"),
        home_levels: 2,
    },
    EnvironmentSpec {
        id: "gradle",
        label: "Gradle",
        kind: "build-tool",
        candidates: &["gradle.bat", "gradle.cmd", "gradle.exe"],
        version_args: &["--version"],
        home_variable: Some("GRADLE_HOME"),
        home_levels: 2,
    },
    EnvironmentSpec {
        id: "go",
        label: "Go",
        kind: "runtime",
        candidates: &["go.exe"],
        version_args: &["version"],
        home_variable: Some("GOROOT"),
        home_levels: 2,
    },
    EnvironmentSpec {
        id: "rust",
        label: "Rust",
        kind: "runtime",
        candidates: &["rustc.exe"],
        version_args: &["--version"],
        home_variable: None,
        home_levels: 0,
    },
    EnvironmentSpec {
        id: "cargo",
        label: "Cargo",
        kind: "build-tool",
        candidates: &["cargo.exe"],
        version_args: &["--version"],
        home_variable: Some("CARGO_HOME"),
        home_levels: 2,
    },
];

#[tauri::command]
pub async fn work_environment_contract() -> Result<WorkEnvironmentContract, String> {
    tauri::async_runtime::spawn_blocking(build_contract)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_launch(
    request: WorkSessionLaunchRequest,
) -> Result<WorkSessionLaunchResult, String> {
    tauri::async_runtime::spawn_blocking(move || launch(request))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_desktop_candidates(
    request: WorkSessionDesktopCandidatesRequest,
) -> Result<Vec<WorkSessionDesktopProcess>, String> {
    tauri::async_runtime::spawn_blocking(move || desktop_candidates(&request.agent_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_desktop_launch(
    request: WorkSessionDesktopRequest,
) -> Result<WorkSessionLaunchResult, String> {
    tauri::async_runtime::spawn_blocking(move || launch_desktop(request))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_desktop_process_status(
    pid: u32,
) -> Result<WorkSessionDesktopProcessStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(WorkSessionDesktopProcessStatus {
            pid,
            running: process_is_running(pid),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_tracking_roots(
    request: WorkSessionTrackingRequest,
) -> Result<Vec<WorkSessionTrackingRoot>, String> {
    tauri::async_runtime::spawn_blocking(move || tracking_roots(request))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_report_save(
    input: WorkSessionReportInput,
) -> Result<WorkSessionReport, String> {
    tauri::async_runtime::spawn_blocking(move || save_report(input))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_report_list() -> Result<Vec<WorkSessionReport>, String> {
    tauri::async_runtime::spawn_blocking(|| list_reports_at(&report_root()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn work_session_report_delete(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete_report_at(&report_root(), &id))
        .await
        .map_err(|error| error.to_string())?
}

fn tracking_roots(
    request: WorkSessionTrackingRequest,
) -> Result<Vec<WorkSessionTrackingRoot>, String> {
    let workspace = fs::canonicalize(request.workspace.trim()).map_err(|_| {
        "The selected project folder does not exist or cannot be accessed.".to_string()
    })?;
    if !workspace.is_dir() {
        return Err("A project folder is required.".into());
    }
    crate::vibe::managed_agent(request.agent_id.trim())?;
    let enabled = request.enabled_items.into_iter().collect::<HashSet<_>>();
    let mut roots = Vec::new();
    push_tracking_root(
        &mut roots,
        "project",
        "Project workspace",
        workspace,
        "project",
        "protected",
        true,
        "Source files and project-owned outputs. Review changes, but never offer automatic cleanup.",
    );
    for (index, path) in agent_data_roots(request.agent_id.trim())
        .into_iter()
        .enumerate()
    {
        push_tracking_root(
            &mut roots,
            &format!("agent:{}:{index}", request.agent_id.trim()),
            "Work agent data",
            path,
            "agent-data",
            "protected",
            true,
            "Agent configuration, sessions or local state. Track growth without treating it as disposable.",
        );
    }
    for (id, label, path, safety, default_enabled, reason) in cache_roots(&enabled) {
        push_tracking_root(
            &mut roots,
            id,
            label,
            path,
            "cache",
            safety,
            default_enabled,
            reason,
        );
    }
    Ok(roots)
}

#[allow(clippy::too_many_arguments)]
fn push_tracking_root(
    roots: &mut Vec<WorkSessionTrackingRoot>,
    id: &str,
    label: &str,
    path: PathBuf,
    category: &str,
    safety: &str,
    default_enabled: bool,
    reason: &str,
) {
    let Ok(path) = fs::canonicalize(path) else {
        return;
    };
    if !path.is_dir() {
        return;
    }
    let comparable = path.to_string_lossy().to_ascii_lowercase();
    if roots
        .iter()
        .any(|root| root.path.to_ascii_lowercase() == comparable)
    {
        return;
    }
    roots.push(WorkSessionTrackingRoot {
        id: id.into(),
        label: label.into(),
        path: crate::space_analysis::windows_fs::display_path(&path),
        category: category.into(),
        safety: safety.into(),
        default_enabled,
        reason: reason.into(),
    });
}

fn agent_data_roots(agent_id: &str) -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let roaming = dirs::data_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();
    match agent_id {
        "claude" => vec![home.join(".claude"), roaming.join("Claude")],
        "codex" => vec![home.join(".codex"), roaming.join("Codex")],
        "antigravity" => vec![home.join(".antigravity"), roaming.join("Antigravity")],
        "opencode" => vec![
            home.join(".config").join("opencode"),
            local.join("opencode"),
        ],
        "zcode" => vec![home.join(".zcode"), roaming.join("ZCode")],
        "kimi" => vec![home.join(".kimi"), roaming.join("Kimi")],
        "workbuddy" => vec![home.join(".workbuddy"), roaming.join("WorkBuddy")],
        "qoder" | "qoder-cn" => vec![home.join(".qoder"), roaming.join("Qoder")],
        "trae-work" | "trae-global" => vec![home.join(".trae"), roaming.join("Trae")],
        "deepseek-harness" => vec![home.join(".deepseek"), roaming.join("DeepSeek Harness")],
        "openclaw" => vec![home.join(".openclaw"), roaming.join("OpenClaw")],
        "hermes" => vec![home.join(".hermes"), roaming.join("Hermes")],
        _ => Vec::new(),
    }
}

type CacheRoot = (
    &'static str,
    &'static str,
    PathBuf,
    &'static str,
    bool,
    &'static str,
);

fn cache_roots(enabled: &HashSet<String>) -> Vec<CacheRoot> {
    let home = dirs::home_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();
    let mut roots = Vec::new();
    if enabled.contains("node") || enabled.contains("npm") {
        roots.extend([
            (
                "cache:npm",
                "npm package cache",
                crate::storage::effective_path("npm-cache")
                    .unwrap_or_else(|| local.join("npm-cache")),
                "rebuildable",
                true,
                "Downloaded npm packages can be fetched again.",
            ),
            (
                "cache:pnpm",
                "pnpm package store",
                crate::storage::effective_path("pnpm-store")
                    .unwrap_or_else(|| local.join("pnpm").join("store")),
                "rebuildable",
                true,
                "The pnpm content-addressed store can be rebuilt by package installs.",
            ),
        ]);
    }
    if enabled.contains("python") || enabled.contains("pip") {
        roots.push((
            "cache:pip",
            "pip download cache",
            crate::storage::effective_path("pip-cache")
                .unwrap_or_else(|| local.join("pip").join("Cache")),
            "rebuildable",
            true,
            "Downloaded Python packages can be fetched again.",
        ));
    }
    if enabled.contains("maven") {
        roots.push((
            "cache:maven",
            "Maven local repository",
            crate::storage::effective_path("maven-local-repository")
                .unwrap_or_else(|| home.join(".m2").join("repository")),
            "review",
            true,
            "Dependencies can usually be downloaded again, but offline or private artifacts require review.",
        ));
    }
    if enabled.contains("gradle") {
        roots.push((
            "cache:gradle",
            "Gradle caches",
            crate::storage::effective_path("gradle-user-home")
                .unwrap_or_else(|| home.join(".gradle"))
                .join("caches"),
            "rebuildable",
            true,
            "Gradle can rebuild these dependency and transform caches.",
        ));
    }
    if enabled.contains("go") {
        let go_cache = crate::storage::effective_path("go-module-cache")
            .or_else(|| std::env::var_os("GOMODCACHE").map(PathBuf::from))
            .unwrap_or_else(|| home.join("go").join("pkg").join("mod"));
        roots.push((
            "cache:go",
            "Go module cache",
            go_cache,
            "rebuildable",
            true,
            "Go modules can be downloaded again from configured module sources.",
        ));
    }
    if enabled.contains("rust") || enabled.contains("cargo") {
        let cargo_home = crate::storage::effective_path("cargo-home")
            .or_else(|| std::env::var_os("CARGO_HOME").map(PathBuf::from))
            .unwrap_or_else(|| home.join(".cargo"));
        roots.extend([
            (
                "cache:cargo-registry",
                "Cargo registry cache",
                cargo_home.join("registry").join("cache"),
                "rebuildable",
                true,
                "Crate archives can be downloaded again from the configured registry.",
            ),
            (
                "cache:cargo-git",
                "Cargo Git cache",
                cargo_home.join("git"),
                "rebuildable",
                true,
                "Cargo Git dependencies can be cloned again.",
            ),
        ]);
    }
    roots
}

fn report_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Stacker")
        .join("work-sessions")
        .join("reports")
}

fn save_report(input: WorkSessionReportInput) -> Result<WorkSessionReport, String> {
    if !matches!(
        input.status.as_str(),
        "completed" | "failed" | "interrupted"
    ) {
        return Err("Invalid work session report status.".into());
    }
    if input.launch.workspace.trim().is_empty() || input.launch.session_id.trim().is_empty() {
        return Err("Work session report is incomplete.".into());
    }
    let ended_at = chrono::Local::now().to_rfc3339();
    let id = format!(
        "{}-{}.json",
        chrono::Local::now().format("%Y%m%dT%H%M%S%3f"),
        sanitize_report_id(&input.launch.session_id)
    );
    let report = WorkSessionReport {
        id,
        ended_at,
        launch: input.launch,
        enabled_items: input.enabled_items,
        tracking_roots: input.tracking_roots,
        monitor: input.monitor,
        status: input.status,
    };
    write_report_at(&report_root(), &report)?;
    Ok(report)
}

fn sanitize_report_id(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(80)
        .collect()
}

fn validate_report_id(id: &str) -> Result<(), String> {
    if id.ends_with(".json")
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        Ok(())
    } else {
        Err("Invalid work session report id.".into())
    }
}

fn write_report_at(root: &Path, report: &WorkSessionReport) -> Result<(), String> {
    validate_report_id(&report.id)?;
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let path = root.join(&report.id);
    let temporary = path.with_extension("json.tmp");
    let stored = StoredWorkSessionReport {
        schema_version: REPORT_SCHEMA_VERSION,
        report: report.clone(),
    };
    fs::write(
        &temporary,
        serde_json::to_vec(&stored).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(&temporary, &path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })?;
    prune_reports_at(root)
}

fn list_reports_at(root: &Path) -> Result<Vec<WorkSessionReport>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut reports = fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<StoredWorkSessionReport>(&bytes).ok())
        .filter(|stored| stored.schema_version == REPORT_SCHEMA_VERSION)
        .map(|stored| stored.report)
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| right.ended_at.cmp(&left.ended_at));
    Ok(reports)
}

fn delete_report_at(root: &Path, id: &str) -> Result<(), String> {
    validate_report_id(id)?;
    let path = root.join(id);
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn prune_reports_at(root: &Path) -> Result<(), String> {
    let reports = list_reports_at(root)?;
    for report in reports.into_iter().skip(MAX_REPORTS) {
        let _ = delete_report_at(root, &report.id);
    }
    Ok(())
}

fn build_contract() -> Result<WorkEnvironmentContract, String> {
    let items = std::thread::scope(|scope| {
        let handles = ENVIRONMENT_SPECS
            .iter()
            .copied()
            .map(|spec| scope.spawn(move || inspect(spec)))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect::<Vec<_>>()
    });
    let fingerprint = fingerprint(&items);
    Ok(WorkEnvironmentContract {
        generated_at: chrono::Local::now().to_rfc3339(),
        fingerprint,
        items,
    })
}

fn inspect(spec: EnvironmentSpec) -> WorkEnvironmentItem {
    let path = resolve_command(spec.candidates);
    let version = path
        .as_deref()
        .and_then(|program| probe_version(spec.id, program, spec.version_args));
    let home_path = path
        .as_deref()
        .and_then(|program| ancestor(program, spec.home_levels))
        .map(|path| path.to_string_lossy().into_owned());
    WorkEnvironmentItem {
        id: spec.id.into(),
        label: spec.label.into(),
        kind: spec.kind.into(),
        available: path.is_some(),
        version,
        path: path.map(|path| path.to_string_lossy().into_owned()),
        home_variable: spec.home_variable.map(str::to_string),
        home_path,
    }
}

fn launch(request: WorkSessionLaunchRequest) -> Result<WorkSessionLaunchResult, String> {
    if !matches!(request.shell.as_str(), "powershell" | "cmd" | "gitbash") {
        return Err("Select a supported terminal.".into());
    }
    let workspace = std::fs::canonicalize(request.workspace.trim()).map_err(|_| {
        "The selected project folder does not exist or cannot be accessed.".to_string()
    })?;
    if !workspace.is_dir() {
        return Err("A project folder is required.".into());
    }
    let cli = crate::vibe::managed_cli(request.agent_id.trim())?;
    let contract = build_contract()?;
    let enabled = request.enabled_items.into_iter().collect::<HashSet<_>>();
    let unknown = enabled
        .iter()
        .find(|id| !ENVIRONMENT_SPECS.iter().any(|spec| spec.id == id.as_str()));
    if let Some(id) = unknown {
        return Err(format!("Unknown environment item: {id}"));
    }

    let selected = contract
        .items
        .iter()
        .filter(|item| item.available && enabled.contains(&item.id))
        .collect::<Vec<_>>();
    let path_dirs = session_path_dirs(&cli.path, &selected);
    let mut environment = vec![(
        "PATH".to_string(),
        std::env::join_paths(&path_dirs)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned(),
    )];
    for item in &selected {
        if let (Some(name), Some(value)) = (&item.home_variable, &item.home_path) {
            if !environment.iter().any(|(existing, _)| existing == name) {
                environment.push((name.clone(), value.clone()));
            }
        }
    }
    let session_id = format!(
        "work-session-{}-{}",
        std::process::id(),
        chrono::Local::now().timestamp_millis()
    );
    environment.extend([
        ("STACKER_WORK_SESSION".into(), session_id.clone()),
        (
            "STACKER_WORKSPACE".into(),
            workspace.to_string_lossy().into_owned(),
        ),
        ("STACKER_AGENT".into(), cli.id.clone()),
    ]);
    crate::installer::launch_managed_shell(
        &request.shell,
        &workspace,
        &cli.path,
        &environment,
        &cli.agent_name,
    )?;
    log::info!(
        target: "stacker::work_session",
        "managed work session launched: id={} agent={} shell={} workspace={} environment={}",
        session_id,
        cli.id,
        request.shell,
        workspace.display(),
        contract.fingerprint
    );
    Ok(WorkSessionLaunchResult {
        session_id,
        agent_id: cli.id,
        agent_name: cli.agent_name,
        cli_name: cli.cli_name.clone(),
        command_path: cli.path.to_string_lossy().into_owned(),
        workspace: workspace.to_string_lossy().into_owned(),
        shell: request.shell,
        started_at: chrono::Local::now().to_rfc3339(),
        environment_fingerprint: contract.fingerprint,
        environment: selected.into_iter().cloned().collect(),
        mode: "cli".into(),
        target_name: cli.cli_name,
        launch_kind: Some("launched".into()),
        process: None,
    })
}

fn launch_desktop(request: WorkSessionDesktopRequest) -> Result<WorkSessionLaunchResult, String> {
    if !matches!(request.action.as_str(), "launch" | "attach") {
        return Err("Select whether to launch the desktop app or attach a running app.".into());
    }
    let workspace = validate_workspace(&request.workspace)?;
    let desktop = crate::vibe::managed_desktop(request.agent_id.trim())?;
    let process = if request.action == "attach" {
        let pid = request
            .pid
            .ok_or_else(|| "Select a running desktop process to attach.".to_string())?;
        Some(
            desktop_candidates(&desktop.id)?
                .into_iter()
                .find(|process| process.pid == pid)
                .ok_or_else(|| "The selected desktop process is no longer running.".to_string())?,
        )
    } else {
        let before = desktop_candidates(&desktop.id)?
            .into_iter()
            .map(|process| process.pid)
            .collect::<HashSet<_>>();
        if desktop.target.is_none() {
            return Err(format!(
                "{} desktop app has no launch target.",
                desktop.agent_name
            ));
        }
        crate::vibe::open_desktop_tool(&desktop.id)?;
        find_launched_desktop_process(&desktop.id, &before)?
    };
    let session_id = format!(
        "work-session-{}-{}",
        std::process::id(),
        chrono::Local::now().timestamp_millis()
    );
    log::info!(
        target: "stacker::work_session",
        "desktop work session prepared: id={} agent={} action={} pid={:?} workspace={}",
        session_id,
        desktop.id,
        request.action,
        process.as_ref().map(|process| process.pid),
        workspace.display()
    );
    let environment_fingerprint = format!("desktop:{}", desktop.id);
    Ok(WorkSessionLaunchResult {
        session_id,
        agent_id: desktop.id,
        agent_name: desktop.agent_name,
        cli_name: String::new(),
        command_path: desktop.target.unwrap_or_default(),
        workspace: workspace.to_string_lossy().into_owned(),
        shell: "desktop".into(),
        started_at: chrono::Local::now().to_rfc3339(),
        environment_fingerprint,
        environment: Vec::new(),
        mode: "desktop".into(),
        target_name: desktop.desktop_name,
        launch_kind: Some(if request.action == "launch" {
            "launched".into()
        } else {
            "attached".into()
        }),
        process,
    })
}

fn validate_workspace(value: &str) -> Result<PathBuf, String> {
    let workspace = fs::canonicalize(value.trim()).map_err(|_| {
        "The selected project folder does not exist or cannot be accessed.".to_string()
    })?;
    if !workspace.is_dir() {
        return Err("A project folder is required.".into());
    }
    Ok(workspace)
}

fn desktop_candidates(agent_id: &str) -> Result<Vec<WorkSessionDesktopProcess>, String> {
    let desktop = crate::vibe::managed_desktop(agent_id.trim())?;
    let mut processes = crate::vibe::desktop_agent_processes(&desktop.id)?
        .into_iter()
        .map(|process| WorkSessionDesktopProcess {
            agent_id: process.agent_id,
            agent_name: desktop.agent_name.clone(),
            desktop_name: desktop.desktop_name.clone(),
            pid: process.pid,
            parent_pid: process.parent_pid,
            process_name: process.process_name,
        })
        .collect::<Vec<_>>();
    processes.sort_by_key(|process| process.pid);
    processes.dedup_by_key(|process| process.pid);
    Ok(processes)
}

fn find_launched_desktop_process(
    agent_id: &str,
    before: &HashSet<u32>,
) -> Result<Option<WorkSessionDesktopProcess>, String> {
    let mut existing = None;
    for _ in 0..4 {
        thread::sleep(Duration::from_millis(900));
        let processes = desktop_candidates(agent_id)?;
        if let Some(process) = processes
            .iter()
            .find(|process| !before.contains(&process.pid))
        {
            return Ok(Some(process.clone()));
        }
        if existing.is_none() {
            existing = processes.into_iter().next();
        }
    }
    Ok(existing)
}

fn process_is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        let filter = format!("PID eq {pid}");
        let mut command = Command::new("tasklist.exe");
        command.args(["/FI", &filter, "/FO", "CSV", "/NH"]);
        // Polled every few seconds; never flash a console window.
        std::os::windows::process::CommandExt::creation_flags(&mut command, 0x08000000);
        let output = command.output();
        let Ok(output) = output else {
            return false;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let expected_pid = pid.to_string();
        text.lines().any(|line| {
            let columns = line
                .split(',')
                .map(|column| column.trim().trim_matches('"'))
                .collect::<Vec<_>>();
            columns.get(1).is_some_and(|value| *value == expected_pid)
        })
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn command_dirs() -> Vec<PathBuf> {
    let mut dirs = crate::env::fresh_path_dirs();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dedup_paths(dirs)
}

fn session_path_dirs(cli_path: &Path, selected: &[&WorkEnvironmentItem]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(parent) = cli_path.parent() {
        directories.push(parent.to_path_buf());
    }
    directories.extend(
        selected
            .iter()
            .filter_map(|item| item.path.as_deref())
            .filter_map(|path| Path::new(path).parent())
            .map(Path::to_path_buf),
    );
    directories.extend(windows_command_dirs());
    dedup_paths(directories)
}

fn windows_command_dirs() -> Vec<PathBuf> {
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    [
        windows.join("System32"),
        windows.clone(),
        windows.join(r"System32\Wbem"),
        windows.join(r"System32\WindowsPowerShell\v1.0"),
        windows.join(r"System32\OpenSSH"),
    ]
    .into_iter()
    .filter(|path| path.is_dir())
    .collect()
}

fn resolve_command(candidates: &[&str]) -> Option<PathBuf> {
    command_dirs().into_iter().find_map(|dir| {
        let windows_apps = dir
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\windowsapps");
        candidates.iter().find_map(|candidate| {
            if windows_apps && candidate.eq_ignore_ascii_case("python.exe") {
                return None;
            }
            let path = dir.join(candidate);
            path.is_file().then_some(path)
        })
    })
}

fn probe_version(id: &str, program: &Path, args: &[&str]) -> Option<String> {
    let command = crate::vibe::command_for_path(program, args);
    let output = run_with_timeout(command, Duration::from_secs(6)).ok()?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    version_line(id, &text)
}

fn version_line(id: &str, text: &str) -> Option<String> {
    let lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if id == "gradle" {
        return lines
            .filter(|line| line.starts_with("Gradle "))
            .map(str::to_string)
            .next();
    }
    lines
        .filter(|line| !line.chars().all(|character| character == '-'))
        .map(str::to_string)
        .next()
}

fn run_with_timeout(
    mut command: std::process::Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::thread;
    use std::time::Instant;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    // Drain both pipes concurrently so a chatty child cannot block on a full pipe.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buffer);
            }
            buffer
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break status,
            None if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Version check timed out.".into());
            }
            None => thread::sleep(Duration::from_millis(40)),
        }
    };
    Ok(std::process::Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn ancestor(path: &Path, levels: usize) -> Option<&Path> {
    let mut current = path;
    for _ in 0..levels {
        current = current.parent()?;
    }
    Some(current)
}

fn dedup_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .filter(|path| seen.insert(path.to_string_lossy().to_ascii_lowercase()))
        .collect()
}

fn fingerprint(items: &[WorkEnvironmentItem]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for item in items {
        item.id.hash(&mut hasher);
        item.version.hash(&mut hasher);
        item.path.hash(&mut hasher);
        item.home_path.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::{
        dedup_paths, delete_report_at, list_reports_at, process_is_running, session_path_dirs,
        version_line, write_report_at, WorkEnvironmentItem, WorkSessionLaunchResult,
        WorkSessionReport,
    };
    use crate::space_analysis::monitor::MonitorSnapshot;
    use std::path::{Path, PathBuf};

    #[test]
    fn gradle_version_skips_banner_lines() {
        assert_eq!(
            version_line("gradle", "----------------\nGradle 9.1\n----------------"),
            Some("Gradle 9.1".into())
        );
    }

    #[test]
    fn path_order_is_preserved_while_deduplicating() {
        assert_eq!(
            dedup_paths(vec![
                PathBuf::from("C:\\One"),
                PathBuf::from("c:\\one"),
                PathBuf::from("D:\\Two")
            ]),
            vec![PathBuf::from("C:\\One"), PathBuf::from("D:\\Two")]
        );
    }

    #[test]
    fn managed_session_path_does_not_inherit_unselected_tools() {
        let selected = WorkEnvironmentItem {
            id: "node".into(),
            label: "Node.js".into(),
            kind: "runtime".into(),
            available: true,
            version: Some("v24".into()),
            path: Some(r"D:\tools\node\node.exe".into()),
            home_variable: None,
            home_path: None,
        };
        let paths = session_path_dirs(Path::new(r"D:\agents\codex.cmd"), &[&selected]);

        assert_eq!(paths[0], PathBuf::from(r"D:\agents"));
        assert_eq!(paths[1], PathBuf::from(r"D:\tools\node"));
        assert!(!paths
            .iter()
            .any(|path| path == &PathBuf::from(r"D:\tools\python")));
    }

    #[test]
    fn legacy_cli_launch_result_keeps_compatible_defaults() {
        let launch: WorkSessionLaunchResult = serde_json::from_value(serde_json::json!({
            "sessionId": "legacy-session",
            "agentId": "codex",
            "agentName": "Codex",
            "cliName": "codex",
            "commandPath": "C:\\tools\\codex.cmd",
            "workspace": "D:\\project",
            "shell": "powershell",
            "startedAt": "2026-08-12T11:00:00+08:00",
            "environmentFingerprint": "legacy"
        }))
        .expect("legacy launch data should remain readable");

        assert_eq!(launch.mode, "cli");
        assert!(launch.environment.is_empty());
        assert!(launch.target_name.is_empty());
        assert!(launch.launch_kind.is_none());
        assert!(launch.process.is_none());
    }

    #[test]
    fn work_session_reports_round_trip_and_delete() {
        let root = tempfile::tempdir().expect("temporary report directory should exist");
        let report = WorkSessionReport {
            id: "20260812-test-session.json".into(),
            ended_at: "2026-08-12T12:00:00+08:00".into(),
            launch: WorkSessionLaunchResult {
                session_id: "test-session".into(),
                agent_id: "codex".into(),
                agent_name: "Codex".into(),
                cli_name: "codex".into(),
                command_path: r"C:\tools\codex.cmd".into(),
                workspace: r"D:\project".into(),
                shell: "powershell".into(),
                started_at: "2026-08-12T11:00:00+08:00".into(),
                environment_fingerprint: "abc".into(),
                environment: vec![WorkEnvironmentItem {
                    id: "node".into(),
                    label: "Node.js".into(),
                    kind: "runtime".into(),
                    available: true,
                    version: Some("v24.0.0".into()),
                    path: Some(r"C:\tools\node.exe".into()),
                    home_variable: None,
                    home_path: None,
                }],
                mode: "cli".into(),
                target_name: "codex".into(),
                launch_kind: Some("launched".into()),
                process: None,
            },
            enabled_items: vec!["git".into(), "node".into()],
            tracking_roots: Vec::new(),
            monitor: MonitorSnapshot {
                task_id: "monitor-1".into(),
                state: "stopped".into(),
                roots: vec![r"D:\project".into()],
                started_at: "2026-08-12T11:00:00+08:00".into(),
                updated_at: "2026-08-12T12:00:00+08:00".into(),
                baseline_bytes: 10,
                current_bytes: 20,
                delta_bytes: 10,
                files_scanned: 1,
                files_changed: 1,
                directories_scanned: 1,
                skipped_paths: 0,
                running_agents: Vec::new(),
                directories: Vec::new(),
                events: Vec::new(),
                attribution_note: "context only".into(),
                error: None,
            },
            status: "completed".into(),
        };

        write_report_at(root.path(), &report).expect("report should be written");
        let reports = list_reports_at(root.path()).expect("report should be listed");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].launch.agent_id, "codex");
        delete_report_at(root.path(), &report.id).expect("report should be deleted");
        assert!(list_reports_at(root.path()).unwrap().is_empty());
    }

    #[test]
    fn desktop_process_status_distinguishes_running_and_stopped_processes() {
        assert!(process_is_running(std::process::id()));
        assert!(!process_is_running(u32::MAX));
    }
}
