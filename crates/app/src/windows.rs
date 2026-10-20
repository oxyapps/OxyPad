//! The editor windows: opening them, the session they share, and quitting.
//!
//! Each window has its own [`Workspace`] and tabs, and the session holds
//! them all, so every window comes back on the next launch. Closing a
//! window closes only it; closing the last one quits, like Notepad++.
//! Tab ids are unique across windows, since unsaved text is backed up by
//! tab id, and so are the numbers of untitled tabs ("new 3").

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui_kit::{component::TitleBar, *};
use oxypad_session::{Session, SessionStore, WindowSession};

use crate::{
    preferences,
    workspace::{LaunchItem, Workspace},
};

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(2);
/// Autosaves that take longer are noted in the diagnostic log.
const SLOW_AUTOSAVE: Duration = Duration::from_millis(100);
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
        quit_confirmed: false,
    };
    let mut session = Session::default();
    match SessionStore::default_location() {
        Ok(mut store) => {
            if preferences::get(cx).restore_session {
                let started = Instant::now();
                match store.load() {
                    Ok(Some(loaded)) => {
                        session = loaded;
                        log::info!(
                            "restored {} window(s), {} tab(s) in {:?}",
                            session.windows.len(),
                            session.tabs().count(),
                            started.elapsed()
                        );
                    }
                    Ok(None) => {}
                    Err(err) => log::error!("could not restore the session: {err:#}"),
                }
            }
            windows.store = Some(Arc::new(Mutex::new(store)));
        }
        Err(err) => log::warn!("sessions are disabled: {err:#}"),
    }
    for tab in session.tabs() {
        windows.next_tab_id = windows.next_tab_id.max(tab.id + 1);
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
        crate::diagnostics::watch_exit();
        log::info!("quitting: saving the session");
        save_now(cx);
        log::info!("quitting: OxyPad is done, GPUI tears down the app");
        async {}
    })
    .detach();
    cx.on_window_closed(|cx, closed| {
        let windows = cx.global_mut::<Windows>();
        windows.open.retain(|window| window.handle.window_id() != closed);
        windows.changed = true;
        log::info!("window closed, {} left", windows.open.len());
        if windows.open.is_empty() {
            crate::diagnostics::watch_exit();
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
        Err(err) => log::error!("could not open a window: {err:#}"),
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

/// The other windows a tab can move to, each with its front tab's title,
/// in the order they were opened.
pub fn others(except: EntityId, cx: &App) -> Vec<(AnyWindowHandle, WeakEntity<Workspace>, SharedString)> {
    cx.global::<Windows>()
        .open
        .iter()
        .filter_map(|window| {
            let workspace = window.workspace.upgrade().filter(|workspace| workspace.entity_id() != except)?;
            let title = workspace.read(cx).active_title(cx);
            Some((window.handle, window.workspace.clone(), title))
        })
        .collect()
}

/// The numbers of the untitled tabs ("new N") in every window but the
/// one of `except`, which is being updated and counts its own.
pub fn untitled_numbers_elsewhere(except: EntityId, cx: &App) -> Vec<usize> {
    workspaces(cx)
        .iter()
        .filter(|workspace| workspace.entity_id() != except)
        .flat_map(|workspace| workspace.read(cx).untitled_numbers(cx))
        .collect()
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
        let started = Instant::now();
        if let Err(err) = store.lock().unwrap().save(&session) {
            log::error!("autosave failed: {err:#}");
        }
        if started.elapsed() > SLOW_AUTOSAVE {
            log::info!("slow autosave: {} in {:?}", describe(&session), started.elapsed());
        }
    })
    .detach();
}

/// Writes the session now, before quitting or closing the last window.
pub fn save_now(cx: &mut App) {
    let Some(store) = cx.global::<Windows>().store.clone() else { return };
    let started = Instant::now();
    let session = session(cx);
    if session_would_be_lost(&session, cx) {
        return;
    }
    let gathered = started.elapsed();
    // Waits for an autosave still writing.
    let mut store = store.lock().unwrap();
    let locked = started.elapsed();
    if let Err(err) = store.save(&session) {
        log::error!("saving the session failed: {err:#}");
    }
    log::info!(
        "saved the session: {}; gathered in {gathered:?}, waited for autosave {:?}, total {:?}",
        describe(&session),
        locked - gathered,
        started.elapsed()
    );
    cx.global_mut::<Windows>().changed = false;
}

/// The size of a session, for the diagnostic log.
fn describe(session: &Session) -> String {
    let unsaved: Vec<usize> = session.tabs().filter_map(|tab| tab.unsaved_text.as_ref().map(String::len)).collect();
    format!(
        "{} window(s), {} tab(s), {} with unsaved text ({} KiB)",
        session.windows.len(),
        session.tabs().count(),
        unsaved.len(),
        unsaved.iter().sum::<usize>() / 1024
    )
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
    log::info!("quit requested; {dirty} tab(s) would lose changes");
    if dirty == 0 {
        crate::diagnostics::watch_exit();
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
