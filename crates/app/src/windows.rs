//! The editor windows: opening them, the session they share, and quitting.
//!
//! Each window has its own [`Workspace`] and tabs, and the session holds
//! them all, so every window comes back on the next launch. Closing a
//! window closes only it; closing the last one quits, like Notepad++.
//! Tab ids and the numbers of untitled tabs ("new 3") are unique across
//! windows, since unsaved text is backed up by tab id.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui_kit::{component::TitleBar, *};
use oxypad_session::{Session, SessionStore, WindowSession};

use crate::{
    preferences,
    workspace::{LaunchItem, Workspace},
};

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(2);
/// How far each new window is moved from the one before, so none hides another.
const CASCADE: Pixels = px(28.);

struct OpenWindow {
    handle: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
}

pub struct Windows {
    /// In the order they were opened.
    open: Vec<OpenWindow>,
    store: Option<Arc<Mutex<SessionStore>>>,
    /// Something in a window changed since the session was last written.
    changed: bool,
    next_tab_id: u64,
    next_untitled: usize,
    /// The user agreed to quit without saving; quitting must not ask again.
    quit_confirmed: bool,
}

impl Global for Windows {}

/// Opens the session's windows, or one empty window, with the files from
/// the command line in the last one, which is in front.
pub fn init(launch: Vec<LaunchItem>, cx: &mut App) {
    let mut windows = Windows {
        open: Vec::new(),
        store: None,
        changed: false,
        next_tab_id: 1,
        next_untitled: 1,
        quit_confirmed: false,
    };
    let mut session = Session::default();
    match SessionStore::default_location() {
        Ok(mut store) => {
            if preferences::get(cx).restore_session {
                match store.load() {
                    Ok(Some(loaded)) => session = loaded,
                    Ok(None) => {}
                    Err(err) => eprintln!("oxypad: could not restore the session: {err:#}"),
                }
            }
            windows.store = Some(Arc::new(Mutex::new(store)));
        }
        Err(err) => eprintln!("oxypad: sessions are disabled: {err:#}"),
    }
    for tab in session.tabs() {
        windows.next_tab_id = windows.next_tab_id.max(tab.id + 1);
        if let Some(number) = tab.title.strip_prefix("new ").and_then(|number| number.parse::<usize>().ok()) {
            windows.next_untitled = windows.next_untitled.max(number + 1);
        }
    }
    cx.set_global(windows);

    let mut contents = session.windows;
    if contents.is_empty() {
        contents.push(WindowSession::default());
    }
    let last = contents.len() - 1;
    let mut launch = Some(launch);
    for (ix, content) in contents.into_iter().enumerate() {
        let files = if ix == last { launch.take().unwrap_or_default() } else { Vec::new() };
        open(content, files, None, cx);
    }

    // Runs until the app quits.
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(AUTOSAVE_INTERVAL).await;
            cx.update(autosave);
        }
    })
    .detach();
    cx.on_app_quit(|cx| {
        save_now(cx);
        async {}
    })
    .detach();
    cx.on_window_closed(|cx, closed| {
        let windows = cx.global_mut::<Windows>();
        windows.open.retain(|window| window.handle.window_id() != closed);
        windows.changed = true;
        if windows.open.is_empty() {
            cx.quit();
        }
    })
    .detach();
}

/// Opens a window showing `content`, then the files in `launch`. It is
/// placed a little below and right of `near`, or of the last window.
pub fn open(content: WindowSession, launch: Vec<LaunchItem>, near: Option<Bounds<Pixels>>, cx: &mut App) {
    let base = if cfg!(target_os = "macos") {
        WindowOptions {
            titlebar: Some(TitlebarOptions { title: Some("OxyPad".into()), ..Default::default() }),
            ..Default::default()
        }
    } else {
        // Windows and Linux draw the title bar themselves to host the menu bar.
        TitleBar::window_options()
    };
    let bounds = match near {
        Some(near) => Bounds { origin: near.origin + point(CASCADE, CASCADE), size: near.size },
        None => {
            let shift = CASCADE * cx.global::<Windows>().open.len() as f32;
            let centered = Bounds::centered(None, size(px(1100.), px(720.)), cx);
            Bounds { origin: centered.origin + point(shift, shift), size: centered.size }
        }
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(480.), px(320.))),
        app_id: Some("org.oxyapps.oxypad".into()),
        ..base
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        preferences::apply_appearance(Some(window), cx);
        cx.new(|cx| Workspace::new(content, launch, window, cx))
    });
    match opened {
        Ok((handle, workspace)) => {
            let windows = cx.global_mut::<Windows>();
            windows.open.push(OpenWindow { handle, workspace: workspace.downgrade() });
            windows.changed = true;
        }
        Err(err) => eprintln!("oxypad: could not open a window: {err:#}"),
    }
}

/// Opens the files sent by later launches (and by Finder on macOS) in the
/// front window, and brings it forward; with no files, only brings it forward.
pub fn receive(mut received: UnboundedReceiver<Vec<LaunchItem>>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Some(items) = received.next().await {
            cx.update(|cx| open_in_front(items, cx));
        }
    })
    .detach();
}

