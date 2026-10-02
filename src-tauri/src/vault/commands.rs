//! Tauri commands of the key vault. Everything except the two trivial ones runs on a blocking
//! thread: Argon2id takes hundreds of milliseconds and must never stall the UI thread.
//! Errors are `E_VAULT_*` codes only; no value is ever logged.

use super::discover::{self, ImportItem, JobStatus, Scope};
use super::errors::{IO, NOT_FOUND};
use super::model::{EntryInput, EntryView, HistoryView, MergeStats};
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

/// The one place the vault writes to `~/.ssh`, and only when the user asks for it.
#[tauri::command]
pub async fn vault_ssh_install_local(
    id: String,
    name: String,
    host: Option<ssh::LocalHost>,
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
