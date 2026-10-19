//! Side-by-side comparison of two texts.
//!
//! [`align`] pairs the lines of both texts into rows a viewer can draw as two
//! columns: equal lines share a row, a changed line sits next to the line it
//! replaced, and a line present on one side only faces an empty filler cell.

use std::{ops::Range, time::Duration};

use similar::{ChangeTag, DiffOp, TextDiff};

/// Above this, the line diff gives up looking for the smallest diff and
/// settles for a correct but larger one, so huge files never hang the UI.
const DIFF_TIMEOUT: Duration = Duration::from_secs(2);

/// Paired lines more different than this get no word emphasis: when nearly
/// every word changed, marking them all is noise.
const MIN_SIMILARITY_FOR_EMPHASIS: f32 = 0.3;

/// Lines longer than this are not compared word by word.
const MAX_EMPHASIS_LINE_LEN: usize = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Equal,
    /// Both sides have a line, and they differ.
    Changed,
    /// Only the left side has a line.
    Removed,
    /// Only the right side has a line.
    Added,
}

/// One side of a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// Zero-based line index in that side's text.
    pub line: usize,
    /// Byte ranges within the line that differ from the other side.
    pub emphasis: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub left: Option<Cell>,
    pub right: Option<Cell>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Alignment {
    pub rows: Vec<Row>,
    /// Index of the first row of each block of differences, in order.
    pub changes: Vec<usize>,
    pub changed: usize,
    pub removed: usize,
    pub added: usize,
}

impl Alignment {
    pub fn is_identical(&self) -> bool {
        self.changes.is_empty()
    }
}

/// The lines of `text` as [`align`] numbers them: split on `\n`, without the
/// terminator, and without a phantom empty line after a final newline.
pub fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// Aligns the lines of `left` and `right` for a side-by-side view.
pub fn align(left: &str, right: &str) -> Alignment {
    let left_lines = lines(left);
    let right_lines = lines(right);
    let diff = TextDiff::configure().timeout(DIFF_TIMEOUT).diff_slices(&left_lines, &right_lines);

    let mut alignment = Alignment::default();
    let equal = |line, other| Row {
        kind: RowKind::Equal,
        left: Some(Cell { line, emphasis: Vec::new() }),
        right: Some(Cell { line: other, emphasis: Vec::new() }),
    };
    let only_left = |line| Row { kind: RowKind::Removed, left: Some(Cell { line, emphasis: Vec::new() }), right: None };
    let only_right = |line| Row { kind: RowKind::Added, left: None, right: Some(Cell { line, emphasis: Vec::new() }) };

    for op in diff.ops() {
        match *op {
            DiffOp::Equal { old_index, new_index, len } => {
                alignment.rows.extend((0..len).map(|i| equal(old_index + i, new_index + i)));
            }
            DiffOp::Delete { old_index, old_len, .. } => {
                alignment.changes.push(alignment.rows.len());
                alignment.removed += old_len;
                alignment.rows.extend((old_index..old_index + old_len).map(only_left));
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                alignment.changes.push(alignment.rows.len());
                alignment.added += new_len;
                alignment.rows.extend((new_index..new_index + new_len).map(only_right));
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                alignment.changes.push(alignment.rows.len());
                let paired = old_len.min(new_len);
                alignment.changed += paired;
                for i in 0..paired {
                    let (l, r) = (old_index + i, new_index + i);
                    let (left_emphasis, right_emphasis) = word_emphasis(left_lines[l], right_lines[r]);
                    alignment.rows.push(Row {
                        kind: RowKind::Changed,
                        left: Some(Cell { line: l, emphasis: left_emphasis }),
                        right: Some(Cell { line: r, emphasis: right_emphasis }),
                    });
                }
                alignment.removed += old_len - paired;
                alignment.added += new_len - paired;
                alignment.rows.extend((old_index + paired..old_index + old_len).map(only_left));
                alignment.rows.extend((new_index + paired..new_index + new_len).map(only_right));
            }
        }
    }
    alignment
}

