use std::path::Path;

use anyhow::{Context, Result};
use oxypad_core::{LineEnding, write_atomic};
use serde::{Deserialize, Serialize};

const PREFERENCES_FILE: &str = "settings.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEndingPreference {
    /// LF on macOS and Linux, CRLF on Windows.
    #[default]
    Native,
    Lf,
    CrLf,
    Cr,
}

impl LineEndingPreference {
    pub fn resolve(self) -> LineEnding {
        match self {
            Self::Native => LineEnding::native(),
            Self::Lf => LineEnding::Lf,
            Self::CrLf => LineEnding::CrLf,
            Self::Cr => LineEnding::Cr,
        }
    }
}

/// User preferences. Missing fields take their default, so files written by
/// older versions keep loading.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Interface language code (`en`, `pt-BR`); empty follows the system.
    pub language: String,
    pub theme: ThemePreference,
    /// Color theme used in light mode, by name; empty means the default.
    pub light_theme: String,
    /// Color theme used in dark mode, by name; empty means the default.
    pub dark_theme: String,
    /// Empty means the theme's monospace font.
    pub font_family: String,
    pub font_size: f32,
    pub tab_width: u8,
    pub insert_spaces: bool,
    pub show_line_numbers: bool,
    pub show_indent_guides: bool,
    /// Word wrap for tabs opened from now on.
    pub wrap_lines: bool,
    pub restore_session: bool,
    pub new_line_ending: LineEndingPreference,
    pub new_utf8_bom: bool,
    pub color_log_levels: bool,
    pub log_max_lines: u32,
    /// How much of an existing log is shown when it is opened, in KiB.
    pub log_initial_kib: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: String::new(),
            theme: ThemePreference::System,
            light_theme: String::new(),
            dark_theme: String::new(),
            font_family: String::new(),
            font_size: 13.0,
            tab_width: 4,
            insert_spaces: true,
            show_line_numbers: true,
            show_indent_guides: true,
            wrap_lines: false,
            restore_session: true,
            new_line_ending: LineEndingPreference::Native,
            new_utf8_bom: false,
            color_log_levels: true,
            log_max_lines: 200_000,
            log_initial_kib: 2048,
        }
    }
}

impl Preferences {
    pub const FONT_SIZES: std::ops::RangeInclusive<f32> = 8.0..=36.0;
    pub const TAB_WIDTHS: std::ops::RangeInclusive<u8> = 1..=16;
    pub const LOG_MAX_LINES: std::ops::RangeInclusive<u32> = 10_000..=2_000_000;
    pub const LOG_INITIAL_KIB: std::ops::RangeInclusive<u32> = 64..=65_536;

    /// Reads the preferences in `dir`; defaults when there are none yet or
    /// the file cannot be read.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(PREFERENCES_FILE);
        let preferences = match std::fs::read(&path) {
            Ok(json) => serde_json::from_slice::<Self>(&json).unwrap_or_else(|err| {
                eprintln!("oxypad: ignoring unreadable {}: {err}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        preferences.clamped()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        write_atomic(&dir.join(PREFERENCES_FILE), &serde_json::to_vec_pretty(self)?)
    }

    /// Keeps hand-edited values within what the UI offers.
    pub fn clamped(mut self) -> Self {
        self.font_size = self.font_size.clamp(*Self::FONT_SIZES.start(), *Self::FONT_SIZES.end());
        self.tab_width = self.tab_width.clamp(*Self::TAB_WIDTHS.start(), *Self::TAB_WIDTHS.end());
        self.log_max_lines = self.log_max_lines.clamp(*Self::LOG_MAX_LINES.start(), *Self::LOG_MAX_LINES.end());
        self.log_initial_kib =
            self.log_initial_kib.clamp(*Self::LOG_INITIAL_KIB.start(), *Self::LOG_INITIAL_KIB.end());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Preferences::load(dir.path()), Preferences::default());
    }

    #[test]
    fn round_trips_and_fills_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let preferences = Preferences { font_size: 15.0, theme: ThemePreference::Dark, ..Default::default() };
        preferences.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), preferences);

        std::fs::write(dir.path().join(PREFERENCES_FILE), r#"{"tab_width": 2}"#).unwrap();
        let loaded = Preferences::load(dir.path());
        assert_eq!(loaded.tab_width, 2);
        assert_eq!(loaded.font_size, 13.0);
    }

    #[test]
    fn clamps_out_of_range_values() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PREFERENCES_FILE), r#"{"font_size": 300, "tab_width": 0}"#).unwrap();
        let loaded = Preferences::load(dir.path());
        assert_eq!((loaded.font_size, loaded.tab_width), (36.0, 1));
    }
}
