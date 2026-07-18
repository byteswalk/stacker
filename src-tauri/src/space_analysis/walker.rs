use super::model::{AnalysisSummary, DirectoryNode, LargeFileRow, Paged, ScanErrorSummary};
use super::windows_fs::{allocated_size, file_identity, FileIdentity};
use chrono::{DateTime, Utc};
use jwalk::{Parallelism, WalkDir};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);
const MAX_PAGE_SIZE: u64 = 200;
const VIEW_ONLY: &str = "ViewOnly";

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

impl WalkStats {
    fn skipped_paths(&self) -> u64 {
        self.skipped
            .saturating_add(self.errors.access_denied)
            .saturating_add(self.errors.vanished)
            .saturating_add(self.errors.invalid_target)
            .saturating_add(self.errors.other)
    }
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

type NodeId = String;

struct NodeRecord {
    node: DirectoryNode,
    direct_allocated_bytes: u64,
    direct_logical_bytes: u64,
    child_ids: Vec<NodeId>,
}

pub(crate) struct IndexedScanResult {
    summary: AnalysisSummary,
    nodes: HashMap<NodeId, NodeRecord>,
    large_files: Vec<LargeFileRow>,
}

impl IndexedScanResult {
    pub(crate) fn summary(&self) -> AnalysisSummary {
        self.summary.clone()
    }

    pub(crate) fn set_task_id(&mut self, task_id: &str) {
        self.summary.task_id = task_id.to_string();
    }

    pub(crate) fn children(
        &self,
        parent_id: &str,
        offset: u64,
        limit: u64,
    ) -> Option<Paged<DirectoryNode>> {
        let record = self.nodes.get(parent_id)?;
        let total = record.child_ids.len() as u64;
        let limit = limit.min(MAX_PAGE_SIZE);
        let items = page_slice(&record.child_ids, offset, limit)
            .iter()
            .filter_map(|node_id| self.nodes.get(node_id))
            .map(|child| child.node.clone())
            .collect();

        Some(Paged {
            items,
            offset,
            limit,
            total,
        })
    }

