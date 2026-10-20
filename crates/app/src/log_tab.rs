use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui_kit::{
    component::{
        ActiveTheme,
        input::{Editor, EditorState, RopeExt, TextDecoration, TextDecorationCollection},
    },
    *,
};
use oxypad_session::{Preferences, TabKind, TabSnapshot};
use oxypad_tail::{LogFollower, LogLevel, ResetReason, TailEvent, level_spans};

use crate::preferences;

const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// What the status bar shows for a log tab.
#[derive(Clone, Debug, PartialEq)]
pub enum LogStatus {
    Following,
    /// Paused because the user scrolled away from the end.
    Paused,
    Reset(ResetReason),
    Error(SharedString),
}

/// A read-only tab that follows a log file like `tail -f`.
pub struct LogTab {
    id: u64,
    path: PathBuf,
    editor: Entity<EditorState>,
    follow: bool,
    status: LogStatus,
    levels: TextDecorationCollection,
    _poll: Task<()>,
}

impl LogTab {
    /// Opens `path` and starts following it. Fails when the file cannot be
    /// read, before any tab is created.
    pub fn open(
        id: u64,
        path: PathBuf,
        follow: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> std::io::Result<Entity<Self>> {
        let initial_bytes = u64::from(preferences::get(cx).log_initial_kib) * 1024;
        let (follower, text) = LogFollower::open(&path, initial_bytes)?;
        Ok(cx.new(|cx| Self::new(id, path, follower, text, follow, window, cx)))
    }

    fn new(
        id: u64,
        path: PathBuf,
        follower: LogFollower,
        text: String,
        follow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("plaintext")
                .line_number(preferences::get(cx).show_line_numbers)
                .folding(false)
                .soft_wrap(false)
                // The newest line sits at the bottom edge while following.
                .scroll_beyond_last_line(Some(0))
                .default_value(text.clone())
        });
        let levels = editor.update(cx, |state, cx| {
            state.create_decorations_collection(level_decorations(&text, 0, cx), cx)
        });
        // No observer on the editor: it notifies on every cursor blink, and
        // what the status bar shows (line count, follow state) changes only
        // through this tab, which notifies itself.

