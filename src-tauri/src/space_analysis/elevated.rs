use super::cleanup_plan::StoredCleanupPlan;
use super::cleanup_tasks::execute_plan;
use super::model::CleanupResult;
use super::walker::{build_indexed_result, CancellationToken, IndexedScanResult, WalkStats};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const REQUEST_VERSION: u32 = 1;
const RESULT_VERSION: u32 = 1;
const MAX_REQUEST_AGE_MS: i64 = 5 * 60 * 1_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ElevatedOperation {
    Cleanup {
        task_id: String,
        plan: StoredCleanupPlan,
    },
    DeepScan,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ElevatedRequest {
    version: u32,
    token: String,
    created_at_ms: i64,
    roots: Vec<String>,
    operation: ElevatedOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum ElevatedPayload {
    Cleanup(CleanupResult),
    DeepScan(IndexedScanResult),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ElevatedResponse {
    version: u32,
    token: String,
    payload: Option<ElevatedPayload>,
    error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct ElevatedProgress {
    files: u64,
    directories: u64,
    logical_bytes: u64,
    allocated_bytes: u64,
    skipped: u64,
}

impl ElevatedProgress {
    fn from_walk_stats(stats: &WalkStats) -> Self {
        Self {
            files: stats.files,
            directories: stats.directories,
            logical_bytes: stats.logical_bytes,
            allocated_bytes: stats.allocated_bytes,
            skipped: stats.skipped,
        }
    }

    fn to_walk_stats(&self) -> WalkStats {
        WalkStats {
            files: self.files,
            directories: self.directories,
            logical_bytes: self.logical_bytes,
            allocated_bytes: self.allocated_bytes,
            skipped: self.skipped,
            ..WalkStats::default()
        }
    }
}

pub(crate) fn helper_arg() -> Option<(String, String)> {
    let args = std::env::args().collect::<Vec<_>>();
    (args.len() >= 4 && args[1] == "__space_helper").then(|| (args[2].clone(), args[3].clone()))
}

pub(crate) fn run_helper_from_file(file: &str, token: &str) -> i32 {
    let request_path = Path::new(file);
    let response_path = response_file(request_path);
    let progress_path = progress_file(request_path);
    let result = read_and_validate_request(request_path, token, Utc::now().timestamp_millis())
        .and_then(|request| execute_request(request, Some(&progress_path)));
    let response = match result {
        Ok(payload) => ElevatedResponse {
            version: RESULT_VERSION,
            token: token.to_string(),
            payload: Some(payload),
            error: None,
        },
        Err(error) => ElevatedResponse {
            version: RESULT_VERSION,
            token: token.to_string(),
            payload: None,
            error: Some(error),
        },
    };
    let exit_code = if response.error.is_none() { 0 } else { 2 };
    let _ = write_json_atomic(&response_path, &response);
    let _ = std::fs::remove_file(request_path);
    exit_code
}

pub(crate) fn run_cleanup(
    plan: StoredCleanupPlan,
    selected_node_ids: &[String],
) -> Result<CleanupResult, String> {
    let plan = super::cleanup_tasks::select_plan_items(plan, selected_node_ids)?;
    let roots = plan
        .validation
        .values()
        .flat_map(|validation| validation.allowed_roots.iter())
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let task_id = format!("cleanup-elevated-{}", request_token());
    let request = ElevatedRequest {
        version: REQUEST_VERSION,
        token: request_token(),
        created_at_ms: Utc::now().timestamp_millis(),
        roots,
        operation: ElevatedOperation::Cleanup { task_id, plan },
    };
    match run_request(request, None, None)? {
        ElevatedPayload::Cleanup(result) => Ok(result),
        ElevatedPayload::DeepScan(_) => {
            Err("The elevated helper returned an unexpected result.".into())
        }
    }
}

pub(crate) fn run_deep_scan(
    roots: &[String],
    cancellation: &CancellationToken,
    report_progress: &mut dyn FnMut(&WalkStats),
) -> Result<IndexedScanResult, String> {
    let request = ElevatedRequest {
        version: REQUEST_VERSION,
        token: request_token(),
        created_at_ms: Utc::now().timestamp_millis(),
        roots: roots.to_vec(),
        operation: ElevatedOperation::DeepScan,
    };
    match run_request(request, Some(cancellation), Some(report_progress))? {
        ElevatedPayload::DeepScan(mut result) => {
            result.restore_after_transfer();
            Ok(result)
        }
        ElevatedPayload::Cleanup(_) => {
            Err("The elevated helper returned an unexpected result.".into())
        }
    }
}

fn execute_request(
    request: ElevatedRequest,
    progress_path: Option<&Path>,
) -> Result<ElevatedPayload, String> {
    match request.operation {
        ElevatedOperation::Cleanup { task_id, plan } => Ok(ElevatedPayload::Cleanup(execute_plan(
            &task_id,
            plan,
            &CancellationToken::default(),
            |_, _, _| {},
        ))),
        ElevatedOperation::DeepScan => {
            let roots = request
                .roots
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            let result = build_indexed_result(&roots, &CancellationToken::default(), |stats| {
                if let Some(path) = progress_path {
                    let _ = write_json_atomic(path, &ElevatedProgress::from_walk_stats(stats));
                }
            })
            .map_err(|error| error.to_string())?;
            Ok(ElevatedPayload::DeepScan(result))
        }
    }
}

fn read_and_validate_request(
    path: &Path,
    expected_token: &str,
    now_ms: i64,
) -> Result<ElevatedRequest, String> {
    validate_request_path(path, expected_token)?;
    let bytes =
        std::fs::read(path).map_err(|_| "The elevated request is unavailable.".to_string())?;
    let request = serde_json::from_slice::<ElevatedRequest>(&bytes)
        .map_err(|_| "The elevated request is invalid.".to_string())?;
    validate_request(&request, expected_token, now_ms)?;
    Ok(request)
}

fn validate_request(
    request: &ElevatedRequest,
    expected_token: &str,
    now_ms: i64,
) -> Result<(), String> {
    if request.version != REQUEST_VERSION {
        return Err("The elevated request version is not supported.".into());
    }
    if request.token != expected_token || request.token.is_empty() {
        return Err("The elevated request token is invalid.".into());
    }
    let age = now_ms.saturating_sub(request.created_at_ms);
    if !(0..=MAX_REQUEST_AGE_MS).contains(&age) {
        return Err("The elevated request has expired.".into());
    }
    if request.roots.is_empty() {
        return Err("The elevated request does not contain a scan root.".into());
    }
    let roots = request
        .roots
        .iter()
        .map(|root| normalize_path(Path::new(root)))
        .collect::<Vec<_>>();
    match &request.operation {
        ElevatedOperation::Cleanup { plan, .. } => {
            if plan.plan.items.is_empty() {
                return Err("The elevated cleanup request is empty.".into());
            }
            for item in &plan.plan.items {
                let Some(validation) = plan.validation.get(&item.node_id) else {
                    return Err("The elevated cleanup allow-list is incomplete.".into());
                };
                let item_path = normalize_path(Path::new(&item.path));
                if !roots
                    .iter()
                    .any(|root| is_same_or_descendant(&item_path, root))
                {
                    return Err("An elevated cleanup item is outside the approved roots.".into());
                }
                if validation.allowed_roots.is_empty()
                    || validation
                        .allowed_roots
                        .iter()
                        .any(|root| !roots.contains(&normalize_path(root)))
                {
                    return Err("The elevated cleanup validation roots are invalid.".into());
                }
            }
        }
        ElevatedOperation::DeepScan => {}
    }
    Ok(())
}

fn validate_request_path(path: &Path, token: &str) -> Result<(), String> {
    let expected = request_file(token);
    let actual = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let expected = expected.canonicalize().unwrap_or(expected);
    if normalize_path(&actual) != normalize_path(&expected) {
        return Err("The elevated request path is invalid.".into());
    }
    Ok(())
}

fn run_request(
    request: ElevatedRequest,
    cancellation: Option<&CancellationToken>,
    report_progress: Option<&mut dyn FnMut(&WalkStats)>,
) -> Result<ElevatedPayload, String> {
    let path = request_file(&request.token);
    let response_path = response_file(&path);
    let progress_path = progress_file(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let _ = std::fs::remove_file(&response_path);
    let _ = std::fs::remove_file(&progress_path);
    write_json_atomic(&path, &request)?;
    let run_result = run_elevated_self(
        &path,
        &request.token,
        cancellation,
        &progress_path,
        report_progress,
    );
    if let Err(error) = run_result {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&response_path);
        let _ = std::fs::remove_file(&progress_path);
        return Err(error);
    }
    let response_bytes = std::fs::read(&response_path);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&response_path);
    let _ = std::fs::remove_file(&progress_path);
    let bytes =
        response_bytes.map_err(|_| "The elevated helper did not return a result.".to_string())?;
    let response = serde_json::from_slice::<ElevatedResponse>(&bytes)
        .map_err(|_| "The elevated helper returned an invalid result.".to_string())?;
    if response.version != RESULT_VERSION || response.token != request.token {
        return Err("The elevated helper result could not be verified.".into());
    }
    if let Some(error) = response.error {
        return Err(error);
    }
    response
        .payload
        .ok_or_else(|| "The elevated helper returned an empty result.".into())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn request_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn helper_dir() -> PathBuf {
    std::env::temp_dir().join("stacker")
}

fn request_file(token: &str) -> PathBuf {
    helper_dir().join(format!("space-helper-{token}.json"))
}

fn response_file(request: &Path) -> PathBuf {
    request.with_extension("result.json")
}

fn progress_file(request: &Path) -> PathBuf {
    request.with_extension("progress.json")
}

fn forward_progress(
    progress_path: &Path,
    last_progress: &mut Option<ElevatedProgress>,
    report_progress: &mut Option<&mut dyn FnMut(&WalkStats)>,
) {
    let Ok(bytes) = std::fs::read(progress_path) else {
        return;
    };
    let Ok(progress) = serde_json::from_slice::<ElevatedProgress>(&bytes) else {
        return;
    };
    if last_progress.as_ref() == Some(&progress) {
        return;
    }
    if let Some(report) = report_progress.as_deref_mut() {
        report(&progress.to_walk_stats());
    }
    *last_progress = Some(progress);
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn is_same_or_descendant(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('\\'))
}

#[cfg(windows)]
fn run_elevated_self(
    file: &Path,
    token: &str,
    cancellation: Option<&CancellationToken>,
    progress_path: &Path,
    mut report_progress: Option<&mut dyn FnMut(&WalkStats)>,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::shared::winerror::WAIT_TIMEOUT;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetExitCodeProcess, TerminateProcess};
    use winapi::um::shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::WAIT_OBJECT_0;
    use winapi::um::winuser::SW_HIDE;

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable = executable
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let verb = "runas\0".encode_utf16().collect::<Vec<_>>();
    let parameters = format!("__space_helper \"{}\" \"{token}\"", file.display());
    let parameters = parameters
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = executable.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err("Administrator approval was cancelled or could not be started.".into());
        }
        let mut last_progress = None;
        loop {
            let wait_result = WaitForSingleObject(info.hProcess, 200);
            if wait_result == WAIT_OBJECT_0 {
                forward_progress(progress_path, &mut last_progress, &mut report_progress);
                break;
            }
            if wait_result != WAIT_TIMEOUT {
                CloseHandle(info.hProcess);
                return Err("The elevated helper could not be monitored.".into());
            }
            if cancellation.is_some_and(CancellationToken::is_cancelled) {
                TerminateProcess(info.hProcess, 3);
                WaitForSingleObject(info.hProcess, 5_000);
                CloseHandle(info.hProcess);
                return Err("The elevated scan was cancelled.".into());
            }
            forward_progress(progress_path, &mut last_progress, &mut report_progress);
        }
        let mut exit_code = 1;
        GetExitCodeProcess(info.hProcess, &mut exit_code);
        CloseHandle(info.hProcess);
        if exit_code == 0 || exit_code == 2 {
            Ok(())
        } else {
            Err("The elevated helper stopped before returning a result.".into())
        }
    }
}

#[cfg(not(windows))]
fn run_elevated_self(
    _: &Path,
    _: &str,
    _: Option<&CancellationToken>,
    _: &Path,
    _: Option<&mut dyn FnMut(&WalkStats)>,
) -> Result<(), String> {
    Err("Elevated space analysis is available only on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::cleanup_plan::build_deep_plan_record;
    use crate::space_analysis::walker::{build_indexed_result, CancellationToken};

    fn cleanup_request(root: &Path) -> ElevatedRequest {
        std::fs::write(root.join("Cargo.toml"), b"[package]").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        let index =
            build_indexed_result(&[root.to_path_buf()], &CancellationToken::default(), |_| {})
                .unwrap();
        let root_node = index.summary().root_nodes[0].clone();
        let node_id = index.children(&root_node.node_id, 0, 10).unwrap().items[0]
            .node_id
            .clone();
        let plan = build_deep_plan_record(&index, "scan-1", "plan-1".into(), &[node_id]).unwrap();
        ElevatedRequest {
            version: REQUEST_VERSION,
            token: "test-token".into(),
            created_at_ms: 1_000,
            roots: vec![root.to_string_lossy().into_owned()],
            operation: ElevatedOperation::Cleanup {
                task_id: "cleanup-1".into(),
                plan,
            },
        }
    }

    #[test]
    fn rejects_mismatched_expired_and_unsupported_requests() {
        let root = tempfile::tempdir().unwrap();
        let request = cleanup_request(root.path());
        assert!(validate_request(&request, "wrong", 1_001).is_err());
        assert!(validate_request(&request, "test-token", 1_000 + MAX_REQUEST_AGE_MS + 1).is_err());
        let mut unsupported = request;
        unsupported.version = 2;
        assert!(validate_request(&unsupported, "test-token", 1_001).is_err());
    }

    #[test]
    fn rejects_nodes_outside_roots_and_incomplete_allow_lists() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let mut request = cleanup_request(root.path());
        let ElevatedOperation::Cleanup { plan, .. } = &mut request.operation else {
            unreachable!();
        };
        plan.plan.items[0].path = outside.path().join("target").to_string_lossy().into_owned();
        assert!(validate_request(&request, "test-token", 1_001).is_err());

        let mut request = cleanup_request(root.path());
        let ElevatedOperation::Cleanup { plan, .. } = &mut request.operation else {
            unreachable!();
        };
        plan.validation.clear();
        assert!(validate_request(&request, "test-token", 1_001).is_err());
    }

    #[test]
    fn response_round_trip_preserves_token_and_payload() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        let index = build_indexed_result(
            &[root.path().to_path_buf()],
            &CancellationToken::default(),
            |_| {},
        )
        .unwrap();
        let response = ElevatedResponse {
            version: RESULT_VERSION,
            token: "token".into(),
            payload: Some(ElevatedPayload::DeepScan(index)),
            error: None,
        };
        let json = serde_json::to_vec(&response).unwrap();
        let mut decoded = serde_json::from_slice::<ElevatedResponse>(&json).unwrap();
        assert_eq!(decoded.version, RESULT_VERSION);
        assert_eq!(decoded.token, "token");
        let Some(ElevatedPayload::DeepScan(index)) = decoded.payload.as_mut() else {
            panic!("expected a transferred deep scan");
        };
        index.restore_after_transfer();
        let root_id = index.summary().root_nodes[0].node_id.clone();
        assert_eq!(index.children(&root_id, 0, 10).unwrap().total, 1);
    }

    #[test]
    fn deep_scan_helper_publishes_progress_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("data.bin"), vec![7_u8; 512]).unwrap();
        let progress_dir = tempfile::tempdir().unwrap();
        let progress_path = progress_dir.path().join("scan.progress.json");
        let request = ElevatedRequest {
            version: REQUEST_VERSION,
            token: "progress-token".into(),
            created_at_ms: 1_000,
            roots: vec![root.path().to_string_lossy().into_owned()],
            operation: ElevatedOperation::DeepScan,
        };

        let payload = execute_request(request, Some(&progress_path)).unwrap();
        assert!(matches!(payload, ElevatedPayload::DeepScan(_)));
        let progress =
            serde_json::from_slice::<ElevatedProgress>(&std::fs::read(&progress_path).unwrap())
                .unwrap();
        assert_eq!(progress.files, 1);
        assert!(progress.directories >= 2);
        assert!(progress.allocated_bytes > 0);
    }

    #[test]
    fn parent_forwards_each_progress_snapshot_once() {
        let progress_dir = tempfile::tempdir().unwrap();
        let progress_path = progress_dir.path().join("scan.progress.json");
        let progress = ElevatedProgress {
            files: 12,
            directories: 4,
            logical_bytes: 1_024,
            allocated_bytes: 4_096,
            skipped: 2,
        };
        write_json_atomic(&progress_path, &progress).unwrap();

        let mut received = Vec::new();
        let mut reporter = |stats: &WalkStats| received.push(stats.clone());
        let mut callback: Option<&mut dyn FnMut(&WalkStats)> = Some(&mut reporter);
        let mut last = None;
        forward_progress(&progress_path, &mut last, &mut callback);
        forward_progress(&progress_path, &mut last, &mut callback);

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].files, 12);
        assert_eq!(received[0].directories, 4);
        assert_eq!(received[0].allocated_bytes, 4_096);
        assert_eq!(received[0].skipped, 2);
    }
}
