fn main() {
    let manifest =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let libdir = manifest.join("../../../vendor-bin/libmpv");
    println!("cargo:rustc-link-search=native={}", libdir.display());
    // Delay-load libmpv so a missing/corrupt DLL becomes a handled error instead of a loader dialog (ADR-0005 L11).
    println!("cargo:rustc-link-arg-bin=granary-spike=/DELAYLOAD:libmpv-2.dll");
    println!("cargo:rustc-link-arg-bin=granary-spike=delayimp.lib");
    tauri_build::build();
}
