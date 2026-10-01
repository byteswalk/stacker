//! 条目模型：编辑、保密字段历史、回收站、视图、导入合并。不含 IO。

use super::crypto;
use super::errors::{INVALID, NOT_FOUND};
use super::ssh::{self, SshInfo};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use zeroize::{Zeroize, Zeroizing};

pub(crate) const HISTORY_PER_FIELD: usize = 10;
pub(crate) const TRASH_DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    ApiKey,
    Token,
    TokenPlan,
    AkSk,
    SshKey,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Zeroize)]
pub(crate) struct Field {
    pub name: String,
    pub value: String,
    pub secret: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Zeroize)]
pub(crate) struct HistoryItem {
    pub field: String,
    pub value: String,
    pub at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Zeroize)]
pub(crate) struct Entry {
    pub id: String,
    pub title: String,
    pub platform: String,
    #[zeroize(skip)]
    pub kind: Kind,
    pub fields: Vec<Field>,
    pub expires_at: Option<String>,
    pub tags: Vec<String>,
    pub note: String,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    #[serde(default)]
    pub history: Vec<HistoryItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Zeroize)]
pub(crate) struct Body {
    pub entries: Vec<Entry>,
    /// Digests of discovered secrets the user chose to ignore.
    #[serde(default)]
    pub ignored: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FieldInput {
    pub name: String,
    pub previous_name: Option<String>,
    /// None keeps the value the field had (under `previous_name` when renamed).
    pub value: Option<String>,
    pub secret: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryInput {
    pub id: Option<String>,
    pub title: String,
    pub platform: String,
    pub kind: Kind,
    pub fields: Vec<FieldInput>,
    pub expires_at: Option<String>,
    pub tags: Vec<String>,
    pub note: String,
    pub favorite: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FieldView {
    pub name: String,
    pub secret: bool,
    /// Only non-secret values travel to the page.
    pub value: Option<String>,
    pub filled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryView {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub kind: Kind,
    pub fields: Vec<FieldView>,
    pub expires_at: Option<String>,
    pub tags: Vec<String>,
    pub note: String,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    pub history_count: usize,
    pub ssh: Option<SshInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryView {
    pub index: usize,
    pub field: String,
    pub at: i64,
}

fn new_id() -> String {
    crypto::to_hex(&crypto::random::<16>())
}

fn normalize_date(value: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok().map(|date| date.format("%Y-%m-%d").to_string())
}

fn clean_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_string();
        if !tag.is_empty() && !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

/// Newest first, at most HISTORY_PER_FIELD per field name; dropped items are wiped.
pub(crate) fn trim_history(history: &mut Vec<HistoryItem>) {
    history.sort_by(|a, b| b.at.cmp(&a.at));
    let mut kept: HashMap<String, usize> = HashMap::new();
    history.retain_mut(|item| {
        let count = kept.entry(item.field.clone()).or_insert(0);
        *count += 1;
        let keep = *count <= HISTORY_PER_FIELD;
        if !keep {
            item.zeroize();
        }
        keep
    });
}

pub(crate) fn apply_input(body: &mut Body, input: EntryInput, now: i64) -> Result<String, String> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(INVALID.into());
    }
    let index = match input.id.as_deref() {
        Some(id) => Some(
            body.entries
                .iter()
                .position(|entry| entry.id == id && entry.deleted_at.is_none())
                .ok_or(NOT_FOUND)?,
        ),
        None => None,
    };
    let previous: &[Field] = index.map(|i| body.entries[i].fields.as_slice()).unwrap_or(&[]);
    let mut fields = Vec::new();
    for field in input.fields {
        let name = field.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let value = match field.value {
            Some(value) => value,
            None => {
                let from = field.previous_name.as_deref().unwrap_or(&name);
                previous.iter().find(|old| old.name == from).map(|old| old.value.clone()).unwrap_or_default()
            }
        };
        fields.push(Field { name, value, secret: field.secret });
    }
    let expires_at = input.expires_at.as_deref().and_then(normalize_date);
    let tags = clean_tags(input.tags);
    let platform = input.platform.trim().to_string();
    match index {
        Some(i) => {
            let entry = &mut body.entries[i];
            for old in entry.fields.iter().filter(|old| old.secret && !old.value.is_empty()) {
                if !fields.iter().any(|new| new.value == old.value) {
                    entry.history.push(HistoryItem { field: old.name.clone(), value: old.value.clone(), at: now });
                }
            }
            trim_history(&mut entry.history);
            let mut replaced = std::mem::replace(&mut entry.fields, fields);
            replaced.zeroize();
            entry.title = title;
            entry.platform = platform;
            entry.kind = input.kind;
            entry.expires_at = expires_at;
            entry.tags = tags;
            entry.note = input.note;
            entry.favorite = input.favorite;
            entry.updated_at = now;
            Ok(entry.id.clone())
        }
        None => {
            let id = new_id();
            body.entries.push(Entry {
                id: id.clone(),
                title,
                platform,
                kind: input.kind,
                fields,
                expires_at,
                tags,
                note: input.note,
                favorite: input.favorite,
                created_at: now,
                updated_at: now,
                deleted_at: None,
                history: Vec::new(),
            });
            Ok(id)
        }
    }
}

fn private_key(entry: &Entry) -> Option<&str> {
    if entry.kind != Kind::SshKey {
        return None;
    }
    entry
        .fields
        .iter()
        .find(|field| field.secret && field.value.contains("PRIVATE KEY-----"))
        .map(|field| field.value.as_str())
}

fn view(entry: &Entry) -> EntryView {
    EntryView {
        id: entry.id.clone(),
        title: entry.title.clone(),
        platform: entry.platform.clone(),
        kind: entry.kind,
        fields: entry
            .fields
            .iter()
            .map(|field| FieldView {
                name: field.name.clone(),
                secret: field.secret,
                value: (!field.secret).then(|| field.value.clone()),
                filled: !field.value.is_empty(),
            })
            .collect(),
        expires_at: entry.expires_at.clone(),
        tags: entry.tags.clone(),
        note: entry.note.clone(),
        favorite: entry.favorite,
        created_at: entry.created_at,
        updated_at: entry.updated_at,
        deleted_at: entry.deleted_at,
        history_count: entry.history.len(),
        ssh: private_key(entry).and_then(ssh::inspect),
    }
}

fn find<'a>(body: &'a Body, id: &str) -> Result<&'a Entry, String> {
    body.entries.iter().find(|entry| entry.id == id).ok_or_else(|| NOT_FOUND.to_string())
}

fn find_mut<'a>(body: &'a mut Body, id: &str) -> Result<&'a mut Entry, String> {
    body.entries.iter_mut().find(|entry| entry.id == id).ok_or_else(|| NOT_FOUND.to_string())
}

