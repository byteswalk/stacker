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
    let path = download_desktop_installer(spec, installer, window)?;
    let result = (|| {
        emit_progress(window, "正在验证安装程序数字签名…");
        let signer = verify_desktop_installer_signature(&path)?;
        emit_progress(window, format!("数字签名有效 · {signer}"));
        run_downloaded_desktop_installer(spec, installer, &path, window)?;

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

pub(crate) fn download_desktop_installer(
    spec: &ToolSpec,
    installer: DirectDesktopInstaller,
    window: &Option<tauri::Window>,
) -> Result<PathBuf, String> {
    let target = std::env::temp_dir().join(format!(
        "stacker-{}-{}-{}",
        spec.id,
        chrono::Local::now().timestamp_millis(),
        installer.file_name
    ));
    let proxy = crate::agents::net::stacker_proxy();
    if let Some(proxy) = &proxy {
        emit_progress(
            window,
            format!("正在通过 Stacker 代理 {proxy} 连接官方下载地址…"),
        );
    }
    let agent = desktop_download_agent(proxy.as_deref())?;
    crate::installer::download_file_candidates_with_agent(
        &agent,
        &[installer.url.to_string()],
        &target,
        1_048_576,
        |message| emit_progress(window, message),
    )?;
    Ok(target)
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

pub(crate) fn run_downloaded_desktop_installer(
    spec: &ToolSpec,
    installer: DirectDesktopInstaller,
    path: &Path,
    window: &Option<tauri::Window>,
) -> Result<(), String> {
    let mut command = Command::new(path);
    command.args(installer.silent_args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 {} 安装程序失败：{e}", spec.desktop.name))?;
    let started = Instant::now();
    let mut last_reported = u64::MAX;
    loop {
        if crate::installer::op_cancelled() {
            terminate_command_tree(&mut child);
            let _ = child.wait();
            return Err(format!("已取消安装 {}", spec.desktop.name));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "{} 安装未完成，安装程序退出代码：{}",
                    spec.desktop.name,
                    status.code().unwrap_or(-1)
                ))
            }
            Ok(None) => {
                let elapsed = started.elapsed().as_secs();
                if elapsed >= 1200 {
                    terminate_command_tree(&mut child);
                    let _ = child.wait();
                    return Err(format!(
                        "{} 安装超过 20 分钟，已停止操作。",
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
