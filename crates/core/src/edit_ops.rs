//! The Edit menu's operations from Notepad++, as MacPad++ has them: Indent,
//! Convert Case, Line Operations, Comment/Uncomment and Blank Operations.
//!
//! [`plan_edit`] takes the text and the selection and says what to change and
//! where to leave the selection; the editor applies that as one undo step.
//! Like in Notepad++, the line operations act on the selected lines or, with
//! no selection, on the whole document. The text uses `\n` line endings.

use std::ops::Range;

use crate::{
    comments::{CommentMode, CommentSymbols, Replacement, block_comment, block_uncomment, comment_lines},
    text_transform::{
        Case, Sort, SortError, Trim, convert_case, join_lines, remove_consecutive_duplicates, remove_duplicates,
        remove_empty_lines, reindent_line, sort_lines, spaces_to_tabs, tabs_to_spaces, trim_line,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOp {
    Indent { increase: bool },
    Case(Case),
    DuplicateLines,
    JoinLines,
    MoveLines { up: bool },
    InsertBlankLine { above: bool },
    RemoveDuplicateLines { consecutive_only: bool },
    RemoveEmptyLines { including_blank: bool },
    Sort(Sort),
    Comment(CommentMode),
    BlockComment,
    BlockUncomment,
    Trim(Trim),
    EolToSpace,
    TrimAndJoin,
    TabsToSpaces,
    SpacesToTabs { leading_only: bool },
}

/// What an operation needs to know besides the text.
#[derive(Clone, Copy, Debug)]
pub struct EditSettings {
    pub tab_width: usize,
    /// Indent with tabs instead of spaces.
    pub hard_tabs: bool,
    /// The comment symbols of the document's language.
    pub comments: CommentSymbols,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditProblem {
    /// A numeric sort met a line that does not start with a number; 1-based line.
    NotANumber { line: usize },
    /// The language has no comment symbols.
    NoComments,
}

/// The changes of one operation, in the text as it was, and the selections to
/// leave, in the text as it will be (the first is the active one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditPlan {
    pub edits: Vec<Replacement>,
    pub selections: Vec<Range<usize>>,
}

/// Plans `op` on `text`. `active` is the main selection, which the line
/// operations act on, and `all` every selection, which Convert Case converts.
/// `None` when there is nothing to change.
pub fn plan_edit(
    text: &str,
    active: Range<usize>,
    all: &[Range<usize>],
    op: EditOp,
    settings: EditSettings,
) -> Result<Option<EditPlan>, EditProblem> {
    let transform = |min_lines: usize, change: &dyn Fn(Vec<String>) -> Vec<String>| {
        Ok(transform_lines(text, &active, min_lines, |lines| Ok(change(lines))).map(into_plan))
    };
    match op {
        EditOp::Indent { increase } => Ok(indent(text, &active, settings, increase)),
        EditOp::Case(case) => Ok(convert(text, all, case)),
        EditOp::DuplicateLines => Ok(duplicate_lines(text, &active)),
        EditOp::JoinLines => Ok(join_selected_lines(text, &active)),
        EditOp::MoveLines { up } => Ok(move_lines(text, &active, up)),
        EditOp::InsertBlankLine { above } => Ok(insert_blank_line(text, &active, above)),
        EditOp::RemoveDuplicateLines { consecutive_only: true } => {
            transform(2, &|lines| remove_consecutive_duplicates(&lines))
        }
        EditOp::RemoveDuplicateLines { consecutive_only: false } => transform(2, &|lines| remove_duplicates(&lines)),
        EditOp::RemoveEmptyLines { including_blank } => transform(1, &|lines| remove_empty_lines(&lines, including_blank)),
        EditOp::Sort(sort) => sort_selected(text, &active, sort),
        EditOp::Comment(mode) => comment(text, &active, settings.comments, mode),
        EditOp::BlockComment => {
            if let Some((edits, selection)) = block_comment(text, active.clone(), settings.comments) {
                Ok(Some(EditPlan { edits, selections: vec![selection] }))
            } else if !settings.comments.line.is_empty() {
                comment(text, &active, settings.comments, CommentMode::Comment)
            } else {
                Err(EditProblem::NoComments)
            }
        }
        EditOp::BlockUncomment => {
            if settings.comments.has_block() {
                Ok(block_uncomment(text, active, settings.comments).map(|(edits, selection)| EditPlan {
                    edits,
                    selections: vec![selection],
                }))
            } else if !settings.comments.line.is_empty() {
                comment(text, &active, settings.comments, CommentMode::Uncomment)
            } else {
                Err(EditProblem::NoComments)
            }
        }
        EditOp::Trim(trim) => transform(1, &|lines| lines.iter().map(|line| trim_line(line, trim)).collect()),
        EditOp::EolToSpace => Ok(eol_to_space(text, &active, false)),
        EditOp::TrimAndJoin => Ok(eol_to_space(text, &active, true)),
        EditOp::TabsToSpaces => {
            transform(1, &|lines| lines.iter().map(|line| tabs_to_spaces(line, settings.tab_width)).collect())
        }
        EditOp::SpacesToTabs { leading_only } => transform(1, &|lines| {
            lines.iter().map(|line| spaces_to_tabs(line, settings.tab_width, leading_only)).collect()
        }),
    }
}

fn into_plan((edit, selection): (Replacement, Range<usize>)) -> EditPlan {
    EditPlan { edits: vec![edit], selections: vec![selection] }
}

// ---- Lines -----------------------------------------------------------------

fn line_start(text: &str, position: usize) -> usize {
    text[..position].rfind('\n').map_or(0, |at| at + 1)
}

fn line_end(text: &str, position: usize) -> usize {
    position + text[position..].find('\n').unwrap_or(text.len() - position)
}

/// The text of the lines a selection touches, without the last line ending:
/// a selection that ends at the start of a line does not include that line.
fn selected_lines(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = line_start(text, selection.start);
    let mut last = selection.end;
    if selection.end > selection.start && selection.end == line_start(text, selection.end) {
        last -= 1;
    }
    start..line_end(text, last)
}

/// `replacement` cut down to what really changes, so the cursor, folds and
/// undo of the untouched text are left alone.
fn narrowed(text: &str, range: Range<usize>, new: &str) -> Replacement {
    let old = &text[range.clone()];
    let mut prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
    // Not past the start of the changed text from the other side.
    let mut suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    suffix = suffix.min(old.len() - prefix).min(new.len() - prefix);
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    prefix = prefix.min(old.len()).min(new.len());
    Replacement { range: range.start + prefix..range.end - suffix, text: new[prefix..new.len() - suffix].to_owned() }
}

/// Replaces the selected lines (or every line) with `change(lines)`.
///
/// A final line ending stays where it is: sorting a file that ends with a
/// newline does not move an empty line to the top.
fn transform_lines(
    text: &str,
    selection: &Range<usize>,
    minimum_lines: usize,
    change: impl FnOnce(Vec<String>) -> Result<Vec<String>, EditProblem>,
) -> Option<(Replacement, Range<usize>)> {
    transform_lines_checked(text, selection, minimum_lines, change).ok().flatten()
}

fn transform_lines_checked(
    text: &str,
    selection: &Range<usize>,
    minimum_lines: usize,
    change: impl FnOnce(Vec<String>) -> Result<Vec<String>, EditProblem>,
) -> Result<Option<(Replacement, Range<usize>)>, EditProblem> {
    let whole = selection.is_empty();
    let range = if whole { 0..text.len() } else { selected_lines(text, selection) };
    let mut items: Vec<String> = text[range.clone()].split('\n').map(str::to_owned).collect();
    let mut trailing_empty = false;
    if whole && items.len() > 1 && items.last().is_some_and(String::is_empty) {
        items.pop();
        trailing_empty = true;
    }
    if items.len() < minimum_lines {
        return Ok(None);
    }
    let mut joined = change(items)?.join("\n");
    if trailing_empty {
        joined.push('\n');
    }
    if joined == text[range.clone()] {
        return Ok(None);
    }
    let selected = if whole {
        // The caret stays where it was in the text, as near as the new text allows.
        let caret = selection.start.min(range.start + joined.len());
        caret..caret
    } else {
        range.start..range.start + joined.len()
    };
    Ok(Some((narrowed(text, range, &joined), selected)))
}

fn sort_selected(
    text: &str,
    active: &Range<usize>,
    sort: Sort,
) -> Result<Option<EditPlan>, EditProblem> {
    // A selection inside one line is not sorted.
    if !active.is_empty() {
        let lines = selected_lines(text, active);
        if !text[lines].contains('\n') {
            return Ok(None);
        }
    }
    // The line the sorted lines start at, for the error message.
    let first_line = if active.is_empty() { 0 } else { text[..line_start(text, active.start)].matches('\n').count() };
    let plan = transform_lines_checked(text, active, 2, |lines| {
        sort_lines(&lines, sort).map_err(|SortError { line_index }| EditProblem::NotANumber { line: first_line + line_index + 1 })
    })?;
    Ok(plan.map(into_plan))
}

fn duplicate_lines(text: &str, active: &Range<usize>) -> Option<EditPlan> {
    let lines = selected_lines(text, active);
    let block = &text[lines.clone()];
    let copy_start = lines.end + 1;
    let selection = if active.is_empty() {
        let caret = active.start + block.len() + 1;
        caret..caret
    } else {
        copy_start..copy_start + block.len()
    };
    Some(EditPlan {
        edits: vec![Replacement { range: lines.end..lines.end, text: format!("\n{block}") }],
        selections: vec![selection],
    })
}

fn join_selected_lines(text: &str, active: &Range<usize>) -> Option<EditPlan> {
    let lines = selected_lines(text, active);
    if !text[lines.clone()].contains('\n') {
        return None;
    }
    let joined = join_lines(&text[lines.clone()]);
    let end = lines.start + joined.len();
    Some(EditPlan {
        edits: vec![Replacement { range: lines.clone(), text: joined }],
        selections: vec![lines.start..end],
    })
}

fn move_lines(text: &str, active: &Range<usize>, up: bool) -> Option<EditPlan> {
    let lines = selected_lines(text, active);
    let block = &text[lines.clone()];
    if up {
        if lines.start == 0 {
            return None;
        }
        let previous = line_start(text, lines.start - 1)..lines.start - 1;
        let shift = previous.len() + 1;
        Some(EditPlan {
            edits: vec![Replacement {
                range: previous.start..lines.end,
                text: format!("{block}\n{}", &text[previous]),
            }],
            selections: vec![active.start - shift..active.end - shift],
        })
    } else {
        if lines.end >= text.len() {
            return None;
        }
        let next = lines.end + 1..line_end(text, lines.end + 1);
        let shift = next.len() + 1;
        Some(EditPlan {
            edits: vec![Replacement {
                range: lines.start..next.end,
                text: format!("{}\n{block}", &text[next]),
            }],
            selections: vec![active.start + shift..active.end + shift],
        })
    }
}

fn insert_blank_line(text: &str, active: &Range<usize>, above: bool) -> Option<EditPlan> {
    let caret = active.end;
    let (at, selection) = if above {
        let start = line_start(text, caret);
        (start, start)
    } else {
        let end = line_end(text, caret);
        (end, end + 1)
    };
    Some(EditPlan {
        edits: vec![Replacement { range: at..at, text: "\n".into() }],
        selections: vec![selection..selection],
    })
}

/// "EOL to Space" joins the selected text, or the whole document, into one
/// line; with `trim`, after trimming every line.
fn eol_to_space(text: &str, active: &Range<usize>, trim: bool) -> Option<EditPlan> {
    let whole = active.is_empty();
    let range = if whole {
        0..text.len()
    } else if trim {
        selected_lines(text, active)
    } else {
        active.clone()
    };
    let source = &text[range.clone()];
    let source = if trim {
        source.split('\n').map(|line| trim_line(line, Trim::Both)).collect::<Vec<_>>().join("\n")
    } else {
        source.to_owned()
    };
    let joined = join_lines(&source);
    if joined == text[range.clone()] {
        return None;
    }
    let selection = if whole {
        let caret = active.start.min(joined.len());
        caret..caret
    } else {
        range.start..range.start + joined.len()
    };
    Some(EditPlan { edits: vec![narrowed(text, range, &joined)], selections: vec![selection] })
}

// ---- Indent ----------------------------------------------------------------

/// Moves the indentation of the selected lines (or the caret's line) by one
/// tab width, skipping empty lines, and keeps the selection on its text.
fn indent(text: &str, active: &Range<usize>, settings: EditSettings, increase: bool) -> Option<EditPlan> {
    let delta = settings.tab_width as isize * if increase { 1 } else { -1 };
    let lines = selected_lines(text, active);
    let old: Vec<&str> = text[lines.clone()].split('\n').collect();
    let new: Vec<String> = old
        .iter()
        .map(|line| {
            if line.is_empty() { String::new() } else { reindent_line(line, settings.tab_width, settings.hard_tabs, delta) }
        })
        .collect();
    let joined = new.join("\n");
    if joined == text[lines.clone()] {
        return None;
    }
    let map = LinesMap::new(&lines, &old, &new);
    Some(EditPlan {
        edits: vec![narrowed(text, lines, &joined)],
        selections: vec![map.position(active.start)..map.position(active.end)],
    })
}

/// Follows a position through lines whose indentation and ends changed but
/// whose text in between did not: in the indentation a position stays at the
/// same distance from the line start (as far as the new indentation allows),
/// anywhere else at the same distance from the line end.
struct LinesMap {
    /// Per line: start, indentation end and end in the old text, and the same in the new.
    lines: Vec<([usize; 3], [usize; 3])>,
}

impl LinesMap {
    fn new(range: &Range<usize>, old: &[&str], new: &[String]) -> Self {
        let (mut old_start, mut new_start) = (range.start, range.start);
        let mut lines = Vec::with_capacity(old.len());
        for (old_line, new_line) in old.iter().zip(new) {
            let indent = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
            lines.push((
                [old_start, old_start + indent(old_line), old_start + old_line.len()],
                [new_start, new_start + indent(new_line), new_start + new_line.len()],
            ));
            old_start += old_line.len() + 1;
            new_start += new_line.len() + 1;
        }
        Self { lines }
    }

    fn position(&self, position: usize) -> usize {
        let Some((old, new)) = self.lines.iter().find(|(old, _)| position <= old[2]).or(self.lines.last()) else {
            return position;
        };
        if position <= old[1] {
            new[0] + (position - old[0]).min(new[1] - new[0])
        } else {
            (new[2] - (old[2] - position.min(old[2]))).max(new[1])
        }
    }
}

// ---- Case ------------------------------------------------------------------

/// Converts each selection; does nothing without one, as in Notepad++.
fn convert(text: &str, all: &[Range<usize>], case: Case) -> Option<EditPlan> {
    let ranges: Vec<&Range<usize>> = all.iter().filter(|range| !range.is_empty()).collect();
    if ranges.is_empty() {
        return None;
    }
    let mut edits = Vec::new();
    let mut selections = Vec::with_capacity(ranges.len());
    let mut delta = 0isize;
    for range in ranges {
        let converted = convert_case(&text[range.clone()], case);
        let start = (range.start as isize + delta) as usize;
        selections.push(start..start + converted.len());
        delta += converted.len() as isize - range.len() as isize;
        if converted != text[range.clone()] {
            edits.push(Replacement { range: range.clone(), text: converted });
        }
    }
    (!edits.is_empty()).then_some(EditPlan { edits, selections })
}

// ---- Comments --------------------------------------------------------------

fn comment(
    text: &str,
    active: &Range<usize>,
    symbols: CommentSymbols,
    mode: CommentMode,
) -> Result<Option<EditPlan>, EditProblem> {
    if !symbols.any() {
        return Err(EditProblem::NoComments);
    }
    let block_uncomment = |text: &str| {
        block_uncomment(text, active.clone(), symbols).map(|(edits, selection)| EditPlan { edits, selections: vec![selection] })
    };
    // Languages with only block comments take their block symbols off lines.
    if symbols.line.is_empty() && mode == CommentMode::Uncomment {
        return Ok(block_uncomment(text));
    }
    let lines = selected_lines(text, active);
    let old: Vec<String> = text[lines.clone()].split('\n').map(str::to_owned).collect();
    let Some(result) = comment_lines(&old, mode, symbols) else { return Err(EditProblem::NoComments) };
    if mode == CommentMode::Uncomment && result.uncommented == 0 && symbols.has_block() {
        return Ok(block_uncomment(text));
    }
    let joined = result.lines.join("\n");
    if joined == text[lines.clone()] {
        return Ok(None);
    }
    let old_refs: Vec<&str> = old.iter().map(String::as_str).collect();
    let map = LinesMap::new(&lines, &old_refs, &result.lines);
    Ok(Some(EditPlan {
        edits: vec![narrowed(text, lines, &joined)],
        selections: vec![map.position(active.start)..map.position(active.end)],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::comment_symbols;

    fn settings(language: &str) -> EditSettings {
        EditSettings { tab_width: 4, hard_tabs: false, comments: comment_symbols(language) }
    }

    /// Runs `op` and returns the text and the selection afterwards.
    fn run(text: &str, selection: Range<usize>, op: EditOp, language: &str) -> (String, Range<usize>) {
        let result = plan_edit(text, selection.clone(), std::slice::from_ref(&selection), op, settings(language));
        let Some(plan) = result.unwrap() else { return (text.to_owned(), selection) };
        let mut edits = plan.edits;
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
        let mut text = text.to_owned();
        for edit in edits {
            text.replace_range(edit.range, &edit.text);
        }
        (text, plan.selections[0].clone())
    }

    fn text_after(text: &str, selection: Range<usize>, op: EditOp, language: &str) -> String {
        run(text, selection, op, language).0
    }

    // Lines

    #[test]
    fn sorting_the_whole_document_keeps_the_final_newline() {
        let sort = EditOp::Sort(Sort::LexicographicAscending);
        assert_eq!(text_after("c\na\nb\n", 0..0, sort, ""), "a\nb\nc\n");
    }

    #[test]
    fn sorting_selected_lines_only() {
        let sort = EditOp::Sort(Sort::LexicographicAscending);
        // From inside line 2 to inside line 4.
        let (text, selection) = run("z\nc\na\nb\ny", 3..7, sort, "");
        assert_eq!(text, "z\na\nb\nc\ny");
        assert_eq!(selection, 2..7);
        // A selection inside one line is not sorted.
        assert_eq!(text_after("b a\nc", 0..2, sort, ""), "b a\nc");
    }

    #[test]
    fn a_failed_numeric_sort_names_the_line() {
        let sort = EditOp::Sort(Sort::DecimalDotAscending);
        let result = plan_edit("1\n2\nx\n", 0..0, &[0..0], sort, settings(""));
        assert_eq!(result, Err(EditProblem::NotANumber { line: 3 }));
        // With a selection the line counts from the document's start.
        let result = plan_edit("0\n1\nx\n", 2..5, &[2..5], sort, settings(""));
        assert_eq!(result, Err(EditProblem::NotANumber { line: 3 }));
    }

    #[test]
    fn removing_duplicate_and_empty_lines() {
        let duplicates = EditOp::RemoveDuplicateLines { consecutive_only: false };
        let text = text_after("a\n\nb\na\n  \nb\n", 0..0, duplicates, "");
        assert_eq!(text, "a\n\nb\n  \n");
        let empty = EditOp::RemoveEmptyLines { including_blank: true };
        assert_eq!(text_after(&text, 0..0, empty, ""), "a\nb\n");
    }

    #[test]
    fn inserting_blank_lines() {
        let above = EditOp::InsertBlankLine { above: true };
        let (text, selection) = run("one\ntwo", 5..5, above, "");
        assert_eq!((text.as_str(), selection), ("one\n\ntwo", 4..4));
        let below = EditOp::InsertBlankLine { above: false };
        let (text, selection) = run(&text, 4..4, below, "");
        assert_eq!((text.as_str(), selection), ("one\n\n\ntwo", 5..5));
    }

    #[test]
    fn duplicating_and_moving_lines() {
        let (text, selection) = run("a\nb\nc", 2..2, EditOp::DuplicateLines, "");
        assert_eq!((text.as_str(), selection), ("a\nb\nb\nc", 4..4));
        let (text, selection) = run("a\nb\nc", 2..3, EditOp::MoveLines { up: true }, "");
        assert_eq!((text.as_str(), selection), ("b\na\nc", 0..1));
        let (text, selection) = run("a\nb\nc", 0..0, EditOp::MoveLines { up: false }, "");
        assert_eq!((text.as_str(), selection), ("b\na\nc", 2..2));
        // Nowhere to go.
        assert_eq!(text_after("a\nb", 0..0, EditOp::MoveLines { up: true }, ""), "a\nb");
        assert_eq!(text_after("a\nb", 2..2, EditOp::MoveLines { up: false }, ""), "a\nb");
    }

    #[test]
    fn joining_lines() {
        let (text, selection) = run("a\nb\nc\nd", 0..5, EditOp::JoinLines, "");
        assert_eq!((text.as_str(), selection), ("a b c\nd", 0..5));
        assert_eq!(text_after("a\nb", 1..1, EditOp::JoinLines, ""), "a\nb");
    }

    #[test]
    fn indenting_keeps_the_caret_in_the_text() {
        let (text, selection) = run("abc", 1..1, EditOp::Indent { increase: true }, "");
        assert_eq!((text.as_str(), selection), ("    abc", 5..5));
        let (text, selection) = run(&text, 5..5, EditOp::Indent { increase: false }, "");
        assert_eq!((text.as_str(), selection), ("abc", 1..1));
        // Several lines at once, skipping the empty one.
        let (text, selection) = run("a\n\nb", 0..4, EditOp::Indent { increase: true }, "");
        assert_eq!((text.as_str(), selection), ("    a\n\n    b", 0..12));
    }

    // Case

    #[test]
    fn converting_the_case_of_each_selection() {
        let text = "hello world";
        let plan = plan_edit(text, 0..5, &[0..5, 6..11], EditOp::Case(Case::Upper), settings("")).unwrap().unwrap();
        assert_eq!(plan.edits.len(), 2);
        assert_eq!(plan.selections, vec![0..5, 6..11]);
        let (result, selection) = run(text, 0..5, EditOp::Case(Case::Upper), "");
        assert_eq!((result.as_str(), selection), ("HELLO world", 0..5));
        // Growing text moves the later selections.
        let plan = plan_edit("ß a", 0..2, &[0..2, 3..4], EditOp::Case(Case::Upper), settings("")).unwrap().unwrap();
        assert_eq!(plan.selections, vec![0..2, 3..4]);
    }

    #[test]
    fn converting_case_without_a_selection_does_nothing() {
        assert_eq!(text_after("hello", 0..0, EditOp::Case(Case::Upper), ""), "hello");
    }

    // Comments

    #[test]
    fn toggling_line_comments_keeps_indentation() {
        let source = "int a;\n    int b;\n\nint c;";
        let toggle = EditOp::Comment(CommentMode::Toggle);
        let (commented, selection) = run(source, 0..source.len(), toggle, "cpp");
        assert_eq!(commented, "// int a;\n    // int b;\n\n// int c;");
        assert_eq!(selection, 0..commented.len());
        assert_eq!(text_after(&commented, 0..commented.len(), toggle, "cpp"), source);
    }

    #[test]
    fn the_caret_stays_in_its_text_when_commenting() {
        let (text, selection) = run("foo bar", 4..4, EditOp::Comment(CommentMode::Comment), "cpp");
        assert_eq!((text.as_str(), selection), ("// foo bar", 7..7));
        let (text, selection) = run(&text, 7..7, EditOp::Comment(CommentMode::Uncomment), "cpp");
        assert_eq!((text.as_str(), selection), ("foo bar", 4..4));
    }

    #[test]
    fn uncommenting_without_a_space_and_in_any_case() {
        let uncomment = EditOp::Comment(CommentMode::Uncomment);
        assert_eq!(text_after("//a\n// b", 0..8, uncomment, "cpp"), "a\nb");
        assert_eq!(text_after("rem echo", 0..0, uncomment, "batch"), "echo");
    }

    #[test]
    fn block_only_languages_wrap_lines() {
        let toggle = EditOp::Comment(CommentMode::Toggle);
        let commented = text_after("<p>hi</p>", 0..0, toggle, "html");
        assert_eq!(commented, "<!-- <p>hi</p> -->");
        assert_eq!(text_after(&commented, 0..0, toggle, "html"), "<p>hi</p>");
    }

    #[test]
    fn block_comments_and_their_fallbacks() {
        let (text, selection) = run("int a = 1;", 8..9, EditOp::BlockComment, "cpp");
        assert_eq!((text.as_str(), selection), ("int a = /* 1 */;", 11..12));
        assert_eq!(text_after(&text, 12..12, EditOp::BlockUncomment, "cpp"), "int a = 1;");
        // Without block symbols, the line comment stands in.
        assert_eq!(text_after("x = 1", 0..0, EditOp::BlockComment, "python"), "# x = 1");
        assert_eq!(text_after("# x = 1", 0..0, EditOp::BlockUncomment, "python"), "x = 1");
    }

    #[test]
    fn uncommenting_lines_falls_back_to_a_block_comment() {
        let (text, _) = run("a /* b */ c", 4..4, EditOp::Comment(CommentMode::Uncomment), "cpp");
        assert_eq!(text, "a b c");
    }

    #[test]
    fn languages_without_comments_say_so() {
        let result = plan_edit("a", 0..0, &[0..0], EditOp::Comment(CommentMode::Toggle), settings("json"));
        assert_eq!(result, Err(EditProblem::NoComments));
        let result = plan_edit("a", 0..0, &[0..0], EditOp::BlockComment, settings("json"));
        assert_eq!(result, Err(EditProblem::NoComments));
    }

    // Blank operations

    #[test]
    fn trimming_and_joining() {
        let text = text_after("  a  \n\tb\t\n", 0..0, EditOp::Trim(Trim::Trailing), "");
        assert_eq!(text, "  a\n\tb\n");
        let text = text_after(&text, 0..0, EditOp::Trim(Trim::Leading), "");
        assert_eq!(text, "a\nb\n");
        assert_eq!(text_after(&text, 0..0, EditOp::EolToSpace, ""), "a b ");
        assert_eq!(text_after("  a \n\tb\n", 0..0, EditOp::TrimAndJoin, ""), "a b ");
        // Only the selection.
        assert_eq!(text_after("a\nb\nc", 0..3, EditOp::EolToSpace, ""), "a b\nc");
    }

    #[test]
    fn converting_between_tabs_and_spaces() {
        let text = text_after("\tx\n", 0..0, EditOp::TabsToSpaces, "");
        assert_eq!(text, "    x\n");
        assert_eq!(text_after(&text, 0..0, EditOp::SpacesToTabs { leading_only: true }, ""), "\tx\n");
    }

    #[test]
    fn edits_are_cut_down_to_what_changes() {
        let plan = plan_edit("a\nb\nc\n", 0..0, &[0..0], EditOp::RemoveEmptyLines { including_blank: true }, settings(""))
            .unwrap();
        assert!(plan.is_none());
        let plan = plan_edit("xxxx\nb\na\nxxxx", 0..0, &[0..0], EditOp::Sort(Sort::LexicographicAscending), settings(""))
            .unwrap()
            .unwrap();
        assert_eq!(plan.edits.len(), 1);
        // The sorted text starts and ends the same, so those stay out of the edit.
        assert!(plan.edits[0].range.len() < "xxxx\nb\na\nxxxx".len());
    }
}
