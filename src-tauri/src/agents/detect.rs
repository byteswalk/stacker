use crate::agents::{install::winget::*, process::*, registry::*, *};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) struct DesktopFound {
    pub(crate) path: Option<PathBuf>,
    pub(crate) version: Option<String>,
    pub(crate) method: Option<String>,
    pub(crate) uninstall: Option<String>,
    pub(crate) launch: Option<String>,
}

pub(crate) fn cli_surface(spec: &ToolSpec, check_latest: bool) -> VibeSurface {
    if spec.cli.command.is_empty() {
        return unavailable_surface(
            spec.cli.name,
            "CLI",
            spec.cli.description,
            spec.cli.install_url,
            spec.cli.docs_url,
        );
    }
    let probe = |path: &Path| {
        run_program_probe(spec.cli.name, path, &["--version"], Duration::from_secs(15))
    };
    let mut installs = super::health::enumerate_candidates(&command_dirs(), spec.cli.candidates)
        .into_iter()
        .filter(|path| !(spec.vendor == Vendor::Xai && is_community_grok(path)))
        .filter(|path| !(spec.vendor == Vendor::Qoder && is_qoder_dispatcher(path)))
        .map(|path| super::health::check_install(&path, &probe));
    let effective = installs.next();
    let other_installs: Vec<_> = installs.collect();
    let program = effective.as_ref().map(|info| PathBuf::from(&info.path));
    let installed = effective.as_ref().is_some_and(|info| info.healthy);
    let version = effective
        .as_ref()
        .and_then(|info| info.version.clone())
        .map(|text| {
            // "Hermes Agent v0.18.2 (2026.7.7.2) · upstream 524041b9 · local …" → "0.18.2"
            match spec.vendor {
                Vendor::Hermes => super::feeds::hermes_checkout()
                    .and_then(|dir| super::feeds::git_head(&dir))
                    .map(|sha| super::feeds::commit_version(&sha))
                    .or_else(|| first_semver(&text))
                    .unwrap_or(text),
                Vendor::Kiro | Vendor::Factory | Vendor::MiniMax => {
                    first_semver(&text).unwrap_or(text)
                }
                Vendor::Cursor => cursor_build(&text).unwrap_or(text),
                _ => text,
            }
        });
    let broken_reason = effective
        .as_ref()
        .filter(|info| !info.healthy)
        .and_then(|info| info.reason.clone());
    let method = detect_install_method(spec, program.as_deref());
    let health = match &effective {
        None => "missing",
        Some(info) if info.healthy => "healthy",
        Some(_) => "broken",
    };
    let repair_kind = (health == "broken")
        .then(|| {
            if other_installs.iter().any(|info| info.healthy) {
                Some("switch")
            } else if method.as_deref() == Some("npm") && spec.cli.npm_package.is_some() {
                // One npm install that does not start: what `npm install -g` would mend.
                Some("reinstall")
            } else if broken_reason
                .as_deref()
                .and_then(|reason| super::process::suggested_repair(reason, spec.cli.command))
                .is_some()
            {
                // The CLI says itself how to mend it ("run `hermes pm repair`").
                Some("command")
            } else {
                None
            }
        })
        .flatten();
    let can_repair = repair_kind.is_some();
    let repair_command = (repair_kind == Some("command"))
        .then(|| {
            broken_reason
                .as_deref()
                .and_then(|reason| super::process::suggested_repair(reason, spec.cli.command))
                .map(|args| format!("{} {}", spec.cli.command, args.join(" ")))
        })
        .flatten();
    let latest_checked = check_latest && installed;
    let (latest, latest_source, latest_error) = if latest_checked {
        split_latest(latest_for_cli(spec, method.as_deref()))
    } else {
        (None, None, None)
    };
    let update_available = installed
        && version
            .as_deref()
            .zip(latest.as_deref())
            .is_some_and(|(cur, next)| super::feeds::is_newer(cur, next));
    let status = if health == "broken" {
        "broken"
    } else if update_available {
        "update"
    } else if installed {
        "installed"
    } else {
        "missing"
    };
    VibeSurface {
        available: true,
        label: spec.cli.name.into(),
        kind: "CLI".into(),
        description: spec.cli.description.into(),
        installed,
        status: status.into(),
        version,
        probe_error: broken_reason.clone(),
        latest,
        update_available,
        path: program.as_ref().map(|p| p.to_string_lossy().into_owned()),
        command: Some(spec.cli.command.into()),
        install_method_label: method.as_deref().and_then(install_method_label),
        install_method: method,
        install_url: spec.cli.install_url.into(),
        docs_url: spec.cli.docs_url.into(),
        can_install: true,
        install_unavailable_reason: None,
        can_update: installed,
        can_uninstall: program.is_some(),
        can_open: installed,
        health: health.into(),
        broken_reason,
        other_installs,
        can_repair,
        repair_command,
        repair_kind: repair_kind.map(str::to_string),
        latest_error,
        latest_source,
        latest_checked,
        launch: None,
    }
}

pub(crate) fn desktop_surface(spec: &ToolSpec, check_latest: bool) -> VibeSurface {
    if !spec.desktop_available {
        return unavailable_surface(
            spec.desktop.name,
            "桌面端",
            spec.desktop.description,
            spec.desktop.install_url,
            spec.desktop.docs_url,
        );
    }
    let found = detect_desktop_for(spec);
    let broken_reason = found
        .as_ref()
        .and_then(|found| found.path.as_deref())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                && path.is_file()
                && !super::health::has_pe_header(path)
        })
        .map(|_| "不是有效的 Windows 程序".to_string());
    let installed = found.is_some() && broken_reason.is_none();
    let method = found.as_ref().and_then(|f| f.method.clone());
    // Kimi Work applies some updates in place. Its uninstall registration can
    // briefly keep the previous DisplayVersion, while Kimi.exe is already new.
    // OpenClaw's tray updates itself and leaves the entry at the installer's own
    // version (0.6.12 beside a 2026.7.1 tray). Prefer the executable's file version.
    // Hermes builds its desktop app from its own checkout; Hermes.exe carries Electron's
    // version, the app's own is in the package.json it was built from.
    let version = found.as_ref().and_then(|found| {
        if spec.vendor == Vendor::Hermes {
            super::feeds::hermes_checkout()
                .and_then(|dir| super::feeds::git_head(&dir))
                .map(|sha| super::feeds::commit_version(&sha))
                .or_else(|| found.path.as_deref().and_then(hermes_desktop_version))
                .or_else(|| found.version.clone())
        } else if matches!(spec.vendor, Vendor::Kimi | Vendor::OpenClaw) {
            found
                .path
                .as_deref()
                .and_then(desktop_executable_version)
                .or_else(|| found.version.clone())
        } else {
            found.version.clone()
        }
    });
    let path = found
        .as_ref()
        .and_then(|f| f.path.as_ref())
        .map(|p| p.to_string_lossy().into_owned());
    let latest_checked = check_latest && installed;
    let (mut latest, mut latest_source, mut latest_error) = if latest_checked {
        split_latest(desktop_latest(spec, version.as_deref()))
    } else {
        (None, None, None)
    };
    // What the app's own updater has downloaded counts too: WinGet's listing trails the
    // vendor by days, and a vendor rolling a version out shows it only to some machines.
    if latest_checked && spec.vendor != Vendor::Hermes {
        let executable = found.as_ref().and_then(|f| f.path.as_deref());
        let downloaded = super::feeds::downloaded_update(&super::feeds::updater_cache_dirs(
            spec.data_dirs,
            executable,
        ));
        if let Some(ready) = downloaded {
            let newer_than_installed = version
                .as_deref()
                .is_some_and(|current| crate::update::ver_lt(current, &ready));
            let newer_than_listed = latest
                .as_deref()
                .map_or(true, |listed| crate::update::ver_lt(listed, &ready));
            if newer_than_installed && newer_than_listed {
                latest = Some(ready);
                latest_source = Some("应用已下载的更新".into());
                latest_error = None;
            }
        }
    }
    let update_available = installed
        && version
            .as_deref()
            .zip(latest.as_deref())
            .is_some_and(|(cur, next)| super::feeds::is_newer(cur, next));
    let has_direct_installer = direct_desktop_installer(spec.vendor, spec.edition).is_some();
    let can_install = spec.desktop.winget_id.is_some() || has_direct_installer;
    VibeSurface {
        available: true,
        label: spec.desktop.name.into(),
        kind: "桌面端".into(),
        description: spec.desktop.description.into(),
        installed,
        status: if broken_reason.is_some() {
            "broken".into()
        } else if update_available {
            "update".into()
        } else if installed {
            "installed".into()
        } else {
            "missing".into()
        },
        version,
        probe_error: None,
        latest,
        update_available,
        path,
        command: None,
        install_method_label: method.as_deref().and_then(install_method_label),
        install_method: method,
        install_url: spec.desktop.install_url.into(),
        docs_url: spec.desktop.docs_url.into(),
        can_install,
        install_unavailable_reason: (!can_install).then(|| {
            spec.desktop
                .install_unavailable_reason
                .unwrap_or(DEFAULT_DESKTOP_UNAVAILABLE_REASON)
                .to_string()
        }),
        // Only where Stacker has a way to update it; an advertised update alone is not one
        // (ZCode used to land in the automatic group and fail with "no update source").
        can_update: installed && (spec.desktop.winget_id.is_some() || has_direct_installer),
        can_uninstall: installed,
        can_open: installed,
        health: if broken_reason.is_some() {
            "broken"
        } else if installed {
            "healthy"
        } else {
            "missing"
        }
        .into(),
        broken_reason,
        other_installs: Vec::new(),
        can_repair: false,
        repair_kind: None,
        repair_command: None,
        latest_error,
        latest_source,
        latest_checked,
        launch: found.as_ref().and_then(|f| f.launch.clone()),
    }
}