pub(crate) fn views(body: &Body, trash: bool) -> Vec<EntryView> {
    let mut list: Vec<&Entry> = body.entries.iter().filter(|entry| entry.deleted_at.is_some() == trash).collect();
    list.sort_by(|a, b| b.favorite.cmp(&a.favorite).then(b.updated_at.cmp(&a.updated_at)));
    list.into_iter().map(view).collect()
}

pub(crate) fn view_of(body: &Body, id: &str) -> Result<EntryView, String> {
    find(body, id).map(view)
}

pub(crate) fn field_value(body: &Body, id: &str, field: &str) -> Result<Zeroizing<String>, String> {
    find(body, id)?
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
        .map(|candidate| Zeroizing::new(candidate.value.clone()))
        .ok_or_else(|| NOT_FOUND.to_string())
}

pub(crate) fn ssh_private(body: &Body, id: &str) -> Result<Zeroizing<String>, String> {
    private_key(find(body, id)?)
        .map(|text| Zeroizing::new(text.to_string()))
        .ok_or_else(|| NOT_FOUND.to_string())
}

pub(crate) fn history_views(body: &Body, id: &str) -> Result<Vec<HistoryView>, String> {
    Ok(find(body, id)?
        .history
        .iter()
        .enumerate()
        .map(|(index, item)| HistoryView { index, field: item.field.clone(), at: item.at })
        .collect())
}

pub(crate) fn history_value(body: &Body, id: &str, index: usize) -> Result<Zeroizing<String>, String> {
    find(body, id)?
        .history
        .get(index)
        .map(|item| Zeroizing::new(item.value.clone()))
        .ok_or_else(|| NOT_FOUND.to_string())
}

pub(crate) fn soft_delete(body: &mut Body, id: &str, now: i64) -> Result<(), String> {
    let entry = find_mut(body, id)?;
    if entry.deleted_at.is_none() {
        entry.deleted_at = Some(now);
    }
    Ok(())
}