        let mut this = Self {
            id,
            path,
            editor,
            follow,
            status: if follow { LogStatus::Following } else { LogStatus::Paused },
            levels,
            _poll: Task::ready(()),
        };
        this._poll = Self::spawn_poll(follower, window, cx);
        if follow {
            this.scroll_to_end(cx);
        }
        this
    }

    /// Polls the file on the background executor and applies changes on the
    /// UI thread. Ends when the tab is closed.
    fn spawn_poll(follower: LogFollower, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        let follower = Arc::new(Mutex::new(follower));
        cx.spawn_in(window, async move |this, cx| {
            let mut caught_up = true;
            loop {
                if caught_up {
                    cx.background_executor().timer(POLL_INTERVAL).await;
                }
                let follower = follower.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { follower.lock().unwrap().poll() })
                    .await;
                let applied = this.update_in(cx, |this, window, cx| match result {
                    Ok(Some(event)) => this.apply(event, window, cx),
                    Ok(None) => true,
                    Err(err) => {
                        this.status = LogStatus::Error(err.to_string().into());
                        cx.notify();
                        true
                    }
                });
                match applied {
                    Ok(done) => caught_up = done,
                    Err(_) => break,
                }
            }
        })
    }

    /// Applies one change from the follower. Returns whether the follower
    /// has caught up with the file.
    fn apply(&mut self, event: TailEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match event {
            TailEvent::Appended { text, caught_up } => {
                self.update_follow_from_scroll(cx);
                self.append(&text, window, cx);
                caught_up
            }
            TailEvent::Reset { text, reason } => {
                self.editor.update(cx, |state, cx| state.set_value(text.clone(), window, cx));
                self.levels.set(level_decorations(&text, 0, cx), cx);
                self.status = LogStatus::Reset(reason);
                if self.follow {
                    self.scroll_to_end(cx);
                }
                cx.notify();
                true
            }
        }
    }

    fn append(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let base = self.editor.read(cx).text().len();
        let follow = self.follow;
        self.editor.update(cx, |state, cx| {
            let selection = state.selected_range();
            let scroll = state.scroll_offset();
            state.set_selected_range(base..base, cx);
            state.insert(text.to_owned(), window, cx);
            if !follow {
                state.set_selected_range(selection, cx);
                state.set_scroll_offset(scroll, cx);
            }
        });
        self.levels.append(level_decorations(text, base, cx), cx);
        self.trim(cx, window);
        if follow {
            self.status = LogStatus::Following;
            self.scroll_to_end(cx);
        }
        cx.notify();
    }

    /// Drops the oldest lines once the buffer is well over the
    /// "lines kept in memory" preference.
    fn trim(&mut self, cx: &mut Context<Self>, window: &mut Window) {
        let max_lines = preferences::get(cx).log_max_lines as usize;
        let lines = self.editor.read(cx).text().lines_len();
        if lines <= max_lines + max_lines / 10 {
            return;
        }
        let cut = self.editor.read(cx).text().line_start_offset(lines - max_lines);
        self.editor.update(cx, |state, cx| {
            let selection = state.selected_range();
            state.set_selected_range(0..cut, cx);
            state.replace("", window, cx);
            let start = selection.start.saturating_sub(cut);
            let end = selection.end.saturating_sub(cut);
            state.set_selected_range(start..end, cx);
        });
        let text = self.editor.read(cx).text().to_string();
        self.levels.set(level_decorations(&text, 0, cx), cx);
    }

    /// Pauses following when the user scrolled away from the end, and
    /// resumes an automatic pause once they scroll back down.
    fn update_follow_from_scroll(&mut self, cx: &mut Context<Self>) {
        let state = self.editor.read(cx);
        let Some(visible) = state.visible_row_range() else { return };
        let at_end = visible.end + 1 >= state.text().lines_len();
        if self.follow && !at_end {
            self.follow = false;
            self.status = LogStatus::Paused;
        } else if !self.follow && at_end {
            self.follow = true;
            self.status = LogStatus::Following;
        }
    }

    fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        self.editor.update(cx, |state, cx| {
            let end = state.text().len();
            state.set_selected_range(end..end, cx);
            // Clamped to the last line on the next layout.
            state.set_scroll_offset(point(px(0.), px(f32::MIN / 2.)), cx);
        });
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn title(&self) -> SharedString {
        self.path
            .file_name()
            .map_or_else(|| self.path.to_string_lossy(), |name| name.to_string_lossy())
            .into_owned()
            .into()
    }

    pub fn status(&self) -> &LogStatus {
        &self.status
    }

    pub fn line_count(&self, cx: &App) -> usize {
        self.editor.read(cx).text().lines_len()
    }

    /// Applies changed preferences: line numbers and level colors.
    pub fn apply_preferences(&mut self, preferences: &Preferences, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |state, cx| state.set_line_number(preferences.show_line_numbers, window, cx));
        let text = self.editor.read(cx).text().to_string();
        self.levels.set(level_decorations(&text, 0, cx), cx);
    }

    /// Opens the find bar. Logs are read-only, so there is no replace.
    pub fn open_search(&self, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |state, cx| {
            state.focus(window, cx);
            state.open_search(false, cx);
        });
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).text().to_string()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn snapshot(&self) -> TabSnapshot {
        TabSnapshot {
            id: self.id,
            kind: TabKind::Tail,
            path: Some(self.path.clone()),
            follow: true,
            ..Default::default()
        }
    }
}

impl Render for LogTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Editor::new(&self.editor).bordered(false).readonly(true).h_full()
    }
}

fn level_decorations(text: &str, base: usize, cx: &App) -> Vec<TextDecoration> {
    if !preferences::get(cx).color_log_levels {
        return Vec::new();
    }
    let theme = cx.theme();
    level_spans(text, base)
        .into_iter()
        .filter_map(|(range, level)| {
            let color = match level {
                LogLevel::Error => theme.red,
                LogLevel::Warn => theme.yellow,
                LogLevel::Debug | LogLevel::Trace => theme.muted_foreground,
                LogLevel::Info => return None,
            };
            Some(TextDecoration::new(range, HighlightStyle { color: Some(color), ..Default::default() }))
        })
        .collect()
}
