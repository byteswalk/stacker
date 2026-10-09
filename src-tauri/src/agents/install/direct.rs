use crate::agents::{install::*, process::*, registry::*, *};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub(crate) fn install_desktop_from_official_package(
    spec: &ToolSpec,
    installer: DirectDesktopInstaller,
    window: &Option<tauri::Window>,
) -> Result<String, String> {
    emit_progress(
        window,
        format!("正在连接 {} 官方下载地址…", spec.desktop.name),
    );
    let resolved = resolve_installer(installer)?;
    let path = download_desktop_installer(spec, &resolved, window)?;
    let result = (|| {
        if installer.signed {
            emit_progress(window, "正在验证安装程序数字签名…");
            let signer = verify_desktop_installer_signature(&path)?;
            emit_progress(window, format!("数字签名有效 · {signer}"));
        } else {
            let expected = resolved
                .sha512
                .as_deref()
                .ok_or("安装包没有数字签名，且发布页未提供校验值，已停止安装。")?;
            if sha512_base64(&path)? != expected {
                return Err("安装包与官方发布的 SHA-512 校验值不一致，已停止安装。".into());
            }
            emit_progress(
                window,
                "安装包未签名，已按官方发布的 SHA-512 校验通过（可确认文件完整，但无法确认发布者身份）",
            );
        }
        run_downloaded_desktop_installer(spec, installer, &resolved, &path, window)?;

        emit_progress(window, format!("正在确认 {} 安装状态…", spec.desktop.name));
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            if desktop_installed_after_action(spec) {
                return Ok(format!("{} 已安装", spec.desktop.name));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!(
            "{} 安装程序已结束，但尚未检测到桌面应用。请检查安装程序提示后重试。",
            spec.desktop.name
        ))
    })();
    let _ = std::fs::remove_file(&path);
    result
}

/// Download location and optional published checksum of an installer.
pub(crate) struct ResolvedInstaller {
    pub(crate) url: String,
    pub(crate) file_name: String,
    pub(crate) sha512: Option<String>,
    /// Arguments for this particular package, when they differ from the product's usual
    /// ones (Qoder's per-machine installer takes /allusers).
    pub(crate) silent_args: Option<Vec<String>>,
}

pub(crate) struct ElectronRelease {
    pub(crate) version: String,
    pub(crate) file_name: String,
    pub(crate) sha512: String,
}

/// Reads the top-level `version`, `path` and `sha512` of an electron-builder `latest.yml`.
pub(crate) fn parse_latest_yml(text: &str) -> Option<ElectronRelease> {
    let top = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
            .map(|value| value.trim().trim_matches(['\'', '"']).to_string())
            .filter(|value| !value.is_empty())
    };
    Some(ElectronRelease {
        version: top("version")?,
        file_name: top("path")?,
        sha512: top("sha512")?,
    })
}

/// Reads a vendor's release manifest. Connections to these hosts get dropped mid-handshake
/// often enough (ZCode's did) that a single attempt reports a working feed as broken.
pub(crate) fn read_release_info(url: &str) -> Result<String, String> {
    let agent = desktop_download_agent(crate::agents::net::stacker_proxy().as_deref())?;
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(600 * attempt));
        }
        match agent.get(url).set("User-Agent", "Stacker").call() {
            Ok(response) => {
                return response
                    .into_string()
                    .map_err(|e| format!("读取发布信息失败：{e}"))
            }
            // A status reply is an answer, not a connection problem: do not retry it.
            Err(ureq::Error::Status(status, _)) => {
                return Err(format!("读取发布信息失败：服务返回 {status}"))
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!(
        "读取发布信息失败：连接中断（已重试 3 次）。多半是网络问题，挂上代理或稍后重试通常就好。原因：{last}"
    ))
}

pub(crate) fn electron_release_latest(base_url: &str) -> Result<ElectronRelease, String> {
    let text = read_release_info(&format!("{base_url}/latest.yml"))?;
    parse_latest_yml(&text).ok_or_else(|| "发布信息格式无法识别".into())
}