/// Byte ranges of the words that differ between two paired lines.
fn word_emphasis(left: &str, right: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if left.len() > MAX_EMPHASIS_LINE_LEN || right.len() > MAX_EMPHASIS_LINE_LEN {
        return (Vec::new(), Vec::new());
    }
    let left_tokens = tokens(left);
    let right_tokens = tokens(right);
    let diff = TextDiff::configure().diff_slices(&left_tokens, &right_tokens);

    let (mut left_ranges, mut right_ranges) = (Vec::new(), Vec::new());
    let (mut left_at, mut right_at) = (0, 0);
    // Similarity counts only non-whitespace text, so shared spacing does not
    // make unrelated lines look alike.
    let mut same = 0;
    for change in diff.iter_all_changes() {
        let token = change.value();
        let len = token.len();
        match change.tag() {
            ChangeTag::Equal => {
                if !token.trim().is_empty() {
                    same += len;
                }
                left_at += len;
                right_at += len;
            }
            ChangeTag::Delete => {
                push_merged(&mut left_ranges, left_at..left_at + len);
                left_at += len;
            }
            ChangeTag::Insert => {
                push_merged(&mut right_ranges, right_at..right_at + len);
                right_at += len;
            }
        }
    }
    let total = non_whitespace_len(left) + non_whitespace_len(right);
    let similarity = if total == 0 { 1.0 } else { (2 * same) as f32 / total as f32 };
    if similarity < MIN_SIMILARITY_FOR_EMPHASIS {
        return (Vec::new(), Vec::new());
    }
    (bridge_whitespace(left, left_ranges), bridge_whitespace(right, right_ranges))
}

/// Joins ranges separated only by whitespace, so a changed phrase reads as
/// one highlight instead of one per word.
fn bridge_whitespace(line: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut joined: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match joined.last_mut() {
            Some(last) if line[last.end..range.start].trim().is_empty() => last.end = range.end,
            _ => joined.push(range),
        }
    }
    joined
}

/// Splits a line into identifiers/numbers, runs of whitespace and single
/// punctuation characters, so `compute(60)` and `compute_fast(60)` differ
/// only in the identifier.
fn tokens(line: &str) -> Vec<&str> {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let current = class(c);
        let joins_next = current != Class::Other
            && chars.peek().is_some_and(|&(_, next)| class(next) == current);
        if !joins_next {
            tokens.push(&line[start..i + c.len_utf8()]);
            start = i + c.len_utf8();
        }
    }
    tokens
}

fn non_whitespace_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).map(char::len_utf8).sum()
}

fn push_merged(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    match ranges.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => ranges.push(range),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(alignment: &Alignment) -> Vec<RowKind> {
        alignment.rows.iter().map(|row| row.kind).collect()
    }

    #[test]
    fn identical_texts_align_line_by_line() {
        let alignment = align("a\nb\n", "a\nb\n");
        assert!(alignment.is_identical());
        assert_eq!(kinds(&alignment), [RowKind::Equal, RowKind::Equal]);
    }

    #[test]
    fn pairs_changed_lines_and_fills_the_rest() {
        let left = "keep\nold one\nold two\nend\n";
        let right = "keep\nnew one\nend\nextra\n";
        let alignment = align(left, right);
        assert_eq!(
            kinds(&alignment),
            [RowKind::Equal, RowKind::Changed, RowKind::Removed, RowKind::Equal, RowKind::Added]
        );
        assert_eq!(alignment.changes, [1, 4]);
        assert_eq!((alignment.changed, alignment.removed, alignment.added), (1, 1, 1));
        // The filler side of a removed row is empty.
        assert_eq!(alignment.rows[2].right, None);
        assert_eq!(alignment.rows[2].left.as_ref().unwrap().line, 2);
    }

    #[test]
    fn emphasizes_only_the_words_that_changed() {
        let alignment = align("let value = compute(60);\n", "let value = compute_fast(60);\n");
        let row = &alignment.rows[0];
        assert_eq!(row.kind, RowKind::Changed);
        let left = row.left.as_ref().unwrap();
        let right = row.right.as_ref().unwrap();
        assert_eq!(&"let value = compute(60);"[left.emphasis[0].clone()], "compute");
        assert_eq!(&"let value = compute_fast(60);"[right.emphasis[0].clone()], "compute_fast");
    }

    #[test]
    fn joins_changed_words_across_spaces() {
        let left = "/// Counts how often each word appears.";
        let alignment = align(&format!("{left}\n"), "/// Counts words in this text.\n");
        let emphasis = &alignment.rows[0].left.as_ref().unwrap().emphasis;
        assert_eq!(emphasis.len(), 1);
        assert_eq!(&left[emphasis[0].clone()], "how often each word appears");
    }

    #[test]
    fn unrelated_lines_get_no_word_emphasis() {
        let alignment = align("alpha beta gamma\n", "one two three four\n");
        assert!(alignment.rows[0].left.as_ref().unwrap().emphasis.is_empty());
    }

    #[test]
    fn missing_final_newline_does_not_add_a_line() {
        assert_eq!(lines("a\nb"), lines("a\nb\n"));
        assert!(align("a\nb", "a\nb\n").is_identical());
    }
}