/// A latest-version lookup: the version and where it came from, `None` when the product
/// publishes no version anywhere Stacker can read, or why the lookup failed.
pub(crate) type LatestLookup = Result<Option<(String, &'static str)>, String>;

fn split_latest(lookup: LatestLookup) -> (Option<String>, Option<String>, Option<String>) {
    match lookup {
        Ok(Some((version, source))) => (Some(version), Some(source.to_string()), None),
        Ok(None) => (None, None, None),
        Err(error) => (None, None, Some(error)),
    }
}

/// Qoder's `qoder` dispatcher in `%USERPROFILE%\.qoder\entry` (`.qoder-cn` for the China
/// edition). The CLI writes it on its first run and puts it first on PATH; it hands `qoder` to
/// the desktop app or to `qodercli`, so it is not where the CLI is installed.
pub(crate) fn is_qoder_dispatcher(path: &Path) -> bool {
    let p = path.to_string_lossy().replace('/', "\\").to_lowercase();
    p.contains("\\.qoder\\entry\\") || p.contains("\\.qoder-cn\\entry\\")
}

/// xAI's Grok Build and the community `grok-dev` both install a `grok` command; the second is
/// neither this product nor xAI's, so it is never counted as the install.
pub(crate) fn is_community_grok(path: &Path) -> bool {
    let p = path.to_string_lossy().replace('/', "\\").to_lowercase();
    // Bun is how that project installs itself; xAI's installer does not use it.
    if p.contains("grok-dev") || p.contains("\\.bun\\") {
        return true;
    }
    // An npm shim names the package it runs. Only one that names `grok-dev` is refused: an
    // npm install of xAI's own package is still xAI's.
    std::fs::read_to_string(path)
        .map(|text| text.to_lowercase().contains("grok-dev"))
        .unwrap_or(false)
}

pub(crate) fn detect_install_method(spec: &ToolSpec, program: Option<&Path>) -> Option<String> {
    let program = program?;
    let p = program.to_string_lossy().replace('/', "\\").to_lowercase();
    if spec.vendor == Vendor::Claude
        && (p.contains("\\.local\\bin\\claude") || p.contains("\\.local\\share\\claude\\"))
    {
        return Some("native".into());
    }
    // The official installer puts its command folder under `Programs\OpenAI\Codex\bin`, a
    // junction into `.codex\packages\standalone`.
    if spec.vendor == Vendor::Codex
        && (p.contains("\\.codex\\")
            || p.contains("\\.local\\bin\\codex")
            || p.contains("\\programs\\openai\\codex\\bin\\"))
    {
        return Some("native".into());
    }
    if spec.vendor == Vendor::Xai && p.contains("\\.grok\\") {
        return Some("native".into());
    }
    if spec.vendor == Vendor::Cursor && p.contains("\\appdata\\local\\cursor-agent\\") {
        return Some("native".into());
    }
    if spec.vendor == Vendor::Factory && p.ends_with("\\bin\\droid.exe") {
        return Some("native".into());
    }
    if spec.vendor == Vendor::Kiro && p.contains("\\kiro-cli\\") {
        return Some("msi".into());
    }
    if spec.vendor == Vendor::MiniMax && p.contains("\\.minimax-code\\") {
        return Some("native".into());
    }
    if spec.vendor == Vendor::MiMo && p.contains("\\.mimocode\\bin\\") {
        return Some("native".into());
    }
    // The official installer's own folder is `%USERPROFILE%\.kimi-code` (its data lives there
    // too, so an uninstall removes the command, not the folder).
    if spec.vendor == Vendor::Kimi
        && (p.contains("\\.local\\bin\\kimi")
            || p.contains("\\kimi-code\\bin\\")
            || p.contains("\\.kimi-code\\bin\\")
            || p.contains("\\appdata\\local\\kimi-code\\"))
    {
        return Some("native".into());
    }
    if spec.vendor == Vendor::Antigravity
        && (p.contains("\\antigravity\\")
            || p.contains("\\.local\\bin\\agy")
            || p.contains("\\agy\\bin\\agy"))
    {
        return Some("native".into());
    }
    if spec.vendor == Vendor::Hermes
        && (p.contains("\\appdata\\local\\hermes\\") || p.contains("\\.hermes\\"))
    {
        return Some("native".into());
    }
    if spec.vendor == Vendor::OpenCode {
        if p.contains("\\scoop\\shims\\") || p.contains("\\scoop\\apps\\opencode\\") {
            return Some("scoop".into());
        }
        if p.contains("\\chocolatey\\bin\\") || p.contains("\\chocolatey\\lib\\opencode\\") {
            return Some("chocolatey".into());
        }
        if p.contains("\\.opencode\\bin\\") {
            return Some("native".into());
        }
    }
    if let Some(pkg) = spec.cli.npm_package {
        if is_conda_path(&p) && is_npm_shim(program, pkg) {
            return Some("conda-npm".into());
        }
        if is_npm_shim(program, pkg) {
            return Some("npm".into());
        }
    }
    // Most installations can be identified from their resolved executable. Only
    // query WinGet when the path itself is inconclusive; `winget list` is slow and
    // may contact package sources, so running it for every npm/native CLI makes a
    // full agent scan unnecessarily expensive.
    if let Some(id) = spec.cli.winget_id {
        if p.contains("\\winget\\links\\") || winget_package_installed(id) {
            return Some("winget".into());
        }
    }
    None
}

pub(crate) fn install_method_label(method: &str) -> Option<String> {
    match method {
        "winget" => Some("WinGet".into()),
        "npm" => Some("npm".into()),
        "native" => Some("官方安装".into()),
        "scoop" => Some("Scoop".into()),
        "chocolatey" => Some("Chocolatey".into()),
        "conda-npm" => Some("Conda npm".into()),
        "appx" => Some("应用商店版".into()),
        "shortcut" => Some("快捷方式".into()),
        "registry" => Some("安装程序版".into()),
        "app" => Some("本地应用".into()),
        "download" => Some("官方下载".into()),
        _ => None,
    }
}

pub(crate) fn is_conda_path(path: &str) -> bool {
    path.contains("\\anaconda")
        || path.contains("\\miniconda")
        || path.contains("\\mambaforge")
        || path.contains("\\miniforge")
        || path.contains("\\conda\\envs\\")
        || path.contains("\\envs\\")
}

pub(crate) fn is_npm_shim(program: &Path, npm_package: &str) -> bool {
    let p = program.to_string_lossy().replace('/', "\\").to_lowercase();
    if p.contains("\\node_modules\\")
        || p.contains("\\npm\\")
        || p.contains("\\node_global\\")
        || p.contains("\\npm-global\\")
    {
        return true;
    }
    let pkg = npm_package.to_lowercase();
    std::fs::read_to_string(program)
        .map(|s| s.to_lowercase().contains(&pkg) || s.to_lowercase().contains("node_modules"))
        .unwrap_or(false)
}

/// The first `x.y.z` in a line of text, without a leading `v`.
pub(crate) fn first_semver(text: &str) -> Option<String> {
    text.split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ')' | ','))
        .map(|word| word.trim_start_matches(['v', 'V']))
        .find(|word| {
            let parts: Vec<&str> = word.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        })
        .map(str::to_string)
}

