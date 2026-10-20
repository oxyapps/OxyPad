use super::*;

/// The highlight name of each run's text, skipping unstyled runs.
fn names(language: &str, code: &str) -> Vec<(&'static str, String)> {
    let text = StrText::new(code);
    let mut highlighter = Highlighter::new(language).expect("a known language");
    highlighter.reset(&text);
    highlighter.lex_to(&text, code.len());
    highlighter
        .styles(&text, 0..code.len())
        .into_iter()
        .filter_map(|(range, name)| Some((name?, code[range].to_owned())))
        .collect()
}

fn has(runs: &[(&str, String)], name: &str, text: &str) -> bool {
    runs.iter().any(|(n, t)| *n == name && t.trim() == text)
}

#[test]
fn every_language_has_a_working_lexer() {
    for language in LANGUAGES {
        let mut highlighter = Highlighter::new(language.name).unwrap_or_else(|| panic!("{}", language.name));
        let text = StrText::new("x = 1 // \"y\"\n");
        highlighter.reset(&text);
        highlighter.lex_to(&text, usize::MAX);
        assert_eq!(highlighter.styled_end(), text.len(), "{}", language.name);
    }
}

#[test]
fn highlights_rust() {
    let runs = names("rust", "// note\nfn main() { let x = \"hi\"; 42 }\n");
    assert!(has(&runs, "comment", "// note"), "{runs:?}");
    assert!(has(&runs, "keyword", "fn"), "{runs:?}");
    assert!(has(&runs, "string", "\"hi\""), "{runs:?}");
    assert!(has(&runs, "number", "42"), "{runs:?}");
}

#[test]
fn highlights_c_with_scite_keywords() {
    let runs = names("c", "#include <stdio.h>\nint main(void) { return 0; }\n");
    assert!(runs.iter().any(|(n, t)| *n == "preproc" && t.starts_with("#include")), "{runs:?}");
    assert!(has(&runs, "keyword", "int"), "{runs:?}");
    assert!(has(&runs, "keyword", "return"), "{runs:?}");
}

#[test]
fn highlights_keywords_added_for_kotlin() {
    let runs = names("kotlin", "fun main() { val x = 1 }\n");
    assert!(has(&runs, "keyword", "fun"), "{runs:?}");
    assert!(has(&runs, "keyword", "val"), "{runs:?}");
}

#[test]
fn highlights_scripts_inside_html() {
    let runs = names("html", "<p class=\"a\">hi</p>\n<script>\nvar x = 'y';\n</script>\n");
    assert!(runs.iter().any(|(n, t)| *n == "tag" && t.contains('p')), "{runs:?}");
    assert!(has(&runs, "attribute", "class"), "{runs:?}");
    assert!(has(&runs, "keyword", "var"), "{runs:?}");
    assert!(has(&runs, "string", "'y'"), "{runs:?}");
}

#[test]
fn folds_braces_from_the_header_to_the_closing_line() {
    let code = "fn a() {\n    let x = 1;\n    let y = 2;\n}\nfn b() {}\n";
    let text = StrText::new(code);
    let mut highlighter = Highlighter::new("rust").unwrap();
    highlighter.reset(&text);
    highlighter.lex_to(&text, code.len());
    assert_eq!(highlighter.folds(&text), vec![0..3]);
}

#[test]
fn folds_python_by_indentation() {
    let code = "def a():\n    x = 1\n    return x\n\ny = 2\n";
    let text = StrText::new(code);
    let mut highlighter = Highlighter::new("python").unwrap();
    highlighter.reset(&text);
    highlighter.lex_to(&text, code.len());
    assert_eq!(highlighter.folds(&text).first(), Some(&(0..2)));
}

/// Applies `replacement` at `at..at + removed` to `before` and checks that
/// the incremental result equals lexing the new text from scratch.
fn check_edit(language: &str, before: &str, at: usize, removed: usize, replacement: &str) {
    let old_text = StrText::new(before);
    let mut highlighter = Highlighter::new(language).unwrap();
    highlighter.reset(&old_text);
    highlighter.lex_to(&old_text, before.len());

    let after = format!("{}{}{}", &before[..at], replacement, &before[at + removed..]);
    let new_text = StrText::new(&after);
    let edit = Edit {
        start: at,
        old_end: at + removed,
        new_end: at + replacement.len(),
        start_line: old_text.line_of(at),
        old_end_line: old_text.line_of(at + removed),
        new_end_line: new_text.line_of(at + replacement.len()),
    };
    highlighter.edit(&new_text, edit);
    highlighter.lex_to(&new_text, after.len());

    let mut fresh = Highlighter::new(language).unwrap();
    fresh.reset(&new_text);
    fresh.lex_to(&new_text, after.len());
    assert_eq!(highlighter.styles(&new_text, 0..after.len()), fresh.styles(&new_text, 0..after.len()));
    assert_eq!(highlighter.folds(&new_text), fresh.folds(&new_text));
}

