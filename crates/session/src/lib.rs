//! Session persistence, in the spirit of Notepad++'s "remember unsaved
//! changes": open tabs and their unsaved text survive quitting and crashes
//! without ever asking the user to save.
//!
//! On disk, in the application data directory:
//!
//! ```text
//! oxypad/
//!   session.json      windows and their tabs, order, cursor positions
//!   backups/<id>.txt  unsaved text of each modified or untitled tab
//! ```

use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use oxypad_core::write_atomic;
use serde::{Deserialize, Serialize};

mod preferences;

pub use preferences::{LineEndingPreference, Preferences, ThemePreference};

const SESSION_FILE: &str = "session.json";
const BACKUP_DIR: &str = "backups";
/// 2: windows, each with its tabs. 1: one list of tabs, read as one window.
const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    #[default]
    Edit,
    /// A read-only log followed like `tail -f`.
    Tail,
}

/// A color the user gave a tab, from the theme's palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabColor {
    Red,
    Yellow,
    Green,
    Cyan,
    Blue,
    Magenta,
}

impl TabColor {
    pub const ALL: [Self; 6] = [Self::Red, Self::Yellow, Self::Green, Self::Cyan, Self::Blue, Self::Magenta];
}

/// One tab as it was when the session was saved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSnapshot {
    /// Stable id, also the backup file name.
    pub id: u64,
    pub kind: TabKind,
    /// File on disk; `None` for untitled tabs.
    pub path: Option<PathBuf>,
    /// Name shown for untitled tabs ("new 1").
    pub title: String,
    /// Zero-based cursor line and column.
    pub cursor: (u32, u32),
    /// Tail tabs: whether the view sticks to the end of the log.
    #[serde(default)]
    pub follow: bool,
    /// Kept at the left of the tab bar, with the other pinned tabs.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TabColor>,
    /// Edit tabs: the text cannot be changed until this is turned off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_only: bool,
    /// Text not yet saved to `path` (or the whole text of an untitled tab).
    /// Stored in `backups/<id>.txt`, never inside `session.json`.
    #[serde(skip)]
    pub unsaved_text: Option<String>,
}

/// One window: its tabs, in order, and the one in front.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSession {
    pub tabs: Vec<TabSnapshot>,
    pub active: usize,
}

/// The open windows, in the order they were opened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    pub windows: Vec<WindowSession>,
}

impl Session {
    pub fn tabs(&self) -> impl Iterator<Item = &TabSnapshot> {
        self.windows.iter().flat_map(|window| &window.tabs)
    }
}

#[derive(Serialize)]
struct SessionFileOut<'a> {
    version: u32,
    windows: &'a [WindowSession],
}

#[derive(Deserialize)]
struct SessionFileIn {
    #[serde(default)]
    windows: Vec<WindowSession>,
    // Version 1: the tabs of the only window.
    #[serde(default)]
    tabs: Vec<TabSnapshot>,
    #[serde(default)]
    active: usize,
}

/// The per-user data directory holding the session and the preferences:
/// `~/Library/Application Support/oxypad` on macOS, `%LOCALAPPDATA%\oxypad`
/// on Windows and `~/.local/share/oxypad` on Linux. `OXYPAD_DATA_DIR`
/// overrides it (development, portable installs).
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("OXYPAD_DATA_DIR") {
        return Ok(dir.into());
    }
    let base = dirs::data_local_dir().context("no data directory for this user")?;
    Ok(base.join("oxypad"))
}

/// Reads and writes the session in one directory. Remembers what it last
/// wrote so unchanged backups are not rewritten on every autosave.
pub struct SessionStore {
    dir: PathBuf,
    written: HashMap<u64, u64>,
}

