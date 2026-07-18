use super::model::{KnownSpaceItem, QuickScanResult, ScanErrorSummary};
use super::walker::{measure_path, CancellationToken, ScanWalkError, WalkStats};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const TEMP_VISIBILITY_THRESHOLD: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafetyClass {
    Safe,
    Rebuildable,
    NeedsConfirmation,
    ViewOnly,
}

impl SafetyClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Rebuildable => "rebuildable",
            Self::NeedsConfirmation => "needsConfirmation",
            Self::ViewOnly => "viewOnly",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupKind {
    Contents,
    WholeDirectory,
    None,
}

impl CleanupKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Contents => "contents",
            Self::WholeDirectory => "wholeDirectory",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownCandidate {
    pub id: String,
    pub name_key: String,
    pub path: PathBuf,
    pub ecosystem: Option<String>,
    pub safety: SafetyClass,
    pub cleanup_kind: CleanupKind,
}

struct KnownRule {
    id: &'static str,
    name_key: &'static str,
    ecosystem: &'static str,
    safety: SafetyClass,
    candidates: Vec<PathBuf>,
}

pub fn known_candidates() -> Vec<KnownCandidate> {
    let mut candidates = known_cache_rules()
        .into_iter()
        .filter_map(|rule| {
            let path = first_plain_directory(&rule.candidates)?;
            Some(KnownCandidate {
                id: rule.id.into(),
                name_key: rule.name_key.into(),
                path,
                ecosystem: Some(rule.ecosystem.into()),
                safety: rule.safety,
                cleanup_kind: CleanupKind::Contents,
            })
        })
        .collect::<Vec<_>>();
    candidates.extend(jetbrains_history_candidates());
    candidates.extend(temp_directory_candidates());
    candidates
}

pub fn scan_known_candidates<F>(
    token: &CancellationToken,
    mut progress: F,
) -> Result<QuickScanResult, ScanWalkError>
where
    F: FnMut(&WalkStats),
{
    let mut result = QuickScanResult::default();
    let mut completed_stats = WalkStats::default();

    for candidate in known_candidates() {
        if token.is_cancelled() {
            return Err(ScanWalkError::Cancelled);
        }

        let stats = measure_path(&candidate.path, token, |current| {
            let aggregate = combined_stats(&completed_stats, current);
            progress(&aggregate);
        })?;
        add_stats(&mut completed_stats, &stats);
        progress(&completed_stats);

        if stats.logical_bytes == 0 || is_small_compatibility_temp(&candidate, &stats) {
            continue;
        }

        let bytes = stats.logical_bytes;
        result.total_bytes = result.total_bytes.saturating_add(bytes);
        if candidate.safety == SafetyClass::Safe {
            result.safely_releasable_bytes = result.safely_releasable_bytes.saturating_add(bytes);
        }
        result.items.push(candidate.into_space_item(bytes));
    }

    result.completed = true;
    result.errors = completed_stats.errors;
    Ok(result)
}

