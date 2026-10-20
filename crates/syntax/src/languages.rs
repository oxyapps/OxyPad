//! The languages OxyPad highlights: which Lexilla lexer each uses, with
//! which properties and keywords, and the highlight name of each style.
//!
//! Highlight names are those of the editor's color themes ("keyword",
//! "string", "comment.doc", …). Style numbers are Lexilla's `SCE_*`
//! constants (`vendor/lexilla/include/SciLexer.h`).

use crate::keywords as scite;

pub struct Language {
    /// The OxyPad language name ("rust", "cpp", …).
    pub name: &'static str,
    /// The Lexilla lexer ("cpp", "hypertext", …).
    pub lexer: &'static str,
    pub(crate) properties: &'static [(&'static str, &'static str)],
    /// Keyword lists, by the lexer's list index.
    pub(crate) keywords: &'static [&'static str],
    /// More words for the same lists, for languages SciTE does not cover.
    pub(crate) extra_keywords: &'static [&'static str],
    pub(crate) style: fn(u8) -> Option<&'static str>,
}

/// Set on every lexer: fold, and keep blank lines after a block out of it.
pub(crate) const COMMON_PROPERTIES: &[(&str, &str)] =
    &[("fold", "1"), ("fold.compact", "0"), ("fold.comment", "1"), ("fold.preprocessor", "1"), ("fold.html", "1")];

const CPP_PROPERTIES: &[(&str, &str)] =
    &[("lexer.cpp.track.preprocessor", "0"), ("lexer.cpp.update.preprocessor", "0"), ("lexer.cpp.escape.sequence", "1")];
const SCRIPT_PROPERTIES: &[(&str, &str)] = &[
    ("lexer.cpp.track.preprocessor", "0"),
    ("lexer.cpp.escape.sequence", "1"),
    ("lexer.cpp.backquoted.strings", "1"),
    ("lexer.cpp.allow.dollars", "1"),
];

const fn language(
    name: &'static str,
    lexer: &'static str,
    properties: &'static [(&'static str, &'static str)],
    keywords: &'static [&'static str],
    extra_keywords: &'static [&'static str],
    style: fn(u8) -> Option<&'static str>,
) -> Language {
    Language { name, lexer, properties, keywords, extra_keywords, style }
}

pub static LANGUAGES: &[Language] = &[
    language("astro", "hypertext", &[], scite::HTML, &[], hypertext),
    language("bash", "bash", &[], scite::BASH, &[], bash),
    language("batch", "batch", &[], scite::BATCH, &[], batch),
    language("c", "cpp", CPP_PROPERTIES, scite::C, &[], cpp),
    language("cmake", "cmake", &[], scite::CMAKE, &[], cmake),
    language("cpp", "cpp", CPP_PROPERTIES, scite::CPP, &[], cpp),
    language("csharp", "cpp", CPP_PROPERTIES, scite::CSHARP, &[], cpp),
    language("css", "css", &[("lexer.css.scss.language", "1")], scite::CSS, &[], css),
    language("diff", "diff", &[], &[], &[], diff),
    language("ejs", "hypertext", &[], scite::HTML, &[], hypertext),
    language("elixir", "ruby", &[], &[], &[ELIXIR], ruby),
    language("erb", "hypertext", &[], scite::HTML, &[], hypertext),
    language("go", "cpp", SCRIPT_PROPERTIES, scite::GO, &[], cpp),
    language("graphql", "cpp", SCRIPT_PROPERTIES, &[], &[GRAPHQL], cpp),
    language("html", "hypertext", &[], scite::HTML, &[], hypertext),
    language("ini", "props", &[], &[], &[], props),
    language("java", "cpp", CPP_PROPERTIES, scite::JAVA, &[], cpp),
    language("javascript", "cpp", SCRIPT_PROPERTIES, scite::JAVASCRIPT, &[], cpp),
    language("json", "json", &[("lexer.json.allow.comments", "1"), ("lexer.json.escape.sequence", "1")], scite::JSON, &[], json),
    language("kotlin", "cpp", SCRIPT_PROPERTIES, &[], &[KOTLIN], cpp),
    language("lua", "lua", &[], scite::LUA, &[], lua),
    language("make", "makefile", &[], &[], &[], make),
    language("markdown", "markdown", &[], &[], &[], markdown),
    language("perl", "perl", &[], scite::PERL, &[], perl),
    language("php", "hypertext", &[], scite::PHP, &[], hypertext),
    language("powershell", "powershell", &[], scite::POWERSHELL, &[], powershell),
    language("proto", "cpp", CPP_PROPERTIES, &[], &[PROTO], cpp),
    language("python", "python", &[], scite::PYTHON, &[], python),
    language("ruby", "ruby", &[], scite::RUBY, &[], ruby),
    language("rust", "rust", &[], scite::RUST, &[], rust),
    language("scala", "cpp", SCRIPT_PROPERTIES, &[], &[SCALA], cpp),
    language("sql", "sql", &[], scite::SQL, &[], sql),
    language("svelte", "hypertext", &[], scite::HTML, &[], hypertext),
    language("swift", "cpp", CPP_PROPERTIES, scite::SWIFT, &[], cpp),
    language("toml", "toml", &[], &[], &[TOML], toml),
    language("tsx", "cpp", SCRIPT_PROPERTIES, scite::JAVASCRIPT, &[TYPESCRIPT], cpp),
    language("typescript", "cpp", SCRIPT_PROPERTIES, scite::JAVASCRIPT, &[TYPESCRIPT], cpp),
    language("xml", "xml", &[], scite::XML, &[], hypertext),
    language("yaml", "yaml", &[], scite::YAML, &[], yaml),
    language("zig", "zig", &[], &[], &[ZIG, "", "", ZIG_TYPES], zig),
];