/// Where a CLI's latest version is published.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CliSource {
    Winget(&'static str),
    MimoRelease,
    /// The version an official installer script pins (Cursor, Factory).
    Script(&'static str),
    KiroManifest,
    /// `pyproject.toml` on the branch `hermes update` pulls.
    HermesRepo,
    Npm(&'static str),
    /// Nowhere Stacker can read: the product checks for updates itself.
    None,
}

pub(crate) fn cli_source(spec: &ToolSpec, method: Option<&str>) -> CliSource {
    if let (Some("winget"), Some(id)) = (method, spec.cli.winget_id) {
        return CliSource::Winget(id);
    }
    if spec.vendor == Vendor::Antigravity {
        return CliSource::None;
    }
    if spec.vendor == Vendor::Hermes {
        return CliSource::HermesRepo;
    }
    if method == Some("native") && spec.vendor == Vendor::MiMo {
        return CliSource::MimoRelease;
    }
    match spec.vendor {
        Vendor::Cursor => return CliSource::Script(CURSOR_CLI_SCRIPT),
        Vendor::Factory => return CliSource::Script(FACTORY_CLI_SCRIPT),
        Vendor::Kiro => return CliSource::KiroManifest,
        _ => {}
    }
    if method == Some("native") && spec.vendor == Vendor::Claude {
        return CliSource::None;
    }
    match spec.cli.npm_package {
        Some(pkg) => CliSource::Npm(pkg),
        None => CliSource::None,
    }
}

/// A CLI's latest version from where it is published. With no such place the answer is
/// `Ok(None)`, never the installed version passed off as the latest.
pub(crate) fn latest_for_cli(spec: &ToolSpec, method: Option<&str>) -> LatestLookup {
    match cli_source(spec, method) {
        CliSource::Winget(id) => winget_latest(id, None).map(|version| Some((version, "WinGet"))),
        CliSource::MimoRelease => mimo_latest().map(|version| Some((version, "MiMo 官方发布"))),
        CliSource::Script(url) => script_latest(url)
            .map(|version| Some((cursor_build(&version).unwrap_or(version), "官方安装脚本"))),
        CliSource::KiroManifest => kiro_latest().map(|version| Some((version, "Kiro 官方发布"))),
        CliSource::HermesRepo => {
            super::feeds::hermes_latest().map(|version| Some((version, "Hermes main 分支")))
        }
        CliSource::Npm(pkg) => npm_latest(pkg).map(|version| Some((version, "npm"))),
        CliSource::None => Ok(None),
    }
}

/// The build Cursor's installer script pins, e.g. `$version = '2026.09.28-64d2043'`.
pub(crate) const CURSOR_CLI_SCRIPT: &str = "https://cursor.com/install?win32=true";
/// The version Factory's installer script pins, e.g. `$version = "0.230.0"`.
pub(crate) const FACTORY_CLI_SCRIPT: &str = "https://app.factory.ai/cli/windows";
/// The manifest Kiro's installer reads its version from.
pub(crate) const KIRO_CLI_MANIFEST: &str =
    "https://prod.download.cli.kiro.dev/stable/latest/manifest.json";

/// The value of the first `$version = '…'` (or `"…"`) assignment in an installer script.
pub(crate) fn script_version(script: &str) -> Option<String> {
    let at = script.find("$version")?;
    let rest = script[at + "$version".len()..]
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let quote = rest.chars().next().filter(|ch| *ch == '\'' || *ch == '"')?;
    let value = rest[1..].split(quote).next()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Cursor numbers its CLI by build date plus a commit, `2026.09.28-64d2043`; only the date
/// orders builds, so the commit is left off before comparing.
pub(crate) fn cursor_build(text: &str) -> Option<String> {
    text.split(|ch: char| ch.is_whitespace() || ch == '-')
        .map(|word| word.trim_start_matches(['v', 'V']))
        .find(|word| {
            let parts: Vec<&str> = word.split('.').collect();
            parts.len() == 3
                && parts[0].len() == 4
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        })
        .map(str::to_string)
}

pub(crate) fn fetch_text(url: &str) -> Result<String, String> {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(8))
        .build()
        .get(url)
        .call()
        .map_err(|e| format!("查询最新版本失败：{e}"))?
        .into_string()
        .map_err(|e| format!("查询最新版本失败：{e}"))
}

pub(crate) fn script_latest(url: &str) -> Result<String, String> {
    script_version(&fetch_text(url)?).ok_or_else(|| "官方安装脚本里没有版本号".to_string())
}

pub(crate) fn kiro_latest() -> Result<String, String> {
    let manifest: Value =
        serde_json::from_str(&fetch_text(KIRO_CLI_MANIFEST)?).map_err(|e| e.to_string())?;
    manifest["version"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "Kiro 的发布清单里没有版本号".to_string())
}

/// Latest MiMo Code release published on Xiaomi's CDN (plain text, e.g. `v0.1.14`).
pub(crate) fn mimo_latest() -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(8))
        .build();
    agent
        .get("https://mimocode.cnbj1.mi-fds.com/mimocode/mimocode/releases/latest")
        .call()
        .map_err(|e| format!("查询最新版本失败：{e}"))?
        .into_string()
        .map(|text| text.trim().trim_start_matches('v').to_string())
        .map_err(|e| format!("查询最新版本失败：{e}"))
}

