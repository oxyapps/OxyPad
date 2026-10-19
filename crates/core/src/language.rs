use std::path::Path;

/// Returns the highlighter language name for a file, or `"plaintext"` when the
/// file type is not recognized. The names match the Tree-sitter grammars
/// bundled with the editor component.
pub fn language_for_path(path: &Path) -> &'static str {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match file_name.as_str() {
        "makefile" | "gnumakefile" => return "make",
        "cmakelists.txt" => return "cmake",
        "cargo.lock" => return "toml",
        ".bashrc" | ".bash_profile" | ".zshrc" | ".zprofile" | ".profile" => return "bash",
        _ => {}
    }

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "rs" => "rust",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "swift" => "swift",
        "zig" => "zig",
        "py" | "pyw" => "python",
        "rb" => "ruby",
        "php" | "phtml" => "php",
        "lua" => "lua",
        "ex" | "exs" => "elixir",
        "sh" | "bash" | "zsh" => "bash",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "html" | "htm" | "xhtml" => "html",
        "css" | "scss" => "css",
        "svelte" => "svelte",
        "astro" => "astro",
        "json" | "jsonc" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" | "mdx" => "markdown",
        "sql" => "sql",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "diff" | "patch" => "diff",
        "cmake" => "cmake",
        "mk" => "make",
        "erb" => "erb",
        "ejs" => "ejs",
        _ => "plaintext",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_and_file_name() {
        assert_eq!(language_for_path(Path::new("src/main.RS")), "rust");
        assert_eq!(language_for_path(Path::new("/x/Makefile")), "make");
        assert_eq!(language_for_path(Path::new("app.log")), "plaintext");
        assert_eq!(language_for_path(Path::new("no_extension")), "plaintext");
    }
}
