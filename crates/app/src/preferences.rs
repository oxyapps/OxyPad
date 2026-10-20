//! User preferences: the app-wide state, how it is applied, and the
//! Settings window.

use std::{borrow::Cow, path::PathBuf};

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Theme, ThemeMode, ThemeRegistry,
        Sizable,
        button::Button,
        setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings},
        v_flex,
    },
    *,
};
use oxypad_session::{LineEndingPreference, Preferences, ThemePreference, data_dir};

use crate::{i18n, log_highlight, themes};

/// App-wide preferences. Observers of this global (the workspace) re-apply
/// them to open tabs when they change.
pub struct AppPreferences {
    values: Preferences,
    dir: Option<PathBuf>,
    /// The theme's own monospace font, used when the font is "Automatic".
    default_mono_font: SharedString,
    window: Option<AnyWindowHandle>,
}

impl Global for AppPreferences {}

/// Loads the preferences. Call after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    let dir = data_dir().ok();
    let values = dir.as_deref().map(Preferences::load).unwrap_or_default();
    let default_mono_font = cx.theme().mono_font_family.clone();
    cx.set_global(AppPreferences { values, dir, default_mono_font, window: None });
}

pub fn get(cx: &App) -> &Preferences {
    &cx.global::<AppPreferences>().values
}

/// Changes the preferences, saves them and applies them everywhere.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Preferences)) {
    let mut values = get(cx).clone();
    change(&mut values);
    let values = values.clamped();
    if &values == get(cx) {
        return;
    }
    cx.update_global::<AppPreferences, _>(|state, _| {
        if let Some(dir) = &state.dir
            && let Err(err) = values.save(dir)
        {
            eprintln!("oxypad: could not save the settings: {err:#}");
        }
        state.values = values;
    });
    apply_appearance(None, cx);
    i18n::apply(cx);
    cx.refresh_windows();
}

/// Applies the theme and the editor font. With the "System" theme, call it
/// again when the system appearance changes.
pub fn apply_appearance(window: Option<&mut Window>, cx: &mut App) {
    let preferences = get(cx).clone();
    let registry = ThemeRegistry::global(cx);
    let pick = |name: &str, mode: ThemeMode| {
        registry.themes().get(name).filter(|theme| theme.mode == mode).cloned().unwrap_or_else(|| {
            if mode.is_dark() { registry.default_dark_theme().clone() } else { registry.default_light_theme().clone() }
        })
    };
    let (light, dark) = (pick(&preferences.light_theme, ThemeMode::Light), pick(&preferences.dark_theme, ThemeMode::Dark));
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    match preferences.theme {
        ThemePreference::System => Theme::sync_system_appearance(window, cx),
        ThemePreference::Light => Theme::change(ThemeMode::Light, window, cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
    let font = if preferences.font_family.is_empty() {
        cx.global::<AppPreferences>().default_mono_font.clone()
    } else {
        preferences.font_family.clone().into()
    };
    let theme = Theme::global_mut(cx);
    theme.mono_font_family = font;
    theme.mono_font_size = px(preferences.font_size);
}

/// Opens the Settings window, or brings it to the front.
pub fn open_window(cx: &mut App) {
    let state = cx.global::<AppPreferences>();
    if let Some(handle) = state.window
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions { title: Some(t!("settings.window_title").into()), ..Default::default() }),
        window_bounds: Some(WindowBounds::centered(size(px(820.), px(580.)), cx)),
        window_min_size: Some(size(px(640.), px(420.))),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| SettingsView::new(window, cx))) {
        Ok((handle, _)) => cx.global_mut::<AppPreferences>().window = Some(handle),
        Err(err) => eprintln!("oxypad: could not open the settings: {err:#}"),
    }
}

struct SettingsView {
    /// Installed monospace fonts, gathered once.
    fonts: Vec<(SharedString, SharedString)>,
}

impl SettingsView {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self { fonts: monospace_fonts(cx) }
    }
}

/// Installed font families that look monospaced by name. The text system
/// cannot tell monospace fonts apart, so this goes by well-known names.
fn monospace_fonts(cx: &App) -> Vec<(SharedString, SharedString)> {
    const HINTS: &[&str] = &[
        "mono", "code", "courier", "consolas", "menlo", "monaco", "hack", "fixed", "terminal", "iosevka",
        "inconsolata", "fira", "jetbrains", "cascadia", "source code", "sf mono", "lilex",
    ];
    let mut names: Vec<String> = cx
        .text_system()
        .all_font_names()
        .into_iter()
        .filter(|name| !name.starts_with('.'))
        .filter(|name| {
            let lower = name.to_lowercase();
            HINTS.iter().any(|hint| lower.contains(hint))
        })
        .collect();
    names.sort_unstable_by_key(|name| name.to_lowercase());
    names.dedup();
    std::iter::once(("".into(), t!("settings.font_automatic").into()))
        .chain(names.into_iter().map(|name| (name.clone().into(), name.into())))
        .collect()
}

