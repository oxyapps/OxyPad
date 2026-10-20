//! Syntax highlighting and folding in the editor, with the Lexilla lexers of
//! `oxypad-syntax`.

use std::{cell::RefCell, ops::Range, path::Path, rc::Rc};

use gpui_kit::{
    component::{
        ActiveTheme,
        highlighter::HighlightTheme,
        input::{EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, InputHighlighterFactory, Rope},
    },
    *,
};
use oxypad_core::language_for_path;
use oxypad_syntax::{Edit, Highlighter, Text};
use ropey::LineType;

/// Documents up to this size are lexed whole after every change, which
/// takes at most some 70 ms and gives every fold. Larger ones are lexed
/// only as far as they have been shown.
const LEX_WHOLE_LIMIT: usize = 2 * 1024 * 1024;

/// The language of a file, by its name; "plaintext" without a path.
pub fn detect(path: Option<&Path>) -> SharedString {
    path.map_or("plaintext", language_for_path).into()
}

/// Creates the editor's highlighters. Install it on every editor.
pub fn factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        let highlighter = Highlighter::new(language)?;
        Some(Box::new(EditorHighlighter {
            language: language.to_owned().into(),
            state: RefCell::new(State { highlighter, text: Rope::new() }),
            folds: Vec::new(),
            _apply_folds: Task::ready(()),
        }) as Box<dyn InputHighlighter>)
    })
}

/// The editor's text as the lexers read it.
struct RopeText<'a>(&'a Rope);

impl Text for RopeText<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn read(&self, mut position: usize, buffer: &mut [u8]) {
        let mut filled = 0;
        while filled < buffer.len() && position < self.0.len() {
            let (chunk, chunk_start) = self.0.chunk(position);
            let bytes = &chunk.as_bytes()[position - chunk_start..];
            let count = bytes.len().min(buffer.len() - filled);
            if count == 0 {
                break;
            }
            buffer[filled..filled + count].copy_from_slice(&bytes[..count]);
            filled += count;
            position += count;
        }
    }

    fn line_of(&self, position: usize) -> usize {
        self.0.byte_to_line_idx(position.min(self.0.len()), LineType::LF)
    }

    fn line_start(&self, line: usize) -> usize {
        if line >= self.line_count() { self.0.len() } else { self.0.line_to_byte_idx(line, LineType::LF) }
    }

    fn line_count(&self) -> usize {
        self.0.len_lines(LineType::LF)
    }
}

struct State {
    highlighter: Highlighter,
    /// The text as of the last update. Cloning a rope shares its storage.
    text: Rope,
}

struct EditorHighlighter {
    language: SharedString,
    /// Styling the visible lines may lex further, from `styles(&self)`.
    state: RefCell<State>,
    /// The folds the editor was last given.
    folds: Vec<FoldRange>,
    _apply_folds: Task<()>,
}

impl InputHighlighter for EditorHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        let state = self.state.get_mut();
        let rope = RopeText(text);
        match edit {
            Some(edit) => state.highlighter.edit(
                &rope,
                Edit {
                    start: edit.start_byte,
                    old_end: edit.old_end_byte,
                    new_end: edit.new_end_byte,
                    start_line: edit.start_position.row,
                    old_end_line: edit.old_end_position.row,
                    new_end_line: edit.new_end_position.row,
                },
            ),
            None => state.highlighter.reset(&rope),
        }
        state.text = text.clone();
        if text.len() <= LEX_WHOLE_LIMIT {
            state.highlighter.lex_to(&rope, text.len());
        }

        let folds: Vec<FoldRange> = if folding {
            state.highlighter.folds(&rope).into_iter().map(|lines| FoldRange::new(lines.start, lines.end)).collect()
        } else {
            Vec::new()
        };
        if folds != self.folds {
            self.folds = folds.clone();
            // The editor merges in only the folds that start on edited lines,
            // but an edit can change folds anywhere after it (an opened
            // brace, say): hand it all of them, once this update is over.
            self._apply_folds = cx.spawn_in(window, async move |editor, cx| {
                let _ = editor.update(cx, |editor, cx| editor.apply_highlighter_fold_candidates(folds, cx));
            });
        }
    }

    fn styles(&self, range: &Range<usize>, resolver: &dyn HighlightStyleResolver) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut state = self.state.borrow_mut();
        let State { highlighter, text } = &mut *state;
        let rope = RopeText(text);
        if highlighter.styled_end() < range.end {
            highlighter.lex_to(&rope, range.end);
        }
        highlighter
            .styles(&rope, range.clone())
            .into_iter()
            .map(|(range, name)| (range, name.and_then(|name| resolver.style(name)).unwrap_or_default()))
            .collect()
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.folds.clone()
    }
}

/// Highlights `text` whole, for views other than the editor. Returns the
/// style runs of the highlighted parts, in order.
pub fn highlight(text: &str, language: &str, cx: &App) -> Vec<(Range<usize>, HighlightStyle)> {
    highlight_with(text, language, &cx.theme().highlight_theme)
}

/// [`highlight`] with the colors of `theme` (printing uses the light one).
pub fn highlight_with(text: &str, language: &str, theme: &HighlightTheme) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(mut highlighter) = Highlighter::new(language) else { return Vec::new() };
    let source = oxypad_syntax::StrText::new(text);
    highlighter.reset(&source);
    highlighter.lex_to(&source, text.len());
    highlighter
        .styles(&source, 0..text.len())
        .into_iter()
        .filter_map(|(range, name)| Some((range, theme.style(name?)?)))
        .collect()
}
