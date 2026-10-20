use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui_kit::{
    component::input::{Editor, EditorState, InputEvent, Position, TabSize},
    *,
};
use oxypad_core::{Document, ExternalChange};
use oxypad_session::{Preferences, TabKind, TabSnapshot};

use crate::{languages, preferences};

fn tab_size(preferences: &Preferences) -> TabSize {
    TabSize { tab_size: preferences.tab_width as usize, hard_tabs: !preferences.insert_spaces }
}

/// A tab holding an editable text document.
pub struct EditorTab {
    id: u64,
    /// Name of an untitled document ("new 1"); unused once it has a path.
    untitled_name: SharedString,
    document: Document,
    editor: Entity<EditorState>,
    dirty: bool,
    soft_wrap: bool,
    /// The cursor the status bar last showed.
    cursor: Position,
    _subscriptions: Vec<Subscription>,
}

impl EditorTab {
    pub fn new(
        id: u64,
        untitled_name: SharedString,
        document: Document,
        text: String,
        dirty: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let language = languages::detect(document.path(), cx);
        let preferences = preferences::get(cx).clone();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(preferences.show_line_numbers)
                .indent_guides(preferences.show_indent_guides)
                .folding(true)
                .soft_wrap(preferences.wrap_lines)
                .tab_size(tab_size(&preferences))
                .default_value(text)
        });

        // The workspace re-renders and marks the session changed whenever
        // this tab notifies, so notify only for edits and cursor moves.
        let _subscriptions = vec![
            // Every edit changes the unsaved text the session keeps.
            cx.subscribe(&editor, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.dirty = true;
                    cx.notify();
                }
            }),
            // Cursor moves change the status bar. The editor also notifies on
            // every cursor blink, which changes nothing here.
            cx.observe(&editor, |this, editor, cx| {
                let cursor = editor.read(cx).cursor_position();
                if cursor != this.cursor {
                    this.cursor = cursor;
                    cx.notify();
                }
            }),
        ];

        Self {
            id,
            untitled_name,
            document,
            editor,
            dirty,
            soft_wrap: preferences.wrap_lines,
            cursor: Position::default(),
            _subscriptions,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn path(&self) -> Option<&Path> {
        self.document.path()
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// An untitled tab nobody has typed into yet; opening a file replaces it.
    pub fn is_pristine(&self, cx: &App) -> bool {
        self.document.path().is_none() && !self.dirty && self.editor.read(cx).text().len() == 0
    }

    pub fn title(&self) -> SharedString {
        match self.document.file_name() {
            Some(name) => name.into(),
            // Stored as "new N" (in the session too); shown in the UI language.
            None => match self.untitled_name.strip_prefix("new ") {
                Some(number) => t!("tabs.untitled", number = number).into(),
                None => self.untitled_name.clone(),
            },
        }
    }

    pub fn language(&self, cx: &App) -> SharedString {
        self.editor.read(cx).language_name()
    }

    pub fn cursor(&self, cx: &App) -> Position {
        self.editor.read(cx).cursor_position()
    }

    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    /// Applies changed preferences. Word wrap is left alone: the preference
    /// only sets it for new tabs.
    pub fn apply_preferences(&mut self, preferences: &Preferences, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |state, cx| {
            state.set_line_number(preferences.show_line_numbers, window, cx);
            state.set_indent_guides(preferences.show_indent_guides, window, cx);
            state.set_tab_size(tab_size(preferences), cx);
        });
    }

    /// Re-detects the language and rebuilds the highlighter, after a Save As
    /// or when a language from the language pack was installed.
    pub fn refresh_language(&mut self, cx: &mut Context<Self>) {
        let language = languages::detect(self.document.path(), cx);
        self.editor.update(cx, |state, cx| state.set_highlighter(language, cx));
        // The status bar shows the language.
        cx.notify();
    }

    pub fn toggle_soft_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.soft_wrap = !self.soft_wrap;
        let soft_wrap = self.soft_wrap;
        self.editor.update(cx, |state, cx| state.set_soft_wrap(soft_wrap, window, cx));
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn set_cursor(&self, (line, column): (u32, u32), window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |state, cx| {
            state.set_cursor_position(Position::new(line, column), window, cx);
        });
    }

    /// Opens the editor's find bar, with the replace field when `replace`.
    pub fn open_search(&self, replace: bool, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |state, cx| {
            state.focus(window, cx);
            state.open_search(replace, cx);
        });
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).text().to_string()
    }

    /// Saves to the current path. Fails for untitled documents.
    pub fn save(&mut self, cx: &mut Context<Self>) -> Result<()> {
        self.document.save(&self.text(cx))?;
        self.dirty = false;
        cx.notify();
        Ok(())
    }

    /// How the file changed on disk since it was opened or last saved.
    pub fn external_change(&self) -> Option<ExternalChange> {
        self.document.external_change()
    }

    /// Replaces the text with the file on disk, dropping unsaved changes and
    /// the undo history. The cursor stays on the same line when it can.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        let path = self.document.path().map(Path::to_path_buf).context("the document has no file path")?;
        let (document, text) = Document::open(&path)?;
        let cursor = self.cursor(cx);
        self.document = document;
        self.editor.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.set_cursor_position(cursor, window, cx);
        });
        self.dirty = false;
        cx.notify();
        Ok(())
    }

    /// Keeps the text in the editor over the version on disk. It now
    /// differs from the file, so the tab counts as modified and saving
    /// overwrites the other program's changes.
    pub fn keep_version(&mut self, cx: &mut Context<Self>) {
        self.document.accept_disk_version();
        self.dirty = true;
        cx.notify();
    }

    /// Forgets the unsaved changes without touching the text, for a tab
    /// about to be replaced (by a log view of the same file).
    pub fn discard_changes(&mut self, cx: &mut Context<Self>) {
        self.dirty = false;
        cx.notify();
    }

    pub fn save_as(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Result<()> {
        let text = self.text(cx);
        self.document.save_as(&path, &text)?;
        self.dirty = false;
        self.refresh_language(cx);
        cx.notify();
        Ok(())
    }

    pub fn snapshot(&self, cx: &App) -> TabSnapshot {
        let cursor = self.cursor(cx);
        TabSnapshot {
            id: self.id,
            kind: TabKind::Edit,
            path: self.document.path().map(Path::to_path_buf),
            title: self.untitled_name.to_string(),
            cursor: (cursor.line, cursor.character),
            follow: false,
            unsaved_text: (self.dirty || self.document.path().is_none()).then(|| self.text(cx)),
        }
    }
}

impl Render for EditorTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Editor::new(&self.editor).bordered(false).h_full()
    }
}
