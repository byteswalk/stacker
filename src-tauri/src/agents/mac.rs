//! The agents page on macOS. CLIs are looked up on the login shell's PATH and installed with
//! npm, or with the vendor's own `install.sh` where there is no npm package; desktop apps are
//! found in the Applications folders and installed with Homebrew where it has a cask. What
//! has no automatic route says so and links to the vendor's download page.

use super::detect::DesktopFound;
use super::install::npm::{npm_install_latest, npm_uninstall};
use super::process::{run_command_streamed, run_command_text};
use super::registry::{ToolSpec, Vendor};
use super::{emit_progress, VibeSurface};
use std::path::{Path, PathBuf};
use std::time::Duration;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(1200);

// ---------- desktop apps ----------

/// What the Mac edition of a product's desktop app is called and where Homebrew has it.
pub(crate) struct MacDesktop {
    /// Bundle names, best first.
    pub apps: &'static [&'static str],
    /// A bundle with one of those names counts only when its identifier contains this
    /// (ChatGPT.app is the Codex app only when it says `com.openai.codex`).
    pub bundle_hint: Option<&'static str>,
    pub cask: Option<&'static str>,
}

/// Products whose Mac app Stacker knows by name. The rest are still found by the keywords
/// the catalogue gives every desktop app.
pub(crate) fn mac_desktop(id: &str) -> Option<MacDesktop> {
    let entry = |apps, bundle_hint, cask| {
        Some(MacDesktop {
            apps,
            bundle_hint,
            cask,
        })
    };
    match id {
        "claude" => entry(&["Claude.app"], Some("anthropic"), Some("claude")),
        "codex" => entry(
            &["Codex.app", "ChatGPT.app"],
            Some("com.openai.codex"),
            Some("chatgpt"),
        ),
        "antigravity" => entry(&["Antigravity.app"], None, Some("antigravity")),
        "opencode" => entry(&["OpenCode.app"], None, Some("opencode-desktop")),
        "zcode" => entry(&["ZCode.app"], None, Some("zcode")),
        "workbuddy-cn" => entry(&["WorkBuddy.app"], None, Some("workbuddy-cn")),
        "workbuddy-global" => entry(&["WorkBuddy AI.app"], None, Some("workbuddy-ai")),
        "qoder" => entry(&["Qoder.app"], None, Some("qoder")),
        "deepseek-harness" => entry(&["DeepSeek Harness.app"], None, Some("deepseek-harness")),
        "openclaw" => entry(&["OpenClaw.app"], None, Some("openclaw")),
        "hermes" => entry(&["Hermes.app"], None, Some("hermes-desktop")),
        "copilot" => entry(&["GitHub Copilot.app"], None, Some("github-copilot-app")),
        "cursor" => entry(&["Cursor.app"], None, Some("cursor")),
        "factory" => entry(&["Factory.app"], None, Some("factory")),
        "kiro" => entry(&["Kiro.app"], None, Some("kiro")),
        _ => None,
    }
}

fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications"));
    }
    dirs
}

