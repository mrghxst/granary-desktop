//! In-app updates (ADR-0015): the release feed is checked in the background, a newer version is downloaded and its
//! minisign signature verified (tauri-plugin-updater), and "Restart to update" runs the NSIS installer in passive mode,
//! which reopens Granary when it is done. Before the installer starts, playback is stopped (final position reported,
//! display state restored) and the log is flushed, as on a normal quit.
//!
//! Debug builds never check the real feed, so a development build cannot replace the installed app; they check only
//! the feed named by `GRANARY_UPDATE_FEED`, for testing.

use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::silo::CommandError;

/// Event carrying every [`UpdateStatus`] change.
pub const EVENT_UPDATE: &str = "update-status";
/// The first automatic check waits until startup and the first screens are done.
const FIRST_CHECK: Duration = Duration::from_secs(20);
/// Automatic checks while the app stays open.
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);
/// Download progress is reported at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateStatus {
    /// Nothing checked yet.
    #[default]
    Idle,
    /// A development build without a test feed: updates come only through the installer.
    Unavailable,
    Checking,
    UpToDate,
    Downloading {
        version: String,
        notes: Option<String>,
        received: u64,
        total: Option<u64>,
    },
    /// Downloaded and verified: "Restart to update" installs it.
    Ready {
        version: String,
        notes: Option<String>,
    },
    Installing {
        version: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Default)]
pub struct UpdateState {
    status: Mutex<UpdateStatus>,
    /// The verified update and its installer, kept until the restart.
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    busy: AtomicBool,
}

impl UpdateState {
    fn status(&self) -> UpdateStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn set(app: &AppHandle, status: UpdateStatus) {
    if let Some(state) = app.try_state::<UpdateState>()
        && let Ok(mut s) = state.status.lock()
    {
        *s = status.clone();
    }
    let _ = app.emit(EVENT_UPDATE, &status);
}

/// The feed to check: the configured one in release builds, `GRANARY_UPDATE_FEED` only in debug builds.
fn test_feed() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("GRANARY_UPDATE_FEED").ok().filter(|s| !s.is_empty())
    } else {
        None
    }
}

fn updates_possible() -> bool {
    !crate::mock_mode() && (!cfg!(debug_assertions) || test_feed().is_some())
}

/// Check the feed and, when a newer version is listed, download and verify it. One run at a time; a downloaded update
/// is kept, so later checks only report it.
pub async fn check(app: &AppHandle) -> UpdateStatus {
    let Some(state) = app.try_state::<UpdateState>() else {
        return UpdateStatus::Idle;
    };
    if !updates_possible() {
        set(app, UpdateStatus::Unavailable);
        return UpdateStatus::Unavailable;
    }
    if state.ready.lock().map(|r| r.is_some()).unwrap_or(false) || state.busy.swap(true, Ordering::SeqCst) {
        return state.status();
    }
    let result = check_and_download(app).await;
    state.busy.store(false, Ordering::SeqCst);
    let status = match result {
        Ok(s) => s,
        Err(message) => {
            tracing::warn!(error = %message, "update check failed");
            UpdateStatus::Failed { message }
        }
    };
    set(app, status.clone());
    status
}

async fn check_and_download(app: &AppHandle) -> Result<UpdateStatus, String> {
    set(app, UpdateStatus::Checking);
    let quit = app.clone();
    let mut builder = app.updater_builder().on_before_exit(move || {
        // As on a normal quit: final position, server session released, display state restored, log flushed.
        if let Some(host) = quit.try_state::<crate::playback::host::PlayerHost>() {
            host.shutdown_blocking(Duration::from_secs(5));
        }
        tracing::info!("Granary closing for the update");
        crate::logging::flush();
    });
    if let Some(feed) = test_feed() {
        let url = feed.parse().map_err(|e| format!("test feed: {e}"))?;
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    let updater = builder.build().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        tracing::info!("no update available");
        return Ok(UpdateStatus::UpToDate);
    };
    let version = update.version.clone();
    let notes = update.body.clone().filter(|b| !b.trim().is_empty());
    tracing::info!(%version, "update available; downloading");
    let mut received: u64 = 0;
    let mut last = Instant::now() - PROGRESS_EVERY;
    set(
        app,
        UpdateStatus::Downloading {
            version: version.clone(),
            notes: notes.clone(),
            received: 0,
            total: None,
        },
    );
    // `download` verifies the signature against the public key in tauri.conf.json before returning the bytes.
    let bytes = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                if last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    set(
                        app,
                        UpdateStatus::Downloading {
                            version: version.clone(),
                            notes: notes.clone(),
                            received,
                            total,
                        },
                    );
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(%version, bytes = bytes.len(), "update downloaded and verified");
    if let Some(state) = app.try_state::<UpdateState>()
        && let Ok(mut r) = state.ready.lock()
    {
        *r = Some((update, bytes));
    }
    Ok(UpdateStatus::Ready { version, notes })
}

/// Automatic checks: once shortly after startup, then every few hours, while the setting allows it.
pub fn spawn_automatic(app: AppHandle) {
    if !updates_possible() {
        set(&app, UpdateStatus::Unavailable);
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let enabled = match app.try_state::<crate::settings::SettingsStore>() {
                Some(store) => store.get().await.auto_update,
                None => false,
            };
            if enabled {
                check(&app).await;
            }
            tokio::time::sleep(RECHECK).await;
        }
    });
}

#[tauri::command]
pub fn update_status(state: tauri::State<'_, UpdateState>) -> UpdateStatus {
    state.status()
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<UpdateStatus, CommandError> {
    Ok(check(&app).await)
}

/// Install the downloaded update: the app closes, the installer runs without questions and reopens Granary.
#[tauri::command]
pub async fn update_install(app: AppHandle, state: tauri::State<'_, UpdateState>) -> Result<(), CommandError> {
    let taken = state
        .ready
        .lock()
        .map_err(|_| CommandError::internal("update state poisoned".into()))?
        .take();
    let Some((update, bytes)) = taken else {
        return Err(CommandError::invalid("no downloaded update".into()));
    };
    let version = update.version.clone();
    set(
        &app,
        UpdateStatus::Installing {
            version: version.clone(),
        },
    );
    tracing::info!(%version, "installing update");
    // `install` exits the process once the installer has started; it only returns on failure.
    let result = tauri::async_runtime::spawn_blocking(move || update.install(&bytes).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    let message = match result {
        Ok(()) => return Ok(()),
        Err(m) => m,
    };
    tracing::error!(error = %message, "update could not be installed");
    set(
        &app,
        UpdateStatus::Failed {
            message: message.clone(),
        },
    );
    Err(CommandError::internal(message))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    #[test]
    fn status_serialises_for_the_ui() {
        let v = serde_json::to_value(UpdateStatus::Downloading {
            version: "0.7.0".into(),
            notes: None,
            received: 5,
            total: Some(10),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"phase": "downloading", "version": "0.7.0", "notes": null, "received": 5, "total": 10})
        );
        assert_eq!(
            serde_json::to_value(UpdateStatus::UpToDate).unwrap(),
            serde_json::json!({"phase": "upToDate"})
        );
    }

    #[test]
    fn debug_builds_never_use_the_real_feed() {
        if cfg!(debug_assertions) && std::env::var_os("GRANARY_UPDATE_FEED").is_none() {
            assert!(test_feed().is_none());
            assert!(!updates_possible());
        }
    }
}