fn open_in_front(items: Vec<LaunchItem>, cx: &mut App) {
    let Some(window) = front(cx) else {
        open(WindowSession::default(), items, None, cx);
        return;
    };
    let Some(workspace) = window.workspace.upgrade() else { return };
    let _ = window.handle.update(cx, |_, window, cx| {
        workspace.update(cx, |workspace, cx| {
            for item in items {
                workspace.open_path(item.path, item.follow, window, cx);
            }
        });
        window.activate_window();
    });
    cx.activate(true);
}

/// The window in front, or else the one opened last.
fn front(cx: &App) -> Option<&OpenWindow> {
    let active = cx.active_window();
    let windows = &cx.global::<Windows>().open;
    windows.iter().find(|window| Some(window.handle) == active).or(windows.last())
}

/// A new, empty window.
pub fn open_empty(near: Option<Bounds<Pixels>>, cx: &mut App) {
    open(WindowSession::default(), Vec::new(), near, cx);
}

pub fn count(cx: &App) -> usize {
    cx.global::<Windows>().open.len()
}

/// Something in a window changed: the session is written on the next autosave.
pub fn mark_changed(cx: &mut App) {
    cx.global_mut::<Windows>().changed = true;
}

/// An id for a new tab, unique across windows.
pub fn next_tab_id(cx: &mut App) -> u64 {
    let windows = cx.global_mut::<Windows>();
    let id = windows.next_tab_id;
    windows.next_tab_id += 1;
    id
}

/// The number of the next untitled tab, unique across windows.
pub fn next_untitled(cx: &mut App) -> usize {
    let windows = cx.global_mut::<Windows>();
    let number = windows.next_untitled;
    windows.next_untitled += 1;
    number
}

fn workspaces(cx: &App) -> Vec<Entity<Workspace>> {
    cx.global::<Windows>().open.iter().filter_map(|window| window.workspace.upgrade()).collect()
}

/// The session to save. Empty when restoring sessions is off, which also
/// removes the backups of an earlier session.
fn session(cx: &App) -> Session {
    if !preferences::get(cx).restore_session {
        return Session::default();
    }
    Session { windows: workspaces(cx).iter().map(|workspace| workspace.read(cx).window_session(cx)).collect() }
}

/// Writes the session on the background executor if anything changed.
fn autosave(cx: &mut App) {
    let windows = cx.global_mut::<Windows>();
    if !windows.changed {
        return;
    }
    windows.changed = false;
    let Some(store) = windows.store.clone() else { return };
    let session = session(cx);
    if session_would_be_lost(&session, cx) {
        return;
    }
    cx.background_spawn(async move {
        if let Err(err) = store.lock().unwrap().save(&session) {
            eprintln!("oxypad: autosave failed: {err:#}");
        }
    })
    .detach();
}

/// Writes the session now, before quitting or closing the last window.
pub fn save_now(cx: &mut App) {
    let Some(store) = cx.global::<Windows>().store.clone() else { return };
    let session = session(cx);
    if session_would_be_lost(&session, cx) {
        return;
    }
    if let Err(err) = store.lock().unwrap().save(&session) {
        eprintln!("oxypad: saving the session failed: {err:#}");
    }
    cx.global_mut::<Windows>().changed = false;
}

/// With sessions on, a session without windows only happens after the last
/// window has closed (and saved it): writing it would forget every tab.
fn session_would_be_lost(session: &Session, cx: &App) -> bool {
    preferences::get(cx).restore_session && session.windows.is_empty()
}

/// Tabs whose unsaved changes quitting would lose: with the session off,
/// every modified tab in every window.
fn changes_lost_on_quit(cx: &App) -> usize {
    if cx.global::<Windows>().quit_confirmed || preferences::get(cx).restore_session {
        return 0;
    }
    workspaces(cx).iter().map(|workspace| workspace.read(cx).dirty_count(cx)).sum()
}

/// Whether quitting now needs the user's confirmation.
pub fn quit_needs_confirmation(cx: &App) -> bool {
    changes_lost_on_quit(cx) > 0
}

/// Quits, first asking in the front window about unsaved changes that
/// would be lost.
pub fn request_quit(cx: &mut App) {
    let dirty = changes_lost_on_quit(cx);
    if dirty == 0 {
        save_now(cx);
        cx.quit();
        return;
    }
    let Some(handle) = front(cx).map(|window| window.handle) else {
        cx.quit();
        return;
    };
    let message = if dirty == 1 { t!("dialog.quit_one").to_string() } else { t!("dialog.quit_many", count = dirty).to_string() };
    let asked = handle.update(cx, |_, window, cx| {
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&t!("dialog.quit_detail")),
            &[PromptButton::new(t!("dialog.quit_without_saving")), PromptButton::cancel(t!("dialog.cancel"))],
            cx,
        );
        cx.spawn(async move |cx| {
            if answer.await == Ok(0) {
                cx.update(|cx| {
                    cx.global_mut::<Windows>().quit_confirmed = true;
                    cx.quit();
                });
            }
        })
        .detach();
    });
    if asked.is_err() {
        cx.quit();
    }
}