/// One key of an app's `Info.plist`, read by Apple's own `plutil`.
pub(crate) fn plist_value(app: &Path, key: &str) -> Option<String> {
    let plist = app.join("Contents").join("Info.plist");
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(&plist)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

fn brew_prefixes() -> [&'static str; 2] {
    ["/opt/homebrew", "/usr/local"]
}

fn cask_installed(cask: &str) -> bool {
    brew_prefixes()
        .iter()
        .any(|prefix| Path::new(prefix).join("Caskroom").join(cask).is_dir())
}

fn found(app: PathBuf, cask: Option<&str>) -> DesktopFound {
    let version = plist_value(&app, "CFBundleShortVersionString")
        .or_else(|| plist_value(&app, "CFBundleVersion"));
    let method = match cask {
        Some(cask) if cask_installed(cask) => "brew",
        _ => "app",
    };
    DesktopFound {
        launch: Some(app.to_string_lossy().into_owned()),
        path: Some(app),
        version,
        method: Some(method.into()),
        uninstall: None,
    }
}

/// The product's app in /Applications or ~/Applications: by its known bundle name first,
/// then by the catalogue's keywords.
pub(crate) fn detect_desktop(spec: &ToolSpec) -> Option<DesktopFound> {
    detect_known(spec).or_else(|| detect_by_keywords(spec))
}

/// The app by the bundle name (and identifier) Stacker knows for this product.
fn detect_known(spec: &ToolSpec) -> Option<DesktopFound> {
    let known = mac_desktop(spec.id)?;
    let cask = known.cask;
    {
        for dir in app_dirs() {
            for name in known.apps {
                let app = dir.join(name);
                if !app.is_dir() {
                    continue;
                }
                let fits = known.bundle_hint.map_or(true, |hint| {
                    plist_value(&app, "CFBundleIdentifier")
                        .is_some_and(|id| id.to_lowercase().contains(hint))
                });
                if fits {
                    return Some(found(app, cask));
                }
            }
        }
    }
    None
}

/// Any app whose name matches the catalogue's keywords: good enough to show, not to remove.
fn detect_by_keywords(spec: &ToolSpec) -> Option<DesktopFound> {
    let cask = mac_desktop(spec.id).and_then(|k| k.cask);
    if spec.desktop.keywords.is_empty() {
        return None;
    }
    for dir in app_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut apps: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        apps.sort();
        for app in apps {
            let Some(name) = app
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".app"))
            else {
                continue;
            };
            if super::detect::desktop_name_matches(
                name,
                spec.desktop.keywords,
                spec.desktop.excludes,
            ) {
                return Some(found(app, cask));
            }
        }
    }
    None
}

/// Homebrew's own command.
fn brew() -> Option<PathBuf> {
    super::process::resolve_command(&["brew"]).or_else(|| {
        brew_prefixes()
            .iter()
            .map(|prefix| Path::new(prefix).join("bin").join("brew"))
            .find(|path| path.is_file())
    })
}

fn run_brew(args: &[&str], window: &Option<tauri::Window>) -> Result<String, String> {
    let brew = brew()
        .ok_or("没有检测到 Homebrew。请先安装 Homebrew（brew.sh），或从官网下载这个应用。")?;
    run_command_streamed(&brew, args, "brew", INSTALL_TIMEOUT, Duration::ZERO, window)
}

/// The cask's current version from Homebrew's local index (`4.2,abc` → `4.2`).
fn cask_latest(cask: &str) -> Result<Option<String>, String> {
    let Some(brew) = brew() else {
        return Ok(None);
    };
    let text = run_command_text(
        &brew,
        &["info", "--cask", "--json=v2", cask],
        "brew info",
        Duration::from_secs(30),
    )?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(value["casks"][0]["version"]
        .as_str()
        .map(|v| v.split(',').next().unwrap_or(v).to_string()))
}

fn app_running(app: &Path) -> bool {
    let inside = format!("{}/", app.to_string_lossy().to_lowercase());
    crate::sessions::footprint::processes::running_images()
        .iter()
        .any(|image| image.to_string_lossy().to_lowercase().starts_with(&inside))
}