pub(crate) fn find(name: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|language| language.name == name)
}

// Keywords for languages SciTE has none for.

const TYPESCRIPT: &str = "any as asserts async await bigint declare enum from get implements infer interface is \
    keyof let module namespace never number object of override readonly satisfies set string symbol type \
    undefined unique unknown";
const KOTLIN: &str = "abstract actual annotation as break by catch class companion const constructor \
    continue crossinline data delegate do dynamic else enum expect external false field file final finally \
    for fun get if import in infix init inline inner interface internal is lateinit noinline null object \
    open operator out override package param private property protected public receiver reified return \
    sealed set setparam super suspend tailrec this throw true try typealias typeof val value var vararg \
    when where while";
const SCALA: &str = "abstract case catch class def derives do else enum export extends extension false \
    final finally for forSome given if implicit import infix inline lazy match new null object opaque open \
    override package private protected return sealed super then this throw trait transparent true try type \
    using val var while with yield";
const PROTO: &str = "bool bytes double enum extend extensions false fixed32 fixed64 float import inf int32 \
    int64 map max message nan oneof option optional package public repeated required reserved returns rpc \
    service sfixed32 sfixed64 sint32 sint64 stream string syntax to true uint32 uint64 weak edition";
const GRAPHQL: &str = "directive enum extend false fragment implements input interface mutation null on \
    query repeatable scalar schema subscription true type union";
const ELIXIR: &str = "after alias and case catch cond def defdelegate defexception defguard defimpl \
    defmacro defmacrop defmodule defoverridable defp defprotocol defstruct do else end false fn for if \
    import in nil not or quote raise receive require rescue reraise super throw true try unless unquote \
    use when with";
const TOML: &str = "true false inf nan";
const ZIG: &str = "addrspace align allowzero and anyframe anytype asm async await break callconv catch \
    comptime const continue defer else enum errdefer error export extern fn for if inline linksection \
    noalias noinline nosuspend opaque or orelse packed pub resume return struct suspend switch test \
    threadlocal try union unreachable usingnamespace var volatile while false null true undefined";
const ZIG_TYPES: &str = "anyerror anyopaque bool c_char c_int c_long c_longdouble c_longlong c_short c_uint \
    c_ulong c_ulonglong c_ushort comptime_float comptime_int f16 f32 f64 f80 f128 i8 i16 i32 i64 i128 \
    isize noreturn type u8 u16 u32 u64 u128 usize void";

