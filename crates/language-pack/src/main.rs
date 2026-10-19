//! Builds OxyPad's additional language pack for the current platform.
//!
//! ```text
//! cargo run -p oxypad-language-pack -- [OUT_DIR]
//! ```
//!
//! Each grammar's C sources are compiled into a shared library and laid out
//! as OxyPad expects them in its `languages` folder:
//!
//! ```text
//! OUT_DIR/oxypad-languages-<version>-<os>-<arch>/
//!   manifest.json
//!   README.txt
//!   kotlin/
//!     language.json         name, extensions, library, symbol
//!     libtree-sitter-kotlin.dylib   (.so on Linux, tree-sitter-kotlin.dll on Windows)
//!     highlights.scm        and injections.scm / locals.scm when the grammar has them
//! ```
//!
//! The C compiler is `cc` (or `$CC`) on macOS and Linux and `cl` on Windows,
//! which needs a Visual Studio developer prompt. On macOS the libraries are
//! universal (Apple silicon and Intel). Extra flags go in `$OXYPAD_PACK_CFLAGS`.
//! To build for another architecture, point the compiler at it and set
//! `$OXYPAD_PACK_ARCH` (e.g. `aarch64`) so the pack is named after it.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde_json::json;

/// A grammar in the pack.
struct Spec {
    /// Language name, as the editor knows it.
    name: &'static str,
    display_name: &'static str,
    package: &'static str,
    /// Directory of `parser.c` (and `scanner.c`) inside the crate.
    src: &'static str,
    symbol: &'static str,
    extensions: &'static [&'static str],
    file_names: &'static [&'static str],
    /// Languages embedded inside this one (for injections).
    injection_languages: &'static [&'static str],
}

const SPECS: &[Spec] = &[
    Spec {
        name: "csharp",
        display_name: "C#",
        package: "tree-sitter-c-sharp",
        src: "src",
        symbol: "tree_sitter_c_sharp",
        extensions: &["cs", "csx"],
        file_names: &[],
        injection_languages: &[],
    },
    Spec {
        name: "elixir",
        display_name: "Elixir",
        package: "tree-sitter-elixir",
        src: "src",
        symbol: "tree_sitter_elixir",
        extensions: &["ex", "exs"],
        file_names: &["mix.lock"],
        injection_languages: &[],
    },
    Spec {
        name: "kotlin",
        display_name: "Kotlin",
        package: "tree-sitter-kotlin-sg",
        src: "src",
        symbol: "tree_sitter_kotlin",
        extensions: &["kt", "kts"],
        file_names: &[],
        injection_languages: &[],
    },
    Spec {
        name: "php",
        display_name: "PHP",
        package: "tree-sitter-php",
        src: "php/src",
        symbol: "tree_sitter_php",
        extensions: &["php", "phtml", "php3", "php4", "php5"],
        file_names: &[],
        injection_languages: &["html"],
    },
    Spec {
        name: "ruby",
        display_name: "Ruby",
        package: "tree-sitter-ruby",
        src: "src",
        symbol: "tree_sitter_ruby",
        extensions: &["rb", "rake", "gemspec", "ru"],
        file_names: &["Gemfile", "Rakefile", "Podfile", "Fastfile"],
        injection_languages: &[],
    },
    Spec {
        name: "scala",
        display_name: "Scala",
        package: "tree-sitter-scala",
        src: "src",
        symbol: "tree_sitter_scala",
        extensions: &["scala", "sc", "sbt"],
        file_names: &[],
        injection_languages: &[],
    },
    Spec {
        name: "swift",
        display_name: "Swift",
        package: "tree-sitter-swift",
        src: "src",
        symbol: "tree_sitter_swift",
        extensions: &["swift"],
        file_names: &[],
        injection_languages: &[],
    },
    Spec {
        name: "zig",
        display_name: "Zig",
        package: "tree-sitter-zig",
        src: "src",
        symbol: "tree_sitter_zig",
        extensions: &["zig", "zon"],
        file_names: &[],
        injection_languages: &[],
    },
];

const QUERY_KINDS: &[&str] = &["highlights", "injections", "locals"];