/// Where a desktop app's latest version is published.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DesktopSource {
    KimiDownload,
    /// An electron-builder feed: a GitHub release's download base, or a vendor's own CDN
    /// folder holding `latest.yml` (Agnes Code).
    ElectronFeed(&'static str),
    /// A repository's newest stable release, read through the GitHub API.
    GithubLatest(&'static str),
    Winget(&'static str, Option<&'static str>),
    /// The manifest ZCode's own updater reads.
    ZcodeManifest,
    /// WorkBuddy's update service at this base URL; it answers for the installed version.
    WorkBuddyUpdate(&'static str),
    /// Qoder's desktop update service at this base URL; it answers for the installed version.
    QoderUpdate(&'static str),
    /// The version resource of the official installer at this URL.
    InstallerVersion(&'static str),
    /// `apps/desktop/package.json` on the branch the Hermes updater pulls.
    HermesRepo,
    /// TRAE Work's own `check_update`, as this edition's app sends it.
    TraeCheckUpdate(&'static super::feeds::TraeFeed),
    /// Nowhere Stacker can read: the app checks for updates itself.
    None,
}

pub(crate) fn desktop_source(spec: &ToolSpec) -> DesktopSource {
    let cn = spec.edition == Edition::Cn;
    match spec.vendor {
        Vendor::Kimi => return DesktopSource::KimiDownload,
        Vendor::ZCode => return DesktopSource::ZcodeManifest,
        Vendor::WorkBuddy => {
            return DesktopSource::WorkBuddyUpdate(if cn {
                "https://copilot.tencent.com"
            } else {
                "https://www.workbuddy.ai"
            })
        }
        Vendor::Qoder => {
            return DesktopSource::QoderUpdate(if cn {
                "https://gateway.qoder.com.cn"
            } else {
                "https://center.qoder.sh"
            })
        }
        Vendor::Trae => {
            return DesktopSource::TraeCheckUpdate(if cn {
                &super::feeds::TRAE_CN
            } else {
                &super::feeds::TRAE_GLOBAL
            })
        }
        Vendor::MiMo => {
            return DesktopSource::InstallerVersion(if cn {
                MIMO_DESKTOP_CN_INSTALLER
            } else {
                MIMO_DESKTOP_GLOBAL_INSTALLER
            })
        }
        Vendor::Hermes => return DesktopSource::HermesRepo,
        // The Windows companion ships from its own repository, not the main openclaw one.
        Vendor::OpenClaw => return DesktopSource::GithubLatest(OPENCLAW_WINDOWS_REPO),
        _ => {}
    }
    if let Some(installer) = direct_desktop_installer(spec.vendor, spec.edition) {
        if let InstallerSource::ElectronRelease { base_url } = installer.source {
            return DesktopSource::ElectronFeed(base_url);
        }
    }
    match spec.desktop.winget_id {
        Some(id) => DesktopSource::Winget(id, spec.desktop.winget_source),
        None => DesktopSource::None,
    }
}

/// A desktop app's latest version: an update Claude has already downloaded first, then where
/// the app is published. With no such place the answer is `Ok(None)`, never the installed
/// version passed off as the latest.
pub(crate) fn desktop_latest(spec: &ToolSpec, current: Option<&str>) -> LatestLookup {
    if spec.vendor == Vendor::Claude {
        if let Some(ready) = claude_desktop_ready_update(current) {
            return Ok(Some((ready, "Claude 已下载的更新")));
        }
    }
    match desktop_source(spec) {
        DesktopSource::KimiDownload => {
            kimi_work_latest().map(|version| Some((version, "Kimi 官方下载地址")))
        }
        DesktopSource::ElectronFeed(base_url) => {
            super::install::direct::electron_release_latest(base_url)
                .map(|release| Some((release.version, electron_feed_label(base_url))))
        }
        DesktopSource::GithubLatest(repo) => {
            super::feeds::github_latest(repo).map(|version| Some((version, "GitHub Releases")))
        }
        // WinGet lists Microsoft Store apps with "Version: Unknown"; the Store publishes no
        // version and updates them itself, which is "no public source", not a failed lookup.
        DesktopSource::Winget(id, Some("msstore")) => Ok(winget_latest(id, Some("msstore"))
            .ok()
            .map(|version| (version, "WinGet"))),
        DesktopSource::Winget(id, source) => {
            winget_latest(id, source).map(|version| Some((version, "WinGet")))
        }
        DesktopSource::ZcodeManifest => {
            super::feeds::zcode_latest().map(|version| Some((version, "ZCode 官方更新服务")))
        }
        DesktopSource::WorkBuddyUpdate(base) => super::feeds::workbuddy_latest(base, current)
            .map(|version| Some((version, "WorkBuddy 官方更新服务"))),
        DesktopSource::QoderUpdate(base) => super::feeds::qoder_latest(base, current)
            .map(|version| Some((version, "Qoder 官方更新服务"))),
        DesktopSource::TraeCheckUpdate(feed) => {
            super::feeds::trae_work_latest(feed).map(|version| Some((version, "TRAE 官方更新服务")))
        }
        DesktopSource::InstallerVersion(url) => {
            super::feeds::installer_version(url).map(|version| Some((version, "官方安装包")))
        }
        DesktopSource::HermesRepo => {
            // Built from the same checkout `hermes update` pulls: behind main is an update.
            super::feeds::hermes_latest().map(|version| Some((version, "Hermes main 分支")))
        }
        DesktopSource::None => Ok(None),
    }
}

/// Where an electron-builder feed lives, named the way the page shows a version's source.
fn electron_feed_label(base_url: &str) -> &'static str {
    if base_url.starts_with("https://github.com/") {
        "GitHub Releases"
    } else {
        "官方发布清单"
    }
}

/// The version WinGet lists for a package. A listing without a usable version (the Store
/// shows "Unknown") is a failed lookup, not a version.
pub(crate) fn winget_latest(id: &str, source: Option<&str>) -> Result<String, String> {
    usable_winget_version(winget_available_update(id, source)?)
}

fn usable_winget_version(listed: Option<String>) -> Result<String, String> {
    match listed {
        Some(version) if version.chars().any(|ch| ch.is_ascii_digit()) => Ok(version),
        _ => Err("WinGet 没有给出版本号".into()),
    }
}

fn kimi_work_latest() -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(5))
        .redirects(3)
        .build();
    let response = agent
        .get("https://appsupport.moonshot.cn/api/app/pkg/latest/windows/download")
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("获取 Kimi Work 最新版本失败：{e}"))?;
    let final_url = response.get_url();
    let file = final_url.rsplit('/').next().unwrap_or_default();
    let version = file
        .strip_prefix("kimi_")
        .and_then(|value| value.strip_suffix(".exe"))
        .unwrap_or_default()
        .trim();
    if version.is_empty() {
        Err("Kimi Work 下载地址未包含版本信息。".into())
    } else {
        Ok(version.to_string())
    }
}

pub(crate) fn claude_desktop_ready_update(current: Option<&str>) -> Option<String> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let candidates = [
        local.join("Claude-3p\\Logs\\main.log"),
        local.join("Claude-3p\\logs\\main.log"),
    ];
    let mut latest_ready: Option<String> = None;
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(version) = parse_claude_ready_update_version(line) {
                latest_ready = Some(version);
            }
        }
    }
    latest_ready.filter(|next| {
        current
            .map(|cur| crate::update::ver_lt(cur, next))
            .unwrap_or(true)
    })
}

pub(crate) fn parse_claude_ready_update_version(line: &str) -> Option<String> {
    if !line.contains("Update downloaded and ready to install") {
        return None;
    }
    let marker = "releaseName: 'Claude ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('\'')?;
    let version = rest[..end].trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// A product's desktop app. MiniMax installs its China and global editions under one name
/// and into one folder; the update feed each was built with (`resources/app-update.yml`)
/// tells them apart, and an install of the other edition is not this one.
pub(crate) fn detect_desktop_for(spec: &ToolSpec) -> Option<DesktopFound> {
    let found = detect_desktop_app(&spec.desktop)?;
    if spec.vendor == Vendor::MiniMax {
        let feed = found
            .path
            .as_deref()
            .and_then(Path::parent)
            .and_then(|dir| {
                std::fs::read_to_string(dir.join("resources").join("app-update.yml")).ok()
            })
            .unwrap_or_default();
        if let Some(global) = minimax_feed_is_global(&feed) {
            if global != (spec.edition == Edition::Global) {
                return None;
            }
        }
    }
    Some(found)
}

/// Which MiniMax edition an `app-update.yml` belongs to: `minimax.io` is the global feed,
/// `minimax.chat` the China one; anything else says nothing.
pub(crate) fn minimax_feed_is_global(feed: &str) -> Option<bool> {
    let feed = feed.to_ascii_lowercase();
    if feed.contains("minimax.io") {
        Some(true)
    } else if feed.contains("minimax.chat") {
        Some(false)
    } else {
        None
    }
}

pub(crate) fn detect_desktop_app(spec: &DesktopSpec) -> Option<DesktopFound> {
    let accept = |found: DesktopFound| (!is_rejected_sibling(spec, &found)).then_some(found);
    desktop_appx_package(spec)
        .and_then(accept)
        .or_else(|| desktop_registry(spec).and_then(accept))
        .or_else(|| desktop_start_menu_shortcut(spec).and_then(accept))
        .or_else(|| desktop_exe_candidate(spec).and_then(accept))
        .or_else(|| {
            spec.winget_id.and_then(|id| {
                winget_package_installed(id).then(|| DesktopFound {
                    path: None,
                    version: None,
                    method: Some("winget".into()),
                    uninstall: None,
                    launch: None,
                })
            })
        })
}

/// A look-alike product (e.g. the Qoder IDE) installed with the same executable name.
fn is_rejected_sibling(spec: &DesktopSpec, found: &DesktopFound) -> bool {
    let Some(dir) = found.path.as_deref().and_then(Path::parent) else {
        return false;
    };
    spec.reject_sibling_files
        .iter()
        .any(|name| dir.join(name).is_file())
}

#[cfg(windows)]
pub(crate) fn desktop_appx_package(spec: &DesktopSpec) -> Option<DesktopFound> {
    if spec.appx_names.is_empty() {
        return None;
    }
    let names = spec
        .appx_names
        .iter()
        .map(|name| ps_single_quoted(name))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$names = @({names})
foreach ($name in $names) {{
  $pkg = Get-AppxPackage -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($pkg) {{
    $entry = (Get-AppxPackageManifest $pkg).Package.Applications.Application | Select-Object -First 1
    $appId = if ($entry.Id) {{ "$($pkg.PackageFamilyName)!$($entry.Id)" }} else {{ "$($pkg.PackageFamilyName)!App" }}
    Write-Output ("{{0}}`t{{1}}`t{{2}}`t{{3}}`t{{4}}" -f $pkg.Name, $pkg.Version, $pkg.InstallLocation, $appId, $pkg.PackageFullName)
    break
  }}
}}
"#
    );
    let text = run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "Get-AppxPackage",
        // The start-menu lookup this used took seconds on slow machines and pushed the whole
        // check past its limit; the app id now comes from the package's own manifest.
        Duration::from_secs(20),
    )
    .ok()?;
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let parts: Vec<&str> = line.splitn(5, '\t').collect();
    if parts.len() < 4 {
        return None;
    }
    let version = (!parts[1].trim().is_empty()).then(|| parts[1].trim().to_string());
    let install = parts[2].trim();
    let app_id = parts[3].trim();
    let package_full_name = parts.get(4).map(|s| s.trim()).unwrap_or_default();
    let path = (!install.is_empty()).then(|| PathBuf::from(install));
    let launch = (!app_id.is_empty()).then(|| format!("shell:AppsFolder\\{app_id}"));
    let uninstall = (!package_full_name.is_empty()).then(|| format!("appx:{package_full_name}"));
    Some(DesktopFound {
        path,
        version,
        method: Some("appx".into()),
        uninstall,
        launch,
    })
}

