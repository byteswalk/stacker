//! How far WinGet's download has got when WinGet itself says nothing. WinGet hands large
//! installers to Windows Delivery Optimization, a system service, and draws no progress for
//! them in a console; the service still knows how many bytes it has. The URL comes from
//! WinGet's own log, so only this download is followed, never a Windows Update one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const MARK: &str = "DeliveryOptimization downloading from url: ";
const FIRST_LOOK: Duration = Duration::from_secs(10);
const EVERY: Duration = Duration::from_secs(10);

/// WinGet's diagnostic logs.
fn log_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local).join(
            "Packages\\Microsoft.DesktopAppInstaller_8wekyb3d8bbwe\\LocalState\\DiagOutputDir",
        ),
    )
}

/// The URL a WinGet log says Delivery Optimization is fetching, if it says so.
pub(crate) fn delivery_url(log: &str) -> Option<String> {
    log.lines()
        .rev()
        .find_map(|line| line.split_once(MARK).map(|(_, url)| url.trim().to_string()))
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
}

/// The newest WinGet log written since `since` that names a Delivery Optimization download.
fn current_url(dir: &Path, since: SystemTime) -> Option<String> {
    let mut logs: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("WinGet-"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .filter(|(modified, _)| *modified >= since)
        .collect();
    logs.sort_by_key(|item| std::cmp::Reverse(item.0));
    logs.into_iter()
        .find_map(|(_, path)| delivery_url(&std::fs::read_to_string(path).ok()?))
}

/// `done total` as the query below prints it, for a job still downloading.
pub(crate) fn parse_progress(text: &str) -> Option<(u64, u64)> {
    let mut words = text.split_whitespace();
    let done = words.next()?.parse().ok()?;
    let total: u64 = words.next()?.parse().ok()?;
    (total > 0).then_some((done, total))
}

fn query(url: &str) -> Option<(u64, u64)> {
    let script = format!(
        "Get-DeliveryOptimizationStatus | Where-Object {{ $_.SourceURL -eq {} -and $_.Status -eq 'Downloading' }} | Select-Object -First 1 | ForEach-Object {{ \"$($_.BytesFromHttp + $_.BytesFromPeers + $_.BytesFromCacheServer) $($_.FileSize)\" }}",
        crate::agents::process::ps_single_quoted(url)
    );
    let text = crate::agents::process::run_powershell(
        &["-NoProfile", "-NonInteractive", "-Command", &script],
        "Delivery Optimization",
        Duration::from_secs(15),
    )
    .ok()?;
    parse_progress(&text)
}

/// Follows WinGet's download until `stop` is set, sending `(done, total)` bytes as they change.
pub(crate) fn watch(started: SystemTime, stop: Arc<AtomicBool>, sender: Sender<(u64, u64)>) {
    let Some(dir) = log_dir() else {
        return;
    };
    std::thread::spawn(move || {
        let wait = |duration: Duration| {
            let until = std::time::Instant::now() + duration;
            while std::time::Instant::now() < until {
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            true
        };
        if !wait(FIRST_LOOK) {
            return;
        }
        let mut url = None;
        let mut last = None;
        loop {
            if url.is_none() {
                url = current_url(&dir, started);
            }
            if let Some(progress) = url.as_deref().and_then(query) {
                if last != Some(progress) {
                    last = Some(progress);
                    if sender.send(progress).is_err() {
                        return;
                    }
                }
            }
            if !wait(EVERY) {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_download_url_is_read_from_wingets_log() {
        let log = "2026-10-06 08:33:20.224 <I> [CORE] Downloading to path: C:\\x\n\
            2026-10-06 08:33:20.225 <I> [CORE] DeliveryOptimization downloading from url: https://prod.download.desktop.kiro.dev/a/kiro.exe\n";
        assert_eq!(
            delivery_url(log).as_deref(),
            Some("https://prod.download.desktop.kiro.dev/a/kiro.exe")
        );
        assert_eq!(
            delivery_url("2026 <I> [CORE] WinINet downloading from url: https://x/y"),
            None
        );
    }

    #[test]
    fn progress_needs_a_size() {
        assert_eq!(
            parse_progress("13631488 209592048\r\n"),
            Some((13631488, 209592048))
        );
        assert_eq!(parse_progress(""), None);
        assert_eq!(parse_progress("5 0"), None);
    }
}
