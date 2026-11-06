//! Granary desktop host (Tauri 2). Startup order matters:
//! 1. WebView2 transparency env var (before the WebView2 environment exists, ADR-0003);
//! 2. logging (redacted, rotated);
//! 3. libmpv probe (delay-loaded DLL, ADR-0005);
//! 4. Tauri plugins: single-instance first, so a second launch exits before touching anything;
//! 5. `setup`: display repair from a leftover restore marker, *then* the main window is created (ADR-0006,
//!    phase-3 review H1). The window is declared with `"create": false` in tauri.conf.json for this reason.

pub mod commands;
pub mod logging;
pub mod paths;
pub mod redact;

use commands::{AppInfo, LibmpvStatus};
use tauri::{Manager, RunEvent};
use tauri_plugin_window_state::StateFlags;

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

fn setup(app: &mut tauri::App, libmpv: LibmpvStatus) -> Result<(), Box<dyn std::error::Error>> {
    // The bundler's version (tauri.conf.json → package.json) is the one users see in the installer (review M6).
    let version = app.package_info().version.to_string();
    tracing::info!(%version, "Granary starting");
    app.manage(AppInfo { version, libmpv });

    #[cfg(windows)]
    repair_displays();

    let main = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or("window `main` missing from tauri.conf.json")?;
    tauri::WebviewWindowBuilder::from_config(app.handle(), &main)?.build()?;
    Ok(())
}

pub fn run() {
    // SAFETY: Rust 2024 marks set_var unsafe because concurrent environment access is UB on some platforms; on
    // Windows the CRT/OS environment block is synchronised, and this runs first in main before Granary starts any
    // thread. WebView2 reads the variable when its environment is created, so the page background can be transparent
    // over the video window (ADR-0003, validated in the Phase 2 spike).
    unsafe { std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "00000000") };

    logging::init(&paths::log_dir());
    std::panic::set_hook(Box::new(|p| tracing::error!(panic = %p, "panic")));

    let libmpv = LibmpvStatus::from_probe(probe_libmpv());
    match &libmpv.error {
        None => tracing::info!(api = ?libmpv.api_version, "libmpv loaded"),
        Some(e) => tracing::error!(error = %e, "libmpv unavailable; playback disabled"),
    }

    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the running window and exits during plugin setup,
        // before `setup` (display repair) runs.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        // Never restore fullscreen/hidden state: quitting during fullscreen playback must reopen a normal window.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() - StateFlags::FULLSCREEN - StateFlags::VISIBLE)
                .build(),
        )
        .setup(move |app| setup(app, libmpv))
        .invoke_handler(tauri::generate_handler![commands::app_info])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|_, event| {
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
