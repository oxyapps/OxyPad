//! Syntax highlighting and folding with Lexilla, the lexers behind Scintilla,
//! SciTE and Notepad++.
//!
//! A lexer keeps one style byte per byte of text and a fold level per line,
//! instead of a syntax tree: a 1 MB file costs about 1 MB, where a Tree-sitter
//! tree costs some 30. Lexing is incremental: after an edit it restarts at the
//! edited line and stops as soon as the result matches what was there before.

mod buffers;
mod ffi;
mod keywords;
mod languages;

use std::ops::Range;

use buffers::{Buffers, FOLD_HEADER, FOLD_NUMBER};
pub use languages::{LANGUAGES, Language};

/// Text a lexer reads. Positions are byte offsets in UTF-8; line numbers
/// start at 0, and text ending in a line break has an empty last line.
pub trait Text {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copies the bytes from `position` into `buffer`, which fits in the text.
    fn read(&self, position: usize, buffer: &mut [u8]);

    /// The line holding `position` (which may be the end of the text).
    fn line_of(&self, position: usize) -> usize;

    /// Where `line` starts; the end of the text past the last line.
    fn line_start(&self, line: usize) -> usize;

    fn line_count(&self) -> usize;
}

/// A string with its line starts, as [`Text`].
pub struct StrText<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> StrText<'a> {
    pub fn new(text: &'a str) -> Self {
        let line_starts = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
        Self { text, line_starts }
    }
}

impl Text for StrText<'_> {
    fn len(&self) -> usize {
        self.text.len()
    }

    fn read(&self, position: usize, buffer: &mut [u8]) {
        buffer.copy_from_slice(&self.text.as_bytes()[position..position + buffer.len()]);
    }

    fn line_of(&self, position: usize) -> usize {
        self.line_starts.partition_point(|&start| start <= position) - 1
    }

    fn line_start(&self, line: usize) -> usize {
        self.line_starts.get(line).copied().unwrap_or(self.text.len())
    }

    fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

/// A replacement in the text: `start..old_end` became `start..new_end`,
/// with the line numbers of those offsets before and after.
#[derive(Clone, Copy, Debug)]
pub struct Edit {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
    pub start_line: usize,
    pub old_end_line: usize,
    pub new_end_line: usize,
}

/// Bytes lexed in one go; at most this much is redone needlessly when
/// lexing could have stopped earlier.
const CHUNK: usize = 16 * 1024;

/// Highlights one document.
pub struct Highlighter {
    language: &'static Language,
    lexer: ffi::Lexer,
    buffers: Buffers,
    /// `0..styled` is lexed. Always a line start, or the end of the text.
    styled: usize,
    /// `styled..reusable` was lexed before the last edits and is probably
    /// still right: lexing stops once its results match it again.
    reusable: usize,
    /// Lexing cannot stop before this, the end of the latest edits.
    edited_to: usize,
}

impl Highlighter {
    /// A highlighter for the OxyPad language `name`, if it has one.
    pub fn new(name: &str) -> Option<Self> {
        let language = languages::find(name)?;
        let mut lexer = ffi::Lexer::new(language.lexer)?;
        for (key, value) in languages::COMMON_PROPERTIES.iter().chain(language.properties) {
            lexer.set_property(key, value);
        }
        for (index, words) in language.keywords.iter().enumerate() {
            let extra = language.extra_keywords.get(index).copied().unwrap_or_default();
            lexer.set_keywords(index, &format!("{words} {extra}"));
        }
        for (index, extra) in language.extra_keywords.iter().enumerate().skip(language.keywords.len()) {
            lexer.set_keywords(index, extra);
        }
        Some(Self { language, lexer, buffers: Buffers::default(), styled: 0, reusable: 0, edited_to: 0 })
    }

    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// Forgets everything lexed, for a new text.
    pub fn reset(&mut self, text: &dyn Text) {
        self.buffers = Buffers::new(text.len(), text.line_count());
        self.styled = 0;
        self.reusable = 0;
        self.edited_to = 0;
    }

    /// Follows an edit already made to `text`. Lexing restarts at the edited
    /// line on the next [`Self::lex_to`].
    pub fn edit(&mut self, text: &dyn Text, edit: Edit) {
        let removed = edit.old_end - edit.start;
        let added = edit.new_end - edit.start;
        let consistent = edit.start <= edit.old_end
            && edit.start <= edit.new_end
            && self.buffers.styles.len() >= edit.old_end
            && self.buffers.styles.len() - removed + added == text.len();
        if !consistent {
            self.reset(text);
            return;
        }
        self.buffers.splice(
            edit.start..edit.old_end,
            added,
            edit.start_line..edit.old_end_line,
            edit.new_end_line - edit.start_line,
        );

        let shift = |position: usize| {
            if position <= edit.start {
                position
            } else if position >= edit.old_end {
                position - edit.old_end + edit.new_end
            } else {
                edit.new_end
            }
        };
        // Earlier edits may still be waiting to be lexed past.
        let pending = self.reusable > self.styled;
        let lexed = self.styled.max(self.reusable);
        let restart = text.line_start(edit.start_line);
        if restart < lexed {
            self.edited_to = if pending { shift(self.edited_to).max(edit.new_end) } else { edit.new_end };
            self.reusable = shift(lexed);
            self.styled = self.styled.min(restart);
        }
        self.styled = self.styled.min(text.len());
    }