// Style tables. `None` leaves text in the default color.

const COMMENT: Option<&str> = Some("comment");
const DOC: Option<&str> = Some("comment.doc");
const KEYWORD: Option<&str> = Some("keyword");
const STRING: Option<&str> = Some("string");
const ESCAPE: Option<&str> = Some("string.escape");
const REGEX: Option<&str> = Some("string.regex");
const NUMBER: Option<&str> = Some("number");
const OPERATOR: Option<&str> = Some("operator");
const PREPROC: Option<&str> = Some("preproc");
const TYPE: Option<&str> = Some("type");
const FUNCTION: Option<&str> = Some("function");
const VARIABLE: Option<&str> = Some("variable");
const CONSTANT: Option<&str> = Some("constant");
const PROPERTY: Option<&str> = Some("property");
const TAG: Option<&str> = Some("tag");
const BOOLEAN: Option<&str> = Some("boolean");
const VARIABLE_SPECIAL: Option<&str> = Some("variable.special");
/// Keys in data and configuration files (JSON, YAML, TOML, INI). Not
/// "property": several themes, the default ones too, draw it nearly in the
/// text color, and these files are mostly keys.
const KEY: Option<&str> = TAG;
const ATTRIBUTE: Option<&str> = Some("attribute");
const LABEL: Option<&str> = Some("label");
const TITLE: Option<&str> = Some("title");

/// SCE_C_*: C, C++, C#, Java, JavaScript, Go, Swift and look-alikes.
fn cpp(style: u8) -> Option<&'static str> {
    // Code in an inactive preprocessor branch: dim it like a comment.
    if style >= 0x40 {
        return COMMENT;
    }
    match style {
        1 | 2 | 23 | 26 => COMMENT,
        3 | 15 | 17 | 18 | 24 => DOC,
        4 => NUMBER,
        5 => KEYWORD,
        6 | 7 | 12 | 13 | 20 | 21 | 22 => STRING,
        8 | 25 => CONSTANT,
        9 => PREPROC,
        10 => OPERATOR,
        14 => REGEX,
        16 | 19 => TYPE,
        27 => ESCAPE,
        _ => None,
    }
}

/// SCE_RUST_*.
fn rust(style: u8) -> Option<&'static str> {
    match style {
        1 | 2 => COMMENT,
        3 | 4 => DOC,
        5 => NUMBER,
        6 | 8..=12 => KEYWORD,
        7 => TYPE,
        13 | 14 | 15 | 21..=25 => STRING,
        16 => OPERATOR,
        18 => LABEL,
        19 => FUNCTION,
        _ => None,
    }
}

/// SCE_ZIG_*.
fn zig(style: u8) -> Option<&'static str> {
    match style {
        1 => COMMENT,
        2 | 3 => DOC,
        4 => NUMBER,
        5 => OPERATOR,
        6 | 7 | 8 | 18 => STRING,
        9 => ESCAPE,
        11 | 12 => FUNCTION,
        13..=15 => KEYWORD,
        16 => TYPE,
        _ => None,
    }
}

/// SCE_P_*.
fn python(style: u8) -> Option<&'static str> {
    match style {
        1 | 12 => COMMENT,
        2 => NUMBER,
        3 | 4 | 6 | 7 | 13 | 16..=19 => STRING,
        5 => KEYWORD,
        8 | 14 => TYPE,
        9 => FUNCTION,
        10 => OPERATOR,
        15 => ATTRIBUTE,
        20 => PROPERTY,
        _ => None,
    }
}

/// SCE_RB_*: Ruby, and Elixir, which looks enough like it.
fn ruby(style: u8) -> Option<&'static str> {
    match style {
        2 | 19 => COMMENT,
        3 => DOC,
        4 => NUMBER,
        5 => KEYWORD,
        6 | 7 | 18 | 20..=28 | 41..=44 => STRING,
        8 | 15 => TYPE,
        9 => FUNCTION,
        10 => OPERATOR,
        12 => REGEX,
        13 | 16 | 17 => VARIABLE,
        14 => Some("string.special.symbol"),
        _ => None,
    }
}

