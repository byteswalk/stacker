use super::known::CleanupKind;
use super::model::{DirectoryNode, ProjectKind, ProjectRoot, SafetyClass};
use super::walker::IndexedScanResult;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactRule {
    pub(crate) project_kind: ProjectKind,
    pub(crate) relative_path: &'static str,
    pub(crate) cleanup_kind: CleanupKind,
    pub(crate) impact_key: &'static str,
    pub(crate) safety: SafetyClass,
    pub(crate) required_root_evidence: &'static [&'static str],
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

    pub(crate) fn project(&self, project_id: &str) -> Option<&ProjectRoot> {
        self.roots
            .iter()
            .find(|project| project.project_id == project_id)
    }
}

const ARTIFACT_RULES: &[ArtifactRule] = &[
    ArtifactRule {
        project_kind: ProjectKind::Node,
        relative_path: "node_modules",
        cleanup_kind: CleanupKind::WholeDirectory,
        impact_key: "spaceAnalysis.impact.nodeDependencies",
        safety: SafetyClass::Rebuildable,
        required_root_evidence: &[],
    },
    ArtifactRule {
        project_kind: ProjectKind::Rust,
        relative_path: "target",
        cleanup_kind: CleanupKind::WholeDirectory,
        impact_key: "spaceAnalysis.impact.rustBuildOutput",
        safety: SafetyClass::Rebuildable,
        required_root_evidence: &[],
    },
    ArtifactRule {
        project_kind: ProjectKind::Maven,
        relative_path: "target",
        cleanup_kind: CleanupKind::WholeDirectory,
        impact_key: "spaceAnalysis.impact.mavenBuildOutput",
        safety: SafetyClass::Rebuildable,
        required_root_evidence: &[],
    },
    ArtifactRule {
        project_kind: ProjectKind::Gradle,
        relative_path: ".gradle",
        cleanup_kind: CleanupKind::WholeDirectory,
        impact_key: "spaceAnalysis.impact.gradleProjectCache",
        safety: SafetyClass::Rebuildable,
        required_root_evidence: &[],
    },
    ArtifactRule {
        project_kind: ProjectKind::Gradle,
        relative_path: "build",
        cleanup_kind: CleanupKind::WholeDirectory,
        impact_key: "spaceAnalysis.impact.gradleBuildOutput",
        safety: SafetyClass::Rebuildable,
        required_root_evidence: &[],
    },
    ArtifactRule {
        project_kind: ProjectKind::Go,
        relative_path: "dist",
        cleanup_kind: CleanupKind::WholeDirectory,
        impact_key: "spaceAnalysis.impact.goReleaseOutput",
        safety: SafetyClass::Rebuildable,
        required_root_evidence: &[".goreleaser.yml", ".goreleaser.yaml"],
    },
];

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
        let own_project = (!is_tool_or_dependency_managed_project_root(Path::new(&entry.node.path)))
            .then(|| detect_project_kind(entry.direct_file_names))
            .flatten()
            .map(|kind| {
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

pub(crate) fn classify_node(
    node: &DirectoryNode,
    projects: &DetectedProjects,
    root_evidence: &HashMap<String, HashSet<String>>,
) -> Classification {
    let project_id = projects
        .nearest_project_id(&node.node_id)
        .map(str::to_string);
    let Some(project) = project_id
        .as_deref()
        .and_then(|project_id| projects.project(project_id))
    else {
        return Classification::view_only(None);
    };
    let Ok(relative_path) = Path::new(&node.path).strip_prefix(&project.path) else {
        return Classification::view_only(project_id);
    };
    let relative_path = normalized_relative_path(relative_path);
    let evidence = root_evidence.get(&project.node_id);

    if let Some(rule) = ARTIFACT_RULES.iter().find(|rule| {
        rule.project_kind == project.kind
            && rule.relative_path == relative_path
            && (rule.required_root_evidence.is_empty()
                || rule
                    .required_root_evidence
                    .iter()
                    .any(|required| evidence.is_some_and(|files| files.contains(*required))))
    }) {
        return Classification {
            safety: rule.safety,
            project_id,
            cleanup_kind: rule.cleanup_kind,
            impact_key: Some(rule.impact_key),
        };
    }

    Classification::view_only(project_id)
}

pub(crate) fn matches_artifact_rule(
    project_kind: ProjectKind,
    project_root: &Path,
    artifact_path: &Path,
    root_evidence: &HashSet<String>,
    cleanup_kind: CleanupKind,
    impact_key: &str,
    safety: SafetyClass,
) -> bool {
    if is_tool_or_dependency_managed_project_root(project_root) {
        return false;
    }
    let Ok(relative_path) = artifact_path.strip_prefix(project_root) else {
        return false;
    };
    let relative_path = normalized_relative_path(relative_path);
    ARTIFACT_RULES.iter().any(|rule| {
        rule.project_kind == project_kind
            && rule.relative_path == relative_path
            && rule.cleanup_kind == cleanup_kind
            && rule.impact_key == impact_key
            && rule.safety == safety
            && (rule.required_root_evidence.is_empty()
                || rule
                    .required_root_evidence
                    .iter()
                    .any(|required| root_evidence.contains(*required)))
    })
}

impl Classification {
    pub(crate) fn view_only(project_id: Option<String>) -> Self {
        Self {
            safety: SafetyClass::ViewOnly,
            project_id,
            cleanup_kind: CleanupKind::None,
            impact_key: None,
        }
    }
}

fn normalized_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
        .to_lowercase()
}

fn path_component_words(path: &Path) -> impl Iterator<Item = String> + '_ {
    path.components().filter_map(|component| match component {
        Component::Normal(value) => Some(value.to_string_lossy().to_ascii_lowercase()),
        _ => None,
    })
}