/// The color scheme of the mode in use: picking one sets it for that mode
/// only, so with "Match System" light and dark each keep their own. Built
/// each time the page renders, so it follows mode changes and themes added
/// to the folder.
fn color_scheme_picker(cx: &App) -> SettingItem {
    let mode = cx.theme().mode;
    let names = themes::names(mode, cx);
    let default = names.first().cloned().unwrap_or_default();
    let choices = names.iter().map(|name| (name.clone(), name.clone())).collect::<Vec<_>>();
    SettingItem::new(
        t!("settings.color_scheme"),
        SettingField::scrollable_dropdown(
            choices,
            move |cx| {
                let p = get(cx);
                let current: SharedString =
                    if mode.is_dark() { p.dark_theme.clone() } else { p.light_theme.clone() }.into();
                if current.is_empty() || !names.contains(&current) { default.clone() } else { current }
            },
            move |value, cx| {
                let value = value.to_string();
                update(cx, |p| if mode.is_dark() { p.dark_theme = value } else { p.light_theme = value });
            },
        ),
    )
}

/// A group whose title is drawn inside it rather than given to the group.
/// Titled groups are listed under their page in the sidebar; for a page
/// whose groups all fit on one screen that only adds clutter.
fn section(title: impl Into<SharedString>) -> SettingGroup {
    let title: SharedString = title.into();
    SettingGroup::new().item(
        SettingItem::render({
            let title = title.clone();
            move |_, _, cx| div().text_color(cx.theme().muted_foreground).child(title.clone())
        })
        .keywords([title]),
    )
}

fn options<const N: usize>(pairs: [(&'static str, Cow<'static, str>); N]) -> Vec<(SharedString, SharedString)> {
    pairs.into_iter().map(|(value, label)| (value.into(), label.into())).collect()
}

fn switch(
    title: impl Into<SharedString>,
    read: fn(&Preferences) -> bool,
    write: fn(&mut Preferences, bool),
) -> SettingItem {
    SettingItem::new(
        title,
        SettingField::switch(move |cx| read(get(cx)), move |value, cx| update(cx, |p| write(p, value))),
    )
}

