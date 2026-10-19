use std::{
    fs::{self, File},
    io::Write,
    path::Path,
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

#[cfg(test)]
mod tests {
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
