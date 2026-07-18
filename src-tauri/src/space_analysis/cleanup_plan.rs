use super::known::CleanupKind;
use super::model::{
    CleanupPlan, CleanupPlanItem, ElevationRequirement, ProjectKind, QuickScanResult, SafetyClass,
};
use super::walker::IndexedScanResult;
use super::windows_fs::{file_identity, FileIdentity};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanError {
    EmptySelection,
    DuplicateNode(String),
    UnknownNode(String),
    NotCleanable(String),
    MissingMetadata(String),
    OverlappingNodes(String, String),
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => formatter.write_str("Select at least one cleanup item."),
            Self::DuplicateNode(node_id) => {
                write!(
                    formatter,
                    "Cleanup item {node_id} was selected more than once."
                )
            }
            Self::UnknownNode(node_id) => {
                write!(formatter, "Cleanup item {node_id} was not found.")
            }
            Self::NotCleanable(node_id) => {
                write!(formatter, "Cleanup item {node_id} is view-only.")
            }
            Self::MissingMetadata(node_id) => {
                write!(
                    formatter,
                    "Cleanup item {node_id} is missing validation metadata."
                )
            }
            Self::OverlappingNodes(left, right) => write!(
                formatter,
                "Cleanup items {left} and {right} overlap. Select only the containing item."
            ),
        }
    }
}

