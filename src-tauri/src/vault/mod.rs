//! 密钥保管：本机加密保管库。主密码与恢复密钥各自包裹同一个数据密钥，数据密钥加密全部条目。
//! 只记不用：不写凭据管理器、不改环境变量、不调用任何平台 API；非 SSH 条目的保密值同步一份到 Windows 凭据管理器，供本机其他程序按名字取用；~/.ssh 只在用户点「放到本机」时新增文件、追加 config，从不覆盖。

pub(crate) mod browser;
pub(crate) mod clipboard;
pub(crate) mod commands;
pub(crate) mod crypto;
pub(crate) mod discover;
pub(crate) mod errors;
pub(crate) mod format;
pub(crate) mod guard;
pub(crate) mod model;
pub(crate) mod session;
pub(crate) mod ssh;
pub(crate) mod wincred;

use std::path::PathBuf;
use std::sync::OnceLock;

pub(crate) fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_default()
        .join("stacker")
        .join("vault.skv")
}

pub(crate) fn vault() -> &'static session::Vault {
    static VAULT: OnceLock<session::Vault> = OnceLock::new();
    VAULT.get_or_init(|| {
        session::Vault::new(default_path(), crypto::KdfParams::STANDARD)
            .mirrored_to(std::sync::Arc::new(wincred::SystemStore))
    })
}
