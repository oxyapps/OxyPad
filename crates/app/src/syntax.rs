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

/// The languages a tab can be switched to from the Language menu, by
/// display name; "plaintext" (no highlighting) is not among them.
pub fn languages() -> impl Iterator<Item = (&'static str, &'static str)> {
    oxypad_syntax::LANGUAGES.iter().map(|language| (language.name, label(language.name)))
}

/// How many languages must share a first letter before the Language menus
/// gather them in a submenu, as Notepad++ does.
const GROUP_FROM: usize = 3;

/// An entry of the Language menus: a language, or a letter's submenu.
pub enum LanguageEntry {
    /// The language's name and label.
    One(&'static str, &'static str),
    /// The letter and its languages, by label.
    Letter(char, Vec<(&'static str, &'static str)>),
}

/// The languages in alphabetical order of their labels, with the ones that
/// share a first letter with two others or more gathered in a submenu.
pub fn language_menu() -> Vec<LanguageEntry> {
    let mut languages: Vec<_> = languages().collect();
    languages.sort_by_key(|(_, label)| label.to_lowercase());
    let mut entries = Vec::new();
    for chunk in languages.chunk_by(|(_, a), (_, b)| first_letter(a) == first_letter(b)) {
        if chunk.len() >= GROUP_FROM {
            entries.push(LanguageEntry::Letter(first_letter(chunk[0].1), chunk.to_vec()));
        } else {
            entries.extend(chunk.iter().map(|&(name, label)| LanguageEntry::One(name, label)));
        }
    }
    entries
}

fn first_letter(label: &str) -> char {
    label.chars().next().unwrap_or_default().to_ascii_uppercase()
}

/// A language's name as menus and the status bar show it.
pub fn label(name: &str) -> &str {
    match name {
        "astro" => "Astro",
        "bash" => "Shell (Bash)",
        "batch" => "Batch",
        "c" => "C",
        "cmake" => "CMake",
        "cpp" => "C++",
        "csharp" => "C#",
        "css" => "CSS",
        "diff" => "Diff",
        "ejs" => "EJS",
        "elixir" => "Elixir",
        "erb" => "ERB",
        "go" => "Go",
        "graphql" => "GraphQL",
        "html" => "HTML",
        "ini" => "INI",
        "java" => "Java",
        "javascript" => "JavaScript",
        "json" => "JSON",
        "kotlin" => "Kotlin",
        "lua" => "Lua",
        "make" => "Makefile",
        "markdown" => "Markdown",
        "perl" => "Perl",
        "php" => "PHP",
        "powershell" => "PowerShell",
        "proto" => "Protocol Buffers",
        "python" => "Python",
        "ruby" => "Ruby",
        "rust" => "Rust",
        "scala" => "Scala",
        "sql" => "SQL",
        "svelte" => "Svelte",
        "swift" => "Swift",
        "toml" => "TOML",
        "tsx" => "TSX",
        "typescript" => "TypeScript",
        "xml" => "XML",
        "yaml" => "YAML",
        "zig" => "Zig",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{LanguageEntry, language_menu};

    #[test]
    fn the_language_menu_gathers_letters_with_three_languages_or_more() {
        let entries = language_menu();
        let letter = |wanted: char| {
            entries.iter().find_map(|entry| match entry {
                LanguageEntry::Letter(letter, languages) if *letter == wanted => {
                    Some(languages.iter().map(|(_, label)| *label).collect::<Vec<_>>())
                }
                _ => None,
            })
        };
        assert_eq!(letter('J'), Some(vec!["Java", "JavaScript", "JSON"]));
        // Two languages are not worth a submenu.
        assert_eq!(letter('G'), None);
        assert!(entries.iter().any(|entry| matches!(entry, LanguageEntry::One("go", "Go"))));
        // Every language is in the menu once.
        let count: usize = entries
            .iter()
            .map(|entry| match entry {
                LanguageEntry::One(..) => 1,
                LanguageEntry::Letter(_, languages) => languages.len(),
            })
            .sum();
        assert_eq!(count, oxypad_syntax::LANGUAGES.len());
    }
}
