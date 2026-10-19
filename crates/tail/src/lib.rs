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

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use same_file::Handle;

mod level;

pub use level::{LogLevel, classify_line, level_spans};

/// Upper bound of bytes returned by a single [`LogFollower::poll`], so one
/// burst of logging never blocks the UI with a giant chunk.
pub const MAX_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

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
    file: File,
    handle: Handle,
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
        let path = path.into();
        let (file, handle) = open_with_handle(&path)?;
        let mut follower = Self {
            path,
            file,
            handle,
            offset: 0,
            pending: Vec::new(),
            initial_bytes,
        };
        let text = follower.read_initial()?;
        Ok((follower, text))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Checks the file for changes. Returns `None` when nothing new was
    /// written.
    pub fn poll(&mut self) -> io::Result<Option<TailEvent>> {
        // A different file at the path means the log was rotated. While the
        // path is missing (between rename and re-create) keep draining the
        // old file.
        if let Ok((file, handle)) = open_with_handle(&self.path)
            && handle != self.handle
        {
            self.file = file;
            self.handle = handle;
            self.offset = 0;
            self.pending.clear();
            let text = self.read_initial()?;
            return Ok(Some(TailEvent::Reset { text, reason: ResetReason::Rotated }));
        }

        let len = self.file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
            self.pending.clear();
            let text = self.read_initial()?;
            return Ok(Some(TailEvent::Reset { text, reason: ResetReason::Truncated }));
        }
        if len == self.offset {
            return Ok(None);
        }

        let to_read = (len - self.offset).min(MAX_CHUNK_BYTES);
        let mut buf = Vec::with_capacity(to_read as usize);
        self.file.seek(SeekFrom::Start(self.offset))?;
        (&mut self.file).take(to_read).read_to_end(&mut buf)?;
        self.offset += buf.len() as u64;

        let text = self.take_complete_lines(buf);
        let caught_up = self.offset >= len;
        Ok((!text.is_empty()).then_some(TailEvent::Appended { text, caught_up }))
    }

    /// Reads the last `initial_bytes` of the current file, starting at a
    /// line boundary.
    fn read_initial(&mut self) -> io::Result<String> {
        let len = self.file.metadata()?.len();
        let start = len.saturating_sub(self.initial_bytes);
        let mut buf = Vec::with_capacity((len - start) as usize);
        self.file.seek(SeekFrom::Start(start))?;
        (&mut self.file).take(len - start).read_to_end(&mut buf)?;
        self.offset = start + buf.len() as u64;

        if start > 0 {
            // The first line is cut in half; drop it.
            let cut = buf.iter().position(|&b| b == b'\n').map_or(buf.len(), |i| i + 1);
            buf.drain(..cut);
        }
        Ok(self.take_complete_lines(buf))
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

fn open_with_handle(path: &Path) -> io::Result<(File, Handle)> {
    let file = File::open(path)?;
    let handle = Handle::from_file(file.try_clone()?)?;
    Ok((file, handle))
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