impl SessionStore {
    /// The session in [`data_dir`].
    pub fn default_location() -> Result<Self> {
        Ok(Self::at(data_dir()?))
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), written: HashMap::new() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Loads the last session, or `None` on first launch. Tabs whose backup
    /// file has gone missing are kept, just without their unsaved text.
    pub fn load(&mut self) -> Result<Option<Session>> {
        let path = self.dir.join(SESSION_FILE);
        let json = match fs::read(&path) {
            Ok(json) => json,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        let file: SessionFileIn =
            serde_json::from_slice(&json).with_context(|| format!("parsing {}", path.display()))?;
        let mut windows = file.windows;
        if windows.is_empty() && !file.tabs.is_empty() {
            windows.push(WindowSession { tabs: file.tabs, active: file.active });
        }

        for window in &mut windows {
            for tab in &mut window.tabs {
                if let Ok(text) = fs::read_to_string(self.backup_path(tab.id)) {
                    self.written.insert(tab.id, hash(&text));
                    tab.unsaved_text = Some(text);
                }
            }
            window.active = window.active.min(window.tabs.len().saturating_sub(1));
        }
        Ok(Some(Session { windows }))
    }

    /// Writes the session and the backups of tabs with unsaved text, and
    /// deletes backups that are no longer needed.
    pub fn save(&mut self, session: &Session) -> Result<()> {
        let backup_dir = self.dir.join(BACKUP_DIR);
        fs::create_dir_all(&backup_dir)
            .with_context(|| format!("creating {}", backup_dir.display()))?;

        let mut keep = HashSet::new();
        for tab in session.tabs() {
            let Some(text) = &tab.unsaved_text else { continue };
            keep.insert(tab.id);
            let digest = hash(text);
            if self.written.get(&tab.id) != Some(&digest) {
                write_atomic(&self.backup_path(tab.id), text.as_bytes())?;
                self.written.insert(tab.id, digest);
            }
        }

        let file = SessionFileOut { version: FORMAT_VERSION, windows: &session.windows };
        write_atomic(&self.dir.join(SESSION_FILE), &serde_json::to_vec_pretty(&file)?)?;

        // Only after session.json no longer refers to them.
        self.written.retain(|id, _| keep.contains(id));
        for entry in fs::read_dir(&backup_dir)?.flatten() {
            let stale = entry
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str()?.parse::<u64>().ok())
                .is_some_and(|id| !keep.contains(&id));
            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    fn backup_path(&self, id: u64) -> PathBuf {
        self.dir.join(BACKUP_DIR).join(format!("{id}.txt"))
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

fn hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, unsaved: Option<&str>) -> TabSnapshot {
        TabSnapshot {
            id,
            title: format!("new {id}"),
            unsaved_text: unsaved.map(str::to_owned),
            ..Default::default()
        }
    }

    fn one_window(tabs: Vec<TabSnapshot>, active: usize) -> Session {
        Session { windows: vec![WindowSession { tabs, active }] }
    }

    #[test]
    fn first_launch_has_no_session() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(SessionStore::at(dir.path()).load().unwrap(), None);
    }

    #[test]
    fn round_trips_windows_tabs_and_unsaved_text() {
        let dir = tempfile::tempdir().unwrap();
        let first = vec![
            tab(1, Some("draft ✍️")),
            TabSnapshot {
                id: 2,
                kind: TabKind::Tail,
                path: Some("/var/log/system.log".into()),
                follow: true,
                pinned: true,
                color: Some(TabColor::Cyan),
                ..Default::default()
            },
            TabSnapshot { id: 3, path: Some("/etc/hosts".into()), read_only: true, ..Default::default() },
        ];
        let session = Session {
            windows: vec![
                WindowSession { tabs: first, active: 1 },
                WindowSession { tabs: vec![tab(4, Some("second window"))], active: 0 },
            ],
        };

        SessionStore::at(dir.path()).save(&session).unwrap();
        let loaded = SessionStore::at(dir.path()).load().unwrap().unwrap();

        assert_eq!(loaded, session);
    }

    #[test]
    fn reads_version_1_sessions_as_one_window() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{"version":1,"tabs":[{"id":1,"kind":"edit","path":null,"title":"new 1","cursor":[0,0]},
            {"id":2,"kind":"edit","path":null,"title":"new 2","cursor":[0,0]}],"active":1}"#;
        fs::write(dir.path().join(SESSION_FILE), json).unwrap();
        let session = SessionStore::at(dir.path()).load().unwrap().unwrap();
        assert_eq!(session.windows.len(), 1);
        assert_eq!(session.windows[0].active, 1);
        let tab = &session.windows[0].tabs[0];
        assert!(!tab.pinned && !tab.read_only && tab.color.is_none());
    }

    #[test]
    fn removes_backups_of_closed_or_saved_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::at(dir.path());
        store.save(&one_window(vec![tab(1, Some("a")), tab(2, Some("b"))], 0)).unwrap();

        store.save(&one_window(vec![tab(1, None)], 0)).unwrap();

        let backups = fs::read_dir(dir.path().join(BACKUP_DIR)).unwrap().count();
        assert_eq!(backups, 0);
    }

    #[test]
    fn clamps_active_index() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::at(dir.path());
        store.save(&one_window(vec![tab(1, None)], 5)).unwrap();
        assert_eq!(store.load().unwrap().unwrap().windows[0].active, 0);
    }
}
