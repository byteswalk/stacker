use encoding_rs::GBK;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tauri::Emitter;

#[derive(Serialize, Default)]
pub struct GitStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub bash_path: Option<String>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub default_branch: Option<String>,
    pub autocrlf: Option<String>,
    pub credential_helper: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub gcm: bool,
}

#[derive(Serialize, Default)]
pub struct GitHubAccountsState {
    pub gcm_available: bool,
    pub gcm_version: Option<String>,
    pub accounts: Vec<String>,
    pub default_account: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct GitAccountProfile {
    pub platform: String,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub authenticated: bool,
    #[serde(default)]
    pub token_verified: bool,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct GitAccountProfilesFile {
    accounts: Vec<GitAccountProfile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInitRequest {
    pub platform: String,
    pub username: String,
    pub directory: String,
    pub repository_name: String,
    pub description: String,
    pub private_repository: bool,
    pub create_remote: bool,
    pub remote_url: Option<String>,
    pub create_readme: bool,
    pub display_name: String,
    pub email: String,
}

#[derive(Serialize)]
pub struct GitInitResult {
    pub directory: String,
    pub remote_url: Option<String>,
    pub initial_commit: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubTransferRequest {
    pub source_account: String,
    pub source_owner: String,
    pub repository: String,
    pub target_account: String,
    pub target_repository: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitMirrorRequest {
    pub source_platform: String,
    pub source_account: String,
    pub source_url: String,
    pub target_platform: String,
    pub target_account: String,
    pub target_url: String,
    pub include_lfs: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitAutoMigrateRequest {
    pub source_platform: String,
    pub source_account: String,
    pub source_owner: String,
    pub source_repository: String,
    pub target_platform: String,
    pub target_account: String,
    pub target_repository: String,
    pub target_private: bool,
    pub include_lfs: bool,
}

#[derive(Serialize)]
pub struct GitAutoMigrateResult {
    pub mode: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Default)]
struct GiteeAccountsFile {
    accounts: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct GitHubTokenAccountsFile {
    accounts: Vec<String>,
}

#[derive(Deserialize)]
struct PlatformUser {
    login: String,
    name: Option<String>,
    email: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
}

struct CustomServiceVerification {
    provider: String,
    service_name: String,
    user: PlatformUser,
}

#[derive(Serialize)]
pub struct GitUpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    pub source_name: String,
    pub release_url: String,
    pub installer_url: String,
}

#[derive(Deserialize)]
struct GitReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GitRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GitReleaseAsset>,
}

#[derive(Deserialize)]
struct GitMirrorEntry {
    name: String,
    #[serde(default)]
    r#type: String,
    url: String,
}

#[tauri::command]
pub async fn git_status() -> GitStatus {
    tauri::async_runtime::spawn_blocking(status_snapshot)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn git_check_update(source_id: String) -> Result<GitUpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let git = ensure_git()?;
        let current_full = run_program(&git, &["--version"], Duration::from_secs(5))?;
        let current = git_version_token(&current_full).unwrap_or(current_full);
        latest_git_release(current, &source_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_install(window: tauri::Window, source_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || install_git_impl(window, &source_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_github_accounts() -> Result<GitHubAccountsState, String> {
    tauri::async_runtime::spawn_blocking(github_accounts_state)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_account_save_token(
    platform: String,
    credential: String,
) -> Result<Vec<GitAccountProfile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let git = ensure_git()?;
        ensure_gcm(&git)?;
        let configured = run_output(
            &git,
            &["credential-manager", "configure"],
            Duration::from_secs(20),
        )?;
        if !configured.status.success() {
            return Err(command_error(
                "无法启用 Git Credential Manager",
                &configured,
            ));
        }
        let platform = normalize_platform(&platform)?;
        validate_credential(&credential)?;
        let verified = verify_platform_token(&platform, credential.trim())?;
        let username = normalize_account_username(&platform, &verified.login)?;

        let input = format!(
            "protocol=https\nhost={}\nusername={username}\npassword={}\n\n",
            platform_host(&platform),
            credential.trim()
        );
        let out = run_output_with_input(
            &git,
            &["credential-manager", "store", "--no-ui"],
            &input,
            Duration::from_secs(15),
        )?;
        if !out.status.success() {
            return Err(command_error("无法保存账号令牌", &out));
        }

        let mut accounts = load_token_accounts(&platform);
        if !accounts
            .iter()
            .any(|account| account.eq_ignore_ascii_case(&username))
        {
            accounts.push(username.clone());
            save_token_accounts(&platform, &accounts)?;
        }

        let mut profiles = load_account_profiles();
        if let Some(profile) = profiles.iter_mut().find(|profile| {
            profile.platform == platform && profile.username.eq_ignore_ascii_case(&username)
        }) {
            profile.authenticated = true;
            profile.token_verified = true;
            profile.expires_at = verified.expires_at;
            if profile.display_name.is_none() {
                profile.display_name = verified.name;
            }
            if profile.email.is_none() {
                profile.email = verified.email;
            }
        } else {
            profiles.push(GitAccountProfile {
                platform,
                username,
                display_name: verified.name,
                email: verified.email,
                expires_at: verified.expires_at,
                authenticated: true,
                token_verified: true,
                service_name: None,
                base_url: None,
                provider: None,
            });
        }
        save_account_profiles(&profiles)?;
        account_profiles()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_account_save_custom_token(
    service_url: String,
    service_name: String,
    username: String,
    credential: String,
) -> Result<Vec<GitAccountProfile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let git = ensure_git()?;
        ensure_gcm(&git)?;
        configure_gcm(&git)?;
        validate_credential(&credential)?;
        let (base_url, host, protocol) = normalize_service_url(&service_url)?;
        let requested_username = normalize_generic_username(&username)?;
        let verification =
            verify_custom_service_token(&base_url, &requested_username, credential.trim())?;
        let (provider, detected_name, verified_user, token_verified) = match verification {
            Some(result) => {
                if result.provider != "aliyun-codeup"
                    && !result.user.login.eq_ignore_ascii_case(&requested_username)
                {
                    return Err(format!(
                        "令牌（token）所属账号为 {}，与填写的账号 {} 不一致。",
                        result.user.login, requested_username
                    ));
                }
                (result.provider, result.service_name, result.user, true)
            }
            None => (
                "generic".to_string(),
                "通用 Git 服务".to_string(),
                PlatformUser {
                    login: requested_username.clone(),
                    name: None,
                    email: None,
                    expires_at: None,
                },
                false,
            ),
        };
        let label = service_name.trim();
        let service_name = if label.is_empty() {
            detected_name
        } else {
            validate_service_name(label)?
        };
        let platform = format!("custom:{host}");
        let input = format!(
            "protocol={protocol}\nhost={host}\nusername={}\npassword={}\n\n",
            verified_user.login,
            credential.trim()
        );
        let out = run_output_with_input(
            &git,
            &["credential-manager", "store", "--no-ui"],
            &input,
            Duration::from_secs(15),
        )?;
        if !out.status.success() {
            return Err(command_error("无法保存账号令牌", &out));
        }

        let mut profiles = load_account_profiles();
        if let Some(profile) = profiles.iter_mut().find(|profile| {
            profile.platform == platform
                && profile.username.eq_ignore_ascii_case(&verified_user.login)
        }) {
            profile.authenticated = true;
            profile.token_verified = token_verified;
            profile.expires_at = verified_user.expires_at;
            profile.service_name = Some(service_name);
            profile.base_url = Some(base_url);
            profile.provider = Some(provider);
            if profile.display_name.is_none() {
                profile.display_name = verified_user.name;
            }
            if profile.email.is_none() {
                profile.email = verified_user.email;
            }
        } else {
            profiles.push(GitAccountProfile {
                platform,
                username: verified_user.login,
                display_name: verified_user.name,
                email: verified_user.email,
                expires_at: verified_user.expires_at,
                authenticated: true,
                token_verified,
                service_name: Some(service_name),
                base_url: Some(base_url),
                provider: Some(provider),
            });
        }
        save_account_profiles(&profiles)?;
        account_profiles()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_account_remove_token(
    platform: String,
    username: String,
) -> Result<Vec<GitAccountProfile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let git = ensure_git()?;
        ensure_gcm(&git)?;
        let platform = normalize_platform(&platform)?;
        let username = normalize_account_username(&platform, &username)?;
        let saved_profile = load_account_profiles().into_iter().find(|profile| {
            profile.platform == platform && profile.username.eq_ignore_ascii_case(&username)
        });
        let host = saved_profile
            .as_ref()
            .map(profile_host)
            .unwrap_or_else(|| platform_host(&platform));
        let protocol = saved_profile
            .as_ref()
            .map(profile_protocol)
            .unwrap_or("https");
        let input = format!("protocol={protocol}\nhost={host}\nusername={username}\n\n");
        let out = run_output_with_input(
            &git,
            &["credential-manager", "erase", "--no-ui"],
            &input,
            Duration::from_secs(15),
        )?;
        if !out.status.success() {
            return Err(command_error("无法从本机移除账号令牌", &out));
        }

        if platform == "github" || platform == "gitee" {
            let mut accounts = load_token_accounts(&platform);
            accounts.retain(|account| !account.eq_ignore_ascii_case(&username));
            save_token_accounts(&platform, &accounts)?;
        }
        let mut profiles = load_account_profiles();
        profiles.retain(|profile| {
            profile.platform != platform || !profile.username.eq_ignore_ascii_case(&username)
        });
        save_account_profiles(&profiles)?;
        account_profiles()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_account_profiles() -> Result<Vec<GitAccountProfile>, String> {
    tauri::async_runtime::spawn_blocking(account_profiles)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_account_save_identity(
    platform: String,
    username: String,
    display_name: String,
    email: String,
) -> Result<Vec<GitAccountProfile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let platform = normalize_platform(&platform)?;
        let username = normalize_account_username(&platform, &username)?;
        ensure_account_exists(&platform, &username)?;
        let display_name = validate_identity(&display_name, &email)?;
        let email = email.trim().to_string();

        let mut profiles = load_account_profiles();
        if let Some(profile) = profiles.iter_mut().find(|profile| {
            profile.platform == platform && profile.username.eq_ignore_ascii_case(&username)
        }) {
            profile.display_name = Some(display_name);
            profile.email = Some(email);
        } else {
            profiles.push(GitAccountProfile {
                platform,
                username,
                display_name: Some(display_name),
                email: Some(email),
                expires_at: None,
                authenticated: true,
                token_verified: true,
                service_name: None,
                base_url: None,
                provider: None,
            });
        }
        save_account_profiles(&profiles)?;
        account_profiles()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_account_set_global(platform: String, username: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = resolve_account_profile(&platform, &username)?;
        let display_name = profile
            .display_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or("该账号尚未配置提交姓名，请先编辑账号身份。")?;
        let email = profile
            .email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or("该账号尚未配置提交邮箱，请先编辑账号身份。")?;
        validate_identity(display_name, email)?;
        git_config_set("user.name", display_name)?;
        git_config_set("user.email", email)?;
        git_config_set(
            &format!(
                "credential.{}://{}.username",
                profile_protocol(&profile),
                profile_host(&profile)
            ),
            &profile.username,
        )?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_account_ai_context(platform: String, username: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = resolve_account_profile(&platform, &username)?;
        Ok(build_account_ai_context(&profile))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn build_account_ai_context(profile: &GitAccountProfile) -> String {
    let platform_name = profile_label(profile);
    let repository_pattern = account_repository_pattern(profile);
    [
        "## Git 账号操作摘要".to_string(),
        format!("- 平台：{platform_name}"),
        format!("- 目标账号：{}", profile.username),
        format!(
            "- 提交身份：{} <{}>",
            profile.display_name.as_deref().unwrap_or("未配置"),
            profile.email.as_deref().unwrap_or("未配置")
        ),
        format!("- HTTPS 仓库格式：{repository_pattern}"),
        "- 认证方式：令牌已保存在系统凭据中；不要读取、输出、复制或索要访问令牌。".into(),
        "- 适用场景：当用户明确要求使用该账号时，用它操作当前工程或目标仓库。".into(),
        "- 可执行操作：在令牌权限范围内 clone、fetch、pull、commit、push、管理分支和标签；创建仓库、PR、Issue、Release 需先确认令牌权限。".into(),
        String::new(),
        "## 操作要求".into(),
        "1. 先确认当前工作目录就是用户要操作的工程根目录。".into(),
        "2. 开始前检查：git status --short --branch、git remote -v、git config --get user.name、git config --get user.email。".into(),
        format!(
            "3. 需要使用账号 {}；如果远程仓库所有者、提交身份或用户要求不一致，先停止并说明。",
            profile.username
        ),
        "4. 如果项目尚未初始化或没有远程仓库，先确认仓库名称、可见性和默认分支，不要自行猜测。".into(),
        "5. 默认只提交当前任务涉及的文件；未经明确授权，不得 force push、改写历史、删除分支、迁移或删除仓库。".into(),
        "6. 完成后报告分支、提交哈希、远程地址、推送结果和未完成事项。".into(),
    ]
    .join("\n")
}

fn build_account_shell_intro(profile: &GitAccountProfile) -> Vec<String> {
    let platform_name = profile_label(profile);
    let repository_pattern = account_repository_pattern(profile);
    let identity = match (profile.display_name.as_deref(), profile.email.as_deref()) {
        (Some(name), Some(email)) if !name.is_empty() && !email.is_empty() => {
            format!("{name} / {email}")
        }
        _ => "not configured".to_string(),
    };
    vec![
        "Git account environment".to_string(),
        format!("Platform: {platform_name}"),
        format!("Account: {}", profile.username),
        format!("Commit identity: {identity}"),
        "Credential: saved in Windows Credential Manager; token is not exposed.".to_string(),
        format!("Repository URL pattern: {repository_pattern}"),
        "Scope: this terminal only; global Git account settings are not modified.".to_string(),
        "Suggested checks: git config --get user.name && git config --get user.email && git remote -v".to_string(),
        String::new(),
    ]
}

fn account_repository_pattern(profile: &GitAccountProfile) -> String {
    let encoded_username = encode_url_userinfo(&profile.username);
    let authenticated_base =
        profile_base_url(profile).replacen("://", &format!("://{encoded_username}@"), 1);
    if profile.provider.as_deref() == Some("aliyun-codeup") {
        format!("{authenticated_base}/<组织或代码组>/<仓库名>.git")
    } else {
        format!("{authenticated_base}/{}/<仓库名>.git", profile.username)
    }
}

fn encode_url_userinfo(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[tauri::command]
pub async fn git_account_open_shell(
    platform: String,
    username: String,
    kind: String,
    cwd: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = resolve_account_profile(&platform, &username)?;
        let env = account_environment(&profile);
        let title = format!("{} · {}", profile_label(&profile), profile.username);
        let intro = build_account_shell_intro(&profile);
        crate::installer::open_scoped_shell_with_intro(&kind, cwd.as_deref(), &title, &env, &intro)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_init_repository(
    window: tauri::Window,
    request: GitInitRequest,
) -> Result<GitInitResult, String> {
    tauri::async_runtime::spawn_blocking(move || init_repository_impl(&window, request))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_auto_migrate_repository(
    window: tauri::Window,
    request: GitAutoMigrateRequest,
) -> Result<GitAutoMigrateResult, String> {
    tauri::async_runtime::spawn_blocking(move || auto_migrate_repository_impl(&window, request))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_apply_proxy() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        ensure_git()?;
        let (host, port) = crate::settings::proxy_addr();
        let http = format!("http://{host}:{port}");
        git_config_set("http.proxy", &http)?;
        git_config_set("https.proxy", &http)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_clear_proxy() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        ensure_git()?;
        git_config_unset("http.proxy")?;
        git_config_unset("https.proxy")?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn status_snapshot() -> GitStatus {
    let git = crate::env::resolve_fresh("git.exe");
    let version = git
        .as_deref()
        .and_then(|p| run_program(p, &["--version"], Duration::from_secs(5)).ok());
    let gcm = git.as_deref().and_then(gcm_version).is_some();
    GitStatus {
        installed: git.is_some() && version.is_some(),
        version,
        path: git.as_ref().map(|p| p.to_string_lossy().into_owned()),
        bash_path: crate::installer::git_bash(),
        user_name: git_config_get("user.name"),
        user_email: git_config_get("user.email"),
        default_branch: git_config_get("init.defaultBranch"),
        autocrlf: git_config_get("core.autocrlf"),
        credential_helper: git_config_get_effective("credential.helper"),
        http_proxy: git_config_get("http.proxy"),
        https_proxy: git_config_get("https.proxy"),
        gcm,
    }
}

fn github_accounts_state() -> Result<GitHubAccountsState, String> {
    let git = ensure_git()?;
    let version = ensure_gcm(&git)?;
    Ok(GitHubAccountsState {
        gcm_available: true,
        gcm_version: Some(version),
        accounts: load_github_token_accounts(),
        default_account: None,
    })
}

fn github_token_accounts_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("git-github-token-accounts.json")
}

fn load_github_token_accounts() -> Vec<String> {
    let mut accounts = std::fs::read_to_string(github_token_accounts_path())
        .ok()
        .and_then(|text| serde_json::from_str::<GitHubTokenAccountsFile>(&text).ok())
        .unwrap_or_default()
        .accounts;
    accounts.retain(|account| validate_github_username(account).is_ok());
    accounts.sort_by_key(|account| account.to_ascii_lowercase());
    accounts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    accounts
}

fn save_github_token_accounts(accounts: &[String]) -> Result<(), String> {
    let path = github_token_accounts_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&GitHubTokenAccountsFile {
        accounts: accounts.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn gitee_accounts_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("git-gitee-accounts.json")
}

fn load_gitee_accounts() -> Vec<String> {
    let mut accounts = std::fs::read_to_string(gitee_accounts_path())
        .ok()
        .and_then(|text| serde_json::from_str::<GiteeAccountsFile>(&text).ok())
        .unwrap_or_default()
        .accounts;
    accounts.retain(|account| normalize_gitee_username(account).is_ok());
    accounts.sort_by_key(|account| account.to_ascii_lowercase());
    accounts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    accounts
}

fn save_gitee_accounts(accounts: &[String]) -> Result<(), String> {
    let path = gitee_accounts_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = GiteeAccountsFile {
        accounts: accounts.to_vec(),
    };
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn load_token_accounts(platform: &str) -> Vec<String> {
    if platform == "github" {
        load_github_token_accounts()
    } else {
        load_gitee_accounts()
    }
}

fn save_token_accounts(platform: &str, accounts: &[String]) -> Result<(), String> {
    if platform == "github" {
        save_github_token_accounts(accounts)
    } else {
        save_gitee_accounts(accounts)
    }
}

fn account_profiles_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stacker")
        .join("git-account-profiles.json")
}

fn load_account_profiles() -> Vec<GitAccountProfile> {
    std::fs::read_to_string(account_profiles_path())
        .ok()
        .and_then(|text| serde_json::from_str::<GitAccountProfilesFile>(&text).ok())
        .unwrap_or_default()
        .accounts
}

fn save_account_profiles(accounts: &[GitAccountProfile]) -> Result<(), String> {
    let path = account_profiles_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&GitAccountProfilesFile {
        accounts: accounts.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn account_profiles() -> Result<Vec<GitAccountProfile>, String> {
    let git = ensure_git()?;
    let metadata = load_account_profiles();
    let github = load_github_token_accounts();
    let gitee = load_gitee_accounts();
    let mut profiles = Vec::new();

    for (platform, usernames) in [("github", github), ("gitee", gitee)] {
        for username in usernames {
            let saved = metadata.iter().find(|profile| {
                profile.platform == platform && profile.username.eq_ignore_ascii_case(&username)
            });
            profiles.push(GitAccountProfile {
                platform: platform.into(),
                authenticated: account_credential_exists_raw(
                    &git,
                    "https",
                    platform_host(platform).as_str(),
                    &username,
                ),
                username,
                display_name: saved.and_then(|profile| profile.display_name.clone()),
                email: saved.and_then(|profile| profile.email.clone()),
                expires_at: saved.and_then(|profile| profile.expires_at.clone()),
                token_verified: true,
                service_name: None,
                base_url: None,
                provider: None,
            });
        }
    }
    for saved in metadata
        .iter()
        .filter(|profile| profile.platform.starts_with("custom:"))
    {
        let mut profile = saved.clone();
        profile.authenticated = account_credential_exists(&git, &profile);
        profiles.push(profile);
    }
    profiles.sort_by_key(|profile| {
        format!(
            "{}:{}",
            profile.platform,
            profile.username.to_ascii_lowercase()
        )
    });
    Ok(profiles)
}

fn account_credential_exists(git: &Path, profile: &GitAccountProfile) -> bool {
    account_credential_exists_raw(
        git,
        profile_protocol(profile),
        &profile_host(profile),
        &profile.username,
    )
}

fn account_credential_exists_raw(git: &Path, protocol: &str, host: &str, username: &str) -> bool {
    let input = format!("protocol={protocol}\nhost={host}\nusername={username}\n\n");
    run_output_with_input(
        git,
        &["credential-manager", "get", "--no-ui"],
        &input,
        Duration::from_secs(5),
    )
    .ok()
    .filter(|output| output.status.success())
    .and_then(|output| {
        output_text(&output)
            .lines()
            .find_map(|line| line.strip_prefix("password="))
            .map(str::to_string)
    })
    .is_some_and(|password| !password.is_empty())
}

fn resolve_account_profile(platform: &str, username: &str) -> Result<GitAccountProfile, String> {
    let platform = normalize_platform(platform)?;
    let username = normalize_account_username(&platform, username)?;
    let profile = account_profiles()?
        .into_iter()
        .find(|profile| {
            profile.platform == platform && profile.username.eq_ignore_ascii_case(&username)
        })
        .ok_or_else(|| "该账号尚未完成授权。".to_string())?;
    if !profile.authenticated {
        return Err("该账号的本机凭据已失效，请重新添加访问令牌。".into());
    }
    Ok(profile)
}

fn ensure_account_exists(platform: &str, username: &str) -> Result<(), String> {
    resolve_account_profile(platform, username).map(|_| ())
}

fn normalize_platform(platform: &str) -> Result<String, String> {
    let platform = platform.trim().to_ascii_lowercase();
    match platform.as_str() {
        "github" => Ok("github".into()),
        "gitee" => Ok("gitee".into()),
        value if value.starts_with("custom:") => {
            let host = value.trim_start_matches("custom:");
            validate_service_host(host)?;
            Ok(format!("custom:{host}"))
        }
        _ => Err("暂不支持该 Git 托管平台。".into()),
    }
}

fn normalize_account_username(platform: &str, username: &str) -> Result<String, String> {
    if platform == "github" {
        let username = username.trim();
        validate_github_username(username)?;
        Ok(username.to_string())
    } else if platform == "gitee" {
        normalize_gitee_username(username)
    } else {
        normalize_generic_username(username)
    }
}

fn platform_label(platform: &str) -> String {
    if platform == "github" {
        "GitHub".into()
    } else if platform == "gitee" {
        "Gitee".into()
    } else {
        "其他 Git 服务".into()
    }
}

fn platform_host(platform: &str) -> String {
    if platform == "github" {
        "github.com".into()
    } else if platform == "gitee" {
        "gitee.com".into()
    } else {
        platform.trim_start_matches("custom:").to_string()
    }
}

fn profile_label(profile: &GitAccountProfile) -> String {
    profile
        .service_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| platform_label(&profile.platform))
}

fn profile_base_url(profile: &GitAccountProfile) -> String {
    profile
        .base_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("https://{}", platform_host(&profile.platform)))
}

fn profile_host(profile: &GitAccountProfile) -> String {
    profile
        .base_url
        .as_deref()
        .and_then(|value| normalize_service_url(value).ok().map(|(_, host, _)| host))
        .unwrap_or_else(|| platform_host(&profile.platform))
}

fn profile_protocol(profile: &GitAccountProfile) -> &'static str {
    if profile
        .base_url
        .as_deref()
        .is_some_and(|value| value.trim().to_ascii_lowercase().starts_with("http://"))
    {
        "http"
    } else {
        "https"
    }
}

fn validate_identity(display_name: &str, email: &str) -> Result<String, String> {
    let display_name = display_name.trim();
    let email = email.trim();
    if display_name.is_empty() || display_name.chars().any(char::is_control) {
        return Err("请填写有效的提交姓名。".into());
    }
    if email.is_empty()
        || !email.contains('@')
        || email
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("请填写有效的提交邮箱。".into());
    }
    Ok(display_name.to_string())
}

fn account_environment(profile: &GitAccountProfile) -> Vec<(String, String)> {
    let mut entries = vec![(
        format!(
            "credential.{}://{}.username",
            profile_protocol(profile),
            profile_host(profile)
        ),
        profile.username.clone(),
    )];
    if let Some(display_name) = profile
        .display_name
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        entries.push(("user.name".into(), display_name.into()));
    }
    if let Some(email) = profile.email.as_deref().filter(|value| !value.is_empty()) {
        entries.push(("user.email".into(), email.into()));
    }

    let mut environment = vec![
        ("STACKER_GIT_PLATFORM".into(), profile.platform.clone()),
        ("STACKER_GIT_ACCOUNT".into(), profile.username.clone()),
        ("GIT_CONFIG_COUNT".into(), entries.len().to_string()),
    ];
    for (index, (key, value)) in entries.into_iter().enumerate() {
        environment.push((format!("GIT_CONFIG_KEY_{index}"), key));
        environment.push((format!("GIT_CONFIG_VALUE_{index}"), value));
    }
    environment
}

fn apply_account_environment(command: &mut Command, profile: &GitAccountProfile) {
    for (key, value) in account_environment(profile) {
        command.env(key, value);
    }
}

fn validate_directory(directory: &str) -> Result<PathBuf, String> {
    let directory = PathBuf::from(directory.trim());
    if directory.as_os_str().is_empty() || !directory.is_dir() {
        return Err("请选择有效的本地目录。".into());
    }
    directory
        .canonicalize()
        .map_err(|e| format!("无法访问所选目录：{e}"))
}

fn validate_repository_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 100
        || name.starts_with('.')
        || name.ends_with('.')
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("仓库名称仅支持字母、数字、短横线、下划线和点。".into());
    }
    Ok(name.to_string())
}

fn validate_remote_url(profile: &GitAccountProfile, url: &str) -> Result<String, String> {
    let url = url.trim();
    let prefix = format!("{}/", profile_base_url(profile));
    if !url.to_ascii_lowercase().starts_with(&prefix) || url.chars().any(char::is_control) {
        return Err(format!("仓库地址必须是 {prefix} 开头的 HTTPS 地址。"));
    }
    Ok(url.to_string())
}

fn init_repository_impl(
    window: &tauri::Window,
    request: GitInitRequest,
) -> Result<GitInitResult, String> {
    let git = ensure_git()?;
    let platform = normalize_platform(&request.platform)?;
    let mut profile = resolve_account_profile(&platform, &request.username)?;
    profile.display_name = Some(validate_identity(&request.display_name, &request.email)?);
    profile.email = Some(request.email.trim().to_string());
    let directory = validate_directory(&request.directory)?;
    let repository_name = validate_repository_name(&request.repository_name)?;
    if directory.join(".git").exists() {
        return Err("所选目录已经是 Git 仓库，请选择尚未初始化的项目目录。".into());
    }

    emit_git_progress(window, "正在初始化本地 Git 仓库…");
    run_git_scoped(
        &git,
        &["init", "-b", "main"],
        &directory,
        &profile,
        Duration::from_secs(30),
    )?;
    run_git_scoped(
        &git,
        &[
            "config",
            "--local",
            "user.name",
            request.display_name.trim(),
        ],
        &directory,
        &profile,
        Duration::from_secs(10),
    )?;
    run_git_scoped(
        &git,
        &["config", "--local", "user.email", request.email.trim()],
        &directory,
        &profile,
        Duration::from_secs(10),
    )?;

    let remote_url = if request.create_remote {
        emit_git_progress(window, "正在使用所选账号创建远程仓库…");
        Some(create_platform_repository(
            &git,
            &profile,
            &repository_name,
            &request.description,
            request.private_repository,
        )?)
    } else {
        request
            .remote_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .map(|url| validate_remote_url(&profile, url))
            .transpose()?
    };

    if let Some(remote) = remote_url.as_deref() {
        emit_git_progress(window, "正在配置远程仓库地址…");
        run_git_scoped(
            &git,
            &["remote", "add", "origin", remote],
            &directory,
            &profile,
            Duration::from_secs(10),
        )?;
    }

    let mut initial_commit = false;
    if request.create_readme {
        emit_git_progress(window, "正在创建 README 和初始提交…");
        let readme = directory.join("README.md");
        if !readme.exists() {
            std::fs::write(&readme, format!("# {repository_name}\n"))
                .map_err(|e| format!("创建 README 失败：{e}"))?;
        }
        run_git_scoped(
            &git,
            &["add", "README.md"],
            &directory,
            &profile,
            Duration::from_secs(10),
        )?;
        run_git_scoped(
            &git,
            &["commit", "-m", "Initial commit"],
            &directory,
            &profile,
            Duration::from_secs(30),
        )?;
        initial_commit = true;

        if remote_url.is_some() {
            emit_git_progress(window, "正在推送初始提交…");
            run_git_scoped(
                &git,
                &["push", "-u", "origin", "main"],
                &directory,
                &profile,
                Duration::from_secs(300),
            )?;
        }
    }

    emit_git_progress(window, "__done__");
    Ok(GitInitResult {
        directory: directory.to_string_lossy().into_owned(),
        remote_url,
        initial_commit,
    })
}

fn github_transfer_impl(
    window: &tauri::Window,
    request: GitHubTransferRequest,
) -> Result<String, String> {
    let git = ensure_git()?;
    let source = resolve_account_profile("github", &request.source_account)?;
    let target_account = normalize_account_username("github", &request.target_account)?;
    if source.username.eq_ignore_ascii_case(&target_account) {
        return Err("源账号和目标账号不能相同。".into());
    }
    let owner = normalize_account_username("github", &request.source_owner)?;
    let repository = validate_repository_name(&request.repository)?;
    let token = account_credential(&git, &source)?;
    emit_git_progress(window, "正在向 GitHub 提交仓库转移请求…");
    let endpoint = format!("https://api.github.com/repos/{owner}/{repository}/transfer");
    let mut payload = serde_json::json!({ "new_owner": target_account });
    if let Some(target_repository) = request
        .target_repository
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        payload["new_name"] =
            serde_json::Value::String(validate_repository_name(target_repository)?);
    }
    let body = payload.to_string();
    let response = api_post(&endpoint, &token, "github", &body)?;
    if response.status() != 202 {
        return Err(format!(
            "GitHub 未接受仓库转移请求（HTTP {}）。",
            response.status()
        ));
    }
    emit_git_progress(window, "__done__");
    Ok(format!(
        "已提交 {owner}/{repository} 的转移请求。目标账号 {} 需要按 GitHub 通知确认接收。",
        target_account
    ))
}

fn auto_migrate_repository_impl(
    window: &tauri::Window,
    request: GitAutoMigrateRequest,
) -> Result<GitAutoMigrateResult, String> {
    let source = resolve_account_profile(&request.source_platform, &request.source_account)?;
    let target = resolve_account_profile(&request.target_platform, &request.target_account)?;
    let source_owner = normalize_account_username(&source.platform, &request.source_owner)?;
    let source_repository = validate_repository_name(&request.source_repository)?;
    let target_repository = validate_repository_name(&request.target_repository)?;

    let same_owner =
        source.platform == target.platform && source_owner.eq_ignore_ascii_case(&target.username);
    if same_owner && source_repository.eq_ignore_ascii_case(&target_repository) {
        return Err("同一账号下，源仓库和目标仓库名称不能相同。".into());
    }

    let use_native_transfer = should_use_native_transfer(
        &source.platform,
        &target.platform,
        &source_owner,
        &target.username,
    );

    if use_native_transfer {
        let message = github_transfer_impl(
            window,
            GitHubTransferRequest {
                source_account: source.username,
                source_owner,
                repository: source_repository,
                target_account: target.username,
                target_repository: Some(target_repository),
            },
        )?;
        return Ok(GitAutoMigrateResult {
            mode: "native_transfer".into(),
            message,
        });
    }

    let git = ensure_git()?;
    emit_git_progress(window, "正在使用目标账号创建空仓库…");
    let target_url = create_platform_repository(
        &git,
        &target,
        &target_repository,
        &format!(
            "Migrated by Stacker from {}/{}",
            source_owner, source_repository
        ),
        request.target_private,
    )?;
    let source_url = format!(
        "{}/{}/{}.git",
        profile_base_url(&source),
        source_owner,
        source_repository
    );
    let message = mirror_repository_impl(
        window,
        GitMirrorRequest {
            source_platform: source.platform,
            source_account: source.username,
            source_url,
            target_platform: target.platform,
            target_account: target.username,
            target_url,
            include_lfs: request.include_lfs,
        },
    )
    .map_err(|error| format!("目标仓库已创建，但镜像迁移未完成：{error}"))?;
    Ok(GitAutoMigrateResult {
        mode: "git_mirror".into(),
        message,
    })
}

fn should_use_native_transfer(
    source_platform: &str,
    target_platform: &str,
    source_owner: &str,
    target_owner: &str,
) -> bool {
    source_platform == "github"
        && target_platform == "github"
        && !source_owner.eq_ignore_ascii_case(target_owner)
}

fn mirror_repository_impl(
    window: &tauri::Window,
    request: GitMirrorRequest,
) -> Result<String, String> {
    let git = ensure_git()?;
    let source = resolve_account_profile(&request.source_platform, &request.source_account)?;
    let target = resolve_account_profile(&request.target_platform, &request.target_account)?;
    let source_url = validate_remote_url(&source, &request.source_url)?;
    let target_url = validate_remote_url(&target, &request.target_url)?;
    if source_url.eq_ignore_ascii_case(&target_url) {
        return Err("源仓库和目标仓库地址不能相同。".into());
    }

    let root = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("stacker")
        .join("git-migrations");
    std::fs::create_dir_all(&root).map_err(|e| format!("创建迁移缓存失败：{e}"))?;
    let session = root.join(format!(
        "{}-{}",
        chrono::Local::now().format("%Y%m%d%H%M%S%3f"),
        std::process::id()
    ));
    if !session.starts_with(&root) {
        return Err("迁移缓存路径校验失败。".into());
    }

    let operation = (|| {
        emit_git_progress(window, "正在完整克隆源仓库的分支和标签…");
        run_git_scoped(
            &git,
            &[
                "clone",
                "--mirror",
                &source_url,
                session.to_string_lossy().as_ref(),
            ],
            &root,
            &source,
            Duration::from_secs(1800),
        )?;

        if request.include_lfs {
            emit_git_progress(window, "正在拉取 Git LFS 对象…");
            run_git_scoped(
                &git,
                &["lfs", "fetch", "--all"],
                &session,
                &source,
                Duration::from_secs(1800),
            )?;
        }

        emit_git_progress(window, "正在向目标仓库推送全部 Git 引用…");
        run_git_scoped(
            &git,
            &["push", "--mirror", &target_url],
            &session,
            &target,
            Duration::from_secs(1800),
        )?;

        if request.include_lfs {
            emit_git_progress(window, "正在向目标仓库推送 Git LFS 对象…");
            run_git_scoped(
                &git,
                &["lfs", "push", "--all", &target_url],
                &session,
                &target,
                Duration::from_secs(1800),
            )?;
        }
        Ok::<(), String>(())
    })();

    let _ = std::fs::remove_dir_all(&session);
    operation?;
    emit_git_progress(window, "__done__");
    Ok("代码、分支和标签迁移完成。Issues、PR、流水线及平台设置不包含在镜像迁移中。".into())
}

fn create_platform_repository(
    git: &Path,
    profile: &GitAccountProfile,
    name: &str,
    description: &str,
    private_repository: bool,
) -> Result<String, String> {
    let token = account_credential(git, profile)?;
    let provider = profile.provider.as_deref().unwrap_or(&profile.platform);
    let base_url = profile_base_url(profile);
    let (endpoint, body) = match provider {
        "github" => (
            "https://api.github.com/user/repos".to_string(),
            serde_json::json!({
                "name": name,
                "description": description.trim(),
                "private": private_repository,
                "auto_init": false
            })
            .to_string(),
        ),
        "gitee" => (
            "https://gitee.com/api/v5/user/repos".to_string(),
            serde_json::json!({
                "name": name,
                "description": description.trim(),
                "private": private_repository,
                "auto_init": false
            })
            .to_string(),
        ),
        "github-enterprise" => (
            format!("{base_url}/api/v3/user/repos"),
            serde_json::json!({
                "name": name,
                "description": description.trim(),
                "private": private_repository,
                "auto_init": false
            })
            .to_string(),
        ),
        "gitlab" => (
            format!("{base_url}/api/v4/projects"),
            serde_json::json!({
                "name": name,
                "path": name,
                "description": description.trim(),
                "visibility": if private_repository { "private" } else { "public" },
                "initialize_with_readme": false
            })
            .to_string(),
        ),
        "gitea" | "forgejo" => (
            format!("{base_url}/api/v1/user/repos"),
            serde_json::json!({
                "name": name,
                "description": description.trim(),
                "private": private_repository,
                "auto_init": false
            })
            .to_string(),
        ),
        _ => {
            return Err(
                "该服务未提供可识别的建库接口。请先在服务端创建仓库，再填写已有远程仓库地址。"
                    .into(),
            )
        }
    };
    let response = api_post(&endpoint, &token, provider, &body)?;
    let status = response.status();
    let text = response
        .into_string()
        .map_err(|e| format!("读取平台响应失败：{e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("远程仓库创建失败（HTTP {status}）：{text}"));
    }
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析平台响应失败：{e}"))?;
    value
        .get("clone_url")
        .or_else(|| value.get("http_url_to_repo"))
        .or_else(|| value.get("html_url"))
        .and_then(|value| value.as_str())
        .map(|url| {
            if url.ends_with(".git") {
                url.to_string()
            } else {
                format!("{url}.git")
            }
        })
        .ok_or_else(|| "平台已创建仓库，但未返回 HTTPS 仓库地址。".into())
}

fn api_post(
    endpoint: &str,
    token: &str,
    platform: &str,
    body: &str,
) -> Result<ureq::Response, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .build();
    let authorization = if platform == "github" || platform == "github-enterprise" {
        format!("Bearer {token}")
    } else {
        format!("token {token}")
    };
    match agent
        .post(endpoint)
        .set("Authorization", &authorization)
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .set("User-Agent", "Stacker")
        .send_string(body)
    {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            Err(format!("平台拒绝了请求（HTTP {code}）：{detail}"))
        }
        Err(error) => Err(format!("连接代码托管平台失败：{error}")),
    }
}

fn normalize_service_url(value: &str) -> Result<(String, String, &'static str), String> {
    let value = value.trim().trim_end_matches('/');
    let lower = value.to_ascii_lowercase();
    let (protocol, rest) = if lower.starts_with("https://") {
        ("https", &value[8..])
    } else if lower.starts_with("http://") {
        ("http", &value[7..])
    } else {
        return Err("服务地址必须以 https:// 或 http:// 开头。".into());
    };
    if rest.is_empty() || rest.contains(['?', '#', '\\']) {
        return Err("请填写有效的 Git 服务地址。".into());
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        return Err("服务地址中不能包含账号或密码。".into());
    }
    let host = authority.to_ascii_lowercase();
    validate_service_host(&host)?;
    Ok((format!("{protocol}://{rest}"), host, protocol))
}

fn validate_service_host(host: &str) -> Result<(), String> {
    if host.is_empty()
        || host.len() > 255
        || host.chars().any(|ch| {
            ch.is_control()
                || ch.is_whitespace()
                || !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '[' | ']'))
        })
    {
        return Err("Git 服务主机地址格式不正确。".into());
    }
    Ok(())
}

fn validate_service_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 60
        || value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\r' | '\n'))
    {
        return Err("服务名称格式不正确。".into());
    }
    Ok(value.to_string())
}

fn normalize_generic_username(username: &str) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty()
        || username.len() > 128
        || username
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '/' | '\\'))
    {
        return Err("账号名称格式不正确。".into());
    }
    Ok(username.to_string())
}

fn custom_api_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(12))
        .timeout_write(Duration::from_secs(12))
        .build()
}

fn custom_service_hint(base_url: &str) -> Option<String> {
    if is_aliyun_codeup(base_url) {
        return Some("aliyun-codeup".into());
    }
    let lower_url = base_url.to_ascii_lowercase();
    let root = custom_api_agent()
        .get(base_url)
        .set("User-Agent", "Stacker")
        .call()
        .ok();
    if let Some(response) = root {
        if response.header("X-GitLab-Meta").is_some() {
            return Some("gitlab".into());
        }
        let body = response
            .into_string()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if let Some(provider) = classify_custom_service_page(&body) {
            return Some(provider.into());
        }
    }
    let version_url = format!("{base_url}/api/v1/version");
    if custom_api_agent()
        .get(&version_url)
        .set("User-Agent", "Stacker")
        .call()
        .is_ok()
    {
        return Some("gitea".into());
    }
    if lower_url.contains("gitlab") {
        return Some("gitlab".into());
    }
    None
}

fn is_aliyun_codeup(base_url: &str) -> bool {
    normalize_service_url(base_url)
        .map(|(_, host, _)| host.eq_ignore_ascii_case("codeup.aliyun.com"))
        .unwrap_or(false)
}

fn classify_custom_service_page(body: &str) -> Option<&'static str> {
    let body = body.to_ascii_lowercase();
    if body.contains("powered by forgejo")
        || body.contains("content=\"forgejo")
        || body.contains("content='forgejo")
    {
        return Some("forgejo");
    }
    if body.contains("powered by gitea")
        || body.contains("content=\"gitea - git with a cup of tea")
        || body.contains("content='gitea - git with a cup of tea")
        || body.contains("gitea (git with a cup of tea)")
    {
        return Some("gitea");
    }
    if body.contains("github enterprise") {
        return Some("github-enterprise");
    }
    if body.contains("x-gitlab-meta")
        || body.contains("content=\"gitlab")
        || body.contains("content='gitlab")
    {
        return Some("gitlab");
    }
    None
}

