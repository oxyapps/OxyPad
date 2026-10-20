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
mod instance;
mod log_highlight;
mod log_tab;
mod menus;
mod preferences;
mod print;
mod syntax;
mod terminal;
mod themes;
mod windows;
mod workspace;

use std::path::PathBuf;

use crate::{
    actions::{OpenSettings, Quit},
    workspace::LaunchItem,
};

const USAGE: &str = "\
Usage: oxypad [FILE]... [-f LOG]...

  FILE          open FILE in an editor tab
  -f, --follow  follow LOG read-only, like `tail -f`
  -V, --version show the version
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
            Some("-V" | "--version") => {
                println!("OxyPad {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            Some("-f" | "--follow") => match args.next() {
                Some(path) => items.push(LaunchItem { path: PathBuf::from(path), follow: true }),
                None => {
                    eprintln!("{USAGE}");
                    std::process::exit(2);
                }
            },
            // Old macOS versions pass a process serial number to apps started from Finder.
            Some(arg) if arg.starts_with("-psn_") => {}
            _ => items.push(LaunchItem { path: PathBuf::from(arg), follow: false }),
        }
    }
    items
}

fn main() {
    let launch = parse_args();
    let listener = match instance::claim(&launch) {
        instance::Claim::Primary(listener) => listener,
        instance::Claim::Forwarded => return,
    };
    // Files from later launches, and on macOS from Finder and the Dock.
    let (opened, received) = futures::channel::mpsc::unbounded();
    if let Some(listener) = listener {
        instance::serve(listener, opened.clone());
    }

    let app = gpui_kit::application().with_assets(assets::AppAssets);
    app.on_open_urls(move |urls| {
        let items = instance::paths_from_urls(&urls).into_iter().map(|path| LaunchItem { path, follow: false });
        let _ = opened.unbounded_send(items.collect());
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        i18n::init(cx);
        themes::init(cx);
        actions::bind_keys(cx);
        menus::install(cx);
        // Quitting may ask about unsaved changes in any window.
        cx.on_action(|_: &Quit, cx| windows::request_quit(cx));
        cx.on_action(|_: &OpenSettings, cx| preferences::open_window(cx));
        windows::init(launch, cx);
        windows::receive(received, cx);
        cx.activate(true);
    });
}
