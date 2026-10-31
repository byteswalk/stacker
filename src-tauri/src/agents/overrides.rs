//! Corrections to the install catalog that reach every copy without a new release. A vendor
//! moves a download or changes a silent switch more often than Stacker ships, so those facts
//! can be overridden by `resources/install-overrides.json` in the repository. It says what
//! gets run, so it counts only when signed with the release key; anything else is ignored.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

const SOURCES: [&str; 2] = [
    "https://raw.githubusercontent.com/byteswalk/stacker/main/resources/install-overrides.json",
    "https://gitee.com/shaxiong/stacker/raw/main/resources/install-overrides.json?download=1",
];
const STALE_AFTER_SECS: u64 = 60 * 60;

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct Overrides {
    /// Old address to new one, as text inside a URL or an install script's command.
    urls: HashMap<String, String>,
    /// Old WinGet package id to new one.
    winget: HashMap<String, String>,
    /// Silent arguments by product surface id, for a vendor that changed its installer.
    silent_args: HashMap<String, Vec<String>>,
}

impl Overrides {
    /// Only secure addresses: a correction must not be able to downgrade a download to HTTP.
    fn parsed(text: &str) -> Option<Overrides> {
        let mut found: Overrides = serde_json::from_str(text).ok()?;
        found.urls.retain(|from, to| {
            let keep = !from.is_empty() && to.starts_with("https://");
            if !keep {
                log::warn!(target: "stacker::overrides", "ignored a non-HTTPS override");
            }
            keep
        });
        Some(found)
    }

    fn url(&self, text: &str) -> String {
        self.urls
            .iter()
            .fold(text.to_string(), |text, (from, to)| text.replace(from, to))
    }
}

/// The overrides in `text`, when `signature` is the release key's over exactly those bytes.
fn verified(text: &str, signature: &str) -> Option<Overrides> {
    crate::update::verify_release_signature(text.as_bytes(), signature).ok()?;
    Overrides::parsed(text)
}

fn fetched(url: &str) -> Option<Overrides> {
    let text = crate::update::fetch(url).ok()?;
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let signature_url = match query {
        "" => format!("{base}.minisig"),
        query => format!("{base}.minisig?{query}"),
    };
    verified(&text, &crate::update::fetch(&signature_url).ok()?)
}

static CURRENT: RwLock<Option<Overrides>> = RwLock::new(None);
static CHECKED_AT: AtomicU64 = AtomicU64::new(0);
static CHECKING: AtomicBool = AtomicBool::new(false);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Looks for a newer file in the background: an install never waits on this, and whatever
/// was last read stays in force when the network is down.
pub(crate) fn refresh() {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        if let Some(found) = SOURCES.iter().find_map(|url| fetched(url)) {
            *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(found);
        }
        CHECKED_AT.store(now_secs(), Ordering::SeqCst);
        CHECKING.store(false, Ordering::SeqCst);
    });
}

fn with_current<T>(read: impl FnOnce(&Overrides) -> T, otherwise: T) -> T {
    if now_secs().saturating_sub(CHECKED_AT.load(Ordering::SeqCst)) > STALE_AFTER_SECS {
        refresh();
    }
    match CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(found) => read(found),
        None => otherwise,
    }
}

/// `text` (an address, or a script command holding one) with any moved address corrected.
pub(crate) fn url(text: &str) -> String {
    with_current(|found| found.url(text), text.to_string())
}

pub(crate) fn winget_id(id: &str) -> String {
    with_current(
        |found| found.winget.get(id).cloned().unwrap_or_else(|| id.into()),
        id.to_string(),
    )
}

pub(crate) fn silent_args(id: &str) -> Option<Vec<String>> {
    with_current(|found| found.silent_args.get(id).cloned(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_file_is_signed_by_the_key_built_into_the_app() {
        let text = include_str!("../../../resources/install-overrides.json");
        let signature = include_str!("../../../resources/install-overrides.json.minisig");
        // Git may hand these over with CRLF line ends; what is signed is the LF form.
        let text = text.replace("\r\n", "\n");
        let signature = signature.replace("\r\n", "\n");
        assert!(verified(&text, &signature).is_some());
    }

    #[test]
    fn a_changed_or_unsigned_file_counts_for_nothing() {
        let signature = include_str!("../../../resources/install-overrides.json.minisig");
        assert_eq!(
            verified(r#"{"urls":{"a":"https://evil.example/"}}"#, signature),
            None
        );
        assert_eq!(verified("{}", "not a signature"), None);
    }

    #[test]
    fn a_moved_address_is_corrected_inside_a_url_or_a_script_command() {
        let found = Overrides::parsed(
            r#"{"urls":{"https://old.example/install.ps1":"https://new.example/install.ps1"}}"#,
        )
        .unwrap();
        assert_eq!(
            found.url("irm https://old.example/install.ps1 | iex"),
            "irm https://new.example/install.ps1 | iex"
        );
        assert_eq!(
            found.url("irm https://other.example/x.ps1 | iex"),
            "irm https://other.example/x.ps1 | iex"
        );
    }

    #[test]
    fn a_correction_cannot_downgrade_to_http_and_an_unknown_field_does_no_harm() {
        let found = Overrides::parsed(
            r#"{"urls":{"https://a.example/":"http://a.example/"},"later":1,"winget":{"A.B":"C.D"}}"#,
        )
        .unwrap();
        assert!(found.urls.is_empty());
        assert_eq!(found.winget["A.B"], "C.D");
        assert_eq!(Overrides::parsed("not json"), None);
    }
}
