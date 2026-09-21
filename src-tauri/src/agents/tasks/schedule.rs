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

    /// What a queued task is shown to be waiting for.
    pub(crate) fn waiting_text(&self) -> &'static str {
        match self {
            Resource::Product(_) => "等同一产品的另一个任务完成",
            Resource::Npm => "等其他 npm 任务完成",
            Resource::Installer => "等其他安装程序完成",
            Resource::Download => "等下载名额",
        }
    }
}

/// For each queued task, in queue order: `None` when it can start now, otherwise the first
/// resource it is waiting for. A task that does not fit is skipped without blocking later tasks.
pub(crate) fn assign(queued: &[Vec<Resource>], running: &[Vec<Resource>]) -> Vec<Option<Resource>> {
    let mut used: HashMap<Resource, usize> = HashMap::new();
    for resource in running.iter().flatten() {
        *used.entry(resource.clone()).or_default() += 1;
    }
    queued
        .iter()
        .map(|resources| {
            let blocker = resources
                .iter()
                .find(|resource| used.get(*resource).copied().unwrap_or(0) >= resource.capacity());
            if blocker.is_none() {
                for resource in resources {
                    *used.entry(resource.clone()).or_default() += 1;
                }
            }
            blocker.cloned()
        })
        .collect()
}

/// Indexes of queued tasks that can start now, in queue order.
#[cfg(test)]
pub(crate) fn runnable(queued: &[Vec<Resource>], running: &[Vec<Resource>]) -> Vec<usize> {
    assign(queued, running)
        .iter()
        .enumerate()
        .filter(|(_, blocker)| blocker.is_none())
        .map(|(index, _)| index)
        .collect()
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
    fn a_waiting_task_names_what_it_waits_for() {
        let running = vec![vec![product("pi"), Npm]];
        let queued = vec![
            vec![product("kimi"), Npm],
            vec![product("pi"), Npm],
            vec![product("zed"), Installer, Download],
        ];
        assert_eq!(
            assign(&queued, &running),
            vec![Some(Npm), Some(product("pi")), None]
        );
    }

    #[test]
    fn released_resources_wake_queued_tasks() {
        let queued = vec![vec![product("a"), Installer]];
        assert!(runnable(&queued, &[vec![product("x"), Installer]]).is_empty());
        assert_eq!(runnable(&queued, &[]), vec![0]);
    }
}
