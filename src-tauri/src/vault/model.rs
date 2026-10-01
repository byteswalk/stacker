//! 条目模型：编辑、保密字段历史、回收站、视图、导入合并。不含 IO。

use super::crypto;
use super::errors::{INVALID, NOT_FOUND};
use super::ssh::{self, SshInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

fn valid_date(value: &str) -> bool {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
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

/// Newest first, at most HISTORY_PER_FIELD per field name.
pub(crate) fn trim_history(history: &mut Vec<HistoryItem>) {
    history.sort_by(|a, b| b.at.cmp(&a.at));
    let mut kept: HashMap<String, usize> = HashMap::new();
    history.retain(|item| {
        let count = kept.entry(item.field.clone()).or_insert(0);
        *count += 1;
        *count <= HISTORY_PER_FIELD
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
    let expires_at = input.expires_at.filter(|date| valid_date(date));
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
    fn favorites_first_then_most_recent() {
        let mut body = Body::default();
        let old = apply_input(&mut body, input(None, "old", vec![]), 1).unwrap();
        let new = apply_input(&mut body, input(None, "new", vec![]), 2).unwrap();
        assert_eq!(views(&body, false)[0].id, new);
        set_favorite(&mut body, &old, true, 3).unwrap();
        assert_eq!(views(&body, false)[0].id, old);
    }
}