/// SCE_H_* and the scripts embedded in HTML: SCE_HJ_*, SCE_HJA_*
/// (JavaScript), SCE_HB_*, SCE_HBA_* (VBScript), SCE_HP_*, SCE_HPA_*
/// (Python) and SCE_HPHP_*.
fn hypertext(style: u8) -> Option<&'static str> {
    match style {
        1 | 2 | 11 | 14 => TAG,
        3 | 4 | 23 => ATTRIBUTE,
        5 => NUMBER,
        6 | 7 | 17 | 19 | 24 | 25 => STRING,
        9 | 20 | 29 | 30 => COMMENT,
        10 | 27 | 28 => CONSTANT,
        12 | 13 | 15 | 16 | 18 => PREPROC,
        21 | 22 => Some("tag.doctype"),
        40..=53 | 55..=68 => match (style - 40) % 15 {
            2 | 3 => COMMENT,
            4 => DOC,
            5 => NUMBER,
            7 => KEYWORD,
            8 | 9 | 11 | 13 => STRING,
            10 => OPERATOR,
            12 => REGEX,
            _ => None,
        },
        70..=77 | 80..=87 => match style % 10 {
            2 => COMMENT,
            3 => NUMBER,
            4 => KEYWORD,
            5 | 7 => STRING,
            _ => None,
        },
        90..=102 | 105..=117 => match (style - 90) % 15 {
            2 => COMMENT,
            3 => NUMBER,
            4 | 5 | 7 | 8 => STRING,
            6 => KEYWORD,
            9 => TYPE,
            10 => FUNCTION,
            11 => OPERATOR,
            _ => None,
        },
        104 | 123 | 126 => VARIABLE,
        119 | 120 => STRING,
        121 => KEYWORD,
        122 => NUMBER,
        124 | 125 => COMMENT,
        127 => OPERATOR,
        _ => None,
    }
}

/// SCE_LUA_*.
fn lua(style: u8) -> Option<&'static str> {
    match style {
        1 | 2 => COMMENT,
        3 => DOC,
        4 => NUMBER,
        5 | 16..=19 => KEYWORD,
        6 | 7 | 8 | 12 => STRING,
        9 => PREPROC,
        10 => OPERATOR,
        13..=15 => FUNCTION,
        20 => LABEL,
        _ => None,
    }
}

/// SCE_SH_*.
fn bash(style: u8) -> Option<&'static str> {
    match style {
        2 => COMMENT,
        3 => NUMBER,
        4 => KEYWORD,
        5 | 6 | 13 => STRING,
        7 => OPERATOR,
        9 | 10 => VARIABLE,
        11 | 12 => Some("string.special"),
        _ => None,
    }
}

/// SCE_CSS_*.
fn css(style: u8) -> Option<&'static str> {
    match style {
        1 => TAG,
        2 | 10 => TYPE,
        3 | 4 | 16 | 18 | 20 | 21 => ATTRIBUTE,
        5 => OPERATOR,
        6 | 7 | 15 | 17 | 19 => PROPERTY,
        8 => CONSTANT,
        9 => COMMENT,
        11 | 12 | 22 => KEYWORD,
        13 | 14 => STRING,
        23 => VARIABLE,
        _ => None,
    }
}

/// SCE_JSON_*.
fn json(style: u8) -> Option<&'static str> {
    match style {
        1 => NUMBER,
        2 | 3 | 9 | 10 => STRING,
        4 => KEY,
        5 => ESCAPE,
        6 | 7 => COMMENT,
        8 => OPERATOR,
        11 | 12 => KEYWORD,
        _ => None,
    }
}

/// SCE_TOML_*.
fn toml(style: u8) -> Option<&'static str> {
    match style {
        1 => COMMENT,
        3 => KEYWORD,
        4 => NUMBER,
        5 => TYPE,
        6 => KEY,
        8 => OPERATOR,
        9..=12 | 15 => STRING,
        13 => ESCAPE,
        14 => CONSTANT,
        _ => None,
    }
}

