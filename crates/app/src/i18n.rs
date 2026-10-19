//! The user interface language. The texts live in `locales/<language>.yml`
//! (see TRANSLATING.md). OxyPad follows the system's preferred languages
//! unless one is chosen in Settings, and falls back to English.

use gpui_kit::App;

use crate::{menus, preferences};

/// Languages with a translation: code (the locale file's name) and the
/// language's own name, shown untranslated in Settings.
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("pt-BR", "Português (Brasil)")];

/// The language to show for the Settings choice `choice` (empty for the
/// system's).
pub fn resolve(choice: &str) -> &'static str {
    if let Some((code, _)) = LANGUAGES.iter().find(|(code, _)| *code == choice) {
        return code;
    }
    sys_locale::get_locales().find_map(|locale| matching(&locale)).unwrap_or("en")
}

/// The translation for a system locale such as `pt-BR`, `pt_PT` or `en-US`:
/// the same locale, else the same language in another region.
fn matching(locale: &str) -> Option<&'static str> {
    let locale = locale.replace('_', "-");
    let language = |code: &str| code.split('-').next().unwrap_or_default().to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(&locale))
        .or_else(|| LANGUAGES.iter().find(|(code, _)| language(code) == language(&locale)))
        .map(|(code, _)| *code)
}

/// Sets the language at startup, before anything is built.
pub fn init(cx: &App) {
    // gpui-kit's components look up their texts in our files first (the
    // `gpui_component` section), so OxyPad's languages cover them too.
    gpui_kit::component::_rust_i18n_extend(crate::_rust_i18n_backend(), "gpui_component");
    rust_i18n::set_locale(resolve(&preferences::get(cx).language));
}

/// Switches the interface to the language in the preferences, rebuilding
/// the menus. Texts drawn on each render (everything else) follow on the
/// next refresh.
pub fn apply(cx: &mut App) {
    let code = resolve(&preferences::get(cx).language);
    if &*rust_i18n::locale() == code {
        return;
    }
    // Shared with gpui-kit's own texts (dialog buttons, context menus).
    rust_i18n::set_locale(code);
    menus::reload(cx);
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_gpui_kit_texts() {
        gpui_kit::component::_rust_i18n_extend(crate::_rust_i18n_backend(), "gpui_component");
        let cut = |locale| gpui_kit::component::_rust_i18n_translate(locale, "Input.Cut").into_owned();
        assert_eq!(cut("pt-BR"), "Recortar");
        assert_eq!(cut("en"), "Cut");
    }

    #[test]
    fn matches_system_locales() {
        assert_eq!(matching("pt-BR"), Some("pt-BR"));
        assert_eq!(matching("pt_PT"), Some("pt-BR"));
        assert_eq!(matching("en-GB"), Some("en"));
        assert_eq!(matching("de-DE"), None);
        assert_eq!(resolve("pt-BR"), "pt-BR");
    }

    /// Dotted keys of a locale file. The files are plain two-space-indented
    /// `key: text` maps, which is all this reads.
    fn keys(file: &str) -> Vec<String> {
        let mut path: Vec<String> = Vec::new();
        let mut keys = Vec::new();
        for line in file.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("_version") {
                continue;
            }
            let depth = (line.len() - trimmed.len()) / 2;
            let (key, value) = trimmed.split_once(':').expect("key: value");
            path.truncate(depth);
            path.push(key.to_string());
            if !value.trim().is_empty() {
                keys.push(path.join("."));
            }
        }
        keys.sort();
        keys
    }

    #[test]
    fn every_language_has_every_text() {
        let english = keys(include_str!("../locales/en.yml"));
        assert!(english.len() > 100);
        for (code, _) in LANGUAGES {
            let file = std::fs::read_to_string(format!("{}/locales/{code}.yml", env!("CARGO_MANIFEST_DIR"))).unwrap();
            assert_eq!(keys(&file), english, "{code}.yml must have exactly the keys of en.yml");
        }
    }
}
