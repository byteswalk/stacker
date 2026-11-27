//! One disk's cleanup: step after step while the disk is still low, and within a step until it
//! has twice the threshold free, so it does not run low again with the next build.

use super::find::{self, Env};
use super::model::{Candidate, DiskSpace, ItemRecord, RunRecord, Step, StepRecord};
use super::remove::{self, Removed};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many items a step lists; the rest are counted.
const LISTED: usize = 100;

/// Where a cleanup gets what it works with; tests give their own.
pub trait Source {
    /// The disk's free bytes now.
    fn free(&self) -> u64;
    /// What this step offers on the disk, in the order it goes.
    fn candidates(&mut self, step: Step) -> Vec<Candidate>;
    fn remove(&self, candidate: &Candidate) -> Removed;
    /// Told when a step starts.
    fn progress(&self, _step: Step) {}
}

pub fn is_low(free: u64, total: u64, threshold_percent: u8) -> bool {
    total > 0 && u128::from(free) * 100 <= u128::from(total) * u128::from(threshold_percent)
}

/// Twice the threshold, but never more than the whole disk.
pub fn goal(total: u64, threshold_percent: u8) -> u64 {
    (u128::from(total) * u128::from(threshold_percent) * 2 / 100).min(u128::from(total)) as u64
}

pub fn run_disk(
    disk: &DiskSpace,
    threshold_percent: u8,
    manual: bool,
    now: u64,
    source: &mut dyn Source,
) -> RunRecord {
    let free_before = source.free();
    let goal = goal(disk.total, threshold_percent);
    let mut steps = Vec::new();
    for step in Step::ALL {
        // The next step only while the disk is still low.
        if step != Step::IdleBuild && !is_low(source.free(), disk.total, threshold_percent) {
            break;
        }
        let candidates = source.candidates(step);
        if candidates.is_empty() {
            continue;
        }
        source.progress(step);
        let mut record = StepRecord {
            step,
            freed: 0,
            items: Vec::new(),
            more: 0,
        };
        for candidate in candidates {
            if step.stops_at_goal() && source.free() >= goal {
                break;
            }
            let removed = source.remove(&candidate);
            record.freed += removed.bytes;
            if removed.bytes == 0 && removed.reason.is_none() {
                continue;
            }
            if record.items.len() < LISTED {
                record.items.push(ItemRecord {
                    path: candidate.path.to_string_lossy().into_owned(),
                    label: candidate.label.clone(),
                    freed: removed.bytes,
                    error: removed.reason.map(str::to_string),
                });
            } else {
                record.more += 1;
            }
        }
        if !record.items.is_empty() || record.more > 0 {
            steps.push(record);
        }
    }
    let free_after = source.free();
    RunRecord {
        drive: disk.root.clone(),
        at: now,
        finished_at: crate::sessions::now().max(now),
        total: disk.total,
        free_before,
        free_after,
        freed: steps.iter().map(|s| s.freed).sum(),
        manual,
        steps,
        notes: Vec::new(),
    }
}

/// The real machine: the projects on the disk are found and scanned once, the first time a
/// step asks for them; caches and temporary files are measured when their step comes.
pub struct System<'a> {
    pub disk: String,
    pub roots: Vec<String>,
    pub env: &'a Env,
    pub on_step: Box<dyn Fn(Step) + 'a>,
    projects: Option<HashMap<Step, Vec<Candidate>>>,
}

impl<'a> System<'a> {
    pub fn new(
        disk: String,
        roots: Vec<String>,
        env: &'a Env,
        on_step: Box<dyn Fn(Step) + 'a>,
    ) -> Self {
        Self {
            disk,
            roots,
            env,
            on_step,
            projects: None,
        }
    }

    fn project_candidates(&mut self) -> &mut HashMap<Step, Vec<Candidate>> {
        let disk = self.disk.clone();
        let roots = self.roots.clone();
        let env = self.env;
        self.projects.get_or_insert_with(|| {
            let walked = find::walk_projects(Path::new(&disk), &env.guard);
            let projects = find::projects_on(&disk, &roots, env, walked);
            let reports = find::scan_projects(&projects);
            let all: Vec<Candidate> = projects
                .iter()
                .zip(reports)
                .filter_map(|(project, report)| {
                    report.map(|report| find::project_candidates(project, &report, env))
                })
                .flatten()
                .collect();
            let mut by_step: HashMap<Step, Vec<Candidate>> = HashMap::new();
            for candidate in find::without_nested(all) {
                by_step.entry(candidate.step).or_default().push(candidate);
            }
            by_step
        })
    }
}

impl Source for System<'_> {
    fn free(&self) -> u64 {
        super::space_of(&self.disk).map_or(0, |(_, free)| free)
    }

    fn candidates(&mut self, step: Step) -> Vec<Candidate> {
        let mut found = self.project_candidates().remove(&step).unwrap_or_default();
        match step {
            Step::Caches => found.extend(
                find::known_cache_candidates(&self.disk, &self.roots, self.env)
                    .into_iter()
                    .filter(|c| c.step == Step::Caches),
            ),
            Step::System => {
                found.extend(
                    find::known_cache_candidates(&self.disk, &self.roots, self.env)
                        .into_iter()
                        .filter(|c| c.step == Step::System),
                );
                found.extend(find::system_candidates(&self.disk, &self.roots, self.env));
            }
            _ => {}
        }
        let mut found = find::without_nested(found);
        find::order(&mut found);
        found
    }

    fn remove(&self, candidate: &Candidate) -> Removed {
        let env = self.env;
        remove::remove(
            &candidate.path,
            candidate.removal,
            &|path: &Path, newest| find::temp_entry_may_go(path, newest, env),
        )
    }

    fn progress(&self, step: Step) {
        (self.on_step)(step);
    }
}

/// Where the system keeps its own temporary files, when it is on this disk.
pub fn system_temp() -> Option<PathBuf> {
    std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(|root| PathBuf::from(root).join("Temp"))
}