fn verify_custom_service_token(
    base_url: &str,
    requested_username: &str,
    token: &str,
) -> Result<Option<CustomServiceVerification>, String> {
    let Some(provider) = custom_service_hint(base_url) else {
        return Ok(None);
    };
    let (endpoint, auth_header, auth_value, service_name) = match provider.as_str() {
        "aliyun-codeup" => (
            "https://openapi-rdc.aliyuncs.com/oapi/v1/platform/user".to_string(),
            "X-Yunxiao-Token",
            token.to_string(),
            "云效 Codeup",
        ),
        "gitlab" => (
            format!("{base_url}/api/v4/user"),
            "PRIVATE-TOKEN",
            token.to_string(),
            "GitLab",
        ),
        "github-enterprise" => (
            format!("{base_url}/api/v3/user"),
            "Authorization",
            format!("Bearer {token}"),
            "GitHub Enterprise",
        ),
        "forgejo" => (
            format!("{base_url}/api/v1/user"),
            "Authorization",
            format!("token {token}"),
            "Forgejo",
        ),
        _ => (
            format!("{base_url}/api/v1/user"),
            "Authorization",
            format!("token {token}"),
            "Gitea",
        ),
    };
    let response = match custom_api_agent()
        .get(&endpoint)
        .set(auth_header, &auth_value)
        .set("Accept", "application/json")
        .set("User-Agent", "Stacker")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(401 | 403, _)) => {
            return Err("令牌（token）无效、已过期或缺少账号读取权限。".into());
        }
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            return Err(format!(
                "服务拒绝验证令牌（token）（HTTP {code}）：{detail}"
            ));
        }
        Err(error) => return Err(format!("连接 Git 服务验证令牌（token）失败：{error}")),
    };
    let content_type = response
        .header("Content-Type")
        .unwrap_or_default()
        .to_string();
    let text = response
        .into_string()
        .map_err(|error| format!("读取账号信息失败：{error}"))?;
    if text.trim().is_empty() {
        return Err("Git 服务返回了空的账号验证结果，请检查服务地址和令牌权限。".into());
    }
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        if text.trim_start().starts_with('<')
            || content_type.to_ascii_lowercase().contains("text/html")
        {
            "Git 服务返回了登录页面而非账号信息，请检查服务地址或令牌类型。".to_string()
        } else {
            format!("解析账号信息失败：{error}")
        }
    })?;
    let mut user = parse_custom_platform_user(&provider, requested_username, &value)?;
    let expires_at = if provider == "gitlab" {
        let endpoint = format!("{base_url}/api/v4/personal_access_tokens/self");
        custom_api_agent()
            .get(&endpoint)
            .set("PRIVATE-TOKEN", token)
            .set("Accept", "application/json")
            .set("User-Agent", "Stacker")
            .call()
            .ok()
            .and_then(|response| response.into_string().ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| {
                value
                    .get("expires_at")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
    } else {
        None
    };
    user.expires_at = expires_at;
    Ok(Some(CustomServiceVerification {
        provider,
        service_name: service_name.to_string(),
        user,
    }))
}

fn parse_custom_platform_user(
    provider: &str,
    requested_username: &str,
    value: &serde_json::Value,
) -> Result<PlatformUser, String> {
    if value.get("success").and_then(|value| value.as_bool()) == Some(false) {
        let code = value
            .get("errorCode")
            .and_then(|value| value.as_str())
            .unwrap_or("UnknownError");
        let message = value
            .get("errorMessage")
            .and_then(|value| value.as_str())
            .unwrap_or("服务未返回具体原因");
        return Err(format!("Git 服务验证失败：{code}：{message}"));
    }
    let account = if provider == "aliyun-codeup" {
        value.get("result").unwrap_or(value)
    } else {
        value
    };
    let login = account
        .get("login")
        .or_else(|| account.get("username"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| (provider == "aliyun-codeup").then(|| requested_username.to_string()))
        .ok_or("服务未返回有效的账号名称。")?;
    let name = account
        .get("name")
        .or_else(|| account.get("full_name"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let email = account
        .get("email")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    Ok(PlatformUser {
        login,
        name,
        email,
        expires_at: None,
    })
}

fn verify_platform_token(platform: &str, token: &str) -> Result<PlatformUser, String> {
    let endpoint = if platform == "github" {
        "https://api.github.com/user"
    } else {
        "https://gitee.com/api/v5/user"
    };
    let authorization = if platform == "github" {
        format!("Bearer {token}")
    } else {
        format!("token {token}")
    };
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .build();
    let response = match agent
        .get(endpoint)
        .set("Authorization", &authorization)
        .set("Accept", "application/json")
        .set("User-Agent", "Stacker")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(401 | 403, _)) => {
            return Err("访问令牌无效、已过期或缺少账号读取权限。".into());
        }
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            return Err(format!("平台拒绝验证访问令牌（HTTP {code}）：{detail}"));
        }
        Err(error) => return Err(format!("无法连接代码托管平台验证访问令牌：{error}")),
    };
    let expires_at = if platform == "github" {
        response
            .header("GitHub-Authentication-Token-Expiration")
            .map(str::to_string)
    } else {
        None
    };
    let text = response
        .into_string()
        .map_err(|e| format!("读取账号验证结果失败：{e}"))?;
    let mut user: PlatformUser =
        serde_json::from_str(&text).map_err(|e| format!("解析账号验证结果失败：{e}"))?;
    if user.login.trim().is_empty() {
        return Err("平台未返回有效的账号信息。".into());
    }
    user.expires_at = expires_at;
    Ok(user)
}

fn account_credential(git: &Path, profile: &GitAccountProfile) -> Result<String, String> {
    let input = format!(
        "protocol={}\nhost={}\nusername={}\n\n",
        profile_protocol(profile),
        profile_host(profile),
        profile.username
    );
    let output = run_output_with_input(
        git,
        &["credential-manager", "get", "--no-ui"],
        &input,
        Duration::from_secs(15),
    )?;
    if !output.status.success() {
        return Err("无法读取该账号的访问令牌，请重新添加账号后再试。".into());
    }
    output_text(&output)
        .lines()
        .find_map(|line| line.strip_prefix("password="))
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "该账号缺少可用访问令牌，请重新添加账号。".into())
}

