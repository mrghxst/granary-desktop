//! Build script: delay-load libmpv for the app binary, place the pinned DLL next to the built exe for `tauri dev` /
//! `cargo run`, and generate the least-privilege app-command permissions.
use std::path::{Path, PathBuf};

const COMMANDS: &[&str] = &[
    "app_info",
    "servers_list",
    "servers_probe",
    "servers_add",
    "servers_remove",
    "servers_activate",
    "auth_login",
    "auth_logout",
    "profiles_list",
    "profiles_select",
    "libraries_list",
    "home_sections",
    "catalog_browse",
    "catalog_item",
    "series_seasons",
    "season_episodes",
    "watch_state",
    "watch_trickplay",
    "player_play",
    "player_stop",
    "player_pause",
    "player_seek",
    "player_set",
    "player_audio",
    "player_subtitle",
    "player_stats",
    "player_state",
    "player_overlay",
    "player_fullscreen",
    "player_mini",
    "player_window",
    "player_audio_devices",
    "display_list",
    "events_connect",
    "events_disconnect",
    "settings_get",
    "settings_set",
];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let dll = manifest.join("../../../vendor-bin/libmpv/libmpv-2.dll");
    println!("cargo:rerun-if-changed={}", dll.display());

    // A missing or broken DLL must surface as a handled error, not a loader dialog (ADR-0005); see
    // granary_player::probe. The player crate's build script provides the link search path.
    println!("cargo:rustc-link-arg-bin=granary=/DELAYLOAD:libmpv-2.dll");
    println!("cargo:rustc-link-arg-bin=granary=delayimp.lib");

    if let Some(profile_dir) = profile_dir() {
        copy_if_newer(&dll, &profile_dir.join("libmpv-2.dll"));
    }

    let attrs = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(e) = tauri_build::try_build(attrs) {
        println!("cargo:warning=tauri-build failed: {e:#}");
        std::process::exit(1);
    }
}

/// `target/<profile>` (OUT_DIR is `target/<profile>/build/<pkg>-<hash>/out`).
fn profile_dir() -> Option<PathBuf> {
    let out = PathBuf::from(std::env::var_os("OUT_DIR")?);
    out.ancestors().nth(3).map(Path::to_path_buf)
}

fn copy_if_newer(src: &Path, dst: &Path) {
    let Ok(src_meta) = std::fs::metadata(src) else {
        println!(
            "cargo:warning=libmpv-2.dll not found at {}; run scripts/fetch-libmpv.ps1",
            src.display()
        );
        return;
    };
    let stale = match std::fs::metadata(dst) {
        Ok(d) => d.len() != src_meta.len() || d.modified().ok() < src_meta.modified().ok(),
        Err(_) => true,
    };
    if stale && let Err(e) = std::fs::copy(src, dst) {
        println!("cargo:warning=could not copy libmpv-2.dll to {}: {e}", dst.display());
    }
}
