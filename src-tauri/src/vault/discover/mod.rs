//! 发现散落密钥：只读扫描本机常见位置，结果只留在内存，保管库锁定即清空。

pub(crate) mod rules;
pub(crate) mod sources;

use super::crypto;
use super::errors::{BUSY, LOCKED};
use super::model::{Digests, EntryInput, FieldInput, Kind};
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
}

pub(crate) struct Outcome {
    pub findings: Vec<Raw>,
    pub truncated: bool,
}

pub(crate) fn scan(scope: &Scope, home: &Path, cancel: &AtomicBool, files: &AtomicUsize) -> Outcome {
    let truncated = AtomicBool::new(false);
    let walk = Walk { cancel, files, truncated: &truncated };
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
    found.retain(|raw| seen.insert((raw.location.clone(), raw.name.clone(), crypto::secret_digest(raw.primary()))));
    Outcome { findings: found, truncated: truncated.load(Ordering::Relaxed) }
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
        return super::ssh::inspect(raw.primary()).and_then(|info| info.fingerprint).unwrap_or_default();
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
            kind: raw.kind,
            risks: raw.risks.clone(),
            status: status_of(raw, digests),
        })
        .collect()
}

/// Entries for the chosen findings that are still new; the note records where each came from.
pub(crate) fn entries_for(findings: &[Raw], items: &[ImportItem], digests: &Digests, note_prefix: &str) -> Vec<EntryInput> {
    items
        .iter()
        .filter_map(|item| {
            let raw = findings.get(item.id)?;
            if status_of(raw, digests) != "new" {
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
                note: format!("{note_prefix}{}", raw.location),
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
    JOB.get_or_init(Default::default).lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
        let outcome = scan(&scope, &home, &cancel, &files);
        let mut job = job();
        if job.generation != generation {
            return;
        }
        job.running = false;
        job.cancelled = cancel.load(Ordering::Relaxed);
        job.truncated = outcome.truncated;
        job.findings = outcome.findings;
    });
    Ok(())
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

pub(crate) fn import(vault: &Vault, items: &[ImportItem], note_prefix: &str) -> Result<usize, String> {
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
        Digests { current: HashSet::new(), old: HashSet::new(), ignored: HashSet::new() }
    }

    fn home_with_findings() -> (tempfile::TempDir, String) {
        let home = tempfile::tempdir().unwrap();
        let token = sample();
        put(&home.path().join(".npmrc"), &format!("//r/:_authToken={token}\n"));
        put(&home.path().join("code/app/.env"), &format!("APP_TOKEN={token}\nAPP_TOKEN={token}\n"));
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
        let outcome = scan(&scope, home.path(), &AtomicBool::new(false), &AtomicUsize::new(0));
        let sources: Vec<Source> = outcome.findings.iter().map(|raw| raw.source).collect();
        assert_eq!(sources, vec![Source::Config, Source::Dotenv]);
        assert!(!outcome.truncated);
    }

    #[test]
    fn statuses_previews_and_import_inputs() {
        let (home, token) = home_with_findings();
        let scope = Scope { ssh: false, configs: true, env: false, project_dirs: vec![] };
        let findings = scan(&scope, home.path(), &AtomicBool::new(false), &AtomicUsize::new(0)).findings;
        let mut digests = no_digests();
        assert_eq!(views(&findings, &digests)[0].status, "new");
        assert_eq!(views(&findings, &digests)[0].preview, "Zq81•••");
        digests.old.insert(crypto::secret_digest(&token));
        assert_eq!(views(&findings, &digests)[0].status, "in_vault_old");
        digests.current.insert(crypto::secret_digest(&token));
        assert_eq!(views(&findings, &digests)[0].status, "in_vault");

        let items = [ImportItem { id: 0, platform: " npm registry ".into(), kind: Kind::Token }];
        assert!(entries_for(&findings, &items, &digests, "来源：").is_empty(), "known values are not imported again");
        let inputs = entries_for(&findings, &items, &no_digests(), "来源：");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].platform, "npm registry");
        assert!(inputs[0].note.starts_with("来源：") && inputs[0].note.ends_with(".npmrc"));
        assert_eq!(inputs[0].fields[0].value.as_deref(), Some(token.as_str()));
    }

    #[test]
    fn the_background_job_imports_ignores_and_forgets_on_clear() {
        let (home, token) = home_with_findings();
        let dir = tempfile::tempdir().unwrap();
        let vault: &'static Vault = Box::leak(Box::new(Vault::new(dir.path().join("vault.skv"), KdfParams::FAST)));
        assert_eq!(start(vault, Scope::default(), home.path().to_path_buf()).err().unwrap(), crate::vault::errors::LOCKED);
        let recovery = vault.create_begin("correct horse battery").unwrap();
        vault.confirm_recovery(crypto::last_group(&recovery)).unwrap();

        let scope = Scope { ssh: false, configs: true, env: false, project_dirs: vec![home.path().join("code").display().to_string()] };
        start(vault, scope, home.path().to_path_buf()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while status(vault).running {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let found = status(vault).findings;
        assert_eq!(found.len(), 2);

        let imported = import(vault, &[ImportItem { id: 0, platform: "npm".into(), kind: Kind::Token }], "来源：").unwrap();
        assert_eq!(imported, 1);
        let after = status(vault).findings;
        assert_eq!(after[0].status, "in_vault");
        assert_eq!(after[1].status, "in_vault", "same value found in two places");
        assert_eq!(vault.list(false).unwrap().len(), 1);
        let _ = token;

        ignore(vault, &[1]).unwrap();
        clear();
        assert!(status(vault).findings.is_empty());
    }
}