#[cfg(not(windows))]
pub(crate) fn desktop_appx_package(_: &DesktopSpec) -> Option<DesktopFound> {
    None
}

#[cfg(windows)]
pub(crate) fn desktop_registry(spec: &DesktopSpec) -> Option<DesktopFound> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;
    let paths = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for path in paths {
            let Ok(uninstall) = root.open_subkey_with_flags(path, KEY_READ) else {
                continue;
            };
            for name in uninstall.enum_keys().flatten() {
                let Ok(key) = uninstall.open_subkey_with_flags(&name, KEY_READ) else {
                    continue;
                };
                let display: String = key.get_value("DisplayName").unwrap_or_default();
                if !desktop_name_matches(&display, spec.keywords, spec.excludes) {
                    continue;
                }
                let version: Option<String> = key.get_value("DisplayVersion").ok();
                let quiet: Option<String> = key.get_value("QuietUninstallString").ok();
                let normal: Option<String> = key.get_value("UninstallString").ok();
                let uninstall_string = quiet.or(normal);
                let icon: Option<String> = key.get_value("DisplayIcon").ok();
                let install_location: Option<String> = key.get_value("InstallLocation").ok();
                let icon_file = icon.as_deref().and_then(parse_registered_file);
                let uninstall_file = uninstall_string
                    .as_deref()
                    .and_then(executable_from_command);
                let path = icon_file
                    .as_ref()
                    .filter(|path| is_launchable_desktop_exe(path, spec.keywords, spec.excludes))
                    .cloned()
                    .or_else(|| {
                        install_location
                            .as_deref()
                            .map(str::trim)
                            .filter(|location| !location.is_empty())
                            .and_then(|location| {
                                find_exe_in_dir(
                                    &PathBuf::from(location),
                                    spec.keywords,
                                    spec.excludes,
                                )
                            })
                    })
                    .or_else(|| {
                        icon_file
                            .as_deref()
                            .and_then(Path::parent)
                            .and_then(|dir| find_exe_in_dir(dir, spec.keywords, spec.excludes))
                    })
                    .or_else(|| {
                        uninstall_file
                            .as_deref()
                            .and_then(Path::parent)
                            .and_then(|dir| find_exe_in_dir(dir, spec.keywords, spec.excludes))
                    });
                return Some(DesktopFound {
                    path,
                    version,
                    method: Some("registry".into()),
                    uninstall: uninstall_string,
                    launch: None,
                });
            }
        }
    }
    None
}

#[cfg(not(windows))]
pub(crate) fn desktop_registry(_: &DesktopSpec) -> Option<DesktopFound> {
    None
}

pub(crate) fn desktop_start_menu_shortcut(spec: &DesktopSpec) -> Option<DesktopFound> {
    for root in start_menu_roots() {
        if let Some(shortcut) = find_shortcut_recursive(&root, spec.keywords, spec.excludes, 4) {
            // An app with no uninstall entry (unpacked to a folder of the user's choosing) is
            // found only through its shortcut; the program it points to carries the version.
            // A shortcut left behind by an uninstall points at nothing and counts for nothing.
            let target = shortcut_target(&shortcut);
            if target.as_deref().is_some_and(|target| !target.exists()) {
                continue;
            }
            let target = target.filter(|target| {
                target
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                    && target.is_file()
            });
            let version = target.as_deref().and_then(desktop_executable_version);
            return Some(DesktopFound {
                path: Some(target.unwrap_or(shortcut)),
                version,
                method: Some("shortcut".into()),
                uninstall: None,
                launch: None,
            });
        }
    }
    None
}

/// The program a `.lnk` shortcut starts.
#[cfg(windows)]
fn shortcut_target(shortcut: &Path) -> Option<PathBuf> {
    let path = ps_single_quoted(&shortcut.to_string_lossy());
    let script = format!(
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; (New-Object -ComObject WScript.Shell).CreateShortcut({path}).TargetPath"
    );
    let output = run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "读取快捷方式",
        Duration::from_secs(5),
    )
    .ok()?;
    let target = output.trim();
    (!target.is_empty()).then(|| PathBuf::from(target))
}

#[cfg(not(windows))]
fn shortcut_target(_: &Path) -> Option<PathBuf> {
    None
}

pub(crate) fn desktop_exe_candidate(spec: &DesktopSpec) -> Option<DesktopFound> {
    for path in desktop_candidate_paths(spec) {
        if path.is_file() {
            return Some(DesktopFound {
                path: Some(path),
                version: None,
                method: Some("app".into()),
                uninstall: None,
                launch: None,
            });
        }
    }
    None
}

/// The version of the Hermes Desktop build that exists: the `package.json` packed into its
/// `app.asar`. The checkout's own `package.json` is not it: `hermes update` pulls new source
/// first, and the rebuild after it can still fail. Found through its Start Menu shortcut the
/// path is the `.lnk`, so the build's usual place stands in for `win-unpacked\Hermes.exe`.
pub(crate) fn hermes_desktop_version(found: &Path) -> Option<String> {
    let beside_exe = found
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        .then(|| found.parent().map(|dir| dir.join(r"resources\app.asar")))
        .flatten();
    let default = std::env::var_os("LOCALAPPDATA").map(|local| {
        PathBuf::from(local)
            .join(r"hermes\hermes-agent\apps\desktop\release\win-unpacked\resources\app.asar")
    });
    [beside_exe, default]
        .into_iter()
        .flatten()
        .find_map(|asar| super::feeds::asar_package_version(&asar))
}

