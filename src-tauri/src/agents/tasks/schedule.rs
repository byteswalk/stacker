use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Resource {
    /// A product id or shared CLI id; its CLI and desktop never change concurrently.
    Product(String),
    /// The single npm global directory.
    Npm,
    /// WinGet, MSI and vendor installers, which lock each other out.
    Installer,
    Download,
}

impl Resource {
    fn capacity(&self) -> usize {
        match self {
            Resource::Download => 3,
            _ => 1,
        }
    }
}

/// Indexes of queued tasks that can start now, in queue order. A task that does not fit
/// is skipped without blocking later tasks.
pub(crate) fn runnable(queued: &[Vec<Resource>], running: &[Vec<Resource>]) -> Vec<usize> {
    let mut used: HashMap<Resource, usize> = HashMap::new();
    for resource in running.iter().flatten() {
        *used.entry(resource.clone()).or_default() += 1;
    }
    let mut start = Vec::new();
    for (index, resources) in queued.iter().enumerate() {
        let fits = resources
            .iter()
            .all(|resource| used.get(resource).copied().unwrap_or(0) < resource.capacity());
        if fits {
            for resource in resources {
                *used.entry(resource.clone()).or_default() += 1;
            }
            start.push(index);
        }
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;
    use Resource::*;

    fn product(id: &str) -> Resource {
        Product(id.into())
    }

    #[test]
    fn npm_tasks_run_one_at_a_time() {
        let queued = vec![vec![product("a"), Npm], vec![product("b"), Npm]];
        assert_eq!(runnable(&queued, &[]), vec![0]);
    }

    #[test]
    fn unrelated_tasks_run_together() {
        let queued = vec![
            vec![product("a"), Npm],
            vec![product("b"), Installer, Download],
        ];
        assert_eq!(runnable(&queued, &[]), vec![0, 1]);
    }

    #[test]
    fn at_most_three_downloads() {
        let queued: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|id| vec![product(id), Download])
            .collect();
        assert_eq!(runnable(&queued, &[]), vec![0, 1, 2]);
    }

    #[test]
    fn a_blocked_task_does_not_block_later_ones() {
        let running = vec![vec![product("a"), Download]];
        let queued = vec![vec![product("a"), Download], vec![product("b"), Download]];
        assert_eq!(runnable(&queued, &running), vec![1]);
    }

    #[test]
    fn released_resources_wake_queued_tasks() {
        let queued = vec![vec![product("a"), Installer]];
        assert!(runnable(&queued, &[vec![product("x"), Installer]]).is_empty());
        assert_eq!(runnable(&queued, &[]), vec![0]);
    }
}