fn run_git_scoped(
    git: &Path,
    args: &[&str],
    cwd: &Path,
    profile: &GitAccountProfile,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(git);
    command.args(args).current_dir(cwd);
    apply_account_environment(&mut command, profile);
    hide_window(&mut command);
    let output = command_output_timeout_drained(command, timeout)?;
    if output.status.success() {
        Ok(output_text(&output))
    } else {
        Err(command_error("Git 操作失败", &output))
    }
}

fn emit_git_progress(window: &tauri::Window, message: &str) {
    let _ = window.emit("git-operation-progress", message.to_string());
}

fn command_output_timeout_drained(
    mut command: Command,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stdout {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stderr {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(80));
            }
            Ok(None) => {
                terminate_process_tree(pid);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err("Git 操作等待超时。".into());
            }
            Err(error) => {
                terminate_process_tree(pid);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(error.to_string());
            }
        }
    };
    Ok(Output {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

fn ensure_gcm(git: &Path) -> Result<String, String> {
    gcm_version(git).ok_or_else(|| {
        "当前 Git 未包含 Git Credential Manager，请更新 Git for Windows 后重试。".into()
    })
}

fn configure_gcm(git: &Path) -> Result<(), String> {
    let configured = run_output(
        git,
        &["credential-manager", "configure"],
        Duration::from_secs(20),
    )?;
    if configured.status.success() {
        Ok(())
    } else {
        Err(command_error(
            "无法启用 Git Credential Manager",
            &configured,
        ))
    }
}

fn gcm_version(git: &Path) -> Option<String> {
    run_program(
        git,
        &["credential-manager", "--version"],
        Duration::from_secs(5),
    )
    .ok()
    .map(|version| version.split('+').next().unwrap_or(&version).to_string())
}

fn validate_github_username(username: &str) -> Result<(), String> {
    if username.is_empty()
        || username.len() > 39
        || !username
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        || username.starts_with('-')
        || username.ends_with('-')
    {
        return Err("GitHub 用户名格式不正确。".into());
    }
    Ok(())
}

fn normalize_gitee_username(username: &str) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty()
        || username.len() > 128
        || username
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("Gitee 用户名或邮箱格式不正确。".into());
    }
    Ok(username.to_string())
}

