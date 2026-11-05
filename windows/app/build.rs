//! Embeds app.manifest with the MSVC linker itself, so no rc.exe or resource crate: PerMonitorV2 DPI awareness,
//! comctl32 v6 for the Settings controls, asInvoker. Linking for any other target is unaffected.
fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("app.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
            manifest.display()
        );
        // The manifest carries its own trustInfo; the linker's would be a duplicate element.
        println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
    }
}
