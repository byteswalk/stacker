#![allow(dead_code)]

pub mod cleanup;
pub mod ledger;
pub mod measure;
pub mod model;
pub mod processes;
pub mod rules;

use std::sync::Arc;

/// Scans every agent root now and caches the result.
pub fn run_scan() -> Result<Arc<ledger::Scan>, String> {
    let (sessions, _, roots) = crate::sessions::commands::annotated_catalog()?;
    let ids = ledger::session_ids(&sessions);
    let running = processes::running_images();
    let scan = ledger::scan(
        &ledger::default_roots(&roots),
        &ids,
        &running,
        crate::sessions::now(),
    );
    Ok(ledger::store(scan))
}

pub fn scan_cached(refresh: bool) -> Result<Arc<ledger::Scan>, String> {
    match ledger::cached() {
        Some(scan) if !refresh => Ok(scan),
        _ => run_scan(),
    }
}