    pub(crate) fn large_files(
        &self,
        min_bytes: u64,
        offset: u64,
        limit: u64,
    ) -> Paged<LargeFileRow> {
        let limit = limit.min(MAX_PAGE_SIZE);
        let threshold_end = self
            .large_files
            .partition_point(|row| row.allocated_bytes >= min_bytes);
        let matching_files = &self.large_files[..threshold_end];
        let total = matching_files.len() as u64;
        let items = page_slice(matching_files, offset, limit)
            .iter()
            .cloned()
            .collect();

        Paged {
            items,
            offset,
            limit,
            total,
        }
    }
}

fn page_slice<T>(items: &[T], offset: u64, limit: u64) -> &[T] {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(items.len());
    let count = usize::try_from(limit).unwrap_or(usize::MAX);
    let end = start.saturating_add(count).min(items.len());
    &items[start..end]
}

#[derive(Default)]
struct AccountingContext {
    file_ids: HashSet<FileIdentity>,
}

trait WalkVisitor {
    fn directory(&mut self, _path: &Path) {}
    fn file(&mut self, _path: &Path, _metadata: &Metadata, _allocated_bytes: u64) {}
}

struct NoopVisitor;
impl WalkVisitor for NoopVisitor {}

pub fn measure_path<F>(
    path: &Path,
    token: &CancellationToken,
    mut on_progress: F,
) -> Result<WalkStats, ScanWalkError>
where
    F: FnMut(&WalkStats),
{
    let mut stats = WalkStats::default();
    let mut accounting = AccountingContext::default();
    let mut visitor = NoopVisitor;
    walk_path(
        path,
        token,
        &mut accounting,
        &mut stats,
        &mut visitor,
        &mut on_progress,
    )?;
    Ok(stats)
}

pub(crate) fn build_indexed_result<F>(
    targets: &[PathBuf],
    token: &CancellationToken,
    mut on_progress: F,
) -> Result<IndexedScanResult, ScanWalkError>
where
    F: FnMut(&WalkStats),
{
    let mut stats = WalkStats::default();
    let mut accounting = AccountingContext::default();
    let mut builder = IndexBuilder::new(targets);

    for target in targets {
        walk_path(
            target,
            token,
            &mut accounting,
            &mut stats,
            &mut builder,
            &mut on_progress,
        )?;
        on_progress(&stats);
    }

    Ok(builder.finish(stats))
}

fn walk_path<V, F>(
    path: &Path,
    token: &CancellationToken,
    accounting: &mut AccountingContext,
    stats: &mut WalkStats,
    visitor: &mut V,
    on_progress: &mut F,
) -> Result<(), ScanWalkError>
where
    V: WalkVisitor,
    F: FnMut(&WalkStats),
{
    if token.is_cancelled() {
        return Err(ScanWalkError::Cancelled);
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse_point(&metadata) => {
            stats.skipped = stats.skipped.saturating_add(1);
            return Ok(());
        }
        Ok(_) => {}
        Err(error) => {
            record_io_error(&mut stats.errors, error.kind());
            return Ok(());
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
    let now = Instant::now();
    let mut last_progress = now.checked_sub(PROGRESS_INTERVAL).unwrap_or(now);

    loop {
        if token.is_cancelled() {
            return Err(ScanWalkError::Cancelled);
        }

        let Some(entry_result) = entries.next() else {
            break;
        };

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
                        visitor.directory(entry.path().as_path());
                    }
                    Ok(metadata) if metadata.is_file() => {
                        match file_identity(entry.path().as_path()) {
                            Ok(identity) if !accounting.file_ids.insert(identity) => {
                                stats.skipped = stats.skipped.saturating_add(1);
                            }
                            Ok(_) => {
                                let allocated_bytes =
                                    allocated_size(entry.path().as_path(), &metadata);
                                stats.files = stats.files.saturating_add(1);
                                stats.logical_bytes =
                                    stats.logical_bytes.saturating_add(metadata.len());
                                stats.allocated_bytes =
                                    stats.allocated_bytes.saturating_add(allocated_bytes);
                                visitor.file(entry.path().as_path(), &metadata, allocated_bytes);
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
            on_progress(stats);
            last_progress = Instant::now();
        }
    }

    Ok(())
}

struct IndexBuilder {
    targets: Vec<PathBuf>,
    path_ids: HashMap<PathBuf, NodeId>,
    nodes: HashMap<NodeId, NodeRecord>,
    large_files: Vec<LargeFileRow>,
    next_node_id: u64,
}

impl IndexBuilder {
    fn new(targets: &[PathBuf]) -> Self {
        Self {
            targets: targets.to_vec(),
            path_ids: HashMap::new(),
            nodes: HashMap::new(),
            large_files: Vec::new(),
            next_node_id: 1,
        }
    }

    fn allocate_node_id(&mut self) -> NodeId {
        let node_id = format!("node-{}", self.next_node_id);
        self.next_node_id = self.next_node_id.saturating_add(1);
        node_id
    }

    fn finish(mut self, stats: WalkStats) -> IndexedScanResult {
        let root_ids = self
            .targets
            .iter()
            .filter_map(|target| self.path_ids.get(target).cloned())
            .collect::<Vec<_>>();

        aggregate_directories(&root_ids, &mut self.nodes);
        sort_children(&mut self.nodes);
        self.large_files.sort_by(compare_large_files);

        let root_nodes = root_ids
            .iter()
            .filter_map(|node_id| self.nodes.get(node_id))
            .map(|record| record.node.clone())
            .collect();
        let summary = AnalysisSummary {
            task_id: String::new(),
            targets: self
                .targets
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            allocated_bytes: stats.allocated_bytes,
            logical_bytes: stats.logical_bytes,
            file_count: stats.files,
            directory_count: self.nodes.len() as u64,
            skipped_paths: stats.skipped_paths(),
            root_nodes,
        };

        IndexedScanResult {
            summary,
            nodes: self.nodes,
            large_files: self.large_files,
        }
    }
}

impl WalkVisitor for IndexBuilder {
    fn directory(&mut self, path: &Path) {
        let path = path.to_path_buf();
        if self.path_ids.contains_key(&path) {
            return;
        }

        let parent_id = path
            .parent()
            .and_then(|parent| self.path_ids.get(parent))
            .cloned();
        let node_id = self.allocate_node_id();
        let name = path
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let record = NodeRecord {
            node: DirectoryNode {
                node_id: node_id.clone(),
                parent_id: parent_id.clone(),
                name,
                path: path.to_string_lossy().into_owned(),
                allocated_bytes: 0,
                logical_bytes: 0,
                child_count: 0,
                safety: VIEW_ONLY.into(),
            },
            direct_allocated_bytes: 0,
            direct_logical_bytes: 0,
            child_ids: Vec::new(),
        };

        self.path_ids.insert(path, node_id.clone());
        self.nodes.insert(node_id.clone(), record);
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(&parent_id) {
                parent.child_ids.push(node_id);
            }
        }
    }

    fn file(&mut self, path: &Path, metadata: &Metadata, allocated_bytes: u64) {
        let Some(parent_id) = path
            .parent()
            .and_then(|parent| self.path_ids.get(parent))
            .cloned()
        else {
            return;
        };
        let logical_bytes = metadata.len();
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.direct_allocated_bytes = parent
                .direct_allocated_bytes
                .saturating_add(allocated_bytes);
            parent.direct_logical_bytes = parent.direct_logical_bytes.saturating_add(logical_bytes);
        }

        let modified_at = metadata
            .modified()
            .ok()
            .map(DateTime::<Utc>::from)
            .map(|timestamp| timestamp.to_rfc3339());
        let node_id = self.allocate_node_id();
        self.large_files.push(LargeFileRow {
            node_id,
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            path: path.to_string_lossy().into_owned(),
            allocated_bytes,
            logical_bytes,
            modified_at,
        });
    }
}

fn aggregate_directories(root_ids: &[NodeId], nodes: &mut HashMap<NodeId, NodeRecord>) {
    let mut stack = root_ids
        .iter()
        .rev()
        .cloned()
        .map(|node_id| (node_id, false))
        .collect::<Vec<_>>();

    while let Some((node_id, expanded)) = stack.pop() {
        let Some(record) = nodes.get(&node_id) else {
            continue;
        };

        if !expanded {
            let child_ids = record.child_ids.clone();
            stack.push((node_id, true));
            stack.extend(
                child_ids
                    .into_iter()
                    .rev()
                    .map(|child_id| (child_id, false)),
            );
            continue;
        }

        let mut allocated_bytes = record.direct_allocated_bytes;
        let mut logical_bytes = record.direct_logical_bytes;
        let child_ids = record.child_ids.clone();
        for child_id in &child_ids {
            if let Some(child) = nodes.get(child_id) {
                allocated_bytes = allocated_bytes.saturating_add(child.node.allocated_bytes);
                logical_bytes = logical_bytes.saturating_add(child.node.logical_bytes);
            }
        }

        if let Some(record) = nodes.get_mut(&node_id) {
            record.node.allocated_bytes = allocated_bytes;
            record.node.logical_bytes = logical_bytes;
            record.node.child_count = child_ids.len().min(u32::MAX as usize) as u32;
        }
    }
}

fn sort_children(nodes: &mut HashMap<NodeId, NodeRecord>) {
    let sort_keys = nodes
        .iter()
        .map(|(node_id, record)| {
            (
                node_id.clone(),
                (
                    record.node.allocated_bytes,
                    record.node.logical_bytes,
                    record.node.name.to_lowercase(),
                ),
            )
        })
        .collect::<HashMap<_, _>>();

    for record in nodes.values_mut() {
        record.child_ids.sort_by(|left, right| {
            let left_key = &sort_keys[left];
            let right_key = &sort_keys[right];
            right_key
                .0
                .cmp(&left_key.0)
                .then_with(|| right_key.1.cmp(&left_key.1))
                .then_with(|| left_key.2.cmp(&right_key.2))
                .then_with(|| left.cmp(right))
        });
    }
}

fn compare_large_files(left: &LargeFileRow, right: &LargeFileRow) -> std::cmp::Ordering {
    right
        .allocated_bytes
        .cmp(&left.allocated_bytes)
        .then_with(|| right.logical_bytes.cmp(&left.logical_bytes))
        .then_with(|| left.path.to_lowercase().cmp(&right.path.to_lowercase()))
        .then_with(|| left.node_id.cmp(&right.node_id))
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
    fn builds_indexed_result() {
        let fixture = tempfile::tempdir().unwrap();
        let largest = fixture.path().join("largest");
        let smaller = fixture.path().join("smaller");
        let nested = smaller.join("nested");
        std::fs::create_dir(&largest).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(largest.join("large.bin"), vec![0u8; 32]).unwrap();
        std::fs::write(smaller.join("small.bin"), vec![0u8; 8]).unwrap();
        std::fs::write(nested.join("medium.bin"), vec![0u8; 16]).unwrap();

        let result = build_indexed_result(
            &[fixture.path().to_path_buf()],
            &CancellationToken::default(),
            |_| {},
        )
        .unwrap();
        let summary = result.summary();
        let root = &summary.root_nodes[0];
        let children = result.children(&root.node_id, 0, 10).unwrap();

        assert_eq!(summary.file_count, 3);
        assert_eq!(summary.directory_count, 4);
        assert_eq!(root.allocated_bytes, summary.allocated_bytes);
        assert_eq!(root.logical_bytes, 56);
        assert_eq!(children.items[0].name, "largest");
        assert_eq!(
            root.allocated_bytes,
            children
                .items
                .iter()
                .map(|child| child.allocated_bytes)
                .sum::<u64>()
        );
        let large_files = result.large_files(20, 0, 10);
        assert_eq!(large_files.items.len(), 1);
        assert_eq!(large_files.items[0].name, "large.bin");
    }

    #[test]
    fn paging_is_bounded_and_large_files_are_sorted() {
        let fixture = tempfile::tempdir().unwrap();
        for index in 0..205 {
            let child = fixture.path().join(format!("child-{index:03}"));
            std::fs::create_dir(&child).unwrap();
            std::fs::write(child.join("file.bin"), vec![0u8; index + 1]).unwrap();
        }

        let result = build_indexed_result(
            &[fixture.path().to_path_buf()],
            &CancellationToken::default(),
            |_| {},
        )
        .unwrap();
        let root_id = &result.summary().root_nodes[0].node_id;
        let children = result.children(root_id, 0, u64::MAX).unwrap();
        let files = result.large_files(0, 0, u64::MAX);

        assert_eq!(children.limit, 200);
        assert_eq!(children.items.len(), 200);
        assert_eq!(children.total, 205);
        assert_eq!(files.limit, 200);
        assert_eq!(files.items.len(), 200);
        assert!(files
            .items
            .windows(2)
            .all(|rows| rows[0].allocated_bytes >= rows[1].allocated_bytes));
    }

    #[test]
    fn large_file_threshold_pages_the_sorted_prefix() {
        let result = IndexedScanResult {
            summary: AnalysisSummary::default(),
            nodes: HashMap::new(),
            large_files: [100, 90, 90, 80, 70]
                .into_iter()
                .enumerate()
                .map(|(index, allocated_bytes)| LargeFileRow {
                    node_id: format!("file-{index}"),
                    name: format!("file-{index}.bin"),
                    path: format!("/file-{index}.bin"),
                    allocated_bytes,
                    logical_bytes: allocated_bytes,
                    modified_at: None,
                })
                .collect(),
        };

        let page = result.large_files(90, 1, 2);
        assert_eq!(page.total, 3);
        assert_eq!(page.offset, 1);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].allocated_bytes, 90);
        assert_eq!(page.items[1].allocated_bytes, 90);
        assert_eq!(result.large_files(91, 0, 10).total, 1);
        assert_eq!(result.large_files(101, 0, 10).total, 0);
        assert!(result.large_files(70, 10, 10).items.is_empty());
    }

