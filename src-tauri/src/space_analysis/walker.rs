use super::model::ScanErrorSummary;
use super::windows_fs::{allocated_size, file_identity, FileIdentity};
use jwalk::{Parallelism, WalkDir};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, Metadata};
use std::io;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);
const CANCELLATION_FILE_INTERVAL: u64 = 256;

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug, Default)]
pub struct WalkStats {
    pub files: u64,
    pub directories: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub skipped: u64,
    pub errors: ScanErrorSummary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanWalkError {
    Cancelled,
}

impl fmt::Display for ScanWalkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("scan cancelled"),
        }
    }
}

impl std::error::Error for ScanWalkError {}

pub fn measure_path<F>(
    path: &Path,
    token: &CancellationToken,
    mut on_progress: F,
) -> Result<WalkStats, ScanWalkError>
where
    F: FnMut(&WalkStats),
{
    if token.is_cancelled() {
        return Err(ScanWalkError::Cancelled);
    }

    let mut stats = WalkStats::default();
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse_point(&metadata) => {
            stats.skipped = 1;
            return Ok(stats);
        }
        Ok(_) => {}
        Err(error) => {
            record_io_error(&mut stats.errors, error.kind());
            return Ok(stats);
        }
    }

    let traversal_token = token.clone();
    let walker = WalkDir::new(path)
        .parallelism(Parallelism::RayonNewPool(4))
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir(move |_, _, _, entries| {
            for entry in entries.iter_mut().filter_map(|entry| entry.as_mut().ok()) {
                if traversal_token.is_cancelled() {
                    entry.read_children_path = None;
                    continue;
                }

                if entry.read_children_path.is_some()
                    && fs::symlink_metadata(entry.path())
                        .map(|metadata| is_link_or_reparse_point(&metadata))
                        .unwrap_or(true)
                {
                    entry.read_children_path = None;
                }
            }
        });

    let mut entries = walker.into_iter();
    let mut file_ids = HashSet::<FileIdentity>::new();
    let now = Instant::now();
    let mut last_progress = now.checked_sub(PROGRESS_INTERVAL).unwrap_or(now);

    loop {
        if token.is_cancelled() {
            return Err(ScanWalkError::Cancelled);
        }

        let Some(entry_result) = entries.next() else {
            break;
        };

        if token.is_cancelled() {
            return Err(ScanWalkError::Cancelled);
        }

        match entry_result {
            Ok(entry) => {
                if let Some(error) = entry.read_children_error.as_ref() {
                    record_walk_error(&mut stats.errors, error);
                }

                match fs::symlink_metadata(entry.path()) {
                    Ok(metadata) if is_link_or_reparse_point(&metadata) => {
                        stats.skipped = stats.skipped.saturating_add(1);
                    }
                    Ok(metadata) if metadata.is_dir() => {
                        stats.directories = stats.directories.saturating_add(1);
                    }
                    Ok(metadata) if metadata.is_file() => {
                        match file_identity(entry.path().as_path()) {
                            Ok(identity) if !file_ids.insert(identity) => {
                                stats.skipped = stats.skipped.saturating_add(1);
                            }
                            Ok(_) => {
                                stats.files = stats.files.saturating_add(1);
                                stats.logical_bytes =
                                    stats.logical_bytes.saturating_add(metadata.len());
                                stats.allocated_bytes = stats.allocated_bytes.saturating_add(
                                    allocated_size(entry.path().as_path(), &metadata),
                                );
                            }
                            Err(error) => {
                                stats.skipped = stats.skipped.saturating_add(1);
                                record_io_error(&mut stats.errors, error.kind());
                            }
                        }
                    }
                    Ok(_) => {
                        stats.skipped = stats.skipped.saturating_add(1);
                    }
                    Err(error) => record_io_error(&mut stats.errors, error.kind()),
                }
            }
            Err(error) => record_walk_error(&mut stats.errors, &error),
        }

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            on_progress(&stats);
            last_progress = Instant::now();
        }

        if stats.files % CANCELLATION_FILE_INTERVAL == 0 && token.is_cancelled() {
            return Err(ScanWalkError::Cancelled);
        }
    }

    Ok(stats)
}

