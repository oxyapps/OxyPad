//! Restarting OxyPad as administrator, to save a file the user may not
//! write to (Windows only).
//!
//! Files such as `C:\Windows\System32\drivers\etc\hosts` open for anyone but
//! are saved only by an administrator, and Windows grants those rights to a
//! program only when it starts. So, like Notepad++, OxyPad offers to start
//! again as administrator: the new OxyPad waits for this one to quit (there
//! is one OxyPad per user, see `instance.rs`), then restores the session,
//! unsaved text included, and the file can be saved.

use std::path::Path;

/// Whether saving `path` failed with `err` for want of rights an
/// administrator has. Not when OxyPad already is one, nor when the file is
/// only marked read-only, which an administrator cannot save over either.
pub fn would_help(err: &anyhow::Error, path: &Path) -> bool {
    let denied = err
        .chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|err| err.kind() == std::io::ErrorKind::PermissionDenied);
    let read_only = std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().readonly());
    denied && !read_only && platform::can_elevate()
}

/// Starts another OxyPad as administrator, with `path` to open, after
/// Windows asks the user for permission. It waits for this one to quit,
/// which is up to the caller. `Ok(false)` when the user said no.
pub fn restart(path: &Path) -> std::io::Result<bool> {
    platform::restart(path)
}

/// Waits for the OxyPad that started this one as administrator (`--after
/// PID`) to quit, so this one takes its place rather than handing it the
/// files. Gives up after a while: that OxyPad may have a question on screen.
pub fn wait_for_exit(pid: u32) {
    platform::wait_for_exit(pid);
}

#[cfg(windows)]
mod platform {
    use std::{io, os::windows::ffi::OsStrExt, path::Path, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_CANCELLED},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        UI::{
            Shell::{IsUserAnAdmin, ShellExecuteW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    /// How long the new OxyPad waits for the old one to quit.
    const WAIT_MS: u32 = 30_000;

    pub fn can_elevate() -> bool {
        unsafe { IsUserAnAdmin() == 0 }
    }

    fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        text.as_ref().encode_wide().chain([0]).collect()
    }

    pub fn restart(path: &Path) -> io::Result<bool> {
        let program = wide(std::env::current_exe()?);
        // A path cannot contain a quote, and a file's does not end in a backslash.
        let arguments = wide(format!("--after {} \"{}\"", std::process::id(), path.display()));
        // "runas" is what makes Windows ask for administrator rights.
        let started = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                wide("runas").as_ptr(),
                program.as_ptr(),
                arguments.as_ptr(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Over 32 means it started; anything else is an error code.
        if started as usize > 32 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(ERROR_CANCELLED as i32) { Ok(false) } else { Err(err) }
    }

    pub fn wait_for_exit(pid: u32) {
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        // Null when it is already gone.
        if !process.is_null() {
            unsafe {
                WaitForSingleObject(process, WAIT_MS);
                CloseHandle(process);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::{io, path::Path};

    // Elsewhere a program cannot be given more rights as it starts; saving
    // such a file takes `sudo` or the like, outside OxyPad.
    pub fn can_elevate() -> bool {
        false
    }

    pub fn restart(_: &Path) -> io::Result<bool> {
        Ok(false)
    }

    pub fn wait_for_exit(_: u32) {}
}
