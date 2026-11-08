//! Granary desktop host (Tauri 2). Startup order matters:
//! 1. WebView2 transparency env var (before the WebView2 environment exists, ADR-0003);
//! 2. logging (redacted, rotated);
//! 3. libmpv probe (delay-loaded DLL, ADR-0005);
//! 4. Tauri plugins: single-instance first, so a second launch exits before touching anything;
//! 5. `setup`: display repair from a leftover restore marker, *then* the main window is created (ADR-0006,
//!    phase-3 review H1). The window is declared with `"create": false` in tauri.conf.json for this reason.

pub mod about;
pub mod artcache;
pub mod catalog;
pub mod commands;
pub mod logging;
pub mod paths;
pub mod playback;
pub mod redact;
pub mod seekr;
pub mod settings;
pub mod silo;

use commands::{AppInfo, LibmpvStatus};
use tauri::{Manager, RunEvent};
use tauri_plugin_window_state::StateFlags;

/// Panic path (ADR-0006 restore guarantee): put back whatever this process changed, from the restore marker, before
/// the process dies. Best effort; a marker that is not fully restored stays for the startup repair.
///
/// Only with `panic = "abort"` (release builds): there every panic ends the process right after the hook. With
/// unwinding (debug/test builds) a panic can be caught (tokio tasks, `catch_unwind`) while playback continues, so the
/// display is left alone and the startup repair handles a real crash (phase-7 review M4).
#[cfg(windows)]
fn restore_displays_now() {
    use granary_display::marker::{self, RestoreMarker};
    use granary_display::win::WindowsDisplayController;
    let path = paths::local_dir().join(marker::FILE_NAME);
    if let Ok(Some(m)) = RestoreMarker::load(&path) {
        let report = marker::restore(&WindowsDisplayController, &m, std::time::Duration::from_secs(3));
        if report.all_ok() {
            let _ = RestoreMarker::remove(&path);
            tracing::warn!("display state restored after a panic");
        } else {
            tracing::error!(?report, "display state not fully restored after a panic; marker kept");
        }
    }
}

#[cfg(not(windows))]
fn restore_displays_now() {}

/// Restore any display state a previous (crashed) run left changed; a marker owned by a still-running process is
/// left alone. Never blocks startup on failure.
#[cfg(windows)]
fn repair_displays() {
    use granary_display::marker;
    use granary_display::win::WindowsDisplayController;
    let path = paths::local_dir().join(marker::FILE_NAME);
    match marker::repair_at_startup(&WindowsDisplayController, &path, std::time::Duration::from_secs(5)) {
        Ok(None) => {}
        Ok(Some(report)) if report.all_ok() => tracing::warn!("restored display state left over by a previous run"),
        Ok(Some(report)) => tracing::error!(?report, "display state could not be fully restored; marker kept"),
        Err(e) => tracing::error!(error = %e, "display restore marker unreadable; kept for inspection"),
    }
}

#[cfg(windows)]
fn probe_libmpv() -> Result<u64, String> {
    granary_player::probe::probe_libmpv().map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn probe_libmpv() -> Result<u64, String> {
    Err("libmpv playback is only supported on Windows".into())
}

/// Server/account state (ADR-0004): HTTP client with this installation's device identity, refresh tokens in
/// Windows Credential Manager, server list in `%APPDATA%\Granary\servers.json`.
///
/// Debug builds only: `GRANARY_MOCK_SERVER=1` swaps in the in-process mock server and an in-memory credential store
/// (UI development and end-to-end tests without a Silo server; user `alice` / `pw`, Kids PIN `1234`).
/// Debug build with `GRANARY_MOCK_SERVER=1`.
fn mock_mode() -> bool {
    cfg!(debug_assertions) && std::env::var("GRANARY_MOCK_SERVER").is_ok_and(|v| v == "1")
}

fn silo_state(version: &str) -> Result<silo::SiloState, Box<dyn std::error::Error>> {
    use granary_silo_api::credentials::{CredentialStore, MemoryStore};
    use granary_silo_api::device::DeviceIdentity;
    use granary_silo_api::{HttpSiloApi, SiloApi};
    use std::sync::Arc;

    let registry = paths::config_dir().join("servers.json");
    if mock_mode() {
        tracing::warn!("GRANARY_MOCK_SERVER=1: using the in-process mock server");
        // The mock streams the generated test clips (scripts/gen-test-media.ps1) for real playback in debug builds.
        let media = std::env::var_os("GRANARY_MOCK_MEDIA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../test-media"));
        let api: Arc<dyn SiloApi> = Arc::new(granary_silo_api::mock::MockSiloApi::demo().with_media_dir(media));
        let store: Arc<dyn CredentialStore> = Arc::new(MemoryStore::default());
        return Ok(silo::SiloState::new(
            api,
            store,
            paths::local_dir().join("mock-servers.json"),
        ));
    }
    let device = DeviceIdentity::load_or_create(&paths::config_dir().join("device.json"), version)?;
    let api: Arc<dyn SiloApi> = Arc::new(HttpSiloApi::new(device)?);
    #[cfg(windows)]
    let store: Arc<dyn CredentialStore> = match granary_silo_api::credentials::WindowsCredentialStore::new() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!(error = %e, "Credential Manager unavailable; sign-ins last until the app closes");
            Arc::new(MemoryStore::default())
        }
    };
    #[cfg(not(windows))]
    let store: Arc<dyn CredentialStore> = Arc::new(MemoryStore::default());
    Ok(silo::SiloState::new(api, store, registry))
}