impl KnownCandidate {
    fn into_space_item(self, bytes: u64) -> KnownSpaceItem {
        KnownSpaceItem {
            id: self.id,
            name_key: self.name_key,
            path: self.path.to_string_lossy().into_owned(),
            bytes,
            safety: self.safety.as_str().into(),
            cleanup_kind: self.cleanup_kind.as_str().into(),
            ecosystem: self.ecosystem,
        }
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn local_app_data() -> PathBuf {
    dirs::data_local_dir().unwrap_or_default()
}

fn roaming_app_data() -> PathBuf {
    dirs::data_dir().unwrap_or_default()
}

fn environment_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn known_cache_rules() -> Vec<KnownRule> {
    let home = home();
    let local = local_app_data();
    let go_module_cache = [
        environment_path("GOMODCACHE"),
        environment_path("GOPATH").map(|path| path.join("pkg").join("mod")),
        Some(home.join("go").join("pkg").join("mod")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let cargo_registry_cache = [
        environment_path("CARGO_HOME").map(|path| path.join("registry").join("cache")),
        Some(home.join(".cargo").join("registry").join("cache")),
    ]
    .into_iter()
    .flatten()
    .collect();

    vec![
        KnownRule {
            id: "gradle",
            name_key: "spaceAnalysis.known.gradle",
            ecosystem: "gradle",
            safety: SafetyClass::Safe,
            candidates: vec![home.join(".gradle").join("caches")],
        },
        KnownRule {
            id: "gomod",
            name_key: "spaceAnalysis.known.goModules",
            ecosystem: "go",
            safety: SafetyClass::Safe,
            candidates: go_module_cache,
        },
        KnownRule {
            id: "pnpm",
            name_key: "spaceAnalysis.known.pnpm",
            ecosystem: "node",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("pnpm").join("store"), home.join(".pnpm-store")],
        },
        KnownRule {
            id: "npm",
            name_key: "spaceAnalysis.known.npm",
            ecosystem: "node",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("npm-cache"), home.join(".npm").join("_cacache")],
        },
        KnownRule {
            id: "cargo",
            name_key: "spaceAnalysis.known.cargoRegistry",
            ecosystem: "rust",
            safety: SafetyClass::Safe,
            candidates: cargo_registry_cache,
        },
        KnownRule {
            id: "pip",
            name_key: "spaceAnalysis.known.pip",
            ecosystem: "python",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("pip").join("Cache")],
        },
        KnownRule {
            id: "electron",
            name_key: "spaceAnalysis.known.electron",
            ecosystem: "electron",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("electron").join("Cache")],
        },
        KnownRule {
            id: "playwright",
            name_key: "spaceAnalysis.known.playwright",
            ecosystem: "playwright",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![local.join("ms-playwright")],
        },
        KnownRule {
            id: "hf",
            name_key: "spaceAnalysis.known.huggingFace",
            ecosystem: "huggingface",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![home.join(".cache").join("huggingface").join("hub")],
        },
        KnownRule {
            id: "m2repo",
            name_key: "spaceAnalysis.known.mavenRepository",
            ecosystem: "maven",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![home.join(".m2").join("repository")],
        },
    ]
}

fn first_plain_directory(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|path| is_plain_directory(path))
        .cloned()
}

fn is_plain_directory(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata.is_dir() && !is_link_or_reparse_point(&metadata)
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn version_key(version: &str) -> Vec<u32> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn split_versioned_dir(name: &str) -> Option<(String, String)> {
    let index = name.find(|character: char| character.is_ascii_digit())?;
    if index == 0 || index >= name.len() {
        return None;
    }
    let product = name[..index].trim_matches(['-', '_', '.', ' ']).to_string();
    let version = name[index..].trim().to_string();
    if product.is_empty() || version_key(&version).is_empty() {
        return None;
    }
    Some((product, version))
}

type VersionedDirectory = (PathBuf, Vec<u32>, String);

fn jetbrains_history_candidates() -> Vec<KnownCandidate> {
    let roots = [
        ("Local", local_app_data().join("JetBrains")),
        ("Roaming", roaming_app_data().join("JetBrains")),
    ];
    let mut candidates = Vec::new();

    for (scope, root) in roots {
        if !is_plain_directory(&root) {
            continue;
        }
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        let mut groups: HashMap<String, Vec<VersionedDirectory>> = HashMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_plain_directory(&path) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some((product, version)) = split_versioned_dir(&name) else {
                continue;
            };
            groups
                .entry(product)
                .or_default()
                .push((path, version_key(&version), name));
        }

        for (_, mut versions) in groups {
            if versions.len() <= 1 {
                continue;
            }
            versions.sort_by(|left, right| left.1.cmp(&right.1).then(left.2.cmp(&right.2)));
            versions.pop();
            candidates.extend(versions.into_iter().map(|(path, _, name)| KnownCandidate {
                id: format!("jetbrains-history:{scope}:{name}"),
                name_key: "spaceAnalysis.known.jetbrainsHistory".into(),
                path,
                ecosystem: Some("jetbrains".into()),
                safety: SafetyClass::NeedsConfirmation,
                cleanup_kind: CleanupKind::WholeDirectory,
            }));
        }
    }

    candidates
}

