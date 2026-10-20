use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable, TitleBar, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        link::Link,
        menu::AppMenuBar,
        notification::Notification,
        status_bar::StatusBar,
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use oxypad_core::{Document, ExternalChange, language_for_path};
use oxypad_tail::ResetReason;
use oxypad_session::{TabKind, TabSnapshot, ThemePreference, WindowSession};

use crate::{
    actions::*,
    compare_tab::{CompareSource, CompareTab},
    editor_tab::EditorTab,
    log_tab::{LogStatus, LogTab},
    preferences::{self, AppPreferences},
    windows,
};

mod tab_bar;
mod toolbar;

use tab_bar::TabMeta;

/// How often the window looks for files changed by other programs while it is in front.
const EXTERNAL_CHANGE_INTERVAL: Duration = Duration::from_secs(2);

const REPOSITORY_URL: &str = "https://github.com/oxyapps/OxyPad";
const WEBSITE_URL: &str = "https://oxyapps.org";

/// A file to open at launch, from the command line.
pub struct LaunchItem {
    pub path: PathBuf,
    pub follow: bool,
}

#[derive(Clone)]
enum TabView {
    Editor(Entity<EditorTab>),
    Log(Entity<LogTab>),
    Compare(Entity<CompareTab>),
}

impl TabView {
    fn id(&self, cx: &App) -> u64 {
        match self {
            Self::Editor(tab) => tab.read(cx).id(),
            Self::Log(tab) => tab.read(cx).id(),
            Self::Compare(tab) => tab.read(cx).id(),
        }
    }

    fn title(&self, cx: &App) -> SharedString {
        match self {
            Self::Editor(tab) => tab.read(cx).title(),
            Self::Log(tab) => tab.read(cx).title(),
            Self::Compare(tab) => tab.read(cx).title(),
        }
    }

    fn is_dirty(&self, cx: &App) -> bool {
        matches!(self, Self::Editor(tab) if tab.read(cx).is_dirty())
    }

    /// The file the tab shows, if any.
    fn path(&self, cx: &App) -> Option<PathBuf> {
        match self {
            Self::Editor(tab) => tab.read(cx).path().map(Path::to_path_buf),
            Self::Log(tab) => Some(tab.read(cx).path().to_path_buf()),
            Self::Compare(_) => None,
        }
    }

    fn is_open_at(&self, path: &Path, follow: bool, cx: &App) -> bool {
        match self {
            Self::Editor(tab) => !follow && tab.read(cx).path() == Some(path),
            Self::Log(tab) => follow && tab.read(cx).path() == path,
            Self::Compare(_) => false,
        }
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        match self {
            Self::Editor(tab) => tab.update(cx, |tab, cx| tab.focus(window, cx)),
            Self::Log(tab) => tab.update(cx, |tab, cx| tab.focus(window, cx)),
            Self::Compare(tab) => tab.update(cx, |tab, cx| tab.focus(window, cx)),
        }
    }

    /// Comparisons are not part of the session: they are cheap to redo.
    fn snapshot(&self, cx: &App) -> Option<TabSnapshot> {
        match self {
            Self::Editor(tab) => Some(tab.read(cx).snapshot(cx)),
            Self::Log(tab) => Some(tab.read(cx).snapshot()),
            Self::Compare(_) => None,
        }
    }

    /// The tab's current text, for comparing it with another tab.
    fn compare_source(&self, cx: &App) -> Option<CompareSource> {
        let (text, language) = match self {
            Self::Editor(tab) => (tab.read(cx).text(cx), tab.read(cx).language(cx)),
            Self::Log(tab) => (tab.read(cx).text(cx), language_for_path(tab.read(cx).path()).into()),
            Self::Compare(_) => return None,
        };
        Some(CompareSource { tab_id: self.id(cx), title: self.title(cx), text, language })
    }

    fn view(&self) -> AnyView {
        match self {
            Self::Editor(tab) => tab.clone().into(),
            Self::Log(tab) => tab.clone().into(),
            Self::Compare(tab) => tab.clone().into(),
        }
    }
}