fn validate_credential(credential: &str) -> Result<(), String> {
    let credential = credential.trim();
    if credential.is_empty() {
        return Err("请填写访问令牌。".into());
    }
    if credential
        .chars()
        .any(|ch| matches!(ch, '\r' | '\n' | '\0'))
    {
        return Err("访问令牌格式不正确。".into());
    }
    Ok(())
}

fn git_version_token(full: &str) -> Option<String> {
    full.split_whitespace()
        .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|s| s.trim().trim_start_matches('v').to_string())
}

fn latest_git_release(current: String, source_id: &str) -> Result<GitUpdateInfo, String> {
    let mirrors = crate::sources::git_runtime_mirrors();
    let mirror = mirrors
        .iter()
        .find(|mirror| mirror.id == source_id)
        .or_else(|| mirrors.iter().find(|mirror| mirror.id == "official"))
        .ok_or("Git 下载源清单为空")?;

    let result = if mirror.id == "official" {
        match latest_git_release_api(&current) {
            Ok(info) => Ok(info),
            Err(api_error) => latest_git_release_html(&current)
                .map_err(|html_error| format!("GitHub API：{api_error}；官方发布页：{html_error}")),
        }
    } else if mirror.id == "npmmirror" || mirror.url.contains("registry.npmmirror.com") {
        latest_git_release_npmmirror(&current, &mirror.name, &mirror.url)
    } else {
        latest_git_release_mirror_html(&current, &mirror.name, &mirror.url)
    };

    result.map_err(|error| {
        format!(
            "无法从「{}」获取 Git for Windows 最新正式版：{}。请切换下载源后重试",
            mirror.name, error
        )
    })
}

