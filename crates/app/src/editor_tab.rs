use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result};
use gpui_kit::{
    base::input::{Diagnostic, DiagnosticSeverity},
    component::{
        ActiveTheme,
        input::{Editor, EditorState, InputEvent, Position, TabSize},
        text::TextView,
    },
    prelude::FluentBuilder,
    *,
};
use oxypad_core::{Document, EditOp, EditProblem, EditSettings, ExternalChange, comment_symbols, plan_edit};
use oxypad_lint::Problem;
use ropey::LineType;
use oxypad_session::{Preferences, TabKind, TabSnapshot};

use crate::{preferences, syntax};

/// How long typing must pause before the syntax is checked again.
const CHECK_DELAY: Duration = Duration::from_millis(400);
/// How far a tab's text is zoomed from the font size, in points, as
/// Scintilla's zoom (and MacPad++) allow.
const ZOOM_RANGE: std::ops::RangeInclusive<i32> = -10..=20;
/// The smallest text size a zoom may reach.
const MIN_TEXT_SIZE: f32 = 4.0;
/// A mouse wheel notch is about one line; a trackpad sends many small pixel deltas.
const WHEEL_PIXELS_PER_STEP: f32 = 20.0;
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
    /// Points added to the font size by zooming; only this tab is zoomed.
    zoom: i32,
    /// Wheel and pinch movement not yet worth a whole point of zoom.
    zoom_remainder: f32,
    /// The cursor the status bar last showed.
    cursor: Position,
    /// The language picked from the Language menu, over the one the file
    /// name gives; kept through Save As unless the new name gives one.
    language: Option<SharedString>,
    /// The syntax error found by the last check (JSON, YAML, TOML, XML).
    problem: Option<Problem>,
    /// Counts edits, so a check of older text is not shown.
    edits: u64,
    /// The Markdown source shown rendered, in place of the editor, while the
    /// preview is on.
    preview: Option<SharedString>,
    /// Holds the focus while the preview is shown, so the shortcuts keep working.
    preview_focus: FocusHandle,
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
                    // The Edit menu and reloading change the text under the preview.
                    if this.preview.is_some() {
                        this.preview = Some(this.text(cx).into());
                    }
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
            zoom: 0,
            zoom_remainder: 0.0,
            cursor: Position::default(),
            language: None,
            problem: None,
            edits: 0,
            preview: None,
            preview_focus: cx.focus_handle(),
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
        let language = self.chosen_language();
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

    /// The text size relative to the font size setting: 13 pt zoomed by +2 is 115%.
    pub fn zoom_percent(&self, cx: &App) -> u32 {
        let size = f32::from(cx.theme().mono_font_size).max(1.0);
        (self.text_size(cx) / size * 100.0).round() as u32
    }

    fn text_size(&self, cx: &App) -> f32 {
        (f32::from(cx.theme().mono_font_size) + self.zoom as f32).max(MIN_TEXT_SIZE)
    }

    /// Restores a zoom saved in the session.
    pub fn set_zoom(&mut self, zoom: i32, cx: &mut Context<Self>) {
        self.zoom_by(zoom - self.zoom, cx);
    }

    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.zoom_by(1, cx);
    }

    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.zoom_by(-1, cx);
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.zoom_by(-self.zoom, cx);
    }

    fn zoom_by(&mut self, points: i32, cx: &mut Context<Self>) {
        let zoom = (self.zoom + points).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        if zoom != self.zoom {
            self.zoom = zoom;
            cx.notify();
        }
    }

    /// Zooms by the scroll wheel (with Cmd, or Ctrl elsewhere), upward zooming in.
    fn zoom_by_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let steps = match event.delta {
            ScrollDelta::Lines(lines) => lines.y,
            ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / WHEEL_PIXELS_PER_STEP,
        };
        self.zoom_by_fraction(steps, cx);
    }

    /// Zooms by a pinch on the trackpad: 0.1 is a 10% growth of the text.
    fn zoom_by_pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        let size = f32::from(cx.theme().mono_font_size);
        self.zoom_by_fraction(event.delta * size, cx);
    }

    fn zoom_by_fraction(&mut self, points: f32, cx: &mut Context<Self>) {
        self.zoom_remainder += points;
        let whole = self.zoom_remainder.trunc();
        self.zoom_remainder -= whole;
        self.zoom_by(whole as i32, cx);
    }

    /// Runs an Edit menu operation on the text: one undo step, acting on the
    /// selected lines or, with no selection, on the whole document.
    pub fn edit(&mut self, op: EditOp, window: &mut Window, cx: &mut Context<Self>) -> Result<(), EditProblem> {
        let editor = self.editor.read(cx);
        let text = editor.text().to_string();
        let (active, all) = (editor.selected_range(), editor.selection_ranges());
        let whole_document = active.is_empty();
        let preferences = preferences::get(cx);
        let settings = EditSettings {
            tab_width: preferences.tab_width as usize,
            hard_tabs: !preferences.insert_spaces,
            comments: comment_symbols(&editor.language_name()),
        };
        let Some(plan) = plan_edit(&text, active, &all, op, settings)? else { return Ok(()) };
        let edits: Vec<_> = plan.edits.into_iter().map(|edit| (edit.range, edit.text)).collect();
        self.editor.update(cx, |state, cx| {
            state.edit_ranges(&edits, &plan.selections, window, cx);
        });
        self.focus(window, cx);
        // Text just formatted as JSON is JSON: highlight and check it so.
        if matches!(op, EditOp::Json { .. }) && whole_document && self.chosen_language() == "plaintext" {
            self.set_language(Some("json".into()), cx);
        }
        Ok(())
    }

    /// The characters of the selection and the number of selections, for the status bar.
    pub fn selection(&self, cx: &App) -> (usize, usize) {
        let editor = self.editor.read(cx);
        (editor.selected_char_count(), editor.selection_count())
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

    /// The picked language, or else the one the file name gives.
    fn chosen_language(&self) -> SharedString {
        self.language.clone().unwrap_or_else(|| syntax::detect(self.document.path()))
    }

    /// Highlights and checks the text as `language` ("plaintext" for none),
    /// whatever the file name says; `None` goes back to the file name's.
    pub fn set_language(&mut self, language: Option<SharedString>, cx: &mut Context<Self>) {
        self.language = language.filter(|language| *language != syntax::detect(self.document.path()));
        self.refresh_language(cx);
    }

    /// Rebuilds the highlighter for the chosen language and checks the syntax again.
    fn refresh_language(&mut self, cx: &mut Context<Self>) {
        let language = self.chosen_language();
        if language != "markdown" {
            self.preview = None;
        }
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
        match self.preview {
            Some(_) => self.preview_focus.focus(window, cx),
            None => self.editor.update(cx, |state, cx| state.focus(window, cx)),
        }
    }

    /// Whether the text is Markdown, which can be shown rendered.
    pub fn is_markdown(&self) -> bool {
        self.chosen_language() == "markdown"
    }

    /// Whether the rendered Markdown is shown in place of the editor.
    pub fn is_previewing(&self) -> bool {
        self.preview.is_some()
    }

    /// Shows the Markdown rendered, or goes back to its source. Only
    /// Markdown has a preview.
    pub fn set_preview(&mut self, preview: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = (preview && self.is_markdown()).then(|| self.text(cx).into());
        self.focus(window, cx);
        cx.notify();
    }

    pub fn set_cursor(&self, (line, column): (u32, u32), window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |state, cx| {
            state.set_cursor_position(Position::new(line, column), window, cx);
        });
    }

    /// Opens the editor's find bar, with the replace field when `replace`.
    /// The find bar belongs to the editor, so a preview gives way to it.
    pub fn open_search(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = None;
        cx.notify();
        self.editor.update(cx, |state, cx| {
            state.focus(window, cx);
            state.open_search(replace, cx);
        });
    }

    /// Puts `text` in place of the selection, or at the cursor. Does nothing
    /// to a read-only tab.
    pub fn insert(&self, text: &str, window: &mut Window, cx: &mut App) {
        if self.read_only {
            return;
        }
        let text = SharedString::from(text.to_owned());
        self.editor.update(cx, |state, cx| state.replace(text, window, cx));
        self.focus(window, cx);
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
        // A name with a known extension decides; otherwise the picked language stays.
        if syntax::detect(Some(&path)) != "plaintext" {
            self.language = None;
        }
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
            zoom: self.zoom,
            kept_over_disk: self.document.overwrites_disk_changes(),
            language: self.language.as_ref().map(ToString::to_string),
            unsaved_text: (self.dirty || self.document.path().is_none()).then(|| self.text(cx)),
            // Pinning and the color belong to the workspace, which fills them in.
            ..Default::default()
        }
    }
}

impl Render for EditorTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = px(self.text_size(cx));
        div()
            .size_full()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                if event.modifiers.secondary() {
                    this.zoom_by_wheel(event, cx);
                }
            }))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| this.zoom_by_pinch(event, cx)))
            .map(|this| match &self.preview {
                Some(markdown) => this.track_focus(&self.preview_focus).bg(cx.theme().background).child(
                    TextView::markdown(("markdown-preview", self.id), markdown.clone())
                        .scrollable(true)
                        .selectable(true)
                        .px_6()
                        .py_4()
                        .text_size(size),
                ),
                None => this
                    .child(Editor::new(&self.editor).bordered(false).readonly(self.read_only).h_full().text_size(size)),
            })
    }
}