pub(crate) fn desktop_surface(spec: &ToolSpec, check_latest: bool) -> VibeSurface {
    let known = mac_desktop(spec.id);
    let cask = known.as_ref().and_then(|k| k.cask);
    if known.is_none() && !spec.desktop_available {
        let mut surface = super::unavailable_surface(
            spec.desktop.name,
            "桌面端",
            spec.desktop.description,
            spec.desktop.install_url,
            spec.desktop.docs_url,
        );
        surface.install_unavailable_reason = Some("官方没有提供 Mac 桌面应用。".into());
        return surface;
    }
    let found = detect_desktop(spec);
    let installed = found.is_some();
    let method = found.as_ref().and_then(|f| f.method.clone());
    let version = found.as_ref().and_then(|f| f.version.clone());
    let by_brew = method.as_deref() == Some("brew");
    let latest_checked = check_latest && installed && by_brew;
    let (latest, latest_source, latest_error) = match (latest_checked, cask) {
        (true, Some(cask)) => match cask_latest(cask) {
            Ok(Some(v)) => (Some(v), Some("Homebrew".to_string()), None),
            Ok(None) => (None, None, None),
            Err(e) => (None, None, Some(e)),
        },
        _ => (None, None, None),
    };
    let update_available = installed
        && version
            .as_deref()
            .zip(latest.as_deref())
            .is_some_and(|(cur, next)| super::feeds::is_newer(cur, next));
    let can_install = cask.is_some();
    VibeSurface {
        available: true,
        label: spec.desktop.name.into(),
        kind: "桌面端".into(),
        description: spec.desktop.description.into(),
        installed,
        status: if update_available {
            "update"
        } else if installed {
            "installed"
        } else {
            "missing"
        }
        .into(),
        version,
        probe_error: None,
        latest,
        update_available,
        path: found
            .as_ref()
            .and_then(|f| f.path.as_ref())
            .map(|p| p.to_string_lossy().into_owned()),
        command: None,
        install_method_label: method
            .as_deref()
            .and_then(super::detect::install_method_label),
        install_method: method,
        install_url: spec.desktop.install_url.into(),
        docs_url: spec.desktop.docs_url.into(),
        can_install,
        install_unavailable_reason: (!can_install)
            .then(|| "Homebrew 没有收录这个应用，请从官网下载安装。".to_string()),
        // An app not installed by Homebrew updates itself.
        can_update: installed && by_brew,
        can_uninstall: installed,
        can_open: installed,
        health: if installed { "healthy" } else { "missing" }.into(),
        broken_reason: None,
        other_installs: Vec::new(),
        can_repair: false,
        repair_kind: None,
        repair_command: None,
        latest_error,
        latest_source,
        latest_checked,
        launch: found.and_then(|f| f.launch),
    }
}

pub(crate) fn install_desktop(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let cask = mac_desktop(spec.id)
        .and_then(|k| k.cask)
        .ok_or("Homebrew 没有收录这个应用，请从官网下载安装。")?;
    emit_progress(
        window,
        format!("正在通过 Homebrew 安装 {}…", spec.desktop.name),
    );
    run_brew(&["install", "--cask", cask], window)?;
    Ok(format!("{} 已通过 Homebrew 安装", spec.desktop.name))
}

pub(crate) fn update_desktop(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let found = detect_desktop(spec).ok_or_else(|| format!("未检测到 {}。", spec.desktop.name))?;
    let cask = mac_desktop(spec.id).and_then(|k| k.cask);
    match (found.method.as_deref(), cask) {
        (Some("brew"), Some(cask)) => {
            if found.path.as_deref().is_some_and(app_running) {
                return Err(format!("{} 正在运行，请先退出再更新。", spec.desktop.name));
            }
            emit_progress(
                window,
                format!("正在通过 Homebrew 更新 {}…", spec.desktop.name),
            );
            run_brew(&["upgrade", "--cask", cask], window)?;
            Ok(format!("{} 已通过 Homebrew 更新", spec.desktop.name))
        }
        _ => Err(format!(
            "{} 不是通过 Homebrew 安装的，请在应用里检查更新。",
            spec.desktop.name
        )),
    }
}