#[test]
fn incremental_edits_match_a_full_lex() {
    let body: String = (0..3000).map(|i| format!("fn f{i}() {{\n    let s = \"{i}\"; // c\n}}\n")).collect();
    // Typing inside a function, far from the end.
    check_edit("rust", &body, 100, 0, "x");
    // Opening a block comment: everything after changes.
    check_edit("rust", &body, 100, 0, "/*");
    // Opening a string.
    check_edit("rust", &body, 200, 0, "\"");
    // Deleting lines.
    check_edit("rust", &body, 50, 400, "");
    // Inserting lines with a new block.
    check_edit("rust", &body, 1000, 0, "\nfn new() {\n    1\n}\n");
}

#[test]
fn incremental_edit_after_a_lazy_lex() {
    let body: String = (0..3000).map(|i| format!("x{i} = \"{i}\"  # c\n")).collect();
    let text = StrText::new(&body);
    let mut highlighter = Highlighter::new("python").unwrap();
    highlighter.reset(&text);
    // Only the start was lexed, as when just the top of a file is on screen.
    highlighter.lex_to(&text, 100);
    assert!(highlighter.styled_end() < body.len());
    check_edit("python", &body, 10, 0, "'''");
}

#[test]
fn runs_do_not_split_characters() {
    let code = "let s = \"ção\"; // é\n";
    let text = StrText::new(code);
    let mut highlighter = Highlighter::new("rust").unwrap();
    highlighter.reset(&text);
    highlighter.lex_to(&text, code.len());
    let runs = highlighter.styles(&text, 0..code.len());
    for (range, _) in &runs {
        assert!(code.is_char_boundary(range.start) && code.is_char_boundary(range.end), "{range:?}");
    }
    assert_eq!(runs.last().unwrap().0.end, code.len());
}

#[test]
fn unlexed_text_is_one_plain_run() {
    let code = "a = 1\n".repeat(10_000);
    let text = StrText::new(&code);
    let mut highlighter = Highlighter::new("python").unwrap();
    highlighter.reset(&text);
    let runs = highlighter.styles(&text, 100..200);
    assert_eq!(runs, vec![(100..200, None)]);
}

#[test]
fn every_detected_language_has_a_lexer() {
    // One file for each language `oxypad-core` detects.
    let files = [
        "a.rs", "a.c", "a.cpp", "a.cs", "a.go", "a.java", "a.kt", "a.scala", "a.swift", "a.zig", "a.py",
        "a.rb", "a.php", "a.lua", "a.ex", "a.sh", "a.js", "a.ts", "a.tsx", "a.html", "a.xml", "a.ini",
        "a.bat", "a.ps1", "a.pl", "a.css", "a.svelte", "a.astro", "a.json", "a.toml", "a.yaml", "a.md",
        "a.sql", "a.graphql", "a.proto", "a.diff", "a.cmake", "Makefile", "a.erb", "a.ejs",
    ];
    for file in files {
        let language = oxypad_core::language_for_path(std::path::Path::new(file));
        assert!(is_supported(language), "{file}: {language}");
    }
}

#[test]
fn unknown_languages_have_no_highlighter() {
    assert!(Highlighter::new("plaintext").is_none());
    assert!(Highlighter::new("klingon").is_none());
}

#[test]
fn highlights_yaml() {
    let code = "# build\njobs:\n  build:\n    runs-on: ubuntu-latest\n    retries: 3\n    cache: true\n    base: &base x\n";
    let runs = names("yaml", code);
    assert!(has(&runs, "comment", "# build"), "{runs:?}");
    assert!(has(&runs, "tag", "runs-on"), "{runs:?}");
    assert!(has(&runs, "string", "ubuntu-latest"), "{runs:?}");
    assert!(has(&runs, "number", "3"), "{runs:?}");
    assert!(has(&runs, "boolean", "true"), "{runs:?}");
}
