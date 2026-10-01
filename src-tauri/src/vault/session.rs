//! Vault session: create, unlock, recover and rotate keys, plus the operations on an unlocked vault.
//! A new recovery key is always shown first and only written to the file after the user
//! confirms it by typing its last group.

use super::crypto::{self, KdfParams, SecretKey};
use super::errors::*;
use super::format::{self, Header, Snapshot, Wrap};
use super::model::{self, Body, Digests, EntryInput, EntryView, HistoryView, MergeStats};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

pub(crate) const MIN_PASSWORD_CHARS: usize = 12;
const MAX_FAILURES: u32 = 5;
const FAILURE_WAIT: Duration = Duration::from_secs(30);

pub(crate) struct Open {
    pub(crate) dek: SecretKey,
    pub(crate) header: Header,
    pub(crate) body: Body,
    pub(crate) snapshot: Snapshot,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

enum Phase {
    Locked,
    /// Opened with the recovery key; nothing is shown until new credentials are confirmed.
    Recovering(Open),
    Unlocked(Open),
}

enum Pending {
    Create {
        dek: SecretKey,
        password: Zeroizing<String>,
        recovery: Zeroizing<String>,
    },
    Rotate {
        password: Zeroizing<String>,
        recovery: Zeroizing<String>,
    },
}

impl Pending {
    fn recovery(&self) -> &str {
        match self {
            Pending::Create { recovery, .. } | Pending::Rotate { recovery, .. } => recovery,
        }
    }
}

struct Inner {
    phase: Phase,
    pending: Option<Pending>,
    failures: u32,
    wait_until: Option<Instant>,
    last_activity: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub exists: bool,
    pub state: &'static str,
    pub wait_seconds: u64,
    pub pending: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub(crate) enum Credential {
    Password(String),
    Recovery(String),
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credential::Password(_) => f.write_str("Password(<redacted>)"),
            Credential::Recovery(_) => f.write_str("Recovery(<redacted>)"),
        }
    }
}

type WriteFn = fn(&Path, &[u8], Option<&Snapshot>) -> Result<Snapshot, String>;

pub(crate) struct Vault {
    pub(crate) path: PathBuf,
    kdf: KdfParams,
    inner: Mutex<Inner>,
}

pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn check_password(password: &str) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        Err(WEAK.into())
    } else {
        Ok(())
    }
}

/// The typed group, read the Crockford way (O → 0, I/L → 1), against the issued one.
fn same_group(expected: &str, typed: &str) -> bool {
    let typed: String = typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect();
    !typed.is_empty() && typed == expected
}

fn check_wait(inner: &mut Inner) -> Result<(), String> {
    match inner.wait_until {
        Some(until) if Instant::now() < until => Err(WAIT.into()),
        _ => {
            inner.wait_until = None;
            Ok(())
        }
    }
}

fn register_failure(inner: &mut Inner) {
    inner.failures += 1;
    if inner.failures >= MAX_FAILURES {
        inner.failures = 0;
        inner.wait_until = Some(Instant::now() + FAILURE_WAIT);
    }
}

pub(crate) fn replace_body(open: &mut Open, next: Body) {
    std::mem::replace(&mut open.body, next).zeroize();
}

/// Where a retired vault and its backup go: `vault-<stamp>[-n].skv.old` / `.skv.bak.old`,
/// with the smallest `n` that overwrites nothing.
fn old_names(vault_path: &Path, stamp: &str) -> (PathBuf, PathBuf) {
    let mut n = 0;
    loop {
        let stem = if n == 0 {
            format!("vault-{stamp}")
        } else {
            format!("vault-{stamp}-{n}")
        };
        let main = vault_path.with_file_name(format!("{stem}.skv.old"));
        let backup = vault_path.with_file_name(format!("{stem}.skv.bak.old"));
        if !main.exists() && !backup.exists() {
            return (main, backup);
        }
        n += 1;
    }
}

type Unwrapper<'a> = &'a dyn Fn(&Header) -> Result<Option<SecretKey>, String>;

fn read_one(path: &Path, unwrap: Unwrapper) -> Result<Option<Open>, String> {
    let loaded = format::read(path)?;
    let Some(dek) = unwrap(&loaded.header)? else {
        return Ok(None);
    };
    let plain = format::decrypt_body(&loaded, &dek)?;
    let body: Body = serde_json::from_slice(&plain).map_err(|_| CORRUPT.to_string())?;
    Ok(Some(Open {
        dek,
        header: loaded.header,
        body,
        snapshot: loaded.snapshot,
    }))
}