pub struct Workspace {
    tabs: Vec<TabView>,
    /// Pinning and color of each tab, by id.
    meta: HashMap<u64, TabMeta>,
    active: usize,
    app_menu_bar: Entity<AppMenuBar>,
    focus_handle: FocusHandle,
    window_title: SharedString,
    /// The user agreed to close this window and lose its unsaved changes.
    close_confirmed: bool,
    /// A file-changed-on-disk question is on screen.
    asking_external_change: bool,
    /// The tab last right-clicked, for the tab context menu.
    context_tab: Rc<Cell<Option<u64>>>,
    /// Enter in the rename dialog's field.
    rename_subscription: Option<Subscription>,
    _external_changes: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    /// A window showing the tabs of `content`, then the files in `launch`.
    pub fn new(content: WindowSession, launch: Vec<LaunchItem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            tabs: Vec::new(),
            meta: HashMap::new(),
            active: 0,
            app_menu_bar: AppMenuBar::new(cx),
            focus_handle: cx.focus_handle(),
            window_title: SharedString::default(),
            close_confirmed: false,
            asking_external_change: false,
            context_tab: Rc::default(),
            rename_subscription: None,
            _external_changes: Task::ready(()),
            _subscriptions: Vec::new(),
        };

        this.restore(content, window, cx);
        for item in launch {
            this.open_path(item.path, item.follow, window, cx);
        }
        if this.tabs.is_empty() {
            this.new_untitled(window, cx);
        }