fn main() -> Result<()> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let out_root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target/language-pack"));

    let packages = package_dirs(&workspace)?;
    let component = packages
        .get("gpui-component")
        .context("gpui-component is not in the workspace")?
        .join("src/highlighter/languages");

    let version = env!("CARGO_PKG_VERSION");
    let platform = platform_label();
    let pack = out_root.join(format!("oxypad-languages-{version}-{platform}"));
    if pack.exists() {
        fs::remove_dir_all(&pack)?;
    }
    fs::create_dir_all(&pack)?;

    let mut names = Vec::new();
    for spec in SPECS {
        let crate_dir = packages.get(spec.package).with_context(|| format!("{} not found", spec.package))?;
        let dir = pack.join(spec.name);
        fs::create_dir_all(&dir)?;

        let library = format!("tree-sitter-{}", spec.name);
        let output = dir.join(library_file(&library));
        compile(&crate_dir.join(spec.src), crate_dir, &output)
            .with_context(|| format!("compiling {}", spec.display_name))?;

        // gpui-component's own queries win: they are tuned to its theme.
        let mut queries = Vec::new();
        for kind in QUERY_KINDS {
            let file = format!("{kind}.scm");
            let source = [component.join(spec.name).join(&file), crate_dir.join("queries").join(&file)]
                .into_iter()
                .find(|path| path.is_file());
            if let Some(source) = source {
                fs::copy(&source, dir.join(&file))?;
                queries.push(*kind);
            }
        }

        let manifest = json!({
            "name": spec.name,
            "display_name": spec.display_name,
            "extensions": spec.extensions,
            "file_names": spec.file_names,
            "library": library,
            "symbol": spec.symbol,
            "injection_languages": spec.injection_languages,
        });
        fs::write(dir.join("language.json"), serde_json::to_string_pretty(&manifest)? + "\n")?;

        let size = fs::metadata(&output)?.len();
        println!("{:<8} {:>6.1} MB  queries: {}", spec.name, size as f64 / 1_048_576.0, queries.join(", "));
        names.push(spec.name);
    }

    let manifest = json!({ "format": 1, "oxypad_version": version, "platform": platform, "languages": names });
    fs::write(pack.join("manifest.json"), serde_json::to_string_pretty(&manifest)? + "\n")?;
    fs::write(pack.join("README.txt"), readme(version, &platform))?;
    println!("Built {}", pack.display());
    Ok(())
}

/// Source directory of every package in the workspace's dependency graph.
fn package_dirs(workspace: &Path) -> Result<HashMap<String, PathBuf>> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(workspace.join("Cargo.toml"))
        .output()
        .context("running cargo metadata")?;
    if !output.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut dirs = HashMap::new();
    for package in metadata["packages"].as_array().context("no packages")? {
        let (Some(name), Some(manifest)) = (package["name"].as_str(), package["manifest_path"].as_str()) else {
            continue;
        };
        if let Some(dir) = Path::new(manifest).parent() {
            dirs.insert(name.to_string(), dir.to_path_buf());
        }
    }
    Ok(dirs)
}

/// `<os>-<arch>` of the pack. `$OXYPAD_PACK_ARCH` names the architecture when
/// the compiler targets another one (`cl` from an x64-to-ARM64 prompt).
fn platform_label() -> String {
    let arch = std::env::var("OXYPAD_PACK_ARCH").ok().filter(|arch| !arch.is_empty()).unwrap_or_else(|| {
        if cfg!(target_os = "macos") { "universal" } else { std::env::consts::ARCH }.to_string()
    });
    format!("{}-{arch}", std::env::consts::OS)
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

/// Compiles `parser.c` (and `scanner.c`, if any) in `src` into a shared
/// library at `output`.
fn compile(src: &Path, crate_dir: &Path, output: &Path) -> Result<()> {
    let mut sources = vec![src.join("parser.c")];
    if src.join("scanner.c").is_file() {
        sources.push(src.join("scanner.c"));
    }
    let extra: Vec<String> = std::env::var("OXYPAD_PACK_CFLAGS")
        .map(|flags| flags.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();

    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("cl");
        command.args(["/nologo", "/O2", "/LD", "/utf-8"]);
        command.arg(format!("/I{}", src.display())).arg(format!("/I{}", crate_dir.display()));
        command.args(&extra).args(&sources);
        command.arg(format!("/Fe{}", output.display()));
        // Keep object files out of the pack.
        command.arg(format!("/Fo{}\\", std::env::temp_dir().display()));
        command
    } else {
        let mut command = Command::new(std::env::var("CC").unwrap_or_else(|_| "cc".into()));
        command.args(["-shared", "-fPIC", "-O2", "-std=c11"]);
        if cfg!(target_os = "macos") {
            command.args(["-arch", "arm64", "-arch", "x86_64", "-mmacosx-version-min=11.0"]);
        }
        command.arg("-I").arg(src).arg("-I").arg(crate_dir);
        command.args(&extra).args(&sources).arg("-o").arg(output);
        command
    };
    let status = command.status().context("starting the C compiler")?;
    if !status.success() {
        bail!("the C compiler failed ({status})");
    }
    Ok(())
}

fn readme(version: &str, platform: &str) -> String {
    format!(
        "OxyPad additional language pack {version} ({platform})\n\
         \n\
         Adds syntax highlighting for: {}.\n\
         \n\
         To install, copy the language folders (kotlin, swift, ...) into OxyPad's\n\
         languages folder, then choose Reload Languages in Settings > Code Languages\n\
         (or restart OxyPad). Open Folder on that page opens it:\n\
         \n\
         \x20 macOS    ~/Library/Application Support/oxypad/languages\n\
         \x20 Windows  %LOCALAPPDATA%\\oxypad\\languages\n\
         \x20 Linux    ~/.local/share/oxypad/languages\n\
         \n\
         Use the pack that matches your OxyPad version and platform.\n",
        SPECS.iter().map(|spec| spec.display_name).collect::<Vec<_>>().join(", ")
    )
}
