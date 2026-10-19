use std::{
    fmt, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

use crate::{fs::write_atomic, language::language_for_path};

/// Line terminator a file uses on disk. Text is always held with `\n` in
/// memory and converted back on save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    /// The platform's conventional line ending, used for new documents.
    pub fn native() -> Self {
        if cfg!(windows) { Self::CrLf } else { Self::Lf }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
            Self::Cr => "CR",
        }
    }

    /// Picks the most frequent terminator in `text`, defaulting to the
    /// native one when the text has no line breaks.
    fn detect(text: &str) -> Self {
        let bytes = text.as_bytes();
        let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                    crlf += 1;
                    i += 1;
                }
                b'\r' => cr += 1,
                b'\n' => lf += 1,
                _ => {}
            }
            i += 1;
        }
        if lf == 0 && crlf == 0 && cr == 0 {
            Self::native()
        } else if crlf >= lf && crlf >= cr {
            Self::CrLf
        } else if cr > lf {
            Self::Cr
        } else {
            Self::Lf
        }
    }
}

/// Character encoding of a file, including whether it starts with a BOM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEncoding {
    encoding: &'static Encoding,
    bom: bool,
}

impl Default for TextEncoding {
    fn default() -> Self {
        Self { encoding: UTF_8, bom: false }
    }
}

impl TextEncoding {
    pub fn name(&self) -> &'static str {
        self.encoding.name()
    }

    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// Detects the encoding from a BOM, then UTF-8 validity, then a
    /// statistical guess for legacy code pages.
    fn detect(bytes: &[u8]) -> Self {
        if let Some((encoding, _)) = Encoding::for_bom(bytes) {
            return Self { encoding, bom: true };
        }
        if std::str::from_utf8(bytes).is_ok() {
            return Self::default();
        }
        let mut detector = chardetng::EncodingDetector::new();
        detector.feed(bytes, true);
        Self { encoding: detector.guess(None, true), bom: false }
    }

    fn decode(&self, bytes: &[u8]) -> String {
        let bytes = if self.bom {
            &bytes[Encoding::for_bom(bytes).map_or(0, |(_, len)| len)..]
        } else {
            bytes
        };
        let (text, _) = self.encoding.decode_without_bom_handling(bytes);
        text.into_owned()
    }

    fn encode(&self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() + 3);
        // encoding_rs never encodes to UTF-16 (it falls back to UTF-8), so
        // those two are done by hand.
        if self.encoding == UTF_16LE || self.encoding == UTF_16BE {
            let little_endian = self.encoding == UTF_16LE;
            if self.bom {
                out.extend_from_slice(if little_endian { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
            }
            for unit in text.encode_utf16() {
                out.extend_from_slice(&if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            return out;
        }
        if self.bom && self.encoding == UTF_8 {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        let (bytes, _, _) = self.encoding.encode(text);
        out.extend_from_slice(&bytes);
        out
    }
}

impl fmt::Display for TextEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bom && self.encoding == UTF_8 {
            write!(f, "UTF-8 BOM")
        } else {
            write!(f, "{}", self.encoding.name())
        }
    }
}

/// What a file looked like on disk when a document last read or wrote it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DiskStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl DiskStamp {
    fn read(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self { modified: metadata.modified().ok(), len: metadata.len() })
    }
}

/// How the file behind a document changed since it was last read or saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalChange {
    /// Another program wrote to the file.
    Modified,
    /// The file was deleted or moved away.
    Deleted,
}

/// A file's identity and on-disk format. The text itself lives in the
/// editor; `Document` knows how to read it from and write it back to disk
/// without changing its encoding or line endings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    path: Option<PathBuf>,
    encoding: TextEncoding,
    line_ending: LineEnding,
    /// The file as last read or written; `None` before that.
    stamp: Option<DiskStamp>,
}

impl Document {
    /// A new, unsaved document using the platform's conventions.
    pub fn untitled() -> Self {
        Self::untitled_with(LineEnding::native(), false)
    }

    /// A new, unsaved UTF-8 document with the given line ending, starting
    /// with a byte order mark when `utf8_bom` is set.
    pub fn untitled_with(line_ending: LineEnding, utf8_bom: bool) -> Self {
        Self { path: None, encoding: TextEncoding { encoding: UTF_8, bom: utf8_bom }, line_ending, stamp: None }
    }

    /// Reads `path` and returns the document with its text normalized to `\n`.
    pub fn open(path: &Path) -> Result<(Self, String)> {
        // Stamped before reading: a write racing with the read shows up as a
        // change later instead of being missed.
        let stamp = DiskStamp::read(path).ok();
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let (mut document, text) = Self::decode(&bytes);
        document.path = Some(path.to_path_buf());
        document.stamp = stamp;
        Ok((document, text))
    }