fn git_release_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .build()
}

fn latest_git_release_api(current: &str) -> Result<GitUpdateInfo, String> {
    let response = git_release_agent()
        .get("https://api.github.com/repos/git-for-windows/git/releases/latest")
        .set("User-Agent", "Stacker")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("请求失败：{e}"))?;
    let body = response
        .into_string()
        .map_err(|e| format!("读取响应失败：{e}"))?;
    let release: GitRelease =
        serde_json::from_str(&body).map_err(|e| format!("响应格式错误：{e}"))?;
    let installer = select_git_installer(&release.assets)
        .ok_or("最新正式版未提供适用于当前 Windows 架构的安装程序")?;
    build_git_update_info(
        current,
        &release.tag_name,
        "官方".into(),
        release.html_url,
        installer.browser_download_url.clone(),
    )
}

fn latest_git_release_html(current: &str) -> Result<GitUpdateInfo, String> {
    let response = git_release_agent()
        .get("https://github.com/git-for-windows/git/releases/latest")
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("无法打开最新发布页：{e}"))?;
    let release_url = response.get_url().to_string();
    let tag = release_tag_from_url(&release_url).ok_or("最新发布页未返回有效版本号")?;
    let assets_url =
        format!("https://github.com/git-for-windows/git/releases/expanded_assets/{tag}");
    let assets_html = git_release_agent()
        .get(&assets_url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("无法读取正式版安装文件列表：{e}"))?
        .into_string()
        .map_err(|e| format!("读取正式版安装文件列表失败：{e}"))?;
    let assets = git_release_assets_from_html(&assets_html);
    let installer =
        select_git_installer(&assets).ok_or("官方发布页未提供适用于当前 Windows 架构的安装程序")?;
    build_git_update_info(
        current,
        &tag,
        "官方".into(),
        release_url,
        installer.browser_download_url.clone(),
    )
}

