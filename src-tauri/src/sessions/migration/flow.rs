//! The migration steps, their undo, and moving back. Pure over paths; persistence is a callback.
use super::fsops::{self, TreeStats};
use crate::runner::CancelFlag;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Copying,
    Copied,
    Renamed,
    Linked,
    Done,
    Cleaned,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub source: PathBuf,
    pub target: PathBuf,
    pub backup: PathBuf,
    pub step: Step,
    pub bytes: u64,
    pub files: u64,
    pub started_at: u64,
}

impl Record {
    pub fn new(source: &Path, target: &Path, now: u64) -> Self {
        let mut backup = source.as_os_str().to_owned();
        backup.push(".stacker-backup");
        Self {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
            backup: PathBuf::from(backup),
            step: Step::Copying,
            bytes: 0,
            files: 0,
            started_at: now,
        }
    }

    pub fn incomplete(&self) -> bool {
        matches!(
            self.step,
            Step::Copying | Step::Copied | Step::Renamed | Step::Linked
        )
    }
}

pub type Persist<'a> = &'a mut dyn FnMut(Option<&Record>) -> Result<(), String>;

fn rename_error(e: std::io::Error) -> String {
    match e.raw_os_error() {
        Some(5) | Some(32) | Some(33) => "E_IN_USE".into(),
        _ => "E_STORAGE".into(),
    }
}

fn same_stats(a: &TreeStats, b: &TreeStats) -> bool {
    a == b
}

/// Copy → verify → rename → link. Any failure is undone before returning.
pub fn migrate(
    record: &mut Record,
    expected: TreeStats,
    copied: &AtomicU64,
    cancel: &CancelFlag,
    persist: Persist,
) -> Result<(), String> {
    record.bytes = expected.bytes;
    record.files = expected.files;
    persist(Some(record))?;
    let result = (|| -> Result<(), String> {
        fsops::copy_tree(&record.source, &record.target, copied, cancel)?;
        if !same_stats(&fsops::stats(&record.target)?, &expected) {
            return Err("E_VERIFY".into());
        }
        record.step = Step::Copied;
        persist(Some(record))?;
        fs::rename(&record.source, &record.backup).map_err(rename_error)?;
        record.step = Step::Renamed;
        persist(Some(record))?;
        fsops::create_junction(&record.source, &record.target)?;
        record.step = Step::Linked;
        persist(Some(record))?;
        // Reading through the junction proves it resolves.
        fs::read_dir(&record.source).map_err(|_| "E_VERIFY".to_string())?;
        record.step = Step::Done;
        persist(Some(record))
    })();
    if let Err(code) = result {
        undo(record, persist)?;
        return Err(code);
    }
    Ok(())
}

/// Returns to the state before the migration started, based on the recorded step.
pub fn undo(record: &Record, persist: Persist) -> Result<(), String> {
    if matches!(record.step, Step::Linked | Step::Done) && fsops::is_link(&record.source) {
        fsops::remove_junction(&record.source)?;
    }
    if matches!(record.step, Step::Renamed | Step::Linked | Step::Done)
        && record.backup.exists()
        && !record.source.exists()
    {
        fs::rename(&record.backup, &record.source).map_err(rename_error)?;
    }
    if record.target.exists() && record.source.exists() && !fsops::is_link(&record.source) {
        fsops::remove_tree(&record.target)?;
    }
    persist(None)
}

pub fn delete_backup(record: &mut Record, persist: Persist) -> Result<(), String> {
    if record.step != Step::Done {
        return Err("E_REQUEST".into());
    }
    if record.backup.exists() {
        fsops::remove_tree(&record.backup)?;
    }
    record.step = Step::Cleaned;
    persist(Some(record))
}

/// Moves the data back to the original location. The copy on the other drive is kept.
pub fn move_back(
    record: &Record,
    copied: &AtomicU64,
    cancel: &CancelFlag,
    persist: Persist,
) -> Result<(), String> {
    match record.step {
        Step::Done => undo_keep_target(record, persist),
        Step::Cleaned => {
            let mut restore = record.source.as_os_str().to_owned();
            restore.push(".stacker-restore");
            let restore = PathBuf::from(restore);
            if restore.exists() {
                fsops::remove_tree(&restore)?;
            }
            let expected = fsops::stats(&record.target)?;
            let result = (|| {
                fsops::copy_tree(&record.target, &restore, copied, cancel)?;
                if fsops::stats(&restore)? != expected {
                    return Err("E_VERIFY".to_string());
                }
                fsops::remove_junction(&record.source)?;
                fs::rename(&restore, &record.source).map_err(rename_error)
            })();
            if let Err(code) = result {
                if !record.source.exists() {
                    let _ = fsops::create_junction(&record.source, &record.target);
                }
                if restore.exists() {
                    let _ = fsops::remove_tree(&restore);
                }
                return Err(code);
            }
            persist(None)
        }
        _ => Err("E_REQUEST".into()),
    }
}