impl std::error::Error for PlanError {}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) enum ValidationSource {
    Known {
        candidate_id: String,
    },
    ProjectArtifact {
        project_root: PathBuf,
        project_kind: ProjectKind,
        project_evidence: HashSet<String>,
    },
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct PlanValidation {
    pub(crate) expected_identity: FileIdentity,
    pub(crate) allowed_roots: Vec<PathBuf>,
    pub(crate) source: ValidationSource,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct StoredCleanupPlan {
    pub(crate) plan: CleanupPlan,
    pub(crate) validation: HashMap<String, PlanValidation>,
}

#[cfg(test)]
pub(crate) fn build_deep_plan(
    result: &IndexedScanResult,
    scan_task_id: &str,
    plan_id: String,
    node_ids: &[String],
) -> Result<CleanupPlan, PlanError> {
    Ok(build_deep_plan_record(result, scan_task_id, plan_id, node_ids)?.plan)
}

pub(crate) fn build_deep_plan_record(
    result: &IndexedScanResult,
    scan_task_id: &str,
    plan_id: String,
    node_ids: &[String],
) -> Result<StoredCleanupPlan, PlanError> {
    validate_selection(node_ids)?;
    let mut items = Vec::with_capacity(node_ids.len());
    let mut validation = HashMap::with_capacity(node_ids.len());
    let allowed_roots = result
        .summary()
        .targets
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    for node_id in node_ids {
        let indexed = result
            .cleanup_node(node_id)
            .ok_or_else(|| PlanError::UnknownNode(node_id.clone()))?;
        if indexed.classification.safety == SafetyClass::ViewOnly
            || indexed.classification.cleanup_kind == CleanupKind::None
        {
            return Err(PlanError::NotCleanable(node_id.clone()));
        }
        let impact_key = indexed
            .classification
            .impact_key
            .ok_or_else(|| PlanError::MissingMetadata(node_id.clone()))?;
        let expected_identity = indexed
            .identity
            .ok_or_else(|| PlanError::MissingMetadata(node_id.clone()))?;
        let project_root = indexed
            .project_root_path
            .map(PathBuf::from)
            .ok_or_else(|| PlanError::MissingMetadata(node_id.clone()))?;
        let project_kind = indexed
            .project_kind
            .ok_or_else(|| PlanError::MissingMetadata(node_id.clone()))?;
        let project_evidence = indexed
            .project_evidence
            .cloned()
            .ok_or_else(|| PlanError::MissingMetadata(node_id.clone()))?;
        items.push(CleanupPlanItem {
            node_id: node_id.clone(),
            path: indexed.node.path.clone(),
            estimated_bytes: indexed.node.allocated_bytes,
            safety: indexed.classification.safety,
            impact_key: impact_key.to_string(),
            cleanup_kind: indexed.classification.cleanup_kind.as_str().to_string(),
            requires_elevation: path_requires_elevation(Path::new(&indexed.node.path)),
            default_selected: indexed.classification.safety == SafetyClass::Safe,
        });
        validation.insert(
            node_id.clone(),
            PlanValidation {
                expected_identity,
                allowed_roots: allowed_roots.clone(),
                source: ValidationSource::ProjectArtifact {
                    project_root,
                    project_kind,
                    project_evidence,
                },
            },
        );
    }
    reject_overlapping_items(&items)?;
    Ok(StoredCleanupPlan {
        plan: finalize_plan(scan_task_id, plan_id, items),
        validation,
    })
}

#[cfg(test)]
pub(crate) fn build_quick_plan(
    result: &QuickScanResult,
    scan_task_id: &str,
    plan_id: String,
    node_ids: &[String],
) -> Result<CleanupPlan, PlanError> {
    Ok(build_quick_plan_record(result, scan_task_id, plan_id, node_ids)?.plan)
}

pub(crate) fn build_quick_plan_record(
    result: &QuickScanResult,
    scan_task_id: &str,
    plan_id: String,
    node_ids: &[String],
) -> Result<StoredCleanupPlan, PlanError> {
    validate_selection(node_ids)?;
    let mut items = Vec::with_capacity(node_ids.len());
    let mut validation = HashMap::with_capacity(node_ids.len());
    for node_id in node_ids {
        let candidate = result
            .items
            .iter()
            .find(|item| item.id == *node_id)
            .ok_or_else(|| PlanError::UnknownNode(node_id.clone()))?;
        let safety = SafetyClass::from_stable_str(&candidate.safety)
            .ok_or_else(|| PlanError::MissingMetadata(node_id.clone()))?;
        let cleanup_kind = CleanupKind::from_stable_str(&candidate.cleanup_kind)
            .ok_or_else(|| PlanError::MissingMetadata(node_id.clone()))?;
        if safety == SafetyClass::ViewOnly || cleanup_kind == CleanupKind::None {
            return Err(PlanError::NotCleanable(node_id.clone()));
        }
        let expected_identity = file_identity(Path::new(&candidate.path))
            .map_err(|_| PlanError::MissingMetadata(node_id.clone()))?;
        items.push(CleanupPlanItem {
            node_id: node_id.clone(),
            path: candidate.path.clone(),
            estimated_bytes: candidate.bytes,
            safety,
            impact_key: candidate.name_key.clone(),
            cleanup_kind: cleanup_kind.as_str().to_string(),
            requires_elevation: path_requires_elevation(Path::new(&candidate.path)),
            default_selected: safety == SafetyClass::Safe,
        });
        validation.insert(
            node_id.clone(),
            PlanValidation {
                expected_identity,
                allowed_roots: vec![PathBuf::from(&candidate.path)],
                source: ValidationSource::Known {
                    candidate_id: node_id.clone(),
                },
            },
        );
    }
    reject_overlapping_items(&items)?;
    Ok(StoredCleanupPlan {
        plan: finalize_plan(scan_task_id, plan_id, items),
        validation,
    })
}

fn finalize_plan(scan_task_id: &str, plan_id: String, items: Vec<CleanupPlanItem>) -> CleanupPlan {
    let estimated_bytes = items
        .iter()
        .map(|item| item.estimated_bytes)
        .fold(0u64, u64::saturating_add);
    let elevation_requirement = if items.iter().any(|item| item.requires_elevation) {
        ElevationRequirement::Required
    } else {
        ElevationRequirement::None
    };
    CleanupPlan {
        plan_id,
        scan_task_id: scan_task_id.to_string(),
        created_at: Utc::now().to_rfc3339(),
        estimated_bytes,
        elevation_requirement,
        items,
    }
}

fn validate_selection(node_ids: &[String]) -> Result<(), PlanError> {
    if node_ids.is_empty() {
        return Err(PlanError::EmptySelection);
    }
    let mut seen = HashSet::with_capacity(node_ids.len());
    for node_id in node_ids {
        if !seen.insert(node_id) {
            return Err(PlanError::DuplicateNode(node_id.clone()));
        }
    }
    Ok(())
}

fn reject_overlapping_items(items: &[CleanupPlanItem]) -> Result<(), PlanError> {
    for (index, left) in items.iter().enumerate() {
        for right in items.iter().skip(index + 1) {
            if paths_overlap(Path::new(&left.path), Path::new(&right.path)) {
                return Err(PlanError::OverlappingNodes(
                    left.node_id.clone(),
                    right.node_id.clone(),
                ));
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    let left = normalize_windows_path(left);
    let right = normalize_windows_path(right);
    is_same_or_descendant(&left, &right) || is_same_or_descendant(&right, &left)
}

fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn is_same_or_descendant(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('\\'))
}

fn path_requires_elevation(path: &Path) -> bool {
    let roots = [
        std::env::var_os("WINDIR"),
        std::env::var_os("ProgramFiles"),
        std::env::var_os("ProgramFiles(x86)"),
        std::env::var_os("ProgramData"),
    ]
    .into_iter()
    .flatten()
    .map(PathBuf::from)
    .collect::<Vec<_>>();
    path_requires_elevation_with_roots(path, &roots)
}

fn path_requires_elevation_with_roots(path: &Path, roots: &[PathBuf]) -> bool {
    let normalized = normalize_windows_path(path);
    roots.iter().any(|root| {
        let root = normalize_windows_path(root);
        is_same_or_descendant(&normalized, &root)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::walker::{build_indexed_result, CancellationToken};
    use std::fs;

    fn fixture_result() -> IndexedScanResult {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("Cargo.toml"), b"[package]").unwrap();
        fs::create_dir(root.join("target")).unwrap();
        fs::create_dir(root.join("notes")).unwrap();
        build_indexed_result(&[root], &CancellationToken::default(), |_| {}).unwrap()
    }

    #[test]
    fn plan_rejects_view_only_and_unknown_nodes() {
        let result = fixture_result();
        let root = result.summary().root_nodes[0].clone();
        let children = result.children(&root.node_id, 0, 10).unwrap();
        let view_only = children
            .items
            .iter()
            .find(|item| item.name == "notes")
            .unwrap();

        assert!(matches!(
            build_deep_plan(
                &result,
                "scan-1",
                "plan-1".into(),
                std::slice::from_ref(&view_only.node_id)
            ),
            Err(PlanError::NotCleanable(_))
        ));
        assert!(matches!(
            build_deep_plan(&result, "scan-1", "plan-2".into(), &["missing".into()]),
            Err(PlanError::UnknownNode(_))
        ));
    }

    #[test]
    fn plan_never_preselects_rebuildable_or_confirmation_items() {
        let result = fixture_result();
        let root = result.summary().root_nodes[0].clone();
        let target = result
            .children(&root.node_id, 0, 10)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.name == "target")
            .unwrap();
        let deep_plan =
            build_deep_plan(&result, "scan-1", "plan-1".into(), &[target.node_id]).unwrap();
        assert!(!deep_plan.items[0].default_selected);

        let quick_root = tempfile::tempdir().unwrap();
        let safe = quick_root.path().join("safe");
        let confirm = quick_root.path().join("confirm");
        fs::create_dir(&safe).unwrap();
        fs::create_dir(&confirm).unwrap();
        let quick = QuickScanResult {
            task_id: "scan-2".into(),
            completed: true,
            total_bytes: 30,
            safely_releasable_bytes: 10,
            items: vec![
                known_item("safe", &safe, SafetyClass::Safe, 10),
                known_item("confirm", &confirm, SafetyClass::NeedsConfirmation, 20),
            ],
            errors: Default::default(),
        };
        let quick_plan = build_quick_plan(
            &quick,
            "scan-2",
            "plan-2".into(),
            &["safe".into(), "confirm".into()],
        )
        .unwrap();
        assert!(quick_plan.items[0].default_selected);
        assert!(!quick_plan.items[1].default_selected);
    }

    fn known_item(
        id: &str,
        path: &Path,
        safety: SafetyClass,
        bytes: u64,
    ) -> super::super::model::KnownSpaceItem {
        super::super::model::KnownSpaceItem {
            id: id.into(),
            name_key: format!("spaceAnalysis.known.{id}"),
            path: path.to_string_lossy().into_owned(),
            bytes,
            safety: safety.as_str().into(),
            cleanup_kind: CleanupKind::Contents.as_str().into(),
            ecosystem: None,
        }
    }

    #[test]
    fn plan_rejects_duplicate_and_overlapping_nodes() {
        let duplicate = vec!["same".to_string(), "same".to_string()];
        assert!(matches!(
            validate_selection(&duplicate),
            Err(PlanError::DuplicateNode(_))
        ));

        let items = vec![
            CleanupPlanItem {
                node_id: "parent".into(),
                path: r"C:\project\target".into(),
                estimated_bytes: 1,
                safety: SafetyClass::Rebuildable,
                impact_key: "parent".into(),
                cleanup_kind: "wholeDirectory".into(),
                requires_elevation: false,
                default_selected: false,
            },
            CleanupPlanItem {
                node_id: "child".into(),
                path: r"C:\project\target\nested".into(),
                estimated_bytes: 1,
                safety: SafetyClass::Rebuildable,
                impact_key: "child".into(),
                cleanup_kind: "wholeDirectory".into(),
                requires_elevation: false,
                default_selected: false,
            },
        ];
        assert!(matches!(
            reject_overlapping_items(&items),
            Err(PlanError::OverlappingNodes(_, _))
        ));
        assert!(paths_overlap(
            Path::new(r"C:\PROJECT\TARGET"),
            Path::new(r"c:\project\target\nested")
        ));
        assert!(!paths_overlap(
            Path::new(r"C:\project\target"),
            Path::new(r"C:\project\target-copy")
        ));
    }

    #[test]
    fn elevation_detection_uses_protected_roots_without_touching_the_filesystem() {
        let roots = vec![PathBuf::from(r"C:\Program Files")];
        assert!(path_requires_elevation_with_roots(
            Path::new(r"c:\program files\tool\cache"),
            &roots
        ));
        assert!(!path_requires_elevation_with_roots(
            Path::new(r"C:\Users\demo\project\target"),
            &roots
        ));
    }
}
