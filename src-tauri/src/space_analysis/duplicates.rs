//! Files that are byte-for-byte the same as another file, which is space paid for twice.
//!
//! Same size is only a hint, so a group is confirmed by reading the files: the head and tail
//! first, because different files rarely agree on both, and then the whole file when it is
//! small enough to read without making the user wait. A group that could not be read in full
//! says so rather than claiming more than it checked.

use super::model::LargeFileRow;
use super::walker::CancellationToken;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// Reading both ends of a file: enough to tell apart files that merely share a size.
const EDGE: u64 = 64 * 1024;
/// Files up to this size are hashed in full; beyond it, the ends stand for the whole.
const FULL_HASH_LIMIT: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// What one copy takes.
    pub bytes: u64,
    /// What every copy past the first takes.
    pub wasted: u64,
    pub paths: Vec<String>,
    /// Whether the files were compared in full, or only by size and their ends.
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateReport {
    pub groups: Vec<DuplicateGroup>,
    /// What deleting every copy but one would free.
    pub wasted: u64,
    /// Whether the search ran to the end.
    pub complete: bool,
}

fn edges(path: &str, size: u64) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(size.to_le_bytes());
    let mut head = vec![0u8; EDGE.min(size) as usize];
    file.read_exact(&mut head).ok()?;
    hasher.update(&head);
    if size > EDGE {
        let tail_at = size.saturating_sub(EDGE);
        file.seek(SeekFrom::Start(tail_at)).ok()?;
        let mut tail = vec![0u8; (size - tail_at) as usize];
        file.read_exact(&mut tail).ok()?;
        hasher.update(&tail);
    }
    Some(hasher.finalize().into())
}

fn whole(path: &str) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hasher.finalize().into())
}

/// Reads files in full to settle a group the scan judged by size and ends alone: the sets of
/// them whose whole content agrees, each of two or more. A file that cannot be read is left
/// out of every set.
pub fn verify(paths: &[String]) -> Vec<Vec<String>> {
    let mut by_hash: HashMap<[u8; 32], Vec<String>> = HashMap::new();
    for path in paths {
        if let Some(key) = whole(path) {
            by_hash.entry(key).or_default().push(path.clone());
        }
    }
    let mut sets: Vec<Vec<String>> = by_hash.into_values().filter(|set| set.len() > 1).collect();
    sets.sort();
    sets
}

/// Groups of identical files among the scanned files, biggest waste first.
pub fn find(files: &[LargeFileRow], token: &CancellationToken, min_bytes: u64) -> DuplicateReport {
    let mut by_size: HashMap<u64, Vec<&LargeFileRow>> = HashMap::new();
    for file in files.iter().filter(|f| f.logical_bytes >= min_bytes.max(1)) {
        by_size.entry(file.logical_bytes).or_default().push(file);
    }
    let mut report = DuplicateReport {
        complete: true,
        ..DuplicateReport::default()
    };
    for (size, candidates) in by_size {
        if candidates.len() < 2 {
            continue;
        }
        if token.is_cancelled() {
            report.complete = false;
            break;
        }
        // Same size and same ends: the pass that throws out almost everything.
        let mut by_edge: HashMap<[u8; 32], Vec<&LargeFileRow>> = HashMap::new();
        for file in candidates {
            if let Some(key) = edges(&file.path, size) {
                by_edge.entry(key).or_default().push(file);
            }
        }
        for (_, group) in by_edge {
            if group.len() < 2 {
                continue;
            }
            let verified = size <= FULL_HASH_LIMIT;
            let confirmed: Vec<Vec<&LargeFileRow>> = if verified {
                let mut by_hash: HashMap<[u8; 32], Vec<&LargeFileRow>> = HashMap::new();
                for file in group {
                    if let Some(key) = whole(&file.path) {
                        by_hash.entry(key).or_default().push(file);
                    }
                }
                by_hash.into_values().filter(|g| g.len() > 1).collect()
            } else {
                vec![group]
            };
            for group in confirmed {
                report.groups.push(DuplicateGroup {
                    bytes: size,
                    wasted: size * (group.len() as u64 - 1),
                    paths: group.iter().map(|f| f.path.clone()).collect(),
                    verified,
                });
            }
        }
    }
    report
        .groups
        .sort_by_key(|group| std::cmp::Reverse(group.wasted));
    report.wasted = report.groups.iter().map(|group| group.wasted).sum();
    report
}