pub(crate) fn restore(body: &mut Body, id: &str) -> Result<(), String> {
    find_mut(body, id)?.deleted_at = None;
    Ok(())
}

pub(crate) fn purge(body: &mut Body, id: &str) -> Result<(), String> {
    let index = body
        .entries
        .iter()
        .position(|entry| entry.id == id && entry.deleted_at.is_some())
        .ok_or(NOT_FOUND)?;
    body.entries.remove(index).zeroize();
    Ok(())
}

pub(crate) fn set_favorite(body: &mut Body, id: &str, favorite: bool, now: i64) -> Result<(), String> {
    let entry = find_mut(body, id)?;
    entry.favorite = favorite;
    entry.updated_at = now;
    Ok(())
}

/// Removes trashed entries older than TRASH_DAYS; true when anything went.
pub(crate) fn purge_expired(body: &mut Body, now: i64) -> bool {
    let limit = now - TRASH_DAYS * DAY_MS;
    let before = body.entries.len();
    body.entries.retain_mut(|entry| {
        let keep = entry.deleted_at.map_or(true, |at| at > limit);
        if !keep {
            entry.zeroize();
        }
        keep
    });
    body.entries.len() != before
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct MergeStats {
    pub added: usize,
    pub updated: usize,
    pub same: usize,
}

/// Imports never delete: a newer copy replaces the content, the local trash state stays,
/// histories and ignored digests are combined.
pub(crate) fn merge(local: &mut Body, incoming: &Body) -> MergeStats {
    let mut stats = MergeStats::default();
    for theirs in &incoming.entries {
        match local.entries.iter_mut().find(|entry| entry.id == theirs.id) {
            None => {
                local.entries.push(theirs.clone());
                stats.added += 1;
            }
            Some(mine) => {
                let mut history = mine.history.clone();
                if theirs.updated_at > mine.updated_at {
                    // Same rule as apply_input: a secret the replacing copy no longer holds is kept.
                    for old in mine.fields.iter().filter(|old| old.secret && !old.value.is_empty()) {
                        let kept = theirs.fields.iter().any(|new| new.secret && new.value == old.value);
                        let item = HistoryItem { field: old.name.clone(), value: old.value.clone(), at: theirs.updated_at };
                        if !kept && !history.contains(&item) {
                            history.push(item);
                        }
                    }
                }
                for item in &theirs.history {
                    if !history.contains(item) {
                        history.push(item.clone());
                    }
                }
                trim_history(&mut history);
                if theirs.updated_at > mine.updated_at {
                    let deleted_at = mine.deleted_at;
                    std::mem::replace(mine, theirs.clone()).zeroize();
                    mine.deleted_at = deleted_at;
                    stats.updated += 1;
                } else {
                    stats.same += 1;
                }
                std::mem::replace(&mut mine.history, history).zeroize();
            }
        }
    }
    for digest in &incoming.ignored {
        if !local.ignored.contains(digest) {
            local.ignored.push(digest.clone());
        }
    }
    stats
}

pub(crate) struct Digests {
    pub current: HashSet<String>,
    pub old: HashSet<String>,
    pub ignored: HashSet<String>,
}

/// Secrets in live entries are current; history and trashed entries count as old values.
pub(crate) fn digests(body: &Body) -> Digests {
    let mut out = Digests {
        current: HashSet::new(),
        old: HashSet::new(),
        ignored: body.ignored.iter().cloned().collect(),
    };
    for entry in &body.entries {
        for field in entry.fields.iter().filter(|field| field.secret && !field.value.is_empty()) {
            let digest = crypto::secret_digest(&field.value);
            if entry.deleted_at.is_none() {
                out.current.insert(digest);
            } else {
                out.old.insert(digest);
            }
        }
        for item in &entry.history {
            out.old.insert(crypto::secret_digest(&item.value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn input(id: Option<&str>, title: &str, fields: Vec<FieldInput>) -> EntryInput {
        EntryInput {
            id: id.map(str::to_string),
            title: title.into(),
            platform: " 火山方舟 ".into(),
            kind: Kind::TokenPlan,
            fields,
            expires_at: Some("2026-10-31".into()),
            tags: vec![" a ".into(), "a".into(), "".into(), "b".into()],
            note: "note".into(),
            favorite: false,
        }
    }

    fn field(name: &str, value: Option<&str>, secret: bool) -> FieldInput {
        FieldInput { name: name.into(), previous_name: None, value: value.map(str::to_string), secret }
    }

    #[test]
    fn creating_cleans_input_and_masks_secrets_in_views() {
        let mut body = Body::default();
        let id = apply_input(&mut body, input(None, " Coding Plan ", vec![
            field("Key", Some("k-1"), true),
            field("Base URL", Some("https://x"), false),
            field("  ", Some("dropped"), false),
        ]), 1).unwrap();
        let view = view_of(&body, &id).unwrap();
        assert_eq!(view.title, "Coding Plan");
        assert_eq!(view.platform, "火山方舟");
        assert_eq!(view.tags, vec!["a", "b"]);
        assert_eq!(view.fields.len(), 2);
        assert_eq!(view.fields[0].value, None);
        assert!(view.fields[0].filled);
        assert_eq!(view.fields[1].value.as_deref(), Some("https://x"));
        assert_eq!(*field_value(&body, &id, "Key").unwrap(), "k-1");
    }

    #[test]
    fn empty_title_and_bad_dates_are_handled() {
        let mut body = Body::default();
        assert_eq!(apply_input(&mut body, input(None, "  ", vec![]), 1).err().unwrap(), INVALID);
        let mut bad = input(None, "t", vec![]);
        bad.expires_at = Some("31/10/2026".into());
        let id = apply_input(&mut body, bad, 1).unwrap();
        assert_eq!(view_of(&body, &id).unwrap().expires_at, None);
    }

    #[test]
    fn a_secret_sent_without_value_keeps_its_value_even_when_renamed() {
        let mut body = Body::default();
        let id = apply_input(&mut body, input(None, "t", vec![field("Key", Some("v1"), true)]), 1).unwrap();
        let renamed = FieldInput { name: "API Key".into(), previous_name: Some("Key".into()), value: None, secret: true };
        apply_input(&mut body, input(Some(&id), "t", vec![renamed]), 2).unwrap();
        assert_eq!(*field_value(&body, &id, "API Key").unwrap(), "v1");
        assert!(history_views(&body, &id).unwrap().is_empty());
    }

    #[test]
    fn changed_secrets_go_to_history_capped_per_field() {
        let mut body = Body::default();
        let id = apply_input(&mut body, input(None, "t", vec![field("Key", Some("v0"), true), field("Url", Some("u0"), false)]), 0).unwrap();
        for n in 1..=12 {
            let value = format!("v{n}");
            apply_input(&mut body, input(Some(&id), "t", vec![field("Key", Some(&value), true), field("Url", Some(&format!("u{n}")), false)]), n).unwrap();
        }
        let history = history_views(&body, &id).unwrap();
        assert_eq!(history.len(), HISTORY_PER_FIELD);
        assert!(history.iter().all(|h| h.field == "Key"));
        assert_eq!(*history_value(&body, &id, 0).unwrap(), "v11");
        assert_eq!(*history_value(&body, &id, 9).unwrap(), "v2");
    }

    #[test]
    fn trash_restore_purge_and_expiry() {
        let mut body = Body::default();
        let a = apply_input(&mut body, input(None, "a", vec![]), 1).unwrap();
        let b = apply_input(&mut body, input(None, "b", vec![]), 1).unwrap();
        assert_eq!(purge(&mut body, &a).err().unwrap(), NOT_FOUND, "only trashed entries can be purged");
        soft_delete(&mut body, &a, 10).unwrap();
        soft_delete(&mut body, &b, 10 + 5 * DAY).unwrap();
        assert!(views(&body, false).is_empty());
        assert_eq!(views(&body, true).len(), 2);
        restore(&mut body, &b).unwrap();
        assert_eq!(views(&body, false).len(), 1);
        assert!(!purge_expired(&mut body, 10 + 29 * DAY));
        assert!(purge_expired(&mut body, 10 + 30 * DAY));
        assert!(views(&body, true).is_empty());
        soft_delete(&mut body, &b, 1).unwrap();
        purge(&mut body, &b).unwrap();
        assert!(body.entries.is_empty());
    }

    #[test]
    fn expiry_dates_are_stored_normalized() {
        let mut body = Body::default();
        let mut loose = input(None, "t", vec![]);
        loose.expires_at = Some("2026-1-5".into());
        let id = apply_input(&mut body, loose, 1).unwrap();
        assert_eq!(view_of(&body, &id).unwrap().expires_at.as_deref(), Some("2026-01-05"));
    }

    fn entry(id: &str, updated_at: i64, secret: &str) -> Entry {
        Entry {
            id: id.into(),
            title: id.into(),
            platform: String::new(),
            kind: Kind::ApiKey,
            fields: vec![Field { name: "Key".into(), value: secret.into(), secret: true }],
            expires_at: None,
            tags: vec![],
            note: String::new(),
            favorite: false,
            created_at: 0,
            updated_at,
            deleted_at: None,
            history: vec![],
        }
    }

    #[test]
    fn merge_adds_updates_and_never_deletes() {
        let mut local = Body { entries: vec![entry("a", 5, "a-local"), entry("b", 5, "b-local")], ignored: vec!["x".into()] };
        local.entries[0].deleted_at = Some(4);
        let mut newer_a = entry("a", 9, "a-backup");
        newer_a.history.push(HistoryItem { field: "Key".into(), value: "a-older".into(), at: 1 });
        let mut trashed_c = entry("c", 1, "c");
        trashed_c.deleted_at = Some(1);
        let incoming = Body {
            entries: vec![newer_a, entry("b", 3, "b-backup"), trashed_c, entry("d", 1, "d")],
            ignored: vec!["x".into(), "y".into()],
        };
        let stats = merge(&mut local, &incoming);
        assert_eq!(stats, MergeStats { added: 2, updated: 1, same: 1 });
        let a = local.entries.iter().find(|e| e.id == "a").unwrap();
        assert_eq!(a.fields[0].value, "a-backup");
        assert_eq!(a.deleted_at, Some(4), "local trash state is kept");
        assert_eq!(a.history.len(), 2, "their older value and the local value the copy replaced");
        assert_eq!(local.entries.iter().find(|e| e.id == "b").unwrap().fields[0].value, "b-local");
        assert!(local.entries.iter().find(|e| e.id == "c").unwrap().deleted_at.is_some());
        assert_eq!(local.ignored, vec!["x", "y"]);
    }

    #[test]
    fn a_newer_copy_archives_the_local_secret_it_replaces() {
        let mut local = Body { entries: vec![entry("a", 5, "rotated-locally")], ignored: vec![] };
        let incoming = Body { entries: vec![entry("a", 9, "from-backup")], ignored: vec![] };
        merge(&mut local, &incoming);
        let a = &local.entries[0];
        assert_eq!(a.fields[0].value, "from-backup");
        assert_eq!(a.history.len(), 1);
        assert_eq!(a.history[0], HistoryItem { field: "Key".into(), value: "rotated-locally".into(), at: 9 });
    }

    #[test]
    fn a_local_secret_the_copy_still_holds_is_not_archived() {
        let mut local = Body { entries: vec![entry("a", 5, "same")], ignored: vec![] };
        let mut newer = entry("a", 9, "same");
        newer.title = "renamed".into();
        merge(&mut local, &Body { entries: vec![newer], ignored: vec![] });
        assert_eq!(local.entries[0].title, "renamed");
        assert!(local.entries[0].history.is_empty());
    }

    #[test]
    fn an_older_copy_archives_nothing() {
        let mut local = Body { entries: vec![entry("a", 9, "local")], ignored: vec![] };
        merge(&mut local, &Body { entries: vec![entry("a", 5, "older")], ignored: vec![] });
        assert_eq!(local.entries[0].fields[0].value, "local");
        assert!(local.entries[0].history.is_empty());
    }

    #[test]
    fn digests_split_current_old_and_ignored() {
        let mut body = Body { entries: vec![entry("a", 1, "live"), entry("b", 1, "gone")], ignored: vec!["ign".into()] };
        body.entries[0].history.push(HistoryItem { field: "Key".into(), value: "rotated\r\n".into(), at: 1 });
        body.entries[1].deleted_at = Some(1);
        let d = digests(&body);
        assert!(d.current.contains(&crypto::secret_digest("live")));
        assert!(d.old.contains(&crypto::secret_digest("rotated")));
        assert!(d.old.contains(&crypto::secret_digest("gone")));
        assert!(d.ignored.contains("ign"));
    }

    #[test]
    fn favorites_first_then_most_recent() {
        let mut body = Body::default();
        let old = apply_input(&mut body, input(None, "old", vec![]), 1).unwrap();
        let new = apply_input(&mut body, input(None, "new", vec![]), 2).unwrap();
        assert_eq!(views(&body, false)[0].id, new);
        set_favorite(&mut body, &old, true, 3).unwrap();
        assert_eq!(views(&body, false)[0].id, old);
    }
}