        this._external_changes = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(EXTERNAL_CHANGE_INTERVAL).await;
                let tick = this.update_in(cx, |this, window, cx| this.check_external_changes(window, cx));
                if tick.is_err() {
                    break;
                }
            }
        });
        // Like Notepad++, look for files changed by other programs when the
        // user comes back to OxyPad (the timer above also looks while it is
        // in front).
        this._subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
            this.check_external_changes(window, cx);
        }));
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.should_close(window, cx)).unwrap_or(true)
        });
        this._subscriptions.push(cx.observe_window_appearance(window, |_, window, cx| {
            if preferences::get(cx).theme == ThemePreference::System {
                preferences::apply_appearance(Some(window), cx);
            }
        }));
        this._subscriptions.push(cx.observe_global_in::<AppPreferences>(window, |this, window, cx| {
            this.apply_preferences(window, cx);
        }));

        let active = this.tabs[this.active].clone();
        window.defer(cx, move |window, cx| active.focus(window, cx));
        this
    }

    // ---- tabs -----------------------------------------------------------

    /// Opens the tabs of a saved window (or of a tab moved from another window).
    fn restore(&mut self, content: WindowSession, window: &mut Window, cx: &mut Context<Self>) {
        for snapshot in content.tabs {
            let (id, meta) = (snapshot.id, TabMeta { pinned: snapshot.pinned, color: snapshot.color });
            let tab = match snapshot.kind {
                TabKind::Edit => self.restore_editor(snapshot, window, cx),
                TabKind::Tail => snapshot
                    .path
                    .and_then(|path| LogTab::open(snapshot.id, path, snapshot.follow, window, cx).ok())
                    .map(TabView::Log),
            };
            if let Some(tab) = tab {
                self.meta.insert(id, meta);
                self.push_tab(tab, cx);
            }
        }
        self.active = content.active.min(self.tabs.len().saturating_sub(1));
        self.sort_pinned_first(cx);
    }

    fn restore_editor(
        &mut self,
        snapshot: TabSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TabView> {
        let (document, disk_text) = match &snapshot.path {
            Some(path) => match Document::open(path) {
                Ok(opened) => opened,
                // The file is gone but we still have its unsaved text: keep
                // the tab so nothing is lost; saving recreates the file.
                Err(_) if snapshot.unsaved_text.is_some() => {
                    let mut document = Document::untitled();
                    document.set_path(path.clone());
                    (document, String::new())
                }
                Err(_) => return None,
            },
            None => (Document::untitled(), String::new()),
        };
        let dirty = snapshot
            .unsaved_text
            .as_ref()
            .is_some_and(|text| snapshot.path.is_some() || !text.is_empty());
        let text = snapshot.unsaved_text.unwrap_or(disk_text);
        let title: SharedString = snapshot.title.into();
        let tab = cx.new(|cx| EditorTab::new(snapshot.id, title, document, text, dirty, window, cx));
        tab.update(cx, |tab, cx| {
            tab.set_cursor(snapshot.cursor, window, cx);
            tab.set_read_only(snapshot.read_only, cx);
        });
        Some(TabView::Editor(tab))
    }

    fn push_tab(&mut self, tab: TabView, cx: &mut Context<Self>) {
        let subscription = match &tab {
            TabView::Editor(entity) => cx.observe(entity, |this, _, cx| this.mark_changed(cx)),
            TabView::Log(entity) => cx.observe(entity, |this, _, cx| this.mark_changed(cx)),
            TabView::Compare(entity) => cx.observe(entity, |this, _, cx| this.mark_changed(cx)),
        };
        self._subscriptions.push(subscription);
        self.tabs.push(tab);
        self.mark_changed(cx);
    }

    fn mark_changed(&mut self, cx: &mut Context<Self>) {
        windows::mark_changed(cx);
        cx.notify();
    }

    fn take_id(&mut self, cx: &mut App) -> u64 {
        windows::next_tab_id(cx)
    }

    fn new_untitled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.take_id(cx);
        let title: SharedString = format!("new {}", windows::next_untitled(cx)).into();
        let format = preferences::get(cx);
        let document = Document::untitled_with(format.new_line_ending.resolve(), format.new_utf8_bom);
        let tab = cx.new(|cx| EditorTab::new(id, title, document, String::new(), false, window, cx));
        self.push_tab(TabView::Editor(tab), cx);
        self.activate(self.tabs.len() - 1, window, cx);
    }

    /// Opens `path` in an editor tab, or as a followed log when `follow` is
    /// set. Activates the tab instead if the file is already open.
    pub fn open_path(&mut self, path: PathBuf, follow: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(ix) = self.tabs.iter().position(|tab| tab.is_open_at(&path, follow, cx)) {
            self.activate(ix, window, cx);
            return;
        }

        let id = self.take_id(cx);
        let tab = if follow {
            LogTab::open(id, path.clone(), true, window, cx)
                .map(TabView::Log)
                .map_err(|err| could_not_follow(&path, err))
        } else {
            Document::open(&path).map(|(document, text)| {
                TabView::Editor(cx.new(|cx| {
                    EditorTab::new(id, SharedString::default(), document, text, false, window, cx)
                }))
            })
        };
        let tab = match tab {
            Ok(tab) => tab,
            Err(err) => {
                show_notification(window, cx, Notification::error(format!("{err:#}")));
                return;
            }
        };

        // Like Notepad++: an empty untitled tab gives way to the opened file.
        let replace = match self.tabs.get(self.active) {
            Some(TabView::Editor(current)) => current.read(cx).is_pristine(cx),
            _ => false,
        };
        self.push_tab(tab, cx);
        if replace {
            let removed = self.tabs.remove(self.active);
            self.meta.remove(&removed.id(cx));
        }
        self.activate(self.tabs.len() - 1, window, cx);
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix).cloned() else { return };
        self.active = ix;
        tab.focus(window, cx);
        self.mark_changed(cx);
    }

    fn remove_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|tab| tab.id(cx) == id) else { return };
        self.tabs.remove(ix);
        self.meta.remove(&id);
        if self.tabs.is_empty() {
            self.new_untitled(window, cx);
            return;
        }
        if self.active > ix || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.activate(self.active, window, cx);
    }

    fn active_editor(&self) -> Option<Entity<EditorTab>> {
        match self.tabs.get(self.active) {
            Some(TabView::Editor(tab)) => Some(tab.clone()),
            _ => None,
        }
    }

    fn active_compare(&self) -> Option<Entity<CompareTab>> {
        match self.tabs.get(self.active) {
            Some(TabView::Compare(tab)) => Some(tab.clone()),
            _ => None,
        }
    }

    /// Puts `tab` in place of the tab at `ix`, keeping its position.
    fn replace_tab(&mut self, ix: usize, tab: TabView, window: &mut Window, cx: &mut Context<Self>) {
        self.push_tab(tab, cx);
        let tab = self.tabs.pop().expect("just pushed");
        self.tabs[ix] = tab;
        self.activate(ix, window, cx);
    }

    /// Focuses the active editor and sends it `action` (undo, redo, …).
    fn dispatch_to_editor(&mut self, action: Box<dyn Action>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_editor() else { return };
        tab.update(cx, |tab, cx| tab.focus(window, cx));
        window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
    }

    // ---- files changed on disk ---------------------------------------------

    /// Asks what to do about the first editor tab, the active one first,
    /// whose file another program changed or deleted. One question at a
    /// time, and only while the window is in front.
    fn check_external_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.asking_external_change || !window.is_window_active() {
            return;
        }
        let order = std::iter::once(self.active).chain((0..self.tabs.len()).filter(|&ix| ix != self.active));
        let found = order.into_iter().find_map(|ix| match self.tabs.get(ix) {
            Some(TabView::Editor(tab)) => tab.read(cx).external_change().map(|change| (ix, tab.clone(), change)),
            _ => None,
        });
        let Some((ix, tab, change)) = found else { return };
        if ix != self.active {
            self.activate(ix, window, cx);
        }

        let (name, dirty) = {
            let tab = tab.read(cx);
            (tab.title(), tab.is_dirty())
        };
        let (message, detail, buttons) = match change {
            ExternalChange::Modified => (
                t!("dialog.changed", name = name).to_string(),
                if dirty { t!("dialog.changed_detail_unsaved") } else { t!("dialog.changed_detail") },
                vec![t!("dialog.reload"), t!("dialog.follow_as_log"), t!("dialog.keep_version"), t!("dialog.close")],
            ),
            ExternalChange::Deleted => (
                t!("dialog.deleted", name = name).to_string(),
                t!("dialog.deleted_detail"),
                vec![t!("dialog.save"), t!("dialog.save_as"), t!("dialog.keep_open"), t!("dialog.close_without_saving")],
            ),
        };
        self.asking_external_change = true;
        let buttons: Vec<PromptButton> = buttons.into_iter().map(PromptButton::new).collect();
        let answer = window.prompt(PromptLevel::Warning, &message, Some(&detail), &buttons, cx);
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.ok();
            let _ = this.update_in(cx, |this, window, cx| {
                this.asking_external_change = false;
                let id = tab.read(cx).id();
                if !this.tabs.iter().any(|open| open.id(cx) == id) {
                    return; // Closed meanwhile.
                }
                match (change, answer) {
                    (ExternalChange::Modified, Some(0)) => {
                        if let Err(err) = tab.update(cx, |tab, cx| tab.reload(window, cx)) {
                            show_notification(window, cx, Notification::error(format!("{err:#}")));
                        }
                    }
                    (ExternalChange::Modified, Some(1)) => {
                        tab.update(cx, |tab, cx| tab.discard_changes(cx));
                        if let Some(ix) = this.tabs.iter().position(|open| open.id(cx) == id) {
                            this.activate(ix, window, cx);
                            this.toggle_tail(window, cx);
                        }
                    }
                    (ExternalChange::Modified, Some(3)) => {
                        if let Some(ix) = this.tabs.iter().position(|open| open.id(cx) == id) {
                            this.close_tab_at(ix, window, cx);
                        }
                    }
                    (ExternalChange::Deleted, Some(3)) => this.remove_tab(id, window, cx),
                    (ExternalChange::Deleted, answer) => {
                        // Stop asking about the missing file whatever happens
                        // next; the tab keeps its text as unsaved changes.
                        tab.update(cx, |tab, cx| tab.keep_version(cx));
                        match answer {
                            Some(0) => this.save_tab(tab, false, false, window, cx),
                            Some(1) => this.save_tab(tab, true, false, window, cx),
                            _ => {}
                        }
                    }
                    // Keep This Version, or the question was dismissed.
                    (ExternalChange::Modified, _) => tab.update(cx, |tab, cx| tab.keep_version(cx)),
                }
            });
        })
        .detach();
    }

    // ---- tail and compare -----------------------------------------------------

    /// Switches the active tab between editing a file and following it as a
    /// read-only log, in place.
    fn toggle_tail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ix = self.active;
        let replacement = match self.tabs.get(ix) {
            Some(TabView::Editor(tab)) => {
                let tab = tab.read(cx);
                let Some(path) = tab.path().map(Path::to_path_buf) else { return };
                if tab.is_dirty() {
                    show_notification(window, cx, Notification::warning(t!("notification.save_before_following").to_string()));
                    return;
                }
                // Same id, so comparisons that reference this tab keep working.
                let id = tab.id();
                LogTab::open(id, path.clone(), true, window, cx)
                    .map(TabView::Log)
                    .map_err(|err| could_not_follow(&path, err))
            }
            Some(TabView::Log(tab)) => {
                let path = tab.read(cx).path().to_path_buf();
                let id = tab.read(cx).id();
                Document::open(&path).map(|(document, text)| {
                    TabView::Editor(cx.new(|cx| {
                        EditorTab::new(id, SharedString::default(), document, text, false, window, cx)
                    }))
                })
            }
            _ => return,
        };
        match replacement {
            Ok(tab) => self.replace_tab(ix, tab, window, cx),
            Err(err) => show_notification(window, cx, Notification::error(format!("{err:#}"))),
        }
    }

    /// Opens a side-by-side comparison of the active tab (left) and the tab
    /// `other_id` (right), or refreshes the one already open for that pair.
    fn open_compare(&mut self, other_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(left) = self.tabs.get(self.active).and_then(|tab| tab.compare_source(cx)) else { return };
        let Some(right) = self
            .tabs
            .iter()
            .find(|tab| tab.id(cx) == other_id)
            .and_then(|tab| tab.compare_source(cx))
        else {
            return;
        };

        let pair = (left.tab_id, right.tab_id);
        let existing = self.tabs.iter().position(
            |tab| matches!(tab, TabView::Compare(compare) if compare.read(cx).sources() == pair),
        );
        if let Some(ix) = existing {
            if let TabView::Compare(compare) = self.tabs[ix].clone() {
                compare.update(cx, |compare, cx| compare.refresh(left, right, cx));
            }
            self.activate(ix, window, cx);
            return;
        }

        let id = self.take_id(cx);
        let compare = cx.new(|cx| CompareTab::new(id, left, right, cx));
        self.push_tab(TabView::Compare(compare), cx);
        self.activate(self.tabs.len() - 1, window, cx);
    }

    // ---- saving -----------------------------------------------------------

    /// Saves the editor tab `tab`; asks for a path when it is untitled or
    /// `save_as` is set. Closes the tab afterwards when `close` is set.
    fn save_tab(&mut self, tab: Entity<EditorTab>, save_as: bool, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        let id = tab.read(cx).id();
        if !save_as && tab.read(cx).path().is_some() {
            match tab.update(cx, |tab, cx| tab.save(cx)) {
                Ok(()) if close => self.remove_tab(id, window, cx),
                Ok(()) => {}
                Err(err) => show_notification(window, cx, Notification::error(format!("{err:#}"))),
            }
            return;
        }

        let (directory, suggested) = {
            let tab = tab.read(cx);
            let directory = tab
                .path()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .or_else(dirs::document_dir)
                .unwrap_or_else(|| PathBuf::from("."));
            let suggested = match tab.path() {
                Some(_) => tab.title().to_string(),
                None => format!("{}.txt", tab.title()),
            };
            (directory, suggested)
        };
        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                match tab.update(cx, |tab, cx| tab.save_as(path, cx)) {
                    Ok(()) if close => this.remove_tab(id, window, cx),
                    Ok(()) => this.mark_changed(cx),
                    Err(err) => show_notification(window, cx, Notification::error(format!("{err:#}"))),
                }
            });
        })
        .detach();
    }

    fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix).cloned() else { return };
        let id = tab.id(cx);
        let TabView::Editor(editor) = tab else {
            self.remove_tab(id, window, cx);
            return;
        };
        if !editor.read(cx).is_dirty() {
            self.remove_tab(id, window, cx);
            return;
        }

        let message = t!("dialog.save_changes", name = editor.read(cx).title()).to_string();
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&t!("dialog.save_changes_detail")),
            &[
                PromptButton::new(t!("dialog.save")),
                PromptButton::new(t!("dialog.dont_save")),
                PromptButton::cancel(t!("dialog.cancel")),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = answer.await else { return };
            let _ = this.update_in(cx, |this, window, cx| match answer {
                0 => this.save_tab(editor, false, true, window, cx),
                1 => this.remove_tab(id, window, cx),
                _ => {}
            });
        })
        .detach();
    }

    // ---- preferences and quitting -------------------------------------------

    fn apply_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The menus may have been rebuilt in another language.
        self.app_menu_bar.update(cx, |bar, cx| bar.reload(cx));
        let preferences = preferences::get(cx).clone();
        for tab in self.tabs.clone() {
            match tab {
                TabView::Editor(tab) => tab.update(cx, |tab, cx| tab.apply_preferences(&preferences, window, cx)),
                TabView::Log(tab) => tab.update(cx, |tab, cx| tab.apply_preferences(&preferences, window, cx)),
                TabView::Compare(_) => {}
            }
        }
        // Turning the session on or off changes what must be saved.
        self.mark_changed(cx);
    }

    /// Tabs with unsaved changes.
    pub fn dirty_count(&self, cx: &App) -> usize {
        self.tabs.iter().filter(|tab| tab.is_dirty(cx)).count()
    }

    /// Called before this window closes. Closing the last window quits,
    /// which may ask about changes the session would not keep; closing
    /// another one loses its unsaved changes, so it asks first.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if windows::count(cx) <= 1 {
            if windows::quit_needs_confirmation(cx) {
                windows::request_quit(cx);
                return false;
            }
            windows::save_now(cx);
            return true;
        }
        let dirty = self.dirty_count(cx);
        if dirty == 0 || self.close_confirmed {
            return true;
        }
        let message = if dirty == 1 {
            t!("dialog.close_window_one").to_string()
        } else {
            t!("dialog.close_window_many", count = dirty).to_string()
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&t!("dialog.close_window_detail")),
            &[PromptButton::new(t!("dialog.close_window")), PromptButton::cancel(t!("dialog.cancel"))],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update_in(cx, |this, window, _| {
                    this.close_confirmed = true;
                    window.remove_window();
                });
            }
        })
        .detach();
        false
    }

    // ---- session ----------------------------------------------------------

    /// This window's tabs, for the session.
    pub fn window_session(&self, cx: &App) -> WindowSession {
        WindowSession {
            tabs: self
                .tabs
                .iter()
                .filter_map(|tab| {
                    let mut snapshot = tab.snapshot(cx)?;
                    let meta = self.meta(snapshot.id);
                    (snapshot.pinned, snapshot.color) = (meta.pinned, meta.color);
                    Some(snapshot)
                })
                .collect(),
            active: self
                .tabs
                .iter()
                .take(self.active)
                .filter(|tab| !matches!(tab, TabView::Compare(_)))
                .count(),
        }
    }

    /// Moves tab `id` to a new window, unsaved text, cursor, pin and color
    /// included (not its undo history). The last tab of a window stays.
    fn move_to_new_window(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() < 2 {
            return;
        }
        let Some(snapshot) = self.tabs.iter().find(|tab| tab.id(cx) == id).and_then(|tab| tab.snapshot(cx)) else {
            return;
        };
        let mut snapshot = snapshot;
        let meta = self.meta(id);
        (snapshot.pinned, snapshot.color) = (meta.pinned, meta.color);
        self.remove_tab(id, window, cx);
        let near = window.bounds();
        cx.defer(move |cx| windows::open(WindowSession { tabs: vec![snapshot], active: 0 }, Vec::new(), Some(near), cx));
    }

    // ---- actions ----------------------------------------------------------

    fn on_new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        self.new_untitled(window, cx);
    }

    fn on_new_window(&mut self, _: &NewWindow, window: &mut Window, cx: &mut Context<Self>) {
        let near = window.bounds();
        cx.defer(move |cx| windows::open_empty(Some(near), cx));
    }

    fn on_open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(false, window, cx);
    }

    fn on_open_log(&mut self, _: &OpenLog, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(true, window, cx);
    }

    fn prompt_and_open(&mut self, follow: bool, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: !follow,
            prompt: Some(if follow { t!("dialog.follow") } else { t!("dialog.open") }.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                for path in paths {
                    this.open_path(path, follow, window, cx);
                }
            });
        })
        .detach();
    }

    fn on_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_editor() {
            self.save_tab(tab, false, false, window, cx);
        }
    }

    fn on_save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_editor() {
            self.save_tab(tab, true, false, window, cx);
        }
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, window, cx);
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.activate((self.active + 1) % self.tabs.len(), window, cx);
    }

    fn on_previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.activate((self.active + self.tabs.len() - 1) % self.tabs.len(), window, cx);
    }

    fn on_toggle_tail(&mut self, _: &ToggleTail, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_tail(window, cx);
    }

    fn on_find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.get(self.active) {
            Some(TabView::Editor(tab)) => tab.update(cx, |tab, cx| tab.open_search(false, window, cx)),
            Some(TabView::Log(tab)) => tab.update(cx, |tab, cx| tab.open_search(window, cx)),
            _ => {}
        }
    }

    fn on_find_replace(&mut self, _: &FindReplace, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_editor() {
            tab.update(cx, |tab, cx| tab.open_search(true, window, cx));
        }
    }

    fn on_next_change(&mut self, _: &NextChange, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_compare() {
            tab.update(cx, |tab, cx| tab.next_change(cx));
        }
    }

    fn on_previous_change(&mut self, _: &PreviousChange, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_compare() {
            tab.update(cx, |tab, cx| tab.previous_change(cx));
        }
    }

    fn on_refresh_compare(&mut self, _: &RefreshCompare, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_compare() else { return };
        let (left_id, right_id) = tab.read(cx).sources();
        let source = |id: u64| self.tabs.iter().find(|tab| tab.id(cx) == id).and_then(|tab| tab.compare_source(cx));
        match (source(left_id), source(right_id)) {
            (Some(left), Some(right)) => tab.update(cx, |tab, cx| tab.refresh(left, right, cx)),
            _ => show_notification(window, cx, Notification::warning(t!("notification.compared_tab_closed").to_string())),
        }
    }

    fn on_toggle_soft_wrap(&mut self, _: &ToggleSoftWrap, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_editor() {
            tab.update(cx, |tab, cx| tab.toggle_soft_wrap(window, cx));
        }
    }

    fn on_about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        let icon = Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../oxypad-icons/png/oxypad-128.png").to_vec(),
        ));
        window.open_dialog(cx, move |dialog, _, cx| {
            dialog.child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .py_2()
                    .child(img(icon.clone()).size_16())
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("OxyPad"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("about.version", version = env!("CARGO_PKG_VERSION")).to_string()),
                    )
                    .child(div().text_sm().child(t!("about.tagline").to_string()))
                    .child(
                        v_flex()
                            .items_center()
                            .pt_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("about.open_source").to_string())
                            .child(t!("about.license").to_string()),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .pt_1()
                            .text_sm()
                            .child(Link::new("about-repository").href(REPOSITORY_URL).child("GitHub"))
                            .child(Link::new("about-website").href(WEBSITE_URL).child("OxyApps.org")),
                    ),
            )
        });
    }

    fn on_drop_paths(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        for path in paths.paths() {
            self.open_path(path.clone(), false, window, cx);
        }
    }

    // ---- rendering ----------------------------------------------------------

    fn sync_window_title(&mut self, window: &mut Window, cx: &App) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let dirty = if tab.is_dirty(cx) { "• " } else { "" };
        let title: SharedString = format!("{dirty}{} — OxyPad", tab.title(cx)).into();
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let item = |text: SharedString| div().text_xs().child(text);
        let mut bar = StatusBar::new();
        match self.tabs.get(self.active) {
            Some(TabView::Editor(tab)) => {
                let tab = tab.read(cx);
                let document = tab.document();
                let cursor = tab.cursor(cx);
                let path = document.path().map_or_else(|| t!("status.untitled").to_string(), display_path);
                bar = bar
                    .left(item(path.into()))
                    .when(tab.is_read_only(), |bar| bar.right(item(t!("status.read_only").into())))
                    .right(item(t!("status.position", line = cursor.line + 1, column = cursor.character + 1).to_string().into()))
                    .right(item(tab.language(cx)))
                    .right(item(document.encoding().to_string().into()))
                    .right(item(document.line_ending().label().into()))
                    .right(
                        Button::new("soft-wrap")
                            .label(t!("status.wrap"))
                            .ghost()
                            .xsmall()
                            .when(tab.soft_wrap(), |button| button.icon(IconName::Check))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_soft_wrap(&ToggleSoftWrap, window, cx)
                            })),
                    );
            }
            Some(TabView::Log(tab)) => {
                let tab = tab.read(cx);
                let status: SharedString = match tab.status() {
                    LogStatus::Following => t!("status.tail_following").into(),
                    LogStatus::Paused => t!("status.tail_paused").into(),
                    LogStatus::Reset(ResetReason::Truncated) => t!("status.tail_truncated").into(),
                    LogStatus::Reset(ResetReason::Rotated) => t!("status.tail_rotated").into(),
                    LogStatus::Error(err) => t!("status.tail_error", error = err).into(),
                };
                bar = bar
                    .left(item(display_path(tab.path()).into()))
                    .right(item(t!("status.lines", count = tab.line_count(cx)).into()))
                    .right(item(t!("status.read_only").into()))
                    .right(item(status));
            }
            Some(TabView::Compare(tab)) => {
                bar = bar.left(item(tab.read(cx).summary())).right(item(t!("status.read_only").into()));
            }
            None => {}
        }
        bar
    }
}

