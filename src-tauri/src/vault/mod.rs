//! 密钥保管：本机加密保管库。主密码与恢复密钥各自包裹同一个数据密钥，数据密钥加密全部条目。
//! 只记不用：不写凭据管理器、不改环境变量、不改 ~/.ssh、不调用任何平台 API。

pub(crate) mod crypto;
pub(crate) mod errors;
pub(crate) mod format;
pub(crate) mod ssh;