fn is_tool_or_dependency_managed_project_root(path: &Path) -> bool {
    let components = path_component_words(path).collect::<Vec<_>>();
    components.iter().any(|component| component == "node_modules")
        || components
            .windows(2)
            .any(|pair| pair == ["fnm", "node-versions"] || pair == ["node-versions", "installation"])
        || components.iter().any(|component| {
            matches!(
                component.as_str(),
                "hbuilderx"
                    | "androidstudio"
                    | "jetbrains"
                    | "plugins"
                    | "extensions"
                    | "app.asar.unpacked"
            )
        })
}

pub(crate) fn detect_project_kind(file_names: &HashSet<String>) -> Option<ProjectKind> {
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
            | ".goreleaser.yml"
            | ".goreleaser.yaml"
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

        let classification = classify_node(node, &projects, &HashMap::new());

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

    fn classify_fixture(root: &Path, target: &Path) -> Classification {
        let index = build_fixture_index(root);
        let projects = detect_projects(&index);
        let evidence = index.project_root_evidence(&projects);
        let node = index
            .directory_entries()
            .find(|entry| Path::new(&entry.node.path) == target)
            .unwrap()
            .node;
        classify_node(node, &projects, &evidence)
    }

    #[test]
    fn verified_project_artifacts_are_rebuildable() {
        let fixtures = [
            ("node", "package.json", "node_modules"),
            ("rust", "Cargo.toml", "target"),
            ("maven", "pom.xml", "target"),
            ("gradle-cache", "settings.gradle", ".gradle"),
            ("gradle-build", "build.gradle.kts", "build"),
        ];

        for (name, marker, artifact) in fixtures {
            let temp = tempfile::tempdir().unwrap();
            let project = create_project(temp.path(), name, marker);
            let artifact_path = project.join(artifact);
            fs::create_dir_all(&artifact_path).unwrap();
            fs::write(artifact_path.join("generated.bin"), b"generated").unwrap();

            let classification = classify_fixture(&project, &artifact_path);
            assert_eq!(classification.safety, SafetyClass::Rebuildable, "{name}");
            assert_eq!(classification.cleanup_kind, CleanupKind::WholeDirectory);
            assert!(classification.project_id.is_some());
            assert!(classification.impact_key.is_some());
        }
    }

    #[test]
    fn same_name_directories_without_matching_project_markers_are_view_only() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["node_modules", "target", "build", ".gradle"] {
            fs::create_dir_all(temp.path().join(name)).unwrap();
            assert_eq!(
                classify_fixture(temp.path(), &temp.path().join(name)).safety,
                SafetyClass::ViewOnly
            );
        }
    }

    #[test]
    fn node_modules_inside_runtime_installations_are_view_only() {
        let temp = tempfile::tempdir().unwrap();
        let npm_root = temp
            .path()
            .join("fnm")
            .join("node-versions")
            .join("v24.18.0")
            .join("installation")
            .join("node_modules")
            .join("npm");
        let nested_modules = npm_root.join("node_modules");
        fs::create_dir_all(&nested_modules).unwrap();
        fs::write(npm_root.join("package.json"), b"marker").unwrap();

        assert_eq!(
            classify_fixture(temp.path(), &nested_modules).safety,
            SafetyClass::ViewOnly
        );
    }

    #[test]
    fn node_modules_inside_tool_plugin_directories_are_view_only() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_root = temp.path().join("HBuilderX").join("plugins").join("yshint");
        let node_modules = plugin_root.join("node_modules");
        fs::create_dir_all(&node_modules).unwrap();
        fs::write(plugin_root.join("package.json"), b"marker").unwrap();

        assert_eq!(
            classify_fixture(temp.path(), &node_modules).safety,
            SafetyClass::ViewOnly
        );
    }

    #[test]
    fn source_metadata_and_user_directories_remain_view_only() {
        let temp = tempfile::tempdir().unwrap();
        let project = create_project(temp.path(), "project", "package.json");
        for name in [".git", "src", "uploads", ".idea"] {
            fs::create_dir_all(project.join(name)).unwrap();
            assert_eq!(
                classify_fixture(&project, &project.join(name)).safety,
                SafetyClass::ViewOnly
            );
        }
    }

    #[test]
    fn go_release_output_requires_goreleaser_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let project = create_project(temp.path(), "go-project", "go.mod");
        let dist = project.join("dist");
        fs::create_dir_all(&dist).unwrap();
        assert_eq!(
            classify_fixture(&project, &dist).safety,
            SafetyClass::ViewOnly
        );

        fs::write(project.join(".goreleaser.yaml"), b"version: 2").unwrap();
        assert_eq!(
            classify_fixture(&project, &dist).safety,
            SafetyClass::Rebuildable
        );
    }
}