fn setup(app: &mut tauri::App, libmpv: LibmpvStatus) -> Result<(), Box<dyn std::error::Error>> {
    // The bundler's version (tauri.conf.json → package.json) is the one users see in the installer (review M6).
    let version = app.package_info().version.to_string();
    tracing::info!(%version, "Granary starting");
    // Repair a display left switched by a crash before anything that can fail and end startup (final review L2).
    #[cfg(windows)]
    // A mock-mode dev build may run beside the installed app, whose marker is not ours to repair.
    if !mock_mode() {
        repair_displays();
    }
    app.manage(silo_state(&version)?);
    app.manage(settings::SettingsStore::load(paths::config_dir().join("settings.json")));
    let libmpv_ok = libmpv.error.is_none();
    app.manage(AppInfo {
        version: version.clone(),
        libmpv,
    });
    app.manage(playback::events::EventsState::default());
    app.manage(playback::preview::PreviewState::default());
    app.manage(artcache::ArtCache::default());
    app.manage(seekr::SeekrState::default());

    let main = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or("window `main` missing from tauri.conf.json")?;
    let dev_url = app.config().build.dev_url.clone();
    tauri::WebviewWindowBuilder::from_config(app.handle(), &main)?
        // Only the app itself may be the top-level document: never the artwork scheme, the IPC origin or a remote
        // page, all of which would otherwise run with the main window's capability (phase-5 review M3).
        .on_navigation(move |url| {
            let allowed = navigation_allowed(url, dev_url.as_ref());
            if !allowed {
                tracing::warn!(scheme = url.scheme(), host = ?url.host_str(), "blocked navigation");
            }
            allowed
        })
        .build()?;
    let hwdec = tauri::async_runtime::block_on(app.state::<settings::SettingsStore>().get())
        .player
        .hwdec;
    if let Err(e) = playback::commands::install(app, version, hwdec, libmpv_ok) {
        // The library UI still works; playback reports the error when requested.
        tracing::error!(error = %e, "player host could not be installed");
    }
    Ok(())
}

/// The app's own origin: `tauri://localhost` / `http(s)://tauri.localhost` for bundled assets, plus the dev server
/// in development builds.
fn navigation_allowed(url: &tauri::Url, dev_url: Option<&tauri::Url>) -> bool {
    let bundled = match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    };
    let dev = cfg!(debug_assertions) && dev_url.is_some_and(|d| d.origin() == url.origin());
    bundled || dev
}

