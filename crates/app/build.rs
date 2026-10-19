//! Embeds the application icon into the Windows executable.

fn main() {
    let rc_file = "resources/windows/oxypad.rc";
    let icon_dir = "../../oxypad-icons";
    println!("cargo:rerun-if-changed={rc_file}");
    println!("cargo:rerun-if-changed={icon_dir}/oxypad.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(rc_file, embed_resource::ParamsIncludeDirs([icon_dir]))
            .manifest_optional()
            .expect("failed to embed the application icon");
    }
}
