//! Follows a growing log file, like `tail -F`.
//!
//! [`LogFollower`] only reads what was appended since the last poll, so a
//! multi-gigabyte log costs no more than the bytes written to it. It keeps
//! working when the file is truncated (`> app.log`) or rotated (renamed and
//! recreated by logrotate and most logging frameworks).
//!
//! It is polled rather than driven by file-system events on purpose: events
//! are unreliable on network shares and some Windows setups, and a stat call
//! every few hundred milliseconds is cheap.
//!
//! It never stands in the way of the program writing the log: on Windows the
//! file is only open while new bytes are read (see [`KEEP_OPEN`]).

use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

mod level;

pub use level::{HighlightRule, LogLevel, LogMatcher, Mark, classify_line, level_spans};

/// Upper bound of bytes returned by a single [`LogFollower::poll`], so one
/// burst of logging never blocks the UI with a giant chunk.
pub const MAX_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// Whether the followed file stays open between polls.
///
/// On Unix it does, so lines written just before a rotation are still read
/// from the renamed file. On Windows a file kept open for reading makes
/// writers that refuse to share it fail with "file in use" (Windows
/// PowerShell's `Add-Content` and `Set-Content`, among others), so there it
/// is opened only to read new bytes and closed right away.
const KEEP_OPEN: bool = cfg!(not(windows));