fn latest_git_release_npmmirror(
    current: &str,
    source_name: &str,
    base_url: &str,
) -> Result<GitUpdateInfo, String> {
    let body = git_release_agent()
        .get(base_url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("无法读取版本目录：{e}"))?
        .into_string()
        .map_err(|e| format!("读取版本目录失败：{e}"))?;
    let entries: Vec<GitMirrorEntry> =
        serde_json::from_str(&body).map_err(|e| format!("版本目录格式错误：{e}"))?;
    let latest = entries
        .iter()
        .filter(|entry| entry.r#type == "dir")
        .filter_map(|entry| normalize_stable_git_tag(&entry.name).map(|tag| (tag, entry)))
        .max_by(|(a, _), (b, _)| compare_git_tags(a, b))
        .ok_or("镜像中没有正式版本")?;

    let assets_body = git_release_agent()
        .get(&latest.1.url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("无法读取 {} 的安装文件列表：{e}", latest.0))?
        .into_string()
        .map_err(|e| format!("读取 {} 的安装文件列表失败：{e}", latest.0))?;
    let assets: Vec<GitMirrorEntry> =
        serde_json::from_str(&assets_body).map_err(|e| format!("安装文件列表格式错误：{e}"))?;
    let release_assets = assets
        .into_iter()
        .filter(|entry| entry.r#type == "file")
        .map(|entry| GitReleaseAsset {
            name: entry.name,
            browser_download_url: entry.url,
        })
        .collect::<Vec<_>>();
    let installer = select_git_installer(&release_assets)
        .ok_or("镜像中的最新正式版缺少适用于当前 Windows 架构的安装程序")?;
    build_git_update_info(
        current,
        &latest.0,
        source_name.into(),
        latest.1.url.clone(),
        installer.browser_download_url.clone(),
    )
}

fn latest_git_release_mirror_html(
    current: &str,
    source_name: &str,
    base_url: &str,
) -> Result<GitUpdateInfo, String> {
    let body = git_release_agent()
        .get(base_url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("无法读取版本目录：{e}"))?
        .into_string()
        .map_err(|e| format!("读取版本目录失败：{e}"))?;
    let latest = html_hrefs(&body)
        .into_iter()
        .filter_map(|href| normalize_stable_git_tag(&href).map(|tag| (tag, href)))
        .max_by(|(a, _), (b, _)| compare_git_tags(a, b))
        .ok_or("镜像中没有正式版本")?;
    let release_url = join_mirror_url(base_url, &latest.1)?;
    let assets_html = git_release_agent()
        .get(&release_url)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("无法读取 {} 的安装文件列表：{e}", latest.0))?
        .into_string()
        .map_err(|e| format!("读取 {} 的安装文件列表失败：{e}", latest.0))?;
    let assets = html_hrefs(&assets_html)
        .into_iter()
        .filter_map(|href| {
            let name = href.trim_end_matches('/').rsplit('/').next()?.to_string();
            let browser_download_url = join_mirror_url(&release_url, &href).ok()?;
            Some(GitReleaseAsset {
                name,
                browser_download_url,
            })
        })
        .collect::<Vec<_>>();
    let installer = select_git_installer(&assets)
        .ok_or("镜像中的最新正式版缺少适用于当前 Windows 架构的安装程序")?;
    build_git_update_info(
        current,
        &latest.0,
        source_name.into(),
        release_url,
        installer.browser_download_url.clone(),
    )
}

fn html_hrefs(body: &str) -> Vec<String> {
    body.split("href=\"")
        .skip(1)
        .filter_map(|part| part.find('"').map(|end| part[..end].to_string()))
        .collect()
}

fn normalize_stable_git_tag(value: &str) -> Option<String> {
    let decoded = value
        .replace("%20", " ")
        .replace("%2F", "/")
        .replace("%2f", "/");
    let segment = decoded
        .trim_end_matches('/')
        .rsplit('/')
        .next()?
        .trim()
        .strip_prefix("Git for Windows ")
        .unwrap_or_else(|| {
            decoded
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
        });
    let tag = segment.trim().trim_start_matches('v');
    let (version, build) = tag.split_once(".windows.")?;
    if version.split('.').count() != 3
        || !version
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        || build.is_empty()
        || !build.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    Some(format!("v{version}.windows.{build}"))
}

fn compare_git_tags(a: &str, b: &str) -> std::cmp::Ordering {
    let a = a.trim_start_matches('v');
    let b = b.trim_start_matches('v');
    if crate::update::ver_lt(a, b) {
        std::cmp::Ordering::Less
    } else if crate::update::ver_lt(b, a) {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

fn join_mirror_url(base: &str, href: &str) -> Result<String, String> {
    if href.starts_with("https://") || href.starts_with("http://") {
        return Ok(href.to_string());
    }
    if href.starts_with('/') {
        let scheme_end = base.find("://").ok_or("下载源地址格式无效")? + 3;
        let host_end = base[scheme_end..]
            .find('/')
            .map(|offset| scheme_end + offset)
            .unwrap_or(base.len());
        return Ok(format!("{}{}", &base[..host_end], href));
    }
    Ok(format!(
        "{}/{}",
        base.trim_end_matches('/'),
        href.trim_start_matches("./")
    ))
}

fn build_git_update_info(
    current: &str,
    tag: &str,
    source_name: String,
    release_url: String,
    installer_url: String,
) -> Result<GitUpdateInfo, String> {
    let latest = tag.trim_start_matches('v').trim().to_string();
    if latest.is_empty()
        || !latest
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err("最新版本号格式无效".into());
    }
    Ok(GitUpdateInfo {
        has_update: current == "未安装" || crate::update::ver_lt(current, &latest),
        current: current.to_string(),
        latest,
        source_name,
        release_url,
        installer_url,
    })
}

fn release_tag_from_url(url: &str) -> Option<String> {
    let tag = url
        .split("/releases/tag/")
        .nth(1)?
        .split(['?', '#', '/'])
        .next()?
        .trim();
    (!tag.is_empty()).then(|| tag.to_string())
}

fn git_release_assets_from_html(body: &str) -> Vec<GitReleaseAsset> {
    body.split("href=\"")
        .skip(1)
        .filter_map(|part| {
            let end = part.find('"')?;
            let href = &part[..end];
            if !href.contains("/git-for-windows/git/releases/download/") {
                return None;
            }
            let name = href.rsplit('/').next()?.to_string();
            let url = if href.starts_with("https://") {
                href.to_string()
            } else if href.starts_with('/') {
                format!("https://github.com{href}")
            } else {
                return None;
            };
            Some(GitReleaseAsset {
                name,
                browser_download_url: url,
            })
        })
        .collect()
}

fn select_git_installer(assets: &[GitReleaseAsset]) -> Option<&GitReleaseAsset> {
    let suffix = if cfg!(target_arch = "aarch64") {
        "-arm64.exe"
    } else if cfg!(target_arch = "x86") {
        "-32-bit.exe"
    } else {
        "-64-bit.exe"
    };
    assets.iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name.starts_with("git-") && name.ends_with(suffix)
    })
}

