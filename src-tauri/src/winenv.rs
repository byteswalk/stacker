//! 环境变量读写。支持用户级（HKCU\Environment）与系统级（HKLM Session Manager\Environment）。
//! 写系统级需要管理员权限（由提权进程调用）。写操作广播 WM_SETTINGCHANGE。

#[derive(Clone, Copy, PartialEq)]
pub enum Hive {
    User,
    System,
}

#[cfg(windows)]
fn open(hive: Hive, access: u32) -> Result<winreg::RegKey, String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let (root, sub) = match hive {
        Hive::User => (HKEY_CURRENT_USER, "Environment"),
        Hive::System => (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    };
    RegKey::predef(root)
        .open_subkey_with_flags(sub, access)
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn get_raw_in(hive: Hive, name: &str) -> Option<String> {
    use winreg::enums::KEY_READ;
    let key = open(hive, KEY_READ).ok()?;
    let v: String = key.get_value(name).ok()?;
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

#[cfg(windows)]
pub fn set_in(hive: Hive, name: &str, value: &str) -> Result<(), String> {
    use winreg::enums::{KEY_READ, KEY_WRITE, REG_EXPAND_SZ};
    use winreg::RegValue;
    let key = open(hive, KEY_READ | KEY_WRITE)?;
    if value.contains('%') {
        let mut bytes: Vec<u8> = Vec::new();
        for u in value.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        key.set_raw_value(
            name,
            &RegValue {
                vtype: REG_EXPAND_SZ,
                bytes,
            },
        )
        .map_err(|e| e.to_string())?;
    } else {
        key.set_value(name, &value.to_string())
            .map_err(|e| e.to_string())?;
    }
    broadcast_change();
    Ok(())
}

#[cfg(windows)]
pub fn remove_in(hive: Hive, name: &str) -> Result<(), String> {
    use winreg::enums::{KEY_READ, KEY_WRITE};
    let key = open(hive, KEY_READ | KEY_WRITE)?;
    match key.delete_value(name) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    broadcast_change();
    Ok(())
}

#[cfg(windows)]
pub fn get_path_in(hive: Hive) -> Vec<String> {
    get_raw_in(hive, "Path")
        .unwrap_or_default()
        .split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(windows)]
pub fn set_path_in(hive: Hive, entries: &[String]) -> Result<(), String> {
    set_in(hive, "Path", &entries.join(";"))
}

#[cfg(windows)]
pub fn prepend_path_in(hive: Hive, dir: &str) -> Result<(), String> {
    let dir = dir.trim();
    if dir.is_empty() {
        return Ok(());
    }
    let mut entries: Vec<String> = get_path_in(hive)
        .into_iter()
        .filter(|e| !e.eq_ignore_ascii_case(dir))
        .collect();
    entries.insert(0, dir.to_string());
    set_path_in(hive, &entries)
}

#[cfg(windows)]
pub fn remove_path_in(hive: Hive, dir: &str) -> Result<(), String> {
    let dir = dir.trim();
    let before = get_path_in(hive);
    let after: Vec<String> = before
        .iter()
        .filter(|e| !e.eq_ignore_ascii_case(dir))
        .cloned()
        .collect();
    if after.len() == before.len() {
        return Ok(());
    }
    set_path_in(hive, &after)
}

#[cfg(windows)]
pub fn broadcast_change() {
    use winapi::shared::minwindef::{LPARAM, WPARAM};
    use winapi::um::winuser::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let env: Vec<u16> = "Environment\0".encode_utf16().collect();
    unsafe {
        let mut result: usize = 0;
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0 as WPARAM,
            env.as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
}

// ── 用户级（HKCU）便捷封装，供 sources/proxy 使用 ──
pub fn get_user_raw(name: &str) -> Option<String> {
    get_raw_in(Hive::User, name)
}
pub fn set_user(name: &str, value: &str) -> Result<(), String> {
    set_in(Hive::User, name, value)
}
pub fn remove_user(name: &str) -> Result<(), String> {
    remove_in(Hive::User, name)
}

// 非 Windows 占位
#[cfg(not(windows))]
pub fn get_raw_in(_: Hive, _: &str) -> Option<String> {
    None
}
#[cfg(not(windows))]
pub fn set_in(_: Hive, _: &str, _: &str) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
#[cfg(not(windows))]
pub fn remove_in(_: Hive, _: &str) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
#[cfg(not(windows))]
pub fn get_path_in(_: Hive) -> Vec<String> {
    Vec::new()
}
#[cfg(not(windows))]
pub fn set_path_in(_: Hive, _: &[String]) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
#[cfg(not(windows))]
pub fn prepend_path_in(_: Hive, _: &str) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn remove_path_in(_: Hive, _: &str) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn get_user_raw(_: &str) -> Option<String> {
    None
}
#[cfg(not(windows))]
pub fn set_user(_: &str, _: &str) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
#[cfg(not(windows))]
pub fn remove_user(_: &str) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