/// Process start, for the cold-start measurement logged by [`commands::app_ready`] (docs/perf.md).
pub static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn run() {
    let _ = STARTED.set(std::time::Instant::now());
    // SAFETY: Rust 2024 marks set_var unsafe because concurrent environment access is UB on some platforms; on
    // Windows the CRT/OS environment block is synchronised, and this runs first in main before Granary starts any
    // thread. WebView2 reads the variable when its environment is created, so the page background can be transparent
    // over the video window (ADR-0003, validated in the Phase 2 spike).
    unsafe { std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "00000000") };

    logging::init(&paths::log_dir());
    std::panic::set_hook(Box::new(|p| {
        tracing::error!(panic = %p, "panic");
        if cfg!(panic = "abort") {
            restore_displays_now();
        }
        logging::flush();
    }));

    let libmpv = LibmpvStatus::from_probe(probe_libmpv());
    match &libmpv.error {
        None => tracing::info!(api = ?libmpv.api_version, "libmpv loaded"),
        Some(e) => tracing::error!(error = %e, "libmpv unavailable; playback disabled"),
    }

    let mut builder = tauri::Builder::default();
    // Must be the first plugin: a second launch focuses the running window and exits during plugin setup, before
    // `setup` (display repair) runs. A mock-mode dev build skips it so it can run beside the installed app.
    if !mock_mode() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }));
    }
    let app = builder
        // Never restore fullscreen/hidden state: quitting during fullscreen playback must reopen a normal window.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() - StateFlags::FULLSCREEN - StateFlags::VISIBLE)
                .build(),
        )
        // Artwork for the WebView: fetched by the host without credentials (ADR-0004 amendment 3), so the CSP can
        // forbid every remote image origin.
        .register_asynchronous_uri_scheme_protocol(catalog::IMAGE_SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let uri = request.uri().to_string();
            tauri::async_runtime::spawn(async move {
                responder.respond(catalog::serve_image(&app, &uri).await);
            });
        })
        .setup(move |app| setup(app, libmpv))
        // The display policy follows the monitor the player window is on (R4 §8.5).
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(
                    event,
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
                )
                && let Some(host) = window.app_handle().try_state::<playback::host::PlayerHost>()
            {
                host.send(playback::host::Cmd::WindowMoved);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::app_ready,
            about::about_info,
            about::open_link,
            about::open_trailer,
            about::logs_tail,
            about::logs_export,
            silo::servers_list,
            silo::servers_probe,
            silo::servers_add,
            silo::servers_remove,
            silo::servers_activate,
            silo::auth_login,
            silo::auth_logout,
            silo::profiles_list,
            silo::profiles_select,
            silo::libraries_list,
            catalog::home_sections,
            catalog::catalog_browse,
            catalog::catalog_item,
            catalog::series_seasons,
            catalog::season_episodes,
            catalog::watch_state,
            catalog::watch_trickplay,
            catalog::catalog_set_flag,
            catalog::catalog_set_rating,
            catalog::home_dismiss,
            catalog::artwork_keep,
            catalog::artwork_cache_info,
            catalog::artwork_cache_clear,
            seekr::seekr_key_status,
            seekr::seekr_key_set,
            seekr::preview_seekr,
            catalog::person_get,
            catalog::library_sections,
            catalog::library_collections,
            catalog::collection_items,
            catalog::catalog_similar,
            catalog::catalog_filters,
            playback::commands::player_play,
            playback::commands::player_stop,
            playback::commands::player_pause,
            playback::commands::player_seek,
            playback::commands::player_set,
            playback::commands::player_audio,
            playback::commands::player_subtitle,
            playback::commands::player_stats,
            playback::preview::player_preview,
            playback::preview::player_preview_near,
            playback::preview::player_preview_warm,
            playback::commands::player_state,
            playback::commands::player_overlay,
            playback::commands::player_fullscreen,
            playback::commands::player_mini,
            playback::commands::player_window,
            playback::commands::player_audio_devices,
            playback::commands::display_list,
            playback::events::events_connect,
            playback::events::events_disconnect,
            settings::settings_get,
            settings::settings_set,
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event
                && let Some(host) = app.try_state::<playback::host::PlayerHost>()
            {
                // Report the final position, release the server session and restore the display state before the
                // process ends (an unfinished restore is repaired from the marker at the next start).
                host.shutdown_blocking(std::time::Duration::from_secs(5));
            }
            if let RunEvent::Exit = event {
                tracing::info!("Granary exiting");
                logging::flush();
            }
        }),
        Err(e) => {
            tracing::error!(error = %e, "Tauri runtime failed");
            logging::flush();
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod nav_tests {
    use super::navigation_allowed;

    #[test]
    fn only_the_app_origin_is_navigable() {
        let u = |s: &str| tauri::Url::parse(s).unwrap();
        let dev = u("http://127.0.0.1:1420");
        for ok in [
            "http://tauri.localhost/index.html",
            "tauri://localhost/#/servers",
            "http://127.0.0.1:1420/#/x",
        ] {
            assert!(navigation_allowed(&u(ok), Some(&dev)), "{ok}");
        }
        for bad in [
            "http://granary-img.localhost/1?u=/a.svg",
            "http://ipc.localhost/plugin",
            "https://evil.example/",
            "http://127.0.0.1:1421/",
            "file:///C:/x.svg",
        ] {
            assert!(!navigation_allowed(&u(bad), Some(&dev)), "{bad}");
        }
    }
}