    /// Decodes raw file contents. The returned document has no path.
    pub fn decode(bytes: &[u8]) -> (Self, String) {
        let encoding = TextEncoding::detect(bytes);
        let raw = encoding.decode(bytes);
        let line_ending = LineEnding::detect(&raw);
        let text = normalize_line_endings(&raw);
        (Self { path: None, encoding, line_ending, stamp: None }, text)
    }

    /// Encodes `text` (with `\n` line breaks) the way this document is stored.
    pub fn encode(&self, text: &str) -> Vec<u8> {
        let text = match self.line_ending {
            LineEnding::Lf => std::borrow::Cow::Borrowed(text),
            other => std::borrow::Cow::Owned(text.replace('\n', other.as_str())),
        };
        self.encoding.encode(&text)
    }

    /// Writes `text` to the document's path. Fails for untitled documents.
    pub fn save(&mut self, text: &str) -> Result<()> {
        let path = self.path.clone().context("the document has no file path yet")?;
        write_atomic(&path, &self.encode(text))?;
        self.stamp = DiskStamp::read(&path).ok();
        Ok(())
    }

    /// Writes `text` to `path` and makes it the document's path.
    pub fn save_as(&mut self, path: &Path, text: &str) -> Result<()> {
        write_atomic(path, &self.encode(text))?;
        self.path = Some(path.to_path_buf());
        self.stamp = DiskStamp::read(path).ok();
        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Points the document at `path` without touching the disk, e.g. to
    /// restore a tab whose file was deleted; the next save recreates it.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// Whether another program changed or removed the file since this
    /// document last read or wrote it. Nothing is reported for documents
    /// whose file was not on disk then (new, or restored after it vanished).
    pub fn external_change(&self) -> Option<ExternalChange> {
        let path = self.path.as_deref()?;
        let known = self.stamp?;
        match DiskStamp::read(path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Some(ExternalChange::Deleted),
            Err(_) => None,
            Ok(stamp) if stamp != known => Some(ExternalChange::Modified),
            Ok(_) => None,
        }
    }

    /// Takes the file as it is now on disk as the known version, so its
    /// current state is no longer reported as a change. For a deleted file
    /// this stops reporting until the document is saved again.
    pub fn accept_disk_version(&mut self) {
        self.stamp = self.path.as_deref().and_then(|path| DiskStamp::read(path).ok());
    }

    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
    }

    /// The highlighter language for this document's file type.
    pub fn language(&self) -> &'static str {
        self.path.as_deref().map_or("plaintext", language_for_path)
    }

    /// File name shown on tabs, if the document has been saved.
    pub fn file_name(&self) -> Option<String> {
        self.path
            .as_deref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }
}

fn normalize_line_endings(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_crlf_and_bom() {
        let bytes = b"\xEF\xBB\xBFone\r\ntwo\r\n";
        let (document, text) = Document::decode(bytes);
        assert_eq!(text, "one\ntwo\n");
        assert_eq!(document.line_ending(), LineEnding::CrLf);
        assert_eq!(document.encoding().to_string(), "UTF-8 BOM");
        assert_eq!(document.encode(&text), bytes);
    }

    #[test]
    fn round_trips_utf16le() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "olá\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let (document, text) = Document::decode(&bytes);
        assert_eq!(text, "olá\n");
        assert_eq!(document.encoding().name(), "UTF-16LE");
        assert_eq!(document.encode(&text), bytes);
    }

    #[test]
    fn detects_legacy_encoding() {
        // "ação, coração e informação" in Windows-1252.
        let bytes = b"a\xe7\xe3o, cora\xe7\xe3o e informa\xe7\xe3o\n";
        let (document, text) = Document::decode(bytes);
        assert_eq!(text, "ação, coração e informação\n");
        assert_eq!(document.encode(&text), bytes);
    }

    #[test]
    fn save_as_sets_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.rs");
        let mut document = Document::untitled();
        assert!(document.save("x").is_err());
        document.save_as(&path, "fn main() {}\n").unwrap();
        assert_eq!(document.language(), "rust");
        assert_eq!(document.file_name().as_deref(), Some("a.rs"));
        let (reopened, text) = Document::open(&path).unwrap();
        assert_eq!(text, "fn main() {}\n");
        assert_eq!(reopened.path(), Some(path.as_path()));
    }

    #[test]
    fn reports_external_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "one\n").unwrap();
        let (mut document, _) = Document::open(&path).unwrap();
        assert_eq!(document.external_change(), None);

        std::fs::write(&path, "one\ntwo\n").unwrap();
        assert_eq!(document.external_change(), Some(ExternalChange::Modified));
        document.accept_disk_version();
        assert_eq!(document.external_change(), None);

        // Our own saves are not external changes.
        document.save("three\n").unwrap();
        assert_eq!(document.external_change(), None);

        std::fs::remove_file(&path).unwrap();
        assert_eq!(document.external_change(), Some(ExternalChange::Deleted));
        document.accept_disk_version();
        assert_eq!(document.external_change(), None);
    }
}
