//! Color themes: the ones built into OxyPad plus any the user puts in the
//! themes folder, which is watched so edits show up right away.

use std::path::PathBuf;

use gpui_kit::{
    App, SharedString,
    component::{ThemeMode, ThemeRegistry},
};
use serde::Deserialize;

use crate::preferences;

/// Theme files from gpui-kit (see `themes/README.md`).
const BUILT_IN: &[&str] = &[
    include_str!("../themes/adventure.json"),
    include_str!("../themes/alduin.json"),
    include_str!("../themes/asciinema.json"),
    include_str!("../themes/aurora.json"),
    include_str!("../themes/ayu.json"),
    include_str!("../themes/catppuccin.json"),
    include_str!("../themes/everforest.json"),
    include_str!("../themes/fahrenheit.json"),
    include_str!("../themes/flexoki.json"),
    include_str!("../themes/gruvbox.json"),
    include_str!("../themes/harper.json"),
    include_str!("../themes/hybrid.json"),
    include_str!("../themes/jellybeans.json"),
    include_str!("../themes/kibble.json"),
    include_str!("../themes/macos-classic.json"),
    include_str!("../themes/mellifluous.json"),
    include_str!("../themes/molokai.json"),
    include_str!("../themes/solarized.json"),
    include_str!("../themes/spaceduck.json"),
    include_str!("../themes/tokyonight.json"),
    include_str!("../themes/twilight.json"),
];

/// The folder users add theme files to.
pub fn dir() -> Option<PathBuf> {
    oxypad_session::data_dir().ok().map(|dir| dir.join("themes"))
}

/// Registers the built-in themes and starts watching the themes folder.
/// Call after `preferences::init`.
pub fn init(cx: &mut App) {
    add_built_in(cx);
    // Reloading the folder drops every theme not in it: put the built-in
    // ones back, then re-apply the chosen theme, which may have changed.
    cx.observe_global::<ThemeRegistry>(|cx| {
        if !add_built_in(cx) {
            preferences::apply_appearance(None, cx);
        }
    })
    .detach();
    if let Some(dir) = dir()
        && let Err(err) = ThemeRegistry::watch_dir(dir, cx, |cx| {
            add_built_in(cx);
            preferences::apply_appearance(None, cx);
        })
    {
        eprintln!("oxypad: custom themes are disabled: {err:#}");
    }
}

/// Adds the built-in themes the registry lacks. A theme file in the themes
/// folder with the same name wins. Returns whether any was added.
fn add_built_in(cx: &mut App) -> bool {
    let registry = ThemeRegistry::global(cx);
    let missing: Vec<&str> = BUILT_IN
        .iter()
        .copied()
        .filter(|file| theme_names(file).iter().any(|name| !registry.themes().contains_key(name.as_str())))
        .collect();
    if missing.is_empty() {
        return false;
    }
    let registry = ThemeRegistry::global_mut(cx);
    for file in missing {
        if let Err(err) = registry.load_themes_from_str(file) {
            eprintln!("oxypad: a built-in theme is invalid: {err:#}");
        }
    }
    true
}

fn theme_names(file: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct Set {
        themes: Vec<Named>,
    }
    #[derive(Deserialize)]
    struct Named {
        name: String,
    }
    serde_json::from_str::<Set>(file).map(|set| set.themes.into_iter().map(|theme| theme.name).collect()).unwrap_or_default()
}

/// Names of the themes for `mode`, the default one first, then by name.
pub fn names(mode: ThemeMode, cx: &App) -> Vec<SharedString> {
    let registry = ThemeRegistry::global(cx);
    let default = if mode.is_dark() { registry.default_dark_theme() } else { registry.default_light_theme() };
    let mut names: Vec<SharedString> = registry
        .themes()
        .values()
        .filter(|theme| theme.mode == mode && theme.name != default.name)
        .map(|theme| theme.name.clone())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.insert(0, default.name.clone());
    names
}