/// What changed in the followed file since the previous poll.
#[derive(Debug, PartialEq, Eq)]
pub enum TailEvent {
    /// New complete lines were appended. Every line ends with `\n`.
    ///
    /// `caught_up` is false when more data is already waiting; poll again
    /// right away instead of waiting for the next tick.
    Appended { text: String, caught_up: bool },
    /// The file was truncated or replaced; `text` is its new tail and
    /// replaces everything shown so far.
    Reset { text: String, reason: ResetReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetReason {
    Truncated,
    Rotated,
}

pub struct LogFollower {
    path: PathBuf,
    /// The file being followed, to notice when another one takes its path.
    id: FileId,
    /// Whether `file` stays open between polls ([`KEEP_OPEN`]).
    keep_open: bool,
    /// The open file, when kept open.
    file: Option<File>,
    /// Bytes of the file consumed, including `pending`.
    offset: u64,
    /// Trailing bytes of a line that has not been terminated yet.
    pending: Vec<u8>,
    initial_bytes: u64,
}

impl LogFollower {
    /// Opens `path` and returns the follower with the file's last
    /// `initial_bytes` (rounded to whole lines) as initial text.
    pub fn open(path: impl Into<PathBuf>, initial_bytes: u64) -> io::Result<(Self, String)> {
        Self::open_with(path.into(), initial_bytes, KEEP_OPEN)
    }

    fn open_with(path: PathBuf, initial_bytes: u64, keep_open: bool) -> io::Result<(Self, String)> {
        let id = FileId::of(&path)?;
        let file = if keep_open { Some(File::open(&path)?) } else { None };
        let mut follower = Self { path, id, keep_open, file, offset: 0, pending: Vec::new(), initial_bytes };
        // A writer may hold the file exclusively for a moment; wait for it.
        for _ in 0..40 {
            if let Some(text) = follower.read_initial()? {
                return Ok((follower, text));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Err(io::Error::new(io::ErrorKind::ResourceBusy, "the file is locked by another program"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Checks the file for changes. Returns `None` when nothing new was
    /// written, or while another program holds the file exclusively.
    pub fn poll(&mut self) -> io::Result<Option<TailEvent>> {
        // A different file at the path means the log was rotated. While the
        // path is missing (between rename and re-create) keep reading the
        // old file where it is still open.
        if let Ok(id) = FileId::of(&self.path)
            && id != self.id
        {
            let file = if self.keep_open {
                match open_shared(&self.path)? {
                    Some(file) => Some(file),
                    None => return Ok(None),
                }
            } else {
                None
            };
            let previous = std::mem::replace(&mut self.file, file);
            return match self.read_initial()? {
                Some(text) => {
                    self.id = id;
                    Ok(Some(TailEvent::Reset { text, reason: ResetReason::Rotated }))
                }
                None => {
                    self.file = previous;
                    Ok(None)
                }
            };
        }

        let Some(len) = self.len()? else { return Ok(None) };
        if len < self.offset {
            let text = self.read_initial()?;
            return Ok(text.map(|text| TailEvent::Reset { text, reason: ResetReason::Truncated }));
        }
        if len == self.offset {
            return Ok(None);
        }

        let to_read = (len - self.offset).min(MAX_CHUNK_BYTES);
        let Some(buf) = self.read_range(self.offset, to_read)? else { return Ok(None) };
        self.offset += buf.len() as u64;

        let text = self.take_complete_lines(buf);
        let caught_up = self.offset >= len;
        Ok((!text.is_empty()).then_some(TailEvent::Appended { text, caught_up }))
    }

    /// Reads the last `initial_bytes` of the current file, starting at a
    /// line boundary. `None` when the file could not be read right now; the
    /// follower is then left as it was.
    fn read_initial(&mut self) -> io::Result<Option<String>> {
        let Some(len) = self.len()? else { return Ok(None) };
        let start = len.saturating_sub(self.initial_bytes);
        let Some(mut buf) = self.read_range(start, len - start)? else { return Ok(None) };
        self.offset = start + buf.len() as u64;
        self.pending.clear();

        if start > 0 {
            // The first line is cut in half; drop it.
            let cut = buf.iter().position(|&b| b == b'\n').map_or(buf.len(), |i| i + 1);
            buf.drain(..cut);
        }
        Ok(Some(self.take_complete_lines(buf)))
    }

    /// Current length of the followed file; `None` while its path is missing
    /// and it is not held open.
    fn len(&self) -> io::Result<Option<u64>> {
        match &self.file {
            Some(file) => Ok(Some(file.metadata()?.len())),
            // Reads attributes only, which never conflicts with a writer.
            None => match fs::metadata(&self.path) {
                Ok(metadata) => Ok(Some(metadata.len())),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(err),
            },
        }
    }

    /// Reads `len` bytes from `start`. `None` while another program holds
    /// the file exclusively.
    fn read_range(&self, start: u64, len: u64) -> io::Result<Option<Vec<u8>>> {
        let opened;
        let mut file = match &self.file {
            Some(file) => file,
            None => match open_shared(&self.path)? {
                Some(file) => {
                    opened = file;
                    &opened
                }
                None => return Ok(None),
            },
        };
        let mut buf = Vec::with_capacity(len as usize);
        file.seek(SeekFrom::Start(start))?;
        file.take(len).read_to_end(&mut buf)?;
        Ok(Some(buf))
    }

    /// Prepends `pending`, returns every complete line in `buf` and keeps the
    /// unterminated rest for the next poll.
    fn take_complete_lines(&mut self, buf: Vec<u8>) -> String {
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(&buf);
        let complete = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        self.pending = bytes.split_off(complete);
        decode_lines(&bytes)
    }
}

/// Opens `path` for reading, letting others read, write and delete it.
/// `None` when another program holds it in a way that excludes readers.
fn open_shared(path: &Path) -> io::Result<Option<File>> {
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(err) if is_busy(&err) => Ok(None),
        Err(err) => Err(err),
    }
}

/// A sharing or lock violation: the file is in use, try again later.
fn is_busy(err: &io::Error) -> bool {
    // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION.
    cfg!(windows) && matches!(err.raw_os_error(), Some(32 | 33))
}

/// Identity of a file, independent of its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileId {
    volume: u64,
    index: u64,
}

impl FileId {
    #[cfg(unix)]
    fn of(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path)?;
        Ok(Self { volume: metadata.dev(), index: metadata.ino() })
    }

    /// Opened without read or write access, which takes no part in file
    /// sharing, so no writer is ever refused because of it.
    #[cfg(windows)]
    fn of(path: &Path) -> io::Result<Self> {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};

        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, GetFileInformationByHandle,
        };

        let file = fs::OpenOptions::new()
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open for the duration of the call and `info`
        // is a valid, writable BY_HANDLE_FILE_INFORMATION.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            volume: info.dwVolumeSerialNumber as u64,
            index: (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64,
        })
    }
}

/// Logs are read as UTF-8; invalid bytes become U+FFFD instead of failing.
/// `\r\n` is normalized to `\n`.
fn decode_lines(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.contains('\r') {
        text.replace("\r\n", "\n")
    } else {
        text.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use super::*;

    fn append(path: &Path, text: &str) {
        fs::OpenOptions::new().append(true).create(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn initial_read_starts_at_a_line_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        append(&path, "first line\nsecond\nthird\n");

        let (_, text) = LogFollower::open(&path, 10).unwrap();

        assert_eq!(text, "third\n");
    }

    #[test]
    fn follows_appends_and_waits_for_complete_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        append(&path, "a\n");
        let (mut follower, text) = LogFollower::open(&path, 1024).unwrap();
        assert_eq!(text, "a\n");
        assert_eq!(follower.poll().unwrap(), None);

        append(&path, "b\r\nhalf");
        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Appended { text: "b\n".into(), caught_up: true })
        );
        append(&path, " line\n");
        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Appended { text: "half line\n".into(), caught_up: true })
        );
    }

    #[test]
    fn detects_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        append(&path, "old 1\nold 2\n");
        let (mut follower, _) = LogFollower::open(&path, 1024).unwrap();

        fs::OpenOptions::new().write(true).truncate(true).open(&path).unwrap().write_all(b"new\n").unwrap();

        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Reset { text: "new\n".into(), reason: ResetReason::Truncated })
        );
    }

    // Draining needs the old file held open, which only Unix does.
    #[cfg(not(windows))]
    #[test]
    fn detects_rotation_and_drains_while_path_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        append(&path, "one\n");
        let (mut follower, _) = LogFollower::open(&path, 1024).unwrap();

        append(&path, "two\n");
        fs::rename(&path, dir.path().join("app.log.1")).unwrap();
        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Appended { text: "two\n".into(), caught_up: true })
        );

        append(&path, "fresh\n");
        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Reset { text: "fresh\n".into(), reason: ResetReason::Rotated })
        );
    }

    /// Windows PowerShell's `Add-Content` opens the log sharing it for
    /// writing only, which fails while anyone has it open for reading.
    #[cfg(windows)]
    #[test]
    fn does_not_lock_out_writers_that_refuse_readers() {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_WRITE: u32 = 2;
        let add_content = |path: &Path| {
            fs::OpenOptions::new().append(true).share_mode(FILE_SHARE_WRITE).open(path)
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        append(&path, "one\n");
        let (mut follower, _) = LogFollower::open(&path, 1024).unwrap();

        // Following must not keep the writer out.
        let mut writer = add_content(&path).expect("the follower must not hold the file open");
        writer.write_all(b"two\n").unwrap();
        // While the writer holds it, polling waits instead of failing.
        assert_eq!(follower.poll().unwrap(), None);
        drop(writer);

        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Appended { text: "two\n".into(), caught_up: true })
        );
    }

    /// The Windows way (file opened only to read) on any platform.
    #[test]
    fn follows_without_keeping_the_file_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        append(&path, "one\n");
        let (mut follower, text) = LogFollower::open_with(path.clone(), 1024, false).unwrap();
        assert_eq!(text, "one\n");
        assert!(follower.file.is_none());

        append(&path, "two\n");
        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Appended { text: "two\n".into(), caught_up: true })
        );

        fs::write(&path, "short\n").unwrap();
        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Reset { text: "short\n".into(), reason: ResetReason::Truncated })
        );

        // Rotation: while the path is missing nothing happens, then the new
        // file is picked up.
        fs::rename(&path, dir.path().join("app.log.1")).unwrap();
        assert_eq!(follower.poll().unwrap(), None);
        append(&path, "fresh\n");
        assert_eq!(
            follower.poll().unwrap(),
            Some(TailEvent::Reset { text: "fresh\n".into(), reason: ResetReason::Rotated })
        );
        assert!(follower.file.is_none());
    }

    #[test]
    fn large_bursts_arrive_in_bounded_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        append(&path, "start\n");
        let (mut follower, _) = LogFollower::open(&path, 1024).unwrap();

        let line = "x".repeat(99) + "\n";
        append(&path, &line.repeat((MAX_CHUNK_BYTES as usize / 100) + 50));

        let mut total = 0;
        let mut polls = 0;
        loop {
            match follower.poll().unwrap() {
                Some(TailEvent::Appended { text, caught_up }) => {
                    assert!(text.len() as u64 <= MAX_CHUNK_BYTES);
                    total += text.len();
                    polls += 1;
                    if caught_up {
                        break;
                    }
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(polls, 2);
        assert_eq!(total, line.len() * ((MAX_CHUNK_BYTES as usize / 100) + 50));
    }
}
