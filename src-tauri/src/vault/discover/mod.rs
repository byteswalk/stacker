//! 发现散落密钥：只读扫描本机常见位置，结果只留在内存，保管库锁定即清空。

pub(crate) mod rules;
pub(crate) mod sources;

use super::crypto;
use super::errors::{BUSY, LOCKED};
use super::model::{Digests, EntryInput, FieldInput, Kind, MAX_FIELD_BYTES};
use super::session::Vault;
use serde::{Deserialize, Serialize};
use sources::{Raw, Source, Walk};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Scope {
    pub ssh: bool,
    pub configs: bool,
    pub env: bool,
    pub project_dirs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindingView {
    pub id: usize,
    pub source: Source,
    pub location: String,
    pub name: String,
    pub preview: String,
    pub platform: String,
    pub kind: Kind,
    pub risks: Vec<&'static str>,
    pub status: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JobStatus {
    pub running: bool,
    pub cancelled: bool,
    pub truncated: bool,
    pub files: usize,
    pub findings: Vec<FindingView>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportItem {
    pub id: usize,
    pub platform: String,
    pub kind: Kind,
    /// Where the finding came from, in the words the list showed ("用户环境变量" for an
    /// environment variable, whose own location is only the scope's id).
    #[serde(default)]
    pub origin: Option<String>,
}

pub(crate) struct Outcome {
    pub findings: Vec<Raw>,
    pub truncated: bool,
}

pub(crate) fn scan(
    scope: &Scope,
    home: &Path,
    cancel: &AtomicBool,
    files: &AtomicUsize,
) -> Outcome {
    let truncated = AtomicBool::new(false);
    let walk = Walk {
        cancel,
        files,
        truncated: &truncated,
    };
    let live = || !cancel.load(Ordering::Relaxed);
    let mut found = Vec::new();
    if scope.ssh {
        found.extend(sources::ssh_keys(home, &walk));
    }
    if scope.configs && live() {
        found.extend(sources::config_files(home));
    }
    if scope.env && live() {
        found.extend(sources::env_vars());
    }
    let dirs: Vec<PathBuf> = scope.project_dirs.iter().map(PathBuf::from).collect();
    if !dirs.is_empty() && live() {
        found.extend(sources::dotenv_files(&dirs, &walk));
    }
    let mut seen = HashSet::new();
    found.retain(|raw| {
        seen.insert((
            raw.location.clone(),
            raw.name.clone(),
            crypto::secret_digest(raw.primary()),
        ))
    });
    Outcome {
        findings: found,
        truncated: truncated.load(Ordering::Relaxed),
    }
}

fn status_of(raw: &Raw, digests: &Digests) -> &'static str {
    let digest = crypto::secret_digest(raw.primary());
    if digests.current.contains(&digest) {
        "in_vault"
    } else if digests.old.contains(&digest) {
        "in_vault_old"
    } else if digests.ignored.contains(&digest) {
        "ignored"
    } else {
        "new"
    }
}

fn preview(raw: &Raw) -> String {
    if raw.kind == Kind::SshKey {
        return super::ssh::inspect(raw.primary())
            .and_then(|info| info.fingerprint)
            .unwrap_or_default();
    }
    let head: String = raw.primary().chars().take(4).collect();
    format!("{head}•••")
}

pub(crate) fn views(findings: &[Raw], digests: &Digests) -> Vec<FindingView> {
    findings
        .iter()
        .enumerate()
        .map(|(id, raw)| FindingView {
            id,
            source: raw.source,
            location: raw.location.clone(),
            name: raw.name.clone(),
            preview: preview(raw),
            platform: raw.platform.clone(),
            kind: raw.kind.general(),
            risks: raw.risks.clone(),
            status: status_of(raw, digests),
        })
        .collect()
}

/// Entries for the chosen findings that are still new; the note records where each came from.
/// The same value chosen twice (found in two places) yields one entry, from the first chosen item.
pub(crate) fn entries_for(
    findings: &[Raw],
    items: &[ImportItem],
    digests: &Digests,
    note_prefix: &str,
) -> Vec<EntryInput> {
    let mut chosen = HashSet::new();
    items
        .iter()
        .filter_map(|item| {
            let raw = findings.get(item.id)?;
            // A value over the per-field limit would make the whole batch fail; leave it out.
            if raw
                .fields
                .iter()
                .any(|field| field.value.len() > MAX_FIELD_BYTES)
                || status_of(raw, digests) != "new"
                || !chosen.insert(crypto::secret_digest(raw.primary()))
            {
                return None;
            }
            Some(EntryInput {
                id: None,
                title: raw.name.clone(),
                platform: item.platform.trim().to_string(),
                kind: item.kind,
                fields: raw
                    .fields
                    .iter()
                    .map(|field| FieldInput {
                        name: field.name.clone(),
                        previous_name: None,
                        value: Some(field.value.to_string()),
                        secret: field.secret,
                    })
                    .collect(),
                expires_at: None,
                tags: Vec::new(),
                note: format!(
                    "{note_prefix}{}",
                    item.origin
                        .as_deref()
                        .map(str::trim)
                        .filter(|origin| !origin.is_empty())
                        .unwrap_or(&raw.location)
                ),
                favorite: false,
            })
        })
        .collect()
}

#[derive(Default)]
struct Job {
    running: bool,
    cancelled: bool,
    truncated: bool,
    generation: u64,
    findings: Vec<Raw>,
    cancel: Arc<AtomicBool>,
    files: Arc<AtomicUsize>,
}

fn job() -> MutexGuard<'static, Job> {
    static JOB: OnceLock<Mutex<Job>> = OnceLock::new();
    JOB.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn start(vault: &'static Vault, scope: Scope, home: PathBuf) -> Result<(), String> {
    if !vault.is_unlocked() {
        return Err(LOCKED.into());
    }
    let (generation, cancel, files) = {
        let mut job = job();
        if job.running {
            return Err(BUSY.into());
        }
        job.generation += 1;
        job.running = true;
        job.cancelled = false;
        job.truncated = false;
        job.findings.clear();
        job.cancel = Arc::new(AtomicBool::new(false));
        job.files = Arc::new(AtomicUsize::new(0));
        (job.generation, job.cancel.clone(), job.files.clone())
    };
    std::thread::spawn(move || {
        let _guard = RunGuard(generation);
        let outcome = scan(&scope, &home, &cancel, &files);
        publish(generation, cancel.load(Ordering::Relaxed), outcome);
    });
    Ok(())
}

/// Stores a finished scan unless `clear` or a newer scan has superseded it.
fn publish(generation: u64, cancelled: bool, outcome: Outcome) {
    let mut job = job();
    if job.generation != generation {
        return;
    }
    job.running = false;
    job.cancelled = cancelled;
    job.truncated = outcome.truncated;
    job.findings = outcome.findings;
}

/// If the scan thread unwinds, the job must not stay "running" forever.
struct RunGuard(u64);

impl Drop for RunGuard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let mut job = job();
        if job.generation == self.0 {
            job.running = false;
            job.findings.clear();
        }
    }
}

