use super::model::{KnownSpaceItem, QuickScanResult, SafetyClass, ScanErrorSummary};
use super::walker::{measure_path, CancellationToken, ScanWalkError, WalkStats};
use crate::storage;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const TEMP_VISIBILITY_THRESHOLD: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupKind {
    Contents,
    WholeDirectory,
    None,
}

impl CleanupKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Contents => "contents",
            Self::WholeDirectory => "wholeDirectory",
            Self::None => "none",
        }
    }

    pub(crate) fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "contents" => Some(Self::Contents),
            "wholeDirectory" => Some(Self::WholeDirectory),
            "none" => Some(Self::None),
            _ => None,
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
    candidates.extend(developer_tool_history_candidates());
    candidates.extend(temp_directory_candidates());
    candidates
}

/// How many known folders are measured at once: the work is all disk, and one thread per
/// folder would be dozens of them.
const KNOWN_WALKERS: usize = 12;

pub fn scan_known_candidates<F>(
    token: &CancellationToken,
    mut progress: F,
) -> Result<QuickScanResult, ScanWalkError>
where
    F: FnMut(&WalkStats),
{
    let candidates = known_candidates();
    let done = std::sync::Mutex::new((WalkStats::default(), vec![None; candidates.len()]));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let finished = std::sync::atomic::AtomicUsize::new(0);
    let order = std::sync::atomic::Ordering::Relaxed;
    std::thread::scope(|scope| {
        for _ in 0..candidates.len().min(KNOWN_WALKERS) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, order);
                let Some(candidate) = candidates.get(index) else {
                    return;
                };
                if token.is_cancelled() {
                    finished.fetch_add(1, order);
                    continue;
                }
                let measured = measure_path(&candidate.path, token, |_| {});
                let mut guard = done.lock().unwrap_or_else(|e| e.into_inner());
                if let Ok(stats) = measured {
                    add_stats(&mut guard.0, &stats);
                    guard.1[index] = Some(stats.logical_bytes);
                }
                drop(guard);
                finished.fetch_add(1, order);
            });
        }
        // The caller's progress callback stays on the caller's thread: it reports what the
        // walkers have finished between them, which only ever grows.
        while finished.load(order) < candidates.len() {
            std::thread::sleep(std::time::Duration::from_millis(120));
            let stats = done.lock().map(|guard| guard.0.clone()).unwrap_or_default();
            progress(&stats);
        }
    });
    if token.is_cancelled() {
        return Err(ScanWalkError::Cancelled);
    }
    let (completed_stats, sizes) = done.into_inner().unwrap_or_else(|e| e.into_inner());
    progress(&completed_stats);
    let mut result = QuickScanResult::default();
    for (candidate, bytes) in candidates.into_iter().zip(sizes) {
        let Some(bytes) = bytes else { continue };
        if !legacy_candidate_is_visible(&candidate, bytes) {
            continue;
        }
        result.total_bytes = result.total_bytes.saturating_add(bytes);
        if candidate.safety == SafetyClass::Safe {
            result.safely_releasable_bytes = result.safely_releasable_bytes.saturating_add(bytes);
        }
        result.items.push(candidate.into_space_item(bytes));
    }
    // Biggest first: the list is read to decide what to clean.
    result
        .items
        .sort_by_key(|item| std::cmp::Reverse(item.bytes));

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
        storage::effective_path("go-module-cache"),
        environment_path("GOMODCACHE"),
        environment_path("GOPATH").map(|path| path.join("pkg").join("mod")),
        Some(home.join("go").join("pkg").join("mod")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let cargo_registry_cache = [
        storage::effective_path("cargo-home").map(|path| path.join("registry").join("cache")),
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
            candidates: vec![storage::effective_path("gradle-user-home")
                .unwrap_or_else(|| home.join(".gradle"))
                .join("caches")],
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
            candidates: vec![
                storage::effective_path("pnpm-store")
                    .unwrap_or_else(|| local.join("pnpm").join("store")),
                home.join(".pnpm-store"),
            ],
        },
        KnownRule {
            id: "npm",
            name_key: "spaceAnalysis.known.npm",
            ecosystem: "node",
            safety: SafetyClass::Safe,
            candidates: vec![
                storage::effective_path("npm-cache").unwrap_or_else(|| local.join("npm-cache")),
                home.join(".npm").join("_cacache"),
            ],
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
            candidates: vec![storage::effective_path("pip-cache")
                .unwrap_or_else(|| local.join("pip").join("Cache"))],
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
            candidates: vec![storage::effective_path("maven-local-repository")
                .unwrap_or_else(|| home.join(".m2").join("repository"))],
        },
        KnownRule {
            id: "yarn",
            name_key: "spaceAnalysis.known.yarn",
            ecosystem: "node",
            safety: SafetyClass::Safe,
            candidates: vec![
                local.join("Yarn").join("Cache"),
                home.join(".yarn").join("berry").join("cache"),
            ],
        },
        KnownRule {
            id: "bun",
            name_key: "spaceAnalysis.known.bun",
            ecosystem: "node",
            safety: SafetyClass::Safe,
            candidates: vec![home.join(".bun").join("install").join("cache")],
        },
        KnownRule {
            id: "deno",
            name_key: "spaceAnalysis.known.deno",
            ecosystem: "node",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("deno")],
        },
        KnownRule {
            id: "nuget",
            name_key: "spaceAnalysis.known.nuget",
            ecosystem: "dotnet",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![environment_path("NUGET_PACKAGES")
                .unwrap_or_else(|| home.join(".nuget").join("packages"))],
        },
        KnownRule {
            id: "uv",
            name_key: "spaceAnalysis.known.uv",
            ecosystem: "python",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("uv").join("cache")],
        },
        KnownRule {
            id: "poetry",
            name_key: "spaceAnalysis.known.poetry",
            ecosystem: "python",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("pypoetry").join("Cache")],
        },
        KnownRule {
            id: "conda",
            name_key: "spaceAnalysis.known.conda",
            ecosystem: "python",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![
                home.join(".conda").join("pkgs"),
                home.join("miniconda3").join("pkgs"),
                home.join("anaconda3").join("pkgs"),
            ],
        },
        KnownRule {
            id: "cargo-src",
            name_key: "spaceAnalysis.known.cargoSources",
            ecosystem: "rust",
            safety: SafetyClass::Safe,
            candidates: vec![environment_path("CARGO_HOME")
                .unwrap_or_else(|| home.join(".cargo"))
                .join("registry")
                .join("src")],
        },
        KnownRule {
            id: "sccache",
            name_key: "spaceAnalysis.known.sccache",
            ecosystem: "rust",
            safety: SafetyClass::Safe,
            candidates: vec![
                local.join("Mozilla").join("sccache"),
                home.join(".cache").join("sccache"),
            ],
        },
        KnownRule {
            id: "android-avd",
            name_key: "spaceAnalysis.known.androidAvd",
            ecosystem: "android",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![home.join(".android").join("avd")],
        },
        KnownRule {
            id: "crash-dumps",
            name_key: "spaceAnalysis.known.crashDumps",
            ecosystem: "windows",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("CrashDumps")],
        },
        KnownRule {
            id: "firefox-cache",
            name_key: "spaceAnalysis.known.firefoxCache",
            ecosystem: "browser",
            safety: SafetyClass::Safe,
            candidates: vec![local.join("Mozilla").join("Firefox").join("Profiles")],
        },
        KnownRule {
            id: "brave-cache",
            name_key: "spaceAnalysis.known.braveCache",
            ecosystem: "browser",
            safety: SafetyClass::Safe,
            candidates: vec![local
                .join("BraveSoftware")
                .join("Brave-Browser")
                .join("User Data")
                .join("Default")
                .join("Cache")],
        },
        // Local models are downloads, not caches: removing one means fetching gigabytes
        // again, so they are never in the safe set.
        KnownRule {
            id: "ollama-models",
            name_key: "spaceAnalysis.known.ollamaModels",
            ecosystem: "ai-model",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![home.join(".ollama").join("models")],
        },
        KnownRule {
            id: "lmstudio-models",
            name_key: "spaceAnalysis.known.lmStudioModels",
            ecosystem: "ai-model",
            safety: SafetyClass::NeedsConfirmation,
            candidates: vec![
                home.join(".lmstudio").join("models"),
                home.join(".cache").join("lm-studio"),
            ],
        },
        KnownRule {
            id: "chrome-cache",
            name_key: "spaceAnalysis.known.chromeCache",
            ecosystem: "browser",
            safety: SafetyClass::Safe,
            candidates: vec![local
                .join("Google")
                .join("Chrome")
                .join("User Data")
                .join("Default")
                .join("Cache")],
        },
        KnownRule {
            id: "edge-cache",
            name_key: "spaceAnalysis.known.edgeCache",
            ecosystem: "browser",
            safety: SafetyClass::Safe,
            candidates: vec![local
                .join("Microsoft")
                .join("Edge")
                .join("User Data")
                .join("Default")
                .join("Cache")],
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

fn developer_tool_history_candidates() -> Vec<KnownCandidate> {
    const ANDROID_STUDIO_PRODUCTS: &[&str] = &["AndroidStudio"];
    let roots = [
        (
            "JetBrainsLocal",
            local_app_data().join("JetBrains"),
            None,
            "jetbrains",
        ),
        (
            "JetBrainsRoaming",
            roaming_app_data().join("JetBrains"),
            None,
            "jetbrains",
        ),
        (
            "AndroidStudioLocal",
            local_app_data().join("Google"),
            Some(ANDROID_STUDIO_PRODUCTS),
            "android-studio",
        ),
        (
            "AndroidStudioRoaming",
            roaming_app_data().join("Google"),
            Some(ANDROID_STUDIO_PRODUCTS),
            "android-studio",
        ),
    ];
    roots
        .into_iter()
        .flat_map(|(scope, root, allowed_products, ecosystem)| {
            history_candidates_in_root(scope, &root, allowed_products, ecosystem)
        })
        .collect()
}

fn history_candidates_in_root(
    scope: &str,
    root: &Path,
    allowed_products: Option<&[&str]>,
    ecosystem: &str,
) -> Vec<KnownCandidate> {
    if !is_plain_directory(root) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
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
        if allowed_products.is_some_and(|allowed| {
            !allowed
                .iter()
                .any(|candidate| product.eq_ignore_ascii_case(candidate))
        }) {
            continue;
        }
        groups
            .entry(product)
            .or_default()
            .push((path, version_key(&version), name));
    }

    let mut candidates = Vec::new();
    for (_, mut versions) in groups {
        if versions.len() <= 1 {
            continue;
        }
        versions.sort_by(|left, right| left.1.cmp(&right.1).then(left.2.cmp(&right.2)));
        versions.pop();
        candidates.extend(versions.into_iter().map(|(path, _, name)| KnownCandidate {
            id: format!("developer-tool-history:{scope}:{name}"),
            name_key: "spaceAnalysis.known.jetbrainsHistory".into(),
            path,
            ecosystem: Some(ecosystem.into()),
            safety: SafetyClass::NeedsConfirmation,
            cleanup_kind: CleanupKind::WholeDirectory,
        }));
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

fn legacy_candidate_is_visible(candidate: &KnownCandidate, bytes: u64) -> bool {
    bytes > 0
        && (!matches!(candidate.id.as_str(), "user-temp" | "windows-temp")
            || bytes > TEMP_VISIBILITY_THRESHOLD)
}

fn add_stats(total: &mut WalkStats, stats: &WalkStats) {
    total.files = total.files.saturating_add(stats.files);
    total.directories = total.directories.saturating_add(stats.directories);
    total.logical_bytes = total.logical_bytes.saturating_add(stats.logical_bytes);
    total.allocated_bytes = total.allocated_bytes.saturating_add(stats.allocated_bytes);
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

    #[test]
    fn android_studio_filter_does_not_match_other_google_apps() {
        let allowed = ["AndroidStudio"];
        let matches = |name: &str| {
            split_versioned_dir(name).is_some_and(|(product, _)| {
                allowed
                    .iter()
                    .any(|candidate| product.eq_ignore_ascii_case(candidate))
            })
        };

        assert!(matches("AndroidStudio2025.3.4"));
        assert!(!matches("Chrome2025.3.4"));
    }

    #[test]
    fn android_studio_history_keeps_only_the_latest_directory() {
        let root = std::env::temp_dir().join(format!(
            "stacker-android-studio-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be valid")
                .as_nanos()
        ));
        for name in [
            "AndroidStudio2025.3.2",
            "AndroidStudio2025.3.4",
            "AndroidStudio2026.1.1",
            "Chrome2025.3.4",
        ] {
            fs::create_dir_all(root.join(name)).expect("test directory should be created");
        }

        let candidates = history_candidates_in_root(
            "AndroidStudioLocal",
            &root,
            Some(&["AndroidStudio"]),
            "android-studio",
        );
        let mut names = candidates
            .iter()
            .filter_map(|candidate| candidate.path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();

        assert_eq!(
            names,
            vec!["AndroidStudio2025.3.2", "AndroidStudio2025.3.4"]
        );
        assert!(candidates
            .iter()
            .all(|candidate| candidate.cleanup_kind == CleanupKind::WholeDirectory));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn legacy_visibility_excludes_small_temp_directories() {
        let candidate = |id: &str| KnownCandidate {
            id: id.into(),
            name_key: "test.known".into(),
            path: PathBuf::from(r"C:\test-only"),
            ecosystem: Some("test".into()),
            safety: SafetyClass::NeedsConfirmation,
            cleanup_kind: CleanupKind::Contents,
        };

        for id in ["user-temp", "windows-temp"] {
            let temp = candidate(id);
            assert!(!legacy_candidate_is_visible(
                &temp,
                TEMP_VISIBILITY_THRESHOLD
            ));
            assert!(legacy_candidate_is_visible(
                &temp,
                TEMP_VISIBILITY_THRESHOLD + 1
            ));
        }

        let cache = candidate("playwright");
        assert!(!legacy_candidate_is_visible(&cache, 0));
        assert!(legacy_candidate_is_visible(&cache, 1));
    }

    #[test]
    fn completed_candidates_accumulate_allocated_bytes() {
        let mut completed = WalkStats::default();
        add_stats(
            &mut completed,
            &WalkStats {
                logical_bytes: 100,
                allocated_bytes: 64,
                ..WalkStats::default()
            },
        );
        add_stats(
            &mut completed,
            &WalkStats {
                logical_bytes: 200,
                allocated_bytes: 128,
                ..WalkStats::default()
            },
        );

        assert_eq!(completed.logical_bytes, 300);
        assert_eq!(completed.allocated_bytes, 192);
    }
}
