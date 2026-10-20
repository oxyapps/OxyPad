//! Languages: the grammars built into the app plus those installed from the
//! additional language pack.
//!
//! The pack is not downloaded by the app. Users get it from the releases page
//! and copy its folders into [`dir`]; each folder holds a `language.json`, a
//! grammar compiled as a shared library and its highlight queries (see
//! `crates/language-pack`). They are loaded at startup and on Reload.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use gpui_kit::{
    App, BorrowAppContext, Global, SharedString,
    component::highlighter::{LanguageConfig, LanguageRegistry},
};
use libloading::{Library, Symbol};
use oxypad_core::language_for_path;
use serde::Deserialize;

pub const RELEASES_URL: &str = "https://github.com/oxyapps/OxyPad/releases";

/// Languages shipped in the pack instead of the app: name and display name.
pub const PACK_LANGUAGES: &[(&str, &str)] = &[
    ("csharp", "C#"),
    ("elixir", "Elixir"),
    ("kotlin", "Kotlin"),
    ("php", "PHP"),
    ("ruby", "Ruby"),
    ("scala", "Scala"),
    ("swift", "Swift"),
    ("zig", "Zig"),
];

#[derive(Deserialize)]
struct LanguageManifest {
    name: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    file_names: Vec<String>,
    library: String,
    symbol: String,
    #[serde(default)]
    injection_languages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InstallStatus {
    Installed,
    Failed(SharedString),
}

/// A language found in the languages folder.
pub struct InstalledLanguage {
    pub name: SharedString,
    pub status: InstallStatus,
    extensions: Vec<String>,
    file_names: Vec<String>,
}

pub struct Languages {
    dir: Option<PathBuf>,
    installed: Vec<InstalledLanguage>,
    /// Loaded grammars must outlive every parser that uses them, so their
    /// libraries are never unloaded.
    libraries: Vec<Library>,
}

impl Global for Languages {}

/// Loads the installed languages. Call once at startup.
pub fn init(cx: &mut App) {
    let dir = oxypad_session::data_dir().ok().map(|dir| dir.join("code-languages"));
    cx.set_global(Languages { dir, installed: Vec::new(), libraries: Vec::new() });
    reload(cx);
}

/// The folder users copy the language pack into.
pub fn dir(cx: &App) -> Option<PathBuf> {
    cx.global::<Languages>().dir.clone()
}

pub fn installed(cx: &App) -> &[InstalledLanguage] {
    &cx.global::<Languages>().installed
}

/// Scans the languages folder and loads languages not loaded yet. Observers
/// of [`Languages`] are notified, so open tabs pick up new grammars.
pub fn reload(cx: &mut App) {
    let Some(dir) = dir(cx) else { return };
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return, // No pack installed yet.
    };
    let mut folders: Vec<PathBuf> =
        entries.flatten().map(|entry| entry.path()).filter(|path| path.join("language.json").is_file()).collect();
    folders.sort();

    cx.update_global::<Languages, _>(|languages, _| {
        for folder in folders {
            let already_loaded = |name: &str| {
                languages
                    .installed
                    .iter()
                    .any(|language| language.name == name && language.status == InstallStatus::Installed)
            };
            match load(&folder) {
                Ok((manifest, _)) if already_loaded(&manifest.name) => {}
                Ok((manifest, library)) => {
                    languages.installed.retain(|language| language.name != manifest.name.as_str());
                    languages.libraries.push(library);
                    languages.installed.push(InstalledLanguage {
                        name: manifest.name.into(),
                        status: InstallStatus::Installed,
                        extensions: manifest.extensions,
                        file_names: manifest.file_names,
                    });
                }
                Err(err) => {
                    let name: SharedString =
                        folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into();
                    if already_loaded(&name) {
                        continue;
                    }
                    eprintln!("oxypad: could not load the language in {}: {err:#}", folder.display());
                    languages.installed.retain(|language| language.name != name);
                    languages.installed.push(InstalledLanguage {
                        name,
                        status: InstallStatus::Failed(format!("{err:#}").into()),
                        extensions: Vec::new(),
                        file_names: Vec::new(),
                    });
                }
            }
        }
    });
}

/// Loads one language folder and registers its grammar with the editor.
fn load(folder: &Path) -> Result<(LanguageManifest, Library)> {
    let manifest: LanguageManifest =
        serde_json::from_slice(&fs::read(folder.join("language.json"))?).context("reading language.json")?;
    let path = folder.join(library_file(&manifest.library));
    if !path.is_file() {
        bail!("{} is missing; is this the pack for this platform?", path.display());
    }

    // SAFETY: the library is a Tree-sitter grammar from the language pack.
    // Loading runs no code besides the library's initializers, and the
    // symbol has the signature every generated Tree-sitter parser exports.
    let library = unsafe { Library::new(&path) }.with_context(|| format!("loading {}", path.display()))?;
    let language = unsafe {
        let constructor: Symbol<unsafe extern "C" fn() -> *const ()> = library
            .get(manifest.symbol.as_bytes())
            .with_context(|| format!("{} has no {}", path.display(), manifest.symbol))?;
        tree_sitter::Language::new(tree_sitter_language::LanguageFn::from_raw(*constructor))
    };

    let abi = language.abi_version();
    let supported = tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION;
    if !supported.contains(&abi) {
        bail!(
            "built for a different OxyPad (grammar ABI {abi}, this version supports {}–{}); \
             download the pack that matches this version",
            supported.start(),
            supported.end()
        );
    }

    let query = |kind: &str| fs::read_to_string(folder.join(format!("{kind}.scm"))).unwrap_or_default();
    let config = LanguageConfig::new(
        manifest.name.clone(),
        language,
        manifest.injection_languages.iter().map(|name| name.clone().into()).collect(),
        &query("highlights"),
        &query("injections"),
        &query("locals"),
    );
    LanguageRegistry::singleton().register(&manifest.name, &config);
    Ok((manifest, library))
}

fn library_file(base: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{base}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{base}.dylib")
    } else {
        format!("lib{base}.so")
    }
}

/// The language of a file: installed languages first (they may add new
/// extensions), then the built-in table.
pub fn detect(path: Option<&Path>, cx: &App) -> SharedString {
    let Some(path) = path else { return "plaintext".into() };
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or_default();
    let installed = installed(cx).iter().filter(|language| language.status == InstallStatus::Installed);
    for language in installed {
        let by_name = language.file_names.iter().any(|name| name == file_name);
        let by_extension = language.extensions.iter().any(|ext| ext.eq_ignore_ascii_case(extension));
        if by_name || by_extension {
            return language.name.clone();
        }
    }
    language_for_path(path).into()
}

/// Whether the editor can highlight `language` right now.
pub fn is_available(language: &str) -> bool {
    language == "plaintext"
        || LanguageRegistry::singleton().language(language).is_some_and(|config| config.has_grammar())
}

/// Whether `language` is one the language pack provides.
pub fn is_in_pack(language: &str) -> bool {
    PACK_LANGUAGES.iter().any(|(name, _)| *name == language)
}

pub fn display_name(language: &str) -> String {
    PACK_LANGUAGES
        .iter()
        .find(|(name, _)| *name == language)
        .map(|(_, display)| display.to_string())
        .unwrap_or_else(|| {
            let mut chars = language.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        })
}