pub(crate) fn uninstall_desktop(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    // Only an app known by its bundle name is removed; one matched by its name alone may be
    // something else that happens to share a word.
    let found = match detect_known(spec) {
        Some(found) => found,
        None if detect_desktop(spec).is_some() => {
            return Err(format!(
                "无法确认找到的就是 {}，Stacker 不替你卸载；请在访达里把它移到废纸篓。",
                spec.desktop.name
            ))
        }
        None => return Err(format!("未检测到 {}。", spec.desktop.name)),
    };
    let app = found.path.clone().ok_or("找不到应用的位置。")?;
    if app_running(&app) {
        return Err(format!("{} 正在运行，请先退出再卸载。", spec.desktop.name));
    }
    let cask = mac_desktop(spec.id).and_then(|k| k.cask);
    if let (Some("brew"), Some(cask)) = (found.method.as_deref(), cask) {
        emit_progress(
            window,
            format!("正在通过 Homebrew 卸载 {}…", spec.desktop.name),
        );
        run_brew(&["uninstall", "--cask", cask], window)?;
        return Ok(format!("{} 已通过 Homebrew 卸载", spec.desktop.name));
    }
    emit_progress(window, format!("正在把 {} 移到废纸篓…", spec.desktop.name));
    crate::macos::trash(&app)?;
    Ok(format!(
        "{} 已移到废纸篓，用户数据保留；需要时可以从废纸篓放回。",
        spec.desktop.name
    ))
}

// ---------- CLIs ----------