fn undo_keep_target(record: &Record, persist: Persist) -> Result<(), String> {
    fsops::remove_junction(&record.source)?;
    if let Err(e) = fs::rename(&record.backup, &record.source) {
        let _ = fsops::create_junction(&record.source, &record.target);
        return Err(rename_error(e));
    }
    persist(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join(".codex");
        fs::create_dir_all(src.join("sessions")).unwrap();
        fs::write(src.join("sessions").join("a.jsonl"), b"{}").unwrap();
        fs::write(src.join("config.toml"), b"x=1").unwrap();
        let target = dir.path().join("D").join("codex");
        (dir, src, target)
    }

    fn run(record: &mut Record, log: &mut Vec<Option<Step>>) -> Result<(), String> {
        let expected = fsops::stats(&record.source).unwrap();
        let mut persist = |r: Option<&Record>| {
            log.push(r.map(|r| r.step));
            Ok(())
        };
        migrate(
            record,
            expected,
            &AtomicU64::new(0),
            &CancelFlag::default(),
            &mut persist,
        )
    }

    #[test]
    fn migrates_links_and_moves_back_both_ways() {
        let (_dir, src, target) = setup();
        let mut record = Record::new(&src, &target, 0);
        let mut log = Vec::new();
        run(&mut record, &mut log).unwrap();
        assert_eq!(log.last().unwrap(), &Some(Step::Done));
        assert!(fsops::is_junction(&src));
        assert_eq!(fs::read(src.join("config.toml")).unwrap(), b"x=1");
        assert!(record.backup.exists());

        // Back while the backup still exists: instant, copy on D kept.
        let mut none = |_: Option<&Record>| Ok(());
        move_back(
            &record,
            &AtomicU64::new(0),
            &CancelFlag::default(),
            &mut none,
        )
        .unwrap();
        assert!(!fsops::is_link(&src) && src.join("config.toml").exists());
        assert!(target.exists());

        // Again, then delete the backup and move back by copying.
        fsops::remove_tree(&target).unwrap();
        let mut record = Record::new(&src, &target, 0);
        run(&mut record, &mut Vec::new()).unwrap();
        delete_backup(&mut record, &mut none).unwrap();
        assert!(!record.backup.exists());
        move_back(
            &record,
            &AtomicU64::new(0),
            &CancelFlag::default(),
            &mut none,
        )
        .unwrap();
        assert!(!fsops::is_link(&src));
        assert_eq!(
            fs::read(src.join("sessions").join("a.jsonl")).unwrap(),
            b"{}"
        );
    }

    #[test]
    fn a_locked_source_rolls_back() {
        let (_dir, src, target) = setup();
        let lock = fs::File::open(src.join("config.toml")).unwrap();
        let mut record = Record::new(&src, &target, 0);
        let mut log = Vec::new();
        let err = run(&mut record, &mut log).unwrap_err();
        drop(lock);
        assert_eq!(err, "E_IN_USE");
        assert_eq!(log.last().unwrap(), &None, "record cleared");
        assert!(!target.exists(), "copy removed");
        assert!(src.join("config.toml").exists() && !fsops::is_link(&src));
    }

    #[test]
    fn an_interrupted_migration_can_be_undone() {
        let (_dir, src, target) = setup();
        let mut record = Record::new(&src, &target, 0);
        run(&mut record, &mut Vec::new()).unwrap();
        // Pretend the app died right after linking.
        record.step = Step::Linked;
        assert!(record.incomplete());
        let mut none = |_: Option<&Record>| Ok(());
        undo(&record, &mut none).unwrap();
        assert!(!fsops::is_link(&src) && src.join("config.toml").exists());
        assert!(!target.exists() && !record.backup.exists());
    }

    #[test]
    fn deletion_checks_hold_through_the_junction() {
        let (_dir, src, target) = setup();
        let mut record = Record::new(&src, &target, 0);
        run(&mut record, &mut Vec::new()).unwrap();
        let inside = fs::canonicalize(src.join("sessions").join("a.jsonl")).unwrap();
        let root = fs::canonicalize(&src).unwrap();
        assert!(
            inside.starts_with(&root),
            "C1/C2 inside-root checks resolve both to the new drive"
        );
    }
}