fn record_walk_error(errors: &mut ScanErrorSummary, error: &jwalk::Error) {
    match error.io_error() {
        Some(error) => record_io_error(errors, error.kind()),
        None => errors.other = errors.other.saturating_add(1),
    }
}

fn record_io_error(errors: &mut ScanErrorSummary, kind: io::ErrorKind) {
    match kind {
        io::ErrorKind::PermissionDenied => {
            errors.access_denied = errors.access_denied.saturating_add(1)
        }
        io::ErrorKind::NotFound => errors.vanished = errors.vanished.saturating_add(1),
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
            errors.invalid_target = errors.invalid_target.saturating_add(1)
        }
        _ => errors.other = errors.other.saturating_add(1),
    }
}

#[cfg(windows)]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn measures_files_and_stops_after_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), vec![0u8; 8]).unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/b.bin"), vec![0u8; 16]).unwrap();

        let token = CancellationToken::default();
        let stats = measure_path(dir.path(), &token, |_| {}).unwrap();
        assert_eq!(stats.logical_bytes, 24);
        let expected_allocated = [dir.path().join("a.bin"), dir.path().join("nested/b.bin")]
            .iter()
            .map(|path| allocated_size(path, &path.metadata().unwrap()))
            .sum::<u64>();
        assert_eq!(stats.allocated_bytes, expected_allocated);
        assert_eq!(stats.files, 2);

        token.cancel();
        assert!(matches!(
            measure_path(dir.path(), &token, |_| {}),
            Err(ScanWalkError::Cancelled)
        ));
    }

    #[test]
    fn cancellation_from_progress_stops_the_active_walk() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..512 {
            std::fs::write(dir.path().join(format!("{index}.bin")), [0u8]).unwrap();
        }

        let token = CancellationToken::default();
        let callback_token = token.clone();
        let result = measure_path(dir.path(), &token, move |_| callback_token.cancel());

        assert_eq!(result.unwrap_err(), ScanWalkError::Cancelled);
    }

    #[test]
    fn read_errors_are_summarized_instead_of_failing_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");

        let stats = measure_path(&missing, &CancellationToken::default(), |_| {}).unwrap();

        assert_eq!(stats.errors.vanished, 1);
    }

    #[test]
    fn includes_hidden_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden.bin"), vec![0u8; 7]).unwrap();

        let stats = measure_path(dir.path(), &CancellationToken::default(), |_| {}).unwrap();

        assert_eq!(stats.logical_bytes, 7);
        assert_eq!(stats.files, 1);
    }

    #[test]
    fn hard_links_are_not_double_counted() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.bin");
        let alias = dir.path().join("alias.bin");
        std::fs::write(&original, vec![0u8; 32]).unwrap();
        std::fs::hard_link(&original, &alias).unwrap();

        let stats = measure_path(dir.path(), &CancellationToken::default(), |_| {}).unwrap();

        assert_eq!(stats.logical_bytes, 32);
        assert_eq!(
            stats.allocated_bytes,
            allocated_size(&original, &original.metadata().unwrap())
        );
        assert_eq!(stats.files, 1);
        assert_eq!(stats.skipped, 1);
    }

    #[test]
    fn progress_callbacks_are_throttled() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..512 {
            std::fs::write(dir.path().join(format!("{index}.bin")), [0u8]).unwrap();
        }

        let mut reports = Vec::new();
        measure_path(dir.path(), &CancellationToken::default(), |_| {
            reports.push(std::time::Instant::now());
        })
        .unwrap();

        assert!(!reports.is_empty());
        assert!(reports
            .windows(2)
            .all(|pair| pair[1].duration_since(pair[0]) >= Duration::from_millis(120)));
    }

    #[cfg(windows)]
    #[test]
    fn refuses_directory_links() {
        use std::os::windows::fs::symlink_dir;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("outside.bin"), vec![0u8; 64]).unwrap();
        let link = dir.path().join("outside-link");
        if symlink_dir(outside.path(), &link).is_err() {
            return;
        }

        let stats = measure_path(dir.path(), &CancellationToken::default(), |_| {}).unwrap();

        assert_eq!(stats.logical_bytes, 0);
        assert_eq!(stats.files, 0);
        assert_eq!(stats.skipped, 1);
    }
}
