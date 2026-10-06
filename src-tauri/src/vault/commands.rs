//! Tauri commands of the key vault. Everything except the two trivial ones runs on a blocking
//! thread: Argon2id takes hundreds of milliseconds and must never stall the UI thread.
//! Errors are `E_VAULT_*` codes only; no value is ever logged.

use super::discover::{self, ImportItem, JobStatus, Scope};
use super::errors::{IO, NOT_FOUND};
use super::model::{EntryInput, EntryView, FieldInput, HistoryView, MergeStats};
use super::session::{Credential, Retired, Status};
use super::{clipboard, guard, ssh, vault};
use std::path::PathBuf;
use zeroize::Zeroizing;

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| IO.to_string())?
}

#[tauri::command]
pub async fn vault_status() -> Result<Status, String> {
    blocking(|| Ok(vault().status())).await
}

#[tauri::command]
pub async fn vault_create_begin(password: String) -> Result<String, String> {
    let password = Zeroizing::new(password);
    blocking(move || vault().create_begin(&password).map(|key| key.to_string())).await
}

#[tauri::command]
pub async fn vault_copy_recovery() -> Result<(), String> {
    blocking(|| clipboard::copy_secret(&vault().pending_recovery()?)).await
}

#[tauri::command]
pub async fn vault_confirm_recovery(last_group: String) -> Result<(), String> {
    blocking(move || vault().confirm_recovery(&last_group)).await
}

#[tauri::command]
pub async fn vault_cancel_pending() -> Result<(), String> {
    blocking(|| {
        vault().cancel_pending();
        Ok(())
    })
    .await
}

