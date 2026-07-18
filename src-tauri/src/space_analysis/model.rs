use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ProjectKind {
    Node,
    Rust,
    Maven,
    Gradle,
    Go,
    DotNet,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRoot {
    pub project_id: String,
    pub node_id: String,
    pub path: String,
    pub kind: ProjectKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScanMode {
    Quick,
    Directories,
    Drives,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub mode: ScanMode,
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub root: String,
    pub label: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub fixed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScanTaskState {
    Queued,
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub task_id: String,
    pub state: ScanTaskState,
    pub scanned_files: u64,
    pub scanned_directories: u64,
    pub accounted_bytes: u64,
    pub skipped_paths: u64,
    pub elapsed_ms: u64,
    pub current_path: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanErrorSummary {
    pub access_denied: u64,
    pub vanished: u64,
    pub invalid_target: u64,
    pub other: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownSpaceItem {
    pub id: String,
    pub name_key: String,
    pub path: String,
    pub bytes: u64,
    pub safety: String,
    pub cleanup_kind: String,
    pub ecosystem: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickScanResult {
    pub task_id: String,
    pub completed: bool,
    pub total_bytes: u64,
    pub safely_releasable_bytes: u64,
    pub items: Vec<KnownSpaceItem>,
    pub errors: ScanErrorSummary,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSummary {
    pub task_id: String,
    pub targets: Vec<String>,
    pub allocated_bytes: u64,
    pub logical_bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
    pub skipped_paths: u64,
    pub root_nodes: Vec<DirectoryNode>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryNode {
    pub node_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub path: String,
    pub allocated_bytes: u64,
    pub logical_bytes: u64,
    pub child_count: u32,
    pub safety: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFileRow {
    pub node_id: String,
    pub name: String,
    pub path: String,
    pub allocated_bytes: u64,
    pub logical_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub offset: u64,
    pub limit: u64,
    pub total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_contract_uses_camel_case_and_explicit_states() {
        let value = serde_json::to_value(ScanProgress {
            task_id: "scan-1".into(),
            state: ScanTaskState::Running,
            scanned_files: 12,
            scanned_directories: 3,
            accounted_bytes: 4096,
            skipped_paths: 1,
            elapsed_ms: 250,
            current_path: Some(r"C:\\Users\\demo".into()),
        })
        .unwrap();
        assert_eq!(value["taskId"], "scan-1");
        assert_eq!(value["state"], "running");
        assert_eq!(value["scannedFiles"], 12);
    }
}
