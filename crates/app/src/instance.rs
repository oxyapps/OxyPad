//! One OxyPad per user session. A second launch (a file opened from
//! Explorer or a file manager, `oxypad file` in a terminal) hands its files
//! to the running OxyPad and exits, so there is one session and one set of
//! backups, written by one process.
//!
//! The running OxyPad listens on a Unix socket (macOS, Linux) or a named
//! pipe (Windows). A message is the files to open, each a kind byte (`o`
//! open, `f` follow) and an absolute path ending in a NUL byte. An empty
//! message brings the running OxyPad to the front.

use std::{
    io::Read,
    path::{Path, PathBuf},
    thread,
};

use futures::channel::mpsc::UnboundedSender;

use crate::workspace::LaunchItem;

/// What a launch should do.
pub enum Claim {
    /// This is the only OxyPad: run, and take the files of later launches.
    Primary(Option<Listener>),
    /// The running OxyPad has the files: exit.
    Forwarded,
}

/// Hands `items` to a running OxyPad, or claims the role for this one.
pub fn claim(items: &[LaunchItem]) -> Claim {
    let message = encode(items);
    match platform::claim(&message) {
        Ok(claim) => claim,
        Err(err) => {
            // Better two OxyPads than none.
            eprintln!("oxypad: running without the single-instance check: {err:#}");
            Claim::Primary(None)
        }
    }
}

/// Passes the files of later launches to `sender`, from a thread of its own.
pub fn serve(listener: Listener, sender: UnboundedSender<Vec<LaunchItem>>) {
    thread::Builder::new()
        .name("oxypad-instance".into())
        .spawn(move || {
            listener.serve(|mut connection| {
                let mut message = Vec::new();
                // A launch that cannot finish its message opens nothing.
                if connection.read_to_end(&mut message).is_ok() {
                    let _ = sender.unbounded_send(decode(&message));
                }
            })
        })
        .expect("the single-instance thread starts");
}

fn encode(items: &[LaunchItem]) -> Vec<u8> {
    let mut message = Vec::new();
    for item in items {
        // The running OxyPad has its own working directory.
        let path = std::path::absolute(&item.path).unwrap_or_else(|_| item.path.clone());
        message.push(if item.follow { b'f' } else { b'o' });
        message.extend_from_slice(&path_bytes(&path));
        message.push(0);
    }
    message
}

fn decode(message: &[u8]) -> Vec<LaunchItem> {
    message
        .split(|byte| *byte == 0)
        .filter_map(|item| {
            let (kind, path) = item.split_first()?;
            (!path.is_empty()).then(|| LaunchItem { path: bytes_path(path), follow: *kind == b'f' })
        })
        .collect()
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn bytes_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(windows)]
fn bytes_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// The paths in `file://` URLs, as macOS sends opened files to an app.
pub fn paths_from_urls(urls: &[String]) -> Vec<PathBuf> {
    urls.iter().filter_map(|url| url.strip_prefix("file://")).map(|path| bytes_path(&percent_decode(path))).collect()
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        let hex = bytes.get(ix + 1..ix + 3).and_then(|hex| std::str::from_utf8(hex).ok());
        match (bytes[ix], hex.and_then(|hex| u8::from_str_radix(hex, 16).ok())) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                ix += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                ix += 1;
            }
        }
    }
    decoded
}

pub use platform::Listener;

#[cfg(unix)]
mod platform {
    use std::{
        io::{self, Write},
        os::unix::net::{UnixListener, UnixStream},
        path::PathBuf,
        time::Duration,
    };

    use super::Claim;

    pub struct Listener {
        listener: UnixListener,
        path: PathBuf,
    }

