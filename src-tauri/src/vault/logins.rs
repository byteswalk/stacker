//! Website logins between the browser extension and the vault, without the two talking to
//! each other directly: the extension's host process (started by the browser, with no access
//! to the unlocked vault) works only through what the user's own Windows account protects.
//!
//! - Saving: a login the user chose to keep goes into Windows Credential Manager as an inbox
//!   item (`StackerInbox:<host>:<n>`); the vault takes it in at the next unlock, or while it
//!   is open, and removes it from there.
//! - Filling: only entries the user put into Windows credentials can be filled. Which sites
//!   and accounts they are is kept in a small index encrypted with DPAPI for this Windows
//!   user; the password itself is read from Credential Manager, by the name the index gives.

use super::browser::{PASSWORD_FIELD, URL_FIELD, USER_FIELD};
use super::model::{Body, EntryInput, FieldInput, Kind};
use super::wincred::{self, CredStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub(crate) const INBOX_PREFIX: &str = "StackerInbox:";

/// A login the extension saw submitted and the user chose to keep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Captured {
    pub url: String,
    pub user: String,
    pub password: String,
    #[serde(default)]
    pub title: String,
    /// Also put it into Windows credentials, so it can be filled later.
    #[serde(default)]
    pub fill: bool,
}

/// One fillable login, as the index keeps it: no password, only where to read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Fillable {
    pub host: String,
    pub user: String,
    pub title: String,
    pub target: String,
}

/// The site a URL belongs to, the way logins are matched: its host, without `www.`.
pub(crate) fn host_of(url: &str) -> String {
    url::Url::parse(url.trim())
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_ascii_lowercase())
        })
        .unwrap_or_default()
}

/// A page on `page` may use a login saved for `saved`: the same site, or a subdomain of it.
/// Never the other way: a login for `mail.example.com` is not offered on `example.com`.
fn same_site(page: &str, saved: &str) -> bool {
    !saved.is_empty() && (page == saved || page.ends_with(&format!(".{saved}")))
}

fn value<'a>(entry: &'a super::model::Entry, name: &str) -> &'a str {
    entry
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
        .unwrap_or_default()
}

/// The logins that can be filled: entries in Windows credentials that name a website.
pub(crate) fn fillable(body: &Body) -> Vec<Fillable> {
    let mirrored = wincred::wanted(body);
    body.entries
        .iter()
        .filter(|entry| {
            entry.deleted_at.is_none() && entry.windows == Some(true) && entry.kind != Kind::SshKey
        })
        .filter_map(|entry| {
            let host = host_of(value(entry, URL_FIELD));
            if host.is_empty() {
                return None;
            }
            let secret = entry
                .fields
                .iter()
                .filter(|field| field.secret && !field.value.is_empty())
                .find(|field| field.name == PASSWORD_FIELD)
                .or_else(|| {
                    entry
                        .fields
                        .iter()
                        .find(|field| field.secret && !field.value.is_empty())
                })?;
            let target = mirrored
                .iter()
                .find(|(item, _)| item.entry_id == entry.id && item.field == secret.name)?
                .0
                .target
                .clone();
            Some(Fillable {
                host,
                user: value(entry, USER_FIELD).to_string(),
                title: entry.title.clone(),
                target,
            })
        })
        .collect()
}

/// Where the index lives: beside the vault.
pub(crate) fn index_path(vault_path: &Path) -> PathBuf {
    vault_path.with_file_name("vault-logins.bin")
}

/// Writes the index for this body, or removes it when nothing can be filled.
pub(crate) fn write_index(path: &Path, body: &Body) {
    let list = fillable(body);
    if list.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Ok(json) = serde_json::to_string(&list) else {
        return;
    };
    if let Ok(sealed) = crate::dpapi::encrypt(&json) {
        let _ = std::fs::write(path, sealed);
    }
}