pub(crate) fn desktop_executable_version(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        let path = ps_single_quoted(&path.to_string_lossy());
        let script = format!(
            "$version = (Get-Item -LiteralPath {path} -ErrorAction Stop).VersionInfo.FileVersion; if ($version) {{ Write-Output $version }}"
        );
        let output = run_powershell(
            &[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ],
            "读取桌面应用版本",
            Duration::from_secs(3),
        )
        .ok()?;
        normalize_desktop_version(&output)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

pub(crate) fn normalize_desktop_version(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches(['v', 'V']);
    let version = value
        .split_whitespace()
        .map(|part| part.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-'))
        .find(|part| part.chars().any(|c| c.is_ascii_digit()))
        .unwrap_or_default()
        .trim();
    if version.is_empty() {
        return None;
    }
    let mut parts = version.split('.').collect::<Vec<_>>();
    while parts.len() > 3 && parts.last().is_some_and(|part| *part == "0") {
        parts.pop();
    }
    Some(parts.join("."))
}

/// A keyword matches anywhere in the name, or only at its start when written `^keyword`: a
/// word as common as "factory" otherwise matches Format Factory, which then gets uninstalled.
pub(crate) fn desktop_name_matches(name: &str, keywords: &[&str], excludes: &[&str]) -> bool {
    let lower = name.to_lowercase();
    keywords.iter().any(|k| match k.strip_prefix('^') {
        Some(start) => lower.starts_with(&start.to_lowercase()),
        None => lower.contains(&k.to_lowercase()),
    }) && !excludes.iter().any(|k| lower.contains(&k.to_lowercase()))
}

pub(crate) fn parse_registered_file(value: &str) -> Option<PathBuf> {
    let mut s = value.trim().trim_matches('"').to_string();
    if let Some(idx) = s.rfind(',') {
        if s[idx + 1..].chars().all(|c| c == '-' || c.is_ascii_digit()) {
            s.truncate(idx);
        }
    }
    let p = PathBuf::from(s.trim().trim_matches('"'));
    p.is_file().then_some(p)
}

pub(crate) fn executable_from_command(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let executable = if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        &rest[..end]
    } else {
        let lower = value.to_ascii_lowercase();
        let end = lower.find(".exe")? + ".exe".len();
        &value[..end]
    };
    let path = PathBuf::from(executable.trim());
    path.is_file().then_some(path)
}

pub(crate) fn is_launchable_desktop_exe(path: &Path, keywords: &[&str], excludes: &[&str]) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lower = name.to_lowercase();
    if !lower.ends_with(".exe")
        || [
            "uninstall",
            "unins",
            "installer",
            "setup",
            "update",
            "crashpad",
            "helper",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return false;
    }
    desktop_name_matches(&lower, keywords, excludes)
}

pub(crate) fn find_exe_in_dir(dir: &Path, keywords: &[&str], excludes: &[&str]) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut fallback = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_launchable_desktop_exe(&path, keywords, excludes) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_lowercase()
            .replace([' ', '-', '_'], "");
        if keywords.iter().any(|keyword| {
            stem == keyword
                .trim_start_matches('^')
                .to_lowercase()
                .replace([' ', '-', '_'], "")
        }) {
            return Some(path);
        }
        fallback.get_or_insert(path);
    }
    fallback
}

pub(crate) fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        roots.push(PathBuf::from(programdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    roots
}

pub(crate) fn find_shortcut_recursive(
    dir: &Path,
    keywords: &[&str],
    excludes: &[&str],
    depth: usize,
) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_shortcut_recursive(&path, keywords, excludes, depth - 1) {
                return Some(found);
            }
            continue;
        }
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        if name.ends_with(".lnk") && desktop_name_matches(&name, keywords, excludes) {
            return Some(path);
        }
    }
    None
}

pub(crate) fn desktop_candidate_paths(spec: &DesktopSpec) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let pf86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
    let mut add = |base: &Option<PathBuf>, rest: &str| {
        if let Some(base) = base {
            out.push(base.join(rest));
        }
    };
    match spec.name {
        name if name.contains("Claude") => {
            add(&local, "Programs\\Claude\\Claude.exe");
            add(&pf, "Claude\\Claude.exe");
        }
        name if name.contains("Codex") => {
            add(&local, "Programs\\Codex\\Codex.exe");
            add(&pf, "Codex\\Codex.exe");
        }
        name if name.contains("Antigravity") => {
            add(&local, "Programs\\Antigravity\\Antigravity.exe");
            add(&local, "Google\\Antigravity\\Application\\antigravity.exe");
            add(&pf, "Google\\Antigravity\\Application\\antigravity.exe");
            add(&pf86, "Google\\Antigravity\\Application\\antigravity.exe");
        }
        name if name.contains("OpenCode") => {
            add(&local, "Programs\\OpenCode\\OpenCode.exe");
            add(&pf, "OpenCode\\OpenCode.exe");
        }
        name if name.contains("ZCode") => {
            add(&local, "Programs\\ZCode\\ZCode.exe");
            add(&pf, "ZCode\\ZCode.exe");
        }
        name if name.contains("Kimi Work") => {
            add(&local, "Programs\\kimi-desktop\\Kimi.exe");
            add(&local, "Programs\\Kimi\\Kimi.exe");
            add(&local, "Programs\\Kimi Work\\Kimi Work.exe");
            add(&local, "Kimi\\Kimi.exe");
            add(&local, "Kimi Work\\Kimi Work.exe");
            add(&pf, "kimi-desktop\\Kimi.exe");
            add(&pf, "Kimi\\Kimi.exe");
            add(&pf, "Kimi Work\\Kimi Work.exe");
        }
        name if name.contains("WorkBuddy") && name.contains("国际版") => {
            add(&local, "Programs\\WorkBuddy AI\\WorkBuddyAI.exe");
            add(&local, "WorkBuddyAI\\WorkBuddyAI.exe");
            add(&pf, "WorkBuddy AI\\WorkBuddyAI.exe");
        }
        name if name.contains("WorkBuddy") => {
            add(&local, "Programs\\WorkBuddy\\WorkBuddy.exe");
            add(&local, "WorkBuddy\\WorkBuddy.exe");
            add(&pf, "WorkBuddy\\WorkBuddy.exe");
        }
        name if name.contains("PI-Desktop") => {
            add(&local, "Programs\\PI-Desktop\\PI-Desktop.exe");
            add(&pf, "PI-Desktop\\PI-Desktop.exe");
        }
        name if name.contains("Qoder") && name.contains("中国版") => {
            add(&local, "Programs\\Qoder CN\\Qoder CN.exe");
            add(&pf, "Qoder CN\\Qoder CN.exe");
        }
        name if name.contains("Qoder") => {
            add(&local, "Programs\\Qoder\\Qoder.exe");
            add(&pf, "Qoder\\Qoder.exe");
        }
        name if name.contains("TRAE Work") => {
            add(&local, "Programs\\TRAE SOLO CN\\TRAE SOLO CN.exe");
            add(&local, "TRAE SOLO CN\\TRAE SOLO CN.exe");
            add(&pf, "TRAE SOLO CN\\TRAE SOLO CN.exe");
        }
        name if name.contains("TRAE") => {
            add(&local, "Programs\\TRAE SOLO\\TRAE SOLO.exe");
            add(&local, "TRAE SOLO\\TRAE SOLO.exe");
            add(&pf, "TRAE SOLO\\TRAE SOLO.exe");
        }
        name if name.contains("OpenClaw") => {
            add(&local, "Programs\\OpenClaw\\OpenClaw.exe");
            add(&local, "OpenClaw\\OpenClaw.exe");
            add(&pf, "OpenClaw\\OpenClaw.exe");
        }
        name if name.contains("Hermes") => {
            add(&local, "Programs\\Hermes\\Hermes.exe");
            add(&local, "hermes\\desktop\\Hermes.exe");
            add(&local, "hermes\\Hermes.exe");
            add(&pf, "Hermes\\Hermes.exe");
        }
        _ => {}
    }
    out
}

pub(crate) fn winget_package_installed(id: &str) -> bool {
    run_winget(
        &["list", "--id", id, "--exact", "--accept-source-agreements"],
        Duration::from_secs(25),
    )
    .map(|text| text.to_lowercase().contains(&id.to_lowercase()))
    .unwrap_or(false)
}

pub(crate) fn winget_available_update(
    id: &str,
    source: Option<&str>,
) -> Result<Option<String>, String> {
    // Scan workers run in parallel, and WinGet processes started together sometimes exit
    // failing with no output while they contend for the source lock. One lookup at a time,
    // and one more try after a failure.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let args = winget_args("show", id, source, true);
    debug_assert!(winget_query_is_read_only(&args));
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // 15 seconds was not enough while WinGet refreshed its sources: the lookup timed out and
    // the agent looked up to date.
    let text = run_winget(&refs, Duration::from_secs(40)).or_else(|_| {
        std::thread::sleep(Duration::from_secs(2));
        run_winget(&refs, Duration::from_secs(40))
    })?;
    Ok(winget_latest_version_from_show(&text))
}

pub(crate) fn winget_latest_version_from_show(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let normalized = line.trim().replace('：', ":");
        let (key, value) = normalized.split_once(':')?;
        let key = key.trim().to_ascii_lowercase();
        if key == "version" || key == "版本" {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        } else {
            None
        }
    })
}

