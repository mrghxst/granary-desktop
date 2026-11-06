//! Link against the pinned libmpv import library in `vendor-bin/libmpv` (run `scripts/fetch-libmpv.ps1` first).
//! The desktop executable delay-loads `libmpv-2.dll` (ADR-0005); unit tests that never call mpv contain no mpv imports
//! after dead-code elimination and run without the DLL.
fn main() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-link-search=native={}",
        manifest.join("../../vendor-bin/libmpv").display()
    );
}