pub(crate) fn read_index(path: &Path) -> Vec<Fillable> {
    std::fs::read(path)
        .ok()
        .and_then(|sealed| crate::dpapi::decrypt(&sealed).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// The logins for a page, best match first: the exact site before its parent domains.
pub(crate) fn matches(index: &[Fillable], page_url: &str) -> Vec<Fillable> {
    let page = host_of(page_url);
    let mut found: Vec<Fillable> = index
        .iter()
        .filter(|item| same_site(&page, &item.host))
        .cloned()
        .collect();
    found.sort_by_key(|item| std::cmp::Reverse(item.host.len()));
    found
}

/// The password of one login for a page, read from Credential Manager.
pub(crate) fn password_for(
    store: &dyn CredStore,
    index: &[Fillable],
    page_url: &str,
    user: &str,
) -> Option<Zeroizing<String>> {
    let found = matches(index, page_url);
    let item = found.iter().find(|item| item.user == user)?;
    store.read(&item.target)
}

/// Keeps a login for the vault to take in. Refused when it would not fit one credential.
pub(crate) fn put_inbox(store: &dyn CredStore, captured: &Captured) -> Result<(), String> {
    let host = host_of(&captured.url);
    if host.is_empty() || captured.password.is_empty() {
        return Err("E_REQUEST".into());
    }
    let json =
        Zeroizing::new(serde_json::to_string(captured).map_err(|_| "E_REQUEST".to_string())?);
    let n = super::session::now_ms();
    let target = format!("{INBOX_PREFIX}{host}:{n}");
    if store.write(&target, &json) {
        Ok(())
    } else {
        Err("E_TOO_LARGE".into())
    }
}

/// What the inbox holds, with the name each item has there.
pub(crate) fn inbox(store: &dyn CredStore) -> Vec<(String, Captured)> {
    store
        .list_prefix(INBOX_PREFIX)
        .into_iter()
        .filter_map(|target| {
            let text = store.read(&target)?;
            let captured: Captured = serde_json::from_str(&text).ok()?;
            Some((target, captured))
        })
        .collect()
}

/// Applies the changes to the body: new entries, new passwords (the old one kept in the
/// entry's history), and the fill choice (only ever turned on here).
pub(crate) fn apply(body: &mut Body, changes: Vec<Change>, now: i64) -> Result<(), String> {
    for change in changes {
        match change {
            Change::Add(input, fill) => {
                let id = super::model::apply_input(body, input, now)?;
                if let Some(entry) = body.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.windows = Some(fill);
                }
            }
            Change::Password(id, password, fill) => {
                let Some(entry) = body.entries.iter_mut().find(|entry| entry.id == id) else {
                    continue;
                };
                if let Some(field) = entry
                    .fields
                    .iter_mut()
                    .find(|field| field.name == PASSWORD_FIELD)
                {
                    let old = std::mem::replace(&mut field.value, password);
                    entry.history.push(super::model::HistoryItem {
                        field: PASSWORD_FIELD.into(),
                        value: old,
                        at: now,
                    });
                    super::model::trim_history(&mut entry.history);
                }
                entry.updated_at = now;
                if fill {
                    entry.windows = Some(true);
                }
            }
            Change::Same(id, fill) => {
                if let (true, Some(entry)) =
                    (fill, body.entries.iter_mut().find(|entry| entry.id == id))
                {
                    entry.windows = Some(true);
                }
            }
        }
    }
    Ok(())
}

/// What taking one login in does to the vault.
#[derive(Debug, PartialEq)]
pub(crate) enum Change {
    /// A new entry.
    Add(EntryInput, bool),
    /// The entry with this id gets the new password (the old one goes to its history).
    Password(String, String, bool),
    /// Already there with this password; only the fill choice may change.
    Same(String, bool),
}

/// Decides, for each login, whether it is new, a changed password, or already kept: the
/// same site and account is the same login.
pub(crate) fn changes(body: &Body, items: &[Captured]) -> Vec<Change> {
    items
        .iter()
        .map(|captured| {
            let host = host_of(&captured.url);
            let existing = body.entries.iter().find(|entry| {
                entry.deleted_at.is_none()
                    && entry.kind != Kind::SshKey
                    && host_of(value(entry, URL_FIELD)) == host
                    && value(entry, USER_FIELD) == captured.user
            });
            match existing {
                Some(entry) if value(entry, PASSWORD_FIELD) == captured.password => {
                    Change::Same(entry.id.clone(), captured.fill)
                }
                Some(entry) => {
                    Change::Password(entry.id.clone(), captured.password.clone(), captured.fill)
                }
                None => {
                    let mut fields = vec![FieldInput {
                        name: URL_FIELD.into(),
                        previous_name: None,
                        value: Some(captured.url.clone()),
                        secret: false,
                    }];
                    if !captured.user.is_empty() {
                        fields.push(FieldInput {
                            name: USER_FIELD.into(),
                            previous_name: None,
                            value: Some(captured.user.clone()),
                            secret: false,
                        });
                    }
                    fields.push(FieldInput {
                        name: PASSWORD_FIELD.into(),
                        previous_name: None,
                        value: Some(captured.password.clone()),
                        secret: true,
                    });
                    let title = if captured.title.trim().is_empty() {
                        host.clone()
                    } else {
                        captured.title.trim().to_string()
                    };
                    Change::Add(
                        EntryInput {
                            id: None,
                            title,
                            platform: host,
                            kind: Kind::Other,
                            fields,
                            expires_at: None,
                            tags: vec!["浏览器".into()],
                            note: String::new(),
                            favorite: false,
                        },
                        captured.fill,
                    )
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::wincred::tests::Memory;

    fn captured(url: &str, user: &str, password: &str, fill: bool) -> Captured {
        Captured {
            url: url.into(),
            user: user.into(),
            password: password.into(),
            title: String::new(),
            fill,
        }
    }

    fn take_in(body: &mut Body, items: &[Captured]) {
        let found = changes(body, items);
        apply(body, found, 1).unwrap();
    }

    #[test]
    fn a_login_is_new_then_the_same_then_a_changed_password() {
        let mut body = Body::default();
        take_in(
            &mut body,
            &[captured("https://www.github.com/login", "me", "p1", false)],
        );
        assert_eq!(body.entries.len(), 1);
        assert_eq!(body.entries[0].platform, "github.com");
        assert_eq!(body.entries[0].windows, Some(false));
        let id = body.entries[0].id.clone();
        assert_eq!(
            changes(
                &body,
                &[captured("https://github.com/session", "me", "p1", true)]
            ),
            vec![Change::Same(id.clone(), true)]
        );
        assert_eq!(
            changes(&body, &[captured("https://github.com/", "me", "p2", false)]),
            vec![Change::Password(id.clone(), "p2".into(), false)]
        );
        take_in(
            &mut body,
            &[captured("https://github.com/", "me", "p2", true)],
        );
        let entry = &body.entries[0];
        assert_eq!(
            entry
                .fields
                .iter()
                .find(|f| f.name == PASSWORD_FIELD)
                .unwrap()
                .value,
            "p2"
        );
        assert_eq!(
            (entry.history.len(), entry.history[0].value.as_str()),
            (1, "p1"),
            "the old password goes to the history"
        );
        assert_eq!(entry.windows, Some(true));
        // Another account on the same site is another login.
        assert!(matches!(
            changes(
                &body,
                &[captured("https://github.com/", "you", "p1", false)]
            )[0],
            Change::Add(..)
        ));
    }

    #[test]
    fn only_logins_in_windows_credentials_can_be_filled_and_only_on_their_site() {
        let mut body = Body::default();
        take_in(
            &mut body,
            &[
                captured("https://github.com/login", "me", "gh", true),
                captured("https://example.com/", "ann", "ex", false),
            ],
        );
        let store = Memory::default();
        wincred::sync(&store, &body);
        let index = fillable(&body);
        assert_eq!(index.len(), 1);
        assert_eq!(
            (index[0].host.as_str(), index[0].user.as_str()),
            ("github.com", "me")
        );

        assert_eq!(
            matches(&index, "https://gist.github.com/x").len(),
            1,
            "a subdomain uses its site's login"
        );
        assert!(matches(&index, "https://evilgithub.com/").is_empty());
        assert!(
            matches(&index, "https://example.com/").is_empty(),
            "not in Windows credentials"
        );
        assert_eq!(
            password_for(&store, &index, "https://github.com/login", "me")
                .unwrap()
                .as_str(),
            "gh"
        );
        assert!(password_for(&store, &index, "https://github.com/login", "you").is_none());
    }

    #[test]
    fn the_index_is_written_sealed_and_removed_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = index_path(&dir.path().join("vault.skv"));
        let mut body = Body::default();
        take_in(
            &mut body,
            &[captured("https://github.com/login", "me", "gh", true)],
        );
        write_index(&path, &body);
        let raw = std::fs::read(&path).unwrap();
        if cfg!(windows) {
            assert!(
                !String::from_utf8_lossy(&raw).contains("github.com"),
                "the index is not plain text"
            );
        }
        assert_eq!(read_index(&path).len(), 1);
        body.entries[0].windows = Some(false);
        write_index(&path, &body);
        assert!(!path.exists());
    }

    #[test]
    fn the_inbox_keeps_a_login_until_it_is_taken() {
        let store = Memory::default();
        put_inbox(
            &store,
            &captured("https://github.com/login", "me", "gh", true),
        )
        .unwrap();
        assert!(put_inbox(&store, &captured("not a url", "me", "x", false)).is_err());
        let items = inbox(&store);
        assert_eq!(items.len(), 1);
        assert!(items[0].0.starts_with("StackerInbox:github.com:"));
        assert_eq!(items[0].1.password, "gh");
        // The vault's own mirror never touches the inbox.
        wincred::sync(&store, &Body::default());
        assert_eq!(inbox(&store).len(), 1);
    }
}
