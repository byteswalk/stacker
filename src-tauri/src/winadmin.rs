//! 系统级环境变量切换：提权（UAC）重启自身写 HKLM。

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize)]
struct SysReq {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    siblings: Vec<String>,
    #[serde(default)]
    vars: HashMap<String, String>,
    token: String,
}

fn request_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn req_file(token: &str) -> PathBuf {
    std::env::temp_dir()
        .join("stacker")
        .join(format!("syssetenv-{token}.json"))
}

/// 解析启动参数：是否是“提权写系统级”的内部调用。
pub fn syssetenv_arg() -> Option<(String, String)> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "__syssetenv" {
        Some((args[2].clone(), args[3].clone()))
    } else {
        None
    }
}

/// GUI 侧：写请求文件 → 提权重启自身执行 → 等待结果。
#[cfg(windows)]
pub fn set_default_system(kind: &str, path: &str, siblings: Vec<String>) -> Result<(), String> {
    let token = request_id();
    let req = SysReq {
        kind: kind.into(),
        path: path.into(),
        siblings,
        vars: HashMap::new(),
        token: token.clone(),
    };
    let file = req_file(&token);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&file, serde_json::to_vec(&req).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if run_elevated_self(&file.to_string_lossy(), &token)? {
        Ok(())
    } else {
        let _ = std::fs::remove_file(&file);
        Err("系统级切换未完成（UAC 取消或写入失败）".into())
    }
}

#[cfg(windows)]
pub fn set_env_system(label: &str, vars: Vec<(String, String)>) -> Result<(), String> {
    let token = request_id();
    let req = SysReq {
        kind: format!("__setenv:{label}"),
        path: String::new(),
        siblings: Vec::new(),
        vars: vars.into_iter().collect(),
        token: token.clone(),
    };
    let file = req_file(&token);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&file, serde_json::to_vec(&req).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if run_elevated_self(&file.to_string_lossy(), &token)? {
        Ok(())
    } else {
        let _ = std::fs::remove_file(&file);
        Err("系统级环境变量未完成写入（UAC 取消或写入失败）".into())
    }
}

/// 被提权实例执行：读请求文件 → 写 HKLM → 返回退出码。
#[cfg(windows)]
pub fn apply_from_file(file: &str, token: &str) -> i32 {
    let Ok(data) = std::fs::read(file) else {
        return 1;
    };
    let Ok(req) = serde_json::from_slice::<SysReq>(&data) else {
        return 1;
    };
    if req.token != token {
        return 3;
    }
    let r = if req.kind.starts_with("__setenv:") {
        let names: Vec<&str> = req.vars.keys().map(String::as_str).collect();
        crate::backup::backup_env(crate::winenv::Hive::System, &req.kind, &names);
        req.vars
            .iter()
            .try_for_each(|(k, v)| crate::winenv::set_in(crate::winenv::Hive::System, k, v))
    } else {
        crate::env::set_default(
            crate::winenv::Hive::System,
            &req.kind,
            &req.path,
            req.siblings,
        )
    };
    let _ = std::fs::remove_file(file);
    if r.is_ok() {
        0
    } else {
        2
    }
}

#[cfg(windows)]
fn run_elevated_self(file: &str, token: &str) -> Result<bool, String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::GetExitCodeProcess;
    use winapi::um::shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::INFINITE;
    use winapi::um::winuser::SW_HIDE;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_w: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb_w: Vec<u16> = "runas\0".encode_utf16().collect();
    let params = format!("__syssetenv \"{file}\" \"{token}\"");
    let params_w: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut sei: SHELLEXECUTEINFOW = std::mem::zeroed();
        sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = SEE_MASK_NOCLOSEPROCESS;
        sei.lpVerb = verb_w.as_ptr();
        sei.lpFile = exe_w.as_ptr();
        sei.lpParameters = params_w.as_ptr();
        sei.nShow = SW_HIDE;
        if ShellExecuteExW(&mut sei) == 0 || sei.hProcess.is_null() {
            return Ok(false); // 用户取消 UAC 或调用失败
        }
        WaitForSingleObject(sei.hProcess, INFINITE);
        let mut code: u32 = 1;
        GetExitCodeProcess(sei.hProcess, &mut code);
        CloseHandle(sei.hProcess);
        Ok(code == 0)
    }
}

#[cfg(not(windows))]
pub fn set_default_system(_: &str, _: &str, _: Vec<String>) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
#[cfg(not(windows))]
pub fn set_env_system(_: &str, _: Vec<(String, String)>) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
#[cfg(not(windows))]
pub fn apply_from_file(_: &str, _: &str) -> i32 {
    1
}