/// Shows a toast. Deferred to after the current update, because during
/// startup the window's notification layer does not exist yet.
fn could_not_follow(path: &Path, err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(t!("notification.could_not_follow", path = path.display(), error = err).to_string())
}

fn show_notification(window: &mut Window, cx: &mut App, notification: impl Into<Notification> + 'static) {
    window.defer(cx, move |window, cx| window.push_notification(notification, cx));
}

/// Status-bar form of a path: home shown as `~`, long paths cut from the
/// left so the file name and the items on the right stay visible.
fn display_path(path: &Path) -> String {
    const MAX_CHARS: usize = 70;
    let home = dirs::home_dir();
    let shown = match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    };
    let count = shown.chars().count();
    if count <= MAX_CHARS {
        return shown;
    }
    let tail: String = shown.chars().skip(count - (MAX_CHARS - 1)).collect();
    format!("…{tail}")
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_window_title(window, cx);
        let content = self.tabs.get(self.active).map(TabView::view);

        v_flex()
            .id("workspace")
            .key_context(WORKSPACE_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_new_window))
            .on_action(cx.listener(Self::on_open_file))
            .on_action(cx.listener(Self::on_open_log))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_previous_tab))
            .on_action(cx.listener(Self::on_toggle_soft_wrap))
            .on_action(cx.listener(Self::on_toggle_tail))
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_find_replace))
            .on_action(cx.listener(Self::on_next_change))
            .on_action(cx.listener(Self::on_previous_change))
            .on_action(cx.listener(Self::on_refresh_compare))
            .on_action(cx.listener(Self::on_about))
            .on_drop(cx.listener(Self::on_drop_paths))
            .when(cfg!(not(target_os = "macos")), |this| {
                this.child(TitleBar::new().child(self.app_menu_bar.clone()))
            })
            .child(self.render_toolbar(cx))
            .child(self.render_tab_bar(cx))
            .child(h_flex().flex_1().min_h_0().children(content.map(|view| div().size_full().child(view))))
            .child(self.render_status_bar(cx))
    }
}