/// A vendor's own installer script for macOS, with the arguments that keep it from asking.
fn cli_script(spec: &ToolSpec) -> Option<(&'static str, &'static [&'static str])> {
    Some(match spec.vendor {
        Vendor::Claude => ("https://claude.ai/install.sh", &[]),
        Vendor::Codex => ("https://chatgpt.com/codex/install.sh", &[]),
        Vendor::Kimi => ("https://code.kimi.com/kimi-code/install.sh", &[]),
        Vendor::Antigravity => ("https://antigravity.google/cli/install.sh", &[]),
        Vendor::Cursor => ("https://cursor.com/install", &[]),
        Vendor::MiniMax => ("https://filecdn.minimax.chat/public/install.sh", &[]),
        Vendor::Xai => ("https://x.ai/cli/install.sh", &[]),
        Vendor::OpenClaw => (
            "https://openclaw.ai/install.sh",
            &["--no-onboard", "--no-prompt"],
        ),
        Vendor::Hermes => (
            "https://hermes-agent.nousresearch.com/install.sh",
            &["--non-interactive"],
        ),
        Vendor::Factory => ("https://app.factory.ai/cli", &[]),
        Vendor::Kiro => ("https://cli.kiro.dev/install", &[]),
        _ => return None,
    })
}

/// How a CLI gets onto this Mac: the vendor's script where Windows uses the vendor's own
/// installer too, npm otherwise.
enum Route {
    Npm(&'static str),
    Script(&'static str, &'static [&'static str]),
}

fn route(spec: &ToolSpec) -> Option<Route> {
    let script = cli_script(spec);
    // Where the vendor recommends its own installer over npm, as Stacker does on Windows.
    let prefer_script = matches!(
        spec.vendor,
        Vendor::Claude | Vendor::Kimi | Vendor::MiniMax | Vendor::OpenClaw
    );
    match (spec.cli.npm_package, script) {
        (_, Some((url, args))) if prefer_script => Some(Route::Script(url, args)),
        (Some(pkg), _) => Some(Route::Npm(pkg)),
        (None, Some((url, args))) => Some(Route::Script(url, args)),
        (None, None) => None,
    }
}

pub(crate) fn can_install_cli(spec: &ToolSpec) -> bool {
    route(spec).is_some()
}

/// Downloads the script with the system's curl (which honours `https_proxy`) and runs it
/// with bash; nobody is at a terminal, so it takes its non-interactive path.
fn run_script(
    spec: &ToolSpec,
    url: &str,
    args: &[&str],
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|e| format!("无法创建临时目录：{e}"))?;
    let script = dir.path().join("install.sh");
    emit_progress(window, format!("正在下载 {} 官方安装脚本…", spec.cli.name));
    let script_text = script.to_string_lossy().into_owned();
    run_command_streamed(
        Path::new("/usr/bin/curl"),
        &["-fsSL", "--retry", "2", "-o", &script_text, url],
        "curl",
        Duration::from_secs(120),
        Duration::ZERO,
        window,
    )?;
    let head = std::fs::read(&script).unwrap_or_default();
    if !head.starts_with(b"#!") {
        return Err("下载到的不是安装脚本，已停止。".into());
    }
    emit_progress(window, format!("正在运行 {} 官方安装脚本…", spec.cli.name));
    let mut full = vec![script_text.as_str()];
    full.extend_from_slice(args);
    run_command_streamed(
        Path::new("/bin/bash"),
        &full,
        "install.sh",
        INSTALL_TIMEOUT,
        Duration::ZERO,
        window,
    )?;
    Ok(())
}

pub(crate) fn install_cli(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    emit_progress(window, format!("正在安装 {}…", spec.cli.name));
    match route(spec).ok_or("这个 CLI 暂无自动安装方案，请查看官方文档。")? {
        Route::Npm(pkg) => {
            npm_install_latest(pkg, None, window)?;
            Ok(format!("{} 已通过 npm 安装", spec.cli.name))
        }
        Route::Script(url, args) => {
            run_script(spec, url, args, window)?;
            Ok(format!("{} 已通过官方安装脚本安装", spec.cli.name))
        }
    }
}

/// The installed command, the first on PATH.
fn installed_cli(spec: &ToolSpec) -> Option<PathBuf> {
    super::process::resolve_command(spec.cli.candidates)
}

/// npm, brew or the vendor's own installer, told from where the command really lives.
pub(crate) fn cli_install_method(spec: &ToolSpec, program: Option<&Path>) -> Option<String> {
    let program = program?;
    let real = program
        .canonicalize()
        .unwrap_or_else(|_| program.to_path_buf());
    let text = real.to_string_lossy().to_lowercase();
    if text.contains("/node_modules/") {
        return Some("npm".into());
    }
    if text.contains("/cellar/") || text.contains("/caskroom/") {
        return Some("brew".into());
    }
    if let Some(pkg) = spec.cli.npm_package {
        if super::detect::is_npm_shim(program, pkg) && !text.contains("/.local/") {
            return Some("npm".into());
        }
    }
    Some("native".into())
}

/// The Homebrew formula or cask a command comes from: `…/Cellar/<name>/…`.
fn brew_package(program: &Path) -> Option<(bool, String)> {
    let real = program.canonicalize().ok()?;
    let parts: Vec<String> = real
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let at = parts
        .iter()
        .position(|p| p.eq_ignore_ascii_case("Cellar") || p.eq_ignore_ascii_case("Caskroom"))?;
    let cask = parts[at].eq_ignore_ascii_case("Caskroom");
    parts.get(at + 1).map(|name| (cask, name.clone()))
}

pub(crate) fn update_cli(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let Some(program) = installed_cli(spec) else {
        return install_cli(spec, window);
    };
    match cli_install_method(spec, Some(&program)).as_deref() {
        Some("npm") => {
            let pkg = spec
                .cli
                .npm_package
                .ok_or_else(|| format!("{} 不是 npm 包。", spec.cli.name))?;
            emit_progress(window, format!("正在通过 npm 更新 {}…", spec.cli.name));
            npm_install_latest(pkg, Some(&program), window)?;
            Ok(format!("{} 已通过 npm 更新", spec.cli.name))
        }
        Some("brew") => {
            let (cask, name) = brew_package(&program).ok_or("无法判断 Homebrew 包名。")?;
            emit_progress(window, format!("正在通过 Homebrew 更新 {}…", spec.cli.name));
            if cask {
                run_brew(&["upgrade", "--cask", &name], window)?;
            } else {
                run_brew(&["upgrade", &name], window)?;
            }
            Ok(format!("{} 已通过 Homebrew 更新", spec.cli.name))
        }
        _ => {
            let (url, args) = cli_script(spec).ok_or_else(|| {
                format!("{} 是用其他方式安装的，请按官方文档更新。", spec.cli.name)
            })?;
            run_script(spec, url, args, window)?;
            Ok(format!("{} 已通过官方安装脚本更新", spec.cli.name))
        }
    }
}

pub(crate) fn uninstall_cli(
    spec: &ToolSpec,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    let program = installed_cli(spec).ok_or_else(|| format!("未检测到 {}。", spec.cli.name))?;
    match cli_install_method(spec, Some(&program)).as_deref() {
        Some("npm") => {
            let pkg = spec
                .cli
                .npm_package
                .ok_or_else(|| format!("{} 不是 npm 包。", spec.cli.name))?;
            emit_progress(window, format!("正在通过 npm 卸载 {}…", spec.cli.name));
            npm_uninstall(pkg, Some(&program), window)?;
            Ok(format!("{} 已通过 npm 卸载", spec.cli.name))
        }
        Some("brew") => {
            let (cask, name) = brew_package(&program).ok_or("无法判断 Homebrew 包名。")?;
            emit_progress(window, format!("正在通过 Homebrew 卸载 {}…", spec.cli.name));
            if cask {
                run_brew(&["uninstall", "--cask", &name], window)?;
            } else {
                run_brew(&["uninstall", &name], window)?;
            }
            Ok(format!("{} 已通过 Homebrew 卸载", spec.cli.name))
        }
        _ => {
            remove_native_cli(&program, spec.cli.command)?;
            Ok(format!(
                "{} 的命令已删除，用户配置和会话数据保留。",
                spec.cli.name
            ))
        }
    }
}

/// Removes a command an installer script put in the user's own folder: the entry on PATH,
/// and the file it links to when that file is the command itself. Configuration and
/// sessions are left alone.
fn remove_native_cli(program: &Path, command: &str) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("找不到用户目录。")?;
    let name = program
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if !name.eq_ignore_ascii_case(command) {
        return Err("命令文件名不匹配，已取消卸载。".into());
    }
    if !program.starts_with(&home) {
        return Err("命令不在当前用户目录内，为避免误删已取消卸载。".into());
    }
    // What the command really is, with `..` and every link resolved.
    let target = program.canonicalize().ok();
    if let Some(manager) = version_manager(program, target.as_deref()) {
        return Err(format!(
            "这个命令由 {manager} 管理，请用 {manager} 自己的命令卸载，Stacker 不动它的文件。"
        ));
    }
    std::fs::remove_file(program).map_err(|e| format!("删除命令失败：{e}"))?;
    // The file behind a link goes too, only when it is plainly this command's own: named
    // after it, or inside the folder its installer keeps for it (`~/.local/share/claude/…`).
    if let Some(target) = target.filter(|t| t.as_path() != program && t.is_file()) {
        let own_folder = home.join(".local").join("share").join(command);
        let named = target
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(command));
        if target.starts_with(&home) && (named || target.starts_with(&own_folder)) {
            let _ = std::fs::remove_file(target);
        }
    }
    Ok(())
}

/// A version manager whose shim stands where the command is: removing it would break every
/// command the manager serves.
fn version_manager(program: &Path, target: Option<&Path>) -> Option<&'static str> {
    let texts = [Some(program), target]
        .into_iter()
        .flatten()
        .map(|p| p.to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    let has = |needle: &str| texts.iter().any(|t| t.contains(needle));
    let named = |file: &str| {
        target
            .and_then(|t| t.file_name())
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(file))
    };
    if has("/.volta/") || named("volta-shim") {
        Some("Volta")
    } else if has("/mise/shims/") || has("/mise/installs/") || named("mise") {
        Some("mise")
    } else if has("/.asdf/") || named("asdf") {
        Some("asdf")
    } else if has("/.nvm/") || has("/.fnm/") || has("/.nodenv/") || has("/n/versions/") {
        Some("Node 版本管理器")
    } else {
        None
    }
}