fn install_git_impl(window: tauri::Window, source_id: &str) -> Result<String, String> {
    crate::installer::op_reset();
    let before = status_snapshot();
    let current = before
        .version
        .as_deref()
        .and_then(git_version_token)
        .unwrap_or_else(|| "未安装".into());
    let release = latest_git_release(current, source_id)?;
    let _ = window.emit(
        "install-progress",
        format!(
            "正在连接「{}」下载 Git for Windows · {}",
            release.source_name, release.latest
        ),
    );
    let installer = download_git_installer(&window, &release)?;
    let result = (|| {
        let signer = verify_authenticode_signature(&installer)?;
        let _ = window.emit("install-progress", format!("安装程序签名有效 · {signer}"));
        let system_install = git_install_requires_elevation(&before);
        let _ = window.emit(
            "install-progress",
            if system_install {
                "正在更新系统级 Git；Windows 将请求一次管理员授权…"
            } else {
                "正在为当前用户静默安装 Git，无需管理员授权…"
            },
        );
        run_git_installer(&window, &installer, system_install)?;
        let _ = window.emit("install-progress", "正在验证 Git、Git Bash 与 GCM…");

        let mut after = GitStatus::default();
        for _ in 0..20 {
            after = status_snapshot();
            if after.installed && after.bash_path.is_some() && after.gcm {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if !after.installed {
            return Err(
                "安装程序已结束，但新终端仍无法找到 Git。请重新启动 Stacker 后刷新状态".into(),
            );
        }
        if after.bash_path.is_none() || !after.gcm {
            return Err(
                "Git 已安装，但 Git Bash 或 GCM 组件不完整。请重新运行安装并保留默认组件".into(),
            );
        }
        let _ = window.emit("install-progress", "__done__");
        Ok(if before.installed {
            format!(
                "Git for Windows 已更新至 {}",
                after.version.unwrap_or(release.latest)
            )
        } else {
            format!(
                "Git for Windows {} 已安装",
                after.version.unwrap_or(release.latest)
            )
        })
    })();
    let _ = std::fs::remove_file(&installer);
    result
}

fn git_install_requires_elevation(status: &GitStatus) -> bool {
    if !status.installed {
        return false;
    }
    let Some(path) = status.path.as_deref() else {
        return true;
    };
    let path = path.to_ascii_lowercase().replace('/', "\\");
    let Some(local) = std::env::var("LOCALAPPDATA").ok() else {
        return true;
    };
    !path.starts_with(&local.to_ascii_lowercase().replace('/', "\\"))
}

fn download_git_installer(
    window: &tauri::Window,
    release: &GitUpdateInfo,
) -> Result<PathBuf, String> {
    let safe_version = release
        .latest
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let target = std::env::temp_dir().join(format!(
        "stacker-git-{safe_version}-{}-setup.exe",
        chrono::Local::now().timestamp_millis()
    ));
    let result = (|| {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .timeout_read(Duration::from_secs(30))
            .timeout_write(Duration::from_secs(30))
            .build();
        let response = agent
            .get(&release.installer_url)
            .set("User-Agent", "Stacker")
            .set("Accept", "application/octet-stream")
            .call()
            .map_err(|e| format!("连接 Git for Windows 下载源失败：{e}"))?;
        let total = response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let mut reader = response.into_reader();
        let mut output = std::fs::File::create(&target)
            .map_err(|e| format!("创建 Git 安装程序临时文件失败：{e}"))?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut received = 0u64;
        let mut last_reported = 0u64;
        loop {
            if crate::installer::op_cancelled() {
                return Err("已取消 Git 下载".into());
            }
            let count = reader
                .read(&mut buffer)
                .map_err(|e| format!("下载 Git for Windows 时连接中断：{e}"))?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .map_err(|e| format!("写入 Git 安装程序失败：{e}"))?;
            received += count as u64;
            if received.saturating_sub(last_reported) >= 512 * 1024 {
                last_reported = received;
                let progress = if total > 0 {
                    format!(
                        "下载 {:.0}% · {:.1}/{:.1} MB",
                        received as f64 * 100.0 / total as f64,
                        received as f64 / 1_048_576.0,
                        total as f64 / 1_048_576.0
                    )
                } else {
                    format!("已下载 {:.1} MB", received as f64 / 1_048_576.0)
                };
                let _ = window.emit("install-progress", progress);
            }
        }
        output
            .flush()
            .map_err(|e| format!("保存 Git 安装程序失败：{e}"))?;
        if received < 1_048_576 {
            return Err("下载到的 Git 安装程序不完整，已停止安装".into());
        }
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&target);
    }
    result
}

fn verify_authenticode_signature(path: &Path) -> Result<String, String> {
    let escaped = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$s=Get-AuthenticodeSignature -LiteralPath '{escaped}'; $subject=if($s.SignerCertificate){{$s.SignerCertificate.Subject}}else{{''}}; Write-Output ($s.Status.ToString()+'|'+$subject)"
    );
    let encoded = crate::installer::powershell_encoded_command(&script);
    let output = run_output(
        Path::new("powershell.exe"),
        &["-NoProfile", "-EncodedCommand", &encoded],
        Duration::from_secs(20),
    )?;
    let text = output_text(&output);
    let Some(subject) = text.strip_prefix("Valid|") else {
        let detail = if text.is_empty() {
            String::new()
        } else {
            format!("：{text}")
        };
        return Err(format!("Git 安装程序数字签名无效，已停止安装{detail}"));
    };
    Ok(subject
        .split(',')
        .next()
        .unwrap_or("Git for Windows")
        .trim()
        .to_string())
}

#[cfg(windows)]
fn run_git_installer(
    window: &tauri::Window,
    installer: &Path,
    system_install: bool,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetExitCodeProcess, TerminateProcess};
    use winapi::um::shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winuser::SW_HIDE;

    let file: Vec<u16> = installer
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = if system_install { "runas\0" } else { "open\0" }
        .encode_utf16()
        .collect();
    let common_arguments = "/VERYSILENT /NORESTART /NOCANCEL /SUPPRESSMSGBOXES /SP- /CLOSEAPPLICATIONS /RESTARTAPPLICATIONS /o:PathOption=Cmd /o:UseCredentialManager=Enabled";
    let arguments = if system_install {
        common_arguments.to_string()
    } else {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("无法读取当前用户的应用数据目录")?
            .join("Programs")
            .join("Git");
        format!(r#"{common_arguments} /DIR="{}""#, local.display())
    };
    let parameters: Vec<u16> = arguments.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            let error = std::io::Error::last_os_error();
            return if system_install && error.raw_os_error() == Some(1223) {
                Err("已取消 Git 安装所需的管理员授权".into())
            } else {
                Err(format!("无法启动 Git 安装程序：{error}"))
            };
        }

        let started = Instant::now();
        let mut last_reported = 0;
        loop {
            if crate::installer::op_cancelled() {
                let _ = TerminateProcess(info.hProcess, 1);
                let _ = WaitForSingleObject(info.hProcess, 5_000);
                CloseHandle(info.hProcess);
                return Err("已取消 Git 安装".into());
            }
            match WaitForSingleObject(info.hProcess, 500) {
                0 => break,
                258 => {
                    let elapsed = started.elapsed().as_secs();
                    if elapsed != last_reported {
                        last_reported = elapsed;
                        let _ = window.emit(
                            "install-progress",
                            format!("正在安装 Git for Windows · 已 {elapsed} 秒"),
                        );
                    }
                }
                _ => {
                    CloseHandle(info.hProcess);
                    return Err("等待 Git 安装程序时发生系统错误".into());
                }
            }
        }
        let mut code = 1u32;
        let read_code = GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if read_code == 0 {
            return Err("无法读取 Git 安装程序退出状态".into());
        }
        if code == 0 {
            Ok(())
        } else {
            Err(format!("Git 安装未完成，安装程序退出代码：{code}"))
        }
    }
}

#[cfg(not(windows))]
fn run_git_installer(_: &tauri::Window, _: &Path, _: bool) -> Result<(), String> {
    Err("Git for Windows 仅支持 Windows".into())
}

fn ensure_git() -> Result<PathBuf, String> {
    crate::env::resolve_fresh("git.exe").ok_or("未检测到 Git，请先安装 Git for Windows。".into())
}

fn git_config_get(key: &str) -> Option<String> {
    git_config_get_with_args(&["config", "--global", "--get", key])
}

fn git_config_get_effective(key: &str) -> Option<String> {
    git_config_get_with_args(&["config", "--get", key])
}

fn git_config_get_with_args(args: &[&str]) -> Option<String> {
    let git = crate::env::resolve_fresh("git.exe")?;
    let out = run_output(&git, args, Duration::from_secs(5)).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = output_text(&out);
    (!text.is_empty()).then_some(text)
}

fn git_config_set(key: &str, value: &str) -> Result<(), String> {
    let git = ensure_git()?;
    let out = run_output(
        &git,
        &["config", "--global", key, value],
        Duration::from_secs(8),
    )?;
    if out.status.success() {
        Ok(())
    } else {
        Err(command_error("写入 Git 全局配置失败", &out))
    }
}