const NPM_DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

/// `/latest` document URLs, the user's configured npm registry first. The small
/// per-version document avoids downloading a package's whole metadata.
pub(crate) fn npm_latest_urls(package: &str, configured: Option<&str>) -> Vec<String> {
    let mut urls = Vec::new();
    for registry in [
        configured.unwrap_or(NPM_DEFAULT_REGISTRY),
        NPM_DEFAULT_REGISTRY,
    ] {
        let url = format!("{}/{package}/latest", registry.trim().trim_end_matches('/'));
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

/// The registry npm itself uses, read once per ten minutes (npm config lookups are slow).
type RegistryCache = std::sync::Mutex<Option<(std::time::Instant, Option<String>)>>;

fn configured_npm_registry() -> Option<String> {
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    static CACHE: OnceLock<RegistryCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((at, value)) = guard.as_ref() {
            if at.elapsed() < Duration::from_secs(600) {
                return value.clone();
            }
        }
    }
    let value = resolve_command(&["npm.cmd", "npm.exe", "npm.bat"])
        .and_then(|npm| {
            run_command_text(
                &npm,
                &["config", "get", "registry"],
                "读取 npm 源",
                Duration::from_secs(8),
            )
            .ok()
        })
        .map(|text| text.trim().to_string())
        .filter(|text| text.starts_with("http"));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), value.clone()));
    }
    value
}