pub(crate) fn resolve_installer(
    installer: DirectDesktopInstaller,
) -> Result<ResolvedInstaller, String> {
    match installer.source {
        InstallerSource::Fixed { url, file_name } => Ok(ResolvedInstaller {
            url: url.into(),
            file_name: file_name.into(),
            sha512: None,
            silent_args: None,
        }),
        InstallerSource::Service { resolve } => resolve(),
        InstallerSource::ElectronManifest { manifest_url } => {
            let text = read_release_info(manifest_url)?;
            let release = parse_latest_yml(&text).ok_or("发布信息格式无法识别")?;
            let url = release.file_name.clone();
            if !url.starts_with("https://") {
                return Err("发布信息里的安装包地址不是 HTTPS 链接".into());
            }
            let file_name = url
                .rsplit('/')
                .next()
                .filter(|name| name.to_ascii_lowercase().ends_with(".exe"))
                .ok_or("发布信息里的安装包地址不是 exe 文件")?
                .to_string();
            Ok(ResolvedInstaller {
                url,
                file_name,
                sha512: Some(release.sha512),
                silent_args: None,
            })
        }
        InstallerSource::ElectronRelease { base_url } => {
            let release = electron_release_latest(base_url)?;
            Ok(ResolvedInstaller {
                // `MiniMax Code Setup 3.1.1.exe`: a name with spaces is one path segment.
                url: format!("{base_url}/{}", release.file_name.replace(' ', "%20")),
                file_name: release.file_name,
                sha512: Some(release.sha512),
                silent_args: None,
            })
        }
    }
}