/// A CLI's latest version on macOS: npm's for an npm install, Homebrew's for a brew one,
/// the vendor's own feed where the Windows code reads one that is not Windows-only.
pub(crate) fn cli_latest(spec: &ToolSpec, method: Option<&str>) -> super::detect::LatestLookup {
    match method {
        Some("npm") => match spec.cli.npm_package {
            Some(pkg) => super::detect::npm_latest(pkg).map(|v| Some((v, "npm"))),
            None => Ok(None),
        },
        Some("brew") => {
            let Some((cask, name)) = installed_cli(spec).as_deref().and_then(brew_package) else {
                return Ok(None);
            };
            let Some(brew) = brew() else {
                return Ok(None);
            };
            let kind = if cask { "--cask" } else { "--formula" };
            let text = run_command_text(
                &brew,
                &["info", kind, "--json=v2", &name],
                "brew info",
                Duration::from_secs(30),
            )?;
            let value: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let version = if cask {
                value["casks"][0]["version"].as_str()
            } else {
                value["formulae"][0]["versions"]["stable"].as_str()
            };
            Ok(version.map(|v| (v.split(',').next().unwrap_or(v).to_string(), "Homebrew")))
        }
        _ => match spec.vendor {
            Vendor::Kiro => super::detect::kiro_latest().map(|v| Some((v, "Kiro 官方发布"))),
            Vendor::Hermes => super::feeds::hermes_latest().map(|v| Some((v, "Hermes main 分支"))),
            _ => match spec.cli.npm_package {
                // The same release goes to npm and to the vendor's own installer.
                Some(pkg) if spec.vendor != Vendor::Claude => {
                    super::detect::npm_latest(pkg).map(|v| Some((v, "npm")))
                }
                _ => Ok(None),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_app_has_a_name_and_the_codex_app_is_told_by_its_identifier() {
        for id in ["claude", "codex", "cursor", "kiro", "deepseek-harness"] {
            assert!(!mac_desktop(id).unwrap().apps.is_empty(), "{id}");
        }
        assert_eq!(
            mac_desktop("codex").unwrap().bundle_hint,
            Some("com.openai.codex")
        );
        assert!(mac_desktop("pi").is_none());
    }

    #[test]
    fn a_cli_with_no_npm_package_uses_its_vendor_script() {
        let spec = super::super::registry::spec_by_id("cursor").unwrap();
        assert!(
            matches!(route(&spec), Some(Route::Script(url, _)) if url.starts_with("https://cursor.com/"))
        );
        let spec = super::super::registry::spec_by_id("copilot").unwrap();
        assert!(matches!(route(&spec), Some(Route::Npm("@github/copilot"))));
        let spec = super::super::registry::spec_by_id("claude").unwrap();
        assert!(matches!(route(&spec), Some(Route::Script(url, _)) if url.ends_with("install.sh")));
    }

    #[test]
    fn a_version_manager_s_shim_is_never_removed() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let volta = home.join(".volta").join("bin");
        std::fs::create_dir_all(&volta).unwrap();
        std::fs::write(volta.join("volta-shim"), b"bin").unwrap();
        std::os::unix::fs::symlink(volta.join("volta-shim"), volta.join("claude")).unwrap();
        let target = volta.join("claude").canonicalize().unwrap();
        assert_eq!(
            version_manager(&volta.join("claude"), Some(&target)),
            Some("Volta")
        );
        let mise = home.join(".local").join("bin").join("mise");
        assert_eq!(
            version_manager(Path::new("/x/.local/share/mise/shims/claude"), Some(&mise)),
            Some("mise")
        );
        assert_eq!(
            version_manager(
                Path::new("/x/.local/bin/claude"),
                Some(Path::new("/x/.local/share/claude/versions/2.1.0"))
            ),
            None
        );
    }

    #[test]
    fn a_homebrew_package_is_named_from_its_cellar_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir
            .path()
            .join("Cellar")
            .join("codex")
            .join("0.1")
            .join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("codex"), b"#!/bin/sh\n").unwrap();
        assert_eq!(
            brew_package(&bin.join("codex")),
            Some((false, "codex".to_string()))
        );
    }
}
