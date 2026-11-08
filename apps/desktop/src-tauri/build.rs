//! Build script: delay-load libmpv for the app binary, place the pinned DLL next to the built exe for `tauri dev` /
//! `cargo run`, and generate the least-privilege app-command permissions.
use std::path::{Path, PathBuf};

const COMMANDS: &[&str] = &[
    "app_info",
    "app_ready",
    "about_info",
    "open_link",
    "open_trailer",
    "logs_tail",
    "logs_export",
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
    "catalog_set_flag",
    "catalog_set_rating",
    "home_dismiss",
    "artwork_keep",
    "artwork_cache_info",
    "artwork_cache_clear",
    "seekr_key_status",
    "seekr_key_set",
    "preview_seekr",
    "person_get",
    "library_sections",
    "library_collections",
    "collection_items",
    "catalog_similar",
    "catalog_filters",
    "player_play",
    "player_stop",
    "player_pause",
    "player_seek",
    "player_set",
    "player_audio",
    "player_subtitle",
    "player_stats",
    "player_preview_want",
    "player_preview_warm",
    "player_state",
    "player_overlay",
    "player_fullscreen",
    "player_mini",
    "player_window",
    "player_audio_devices",
    "player_gpu",
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

    // The commit the binary was built from (About links the exact source tree, ADR-0005). Rebuilds when HEAD moves.
    let git = manifest.join("../../../.git");
    for f in ["HEAD", "refs/heads", "packed-refs", "index"] {
        if git.join(f).exists() {
            println!("cargo:rerun-if-changed={}", git.join(f).display());
        }
    }
    println!("cargo:rustc-env=GRANARY_COMMIT={}", build_commit(&manifest));

    if let Some(profile_dir) = profile_dir() {
        copy_if_newer(&dll, &profile_dir.join("libmpv-2.dll"));
    }

    let attrs = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(e) = tauri_build::try_build(attrs) {
        println!("cargo:warning=tauri-build failed: {e:#}");
        std::process::exit(1);
    }
}

/// Full SHA of HEAD, `<sha>-dirty` when tracked files differ from it (About then links the repository root instead
/// of a tree that is not the binary, phase-9 review L3), or `unknown` outside a git checkout. Release builds come from
/// clean CI checkouts; locally the flag is refreshed when HEAD or the index changes.
fn build_commit(dir: &Path) -> String {
    let sha = git(dir, &["rev-parse", "HEAD"]).filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    match sha {
        Some(sha) => match git(dir, &["status", "--porcelain", "--untracked-files=no"]) {
            Some(changes) if changes.is_empty() => sha,
            _ => format!("{sha}-dirty"),
        },
        None => "unknown".to_owned(),
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
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