fn git_config_unset(key: &str) -> Result<(), String> {
    let git = ensure_git()?;
    let out = run_output(
        &git,
        &["config", "--global", "--unset-all", key],
        Duration::from_secs(8),
    )?;
    if out.status.success() || matches!(out.status.code(), Some(1 | 5)) {
        Ok(())
    } else {
        Err(command_error("清除 Git 全局配置失败", &out))
    }
}

fn run_program(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let out = run_output(program, args, timeout)?;
    if !out.status.success() {
        return Err(output_text(&out));
    }
    Ok(output_text(&out)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("可用")
        .to_string())
}

fn run_output(program: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    hide_window(&mut cmd);
    command_output_timeout(cmd, timeout)
}

fn run_output_with_input(
    program: &Path,
    args: &[&str],
    input: &str,
    timeout: Duration,
) -> Result<Output, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::piped());
    hide_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let pid = child.id();
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input.as_bytes()) {
            terminate_process_tree(pid);
            let _ = child.wait();
            return Err(error.to_string());
        }
    }
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    terminate_process_tree(pid);
                    let _ = child.wait();
                    return Err("凭据管理器响应超时。".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn hide_window(cmd: &mut Command) {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
}

fn command_output_timeout(mut command: Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let pid = child.id();
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    terminate_process_tree(pid);
                    let _ = child.wait();
                    return Err("操作等待超时。".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn terminate_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill.exe");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        hide_window(&mut cmd);
        let _ = cmd.status();
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
}

fn command_error(prefix: &str, out: &Output) -> String {
    let detail = output_text(out);
    if detail.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}：{detail}")
    }
}

fn decode_command_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (text, _, _) = GBK.decode(bytes);
            text.into_owned()
        }
    }
}

fn output_text(out: &Output) -> String {
    let bytes = if out.stdout.is_empty() {
        &out.stderr
    } else {
        &out.stdout
    };
    decode_command_bytes(bytes).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_environment_is_process_scoped_and_contains_no_secret() {
        let profile = GitAccountProfile {
            platform: "github".into(),
            username: "byteswalk".into(),
            display_name: Some("Bytes Walk".into()),
            email: Some("dev@example.com".into()),
            expires_at: None,
            authenticated: true,
            token_verified: true,
            service_name: None,
            base_url: None,
            provider: None,
        };
        let environment = account_environment(&profile);
        assert!(environment
            .iter()
            .any(|(key, value)| { key == "STACKER_GIT_ACCOUNT" && value == "byteswalk" }));
        assert!(environment
            .iter()
            .any(|(key, value)| { key.starts_with("GIT_CONFIG_VALUE_") && value == "Bytes Walk" }));
        assert!(!environment
            .iter()
            .any(|(key, _)| key.to_ascii_lowercase().contains("token")));
    }

    #[test]
    fn ai_context_describes_capabilities_and_safety_boundaries() {
        let profile = GitAccountProfile {
            platform: "github".into(),
            username: "byteswalk".into(),
            display_name: Some("Bytes Walk".into()),
            email: Some("dev@example.com".into()),
            expires_at: None,
            authenticated: true,
            token_verified: true,
            service_name: None,
            base_url: None,
            provider: None,
        };
        let context = build_account_ai_context(&profile);
        assert!(context.contains("clone、fetch、pull、commit、push"));
        assert!(context.contains("创建仓库、PR、Issue、Release"));
        assert!(context.contains("git status --short --branch"));
        assert!(context.contains("不得 force push"));
        assert!(!context.contains("Stacker"));
        assert!(!context.contains("password="));
    }

    #[test]
    fn remote_url_must_match_selected_platform() {
        let profile = GitAccountProfile {
            platform: "github".into(),
            username: "byteswalk".into(),
            token_verified: true,
            authenticated: true,
            ..Default::default()
        };
        assert!(validate_remote_url(&profile, "https://github.com/byteswalk/stacker.git").is_ok());
        assert!(validate_remote_url(&profile, "https://gitee.com/byteswalk/stacker.git").is_err());
        assert!(validate_remote_url(&profile, "file:///tmp/repo").is_err());
    }

    #[test]
    fn custom_service_url_keeps_host_and_optional_path() {
        assert_eq!(
            normalize_service_url("https://git.example.com/platform/").unwrap(),
            (
                "https://git.example.com/platform".into(),
                "git.example.com".into(),
                "https"
            )
        );
        assert!(normalize_service_url("file:///tmp/git").is_err());
        assert!(normalize_service_url("https://user@git.example.com").is_err());
    }

    #[test]
    fn aliyun_codeup_uses_its_openapi_identity_and_repository_path() {
        assert!(is_aliyun_codeup("https://codeup.aliyun.com/"));
        assert!(!is_aliyun_codeup("https://gitlab.example.com/"));
        assert_eq!(
            normalize_generic_username("simplechinese@gmail.com").as_deref(),
            Ok("simplechinese@gmail.com")
        );
        let profile = GitAccountProfile {
            platform: "custom:codeup.aliyun.com".into(),
            username: "simplechinese@gmail.com".into(),
            base_url: Some("https://codeup.aliyun.com".into()),
            provider: Some("aliyun-codeup".into()),
            ..Default::default()
        };
        assert_eq!(
            account_repository_pattern(&profile),
            "https://simplechinese%40gmail.com@codeup.aliyun.com/<组织或代码组>/<仓库名>.git"
        );

        let response = serde_json::json!({
            "id": "user-id",
            "name": "Simple Chinese",
            "email": "simplechinese@gmail.com",
            "lastOrganization": "organization-id",
            "createdAt": "2026-07-16T00:00:00Z"
        });
        let user =
            parse_custom_platform_user("aliyun-codeup", "simplechinese@gmail.com", &response)
                .unwrap();
        assert_eq!(user.login, "simplechinese@gmail.com");
        assert_eq!(user.email.as_deref(), Some("simplechinese@gmail.com"));
    }

    #[test]
    fn gitea_page_is_not_misidentified_by_gitlab_emoji_name() {
        let page = r#"
            <meta name="author" content="Gitea - Git with a cup of tea">
            <script>customEmojis = {"gitea": ":gitea:", "gitlab": ":gitlab:"};</script>
            <footer>Powered by Gitea</footer>
        "#;
        assert_eq!(classify_custom_service_page(page), Some("gitea"));
    }

    #[test]
    fn repository_name_rejects_paths() {
        assert!(validate_repository_name("stacker-core").is_ok());
        assert!(validate_repository_name("../stacker").is_err());
        assert!(validate_repository_name("owner/repo").is_err());
    }

    #[test]
    fn migration_mode_is_selected_from_platform_and_owner() {
        assert!(should_use_native_transfer(
            "github",
            "github",
            "account-a",
            "account-b"
        ));
        assert!(!should_use_native_transfer(
            "github",
            "github",
            "account-a",
            "account-a"
        ));
        assert!(!should_use_native_transfer(
            "github",
            "gitee",
            "account-a",
            "account-b"
        ));
    }

    #[test]
    fn release_asset_selects_full_git_installer_for_current_architecture() {
        let assets = vec![
            GitReleaseAsset {
                name: "PortableGit-2.55.0-64-bit.7z.exe".into(),
                browser_download_url: "portable".into(),
            },
            GitReleaseAsset {
                name: "Git-2.55.0-universal.zip".into(),
                browser_download_url: "archive".into(),
            },
            GitReleaseAsset {
                name: if cfg!(target_arch = "aarch64") {
                    "Git-2.55.0-arm64.exe".into()
                } else if cfg!(target_arch = "x86") {
                    "Git-2.55.0-32-bit.exe".into()
                } else {
                    "Git-2.55.0-64-bit.exe".into()
                },
                browser_download_url: "expected".into(),
            },
        ];
        assert_eq!(
            select_git_installer(&assets).map(|asset| asset.browser_download_url.as_str()),
            Some("expected")
        );
    }

    #[test]
    fn github_release_html_fallback_parses_official_installer() {
        let name = if cfg!(target_arch = "aarch64") {
            "Git-2.55.0.2-arm64.exe"
        } else if cfg!(target_arch = "x86") {
            "Git-2.55.0.2-32-bit.exe"
        } else {
            "Git-2.55.0.2-64-bit.exe"
        };
        let body = format!(
            r#"<a href="/git-for-windows/git/releases/download/v2.55.0.windows.2/PortableGit-2.55.0.2-64-bit.7z.exe">portable</a><a href="/git-for-windows/git/releases/download/v2.55.0.windows.2/{name}">installer</a>"#
        );
        let assets = git_release_assets_from_html(&body);
        let installer = select_git_installer(&assets).expect("installer");
        assert_eq!(installer.name, name);
        assert!(installer
            .browser_download_url
            .starts_with("https://github.com/"));
        assert_eq!(
            release_tag_from_url(
                "https://github.com/git-for-windows/git/releases/tag/v2.55.0.windows.2"
            )
            .as_deref(),
            Some("v2.55.0.windows.2")
        );
    }

    #[test]
    fn mirror_directory_names_keep_only_stable_releases() {
        assert_eq!(
            normalize_stable_git_tag("v2.55.0.windows.2/").as_deref(),
            Some("v2.55.0.windows.2")
        );
        assert_eq!(
            normalize_stable_git_tag("Git%20for%20Windows%20v2.55.0.windows.2/").as_deref(),
            Some("v2.55.0.windows.2")
        );
        assert!(normalize_stable_git_tag("v2.55.0-rc2.windows.1/").is_none());
    }

    #[test]
    fn mirror_urls_join_relative_and_root_paths() {
        assert_eq!(
            join_mirror_url("https://mirror.example/git/", "v2.55.0.windows.2/").as_deref(),
            Ok("https://mirror.example/git/v2.55.0.windows.2/")
        );
        assert_eq!(
            join_mirror_url("https://mirror.example/git/", "/files/Git.exe").as_deref(),
            Ok("https://mirror.example/files/Git.exe")
        );
    }
}