    impl Listener {
        pub fn serve(self, mut handle: impl FnMut(UnixStream)) {
            for connection in self.listener.incoming().flatten() {
                // A launch that stops sending must not hold up the next.
                let _ = connection.set_read_timeout(Some(Duration::from_secs(5)));
                handle(connection);
            }
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Per user: in the user's runtime directory on Linux, and in their own
    /// temporary directory on macOS.
    fn socket_path() -> PathBuf {
        let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
        let user = std::env::var("USER").unwrap_or_default();
        dir.join(format!("oxypad-{user}.sock"))
    }

    fn send(mut stream: UnixStream, message: &[u8]) -> io::Result<()> {
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.write_all(message)
    }

    pub fn claim(message: &[u8]) -> io::Result<Claim> {
        let path = socket_path();
        // Two tries: a socket left by an OxyPad that crashed is removed
        // after the first.
        for _ in 0..2 {
            if let Ok(stream) = UnixStream::connect(&path) {
                send(stream, message)?;
                return Ok(Claim::Forwarded);
            }
            match UnixListener::bind(&path) {
                Ok(listener) => return Ok(Claim::Primary(Some(Listener { listener, path }))),
                Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
                    // Another launch may have just started listening.
                    if let Ok(stream) = UnixStream::connect(&path) {
                        send(stream, message)?;
                        return Ok(Claim::Forwarded);
                    }
                    std::fs::remove_file(&path)?;
                }
                Err(err) => return Err(err),
            }
        }
        Err(io::Error::other(format!("{} is in use", path.display())))
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        io::{self, Write},
        os::windows::io::{FromRawHandle, OwnedHandle},
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND},
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
            },
            RemoteDesktop::ProcessIdToSessionId,
        },
        UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow},
    };

    use super::Claim;

    pub struct Listener {
        pipe: OwnedHandle,
    }

    impl Listener {
        pub fn serve(self, mut handle: impl FnMut(&File)) {
            let raw = std::os::windows::io::AsRawHandle::as_raw_handle(&self.pipe);
            // Borrowed by the File for reading; the pipe stays owned here.
            let pipe = std::mem::ManuallyDrop::new(unsafe { File::from_raw_handle(raw) });
            loop {
                // Waits for the next launch; it may already be connected.
                let connected = unsafe { ConnectNamedPipe(raw as _, ptr::null_mut()) } != 0
                    || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                if connected {
                    handle(&pipe);
                }
                unsafe { DisconnectNamedPipe(raw as _) };
            }
        }
    }

    /// Per user and per logon session, so users of one PC (or of Remote
    /// Desktop) each have their own OxyPad.
    fn pipe_name() -> Vec<u16> {
        let mut session = 0u32;
        unsafe { ProcessIdToSessionId(std::process::id(), &mut session) };
        let user = std::env::var("USERNAME").unwrap_or_default();
        format!(r"\\.\pipe\org.oxyapps.oxypad-{session}-{user}").encode_utf16().chain([0]).collect()
    }

    fn send(name: &[u16], message: &[u8]) -> io::Result<Option<Claim>> {
        let path = String::from_utf16_lossy(&name[..name.len() - 1]);
        // The running OxyPad may take a moment, serving another launch.
        for _ in 0..10 {
            match OpenOptions::new().write(true).open(&path) {
                Ok(mut pipe) => {
                    // Lets the running OxyPad bring its window to the front.
                    unsafe { AllowSetForegroundWindow(ASFW_ANY) };
                    pipe.write_all(message)?;
                    return Ok(Some(Claim::Forwarded));
                }
                Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                    unsafe { WaitNamedPipeW(name.as_ptr(), 1000) };
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(err) => return Err(err),
            }
        }
        Err(io::Error::other("the running OxyPad does not answer"))
    }

    pub fn claim(message: &[u8]) -> io::Result<Claim> {
        let name = pipe_name();
        for _ in 0..2 {
            if let Some(claim) = send(&name, message)? {
                return Ok(claim);
            }
            let pipe = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    0,
                    64 * 1024,
                    0,
                    ptr::null(),
                )
            };
            if pipe != INVALID_HANDLE_VALUE {
                let pipe = unsafe { OwnedHandle::from_raw_handle(pipe as _) };
                return Ok(Claim::Primary(Some(Listener { pipe })));
            }
            // Another launch has just created it: send to that one.
            if unsafe { GetLastError() } != ERROR_ACCESS_DENIED {
                return Err(io::Error::last_os_error());
            }
        }
        Err(io::Error::other("the OxyPad pipe is in use"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let items = vec![
            LaunchItem { path: PathBuf::from("/tmp/a b.txt"), follow: false },
            LaunchItem { path: PathBuf::from("/var/log/system.log"), follow: true },
        ];
        let decoded = decode(&encode(&items));
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].path, std::path::absolute("/tmp/a b.txt").unwrap());
        assert!(!decoded[0].follow);
        assert!(decoded[1].follow);
        assert!(decode(&[]).is_empty());
    }

    #[test]
    fn file_urls_become_paths() {
        let urls = vec!["file:///Users/me/My%20Notes/caf%C3%A9.txt".to_owned(), "https://example.com".to_owned()];
        assert_eq!(paths_from_urls(&urls), vec![PathBuf::from("/Users/me/My Notes/café.txt")]);
    }
}
