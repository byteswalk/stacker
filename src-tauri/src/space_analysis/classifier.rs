use super::known::{CleanupKind, SafetyClass};
use super::model::{DirectoryNode, ProjectKind, ProjectRoot};
use super::walker::IndexedScanResult;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactRule {
    pub(crate) project_kind: ProjectKind,
    pub(crate) relative_path: &'static str,
    pub(crate) cleanup_kind: CleanupKind,
    pub(crate) impact_key: &'static str,
    pub(crate) safety: SafetyClass,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Classification {
    pub(crate) safety: SafetyClass,
    pub(crate) project_id: Option<String>,
    pub(crate) cleanup_kind: CleanupKind,
    pub(crate) impact_key: Option<&'static str>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DetectedProjects {
    roots: Vec<ProjectRoot>,
    nearest_project_by_node: HashMap<String, String>,
}

impl DetectedProjects {
    pub(crate) fn roots(&self) -> &[ProjectRoot] {
        &self.roots
    }

    pub(crate) fn nearest_project_id(&self, node_id: &str) -> Option<&str> {
        self.nearest_project_by_node
            .get(node_id)
            .map(String::as_str)
    }
}

pub(crate) fn detect_projects(index: &IndexedScanResult) -> DetectedProjects {
    let mut entries = index.directory_entries().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        path_depth(&left.node.path)
            .cmp(&path_depth(&right.node.path))
            .then_with(|| left.node.path.cmp(&right.node.path))
            .then_with(|| left.node.node_id.cmp(&right.node.node_id))
    });

    let mut detected = DetectedProjects::default();
    for entry in entries {
        let own_project = detect_project_kind(entry.direct_file_names).map(|kind| {
            let project_id = format!("project-{}", entry.node.node_id);
            detected.roots.push(ProjectRoot {
                project_id: project_id.clone(),
                node_id: entry.node.node_id.clone(),
                path: entry.node.path.clone(),
                kind,
            });
            project_id
        });

        let nearest = own_project.or_else(|| {
            entry
                .node
                .parent_id
                .as_ref()
                .and_then(|parent_id| detected.nearest_project_by_node.get(parent_id))
                .cloned()
        });
        if let Some(project_id) = nearest {
            detected
                .nearest_project_by_node
                .insert(entry.node.node_id.clone(), project_id);
        }
    }

    detected
}

pub(crate) fn classify_node(node: &DirectoryNode, projects: &DetectedProjects) -> Classification {
    Classification {
        safety: SafetyClass::ViewOnly,
        project_id: projects
            .nearest_project_id(&node.node_id)
            .map(str::to_string),
        cleanup_kind: CleanupKind::None,
        impact_key: None,
    }
}

fn detect_project_kind(file_names: &HashSet<String>) -> Option<ProjectKind> {
    if file_names.contains("package.json") {
        Some(ProjectKind::Node)
    } else if file_names.contains("cargo.toml") {
        Some(ProjectKind::Rust)
    } else if file_names.contains("pom.xml") {
        Some(ProjectKind::Maven)
    } else if file_names.iter().any(|name| {
        matches!(
            name.as_str(),
            "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts"
        )
    }) {
        Some(ProjectKind::Gradle)
    } else if file_names.contains("go.mod") {
        Some(ProjectKind::Go)
    } else if file_names
        .iter()
        .any(|name| name.ends_with(".sln") || name.ends_with(".csproj"))
    {
        Some(ProjectKind::DotNet)
    } else {
        None
    }
}

pub(crate) fn is_project_marker_file_name(file_name: &str) -> bool {
    let file_name = file_name.to_lowercase();
    matches!(
        file_name.as_str(),
        "package.json"
            | "cargo.toml"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "go.mod"
    ) || file_name.ends_with(".sln")
        || file_name.ends_with(".csproj")
}

