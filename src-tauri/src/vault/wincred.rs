//! The vault's secrets, mirrored into Windows Credential Manager so that a program run by
//! the same Windows user (an AI agent, a script) can take one by name without the value ever
//! being pasted anywhere. The vault file stays the complete, backed-up copy; this is what
//! other programs read. Once a value is here the master password no longer guards it.

use super::model::{Body, Kind};
use std::collections::HashSet;
use zeroize::Zeroizing;

/// Every credential of ours starts with this, so the ones to remove can be told apart.
pub(crate) const PREFIX: &str = "Stacker:";
/// `CRED_MAX_CREDENTIAL_BLOB_SIZE`: what one generic credential holds, in bytes of UTF-16.
const MAX_BLOB_BYTES: usize = 5 * 512;

pub(crate) trait CredStore: Send + Sync {
    /// The names of the credentials under [`PREFIX`].
    fn list(&self) -> Vec<String>;
    fn read(&self, target: &str) -> Option<Zeroizing<String>>;
    fn write(&self, target: &str, value: &str) -> bool;
    fn delete(&self, target: &str);
}

/// One secret field of one entry and the name it goes by in Credential Manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Wanted {
    pub entry_id: String,
    pub field: String,
    pub target: String,
}

/// A name part that can sit inside a quoted command line: letters and digits of any
/// script, and a few plain marks; everything else becomes a dash.
fn part(text: &str) -> String {
    let cleaned: String = text
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " ._-@/+()".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "-".into()
    } else {
        cleaned
    }
}

/// What should be in Credential Manager for this vault: every filled secret field of every
/// live entry, except SSH keys (ssh reads those from `~/.ssh`, and an RSA key does not fit).
/// The name is `Stacker:<title>:<field>`; a second entry with the same title and field gets
/// its id appended, the older one keeping the plain name.
pub(crate) fn wanted(body: &Body) -> Vec<(Wanted, &str)> {
    let mut entries: Vec<_> = body
        .entries
        .iter()
        .filter(|entry| entry.deleted_at.is_none() && entry.kind != Kind::SshKey)
        .collect();
    entries.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    let mut taken = HashSet::new();
    let mut out = Vec::new();
    for entry in entries {
        for field in &entry.fields {
            if !field.secret
                || field.value.is_empty()
                || field.value.encode_utf16().count() * 2 > MAX_BLOB_BYTES
            {
                continue;
            }
            let mut target = format!("{PREFIX}{}:{}", part(&entry.title), part(&field.name));
            if !taken.insert(target.to_lowercase()) {
                let short: String = entry.id.chars().take(6).collect();
                target = format!("{target}#{short}");
                taken.insert(target.to_lowercase());
            }
            out.push((
                Wanted {
                    entry_id: entry.id.clone(),
                    field: field.name.clone(),
                    target,
                },
                field.value.as_str(),
            ));
        }
    }
    out
}

/// Makes Credential Manager agree with the vault: writes what is missing or different,
/// removes what the vault no longer has (deleted, renamed, moved to the trash).
pub(crate) fn sync(store: &dyn CredStore, body: &Body) {
    let wanted = wanted(body);
    let keep: HashSet<String> = wanted
        .iter()
        .map(|(item, _)| item.target.to_lowercase())
        .collect();
    for target in store.list() {
        if !keep.contains(&target.to_lowercase()) {
            store.delete(&target);
        }
    }
    for (item, value) in wanted {
        if store.read(&item.target).as_deref().map(String::as_str) != Some(value) {
            store.write(&item.target, value);
        }
    }
}

/// The names for one entry's fields, for the page and for the text handed to an AI.
pub(crate) fn targets_of(body: &Body, id: &str) -> Vec<Wanted> {
    wanted(body)
        .into_iter()
        .map(|(item, _)| item)
        .filter(|item| item.entry_id == id)
        .collect()
}

pub(crate) use platform::System as SystemStore;

/// A credential someone else saved: its name, the account in it, and the secret.
pub(crate) struct Saved {
    pub target: String,
    pub user: String,
    pub secret: Zeroizing<String>,
}

/// The credentials Git (through Git Credential Manager) keeps for each server, named
/// `git:https://github.com` and the like. Read-only.
pub(crate) fn git_saved() -> Vec<Saved> {
    platform::saved("git:*")
}