/// A fresh session never inherits findings from an earlier one, so every successful unlock
/// drops them (after the vault's own lock is released).
#[tauri::command]
pub async fn vault_unlock(password: String) -> Result<(), String> {
    let password = Zeroizing::new(password);
    blocking(move || {
        vault().unlock(&password)?;
        discover::clear();
        // Logins the browser extension saved while the vault was locked.
        let _ = vault().import_inbox();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn vault_unlock_recovery(recovery_key: String) -> Result<(), String> {
    let recovery_key = Zeroizing::new(recovery_key);
    blocking(move || {
        vault().unlock_recovery(&recovery_key)?;
        discover::clear();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn vault_recovery_set_password(password: String) -> Result<String, String> {
    let password = Zeroizing::new(password);
    blocking(move || {
        vault()
            .recovery_set_password(&password)
            .map(|key| key.to_string())
    })
    .await
}

#[tauri::command]
pub async fn vault_change_password(current: String, next: String) -> Result<(), String> {
    let (current, next) = (Zeroizing::new(current), Zeroizing::new(next));
    blocking(move || vault().change_password(&current, &next)).await
}

#[tauri::command]
pub async fn vault_reset_recovery(password: String) -> Result<String, String> {
    let password = Zeroizing::new(password);
    blocking(move || vault().reset_recovery(&password).map(|key| key.to_string())).await
}

#[tauri::command]
pub async fn vault_lock() -> Result<(), String> {
    blocking(|| {
        guard::lock_everything();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn vault_touch() -> Result<(), String> {
    vault().touch();
    Ok(())
}

#[tauri::command]
pub async fn vault_list(trash: bool) -> Result<Vec<EntryView>, String> {
    blocking(move || vault().list(trash)).await
}

#[tauri::command]
pub async fn vault_save(input: EntryInput) -> Result<EntryView, String> {
    blocking(move || vault().save(input)).await
}

#[tauri::command]
pub async fn vault_reveal(id: String, field: String) -> Result<String, String> {
    blocking(move || vault().reveal(&id, &field).map(|value| value.to_string())).await
}

#[tauri::command]
pub async fn vault_copy(id: String, field: String) -> Result<(), String> {
    blocking(move || clipboard::copy_secret(&vault().reveal(&id, &field)?)).await
}

#[tauri::command]
pub async fn vault_history(id: String) -> Result<Vec<HistoryView>, String> {
    blocking(move || vault().history(&id)).await
}

#[tauri::command]
pub async fn vault_history_reveal(id: String, index: usize) -> Result<String, String> {
    blocking(move || {
        vault()
            .history_value(&id, index)
            .map(|value| value.to_string())
    })
    .await
}

#[tauri::command]
pub async fn vault_history_copy(id: String, index: usize) -> Result<(), String> {
    blocking(move || clipboard::copy_secret(&vault().history_value(&id, index)?)).await
}

#[tauri::command]
pub async fn vault_delete(id: String) -> Result<(), String> {
    blocking(move || vault().delete(&id)).await
}

#[tauri::command]
pub async fn vault_restore(id: String) -> Result<(), String> {
    blocking(move || vault().restore(&id)).await
}

#[tauri::command]
pub async fn vault_purge(id: String) -> Result<(), String> {
    blocking(move || vault().purge(&id)).await
}

#[tauri::command]
pub async fn vault_favorite(id: String, favorite: bool) -> Result<(), String> {
    blocking(move || vault().favorite(&id, favorite)).await
}

#[tauri::command]
pub async fn vault_ssh_export(id: String, dest: String) -> Result<(), String> {
    blocking(move || ssh::export_private(&vault().ssh_private(&id)?, &PathBuf::from(dest))).await
}

/// A fresh key pair for the page to save as an entry; nothing is stored here.
#[tauri::command]
pub async fn vault_ssh_generate(
    algorithm: String,
    comment: String,
    passphrase: String,
) -> Result<ssh::KeyPair, String> {
    let passphrase = Zeroizing::new(passphrase);
    blocking(move || ssh::generate(&algorithm, &comment, &passphrase)).await
}

/// Sets, changes or removes the passphrase of an entry's private key, and records the new
/// passphrase in the entry's `passphrase_field` beside it. With `old` left empty, the
/// passphrase the entry already records is tried.
#[tauri::command]
pub async fn vault_ssh_set_passphrase(
    id: String,
    old: String,
    new: String,
    passphrase_field: String,
) -> Result<EntryView, String> {
    let (old, new) = (Zeroizing::new(old), Zeroizing::new(new));
    blocking(move || {
        let private = vault().ssh_private(&id)?;
        let view = vault()
            .list(false)?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or(NOT_FOUND)?;
        let recorded = |name: &str| {
            view.fields
                .iter()
                .any(|field| field.name == name && field.secret && field.filled)
                .then(|| vault().reveal(&id, name).ok())
                .flatten()
        };
        let old = if old.is_empty() {
            recorded(&passphrase_field).unwrap_or(old)
        } else {
            old
        };
        let rewritten = ssh::set_passphrase(&private, &old, &new)?;
        let key_field = view
            .fields
            .iter()
            .filter(|field| field.secret && field.filled)
            .find(|field| recorded(&field.name).is_some_and(|value| *value == *private))
            .map(|field| field.name.clone())
            .ok_or(NOT_FOUND)?;
        let mut fields: Vec<FieldInput> = view
            .fields
            .iter()
            .filter(|field| field.name != passphrase_field)
            .map(|field| FieldInput {
                name: field.name.clone(),
                previous_name: Some(field.name.clone()),
                value: if field.name == key_field {
                    Some(rewritten.to_string())
                } else if field.secret {
                    None
                } else {
                    Some(field.value.clone().unwrap_or_default())
                },
                secret: field.secret,
            })
            .collect();
        if !new.is_empty() {
            let at = fields
                .iter()
                .position(|field| field.name == key_field)
                .map_or(fields.len(), |index| index + 1);
            fields.insert(
                at,
                FieldInput {
                    name: passphrase_field.clone(),
                    previous_name: None,
                    value: Some(new.to_string()),
                    secret: true,
                },
            );
        }
        vault().save(EntryInput {
            id: Some(id.clone()),
            title: view.title.clone(),
            platform: view.platform.clone(),
            kind: view.kind,
            fields,
            expires_at: view.expires_at.clone(),
            tags: view.tags.clone(),
            note: view.note.clone(),
            favorite: view.favorite,
        })
    })
    .await
}

/// Reads a browser's password export: what it would add, or (with `apply`) adds it.
#[tauri::command]
pub async fn vault_import_browser(
    src: String,
    apply: bool,
) -> Result<super::browser::BrowserStats, String> {
    blocking(move || {
        let text = super::browser::read(&PathBuf::from(src))?;
        vault().import_browser(&text, apply)
    })
    .await
}

/// Puts the secrets of `ids` into Windows Credential Manager, or takes them out.
#[tauri::command]
pub async fn vault_set_windows(ids: Vec<String>, on: bool) -> Result<usize, String> {
    blocking(move || vault().set_windows(&ids, on)).await
}

/// A secret field and the name it has in Windows Credential Manager.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialTarget {
    field: String,
    target: String,
}

/// Under which names an entry's secrets can be read from Credential Manager.
#[tauri::command]
pub async fn vault_credential_targets(id: String) -> Result<Vec<CredentialTarget>, String> {
    blocking(move || {
        Ok(vault()
            .credential_targets(&id)?
            .into_iter()
            .map(|item| CredentialTarget {
                field: item.field,
                target: item.target,
            })
            .collect())
    })
    .await
}

/// An environment variable that already holds one of an entry's secrets.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvHolder {
    field: String,
    name: String,
    scope: &'static str,
}

/// Which of an entry's secrets a program on this computer can already read from the
/// environment, so an AI agent is told the variable's name and never the value. Read-only.
#[tauri::command]
pub async fn vault_env_holders(id: String) -> Result<Vec<EnvHolder>, String> {
    blocking(move || {
        let view = vault()
            .list(false)?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or(NOT_FOUND)?;
        let mut out = Vec::new();
        for field in view
            .fields
            .iter()
            .filter(|field| field.secret && field.filled)
        {
            let value = vault().reveal(&id, &field.name)?;
            out.extend(
                discover::sources::env_names_holding(&value)
                    .into_iter()
                    .map(|(scope, name)| EnvHolder {
                        field: field.name.clone(),
                        name,
                        scope,
                    }),
            );
        }
        Ok(out)
    })
    .await
}

/// The public key line of a private key pasted into the editor; nothing is stored.
#[tauri::command]
pub async fn vault_ssh_public_of(private: String) -> Result<Option<String>, String> {
    let private = Zeroizing::new(private);
    blocking(move || Ok(ssh::public_of(&private))).await
}

/// Whether the local ssh already has an entry's key, and under which alias: read-only.
#[tauri::command]
pub async fn vault_ssh_local(id: String) -> Result<ssh::LocalKey, String> {
    blocking(move || {
        let dir = dirs::home_dir().ok_or(NOT_FOUND)?.join(".ssh");
        Ok(ssh::find_local(&dir, &vault().ssh_private(&id)?))
    })
    .await
}

/// The one place the vault writes to `~/.ssh`, and only when the user asks for it.
#[tauri::command]
pub async fn vault_ssh_install_local(
    id: String,
    name: String,
    host: Option<ssh::LocalHost>,
    overwrite: bool,
) -> Result<String, String> {
    blocking(move || {
        let dir = dirs::home_dir().ok_or(NOT_FOUND)?.join(".ssh");
        let private = vault().ssh_private(&id)?;
        let public = ssh::inspect(&private).and_then(|info| info.public_key);
        ssh::install_local(
            &dir,
            &name,
            &private,
            public.as_deref(),
            host.as_ref(),
            overwrite,
            |config| {
                crate::backup::backup_file(config);
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn vault_export(password: String, dest: String) -> Result<(), String> {
    let password = Zeroizing::new(password);
    blocking(move || vault().export(&password, &PathBuf::from(dest))).await
}

#[tauri::command]
pub async fn vault_import_preview(
    src: String,
    credential: Credential,
) -> Result<MergeStats, String> {
    blocking(move || vault().import_preview(&PathBuf::from(src), &credential)).await
}

#[tauri::command]
pub async fn vault_import_apply(src: String, credential: Credential) -> Result<MergeStats, String> {
    blocking(move || vault().import_apply(&PathBuf::from(src), &credential)).await
}

#[tauri::command]
pub async fn vault_restore_backup(src: String) -> Result<(), String> {
    blocking(move || vault().restore_backup(&PathBuf::from(src))).await
}

#[tauri::command]
pub async fn vault_reset() -> Result<String, String> {
    blocking(|| {
        discover::clear();
        vault().reset()
    })
    .await
}

#[tauri::command]
pub async fn vault_retired() -> Result<Vec<Retired>, String> {
    blocking(|| Ok(vault().retired())).await
}

/// What the clipboard holds as text, for the recovery key's paste button. Capped well above a
/// key's length so a large copy is never carried across.
#[tauri::command]
pub async fn vault_clipboard_text() -> Result<String, String> {
    blocking(|| Ok(clipboard::read_text())).await
}

#[tauri::command]
pub async fn vault_discover_start(scope: Scope) -> Result<(), String> {
    blocking(move || {
        let home = dirs::home_dir().ok_or(NOT_FOUND)?;
        discover::start(vault(), scope, home)
    })
    .await
}

#[tauri::command]
pub async fn vault_discover_status() -> Result<JobStatus, String> {
    blocking(|| Ok(discover::status(vault()))).await
}

#[tauri::command]
pub async fn vault_discover_cancel() -> Result<(), String> {
    discover::cancel();
    Ok(())
}

/// Leaving the discover tab drops the findings (their raw values are plaintext).
#[tauri::command]
pub async fn vault_discover_clear() -> Result<(), String> {
    discover::clear();
    Ok(())
}

#[tauri::command]
pub async fn vault_discover_import(
    items: Vec<ImportItem>,
    note_prefix: String,
) -> Result<usize, String> {
    blocking(move || discover::import(vault(), &items, &note_prefix)).await
}

#[tauri::command]
pub async fn vault_discover_ignore(ids: Vec<usize>) -> Result<(), String> {
    blocking(move || discover::ignore(vault(), &ids)).await
}