fn path_depth(path: &str) -> usize {
    Path::new(path).components().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::walker::{build_indexed_result, CancellationToken};
    use std::fs;
    use std::path::PathBuf;

    fn build_fixture_index(root: &Path) -> IndexedScanResult {
        build_indexed_result(&[root.to_path_buf()], &CancellationToken::default(), |_| {}).unwrap()
    }

    fn create_project(root: &Path, name: &str, marker: &str) -> PathBuf {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(marker), b"marker").unwrap();
        directory
    }

    #[test]
    fn detects_project_markers() {
        let temp = tempfile::tempdir().unwrap();
        let expected = [
            ("node", "package.json", ProjectKind::Node),
            ("rust", "Cargo.toml", ProjectKind::Rust),
            ("maven", "pom.xml", ProjectKind::Maven),
            ("gradle", "settings.gradle.kts", ProjectKind::Gradle),
            ("go", "go.mod", ProjectKind::Go),
            ("dotnet-sln", "Sample.sln", ProjectKind::DotNet),
            ("dotnet-project", "Sample.csproj", ProjectKind::DotNet),
        ];
        for (name, marker, _) in &expected {
            create_project(temp.path(), name, marker);
        }
        fs::create_dir_all(temp.path().join("plain")).unwrap();

        let index = build_fixture_index(temp.path());
        let projects = detect_projects(&index);
        assert_eq!(projects.roots().len(), expected.len());
        for (name, _, kind) in expected {
            let path = temp.path().join(name);
            let project = projects
                .roots()
                .iter()
                .find(|project| Path::new(&project.path) == path)
                .unwrap();
            assert_eq!(project.kind, kind);
            assert_eq!(project.project_id, format!("project-{}", project.node_id));
        }
        assert!(projects
            .roots()
            .iter()
            .all(|project| !project.path.ends_with("plain")));
    }

    #[test]
    fn nested_projects_override_the_parent_project() {
        let temp = tempfile::tempdir().unwrap();
        let parent = create_project(temp.path(), "workspace", "package.json");
        let child = create_project(&parent, "native", "Cargo.toml");
        let nested_directory = child.join("src");
        fs::create_dir_all(&nested_directory).unwrap();

        let index = build_fixture_index(&parent);
        let projects = detect_projects(&index);
        let child_project = projects
            .roots()
            .iter()
            .find(|project| Path::new(&project.path) == child)
            .unwrap();
        let nested_node = index
            .directory_entries()
            .find(|entry| Path::new(&entry.node.path) == nested_directory)
            .unwrap()
            .node;

        assert_eq!(child_project.kind, ProjectKind::Rust);
        assert_eq!(
            projects.nearest_project_id(&nested_node.node_id),
            Some(child_project.project_id.as_str())
        );
    }

    #[test]
    fn project_detection_stops_at_the_selected_boundary() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("package.json"), b"marker").unwrap();
        let selected = temp.path().join("selected");
        fs::create_dir_all(selected.join("src")).unwrap();

        let index = build_fixture_index(&selected);
        let projects = detect_projects(&index);

        assert!(projects.roots().is_empty());
        assert!(index
            .directory_entries()
            .all(|entry| projects.nearest_project_id(&entry.node.node_id).is_none()));
    }

    #[test]
    fn unmatched_nodes_remain_view_only() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("plain")).unwrap();
        let index = build_fixture_index(temp.path());
        let projects = detect_projects(&index);
        let node = index.directory_entries().next().unwrap().node;

        let classification = classify_node(node, &projects);

        assert_eq!(classification.safety, SafetyClass::ViewOnly);
        assert_eq!(classification.cleanup_kind, CleanupKind::None);
    }

    #[test]
    fn marker_filter_is_case_insensitive_and_rejects_unrelated_files() {
        assert!(is_project_marker_file_name("Cargo.toml"));
        assert!(is_project_marker_file_name("Sample.CSPROJ"));
        assert!(!is_project_marker_file_name("README.md"));
        assert!(!is_project_marker_file_name("package-lock.json"));
    }
}
