//! Vault session: create, unlock, recover and rotate keys, plus the operations on an unlocked vault.
//! A new recovery key is always shown first and only written to the file after the user
//! confirms it by typing its last group.

use super::crypto::{self, KdfParams, SecretKey};
use super::errors::*;
use super::format::{self, Header, Snapshot, Wrap};
use super::model::Body;
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
    Create { dek: SecretKey, password: Zeroizing<String>, recovery: Zeroizing<String> },
    Rotate { password: Zeroizing<String>, recovery: Zeroizing<String> },
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub(crate) enum Credential {
    Password(String),
    Recovery(String),
}

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

type Unwrapper<'a> = &'a dyn Fn(&Header) -> Result<Option<SecretKey>, String>;

fn read_one(path: &Path, unwrap: Unwrapper) -> Result<Option<Open>, String> {
    let loaded = format::read(path)?;
    let Some(dek) = unwrap(&loaded.header)? else {
        return Ok(None);
    };
    let plain = format::decrypt_body(&loaded, &dek)?;
    let body: Body = serde_json::from_slice(&plain).map_err(|_| CORRUPT.to_string())?;
    Ok(Some(Open { dek, header: loaded.header, body, snapshot: loaded.snapshot }))
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
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .map(|until| until.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64)
            .unwrap_or(0);
        Status { exists, state, wait_seconds, pending: inner.pending.is_some() }
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
        let plain = Zeroizing::new(serde_json::to_vec(body).map_err(|_| IO.to_string())?);
        let bytes = format::encode(header, dek, &plain)?;
        format::write(&self.path, &bytes, expected)
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
            Pending::Create { dek, password, recovery } => {
                if self.path.exists() {
                    return Err(EXISTS.into());
                }
                let header = self.header_for(&dek, &password, &recovery)?;
                let body = Body::default();
                let snapshot = self.write_body(&header, &dek, &body, None)?;
                inner.phase = Phase::Unlocked(Open { dek, header, body, snapshot });
            }
            Pending::Rotate { password, recovery } => {
                let dek = crypto::new_key();
                let header = self.header_for(&dek, &password, &recovery)?;
                let open = match &mut inner.phase {
                    Phase::Recovering(open) | Phase::Unlocked(open) => open,
                    Phase::Locked => return Err(LOCKED.into()),
                };
                let snapshot = self.write_body(&header, &dek, &open.body, Some(&open.snapshot))?;
                let body = std::mem::take(&mut open.body);
                inner.phase = Phase::Unlocked(Open { dek, header, body, snapshot });
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
        let opened = self.read_open(&|header| header.password.unwrap_key(password.as_bytes(), header.kdf))?;
        let Some(mut open) = opened else {
            register_failure(&mut inner);
            return Err(PASSWORD.into());
        };
        inner.failures = 0;
        inner.pending = None;
        let mut purged = open.body.clone();
        if super::model::purge_expired(&mut purged, now_ms()) {
            open.snapshot = self.write_body(&open.header, &open.dek, &purged, Some(&open.snapshot))?;
            replace_body(&mut open, purged);
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
        let opened = self.read_open(&|header| header.recovery.unwrap_key(raw.as_slice(), header.kdf))?;
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

    pub(crate) fn recovery_set_password(&self, password: &str) -> Result<Zeroizing<String>, String> {
        let mut inner = self.inner();
        if !matches!(inner.phase, Phase::Recovering(_)) {
            return Err(LOCKED.into());
        }
        check_password(password)?;
        let recovery = crypto::new_recovery_key();
        inner.pending = Some(Pending::Rotate { password: Zeroizing::new(password.to_string()), recovery: recovery.clone() });
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
        if open.header.password.unwrap_key(current.as_bytes(), open.header.kdf)?.is_none() {
            register_failure(&mut inner);
            return Err(PASSWORD.into());
        }
        let mut header = open.header.clone();
        header.password = Wrap::new(next.as_bytes(), &open.dek, header.kdf)?;
        open.snapshot = self.write_body(&header, &open.dek, &open.body, Some(&open.snapshot))?;
        open.header = header;
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
        if open.header.password.unwrap_key(password.as_bytes(), open.header.kdf)?.is_none() {
            register_failure(&mut inner);
            return Err(PASSWORD.into());
        }
        inner.failures = 0;
        let recovery = crypto::new_recovery_key();
        inner.pending = Some(Pending::Rotate { password: Zeroizing::new(password.to_string()), recovery: recovery.clone() });
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
        std::fs::copy(src, &self.path).map(|_| ()).map_err(|_| IO.to_string())
    }

    /// Both credentials lost: the file is renamed, never deleted, so it can be imported later.
    pub(crate) fn reset(&self) -> Result<String, String> {
        let mut inner = self.inner();
        inner.phase = Phase::Locked;
        inner.pending = None;
        if !self.path.exists() {
            return Err(MISSING.into());
        }
        let name = format!("vault-{}.skv.old", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let target = self.path.with_file_name(name);
        std::fs::rename(&self.path, &target).map_err(|_| IO.to_string())?;
        Ok(target.display().to_string())
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
        vault.confirm_recovery(crypto::last_group(&recovery)).unwrap();
        (vault, recovery.to_string())
    }

    #[test]
    fn creating_waits_for_the_recovery_key_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let vault = fresh(&dir);
        let recovery = vault.create_begin(PW).unwrap();
        assert!(!vault.path.exists());
        assert_eq!(vault.status(), Status { exists: false, state: "missing", wait_seconds: 0, pending: true });
        assert_eq!(*vault.pending_recovery().unwrap(), *recovery);
        assert_eq!(vault.confirm_recovery("ZZZZ").err().unwrap(), CONFIRM);
        assert!(vault.status().pending, "a wrong group keeps the pending key");
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
        assert_eq!(vault.unlock_recovery("0000-0000-0000-0000-0000-0000-0000-0000").err().unwrap(), RECOVERY);
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
        assert_eq!(vault.change_password("wrong password!!", PW2).err().unwrap(), PASSWORD);
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
        assert_eq!(vault.reset_recovery("wrong password!!").err().unwrap(), PASSWORD);
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
        assert_eq!(fresh(&tempfile::tempdir().unwrap()).restore_backup(&garbage).err().unwrap(), CORRUPT);
    }
}
