//! Compiles Lexilla (its lexers, support library and catalogue) and the
//! bridge in `src/shim.cpp` into a static library.

use std::path::Path;

fn main() {
    let vendor = Path::new("vendor");
    let lexilla = vendor.join("lexilla");
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed=vendor");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .include(lexilla.join("include"))
        .include(lexilla.join("lexlib"))
        .include(vendor.join("scintilla/include"))
        .file("src/shim.cpp")
        .file(lexilla.join("src/Lexilla.cxx"));
    for dir in ["lexlib", "lexers"] {
        let mut files: Vec<_> = std::fs::read_dir(lexilla.join(dir))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "cxx"))
            .collect();
        files.sort();
        build.files(files);
    }
    if build.get_compiler().is_like_msvc() {
        build.flag("/EHsc").flag("/utf-8");
    }
    build.compile("lexilla");
}
