//! Starting a command-line program inside a console of its own that is never shown.
//!
//! `CREATE_NO_WINDOW` gives a program no console at all. That is fine for the program, but
//! when it starts a console program in turn (agy runs itself twice over), that grandchild has
//! no console to share and Windows makes it a new one: a window that flashes up, in Windows
//! Terminal when that is the default terminal. A hidden console of its own is shared by
//! everything the program starts. Rust's `Command` cannot ask for a hidden window, so this
//! starts the process with `CreateProcessW` and hands back the pipes like `Child` does.

#[cfg(windows)]
pub(crate) use imp::*;

#[cfg(windows)]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::os::windows::process::ExitStatusExt;
    use std::process::{Command, ExitStatus};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT,
        EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES,
        STARTUPINFOEXW,
    };

    /// `SW_HIDE`.
    const HIDE: u16 = 0;

    /// A program started in a hidden console: its pipes, and the process to wait for or end.
    pub(crate) struct HiddenChild {
        process: usize,
        pub stdin: Option<File>,
        pub stdout: Option<File>,
        pub stderr: Option<File>,
        job: Option<crate::agents::process::ProcessJob>,
    }

    impl HiddenChild {
        pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            // SAFETY: `process` is a live process handle owned by this value.
            unsafe {
                if WaitForSingleObject(self.process as HANDLE, 0) != WAIT_OBJECT_0 {
                    return Ok(None);
                }
                let mut code = 0u32;
                if GetExitCodeProcess(self.process as HANDLE, &mut code) == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(ExitStatus::from_raw(code)))
            }
        }

        pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
            // SAFETY: as above.
            unsafe { WaitForSingleObject(self.process as HANDLE, INFINITE) };
            self.try_wait()?
                .ok_or_else(|| io::Error::other("the process did not end"))
        }

        /// Ends the program and everything it started.
        pub(crate) fn kill_tree(&mut self) {
            match &self.job {
                Some(job) => job.terminate(),
                // SAFETY: as above.
                None => unsafe {
                    TerminateProcess(self.process as HANDLE, 1);
                },
            }
        }
    }

    impl Drop for HiddenChild {
        fn drop(&mut self) {
            // SAFETY: closed once, here.
            unsafe { CloseHandle(self.process as HANDLE) };
        }
    }

    /// Windows command-line quoting for one argument, as the C runtime reads it back.
    pub(crate) fn quote(arg: &str) -> String {
        if !arg.is_empty() && !arg.chars().any(|c| c == ' ' || c == '\t' || c == '"') {
            return arg.to_string();
        }
        let mut out = String::from("\"");
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                _ => {
                    out.push_str(&"\\".repeat(backslashes));
                    out.push(c);
                    backslashes = 0;
                }
            }
        }
        out.push_str(&"\\".repeat(backslashes * 2));
        out.push('"');
        out
    }

    /// This process's environment with `cmd`'s changes, as one sorted block.
    fn environment(cmd: &Command) -> Vec<u16> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        for (key, value) in cmd.get_envs() {
            let upper = key.to_string_lossy().to_uppercase();
            vars.retain(|(k, _)| k.to_string_lossy().to_uppercase() != upper);
            if let Some(value) = value {
                vars.push((key.to_os_string(), value.to_os_string()));
            }
        }
        vars.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
        let mut block = Vec::new();
        for (key, value) in vars {
            block.extend(key.encode_wide());
            block.push(u16::from(b'='));
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }

    /// One pipe: the end this process keeps, and the end the child inherits.
    unsafe fn pipe(child_reads: bool) -> io::Result<(HANDLE, HANDLE)> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut read, &mut write, &attributes, 0) == 0 {
            return Err(io::Error::last_os_error());
        }
        let (ours, theirs) = if child_reads {
            (write, read)
        } else {
            (read, write)
        };
        SetHandleInformation(ours, HANDLE_FLAG_INHERIT, 0);
        Ok((ours, theirs))
    }

    /// Starts `cmd` (program, arguments, environment and folder as set on it) in a hidden
    /// console, its standard input, output and error piped. A batch file is not started this
    /// way: `cmd.exe` would read its arguments with other rules.
    pub(crate) fn spawn(cmd: &Command) -> io::Result<HiddenChild> {
        let program = cmd.get_program();
        let lower = program.to_string_lossy().to_ascii_lowercase();
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "batch file"));
        }
        let mut line = std::iter::once(quote(&program.to_string_lossy()))
            .chain(cmd.get_args().map(|arg| quote(&arg.to_string_lossy())))
            .collect::<Vec<_>>()
            .join(" ");
        line.push('\0');
        let mut line: Vec<u16> = line.encode_utf16().collect();
        let block = environment(cmd);
        let folder: Option<Vec<u16>> = cmd
            .get_current_dir()
            .map(|dir| dir.as_os_str().encode_wide().chain(Some(0)).collect());
        let application: Option<Vec<u16>> = std::path::Path::new(program)
            .is_absolute()
            .then(|| OsStr::new(program).encode_wide().chain(Some(0)).collect());

        // SAFETY: plain Win32 calls. Every handle made here is either handed to a `File`,
        // owned by the returned value, or closed before returning; the attribute list and the
        // handles it names outlive CreateProcessW.
        unsafe {
            let (stdin_ours, stdin_theirs) = pipe(true)?;
            let (stdout_ours, stdout_theirs) = pipe(false)?;
            let (stderr_ours, stderr_theirs) = pipe(false)?;
            let close_all = |handles: &[HANDLE]| {
                for &handle in handles {
                    CloseHandle(handle);
                }
            };
            let theirs = [stdin_theirs, stdout_theirs, stderr_theirs];

            // Only these three are inherited, whatever else is inheritable at the moment.
            let mut bytes = 0usize;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
            let mut attrs = vec![0u8; bytes];
            let list = attrs.as_mut_ptr().cast();
            if InitializeProcThreadAttributeList(list, 1, 0, &mut bytes) == 0
                || UpdateProcThreadAttribute(
                    list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    theirs.as_ptr().cast(),
                    std::mem::size_of_val(&theirs),
                    null_mut(),
                    null(),
                ) == 0
            {
                let error = io::Error::last_os_error();
                close_all(&[stdin_ours, stdout_ours, stderr_ours]);
                close_all(&theirs);
                return Err(error);
            }

            let mut startup: STARTUPINFOEXW = std::mem::zeroed();
            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = list;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
            startup.StartupInfo.wShowWindow = HIDE;
            startup.StartupInfo.hStdInput = stdin_theirs;
            startup.StartupInfo.hStdOutput = stdout_theirs;
            startup.StartupInfo.hStdError = stderr_theirs;

            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let created = CreateProcessW(
                application.as_ref().map_or(null(), |name| name.as_ptr()),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                block.as_ptr().cast(),
                folder.as_ref().map_or(null(), |dir| dir.as_ptr()),
                &startup.StartupInfo,
                &mut info,
            );
            let error = io::Error::last_os_error();
            DeleteProcThreadAttributeList(list);
            // The child has its own copies now; ours would keep its output from ever ending.
            close_all(&theirs);
            if created == 0 {
                close_all(&[stdin_ours, stdout_ours, stderr_ours]);
                return Err(error);
            }
            CloseHandle(info.hThread);
            let job = crate::agents::process::ProcessJob::attach_handle(info.hProcess.cast());
            Ok(HiddenChild {
                process: info.hProcess as usize,
                stdin: Some(File::from_raw_handle(stdin_ours.cast())),
                stdout: Some(File::from_raw_handle(stdout_ours.cast())),
                stderr: Some(File::from_raw_handle(stderr_ours.cast())),
                job,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};

        #[test]
        fn a_program_in_a_hidden_console_gets_its_input_arguments_environment_and_folder() {
            let dir = tempfile::tempdir().unwrap();
            let mut cmd =
                Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
            cmd.args([
                "-NoProfile",
                "-Command",
                "$in = [Console]::In.ReadToEnd(); [Console]::Out.Write(\"$in|$env:STACKER_PROBE|$((Get-Location).Path)|a b\"); [Console]::Error.Write('err'); exit 3",
            ])
            .env("STACKER_PROBE", "yes")
            .current_dir(dir.path());
            let mut child = spawn(&cmd).unwrap();
            child.stdin.take().unwrap().write_all(b"hello").unwrap();
            let mut out = String::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut out)
                .unwrap();
            let mut err = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut err)
                .unwrap();
            let status = child.wait().unwrap();
            assert_eq!(status.code(), Some(3));
            let folder = dir.path().to_string_lossy().to_string();
            assert_eq!(out, format!("hello|yes|{folder}|a b"));
            assert_eq!(err, "err");
        }

        #[test]
        fn a_batch_file_is_left_to_the_ordinary_way() {
            let cmd = Command::new(r"C:\tools\npm.cmd");
            assert_eq!(
                spawn(&cmd).err().unwrap().kind(),
                io::ErrorKind::Unsupported
            );
        }
    }
}
