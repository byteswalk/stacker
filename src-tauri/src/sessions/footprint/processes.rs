//! Image paths of running processes, used to keep versions in use and to block cleanup.
use std::path::PathBuf;

#[cfg(windows)]
pub fn running_images() -> Vec<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use winapi::shared::minwindef::{DWORD, FALSE};
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use winapi::um::winbase::QueryFullProcessImageNameW;
    use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;

    let mut images = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return images;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as DWORD;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let process = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION,
                FALSE,
                entry.th32ProcessID,
            );
            if !process.is_null() {
                let mut buffer = [0u16; 1024];
                let mut size = buffer.len() as DWORD;
                if QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) != 0 {
                    images.push(PathBuf::from(OsString::from_wide(&buffer[..size as usize])));
                }
                CloseHandle(process);
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    images
}

#[cfg(target_os = "macos")]
pub fn running_images() -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: the first call asks for the count, the second fills a buffer of that size;
    // proc_pidpath writes at most the buffer's length.
    unsafe {
        let count = libc::proc_listallpids(std::ptr::null_mut(), 0);
        if count <= 0 {
            return Vec::new();
        }
        let mut pids = vec![0 as libc::pid_t; count as usize + 64];
        let size = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        let found = libc::proc_listallpids(pids.as_mut_ptr().cast(), size);
        if found <= 0 {
            return Vec::new();
        }
        pids.truncate(found as usize);
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        pids.into_iter()
            .filter(|pid| *pid > 0)
            .filter_map(|pid| {
                let len = libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32);
                (len > 0)
                    .then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&buffer[..len as usize])))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn includes_the_current_process() {
        let me = std::env::current_exe().unwrap();
        let me = std::fs::canonicalize(me).unwrap();
        assert!(super::running_images()
            .iter()
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .any(|p| p == me));
    }
}