/// Nothing is shown while the vault is locked.
pub(crate) fn status(vault: &Vault) -> JobStatus {
    let Ok(digests) = vault.digest_sets() else {
        return JobStatus::default();
    };
    let job = job();
    JobStatus {
        running: job.running,
        cancelled: job.cancelled,
        truncated: job.truncated,
        files: job.files.load(Ordering::Relaxed),
        findings: views(&job.findings, &digests),
    }
}

pub(crate) fn cancel() {
    job().cancel.store(true, Ordering::Relaxed);
}

/// Forgets every finding; called whenever the vault locks.
pub(crate) fn clear() {
    let mut job = job();
    job.cancel.store(true, Ordering::Relaxed);
    job.generation += 1;
    job.running = false;
    job.cancelled = false;
    job.truncated = false;
    job.findings.clear();
}

pub(crate) fn import(
    vault: &Vault,
    items: &[ImportItem],
    note_prefix: &str,
) -> Result<usize, String> {
    let digests = vault.digest_sets()?;
    let inputs = entries_for(&job().findings, items, &digests, note_prefix);
    if inputs.is_empty() {
        return Ok(0);
    }
    vault.add_entries(inputs)
}

pub(crate) fn ignore(vault: &Vault, ids: &[usize]) -> Result<(), String> {
    let digests: Vec<String> = {
        let job = job();
        ids.iter()
            .filter_map(|id| job.findings.get(*id))
            .map(|raw| crypto::secret_digest(raw.primary()))
            .collect()
    };
    if digests.is_empty() {
        return Ok(());
    }
    vault.ignore(digests)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::crypto::{self, KdfParams};
    use std::collections::HashSet;

    fn sample() -> String {
        ["Zq81", "vKp3", "Lm0X", "w7Rt", "2YbN", "c4Hd"].concat()
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn no_digests() -> Digests {
        Digests {
            current: HashSet::new(),
            old: HashSet::new(),
            ignored: HashSet::new(),
        }
    }

    fn home_with_findings() -> (tempfile::TempDir, String) {
        let home = tempfile::tempdir().unwrap();
        let token = sample();
        put(
            &home.path().join(".npmrc"),
            &format!("//r/:_authToken={token}\n"),
        );
        put(
            &home.path().join("code/app/.env"),
            &format!("APP_TOKEN={token}\nAPP_TOKEN={token}\n"),
        );
        (home, token)
    }

    #[test]
    fn scan_runs_the_chosen_sources_once_per_value_and_place() {
        let (home, _) = home_with_findings();
        let scope = Scope {
            ssh: true,
            configs: true,
            env: false,
            project_dirs: vec![home.path().join("code").display().to_string()],
        };
        let outcome = scan(
            &scope,
            home.path(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        let sources: Vec<Source> = outcome.findings.iter().map(|raw| raw.source).collect();
        assert_eq!(sources, vec![Source::Config, Source::Dotenv]);
        assert!(!outcome.truncated);
    }

    #[test]
    fn statuses_previews_and_import_inputs() {
        let (home, token) = home_with_findings();
        let scope = Scope {
            ssh: false,
            configs: true,
            env: false,
            project_dirs: vec![],
        };
        let findings = scan(
            &scope,
            home.path(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        )
        .findings;
        let mut digests = no_digests();
        assert_eq!(views(&findings, &digests)[0].status, "new");
        assert_eq!(views(&findings, &digests)[0].preview, "Zq81•••");
        digests.old.insert(crypto::secret_digest(&token));
        assert_eq!(views(&findings, &digests)[0].status, "in_vault_old");
        digests.current.insert(crypto::secret_digest(&token));
        assert_eq!(views(&findings, &digests)[0].status, "in_vault");

        let items = [ImportItem {
            id: 0,
            platform: " npm registry ".into(),
            kind: Kind::Token,
            origin: None,
        }];
        assert!(
            entries_for(&findings, &items, &digests, "来源：").is_empty(),
            "known values are not imported again"
        );
        let inputs = entries_for(&findings, &items, &no_digests(), "来源：");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].platform, "npm registry");
        assert!(inputs[0].note.starts_with("来源：") && inputs[0].note.ends_with(".npmrc"));
        assert_eq!(inputs[0].fields[0].value.as_deref(), Some(token.as_str()));
    }

    #[test]
    fn a_finding_over_the_field_limit_is_left_out_of_the_import() {
        let (home, _) = home_with_findings();
        let scope = Scope {
            ssh: false,
            configs: true,
            env: false,
            project_dirs: vec![],
        };
        let mut findings = scan(
            &scope,
            home.path(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        )
        .findings;
        let items = [ImportItem {
            id: 0,
            platform: "p".into(),
            kind: Kind::Token,
            origin: None,
        }];
        assert_eq!(entries_for(&findings, &items, &no_digests(), "").len(), 1);
        findings[0].fields[0].value = zeroize::Zeroizing::new("x".repeat(MAX_FIELD_BYTES + 1));
        assert!(entries_for(&findings, &items, &no_digests(), "").is_empty());
    }

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn unlocked_vault() -> &'static Vault {
        let dir = tempfile::tempdir().unwrap();
        let vault: &'static Vault = Box::leak(Box::new(Vault::new(
            dir.path().join("vault.skv"),
            KdfParams::FAST,
        )));
        std::mem::forget(dir);
        let recovery = vault.create_begin("correct horse battery").unwrap();
        vault
            .confirm_recovery(crypto::last_group(&recovery))
            .unwrap();
        vault
    }

    fn wait_until_idle(vault: &Vault) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while status(vault).running {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn token(id: usize) -> ImportItem {
        ImportItem {
            id,
            platform: "npm".into(),
            kind: Kind::Token,
            origin: None,
        }
    }

    #[test]
    fn one_value_found_in_two_places_is_imported_once() {
        let (home, token_value) = home_with_findings();
        let scope = Scope {
            ssh: false,
            configs: true,
            env: false,
            project_dirs: vec![home.path().join("code").display().to_string()],
        };
        let findings = scan(
            &scope,
            home.path(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        )
        .findings;
        assert_eq!(findings.len(), 2);
        let inputs = entries_for(&findings, &[token(1), token(0)], &no_digests(), "from: ");
        assert_eq!(inputs.len(), 1);
        assert!(
            inputs[0].note.ends_with(".env"),
            "the first selected item wins"
        );
        assert_eq!(
            inputs[0].fields[0].value.as_deref(),
            Some(token_value.as_str())
        );
    }

    #[test]
    fn the_background_job_imports_ignores_and_forgets_on_clear() {
        let _serial = serial();
        let (home, _) = home_with_findings();
        let other = ["Qw3r", "Ty7u", "Io9p", "As5d", "Fg1h", "Jk6l"].concat();
        put(
            &home.path().join("code/other/.env"),
            &format!("OTHER_TOKEN={other}\n"),
        );
        let dir = tempfile::tempdir().unwrap();
        let vault: &'static Vault = Box::leak(Box::new(Vault::new(
            dir.path().join("vault.skv"),
            KdfParams::FAST,
        )));
        assert_eq!(
            start(vault, Scope::default(), home.path().to_path_buf())
                .err()
                .unwrap(),
            crate::vault::errors::LOCKED
        );
        let recovery = vault.create_begin("correct horse battery").unwrap();
        vault
            .confirm_recovery(crypto::last_group(&recovery))
            .unwrap();

        let scope = Scope {
            ssh: false,
            configs: true,
            env: false,
            project_dirs: vec![home.path().join("code").display().to_string()],
        };
        start(vault, scope, home.path().to_path_buf()).unwrap();
        wait_until_idle(vault);
        let found = status(vault).findings;
        assert_eq!(found.len(), 3);
        let same: Vec<usize> = found
            .iter()
            .filter(|f| f.preview == "Zq81•••")
            .map(|f| f.id)
            .collect();
        let new_one = found.iter().find(|f| f.preview == "Qw3r•••").unwrap().id;
        assert_eq!(same.len(), 2);

        let imported = import(vault, &[token(same[0]), token(same[1])], "来源：").unwrap();
        assert_eq!(imported, 1, "one value, one entry");
        let after = status(vault).findings;
        assert!(
            same.iter().all(|id| after[*id].status == "in_vault"),
            "same value found in two places"
        );
        assert_eq!(after[new_one].status, "new");
        assert_eq!(vault.list(false).unwrap().len(), 1);

        ignore(vault, &[new_one]).unwrap();
        assert_eq!(status(vault).findings[new_one].status, "ignored");
        assert_eq!(
            import(vault, &[token(new_one)], "来源：").unwrap(),
            0,
            "ignored values are not imported"
        );
        assert_eq!(vault.list(false).unwrap().len(), 1);
        assert!(
            vault
                .digest_sets()
                .unwrap()
                .ignored
                .contains(&crypto::secret_digest(&other)),
            "the ignore is stored in the vault"
        );
        ignore(vault, &[999]).unwrap();

        clear();
        assert!(status(vault).findings.is_empty());
    }

    #[test]
    fn a_cleared_scan_never_publishes_its_findings() {
        let _serial = serial();
        let (home, _) = home_with_findings();
        let vault = unlocked_vault();
        let scope = Scope {
            ssh: false,
            configs: true,
            env: false,
            project_dirs: vec![],
        };
        // Deterministic core: a finished scan whose generation was superseded is discarded.
        start(vault, scope.clone(), home.path().to_path_buf()).unwrap();
        wait_until_idle(vault);
        let stale = job().generation;
        clear();
        let outcome = scan(
            &scope,
            home.path(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        assert!(!outcome.findings.is_empty());
        publish(stale, false, outcome);
        assert!(status(vault).findings.is_empty());
        assert!(!status(vault).running);
        // End to end: clearing right after start leaves nothing behind whichever side wins the race.
        start(vault, scope, home.path().to_path_buf()).unwrap();
        clear();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(status(vault).findings.is_empty());
        assert!(!status(vault).running);
    }

    #[test]
    fn a_panicking_scan_does_not_leave_the_job_running() {
        let _serial = serial();
        clear();
        let generation = {
            let mut job = job();
            job.generation += 1;
            job.running = true;
            job.generation
        };
        let result = std::thread::spawn(move || {
            let _guard = RunGuard(generation);
            panic!("scan failed");
        })
        .join();
        assert!(result.is_err());
        assert!(!job().running);

        // A superseded scan that panics must not disturb the newer one.
        let stale = {
            let mut job = job();
            job.generation += 1;
            job.running = true;
            job.generation
        };
        job().generation += 1;
        let _ = std::thread::spawn(move || {
            let _guard = RunGuard(stale);
            panic!("scan failed");
        })
        .join();
        assert!(job().running);
        clear();
    }
}