/// Base64 SHA-512, the form electron-builder publishes in `latest.yml`.
pub(crate) fn sha512_base64(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha512};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha512::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digest = hasher.finalize();
    let mut out = String::new();
    for chunk in digest.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(ALPHABET[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    Ok(out)
}

pub(crate) fn download_desktop_installer(
    spec: &ToolSpec,
    installer: &ResolvedInstaller,
    window: &Option<tauri::Window>,
) -> Result<PathBuf, String> {
    // A stable name, so a download interrupted by a cancel or a closed Stacker leaves a
    // .part file the next run can continue from.
    let target = std::env::temp_dir().join(format!("stacker-{}-{}", spec.id, installer.file_name));
    // An installer window left open by an earlier run (one that had to ask the user) holds
    // the file: the new download could not replace it ("拒绝访问").
    if target
        .file_name()
        .is_some_and(|name| image_is_running(&name.to_string_lossy()))
    {
        return Err(format!(
            "{} 的安装程序已经在运行，请在已打开的安装窗口中完成安装（或关闭它）后再试",
            spec.desktop.name
        ));
    }
    drop_timestamped_leftovers(spec.id);
    let proxy = crate::agents::net::stacker_proxy();
    if let Some(proxy) = &proxy {
        emit_progress(
            window,
            format!("正在通过 Stacker 代理 {proxy} 连接官方下载地址…"),
        );
    }
    let agent = desktop_download_agent(proxy.as_deref())?;
    // A CDN that drops the connection mid-file (static.qoder.com.cn did, once in two tries)
    // gets one more attempt before the install fails.
    let mut attempt = 1;
    loop {
        match crate::installer::download_file_candidates_with_agent(
            &agent,
            std::slice::from_ref(&installer.url),
            &target,
            1_048_576,
            |message| emit_progress(window, message),
        ) {
            Ok(_) => return Ok(target),
            Err(err)
                if attempt == 1 && !crate::installer::op_cancelled() && !err.contains("取消") =>
            {
                emit_progress(window, format!("下载中断（{err}），正在重试…"));
                let _ = std::fs::remove_file(&target);
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

/// Earlier releases wrote `stacker-<id>-<timestamp>-<file>` installers that a later run could
/// never continue, so a cancelled download left hundreds of MB behind for good. They are
/// Stacker's own files, and nothing can resume them: drop them.
fn drop_timestamped_leftovers(id: &str) {
    let prefix = format!("stacker-{id}-");
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let timestamped = rest.split_once('-').is_some_and(|(stamp, _)| {
            stamp.len() >= 10 && stamp.chars().all(|c| c.is_ascii_digit())
        });
        if timestamped && entry.path().is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub(crate) fn desktop_download_agent(proxy: Option<&str>) -> Result<ureq::Agent, String> {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30));
    if let Some(address) = proxy {
        let proxy =
            ureq::Proxy::new(address).map_err(|e| format!("代理地址无效（{address}）：{e}"))?;
        builder = builder.proxy(proxy);
    }
    Ok(builder.build())
}

pub(crate) fn verify_desktop_installer_signature(path: &Path) -> Result<String, String> {
    let path = ps_single_quoted(&path.to_string_lossy());
    let script = format!(
        "$s=Get-AuthenticodeSignature -LiteralPath {path}; if ($s.Status -ne 'Valid') {{ throw ('数字签名状态：' + $s.Status) }}; $s.SignerCertificate.Subject"
    );
    let output = run_powershell(
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        "安装程序签名验证",
        Duration::from_secs(30),
    )?;
    first_output_line(&output).ok_or_else(|| "安装程序没有有效的发布者签名。".into())
}

/// Quotes one installer argument for the single command line ShellExecute takes.
pub(crate) fn command_line(args: &[&str]) -> String {
    args.iter()
        .map(|arg| {
            if arg.contains(' ') && !arg.starts_with('"') {
                format!("\"{arg}\"")
            } else {
                (*arg).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// An installer whose manifest asks for administrator rights cannot be started with
/// `CreateProcess`: Windows refuses with error 740. Only the shell can raise the UAC prompt,
/// so the run is repeated through it and waited on the same way.
#[cfg(windows)]
fn run_installer_elevated(
    spec: &ToolSpec,
    path: &Path,
    args: &[&str],
    log_path: &Path,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetExitCodeProcess, TerminateProcess};
    use winapi::um::shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winuser::{SW_HIDE, SW_SHOWNORMAL};

    emit_progress(
        window,
        format!(
            "{} 的安装程序需要管理员权限：请在弹出的 UAC 窗口中点「是」",
            spec.desktop.name
        ),
    );
    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let parameters: Vec<u16> = command_line(args)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let name = &spec.desktop.name;
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        // A silent installer stays hidden; one without silent switches has to be visible.
        info.nShow = if args.is_empty() {
            SW_SHOWNORMAL
        } else {
            SW_HIDE
        };
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(1223) {
                Err(format!("已取消：更新 {name} 需要管理员授权"))
            } else {
                Err(format!("启动 {name} 安装程序失败：{error}"))
            };
        }
        let started = Instant::now();
        let mut last_reported = u64::MAX;
        loop {
            if crate::installer::op_cancelled() {
                let _ = TerminateProcess(info.hProcess, 1);
                let _ = WaitForSingleObject(info.hProcess, 5_000);
                CloseHandle(info.hProcess);
                return Err(format!("已取消安装 {name}"));
            }
            match WaitForSingleObject(info.hProcess, 250) {
                // WAIT_OBJECT_0: it finished.
                0 => {
                    let mut code: u32 = 0;
                    let read = GetExitCodeProcess(info.hProcess, &mut code);
                    CloseHandle(info.hProcess);
                    return if read != 0 && code != 0 {
                        Err(installer_failure_logged(spec, code as i32, log_path, path))
                    } else {
                        Ok(())
                    };
                }
                // WAIT_TIMEOUT: still running.
                258 => {
                    let elapsed = started.elapsed().as_secs();
                    if elapsed >= 1200 {
                        let _ = TerminateProcess(info.hProcess, 1);
                        let _ = WaitForSingleObject(info.hProcess, 5_000);
                        CloseHandle(info.hProcess);
                        return Err(format!("{name} 安装超过 20 分钟，已停止操作。"));
                    }
                    if elapsed != last_reported {
                        last_reported = elapsed;
                        emit_progress(window, format!("正在安装 {name} · 已 {elapsed} 秒"));
                    }
                }
                _ => {
                    CloseHandle(info.hProcess);
                    return Err(format!("等待 {name} 安装程序时发生系统错误"));
                }
            }
        }
    }
}

/// What a non-zero installer exit code most likely means, plus what its own log said.
pub(crate) fn installer_failure_logged(
    spec: &ToolSpec,
    code: i32,
    log: &Path,
    installer: &Path,
) -> String {
    let detail = std::fs::read_to_string(log)
        .ok()
        .as_deref()
        .and_then(inno_failure_detail);
    match detail {
        // 360 blocks by where the program runs from, so naming the installer's own path is
        // what lets the user allow it: whitelisting the target folder changes nothing.
        Some(detail) => format!(
            "{} 安装未完成：{detail}。多半是 360 等安全软件拦截了安装程序，或该程序还在运行；请退出它，或把安装程序 {} 加入安全软件的信任区后重试",
            spec.desktop.name,
            installer.display()
        ),
        None => installer_failure(spec, code),
    }
}

/// What a non-zero installer exit code most likely means.
pub(crate) fn installer_failure(spec: &ToolSpec, code: i32) -> String {
    let hint = match code {
        // Inno Setup 2 and 5, MSI 1602: the installer was closed, or the UAC prompt refused.
        2 | 5 | 1602 => "看起来是安装程序被取消了（UAC 窗口点了「否」或关掉了安装窗口）",
        // MSI: the installation itself failed, usually because a file could not be written.
        1603 => "安装过程出错，多半是文件写不进去",
        740 => "需要管理员权限",
        _ => "",
    };
    let hint = if hint.is_empty() {
        String::new()
    } else {
        format!("{hint}。")
    };
    format!(
        "{} 安装未完成，安装程序退出代码：{code}。{hint}如果装了 360 等安全软件，请检查它是否拦截了安装程序写入的文件",
        spec.desktop.name
    )
}

/// Inno Setup writes a detailed log when asked, and it is the only place that says which file
/// could not be replaced. Its silent mode otherwise answers the error box with "abort" and
/// exits with code 5, which on its own says nothing.
pub(crate) fn inno_log_argument(args: &[&str], log: &Path) -> Option<String> {
    let inno = args.iter().any(|a| {
        let a = a.to_ascii_uppercase();
        a == "/VERYSILENT" || a == "/SILENT"
    });
    inno.then(|| format!("/LOG={}", log.display()))
}

/// The line in an Inno log that says what actually went wrong.
pub(crate) fn inno_failure_detail(log: &str) -> Option<String> {
    // Every line starts with a timestamp, so each marker is looked for inside the line.
    let blocked = log
        .lines()
        .rev()
        .find_map(|line| line.split_once("Dest filename: "))
        .map(|(_, file)| file.trim().to_string());
    let reason = log
        .lines()
        .rev()
        .find(|line| line.contains("MoveFile failed") || line.contains("appears to be in use"))
        .map(|line| {
            line.rsplit("   ")
                .next()
                .unwrap_or(line)
                .trim()
                .trim_end_matches('.')
                .to_string()
        })?;
    let file = blocked.unwrap_or_default();
    Some(if file.is_empty() {
        reason.to_string()
    } else {
        format!("无法替换 {file}（{reason}）")
    })
}

pub(crate) fn run_downloaded_desktop_installer(
    spec: &ToolSpec,
    installer: DirectDesktopInstaller,
    resolved: &ResolvedInstaller,
    path: &Path,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let mut args: Vec<&str> = match &resolved.silent_args {
        Some(own) => own.iter().map(String::as_str).collect(),
        None => installer.silent_args.to_vec(),
    };
    let log_path = std::env::temp_dir().join(format!("stacker-{}-setup.log", spec.id));
    let _ = std::fs::remove_file(&log_path);
    let log_argument = inno_log_argument(&args, &log_path);
    if let Some(argument) = log_argument.as_deref() {
        args.push(argument);
    }
    if args.is_empty() {
        emit_progress(
            window,
            format!(
                "{0} 安装程序没有静默模式，请在弹出的 {0} 窗口中点击安装并等待完成…",
                spec.desktop.name
            ),
        );
    }
    match run_installer_once(spec, path, &args, &log_path, window)? {
        InstallerExit::Done => Ok(()),
        // Qoder's installer (from 0.4.3) quits at once with code 22 under `/S` and installs
        // fine from its window: a silent run refused outright is handed to the user.
        InstallerExit::Failed { code, elapsed }
            if !args.is_empty() && elapsed < Duration::from_secs(20) && !cancel_code(code) =>
        {
            emit_progress(
                window,
                format!(
                    "{0} 安装程序拒绝静默安装（退出代码 {code}），已改为打开安装窗口，请在 {0} 窗口中完成安装…",
                    spec.desktop.name
                ),
            );
            match run_installer_once(spec, path, &[], &log_path, window)? {
                InstallerExit::Done => Ok(()),
                InstallerExit::Failed { code, .. } => Err(installer_failure(spec, code)),
            }
        }
        InstallerExit::Failed { code, .. } => {
            Err(installer_failure_logged(spec, code, &log_path, path))
        }
    }
}

/// How an installer run ended, when it ran at all.
enum InstallerExit {
    Done,
    Failed { code: i32, elapsed: Duration },
}

/// Exit codes that mean the user (or the UAC prompt) said no: never asked again.
fn cancel_code(code: i32) -> bool {
    matches!(code, 2 | 5 | 1602)
}

fn run_installer_once(
    spec: &ToolSpec,
    path: &Path,
    args: &[&str],
    log_path: &Path,
    window: &Option<tauri::Window>,
) -> Result<InstallerExit, String> {
    let mut command = Command::new(path);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        // ERROR_ELEVATION_REQUIRED: the installer is marked as needing administrator rights,
        // which only the shell can ask for. Qoder's system-wide updater is one of these.
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(740) => {
            return run_installer_elevated(spec, path, args, log_path, window)
                .map(|()| InstallerExit::Done)
        }
        Err(e) => return Err(format!("启动 {} 安装程序失败：{e}", spec.desktop.name)),
    };
    let job = ProcessJob::attach(&child);
    let started = Instant::now();
    let mut last_reported = u64::MAX;
    // An installer run with its window is the user's to watch and close. Hermes-Setup clones
    // its repository, installs Python and builds the desktop app in there: past 20 minutes
    // on a slow line, and stopping it then leaves a half-made install.
    let limit_minutes = if args.is_empty() { 60 } else { 20 };
    loop {
        if crate::installer::op_cancelled() {
            stop_command(&mut child, job.as_ref());
            let _ = child.wait();
            return Err(format!("已取消安装 {}", spec.desktop.name));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(InstallerExit::Done),
            Ok(Some(status)) => {
                return Ok(InstallerExit::Failed {
                    code: status.code().unwrap_or(-1),
                    elapsed: started.elapsed(),
                })
            }
            Ok(None) => {
                let elapsed = started.elapsed().as_secs();
                if elapsed >= limit_minutes * 60 {
                    stop_command(&mut child, job.as_ref());
                    let _ = child.wait();
                    return Err(format!(
                        "{} 安装超过 {limit_minutes} 分钟，已停止操作。",
                        spec.desktop.name
                    ));
                }
                if elapsed != last_reported {
                    last_reported = elapsed;
                    emit_progress(
                        window,
                        format!("正在安装 {} · 已 {} 秒", spec.desktop.name, elapsed),
                    );
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => return Err(format!("读取 {} 安装状态失败：{e}", spec.desktop.name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inno_failure_names_the_file_it_could_not_replace() {
        let args = ["/VERYSILENT", "/SUPPRESSMSGBOXES"];
        assert_eq!(
            inno_log_argument(&args, Path::new(r"C:\t\s.log")).as_deref(),
            Some(r"/LOG=C:\t\s.log")
        );
        assert_eq!(inno_log_argument(&["/S"], Path::new(r"C:\t\s.log")), None);
        let log = concat!(
            "2026-09-26 10:14:25.109   Dest filename: D:\\AITools\\OpenClawTray\\OpenClaw.Tray.WinUI.exe\n",
            "   MoveFile: The existing file appears to be in use (5). Retrying.\n",
            "   MoveFile failed; code 5.\n",
            "   User canceled the installation process.\n",
        );
        let detail = inno_failure_detail(log).unwrap();
        assert!(detail.contains("OpenClaw.Tray.WinUI.exe"), "{detail}");
        assert!(detail.contains("MoveFile failed"), "{detail}");
        assert_eq!(inno_failure_detail("nothing to see here"), None);
    }

    #[test]
    fn an_elevated_installer_gets_one_command_line() {
        assert_eq!(command_line(&["/S", "/allusers"]), "/S /allusers");
        assert_eq!(
            command_line(&["/DIR=C:\\Program Files\\X"]),
            "\"/DIR=C:\\Program Files\\X\""
        );
        assert_eq!(command_line(&[]), "");
    }

    #[test]
    fn electron_latest_yml_names_installer_and_checksum() {
        let yml = "version: 0.15.0\nfiles:\n  - url: PI-Desktop-Setup-0.15.0.exe\n    sha512: AAA==\n    size: 1\npath: PI-Desktop-Setup-0.15.0.exe\nsha512: +IXh/q==\nreleaseDate: '2026-09-17T12:55:41.495Z'\n";
        let release = parse_latest_yml(yml).unwrap();
        assert_eq!(release.version, "0.15.0");
        assert_eq!(release.file_name, "PI-Desktop-Setup-0.15.0.exe");
        assert_eq!(release.sha512, "+IXh/q==");
        assert!(parse_latest_yml("files: []").is_none());
    }

    #[test]
    fn sha512_is_encoded_like_electron_builder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.bin");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha512_base64(&file).unwrap(),
            "3a81oZNherrMQXNJriBBMRLm+k6JqX6iCp7u5ktV05ohkpkqJ0/BqDa6PCOj/uu9RU1EI2Q86A4qmslPpUyknw=="
        );
    }
}