/// When a file was made and last changed, as milliseconds since 1970; what cannot be read is 0.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileTimes {
    pub created: i64,
    pub modified: i64,
}

pub fn times(paths: &[String]) -> Vec<FileTimes> {
    let ms = |time: std::io::Result<std::time::SystemTime>| {
        time.ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_millis() as i64)
    };
    paths
        .iter()
        .map(|path| match std::fs::metadata(path) {
            Ok(meta) => FileTimes {
                created: ms(meta.created()),
                modified: ms(meta.modified()),
            },
            Err(_) => FileTimes::default(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn row(path: &std::path::Path, bytes: u64) -> LargeFileRow {
        LargeFileRow {
            node_id: path.to_string_lossy().into_owned(),
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            allocated_bytes: bytes,
            logical_bytes: bytes,
            modified_at: None,
        }
    }

    fn write(path: &std::path::Path, content: &[u8]) {
        let mut file = File::create(path).unwrap();
        file.write_all(content).unwrap();
    }

    #[test]
    fn files_of_one_size_are_only_duplicates_when_their_bytes_agree() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        let c = dir.path().join("c.bin");
        // Same size, same ends, different middle: not a duplicate.
        let mut same = vec![7u8; 400 * 1024];
        write(&a, &same);
        write(&b, &same);
        same[200 * 1024] = 9;
        write(&c, &same);

        let files = [
            row(&a, 400 * 1024),
            row(&b, 400 * 1024),
            row(&c, 400 * 1024),
        ];
        let report = find(&files, &CancellationToken::default(), 1024);
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].paths.len(), 2);
        assert_eq!(report.groups[0].wasted, 400 * 1024);
        assert!(report.groups[0].verified);
        assert_eq!(report.wasted, 400 * 1024);
        assert!(report.complete);
    }

    #[test]
    fn reading_in_full_splits_files_that_only_shared_their_ends() {
        let dir = tempfile::tempdir().unwrap();
        let mut middle = vec![7u8; 300 * 1024];
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        let c = dir.path().join("c.bin");
        write(&a, &middle);
        write(&b, &middle);
        middle[150 * 1024] = 8;
        write(&c, &middle);
        let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
        let missing = path(&dir.path().join("gone.bin"));
        assert_eq!(
            verify(&[path(&a), path(&c), path(&b), missing]),
            vec![vec![path(&a), path(&b)]]
        );
    }

    #[test]
    fn a_file_with_nothing_of_its_size_beside_it_is_not_a_group() {
        let dir = tempfile::tempdir().unwrap();
        let only = dir.path().join("only.bin");
        write(&only, &[1u8; 2048]);
        let report = find(&[row(&only, 2048)], &CancellationToken::default(), 1024);
        assert!(report.groups.is_empty());
        assert_eq!(report.wasted, 0);
    }

    #[test]
    fn small_files_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        write(&a, b"same");
        write(&b, b"same");
        let files = [row(&a, 4), row(&b, 4)];
        // Below the threshold there is nothing worth reporting, however identical.
        assert!(find(&files, &CancellationToken::default(), 1024)
            .groups
            .is_empty());
    }

    #[test]
    fn a_file_says_when_it_was_made_and_changed_and_a_missing_one_says_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.bin");
        write(&file, b"x");
        let missing = dir.path().join("gone.bin");
        let found = times(&[
            file.to_string_lossy().into_owned(),
            missing.to_string_lossy().into_owned(),
        ]);
        assert!(found[0].created > 0 && found[0].modified > 0);
        assert_eq!(found[1], FileTimes::default());
    }
}
