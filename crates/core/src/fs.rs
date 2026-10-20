use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

/// Writes `bytes` to `path` so that a crash never leaves a half-written file:
/// the data goes to a temporary file in the same directory, is flushed to
/// disk, and then renamed over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .with_context(|| format!("{} has no file name", path.display()))?;

    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp_path = dir.join(tmp_name);

    let result = (|| {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("creating {}", tmp_path.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        // Keep the permissions of the file being replaced (e.g. executable scripts).
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&tmp_path, metadata.permissions())?;
        }
        fs::rename(&tmp_path, path)
            .with_context(|| format!("replacing {}", path.display()))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

/// The absolute path of `path` with links resolved, written the way people
/// and other programs know it: on Windows without the `\\?\` prefix that
/// `std::fs::canonicalize` adds, which File Explorer does not understand.
/// `path` itself when it cannot be resolved (e.g. it does not exist).
pub fn canonical_path(path: &Path) -> PathBuf {
    match fs::canonicalize(path) {
        Ok(resolved) => simplified(&resolved),
        Err(_) => simplified(path),
    }
}

/// `path` without a Windows `\\?\` prefix, where the plain form means the
/// same file: `\\?\C:\dir` becomes `C:\dir`, `\\?\UNC\server\share`
/// becomes `\\server\share`.
pub fn simplified(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else { return path.to_path_buf() };
    match strip_verbatim(text) {
        Some(plain) => PathBuf::from(plain),
        None => path.to_path_buf(),
    }
}

fn strip_verbatim(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{rest}"));
    }
    let rest = path.strip_prefix(r"\\?\")?;
    let bytes = rest.as_bytes();
    // A drive path; others (volume GUIDs) have no plain form. Over 259
    // characters the plain form would hit the old MAX_PATH limit.
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    (drive && rest.len() < 260).then(|| rest.to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn verbatim_prefixes_are_removed() {
        use super::strip_verbatim;
        assert_eq!(strip_verbatim(r"\\?\C:\temp\a.txt").as_deref(), Some(r"C:\temp\a.txt"));
        assert_eq!(strip_verbatim(r"\\?\UNC\server\share\a.txt").as_deref(), Some(r"\\server\share\a.txt"));
        assert_eq!(strip_verbatim(r"\\?\Volume{1234}\a.txt"), None);
        assert_eq!(strip_verbatim(r"C:\temp\a.txt"), None);
        assert_eq!(strip_verbatim("/tmp/a.txt"), None);
    }

    use super::*;

    #[test]
    fn replaces_existing_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, "old").unwrap();

        write_atomic(&path, b"new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