    #[test]
    fn deeply_nested_directories_are_aggregated_without_recursion() {
        const DEPTH: usize = 25_000;

        let mut nodes = HashMap::with_capacity(DEPTH);
        for index in 0..DEPTH {
            let node_id = format!("node-{index}");
            let child_ids = (index + 1 < DEPTH)
                .then(|| vec![format!("node-{}", index + 1)])
                .unwrap_or_default();
            nodes.insert(
                node_id.clone(),
                NodeRecord {
                    node: DirectoryNode {
                        node_id,
                        parent_id: (index > 0).then(|| format!("node-{}", index - 1)),
                        name: index.to_string(),
                        path: format!("/{index}"),
                        allocated_bytes: 0,
                        logical_bytes: 0,
                        child_count: 0,
                        safety: VIEW_ONLY.into(),
                    },
                    direct_allocated_bytes: 1,
                    direct_logical_bytes: 2,
                    child_ids,
                },
            );
        }

        aggregate_directories(&["node-0".into()], &mut nodes);

        let root = &nodes["node-0"].node;
        assert_eq!(root.allocated_bytes, DEPTH as u64);
        assert_eq!(root.logical_bytes, (DEPTH * 2) as u64);
        assert_eq!(root.child_count, 1);
        assert_eq!(nodes[&format!("node-{}", DEPTH - 1)].node.child_count, 0);
    }

    #[test]
    fn hard_links_across_targets_are_counted_once() {
        let fixture = tempfile::tempdir().unwrap();
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let original = first.join("original.bin");
        std::fs::write(&original, vec![0u8; 32]).unwrap();
        std::fs::hard_link(&original, second.join("alias.bin")).unwrap();

        let result =
            build_indexed_result(&[first, second], &CancellationToken::default(), |_| {}).unwrap();

        assert_eq!(result.summary().file_count, 1);
        assert_eq!(result.summary().logical_bytes, 32);
        assert_eq!(result.large_files(0, 0, 10).total, 1);
    }

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
