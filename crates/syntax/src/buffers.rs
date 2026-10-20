//! What the lexer writes: a style per byte, and a fold level and lexer state
//! per line. About one byte per byte of text, plus eight per line.

use std::{iter, ops::Range};

/// `SC_FOLDLEVELBASE`: the level of a line outside any fold.
pub(crate) const FOLD_BASE: i32 = 0x400;
/// `SC_FOLDLEVELNUMBERMASK`.
pub(crate) const FOLD_NUMBER: i32 = 0x0FFF;
/// `SC_FOLDLEVELHEADERFLAG`: the line starts a fold.
pub(crate) const FOLD_HEADER: i32 = 0x2000;

#[derive(Default)]
pub(crate) struct Buffers {
    pub(crate) styles: Vec<u8>,
    levels: Vec<i32>,
    states: Vec<i32>,
}

impl Buffers {
    pub(crate) fn new(len: usize, lines: usize) -> Self {
        Self { styles: vec![0; len], levels: vec![FOLD_BASE; lines], states: vec![0; lines] }
    }

    pub(crate) fn level(&self, line: usize) -> i32 {
        self.levels.get(line).copied().unwrap_or(FOLD_BASE)
    }

    pub(crate) fn set_level(&mut self, line: usize, level: i32) {
        if line >= self.levels.len() {
            self.levels.resize(line + 1, FOLD_BASE);
        }
        self.levels[line] = level;
    }

    pub(crate) fn line_state(&self, line: usize) -> i32 {
        self.states.get(line).copied().unwrap_or(0)
    }

    pub(crate) fn set_line_state(&mut self, line: usize, state: i32) {
        if line >= self.states.len() {
            self.states.resize(line + 1, 0);
        }
        self.states[line] = state;
    }

    /// Makes room for an edit that replaced `bytes` (old offsets) with
    /// `new_len` bytes, and the lines after `lines.start` up to `lines.end`
    /// (old numbering) with `new_lines` lines. New entries are blank.
    pub(crate) fn splice(&mut self, bytes: Range<usize>, new_len: usize, lines: Range<usize>, new_lines: usize) {
        let bytes = bytes.start.min(self.styles.len())..bytes.end.min(self.styles.len());
        self.styles.splice(bytes, iter::repeat_n(0, new_len));
        for (column, blank) in [(&mut self.levels, FOLD_BASE), (&mut self.states, 0)] {
            let start = (lines.start + 1).min(column.len());
            let end = (lines.end + 1).clamp(start, column.len());
            column.splice(start..end, iter::repeat_n(blank, new_lines));
        }
    }
}
