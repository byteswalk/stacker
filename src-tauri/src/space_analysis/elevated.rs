use super::cleanup_plan::StoredCleanupPlan;
use super::cleanup_tasks::execute_plan;
use super::model::{AnalysisSummary, CleanupResult};
use super::walker::{build_indexed_result, CancellationToken};
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
    SupplementScan {
        task_id: String,
    },
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
    SupplementScan(AnalysisSummary),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ElevatedResponse {
    version: u32,
    token: String,
    payload: Option<ElevatedPayload>,
    error: Option<String>,
}

pub(crate) fn helper_arg() -> Option<(String, String)> {
    let args = std::env::args().collect::<Vec<_>>();
    (args.len() >= 4 && args[1] == "__space_helper").then(|| (args[2].clone(), args[3].clone()))
}

pub(crate) fn run_helper_from_file(file: &str, token: &str) -> i32 {
    let request_path = Path::new(file);
    let response_path = response_file(request_path);
    let result = read_and_validate_request(request_path, token, Utc::now().timestamp_millis())
        .and_then(execute_request);
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
    match run_request(request)? {
        ElevatedPayload::Cleanup(result) => Ok(result),
        ElevatedPayload::SupplementScan(_) => {
            Err("The elevated helper returned an unexpected result.".into())
        }
    }
}

pub(crate) fn run_supplement_scan(
    task_id: &str,
    roots: &[String],
) -> Result<AnalysisSummary, String> {
    let request = ElevatedRequest {
        version: REQUEST_VERSION,
        token: request_token(),
        created_at_ms: Utc::now().timestamp_millis(),
        roots: roots.to_vec(),
        operation: ElevatedOperation::SupplementScan {
            task_id: task_id.to_string(),
        },
    };
    match run_request(request)? {
        ElevatedPayload::SupplementScan(result) => Ok(result),
        ElevatedPayload::Cleanup(_) => {
            Err("The elevated helper returned an unexpected result.".into())
        }
    }
}

fn execute_request(request: ElevatedRequest) -> Result<ElevatedPayload, String> {
    match request.operation {
        ElevatedOperation::Cleanup { task_id, plan } => Ok(ElevatedPayload::Cleanup(execute_plan(
            &task_id,
            plan,
            &CancellationToken::default(),
            |_, _, _| {},
        ))),
        ElevatedOperation::SupplementScan { task_id } => {
            let roots = request
                .roots
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            let result = build_indexed_result(&roots, &CancellationToken::default(), |_| {})
                .map_err(|error| error.to_string())?;
            let mut summary = result.summary();
            summary.task_id = task_id;
            Ok(ElevatedPayload::SupplementScan(summary))
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
    if age < 0 || age > MAX_REQUEST_AGE_MS {
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
        ElevatedOperation::SupplementScan { .. } => {}
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

fn run_request(request: ElevatedRequest) -> Result<ElevatedPayload, String> {
    let path = request_file(&request.token);
    let response_path = response_file(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    write_json_atomic(&path, &request)?;
    let run_result = run_elevated_self(&path, &request.token);
    if let Err(error) = run_result {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&response_path);
        return Err(error);
    }
    let bytes = std::fs::read(&response_path)
        .map_err(|_| "The elevated helper did not return a result.".to_string())?;
    let _ = std::fs::remove_file(&response_path);
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
fn run_elevated_self(file: &Path, token: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::GetExitCodeProcess;
    use winapi::um::shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::INFINITE;
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
        WaitForSingleObject(info.hProcess, INFINITE);
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
fn run_elevated_self(_: &Path, _: &str) -> Result<(), String> {
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
        let response = ElevatedResponse {
            version: RESULT_VERSION,
            token: "token".into(),
            payload: Some(ElevatedPayload::SupplementScan(AnalysisSummary::default())),
            error: None,
        };
        let json = serde_json::to_vec(&response).unwrap();
        let decoded = serde_json::from_slice::<ElevatedResponse>(&json).unwrap();
        assert_eq!(decoded.version, RESULT_VERSION);
        assert_eq!(decoded.token, "token");
        assert!(matches!(
            decoded.payload,
            Some(ElevatedPayload::SupplementScan(_))
        ));
    }
}