fn number(
    title: impl Into<SharedString>,
    range: (f64, f64, f64),
    read: fn(&Preferences) -> f64,
    write: fn(&mut Preferences, f64),
) -> SettingItem {
    let (min, max, step) = range;
    SettingItem::new(
        title,
        SettingField::number_input(
            NumberFieldOptions { min, max, step },
            move |cx| read(get(cx)),
            move |value, cx| update(cx, |p| write(p, value)),
        ),
    )
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Follows a change of language.
        window.set_window_title(&t!("settings.window_title"));
        let fonts = self.fonts.clone();
        let general = SettingPage::new(t!("settings.general")).icon(IconName::Settings).default_open(true).groups([
            SettingGroup::new().item(SettingItem::new(
                t!("settings.language"),
                SettingField::dropdown(
                    std::iter::once((SharedString::default(), SharedString::from(t!("settings.language_system"))))
                        .chain(i18n::LANGUAGES.iter().map(|(code, name)| ((*code).into(), (*name).into())))
                        .collect(),
                    |cx| get(cx).language.clone().into(),
                    |value, cx| update(cx, |p| p.language = value.to_string()),
                ),
            )),
            section(t!("settings.session"))
                .item(
                    switch(t!("settings.remember_tabs"), |p| p.restore_session, |p, v| p.restore_session = v)
                        .description(t!("settings.remember_tabs_detail").to_string()),
                )
                .footer(|_, _| t!("settings.session_footer").to_string()),
        ]);

        let editor = SettingPage::new(t!("settings.editor")).icon(IconName::SquareTerminal).groups([
            section(t!("settings.font"))
                .item(SettingItem::new(
                    t!("settings.font"),
                    SettingField::scrollable_dropdown(
                        fonts,
                        |cx| get(cx).font_family.clone().into(),
                        |value, cx| update(cx, |p| p.font_family = value.to_string()),
                    ),
                ))
                .item(number(t!("settings.font_size"), (8., 36., 1.), |p| p.font_size as f64, |p, v| p.font_size = v as f32))
                .footer(|_, _| t!("settings.font_footer").to_string()),
            section(t!("settings.indentation"))
                .item(number(t!("settings.tab_width"), (1., 16., 1.), |p| p.tab_width as f64, |p, v| p.tab_width = v as u8))
                .item(switch(t!("settings.insert_spaces"), |p| p.insert_spaces, |p, v| p.insert_spaces = v)),
            section(t!("settings.display"))
                .item(switch(t!("settings.line_numbers"), |p| p.show_line_numbers, |p, v| p.show_line_numbers = v))
                .item(switch(t!("settings.indent_guides"), |p| p.show_indent_guides, |p, v| p.show_indent_guides = v))
                .item(switch(t!("settings.wrap_lines"), |p| p.wrap_lines, |p, v| p.wrap_lines = v)),
        ]);

        let appearance = SettingPage::new(t!("settings.appearance")).icon(IconName::Palette).groups([
            section(t!("settings.theme"))
                .item(SettingItem::new(
                    t!("settings.mode"),
                    SettingField::dropdown(
                        options([
                            ("system", t!("settings.mode_system")),
                            ("light", t!("settings.mode_light")),
                            ("dark", t!("settings.mode_dark")),
                        ]),
                        |cx| {
                            match get(cx).theme {
                                ThemePreference::System => "system",
                                ThemePreference::Light => "light",
                                ThemePreference::Dark => "dark",
                            }
                            .into()
                        },
                        |value, cx| {
                            let theme = match value.as_ref() {
                                "light" => ThemePreference::Light,
                                "dark" => ThemePreference::Dark,
                                _ => ThemePreference::System,
                            };
                            update(cx, |p| p.theme = theme);
                        },
                    ),
                ))
                .item(color_scheme_picker(cx)),
            section(t!("settings.custom_themes"))
                .item(
                    SettingItem::new(
                        t!("settings.themes_folder"),
                        SettingField::render(|options, _, _| {
                            Button::new("open-themes-folder")
                                .outline()
                                .with_size(options.size())
                                .label(t!("settings.open_folder"))
                                .on_click(|_, _, cx| {
                                    if let Some(dir) = themes::dir() {
                                        let _ = std::fs::create_dir_all(&dir);
                                        cx.open_with_system(&dir);
                                    }
                                })
                        }),
                    )
                    .description(folder_description(themes::dir())),
                )
                .footer(|_, _| t!("settings.themes_footer").to_string()),
        ]);

        let new_documents = SettingPage::new(t!("settings.new_documents")).icon(IconName::FileText).groups([
            section(t!("settings.format"))
                .item(SettingItem::new(
                    t!("settings.line_endings"),
                    SettingField::dropdown(
                        options([
                            ("native", t!("settings.line_endings_native")),
                            ("lf", t!("settings.line_endings_lf")),
                            ("crlf", t!("settings.line_endings_crlf")),
                            ("cr", t!("settings.line_endings_cr")),
                        ]),
                        |cx| {
                            match get(cx).new_line_ending {
                                LineEndingPreference::Native => "native",
                                LineEndingPreference::Lf => "lf",
                                LineEndingPreference::CrLf => "crlf",
                                LineEndingPreference::Cr => "cr",
                            }
                            .into()
                        },
                        |value, cx| {
                            let line_ending = match value.as_ref() {
                                "lf" => LineEndingPreference::Lf,
                                "crlf" => LineEndingPreference::CrLf,
                                "cr" => LineEndingPreference::Cr,
                                _ => LineEndingPreference::Native,
                            };
                            update(cx, |p| p.new_line_ending = line_ending);
                        },
                    ),
                ))
                .item(SettingItem::new(
                    t!("settings.encoding"),
                    SettingField::dropdown(
                        options([("utf8", "UTF-8".into()), ("utf8-bom", t!("settings.utf8_bom"))]),
                        |cx| if get(cx).new_utf8_bom { "utf8-bom".into() } else { "utf8".into() },
                        |value, cx| update(cx, |p| p.new_utf8_bom = value.as_ref() == "utf8-bom"),
                    ),
                ))
                .footer(|_, _| t!("settings.format_footer").to_string()),
        ]);

        let logs = SettingPage::new(t!("settings.logs")).icon(IconName::Eye).groups([
            section(t!("settings.tail"))
                .item(
                    switch(t!("settings.color_levels"), |p| p.color_log_levels, |p, v| p.color_log_levels = v)
                        .description(t!("settings.color_levels_detail").to_string()),
                )
                .item(
                    SettingItem::new(
                        t!("settings.log_highlighting"),
                        SettingField::render(|options, _, _| {
                            Button::new("configure-log-highlighting")
                                .outline()
                                .with_size(options.size())
                                .label(t!("settings.configure"))
                                .on_click(|_, window, cx| log_highlight::editor::open(window, cx))
                        }),
                    )
                    .description(t!("settings.log_highlighting_detail").to_string()),
                )
                .item(
                    number(t!("settings.max_lines"), (10_000., 2_000_000., 10_000.), |p| p.log_max_lines as f64, |p, v| {
                        p.log_max_lines = v as u32
                    })
                    .description(t!("settings.max_lines_detail").to_string()),
                )
                .item(
                    number(t!("settings.initial_kib"), (64., 65_536., 512.), |p| p.log_initial_kib as f64, |p, v| {
                        p.log_initial_kib = v as u32
                    })
                    .description(t!("settings.initial_kib_detail").to_string()),
                ),
        ]);

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                Settings::new("oxypad-settings").pages([general, editor, appearance, new_documents, logs]),
            )
    }
}

fn folder_description(dir: Option<PathBuf>) -> String {
    dir.map_or_else(|| t!("settings.unavailable").to_string(), |dir| dir.display().to_string())
}
