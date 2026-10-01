//! 复制保密值：设置排除格式，不进 Win+V 历史与云剪贴板；30 秒后、锁定时、退出时，
//! 仅当剪贴板仍是这次复制的内容才清空。

use super::errors::IO;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub(crate) const CLEAR_AFTER: Duration = Duration::from_secs(30);
static LAST_SEQUENCE: AtomicU32 = AtomicU32::new(0);

pub(crate) fn copy_secret(text: &str) -> Result<(), String> {
    let sequence = platform::write(text)?;
    LAST_SEQUENCE.store(sequence, Ordering::SeqCst);
    std::thread::spawn(move || {
        std::thread::sleep(CLEAR_AFTER);
        clear_if_ours(sequence);
    });
    Ok(())
}

pub(crate) fn clear_now() {
    let sequence = LAST_SEQUENCE.load(Ordering::SeqCst);
    if sequence != 0 {
        clear_if_ours(sequence);
    }
}

fn clear_if_ours(sequence: u32) {
    if platform::sequence() == sequence {
        platform::clear();
    }
    let _ = LAST_SEQUENCE.compare_exchange(sequence, 0, Ordering::SeqCst, Ordering::SeqCst);
}

#[cfg(windows)]
mod platform {
    use super::IO;
    use std::time::Duration;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW,
        SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use zeroize::Zeroize;

    const CF_UNICODETEXT: u32 = 13;
    /// Formats Windows reads to keep content out of clipboard history and cloud sync.
    const EXCLUSIONS: [&str; 3] = [
        "ExcludeClipboardContentFromMonitorProcessing",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
    ];

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open() -> bool {
        for _ in 0..10 {
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Hands a copy of `bytes` to the clipboard, which owns the memory afterwards.
    unsafe fn put(format: u32, bytes: &[u8]) -> bool {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            return false;
        }
        let target = GlobalLock(handle) as *mut u8;
        if target.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        GlobalUnlock(handle);
        !SetClipboardData(format, handle as _).is_null()
    }

    pub(super) fn write(text: &str) -> Result<u32, String> {
        if !open() {
            return Err(IO.into());
        }
        let mut units = wide(text);
        let mut bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        let ok = unsafe {
            EmptyClipboard();
            let ok = put(CF_UNICODETEXT, &bytes);
            for name in EXCLUSIONS {
                let format = RegisterClipboardFormatW(wide(name).as_ptr());
                if format != 0 {
                    put(format, &0u32.to_le_bytes());
                }
            }
            CloseClipboard();
            ok
        };
        units.zeroize();
        bytes.zeroize();
        if ok {
            Ok(unsafe { GetClipboardSequenceNumber() })
        } else {
            Err(IO.into())
        }
    }

    pub(super) fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    pub(super) fn clear() {
        if open() {
            unsafe {
                EmptyClipboard();
                CloseClipboard();
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub(super) fn write(_: &str) -> Result<u32, String> {
        Err(super::IO.into())
    }
    pub(super) fn sequence() -> u32 {
        0
    }
    pub(super) fn clear() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "touches the real clipboard; run by hand with --ignored"]
    fn copied_text_is_cleared_only_while_unchanged() {
        copy_secret("vault clipboard check").unwrap();
        let ours = LAST_SEQUENCE.load(Ordering::SeqCst);
        assert_eq!(platform::sequence(), ours);
        clear_now();
        assert_ne!(platform::sequence(), ours);
    }
}
