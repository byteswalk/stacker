//! 复制保密值：设置排除格式，不进 Win+V 历史与云剪贴板；30 秒后、锁定时、退出时，
//! 仅当剪贴板仍是这次复制的内容才清空。

use super::errors::IO;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub(crate) const CLEAR_AFTER: Duration = Duration::from_secs(30);
const RETRY_AFTER: Duration = Duration::from_millis(500);
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

/// Forgets the recorded copy only once the clipboard no longer holds it; one retry if it could not be opened.
fn clear_if_ours(sequence: u32) {
    if !platform::clear(sequence) {
        std::thread::sleep(RETRY_AFTER);
        if !platform::clear(sequence) {
            return;
        }
    }
    let _ = LAST_SEQUENCE.compare_exchange(sequence, 0, Ordering::SeqCst, Ordering::SeqCst);
}

#[cfg(windows)]
mod platform {
    use super::IO;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
        RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use zeroize::Zeroize;

    const CF_UNICODETEXT: u32 = 13;
    /// Formats Windows reads to keep content out of clipboard history and cloud sync.
    const EXCLUSIONS: [&str; 3] = [
        "ExcludeClipboardContentFromMonitorProcessing",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
    ];

    /// UTF-16 units never outnumber UTF-8 bytes, so the buffer never reallocates (no stray copies of the text).
    fn wide(text: &str) -> Vec<u16> {
        let mut units = Vec::with_capacity(text.len() + 1);
        units.extend(text.encode_utf16());
        units.push(0);
        units
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
    /// On failure the block is wiped and freed so no plaintext is left behind.
    unsafe fn put(format: u32, bytes: &[u8]) -> bool {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            return false;
        }
        let target = GlobalLock(handle) as *mut u8;
        if target.is_null() {
            GlobalFree(handle);
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        GlobalUnlock(handle);
        if !SetClipboardData(format, handle as _).is_null() {
            return true;
        }
        let target = GlobalLock(handle) as *mut u8;
        if !target.is_null() {
            std::ptr::write_bytes(target, 0, bytes.len());
            GlobalUnlock(handle);
        }
        GlobalFree(handle);
        false
    }

    /// Puts the text and every exclusion format on the (already open) clipboard.
    unsafe fn put_all(bytes: &[u8]) -> bool {
        let mut ok = put(CF_UNICODETEXT, bytes);
        for name in EXCLUSIONS {
            let format = RegisterClipboardFormatW(wide(name).as_ptr());
            ok = ok && format != 0 && put(format, &0u32.to_le_bytes());
        }
        ok
    }

    pub(super) fn write(text: &str) -> Result<u32, String> {
        if !open() {
            return Err(IO.into());
        }
        let mut units = wide(text);
        let mut bytes: Vec<u8> = Vec::with_capacity(units.len() * 2);
        bytes.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
        let held = unsafe {
            EmptyClipboard();
            // Fail closed: a secret without every exclusion format could reach history or the cloud.
            let ok = put_all(&bytes) && GetClipboardSequenceNumber() != 0;
            if !ok {
                EmptyClipboard();
            }
            CloseClipboard();
            ok
        };
        units.zeroize();
        bytes.zeroize();
        if !held {
            return Err(IO.into());
        }
        // CloseClipboard itself advances the sequence number (measured: +3), so the number to remember
        // can only be read after closing. If another app copied in that gap it replaced our formats.
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence == 0 {
            if open() {
                unsafe {
                    EmptyClipboard();
                    CloseClipboard();
                }
            }
            return Err(IO.into());
        }
        if !all_exclusions_present() {
            // Only clears while the clipboard still holds our write; never touches a newer copy.
            clear(sequence);
            return Err(IO.into());
        }
        Ok(sequence)
    }

    fn all_exclusions_present() -> bool {
        use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable;
        EXCLUSIONS.iter().all(|name| unsafe {
            let format = RegisterClipboardFormatW(wide(name).as_ptr());
            format != 0 && IsClipboardFormatAvailable(format) != 0
        })
    }

    #[cfg(test)]
    pub(super) fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    /// Empties the clipboard only while it still holds `expected`, checked with the clipboard held open.
    /// Returns true once the content is no longer ours (cleared, or already replaced); false if it could not be opened.
    pub(super) fn clear(expected: u32) -> bool {
        if !open() {
            return false;
        }
        unsafe {
            if GetClipboardSequenceNumber() == expected {
                EmptyClipboard();
            }
            CloseClipboard();
        }
        true
    }

    /// Test helper: plain text with no exclusion formats, as an ordinary app would copy it.
    #[cfg(test)]
    pub(super) fn write_plain(text: &str) -> u32 {
        assert!(open());
        let bytes: Vec<u8> = wide(text)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        unsafe {
            EmptyClipboard();
            assert!(put(CF_UNICODETEXT, &bytes));
            CloseClipboard();
            GetClipboardSequenceNumber()
        }
    }

    #[cfg(test)]
    pub(super) fn has_text() -> bool {
        unsafe {
            windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable(CF_UNICODETEXT)
                != 0
        }
    }

    #[cfg(test)]
    pub(super) fn exclusions_present() -> bool {
        all_exclusions_present()
    }
}

#[cfg(not(windows))]
mod platform {
    pub(super) fn write(_: &str) -> Result<u32, String> {
        Err(super::IO.into())
    }
    #[cfg(test)]
    pub(super) fn sequence() -> u32 {
        0
    }
    pub(super) fn clear(_: u32) -> bool {
        true
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "touches the real clipboard; run by hand with --ignored"]
    fn copied_text_is_cleared_only_while_unchanged() {
        // Secret copied with every exclusion format, and cleared when still ours.
        copy_secret("vault clipboard check").unwrap();
        let ours = LAST_SEQUENCE.load(Ordering::SeqCst);
        assert_eq!(platform::sequence(), ours);
        assert!(platform::exclusions_present());
        clear_now();
        assert_ne!(platform::sequence(), ours);
        assert_eq!(LAST_SEQUENCE.load(Ordering::SeqCst), 0);

        // Something else copied in between must survive clear_now.
        copy_secret("vault clipboard check").unwrap();
        let other = platform::write_plain("someone else's text");
        assert_ne!(other, LAST_SEQUENCE.load(Ordering::SeqCst));
        clear_now();
        assert_eq!(platform::sequence(), other);
        assert!(platform::has_text());
        assert_eq!(LAST_SEQUENCE.load(Ordering::SeqCst), 0);

        // Leave the clipboard empty.
        assert!(platform::clear(other));
        assert!(!platform::has_text());
    }
}