fn temp_directory_candidates() -> Vec<KnownCandidate> {
    let mut paths = Vec::new();
    if let Some(path) = environment_path("TEMP").or_else(|| environment_path("TMP")) {
        paths.push(path);
    }
    paths.push(local_app_data().join("Temp"));
    if let Some(system_root) = environment_path("SystemRoot").or_else(|| environment_path("WINDIR"))
    {
        paths.push(system_root.join("Temp"));
    }

    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| {
            seen.insert(path.to_string_lossy().to_ascii_lowercase()) && is_plain_directory(path)
        })
        .map(|path| {
            let is_system = is_windows_temp(&path);
            KnownCandidate {
                id: if is_system {
                    "windows-temp"
                } else {
                    "user-temp"
                }
                .into(),
                name_key: if is_system {
                    "spaceAnalysis.known.windowsTemp"
                } else {
                    "spaceAnalysis.known.userTemp"
                }
                .into(),
                path,
                ecosystem: Some("windows".into()),
                safety: SafetyClass::NeedsConfirmation,
                cleanup_kind: CleanupKind::Contents,
            }
        })
        .collect()
}

fn is_windows_temp(path: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains("\\windows\\temp")
}

fn is_small_compatibility_temp(candidate: &KnownCandidate, stats: &WalkStats) -> bool {
    matches!(candidate.id.as_str(), "user-temp" | "windows-temp")
        && stats.logical_bytes <= TEMP_VISIBILITY_THRESHOLD
}

fn combined_stats(completed: &WalkStats, current: &WalkStats) -> WalkStats {
    let mut combined = completed.clone();
    add_stats(&mut combined, current);
    combined
}

fn add_stats(total: &mut WalkStats, stats: &WalkStats) {
    total.files = total.files.saturating_add(stats.files);
    total.directories = total.directories.saturating_add(stats.directories);
    total.logical_bytes = total.logical_bytes.saturating_add(stats.logical_bytes);
    total.skipped = total.skipped.saturating_add(stats.skipped);
    add_errors(&mut total.errors, &stats.errors);
}

fn add_errors(total: &mut ScanErrorSummary, errors: &ScanErrorSummary) {
    total.access_denied = total.access_denied.saturating_add(errors.access_denied);
    total.vanished = total.vanished.saturating_add(errors.vanished);
    total.invalid_target = total.invalid_target.saturating_add(errors.invalid_target);
    total.other = total.other.saturating_add(errors.other);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_jetbrains_product_and_version() {
        assert_eq!(
            split_versioned_dir("IntelliJIdea2026.1"),
            Some(("IntelliJIdea".into(), "2026.1".into()))
        );
        assert_eq!(
            split_versioned_dir("AndroidStudio2025.3.4"),
            Some(("AndroidStudio".into(), "2025.3.4".into()))
        );
    }

    #[test]
    fn ignores_directories_without_a_version() {
        assert_eq!(split_versioned_dir("SharedIndex"), None);
        assert_eq!(split_versioned_dir("2026.1"), None);
    }

    #[test]
    fn quick_rules_never_mark_cautious_items_safe() {
        for candidate in known_candidates() {
            if matches!(candidate.id.as_str(), "playwright" | "hf" | "m2repo") {
                assert_eq!(candidate.safety, SafetyClass::NeedsConfirmation);
            }
        }
    }

    #[test]
    fn jetbrains_history_keeps_the_highest_version_per_product() {
        assert!(version_key("2026.1") > version_key("2025.3.4"));
    }
}