impl Vault {
    pub(crate) fn new(path: PathBuf, kdf: KdfParams) -> Vault {
        Vault {
            path,
            kdf,
            inner: Mutex::new(Inner {
                phase: Phase::Locked,
                pending: None,
                failures: 0,
                wait_until: None,
                last_activity: Instant::now(),
            }),
        }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn status(&self) -> Status {
        let inner = self.inner();
        let exists = self.path.exists();
        let state = match inner.phase {
            Phase::Unlocked(_) => "unlocked",
            Phase::Recovering(_) => "recovering",
            Phase::Locked if exists => "locked",
            Phase::Locked => "missing",
        };
        let wait_seconds = inner
            .wait_until
            .map(|until| {
                until
                    .saturating_duration_since(Instant::now())
                    .as_secs_f64()
                    .ceil() as u64
            })
            .unwrap_or(0);
        Status {
            exists,
            state,
            wait_seconds,
            pending: inner.pending.is_some(),
        }
    }

    fn header_for(&self, dek: &[u8; 32], password: &str, recovery: &str) -> Result<Header, String> {
        let raw = crypto::decode_recovery(recovery).ok_or(CORRUPT)?;
        Ok(Header {
            kdf: self.kdf,
            password: Wrap::new(password.as_bytes(), dek, self.kdf)?,
            recovery: Wrap::new(raw.as_slice(), dek, self.kdf)?,
        })
    }

    pub(crate) fn write_body(
        &self,
        header: &Header,
        dek: &[u8; 32],
        body: &Body,
        expected: Option<&Snapshot>,
    ) -> Result<Snapshot, String> {
        self.write_with(header, dek, body, expected, format::write)
    }

    /// For writes that change a credential: the backup gets the new bytes as well.
    fn write_body_rekeyed(
        &self,
        header: &Header,
        dek: &[u8; 32],
        body: &Body,
        expected: Option<&Snapshot>,
    ) -> Result<Snapshot, String> {
        self.write_with(header, dek, body, expected, format::write_rekeyed)
    }

    fn write_with(
        &self,
        header: &Header,
        dek: &[u8; 32],
        body: &Body,
        expected: Option<&Snapshot>,
        write: WriteFn,
    ) -> Result<Snapshot, String> {
        let plain = Zeroizing::new(serde_json::to_vec(body).map_err(|_| IO.to_string())?);
        let bytes = format::encode(header, dek, &plain)?;
        write(&self.path, &bytes, expected)
    }

    /// Reads the vault; when the file itself is damaged, the backup is tried with the same
    /// credential and the next save replaces the damaged file.
    fn read_open(&self, unwrap: Unwrapper) -> Result<Option<Open>, String> {
        match read_one(&self.path, unwrap) {
            Err(error) if error == CORRUPT && format::backup_path(&self.path).exists() => {
                let opened = read_one(&format::backup_path(&self.path), unwrap)?;
                Ok(opened.map(|mut open| {
                    if let Some(current) = format::current_snapshot(&self.path) {
                        open.snapshot = current;
                    }
                    open
                }))
            }
            other => other,
        }
    }

    pub(crate) fn create_begin(&self, password: &str) -> Result<Zeroizing<String>, String> {
        if self.path.exists() {
            return Err(EXISTS.into());
        }
        check_password(password)?;
        let recovery = crypto::new_recovery_key();
        self.inner().pending = Some(Pending::Create {
            dek: crypto::new_key(),
            password: Zeroizing::new(password.to_string()),
            recovery: recovery.clone(),
        });
        Ok(recovery)
    }

    pub(crate) fn pending_recovery(&self) -> Result<Zeroizing<String>, String> {
        self.inner()
            .pending
            .as_ref()
            .map(|pending| Zeroizing::new(pending.recovery().to_string()))
            .ok_or_else(|| NO_PENDING.to_string())
    }

    pub(crate) fn confirm_recovery(&self, last_group: &str) -> Result<(), String> {
        let mut inner = self.inner();
        let pending = inner.pending.as_ref().ok_or(NO_PENDING)?;
        if !same_group(crypto::last_group(pending.recovery()), last_group) {
            return Err(CONFIRM.into());
        }
        match inner.pending.take().expect("checked above") {
            Pending::Create {
                dek,
                password,
                recovery,
            } => {
                if self.path.exists() {
                    return Err(EXISTS.into());
                }
                let header = self.header_for(&dek, &password, &recovery)?;
                let body = Body::default();
                let snapshot = self.write_body_rekeyed(&header, &dek, &body, None)?;
                inner.phase = Phase::Unlocked(Open {
                    dek,
                    header,
                    body,
                    snapshot,
                });
            }
            Pending::Rotate { password, recovery } => {
                let dek = crypto::new_key();
                let header = self.header_for(&dek, &password, &recovery)?;
                let open = match &mut inner.phase {
                    Phase::Recovering(open) | Phase::Unlocked(open) => open,
                    Phase::Locked => return Err(LOCKED.into()),
                };
                let snapshot =
                    self.write_body_rekeyed(&header, &dek, &open.body, Some(&open.snapshot))?;
                let body = std::mem::take(&mut open.body);
                inner.phase = Phase::Unlocked(Open {
                    dek,
                    header,
                    body,
                    snapshot,
                });
            }
        }
        inner.last_activity = Instant::now();
        Ok(())
    }

    /// Drops whatever was waiting for confirmation; a recovery in progress goes back to locked.
    pub(crate) fn cancel_pending(&self) {
        let mut inner = self.inner();
        inner.pending = None;
        if matches!(inner.phase, Phase::Recovering(_)) {
            inner.phase = Phase::Locked;
        }
    }

    pub(crate) fn unlock(&self, password: &str) -> Result<(), String> {
        let mut inner = self.inner();
        check_wait(&mut inner)?;
        let opened =
            self.read_open(&|header| header.password.unwrap_key(password.as_bytes(), header.kdf))?;
        let Some(mut open) = opened else {
            register_failure(&mut inner);
            return Err(PASSWORD.into());
        };
        inner.failures = 0;
        inner.pending = None;
        let mut purged = open.body.clone();
        // A failed purge write must not keep the user out: they unlock with the old body and
        // the purge is retried at the next unlock.
        if model::purge_expired(&mut purged, now_ms()) {
            match self.write_body(&open.header, &open.dek, &purged, Some(&open.snapshot)) {
                Ok(snapshot) => {
                    open.snapshot = snapshot;
                    replace_body(&mut open, purged);
                }
                Err(_) => purged.zeroize(),
            }
        } else {
            purged.zeroize();
        }
        inner.phase = Phase::Unlocked(open);
        inner.last_activity = Instant::now();
        Ok(())
    }

    pub(crate) fn unlock_recovery(&self, recovery_key: &str) -> Result<(), String> {
        let mut inner = self.inner();
        check_wait(&mut inner)?;
        let Some(raw) = crypto::decode_recovery(recovery_key) else {
            register_failure(&mut inner);
            return Err(RECOVERY.into());
        };
        let opened =
            self.read_open(&|header| header.recovery.unwrap_key(raw.as_slice(), header.kdf))?;
        let Some(open) = opened else {
            register_failure(&mut inner);
            return Err(RECOVERY.into());
        };
        inner.failures = 0;
        inner.pending = None;
        inner.phase = Phase::Recovering(open);
        inner.last_activity = Instant::now();
        Ok(())
    }

    pub(crate) fn recovery_set_password(
        &self,
        password: &str,
    ) -> Result<Zeroizing<String>, String> {
        let mut inner = self.inner();
        if !matches!(inner.phase, Phase::Recovering(_)) {
            return Err(LOCKED.into());
        }
        check_password(password)?;
        let recovery = crypto::new_recovery_key();
        inner.pending = Some(Pending::Rotate {
            password: Zeroizing::new(password.to_string()),
            recovery: recovery.clone(),
        });
        Ok(recovery)
    }

    /// The recovery key is not at hand here, so the data key stays and is only re-wrapped.
    pub(crate) fn change_password(&self, current: &str, next: &str) -> Result<(), String> {
        let mut inner = self.inner();
        check_wait(&mut inner)?;
        check_password(next)?;
        let Phase::Unlocked(open) = &mut inner.phase else {
            return Err(LOCKED.into());
        };
        if open
            .header
            .password
            .unwrap_key(current.as_bytes(), open.header.kdf)?
            .is_none()
        {
            register_failure(&mut inner);
            return Err(PASSWORD.into());
        }
        let mut header = open.header.clone();
        header.password = Wrap::new(next.as_bytes(), &open.dek, header.kdf)?;
        open.snapshot =
            self.write_body_rekeyed(&header, &open.dek, &open.body, Some(&open.snapshot))?;
        open.header = header;
        // A rotation waiting for confirmation carries the old password; confirming it later would
        // write that password back.
        inner.pending = None;
        inner.failures = 0;
        inner.last_activity = Instant::now();
        Ok(())
    }

    pub(crate) fn reset_recovery(&self, password: &str) -> Result<Zeroizing<String>, String> {
        let mut inner = self.inner();
        check_wait(&mut inner)?;
        let Phase::Unlocked(open) = &inner.phase else {
            return Err(LOCKED.into());
        };
        if open
            .header
            .password
            .unwrap_key(password.as_bytes(), open.header.kdf)?
            .is_none()
        {
            register_failure(&mut inner);
            return Err(PASSWORD.into());
        }
        inner.failures = 0;
        let recovery = crypto::new_recovery_key();
        inner.pending = Some(Pending::Rotate {
            password: Zeroizing::new(password.to_string()),
            recovery: recovery.clone(),
        });
        Ok(recovery)
    }

    pub(crate) fn lock(&self) {
        let mut inner = self.inner();
        inner.phase = Phase::Locked;
        inner.pending = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        !matches!(self.inner().phase, Phase::Locked)
    }

    pub(crate) fn is_unlocked(&self) -> bool {
        matches!(self.inner().phase, Phase::Unlocked(_))
    }

    pub(crate) fn touch(&self) {
        self.inner().last_activity = Instant::now();
    }

    pub(crate) fn idle_for(&self) -> Duration {
        self.inner().last_activity.elapsed()
    }

    pub(crate) fn restore_backup(&self, src: &Path) -> Result<(), String> {
        if self.path.exists() {
            return Err(EXISTS.into());
        }
        format::read(src)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|_| IO.to_string())?;
        }
        std::fs::copy(src, &self.path)
            .map(|_| ())
            .map_err(|_| IO.to_string())
    }

    /// Both credentials lost: the file is renamed, never deleted, so it can be imported later.
    pub(crate) fn reset(&self) -> Result<String, String> {
        let mut inner = self.inner();
        inner.phase = Phase::Locked;
        inner.pending = None;
        if !self.path.exists() {
            return Err(MISSING.into());
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let (target, backup_target) = old_names(&self.path, &stamp);
        std::fs::rename(&self.path, &target).map_err(|_| IO.to_string())?;
        let backup = format::backup_path(&self.path);
        if backup.exists() {
            std::fs::rename(&backup, &backup_target).map_err(|_| IO.to_string())?;
        }
        Ok(target.display().to_string())
    }

    fn with_unlocked<T>(
        &self,
        f: impl FnOnce(&mut Open) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut inner = self.inner();
        inner.last_activity = Instant::now();
        match &mut inner.phase {
            Phase::Unlocked(open) => f(open),
            _ => Err(LOCKED.into()),
        }
    }

    /// Changes a copy, saves it, and only then swaps it in, so a failed save leaves memory
    /// and disk agreeing.
    fn mutate<T>(&self, f: impl FnOnce(&mut Body) -> Result<T, String>) -> Result<T, String> {
        self.with_unlocked(|open| {
            let mut next = open.body.clone();
            let out = match f(&mut next) {
                Ok(out) => out,
                Err(error) => {
                    next.zeroize();
                    return Err(error);
                }
            };
            match self.write_body(&open.header, &open.dek, &next, Some(&open.snapshot)) {
                Ok(snapshot) => {
                    open.snapshot = snapshot;
                    replace_body(open, next);
                    Ok(out)
                }
                Err(error) => {
                    next.zeroize();
                    Err(error)
                }
            }
        })
    }

    pub(crate) fn list(&self, trash: bool) -> Result<Vec<EntryView>, String> {
        self.with_unlocked(|open| Ok(model::views(&open.body, trash)))
    }

    pub(crate) fn save(&self, input: EntryInput) -> Result<EntryView, String> {
        let id = self.mutate(|body| model::apply_input(body, input, now_ms()))?;
        self.with_unlocked(|open| model::view_of(&open.body, &id))
    }

    pub(crate) fn reveal(&self, id: &str, field: &str) -> Result<Zeroizing<String>, String> {
        self.with_unlocked(|open| model::field_value(&open.body, id, field))
    }

    pub(crate) fn history(&self, id: &str) -> Result<Vec<HistoryView>, String> {
        self.with_unlocked(|open| model::history_views(&open.body, id))
    }

    pub(crate) fn history_value(
        &self,
        id: &str,
        index: usize,
    ) -> Result<Zeroizing<String>, String> {
        self.with_unlocked(|open| model::history_value(&open.body, id, index))
    }

    pub(crate) fn delete(&self, id: &str) -> Result<(), String> {
        self.mutate(|body| model::soft_delete(body, id, now_ms()))
    }

    pub(crate) fn restore(&self, id: &str) -> Result<(), String> {
        self.mutate(|body| model::restore(body, id))
    }

    pub(crate) fn purge(&self, id: &str) -> Result<(), String> {
        self.mutate(|body| model::purge(body, id))
    }

    pub(crate) fn favorite(&self, id: &str, favorite: bool) -> Result<(), String> {
        self.mutate(|body| model::set_favorite(body, id, favorite, now_ms()))
    }

    pub(crate) fn ssh_private(&self, id: &str) -> Result<Zeroizing<String>, String> {
        self.with_unlocked(|open| model::ssh_private(&open.body, id))
    }

    fn verify_password(&self, password: &str) -> Result<(), String> {
        let mut inner = self.inner();
        check_wait(&mut inner)?;
        let Phase::Unlocked(open) = &inner.phase else {
            return Err(LOCKED.into());
        };
        if open
            .header
            .password
            .unwrap_key(password.as_bytes(), open.header.kdf)?
            .is_none()
        {
            register_failure(&mut inner);
            return Err(PASSWORD.into());
        }
        inner.failures = 0;
        Ok(())
    }

    /// The backup is the vault file as it is, still sealed by its master password and recovery key.
    pub(crate) fn export(&self, password: &str, dest: &Path) -> Result<(), String> {
        self.verify_password(password)?;
        if dest == self.path {
            return Err(FILE_EXISTS.into());
        }
        std::fs::copy(&self.path, dest)
            .map(|_| ())
            .map_err(|_| IO.to_string())
    }

    fn read_backup(&self, src: &Path, credential: &Credential) -> Result<Body, String> {
        let mut opened = match credential {
            Credential::Password(password) => read_one(src, &|header| {
                header.password.unwrap_key(password.as_bytes(), header.kdf)
            })?
            .ok_or(PASSWORD)?,
            Credential::Recovery(key) => {
                let raw = crypto::decode_recovery(key).ok_or(RECOVERY)?;
                read_one(src, &|header| {
                    header.recovery.unwrap_key(raw.as_slice(), header.kdf)
                })?
                .ok_or(RECOVERY)?
            }
        };
        Ok(std::mem::take(&mut opened.body))
    }

    pub(crate) fn import_preview(
        &self,
        src: &Path,
        credential: &Credential,
    ) -> Result<MergeStats, String> {
        if !self.is_unlocked() {
            return Err(LOCKED.into());
        }
        let mut incoming = self.read_backup(src, credential)?;
        let stats = self.with_unlocked(|open| {
            let mut copy = open.body.clone();
            let stats = model::merge(&mut copy, &incoming);
            copy.zeroize();
            Ok(stats)
        });
        incoming.zeroize();
        stats
    }

    pub(crate) fn import_apply(
        &self,
        src: &Path,
        credential: &Credential,
    ) -> Result<MergeStats, String> {
        if !self.is_unlocked() {
            return Err(LOCKED.into());
        }
        let mut incoming = self.read_backup(src, credential)?;
        let stats = self.mutate(|body| Ok(model::merge(body, &incoming)));
        incoming.zeroize();
        stats
    }

    pub(crate) fn digest_sets(&self) -> Result<Digests, String> {
        self.with_unlocked(|open| Ok(model::digests(&open.body)))
    }

    pub(crate) fn add_entries(&self, inputs: Vec<EntryInput>) -> Result<usize, String> {
        self.mutate(|body| {
            let now = now_ms();
            let count = inputs.len();
            for input in inputs {
                model::apply_input(body, input, now)?;
            }
            Ok(count)
        })
    }

    pub(crate) fn ignore(&self, digests: Vec<String>) -> Result<(), String> {
        self.mutate(|body| {
            for digest in digests {
                if !body.ignored.contains(&digest) {
                    body.ignored.push(digest);
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const PW: &str = "correct horse battery";
    const PW2: &str = "another long password";

    pub(super) fn fresh(dir: &tempfile::TempDir) -> Vault {
        Vault::new(dir.path().join("vault.skv"), KdfParams::FAST)
    }

    /// A created, unlocked vault and its recovery key.
    pub(super) fn created(dir: &tempfile::TempDir) -> (Vault, String) {
        let vault = fresh(dir);
        let recovery = vault.create_begin(PW).unwrap();
        vault
            .confirm_recovery(crypto::last_group(&recovery))
            .unwrap();
        (vault, recovery.to_string())
    }

    #[test]
    fn creating_waits_for_the_recovery_key_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let vault = fresh(&dir);
        let recovery = vault.create_begin(PW).unwrap();
        assert!(!vault.path.exists());
        assert_eq!(
            vault.status(),
            Status {
                exists: false,
                state: "missing",
                wait_seconds: 0,
                pending: true
            }
        );
        assert_eq!(*vault.pending_recovery().unwrap(), *recovery);
        assert_eq!(vault.confirm_recovery("ZZZZ").err().unwrap(), CONFIRM);
        assert!(
            vault.status().pending,
            "a wrong group keeps the pending key"
        );
        let typed = crypto::last_group(&recovery).to_ascii_lowercase();
        vault.confirm_recovery(&typed).unwrap();
        assert!(vault.path.exists());
        assert_eq!(vault.status().state, "unlocked");
    }

    #[test]
    fn creating_rejects_short_passwords_and_second_vaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(fresh(&dir).create_begin("short").err().unwrap(), WEAK);
        let (vault, _) = created(&dir);
        assert_eq!(vault.create_begin(PW).err().unwrap(), EXISTS);
    }

    #[test]
    fn lock_and_unlock_with_the_master_password() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        vault.lock();
        assert_eq!(vault.status().state, "locked");
        assert_eq!(vault.unlock("wrong password!!").err().unwrap(), PASSWORD);
        vault.unlock(PW).unwrap();
        assert!(vault.is_unlocked());
        let again = fresh(&dir);
        again.unlock(PW).unwrap();
    }

    #[test]
    fn five_wrong_attempts_make_it_wait() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        vault.lock();
        for _ in 0..5 {
            assert_eq!(vault.unlock("wrong password!!").err().unwrap(), PASSWORD);
        }
        assert_eq!(vault.unlock(PW).err().unwrap(), WAIT);
        assert!(vault.status().wait_seconds > 0);
    }

    #[test]
    fn recovery_unlock_issues_new_credentials_and_retires_the_old_key() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        vault.lock();
        assert_eq!(
            vault
                .unlock_recovery("0000-0000-0000-0000-0000-0000-0000-0000")
                .err()
                .unwrap(),
            RECOVERY
        );
        vault.unlock_recovery(&recovery).unwrap();
        assert_eq!(vault.status().state, "recovering");
        assert!(!vault.is_unlocked());
        let next = vault.recovery_set_password(PW2).unwrap();
        vault.confirm_recovery(crypto::last_group(&next)).unwrap();
        assert!(vault.is_unlocked());
        vault.lock();
        assert_eq!(vault.unlock_recovery(&recovery).err().unwrap(), RECOVERY);
        assert_eq!(vault.unlock(PW).err().unwrap(), PASSWORD);
        vault.unlock(PW2).unwrap();
        vault.lock();
        vault.unlock_recovery(&next).unwrap();
    }

    #[test]
    fn cancelling_recovery_keeps_the_vault_locked_and_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        vault.lock();
        vault.unlock_recovery(&recovery).unwrap();
        vault.recovery_set_password(PW2).unwrap();
        vault.cancel_pending();
        assert_eq!(vault.status().state, "locked");
        vault.unlock(PW).unwrap();
    }

    #[test]
    fn changing_the_password_keeps_the_recovery_key() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        assert_eq!(
            vault
                .change_password("wrong password!!", PW2)
                .err()
                .unwrap(),
            PASSWORD
        );
        assert_eq!(vault.change_password(PW, "short").err().unwrap(), WEAK);
        vault.change_password(PW, PW2).unwrap();
        vault.lock();
        assert_eq!(vault.unlock(PW).err().unwrap(), PASSWORD);
        vault.unlock(PW2).unwrap();
        vault.lock();
        vault.unlock_recovery(&recovery).unwrap();
    }

    #[test]
    fn resetting_the_recovery_key_needs_the_password_and_retires_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        assert_eq!(
            vault.reset_recovery("wrong password!!").err().unwrap(),
            PASSWORD
        );
        let next = vault.reset_recovery(PW).unwrap();
        assert!(vault.is_unlocked(), "entries stay visible while confirming");
        vault.confirm_recovery(crypto::last_group(&next)).unwrap();
        vault.lock();
        assert_eq!(vault.unlock_recovery(&recovery).err().unwrap(), RECOVERY);
        vault.unlock_recovery(&next).unwrap();
        vault.cancel_pending();
        vault.unlock(PW).unwrap();
    }

    #[test]
    fn a_damaged_file_falls_back_to_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let next = vault.reset_recovery(PW).unwrap();
        vault.confirm_recovery(crypto::last_group(&next)).unwrap();
        vault.lock();
        std::fs::write(&vault.path, b"not a vault at all").unwrap();
        vault.unlock(PW).unwrap();
    }

    #[test]
    fn reset_keeps_the_old_file_under_a_new_name() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let kept = vault.reset().unwrap();
        assert!(kept.ends_with(".skv.old"));
        assert!(std::path::Path::new(&kept).exists());
        assert_eq!(vault.status().state, "missing");
    }

    #[test]
    fn restoring_a_backup_only_into_an_empty_place() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let elsewhere = tempfile::tempdir().unwrap();
        let target = fresh(&elsewhere);
        target.restore_backup(&vault.path).unwrap();
        assert_eq!(target.status().state, "locked");
        target.unlock(PW).unwrap();
        assert_eq!(target.restore_backup(&vault.path).err().unwrap(), EXISTS);
        let garbage = elsewhere.path().join("garbage.skv");
        std::fs::write(&garbage, b"nothing").unwrap();
        assert_eq!(
            fresh(&tempfile::tempdir().unwrap())
                .restore_backup(&garbage)
                .err()
                .unwrap(),
            CORRUPT
        );
    }

    fn corrupt_main_file(vault: &Vault) {
        std::fs::write(&vault.path, b"not a vault at all").unwrap();
    }

    #[test]
    fn the_retired_recovery_key_does_not_open_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, old) = created(&dir);
        let next = vault.reset_recovery(PW).unwrap();
        vault.confirm_recovery(crypto::last_group(&next)).unwrap();
        vault.lock();
        corrupt_main_file(&vault);
        assert_eq!(vault.unlock_recovery(&old).err().unwrap(), RECOVERY);
        vault.unlock_recovery(&next).unwrap();
    }

    #[test]
    fn the_retired_password_does_not_open_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        vault.change_password(PW, PW2).unwrap();
        vault.lock();
        corrupt_main_file(&vault);
        assert_eq!(vault.unlock(PW).err().unwrap(), PASSWORD);
        vault.unlock(PW2).unwrap();
    }

    #[test]
    fn changing_the_password_drops_a_pending_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let next = vault.reset_recovery(PW).unwrap();
        vault.change_password(PW, PW2).unwrap();
        assert!(!vault.status().pending);
        assert_eq!(
            vault
                .confirm_recovery(crypto::last_group(&next))
                .err()
                .unwrap(),
            NO_PENDING
        );
        vault.lock();
        assert_eq!(vault.unlock(PW).err().unwrap(), PASSWORD);
        vault.unlock(PW2).unwrap();
    }

    #[test]
    fn an_abandoned_recovery_leaves_the_file_and_the_old_key_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        vault.lock();
        let before = format::current_snapshot(&vault.path);
        vault.unlock_recovery(&recovery).unwrap();
        vault.recovery_set_password(PW2).unwrap();
        vault.cancel_pending();
        assert_eq!(format::current_snapshot(&vault.path), before);
        vault.unlock_recovery(&recovery).unwrap();
    }

    #[test]
    fn an_abandoned_rotation_leaves_the_file_and_the_old_key_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        let before = format::current_snapshot(&vault.path);
        vault.reset_recovery(PW).unwrap();
        vault.cancel_pending();
        assert_eq!(format::current_snapshot(&vault.path), before);
        vault.reset_recovery(PW).unwrap();
        vault.lock();
        assert_eq!(format::current_snapshot(&vault.path), before);
        vault.unlock_recovery(&recovery).unwrap();
    }

    #[test]
    fn reset_renames_the_backup_alongside_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let next = vault.reset_recovery(PW).unwrap();
        vault.confirm_recovery(crypto::last_group(&next)).unwrap();
        assert!(format::backup_path(&vault.path).exists());
        let first = vault.reset().unwrap();
        assert!(!format::backup_path(&vault.path).exists());
        assert!(std::path::Path::new(&first.replace(".skv.old", ".skv.bak.old")).exists());
        // The same second again: the earlier files must survive.
        vault.restore_backup(std::path::Path::new(&first)).unwrap();
        let second = vault.reset().unwrap();
        assert_ne!(first, second);
        assert!(std::path::Path::new(&first).exists());
        assert!(std::path::Path::new(&second).exists());
    }

    #[test]
    fn old_names_skip_what_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.skv");
        let (main, backup) = old_names(&vault_path, "20260101-000000");
        assert_eq!(main.file_name().unwrap(), "vault-20260101-000000.skv.old");
        assert_eq!(
            backup.file_name().unwrap(),
            "vault-20260101-000000.skv.bak.old"
        );
        std::fs::write(&main, b"x").unwrap();
        assert_eq!(
            old_names(&vault_path, "20260101-000000")
                .0
                .file_name()
                .unwrap(),
            "vault-20260101-000000-1.skv.old"
        );
        std::fs::write(dir.path().join("vault-20260101-000000-1.skv.bak.old"), b"x").unwrap();
        std::fs::write(dir.path().join("vault-20260101-000000-1.skv.old"), b"x").unwrap();
        let (main, backup) = old_names(&vault_path, "20260101-000000");
        assert_eq!(main.file_name().unwrap(), "vault-20260101-000000-2.skv.old");
        assert_eq!(
            backup.file_name().unwrap(),
            "vault-20260101-000000-2.skv.bak.old"
        );
    }

    #[test]
    fn a_failed_password_change_leaves_memory_and_file_in_agreement() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let blocker = vault.path.with_extension("skv.bak.tmp");
        std::fs::create_dir(&blocker).unwrap();
        let before = format::current_snapshot(&vault.path);
        assert_eq!(vault.change_password(PW, PW2).err().unwrap(), IO);
        assert_eq!(format::current_snapshot(&vault.path), before);
        assert!(!vault.path.with_extension("skv.tmp").exists());
        std::fs::remove_dir(&blocker).unwrap();
        // The in-memory snapshot still matches the file, so a retry is not refused as CHANGED.
        vault.change_password(PW, PW2).unwrap();
        vault.lock();
        assert_eq!(vault.unlock(PW).err().unwrap(), PASSWORD);
        vault.unlock(PW2).unwrap();
    }

    #[test]
    fn a_failed_password_change_keeps_the_old_password_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        std::fs::create_dir(vault.path.with_extension("skv.bak.tmp")).unwrap();
        assert_eq!(vault.change_password(PW, PW2).err().unwrap(), IO);
        vault.lock();
        vault.unlock(PW).unwrap();
    }

    #[test]
    fn a_failed_confirm_keeps_the_old_recovery_key_working() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, old) = created(&dir);
        let next = vault.reset_recovery(PW).unwrap();
        std::fs::create_dir(vault.path.with_extension("skv.bak.tmp")).unwrap();
        assert_eq!(
            vault
                .confirm_recovery(crypto::last_group(&next))
                .err()
                .unwrap(),
            IO
        );
        vault.lock();
        vault.unlock_recovery(&old).unwrap();
    }

    #[test]
    fn a_failing_purge_write_does_not_keep_the_user_out() {
        use super::super::model::{self, EntryInput, Kind};
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let mut body = Body::default();
        let id = model::apply_input(
            &mut body,
            EntryInput {
                id: None,
                title: "old".into(),
                platform: String::new(),
                kind: Kind::TokenPlan,
                fields: vec![],
                expires_at: None,
                tags: vec![],
                note: String::new(),
                favorite: false,
            },
            1,
        )
        .unwrap();
        model::soft_delete(&mut body, &id, 1).unwrap();
        {
            let mut inner = vault.inner();
            let Phase::Unlocked(open) = &mut inner.phase else {
                panic!("created vault is unlocked")
            };
            open.snapshot = vault
                .write_body(&open.header, &open.dek, &body, Some(&open.snapshot))
                .unwrap();
        }
        vault.lock();
        // A directory where the temporary file belongs makes the purge write fail.
        std::fs::create_dir(vault.path.with_extension("skv.tmp")).unwrap();
        let before = format::current_snapshot(&vault.path);
        vault.unlock(PW).unwrap();
        assert!(vault.is_unlocked());
        assert_eq!(
            format::current_snapshot(&vault.path),
            before,
            "nothing was purged on disk"
        );
        let inner = vault.inner();
        let Phase::Unlocked(open) = &inner.phase else {
            panic!("unlocked")
        };
        assert_eq!(
            open.body.entries.len(),
            1,
            "the expired entry is still there, to be purged next time"
        );
    }

    #[test]
    fn credentials_never_print_their_values() {
        let shown = format!(
            "{:?} {:?}",
            Credential::Password("hunter2-hunter2".into()),
            Credential::Recovery("ABCD".into())
        );
        assert!(!shown.contains("hunter2") && !shown.contains("ABCD"));
        assert!(shown.contains("Password") && shown.contains("Recovery"));
    }
}