#[cfg(windows)]
mod platform {
    use super::{CredStore, PREFIX};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredEnumerateW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };
    use zeroize::{Zeroize, Zeroizing};

    pub(crate) struct System;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// SAFETY: `text` must be a NUL-terminated UTF-16 string that stays valid for the call.
    unsafe fn from_wide(text: *const u16) -> String {
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
    }

    /// Every generic credential whose name matches `filter`, with its account and secret.
    pub(super) fn saved(filter: &str) -> Vec<super::Saved> {
        let filter = wide(filter);
        let mut count = 0u32;
        let mut found: *mut *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: as in `list`; each blob is `CredentialBlobSize` bytes, read before CredFree.
        unsafe {
            if CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut found) == 0 {
                return Vec::new();
            }
            let out = std::slice::from_raw_parts(found, count as usize)
                .iter()
                .filter(|item| (***item).Type == CRED_TYPE_GENERIC)
                .map(|item| {
                    let credential = &**item;
                    let bytes = std::slice::from_raw_parts(
                        credential.CredentialBlob,
                        credential.CredentialBlobSize as usize,
                    );
                    let mut units: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    let secret = Zeroizing::new(String::from_utf16_lossy(&units));
                    units.zeroize();
                    super::Saved {
                        target: from_wide(credential.TargetName),
                        user: if credential.UserName.is_null() {
                            String::new()
                        } else {
                            from_wide(credential.UserName)
                        },
                        secret,
                    }
                })
                .collect();
            CredFree(found as _);
            out
        }
    }

    impl CredStore for System {
        fn list(&self) -> Vec<String> {
            let filter = wide(&format!("{PREFIX}*"));
            let mut count = 0u32;
            let mut found: *mut *mut CREDENTIALW = std::ptr::null_mut();
            // SAFETY: on success `found` is an array of `count` credential pointers that
            // stays valid until CredFree; each TargetName is a NUL-terminated string.
            unsafe {
                if CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut found) == 0 {
                    return Vec::new();
                }
                let names = std::slice::from_raw_parts(found, count as usize)
                    .iter()
                    .filter(|item| (***item).Type == CRED_TYPE_GENERIC)
                    .map(|item| from_wide((**item).TargetName))
                    .collect();
                CredFree(found as _);
                names
            }
        }

        fn read(&self, target: &str) -> Option<Zeroizing<String>> {
            let name = wide(target);
            let mut found: *mut CREDENTIALW = std::ptr::null_mut();
            // SAFETY: on success `found` points at one credential, valid until CredFree,
            // whose blob is `CredentialBlobSize` bytes of UTF-16.
            unsafe {
                if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) == 0 {
                    return None;
                }
                let bytes = std::slice::from_raw_parts(
                    (*found).CredentialBlob,
                    (*found).CredentialBlobSize as usize,
                );
                let mut units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                let text = Zeroizing::new(String::from_utf16_lossy(&units));
                units.zeroize();
                CredFree(found as _);
                Some(text)
            }
        }

        fn write(&self, target: &str, value: &str) -> bool {
            let mut name = wide(target);
            let mut user = wide("Stacker");
            let mut blob: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
            // SAFETY: CREDENTIALW is plain data for which all-zero is a valid value; every
            // pointer put into it outlives the call.
            let ok = unsafe {
                let mut credential: CREDENTIALW = std::mem::zeroed();
                credential.Type = CRED_TYPE_GENERIC;
                credential.TargetName = name.as_mut_ptr();
                credential.UserName = user.as_mut_ptr();
                credential.CredentialBlobSize = blob.len() as u32;
                credential.CredentialBlob = blob.as_mut_ptr();
                credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
                CredWriteW(&credential, 0) != 0
            };
            blob.zeroize();
            ok
        }

        fn delete(&self, target: &str) {
            let name = wide(target);
            // SAFETY: `name` is a NUL-terminated string valid for the call.
            unsafe {
                CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::CredStore;
    use zeroize::Zeroizing;

    pub(crate) struct System;

    pub(super) fn saved(_: &str) -> Vec<super::Saved> {
        Vec::new()
    }

    impl CredStore for System {
        fn list(&self) -> Vec<String> {
            Vec::new()
        }
        fn read(&self, _: &str) -> Option<Zeroizing<String>> {
            None
        }
        fn write(&self, _: &str, _: &str) -> bool {
            false
        }
        fn delete(&self, _: &str) {}
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::vault::model::{Entry, Field};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Credential Manager in memory: tests never touch the real one.
    #[derive(Default)]
    pub(crate) struct Memory(pub Mutex<BTreeMap<String, String>>);

    impl CredStore for Memory {
        fn list(&self) -> Vec<String> {
            self.0.lock().unwrap().keys().cloned().collect()
        }
        fn read(&self, target: &str) -> Option<Zeroizing<String>> {
            self.0
                .lock()
                .unwrap()
                .get(target)
                .cloned()
                .map(Zeroizing::new)
        }
        fn write(&self, target: &str, value: &str) -> bool {
            self.0.lock().unwrap().insert(target.into(), value.into());
            true
        }
        fn delete(&self, target: &str) {
            self.0.lock().unwrap().remove(target);
        }
    }

    fn entry(
        id: &str,
        title: &str,
        kind: Kind,
        created_at: i64,
        fields: &[(&str, &str, bool)],
    ) -> Entry {
        Entry {
            id: id.into(),
            title: title.into(),
            platform: String::new(),
            kind,
            fields: fields
                .iter()
                .map(|(name, value, secret)| Field {
                    name: (*name).into(),
                    value: (*value).into(),
                    secret: *secret,
                })
                .collect(),
            expires_at: None,
            tags: Vec::new(),
            note: String::new(),
            favorite: false,
            created_at,
            updated_at: created_at,
            deleted_at: None,
            history: Vec::new(),
        }
    }

    fn names(store: &Memory) -> Vec<(String, String)> {
        store.0.lock().unwrap().clone().into_iter().collect()
    }

    #[test]
    fn only_filled_secrets_of_live_entries_that_are_not_ssh_keys_are_mirrored() {
        let mut gone = entry("e3", "Old", Kind::Token, 3, &[("Token", "t-old", true)]);
        gone.deleted_at = Some(9);
        let body = Body {
            entries: vec![
                entry(
                    "e1",
                    "HOSTINGER_API_TOKEN",
                    Kind::Token,
                    1,
                    &[
                        ("Token", "t-1", true),
                        ("账号", "me", false),
                        ("Extra", "", true),
                    ],
                ),
                entry(
                    "e2",
                    "vps",
                    Kind::SshKey,
                    2,
                    &[("私钥", "-----BEGIN OPENSSH PRIVATE KEY-----", true)],
                ),
                gone,
                entry(
                    "e4",
                    "Big",
                    Kind::Other,
                    4,
                    &[("Blob", &"x".repeat(MAX_BLOB_BYTES), true)],
                ),
            ],
            ignored: Vec::new(),
        };
        let store = Memory::default();
        sync(&store, &body);
        assert_eq!(
            names(&store),
            vec![(
                "Stacker:HOSTINGER_API_TOKEN:Token".to_string(),
                "t-1".to_string()
            )]
        );
        assert_eq!(
            targets_of(&body, "e1")[0].target,
            "Stacker:HOSTINGER_API_TOKEN:Token"
        );
        assert!(targets_of(&body, "e2").is_empty());
    }

    #[test]
    fn a_change_in_the_vault_replaces_renames_and_removes() {
        let store = Memory::default();
        // Something else's credential and a stale one of ours.
        store.write("Stacker:Gone:Key", "stale");
        let mut body = Body {
            entries: vec![entry("e1", "Ark", Kind::ApiKey, 1, &[("Key", "k-1", true)])],
            ignored: Vec::new(),
        };
        sync(&store, &body);
        assert_eq!(
            names(&store),
            vec![("Stacker:Ark:Key".to_string(), "k-1".to_string())]
        );

        body.entries[0].fields[0].value = "k-2".into();
        body.entries[0].title = "Ark: prod 'x'".into();
        sync(&store, &body);
        assert_eq!(
            names(&store),
            vec![("Stacker:Ark- prod -x-:Key".to_string(), "k-2".to_string())]
        );

        body.entries[0].deleted_at = Some(5);
        sync(&store, &body);
        assert!(names(&store).is_empty());
    }

    #[test]
    fn two_entries_with_one_title_keep_apart_and_the_older_keeps_the_plain_name() {
        let body = Body {
            entries: vec![
                entry(
                    "bbbbbbbb-2",
                    "GitHub",
                    Kind::Token,
                    2,
                    &[("Token", "new", true)],
                ),
                entry(
                    "aaaaaaaa-1",
                    "github",
                    Kind::Token,
                    1,
                    &[("Token", "old", true)],
                ),
            ],
            ignored: Vec::new(),
        };
        let store = Memory::default();
        sync(&store, &body);
        assert_eq!(
            names(&store),
            vec![
                ("Stacker:GitHub:Token#bbbbbb".to_string(), "new".to_string()),
                ("Stacker:github:Token".to_string(), "old".to_string()),
            ]
        );
    }

    #[test]
    #[ignore = "reads the real Windows Credential Manager; run by hand with --ignored"]
    fn git_saved_reads_what_git_keeps() {
        // Names and lengths only: the secrets themselves are never printed.
        for item in git_saved() {
            eprintln!(
                "{} user={} secret_len={}",
                item.target,
                !item.user.is_empty(),
                item.secret.len()
            );
        }
    }

    #[test]
    #[ignore = "touches the real Windows Credential Manager; run by hand with --ignored"]
    fn the_real_store_writes_lists_reads_and_deletes() {
        let store = SystemStore;
        let target = "Stacker:__test__:Token";
        assert!(store.write(target, "值-value-1"));
        assert!(store.list().iter().any(|name| name == target));
        assert_eq!(store.read(target).unwrap().as_str(), "值-value-1");
        store.delete(target);
        assert!(store.read(target).is_none());
    }
}
