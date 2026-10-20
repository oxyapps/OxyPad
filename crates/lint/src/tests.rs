use std::path::Path;

use super::*;

fn at(problem: Option<Problem>) -> (usize, usize) {
    let problem = problem.expect("a problem");
    (problem.line, problem.column)
}

#[test]
fn valid_files_pass() {
    assert_eq!(check("json", None, r#"{"a": [1, 2], "b": "ç"}"#), None);
    assert_eq!(check("yaml", None, "jobs:\n  build:\n    runs-on: ubuntu-latest\n---\n- a\n"), None);
    assert_eq!(check("toml", None, "[package]\nname = \"x\"\n"), None);
    assert_eq!(check("xml", None, "<?xml version=\"1.0\"?>\n<a b=\"1\"><c/></a>\n"), None);
    assert_eq!(check("rust", None, "fn main( {"), None);
}

#[test]
fn json_errors_point_at_the_mistake() {
    let problem = check("json", None, "{\n  \"a\": 1,\n  \"b\" 2\n}\n").unwrap();
    assert_eq!((problem.line, problem.column), (2, 6));
    assert_eq!(problem.message, "Expected `:`");
    // Columns count characters, not bytes.
    assert_eq!(at(check("json", None, "{\"é\": x}")), (0, 6));
}

#[test]
fn jsonc_allows_comments_and_trailing_commas() {
    let jsonc = "{\n  // comment\n  \"a\": [1, 2,],\n  /* more */ \"b\": 1,\n}\n";
    assert_eq!(check("json", None, jsonc), None);
    assert_eq!(check("json", Some(Path::new("/p/tsconfig.json")), "{\"a\": 1,}"), None);
    // Plain JSON keeps the strict rules.
    assert!(check("json", Some(Path::new("/p/package.json")), "{\"a\": 1,}").is_some());
    // Comment markers inside strings are text.
    assert_eq!(check("json", None, r#"{"url": "http://x/*y*/", "q": "a\"//b"}"#), None);
    // An error after a comment is still placed right.
    assert_eq!(at(check("json", None, "{ /* é */ \"a\": }")), (0, 15));
}

#[test]
fn yaml_errors_point_at_the_mistake() {
    let problem = check("yaml", None, "a: 1\nb: [1, 2\nc: 3\n").unwrap();
    assert!(problem.line >= 1, "{problem:?}");
    let tab = check("yaml", None, "a:\n\tb: 1\n").unwrap();
    assert_eq!(tab.line, 1, "{tab:?}");
}

#[test]
fn toml_errors_point_at_the_mistake() {
    assert_eq!(at(check("toml", None, "a = 1\nb = \n")), (1, 4));
    assert_eq!(at(check("toml", None, "[x]\n[x]\n")).0, 1);
}

#[test]
fn xml_errors_point_at_the_mistake() {
    let problem = check("xml", None, "<a>\n  <b></c>\n</a>\n").unwrap();
    assert_eq!(problem.line, 1, "{problem:?}");
    assert!(!problem.message.contains(" at "), "{problem:?}");
}

