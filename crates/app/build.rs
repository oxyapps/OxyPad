//! Embeds the application icon and version information into the Windows
//! executable.

use std::{env, fs, path::PathBuf};

fn main() {
    let icon_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../oxypad-icons");
    println!("cargo:rerun-if-changed={}", icon_dir.join("oxypad.ico").display());
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // The resource script is generated so the version always matches
    // Cargo.toml: Windows shows it in the file's Properties and Task Manager,
    // and the MSI compares it when upgrading.
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let number = |part: &str| env::var(part).unwrap().parse::<u16>().unwrap_or(0);
    let (major, minor, patch) =
        (number("CARGO_PKG_VERSION_MAJOR"), number("CARGO_PKG_VERSION_MINOR"), number("CARGO_PKG_VERSION_PATCH"));
    let script = format!(
        r#"// GPUI loads the window and taskbar icon from resource ID 1.
1 ICON "oxypad.ico"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "OxyApps"
      VALUE "FileDescription", "OxyPad"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "oxypad"
      VALUE "LegalCopyright", "Licensed under the Apache License 2.0"
      VALUE "OriginalFilename", "OxyPad.exe"
      VALUE "ProductName", "OxyPad"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let rc_file = PathBuf::from(env::var("OUT_DIR").unwrap()).join("oxypad.rc");
    fs::write(&rc_file, script).expect("failed to write the resource script");
    embed_resource::compile(&rc_file, embed_resource::ParamsIncludeDirs([&icon_dir]))
        .manifest_optional()
        .expect("failed to embed the Windows resources");
}