#[cfg(test)]
mod entry_tests {
    use super::tests::{created, fresh, PW};
    use super::*;
    use crate::vault::model::{FieldInput, Kind};

    fn input(title: &str, secret: &str) -> EntryInput {
        EntryInput {
            id: None,
            title: title.into(),
            platform: "GitHub".into(),
            kind: Kind::Token,
            fields: vec![FieldInput {
                name: "Token".into(),
                previous_name: None,
                value: Some(secret.into()),
                secret: true,
            }],
            expires_at: None,
            tags: vec![],
            note: String::new(),
            favorite: false,
        }
    }

    #[test]
    fn entries_are_saved_encrypted_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let view = vault.save(input("gh", "tok-value-1")).unwrap();
        assert_eq!(view.fields[0].value, None);
        let raw = std::fs::read(&vault.path).unwrap();
        assert!(
            !raw.windows(11).any(|w| w == b"tok-value-1"),
            "never stored in clear"
        );
        let again = fresh(&dir);
        again.unlock(PW).unwrap();
        assert_eq!(*again.reveal(&view.id, "Token").unwrap(), "tok-value-1");
    }

    #[test]
    fn everything_needs_an_unlocked_vault() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let id = vault.save(input("gh", "v")).unwrap().id;
        vault.lock();
        assert_eq!(vault.list(false).err().unwrap(), LOCKED);
        assert_eq!(vault.reveal(&id, "Token").err().unwrap(), LOCKED);
        assert_eq!(vault.save(input("x", "y")).err().unwrap(), LOCKED);
    }

    #[test]
    fn a_recovering_vault_shows_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        let id = vault.save(input("gh", "v")).unwrap().id;
        vault.lock();
        vault.unlock_recovery(&recovery).unwrap();
        assert_eq!(vault.list(false).err().unwrap(), LOCKED);
        assert_eq!(vault.reveal(&id, "Token").err().unwrap(), LOCKED);
        assert_eq!(vault.history(&id).err().unwrap(), LOCKED);
        assert_eq!(vault.ssh_private(&id).err().unwrap(), LOCKED);
        assert_eq!(vault.digest_sets().err().unwrap(), LOCKED);
        assert_eq!(vault.save(input("x", "y")).err().unwrap(), LOCKED);
        assert_eq!(
            vault.add_entries(vec![input("x", "y")]).err().unwrap(),
            LOCKED
        );
        assert_eq!(vault.ignore(vec!["d".into()]).err().unwrap(), LOCKED);
        let elsewhere = dir.path().join("b.skv");
        assert_eq!(
            vault
                .import_preview(&elsewhere, &Credential::Password(PW.into()))
                .err()
                .unwrap(),
            LOCKED
        );
        assert_eq!(
            vault
                .import_apply(&elsewhere, &Credential::Password(PW.into()))
                .err()
                .unwrap(),
            LOCKED
        );
        assert_eq!(vault.export(PW, &elsewhere).err().unwrap(), LOCKED);
    }

    #[test]
    fn delete_restore_purge_favorite_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let id = vault.save(input("gh", "first")).unwrap().id;
        let mut edit = input("gh", "second");
        edit.id = Some(id.clone());
        vault.save(edit).unwrap();
        assert_eq!(vault.history(&id).unwrap().len(), 1);
        assert_eq!(*vault.history_value(&id, 0).unwrap(), "first");
        vault.favorite(&id, true).unwrap();
        assert!(vault.list(false).unwrap()[0].favorite);
        vault.delete(&id).unwrap();
        assert!(vault.list(false).unwrap().is_empty());
        vault.restore(&id).unwrap();
        vault.delete(&id).unwrap();
        vault.purge(&id).unwrap();
        assert!(vault.list(true).unwrap().is_empty());
    }

    #[test]
    fn a_file_changed_elsewhere_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        let other = fresh(&dir);
        other.unlock(PW).unwrap();
        other.save(input("from other", "o")).unwrap();
        assert_eq!(vault.save(input("mine", "m")).err().unwrap(), CHANGED);
        assert!(
            vault.list(false).unwrap().is_empty(),
            "memory stays as it was"
        );
    }

    #[test]
    fn export_then_import_merges_without_deleting() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, recovery) = created(&dir);
        vault.save(input("shared", "s")).unwrap();
        let backup = dir.path().join("backup.skv");
        assert_eq!(
            vault.export("wrong password!!", &backup).err().unwrap(),
            PASSWORD
        );
        vault.export(PW, &backup).unwrap();

        let other_dir = tempfile::tempdir().unwrap();
        let (other, _) = created(&other_dir);
        other.save(input("local only", "l")).unwrap();
        let by_password = Credential::Password(PW.into());
        let stats = other.import_preview(&backup, &by_password).unwrap();
        assert_eq!(stats.added, 1);
        assert_eq!(
            other.list(false).unwrap().len(),
            1,
            "a preview changes nothing"
        );
        assert_eq!(
            other
                .import_apply(&backup, &Credential::Password("wrong password!!".into()))
                .err()
                .unwrap(),
            PASSWORD
        );
        other
            .import_apply(&backup, &Credential::Recovery(recovery))
            .unwrap();
        assert_eq!(other.list(false).unwrap().len(), 2);
    }

    #[test]
    fn discovery_helpers_add_ignore_and_report_digests() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _) = created(&dir);
        assert_eq!(
            vault
                .add_entries(vec![input("a", "va"), input("b", "vb")])
                .unwrap(),
            2
        );
        vault.ignore(vec!["d1".into(), "d1".into()]).unwrap();
        let digests = vault.digest_sets().unwrap();
        assert!(digests.current.contains(&crypto::secret_digest("va")));
        assert_eq!(digests.ignored.len(), 1);
    }
}
