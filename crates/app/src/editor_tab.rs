use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result};
use gpui_kit::{
    base::input::{Diagnostic, DiagnosticSeverity},
    component::input::{Editor, EditorState, InputEvent, Position, TabSize},
    *,
};
use oxypad_core::{Document, ExternalChange};
use oxypad_lint::Problem;
use ropey::LineType;
use oxypad_session::{Preferences, TabKind, TabSnapshot};

use crate::{preferences, syntax};

/// How long typing must pause before the syntax is checked again.
const CHECK_DELAY: Duration = Duration::from_millis(400);
/// Larger files are not checked: parsing them on every pause would cost
/// more than it helps.
const CHECK_LIMIT: usize = 4 * 1024 * 1024;

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
    /// Editing is locked; the text can still be saved, searched and copied.
    read_only: bool,
    /// The cursor the status bar last showed.
    cursor: Position,
    /// The syntax error found by the last check (JSON, YAML, TOML, XML).
    problem: Option<Problem>,
    /// Counts edits, so a check of older text is not shown.
    edits: u64,
    _check: Task<()>,
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
        let language = syntax::detect(document.path());
        let preferences = preferences::get(cx).clone();
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx);
            state.set_highlighter_factory(syntax::factory(), cx);
            state
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
                    this.edits += 1;
                    this.check_syntax(true, cx);
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

        let mut this = Self {
            id,
            untitled_name,
            document,
            editor,
            dirty,
            soft_wrap: preferences.wrap_lines,
            read_only: false,
            cursor: Position::default(),
            problem: None,
            edits: 0,
            _check: Task::ready(()),
            _subscriptions,
        };
        this.check_syntax(false, cx);
        this
    }

    /// The syntax error in a JSON, YAML, TOML or XML file, if any.
    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    /// Checks the syntax in the background, after a pause in typing when
    /// `wait`, and marks the error in the editor.
    fn check_syntax(&mut self, wait: bool, cx: &mut Context<Self>) {
        let language = syntax::detect(self.document.path());
        let text = self.editor.read(cx).text().clone();
        if !oxypad_lint::is_supported(&language) || text.len() > CHECK_LIMIT {
            self._check = Task::ready(());
            self.show_problem(None, cx);
            return;
        }
        let path = self.document.path().map(Path::to_path_buf);
        let edits = self.edits;
        self._check = cx.spawn(async move |this, cx| {
            if wait {
                cx.background_executor().timer(CHECK_DELAY).await;
            }
            let problem =
                cx.background_spawn(async move { oxypad_lint::check(&language, path.as_deref(), &text.to_string()) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.edits == edits {
                    this.show_problem(problem, cx);
                }
            });
        });
    }

    fn show_problem(&mut self, problem: Option<Problem>, cx: &mut Context<Self>) {
        if problem == self.problem && problem.is_none() {
            return;
        }
        self.editor.update(cx, |state, cx| {
            let line_length = |line: usize| {
                let text = state.text();
                if line >= text.len_lines(LineType::LF) {
                    return 0;
                }
                text.line(line, LineType::LF).chars().filter(|ch| *ch != '\n' && *ch != '\r').count()
            };
            let diagnostic = problem.as_ref().map(|problem| {
                // One character wide, so an error at the end of a line shows too.
                let length = line_length(problem.line);
                let start = problem.column.min(length.saturating_sub(1));
                let range = Position::new(problem.line as u32, start as u32)
                    ..Position::new(problem.line as u32, (start + 1).min(length.max(1)) as u32);
                Diagnostic::new(range, problem.message.clone()).with_severity(DiagnosticSeverity::Error)
            });
            if let Some(diagnostics) = state.diagnostics_mut() {
                diagnostics.clear();
                diagnostics.extend(diagnostic);
            }
            cx.notify();
        });
        self.problem = problem;
        cx.notify();
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

    /// An untitled tab nobody has typed into or renamed yet; opening a file
    /// replaces it.
    pub fn is_pristine(&self, cx: &App) -> bool {
        self.document.path().is_none()
            && !self.dirty
            && self.untitled_name.starts_with("new ")
            && self.editor.read(cx).text().len() == 0
    }

    pub fn is_untitled(&self) -> bool {
        self.document.path().is_none()
    }

    /// N for an untitled tab named "new N".
    pub fn untitled_number(&self) -> Option<usize> {
        if !self.is_untitled() {
            return None;
        }
        self.untitled_name.strip_prefix("new ")?.parse().ok()
    }

    /// Renames an untitled tab; tabs with a file are renamed by Save As.
    pub fn rename(&mut self, name: SharedString, cx: &mut Context<Self>) {
        if self.is_untitled() {
            self.untitled_name = name;
            cx.notify();
        }
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.notify();
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

    /// Re-detects the language and rebuilds the highlighter, after a Save As.
    pub fn refresh_language(&mut self, cx: &mut Context<Self>) {
        let language = syntax::detect(self.document.path());
        self.editor.update(cx, |state, cx| state.set_highlighter(language, cx));
        self.check_syntax(false, cx);
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

    /// Keeps the text in the editor over the version another program wrote.
    /// It differs from the file, so the tab counts as modified; OxyPad stops
    /// asking about the file, and saving asks before overwriting it.
    pub fn keep_version(&mut self, cx: &mut Context<Self>) {
        self.document.keep_over_disk();
        self.dirty = true;
        cx.notify();
    }

    /// Keeps the tab of a file that was deleted, as unsaved text; saving
    /// recreates the file.
    pub fn keep_deleted(&mut self, cx: &mut Context<Self>) {
        self.document.accept_disk_version();
        self.dirty = true;
        cx.notify();
    }

    /// Whether saving would overwrite another program's changes (see `keep_version`).
    pub fn overwrites_disk_changes(&self) -> bool {
        self.document.overwrites_disk_changes()
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
            read_only: self.read_only,
            kept_over_disk: self.document.overwrites_disk_changes(),
            unsaved_text: (self.dirty || self.document.path().is_none()).then(|| self.text(cx)),
            // Pinning and the color belong to the workspace, which fills them in.
            ..Default::default()
        }
    }
}

impl Render for EditorTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Editor::new(&self.editor).bordered(false).readonly(self.read_only).h_full()
    }
}
