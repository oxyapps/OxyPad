//! OxyPad: a fast, native, cross-platform text editor.

// No console window behind the app on Windows (debug builds keep it for logs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
extern crate rust_i18n;

// Interface texts, from locales/<language>.yml.
i18n!("locales", fallback = "en");

mod actions;
mod assets;
mod compare_tab;
mod editor_tab;
mod i18n;
mod log_tab;
mod menus;
mod preferences;
mod syntax;
mod themes;
mod workspace;

use std::path::PathBuf;

use gpui_kit::{component::TitleBar, *};

use crate::{
    actions::{OpenSettings, Quit},
    workspace::{LaunchItem, MainWorkspace, Workspace},
};

const USAGE: &str = "\
Usage: oxypad [FILE]... [-f LOG]...

  FILE          open FILE in an editor tab
  -f, --follow  follow LOG read-only, like `tail -f`
  -h, --help    show this help";

fn parse_args() -> Vec<LaunchItem> {
    let mut items = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            Some("-f" | "--follow") => match args.next() {
                Some(path) => items.push(LaunchItem { path: PathBuf::from(path), follow: true }),
                None => {
                    eprintln!("{USAGE}");
                    std::process::exit(2);
                }
            },
            _ => items.push(LaunchItem { path: PathBuf::from(arg), follow: false }),
        }
    }
    items
}

fn main() {
    let launch = parse_args();

    gpui_kit::application().with_assets(assets::AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        i18n::init(cx);
        themes::init(cx);
        actions::bind_keys(cx);
        let app_menu_bar = menus::install(cx);
        // Quitting goes through the workspace, which may ask about unsaved changes.
        cx.on_action(|_: &Quit, cx| Workspace::request_quit_app(cx));
        cx.on_action(|_: &OpenSettings, cx| preferences::open_window(cx));

        let base = if cfg!(target_os = "macos") {
            WindowOptions {
                titlebar: Some(TitlebarOptions { title: Some("OxyPad".into()), ..Default::default() }),
                ..Default::default()
            }
        } else {
            // Windows and Linux draw the title bar themselves to host the menu bar.
            TitleBar::window_options()
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1100.), px(720.)), cx)),
            window_min_size: Some(size(px(480.), px(320.))),
            app_id: Some("org.oxyapps.oxypad".into()),
            ..base
        };

        let (window, workspace) = gpui_kit::open_window(options, cx, |window, cx| {
            preferences::apply_appearance(Some(window), cx);
            cx.new(|cx| Workspace::new(launch, app_menu_bar, window, cx))
        })
        .expect("failed to open the main window");
        cx.set_global(MainWorkspace { window, workspace: workspace.downgrade() });

        // One main window per app: closing it quits, like Notepad++. Other
        // windows (Settings) close on their own.
        let main_window = window.window_id();
        cx.on_window_closed(move |cx, closed| {
            if closed == main_window {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
