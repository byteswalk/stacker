//! Error codes returned to the page; the page owns every user-facing sentence.

pub(crate) const MISSING: &str = "E_VAULT_MISSING";
pub(crate) const EXISTS: &str = "E_VAULT_EXISTS";
pub(crate) const LOCKED: &str = "E_VAULT_LOCKED";
pub(crate) const PASSWORD: &str = "E_VAULT_PASSWORD";
pub(crate) const RECOVERY: &str = "E_VAULT_RECOVERY";
pub(crate) const WEAK: &str = "E_VAULT_WEAK";
pub(crate) const WAIT: &str = "E_VAULT_WAIT";
pub(crate) const CORRUPT: &str = "E_VAULT_CORRUPT";
pub(crate) const NEWER: &str = "E_VAULT_NEWER";
pub(crate) const CHANGED: &str = "E_VAULT_CHANGED";
pub(crate) const CONFIRM: &str = "E_VAULT_CONFIRM";
pub(crate) const NO_PENDING: &str = "E_VAULT_NO_PENDING";
pub(crate) const NOT_FOUND: &str = "E_VAULT_NOT_FOUND";
pub(crate) const FILE_EXISTS: &str = "E_VAULT_FILE_EXISTS";
pub(crate) const INVALID: &str = "E_VAULT_INVALID";
pub(crate) const BUSY: &str = "E_VAULT_BUSY";
pub(crate) const NAME: &str = "E_VAULT_NAME";
pub(crate) const HOST: &str = "E_VAULT_HOST";
pub(crate) const HOST_EXISTS: &str = "E_VAULT_HOST_EXISTS";
pub(crate) const IO: &str = "E_VAULT_IO";