pub(crate) fn npm_latest(package: &str) -> Result<String, String> {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(8));
    if let Some(proxy) = crate::agents::net::stacker_proxy() {
        if let Ok(proxy) = ureq::Proxy::new(&proxy) {
            builder = builder.proxy(proxy);
        }
    }
    let agent = builder.build();
    let mut last_error = String::new();
    for url in npm_latest_urls(package, configured_npm_registry().as_deref()) {
        let response = agent
            .get(&url)
            .set("User-Agent", "Stacker")
            .call()
            .map_err(|e| e.to_string())
            .and_then(|response| response.into_string().map_err(|e| e.to_string()));
        match response {
            Ok(body) => {
                let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
                if let Some(version) = v.get("version").and_then(Value::as_str) {
                    return Ok(version.to_string());
                }
                last_error = "npm 源未返回版本号".into();
            }
            Err(error) => last_error = error,
        }
    }
    Err(format!("查询最新版本失败：{last_error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anchored_keyword_does_not_match_another_app_that_contains_it() {
        let excludes = &["satisfactory"];
        assert!(desktop_name_matches("Factory", &["^factory"], excludes));
        assert!(desktop_name_matches(
            "factory-desktop.exe",
            &["^factory"],
            excludes
        ));
        assert!(!desktop_name_matches(
            "Format Factory 5.18",
            &["^factory"],
            excludes
        ));
        assert!(desktop_name_matches(
            "Format Factory 5.18",
            &["factory"],
            excludes
        ));
    }

    #[test]
    fn the_community_grok_is_not_taken_for_xai_s() {
        let dir = tempfile::tempdir().unwrap();
        // An npm shim that runs the community package.
        let community = dir.path().join("grok.cmd");
        std::fs::write(
            &community,
            "@\"%~dp0\\node_modules\\grok-dev\\dist\\cli.js\" %*",
        )
        .unwrap();
        assert!(is_community_grok(&community));
        // An npm shim of xAI's own package, and the official installer's binary, are kept.
        let official_npm = dir.path().join("official.cmd");
        std::fs::write(
            &official_npm,
            "@\"%~dp0\\node_modules\\@xai-official\\grok\\bin\\grok\" %*",
        )
        .unwrap();
        assert!(!is_community_grok(&official_npm));
        assert!(!is_community_grok(Path::new(
            "C:\\Users\\me\\.grok\\bin\\grok.exe"
        )));
        // Bun is how the community project installs itself.
        assert!(is_community_grok(Path::new(
            "C:\\Users\\me\\.bun\\bin\\grok.exe"
        )));
    }

    #[test]
    fn minimax_editions_are_told_apart_by_their_update_feed() {
        let global = "provider: generic
url: https://file.cdn.minimax.io/public/minimax-agent/release
";
        let china = "provider: generic
url: https://filecdn.minimax.chat/public/minimax-agent/release
";
        assert_eq!(minimax_feed_is_global(global), Some(true));
        assert_eq!(minimax_feed_is_global(china), Some(false));
        assert_eq!(minimax_feed_is_global(""), None);
    }

    #[test]
    fn an_installer_script_names_the_version_it_installs() {
        assert_eq!(
            script_version("$downloadUrl = 'x'\n$version = '2026.09.28-64d2043'\nfunction"),
            Some("2026.09.28-64d2043".into())
        );
        assert_eq!(
            script_version("$binaryName = \"droid.exe\"\n$version = \"0.230.0\"\n"),
            Some("0.230.0".into())
        );
        assert_eq!(script_version("$Version = $manifest.version"), None);
    }

    #[test]
    fn a_cursor_build_is_compared_by_its_date() {
        assert_eq!(
            cursor_build("2026.09.28-64d2043"),
            Some("2026.09.28".into())
        );
        assert_eq!(
            cursor_build("cursor-agent 2026.10.02-a1b2c3d"),
            Some("2026.10.02".into())
        );
        assert_eq!(cursor_build("1.2.3"), None);
        assert!(crate::update::ver_lt("2026.09.28", "2026.10.02"));
    }

    #[test]
    fn script_installed_clis_read_their_version_where_the_script_does() {
        assert_eq!(
            cli_source(&spec("cursor"), Some("native")),
            CliSource::Script(CURSOR_CLI_SCRIPT)
        );
        assert_eq!(
            cli_source(&spec("factory"), Some("native")),
            CliSource::Script(FACTORY_CLI_SCRIPT)
        );
        assert_eq!(
            cli_source(&spec("kiro"), Some("msi")),
            CliSource::KiroManifest
        );
        assert_eq!(
            cli_source(&spec("minimax-cn"), Some("native")),
            CliSource::Npm("@minimax-ai/code")
        );
    }

    #[test]
    fn script_installs_are_told_apart_by_where_they_live() {
        let method = |id: &str, path: &str| detect_install_method(&spec(id), Some(Path::new(path)));
        // Qoder's dispatcher on PATH is not where its CLI is installed.
        assert!(is_qoder_dispatcher(Path::new(
            r"C:\Users\me\.qoder\entry\qoder.cmd"
        )));
        assert!(is_qoder_dispatcher(Path::new(
            r"C:\Users\me\.qoder-cn\entry\qodercn.cmd"
        )));
        assert!(!is_qoder_dispatcher(Path::new(r"C:\nodejs\qoder.cmd")));
        // Kimi's official installer puts it in %USERPROFILE%\.kimi-code\bin.
        assert_eq!(
            method("kimi", r"C:\Users\me\.kimi-code\bin\kimi.exe").as_deref(),
            Some("native")
        );
        assert_eq!(
            method(
                "cursor",
                r"C:\Users\me\AppData\Local\cursor-agent\cursor-agent.cmd"
            )
            .as_deref(),
            Some("native")
        );
        assert_eq!(
            method("factory", r"C:\Users\me\bin\droid.exe").as_deref(),
            Some("native")
        );
        assert_eq!(
            method("kiro", r"C:\Program Files\Kiro-Cli\kiro-cli.exe").as_deref(),
            Some("msi")
        );
        assert_eq!(
            method("minimax-global", r"C:\Users\me\.minimax-code\mcode.cmd").as_deref(),
            Some("native")
        );
    }

    #[test]
    #[ignore = "reads the vendors' live release sources"]
    fn script_installed_clis_have_a_live_latest_version() {
        println!("cursor {:?}", script_latest(CURSOR_CLI_SCRIPT));
        println!("factory {:?}", script_latest(FACTORY_CLI_SCRIPT));
        println!("kiro {:?}", kiro_latest());
    }

    #[test]
    fn hermes_version_line_is_trimmed_to_its_release() {
        let line = "Hermes Agent v0.18.2 (2026.7.7.2) · upstream 524041b9 · local 569b912d (+1 carried commit)";
        assert_eq!(first_semver(line), Some("0.18.2".into()));
        assert_eq!(first_semver("no version here 1.2"), None);
    }

    fn spec(id: &str) -> ToolSpec {
        crate::agents::registry::spec_by_id(id).unwrap()
    }

    #[test]
    fn products_that_only_update_themselves_have_no_version_source() {
        // For these the card must say "no public source", not echo the installed version.
        for (id, method) in [("antigravity", None), ("claude", Some("native"))] {
            assert_eq!(cli_source(&spec(id), method), CliSource::None, "{id}");
        }
    }

    #[test]
    fn every_desktop_app_has_a_version_source() {
        for spec in crate::agents::registry::tool_specs() {
            if spec.desktop_available {
                assert_ne!(desktop_source(&spec), DesktopSource::None, "{}", spec.id);
            }
        }
    }

    #[test]
    fn products_with_a_published_version_name_where_it_is() {
        assert_eq!(desktop_source(&spec("kimi")), DesktopSource::KimiDownload);
        assert_eq!(desktop_source(&spec("zcode")), DesktopSource::ZcodeManifest);
        assert_eq!(
            desktop_source(&spec("mimo-cn")),
            DesktopSource::InstallerVersion(MIMO_DESKTOP_CN_INSTALLER)
        );
        assert_eq!(
            desktop_source(&spec("mimo-global")),
            DesktopSource::InstallerVersion(MIMO_DESKTOP_GLOBAL_INSTALLER)
        );
        assert_eq!(desktop_source(&spec("hermes")), DesktopSource::HermesRepo);
        assert_eq!(cli_source(&spec("hermes"), None), CliSource::HermesRepo);
        assert_eq!(
            desktop_source(&spec("workbuddy-cn")),
            DesktopSource::WorkBuddyUpdate("https://copilot.tencent.com")
        );
        assert_eq!(
            desktop_source(&spec("workbuddy-global")),
            DesktopSource::WorkBuddyUpdate("https://www.workbuddy.ai")
        );
        assert_eq!(
            desktop_source(&spec("qoder-cn")),
            DesktopSource::QoderUpdate("https://gateway.qoder.com.cn")
        );
        assert_eq!(
            desktop_source(&spec("qoder")),
            DesktopSource::QoderUpdate("https://center.qoder.sh")
        );
        assert_eq!(
            desktop_source(&spec("trae-work")),
            DesktopSource::TraeCheckUpdate(&crate::agents::feeds::TRAE_CN)
        );
        assert_eq!(
            desktop_source(&spec("trae-global")),
            DesktopSource::TraeCheckUpdate(&crate::agents::feeds::TRAE_GLOBAL)
        );
        assert_eq!(
            desktop_source(&spec("openclaw")),
            DesktopSource::GithubLatest("openclaw/openclaw-windows-node")
        );
        assert!(matches!(
            desktop_source(&spec("pi")),
            DesktopSource::ElectronFeed(_)
        ));
        // Agnes publishes the same kind of feed from its own CDN, so the source is named
        // for the vendor rather than for GitHub.
        assert_eq!(
            desktop_source(&spec("agnes-cn")),
            DesktopSource::ElectronFeed(crate::agents::registry::AGNES_CN_RELEASE_BASE)
        );
        assert_eq!(
            electron_feed_label(crate::agents::registry::AGNES_CN_RELEASE_BASE),
            "官方发布清单"
        );
        assert_eq!(
            desktop_source(&spec("codex")),
            DesktopSource::Winget("9PLM9XGG6VKS", Some("msstore"))
        );
        // TRAE's CLI is enterprise-only, so the catalogue offers no CLI to install or check.
        assert_eq!(spec("trae-work").cli_id, None);
        assert!(matches!(
            cli_source(&spec("codex"), Some("npm")),
            CliSource::Npm(_)
        ));
    }

    #[test]
    fn a_winget_listing_without_a_version_is_a_failed_lookup() {
        assert_eq!(
            usable_winget_version(Some("1.2.3".into())),
            Ok("1.2.3".into())
        );
        assert!(usable_winget_version(Some("Unknown".into())).is_err());
        assert!(usable_winget_version(None).is_err());
    }

    #[test]
    fn a_lookup_splits_into_version_source_and_error() {
        assert_eq!(
            split_latest(Ok(Some(("2.0".into(), "npm")))),
            (Some("2.0".into()), Some("npm".into()), None)
        );
        assert_eq!(split_latest(Ok(None)), (None, None, None));
        assert_eq!(
            split_latest(Err("offline".into())),
            (None, None, Some("offline".into()))
        );
    }

    #[test]
    fn npm_latest_urls_prefer_the_configured_registry() {
        assert_eq!(
            npm_latest_urls(
                "@moonshot-ai/kimi-code",
                Some("https://mirrors.cloud.tencent.com/npm/")
            ),
            vec![
                "https://mirrors.cloud.tencent.com/npm/@moonshot-ai/kimi-code/latest".to_string(),
                "https://registry.npmjs.org/@moonshot-ai/kimi-code/latest".to_string(),
            ]
        );
        assert_eq!(
            npm_latest_urls("pi", Some("https://registry.npmjs.org")),
            vec!["https://registry.npmjs.org/pi/latest".to_string()]
        );
        assert_eq!(
            npm_latest_urls("pi", None),
            vec!["https://registry.npmjs.org/pi/latest".to_string()]
        );
    }
    use std::path::Path;

    #[test]
    fn desktop_launcher_rejects_uninstaller_and_icon_files() {
        let keywords = &["zcode"];
        assert!(is_launchable_desktop_exe(
            Path::new(r"D:\AITools\ZCode\ZCode.exe"),
            keywords,
            &[],
        ));
        assert!(!is_launchable_desktop_exe(
            Path::new(r"D:\AITools\ZCode\Uninstall ZCode.exe"),
            keywords,
            &[],
        ));
        assert!(!is_launchable_desktop_exe(
            Path::new(r"D:\AITools\ZCode\uninstallerIcon.ico"),
            keywords,
            &[],
        ));
    }

    #[test]
    fn trae_desktop_editions_match_their_actual_windows_registration_names() {
        let cn = spec_by_id("trae-work").expect("TRAE CN catalog entry");
        assert!(desktop_name_matches(
            "TraeWork CN (User)",
            cn.desktop.keywords,
            cn.desktop.excludes,
        ));
        assert!(desktop_name_matches(
            "TRAE SOLO CN",
            cn.desktop.keywords,
            cn.desktop.excludes,
        ));
        assert!(!desktop_name_matches(
            "TraeWork (User)",
            cn.desktop.keywords,
            cn.desktop.excludes,
        ));

        let global = spec_by_id("trae-global").expect("TRAE global catalog entry");
        assert!(desktop_name_matches(
            "TraeWork (User)",
            global.desktop.keywords,
            global.desktop.excludes,
        ));
        assert!(!desktop_name_matches(
            "TraeWork CN (User)",
            global.desktop.keywords,
            global.desktop.excludes,
        ));
    }

    #[test]
    fn winget_show_version_parser_supports_english_and_chinese_output() {
        assert_eq!(
            winget_latest_version_from_show(
                "Found Claude Code [Anthropic.ClaudeCode]\nVersion: 2.1.248\nPublisher: Anthropic"
            ),
            Some("2.1.248".into())
        );
        assert_eq!(
            winget_latest_version_from_show(
                &[
                    "\u{5df2}\u{627e}\u{5230} Claude Code",
                    "\u{7248}\u{672c}\u{ff1a}2.1.248",
                    "\u{53d1}\u{5e03}\u{8005}\u{ff1a}Anthropic",
                ]
                .join("\n")
            ),
            Some("2.1.248".into())
        );
    }

    #[test]
    fn desktop_file_version_removes_only_windows_build_zeroes() {
        assert_eq!(normalize_desktop_version("3.2.3.0"), Some("3.2.3".into()));
        assert_eq!(
            normalize_desktop_version("v26.707.3351.0"),
            Some("26.707.3351".into())
        );
        assert_eq!(
            normalize_desktop_version("Kimi 3.2.3"),
            Some("3.2.3".into())
        );
    }
}