    /// Lexes up to at least `end` (clamped to the text).
    pub fn lex_to(&mut self, text: &dyn Text, end: usize) {
        let end = end.min(text.len());
        if self.buffers.styles.len() != text.len() {
            self.reset(text);
        }
        while self.styled < end {
            let start = self.styled;
            let want = (start + CHUNK).min(text.len());
            let chunk_end = if want >= text.len() { text.len() } else { text.line_start(text.line_of(want) + 1) };
            let chunk_end = chunk_end.max(want).min(text.len());

            // Lexing may stop early only inside the styles kept from before
            // the edits and past the edits themselves.
            let check = chunk_end > self.edited_to && chunk_end < self.reusable;
            let before = check.then(|| self.state_at(text, chunk_end));

            if !self.lexer.run(text, &mut self.buffers, start, chunk_end) {
                // A lexer error: leave the rest unstyled.
                self.styled = text.len();
                self.reusable = 0;
                return;
            }
            self.styled = chunk_end;
            if before.is_some() && before == Some(self.state_at(text, chunk_end)) {
                self.styled = self.reusable;
            }
            if self.styled >= self.reusable {
                self.reusable = 0;
            }
        }
    }

    /// What the lexer carries into `position` (a line start): the style
    /// before it and the level and state of the line it ends.
    fn state_at(&self, text: &dyn Text, position: usize) -> (u8, i32, i32) {
        let line = text.line_of(position.saturating_sub(1));
        (self.buffers.styles[position - 1], self.buffers.level(line), self.buffers.line_state(line))
    }

    /// Where lexed text ends.
    pub fn styled_end(&self) -> usize {
        self.styled
    }

    /// Style runs covering `range` in order, with their highlight names
    /// (`None` for plain text, and for anything not lexed yet). Runs never
    /// split a UTF-8 character.
    pub fn styles(&self, text: &dyn Text, range: Range<usize>) -> Vec<(Range<usize>, Option<&'static str>)> {
        let end = range.end.min(text.len());
        let mut runs: Vec<(Range<usize>, Option<&'static str>)> = Vec::new();
        let mut at = range.start.min(end);
        let lexed_end = self.styled.min(end);
        let mut byte = [0u8];
        while at < lexed_end {
            let style = self.buffers.styles[at];
            let mut next = at + 1;
            while next < lexed_end {
                if self.buffers.styles[next] != style {
                    text.read(next, &mut byte);
                    // Never split a character: continue through its trail bytes.
                    if byte[0] & 0xC0 != 0x80 {
                        break;
                    }
                }
                next += 1;
            }
            let name = (self.language.style)(style);
            match runs.last_mut() {
                Some((last, last_name)) if *last_name == name => last.end = next,
                _ => runs.push((at..next, name)),
            }
            at = next;
        }
        if at < end {
            runs.push((at..end, None));
        }
        runs
    }

    /// Foldable line ranges in the lexed text: each fold's first line and
    /// its last, ordered by first line.
    pub fn folds(&self, text: &dyn Text) -> Vec<Range<usize>> {
        let last_line = if self.styled >= text.len() {
            text.line_count().saturating_sub(1)
        } else if self.styled == 0 {
            return Vec::new();
        } else {
            text.line_of(self.styled) - 1
        };
        let mut folds = Vec::new();
        let mut open: Vec<(usize, i32)> = Vec::new();
        for line in 0..=last_line {
            let level = self.buffers.level(line);
            let number = level & FOLD_NUMBER;
            while let Some(&(header, header_number)) = open.last() {
                if number > header_number {
                    break;
                }
                open.pop();
                if line - 1 > header {
                    folds.push(header..line - 1);
                }
            }
            if level & FOLD_HEADER != 0 {
                open.push((line, number));
            }
        }
        for (header, _) in open {
            if last_line > header {
                folds.push(header..last_line);
            }
        }
        folds.sort_by_key(|fold| fold.start);
        folds
    }
}

/// The OxyPad language names that have a lexer.
pub fn is_supported(name: &str) -> bool {
    languages::find(name).is_some()
}

#[cfg(test)]
mod tests;