/// SCE_YAML_*. Plain values are strings, as YAML reads them.
fn yaml(style: u8) -> Option<&'static str> {
    match style {
        0 | 7 => STRING,
        1 => COMMENT,
        2 => KEY,
        3 => BOOLEAN,
        4 => NUMBER,
        5 => VARIABLE_SPECIAL,
        6 => PREPROC,
        9 => OPERATOR,
        _ => None,
    }
}

/// SCE_MARKDOWN_*.
fn markdown(style: u8) -> Option<&'static str> {
    match style {
        2 | 3 => Some("emphasis.strong"),
        4 | 5 => Some("emphasis"),
        6..=11 => TITLE,
        13 | 14 => Some("punctuation.list_marker"),
        15 => COMMENT,
        17 => Some("punctuation.special"),
        18 => Some("link_uri"),
        19..=21 => Some("text.literal"),
        _ => None,
    }
}

/// SCE_SQL_*.
fn sql(style: u8) -> Option<&'static str> {
    match style {
        1 | 2 | 13 => COMMENT,
        3 | 15 | 17 | 18 => DOC,
        4 => NUMBER,
        5 => KEYWORD,
        6 | 7 => STRING,
        8 | 9 => PREPROC,
        10 | 24 => OPERATOR,
        16 => TYPE,
        19..=22 => FUNCTION,
        _ => None,
    }
}

/// SCE_DIFF_*. Themes have no added/removed colors; strings are usually
/// green and tags red.
fn diff(style: u8) -> Option<&'static str> {
    match style {
        1 => COMMENT,
        2 => KEYWORD,
        3 => TITLE,
        4 => FUNCTION,
        5 | 9 | 11 => TAG,
        6 | 8 | 10 => STRING,
        7 => NUMBER,
        _ => None,
    }
}

/// SCE_CMAKE_*.
fn cmake(style: u8) -> Option<&'static str> {
    match style {
        1 => COMMENT,
        2..=4 => STRING,
        5 | 8 => FUNCTION,
        6 => PROPERTY,
        7 | 13 => VARIABLE,
        9..=12 => KEYWORD,
        14 => NUMBER,
        _ => None,
    }
}

/// SCE_MAKE_*.
fn make(style: u8) -> Option<&'static str> {
    match style {
        1 => COMMENT,
        2 => PREPROC,
        3 => VARIABLE,
        4 => OPERATOR,
        5 => FUNCTION,
        _ => None,
    }
}

/// SCE_PROPS_*: INI and .properties files.
fn props(style: u8) -> Option<&'static str> {
    match style {
        1 => COMMENT,
        2 => TYPE,
        3 => OPERATOR,
        4 => CONSTANT,
        5 => KEY,
        _ => None,
    }
}

/// SCE_BAT_*.
fn batch(style: u8) -> Option<&'static str> {
    match style {
        1 => COMMENT,
        2 => KEYWORD,
        3 => LABEL,
        4 | 7 => OPERATOR,
        5 => FUNCTION,
        6 => VARIABLE,
        _ => None,
    }
}

/// SCE_POWERSHELL_*.
fn powershell(style: u8) -> Option<&'static str> {
    match style {
        1 | 13 => COMMENT,
        2 | 3 | 14 | 15 => STRING,
        4 => NUMBER,
        5 => VARIABLE,
        6 => OPERATOR,
        8 => KEYWORD,
        9..=11 => FUNCTION,
        16 => DOC,
        _ => None,
    }
}

/// SCE_PL_*.
fn perl(style: u8) -> Option<&'static str> {
    match style {
        2 | 21 => COMMENT,
        3 | 31 => DOC,
        4 => NUMBER,
        5 => KEYWORD,
        6 | 7 | 19 | 20 | 23..=30 | 43 | 57 | 61..=66 => STRING,
        8 => Some("punctuation"),
        9 => PREPROC,
        10 => OPERATOR,
        12..=16 => VARIABLE,
        17 | 18 | 44 | 54 | 55 => REGEX,
        22 => Some("string.special"),
        40 => TYPE,
        _ => None,
    }
}
