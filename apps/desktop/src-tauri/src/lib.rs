//! Granary desktop host (Tauri 2). Startup order matters:
//! 1. WebView2 transparency env var (before any thread exists, ADR-0003);
//! 2. logging (redacted, rotated);
//! 3. display repair from a leftover restore marker, before any window (ADR-0006);
//! 4. libmpv probe (delay-loaded DLL, ADR-0005);
//! 5. Tauri with single-instance + window-state.

pub mod commands;
pub mod logging;
pub mod paths;
pub mod redact;

use commands::{AppInfo, LibmpvStatus};
use tauri::Manager;

/// Restore any display state a previous (crashed) run left changed. Never blocks startup on failure.
#[cfg(windows)]
fn repair_displays() {
    use granary_display::marker;
    use granary_display::win::WindowsDisplayController;
    let Some(path) = marker::default_path() else { return };
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

pub fn run() {
    // SAFETY: called first in main, before any other thread exists; WebView2 reads it when the controller is created
    // so the page background can be transparent over the video window (ADR-0003).
    unsafe { std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "00000000") };

    let _log_guard = logging::init(&paths::log_dir());
    let version = env!("CARGO_PKG_VERSION").to_string();
    tracing::info!(%version, "Granary starting");
    std::panic::set_hook(Box::new(|p| tracing::error!(panic = %p, "panic")));

    #[cfg(windows)]
    repair_displays();

    let libmpv = LibmpvStatus::from_probe(probe_libmpv());
    match &libmpv.error {
        None => tracing::info!(api = ?libmpv.api_version, "libmpv loaded"),
        Some(e) => tracing::error!(error = %e, "libmpv unavailable; playback disabled"),
    }

    let result = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the running window and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(AppInfo { version, libmpv })
        .invoke_handler(tauri::generate_handler![commands::app_info])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!(error = %e, "Tauri runtime failed");
        std::process::exit(1);
    }
}
