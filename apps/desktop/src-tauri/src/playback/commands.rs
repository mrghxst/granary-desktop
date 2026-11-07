//! Tauri commands for the player and the app wiring of [`PlayerHost`] (mpv backend, video window, media keys,
//! keep-awake, mini player).
use super::controller::{Environment, StartArgs, SubtitleChoice};
use super::host::{BackendFactory, Cmd, MediaItem, PlayerHost, Ui};
use crate::catalog::PlayRequest;
use crate::silo::{CmdResult, CommandError, SiloState};
use granary_silo_api::ApiError;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State};

pub const EVENT_ACTIVITY: &str = "player://activity";
pub const EVENT_WINDOW: &str = "player://window";

/// The UI side of the host in the app: Tauri events, overlay/cursor policy, SMTC and keep-awake.
struct TauriUi {
    app: AppHandle,
    #[cfg(windows)]
    media: Option<super::platform::MediaControls>,
    #[cfg(windows)]
    awake: Mutex<Option<super::platform::KeepAwake>>,
}

impl Ui for TauriUi {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Err(e) = self.app.emit(event, payload) {
            tracing::warn!(event, error = %e, "event not delivered to the UI");
        }
    }

    fn playing_changed(&self, active: bool, playing: bool, item: Option<&MediaItem>) {
        #[cfg(windows)]
        {
            let _ = self.app.run_on_main_thread(move || super::video::set_playing(active));
            if let Ok(mut a) = self.awake.lock() {
                if a.is_none() && playing {
                    *a = super::platform::KeepAwake::new();
                }
                if let Some(a) = a.as_mut() {
                    a.set(playing);
                }
            }
            if let Some(m) = &self.media {
                if !active {
                    m.set_item(None, false, false);
                } else if let Some(i) = item {
                    let title = if i.title.is_empty() {
                        "Granary"
                    } else {
                        i.title.as_str()
                    };
                    m.set_item(Some((title, &i.subtitle)), i.has_previous, i.has_next);
                }
            }
        }
        #[cfg(not(windows))]
        let _ = (active, playing, item);
    }

    fn position_changed(&self, playing: bool, position: f64, duration: f64) {
        #[cfg(windows)]
        if let Some(m) = &self.media {
            m.set_status(playing, position, duration);
        }
        #[cfg(not(windows))]
        let _ = (playing, position, duration);
    }
}

/// Position, size and maximized flag of the window before the mini player.
type SavedWindow = (PhysicalPosition<i32>, PhysicalSize<u32>, bool);

/// Window geometry saved while the mini player is up.
#[derive(Default)]
pub struct WindowModes {
    mini: Mutex<Option<SavedWindow>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub fullscreen: bool,
    pub mini: bool,
}

#[cfg(windows)]
fn mpv_factory(app_version: String, hwdec: String) -> BackendFactory {
    use granary_player::mpv::Mpv;
    use granary_player::options;
    Box::new(move |tx| {
        let wid = super::video::video_hwnd().0 as isize;
        if wid == 0 {
            return Err("the video window does not exist".into());
        }
        let extra: Vec<(String, String)> = [
            ("hwdec", hwdec.as_str()),
            // FFmpeg reconnects dropped HTTP connections itself before mpv reports an error (ADR-0008).
            (
                "stream-lavf-o",
                "reconnect=1,reconnect_streamed=1,reconnect_on_network_error=1,reconnect_delay_max=8",
            ),
            ("network-timeout", "30"),
            ("idle", "yes"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .chain([("user-agent".to_owned(), format!("Granary/{app_version}"))])
        .collect();
        let opts = options::merge(options::forced(wid), &[options::desktop_baseline(), extra]);
        let mpv = Mpv::new(&opts).map_err(|e| e.to_string())?;
        for (name, format) in granary_player::state::OBSERVED.iter().chain(super::host::UI_PROPERTIES) {
            mpv.observe(name, *format).map_err(|e| e.to_string())?;
        }
        let gone = tx.clone();
        mpv.spawn_events(move |e| {
            let shutdown = matches!(e, granary_player::MpvEvent::Shutdown);
            let _ = tx.send(Cmd::Event(e));
            if shutdown {
                let _ = gone.send(Cmd::BackendGone);
            }
        })
        .map_err(|e| e.to_string())?;
        tracing::info!(wid, "mpv created");
        Ok(mpv as Arc<dyn granary_player::PlayerBackend>)
    })
}

#[cfg(not(windows))]
fn mpv_factory(_app_version: String, _hwdec: String) -> BackendFactory {
    Box::new(|_| Err("libmpv playback is only supported on Windows".into()))
}

/// Create the video window, media controls and the host. Runs in `setup` on the UI thread, after the main window.
pub fn install(app: &tauri::App, app_version: String, hwdec: String, libmpv_ok: bool) -> Result<(), String> {
    let handle = app.handle().clone();
    #[cfg(windows)]
    let media = {
        let main = app
            .get_webview_window("main")
            .ok_or("main window missing")?
            .hwnd()
            .map_err(|e| e.to_string())?
            .0 as isize;
        super::video::attach(main)?;
        tracing::info!(below = super::video::video_below_webview(), "video window attached");
        let h = handle.clone();
        super::video::on_activity(move |a| {
            let _ = h.emit(EVENT_ACTIVITY, a);
        });
        let h = handle.clone();
        match super::platform::MediaControls::new(main, move |button| {
            if let Some(host) = h.try_state::<PlayerHost>() {
                host.send(Cmd::MediaKey(button));
            }
        }) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!(error = %e, "media controls unavailable");
                None
            }
        }
    };
    let ui = Arc::new(TauriUi {
        app: handle.clone(),
        #[cfg(windows)]
        media,
        #[cfg(windows)]
        awake: Mutex::new(None),
    });
    let factory: BackendFactory = if libmpv_ok {
        mpv_factory(app_version.clone(), hwdec)
    } else {
        Box::new(|_| Err("libmpv is not available".into()))
    };
    let hardware = Arc::new(std::sync::OnceLock::new());
    #[cfg(windows)]
    let settings_handle = handle.clone();
    let env = Arc::new(move || {
        #[cfg(windows)]
        {
            Environment {
                app_version: app_version.clone(),
                os_version: super::platform::os_version(),
                output: super::platform::output_facts(
                    settings_handle
                        .try_state::<crate::settings::SettingsStore>()
                        .is_none_or(|s| s.current().display.hdr_mode != "off"),
                ),
                hardware: hardware.get_or_init(super::platform::probe_hardware_decoders).clone(),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = &hardware;
            Environment {
                app_version: app_version.clone(),
                ..Environment::default()
            }
        }
    });
    let output = Arc::new(display_output(app));
    let host = tauri::async_runtime::block_on(async move { PlayerHost::start(ui, factory, env, output) });
    app.manage(host);
    app.manage(WindowModes::default());
    Ok(())
}

/// The display/audio output of the player (Phase 7): the Windows display controller, the monitor of the main window
/// and the current settings; the restore marker is the one startup repair reads.
fn display_output(app: &tauri::App) -> super::output::Output {
    use granary_display::session::Timing;
    let marker = crate::paths::local_dir().join(granary_display::marker::FILE_NAME);
    let handle = app.handle().clone();
    let settings: super::output::SettingsOf = Box::new(move || {
        handle
            .try_state::<crate::settings::SettingsStore>()
            .map(|s| s.current())
            .unwrap_or_default()
    });
    #[cfg(windows)]
    {
        use granary_display::win::{WindowsDisplayController, gdi_name_for_window};
        let main = app
            .get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map_or(0, |h| h.0 as isize);
        super::output::Output::new(
            Some(Arc::new(WindowsDisplayController)),
            marker,
            Timing::default(),
            Box::new(move || gdi_name_for_window(main)),
            settings,
        )
    }
    #[cfg(not(windows))]
    {
        super::output::Output::new(None, marker, Timing::default(), Box::new(|| None), settings)
    }
}

/// One monitor for the per-monitor display settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorView {
    /// Settings key (reboot-stable monitor device path).
    pub key: String,
    pub name: String,
    pub hdr_supported: bool,
    pub hdr_enabled: bool,
    pub refresh_hz: f64,
    /// Refresh rates at the current resolution.
    pub modes_hz: Vec<f64>,
    /// DXGI peak / black luminance (nits); unstable for ~1.5 s after an HDR toggle.
    pub peak_nits: f32,
    pub black_nits: f32,
    pub sdr_white_nits: f64,
    /// The player window is on this monitor.
    pub current: bool,
}

#[tauri::command]
pub async fn display_list(app: AppHandle) -> CmdResult<Vec<MonitorView>> {
    #[cfg(windows)]
    {
        use granary_display::DisplayController;
        use granary_display::win::{WindowsDisplayController, gdi_name_for_window};
        let main = app
            .get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map(|h| h.0 as isize);
        let list = tauri::async_runtime::spawn_blocking(move || {
            let current = main.and_then(gdi_name_for_window);
            WindowsDisplayController.displays().map(|ds| {
                ds.into_iter()
                    .map(|d| MonitorView {
                        modes_hz: WindowsDisplayController::refresh_rates(&d.gdi_name)
                            .into_iter()
                            .map(|r| (r.hz() * 1000.0).round() / 1000.0)
                            .collect(),
                        current: current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&d.gdi_name)),
                        key: d.monitor_path,
                        name: d.friendly_name,
                        hdr_supported: d.hdr_supported,
                        hdr_enabled: d.hdr_enabled,
                        refresh_hz: (d.refresh.hz() * 1000.0).round() / 1000.0,
                        peak_nits: d.max_luminance,
                        black_nits: d.min_luminance,
                        sdr_white_nits: d.sdr_white_nits,
                    })
                    .collect::<Vec<_>>()
            })
        })
        .await
        .map_err(|e| CommandError::plain("internal", e.to_string()))?;
        list.map_err(|e| CommandError::plain("display_error", e.to_string()))
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn player_audio_devices(host: State<'_, PlayerHost>) -> CmdResult<Vec<super::host::AudioDevice>> {
    Ok(host.ask(Cmd::AudioDevices).await.unwrap_or_default())
}

fn host_error(e: String) -> CommandError {
    CommandError::plain("player_error", e)
}

#[tauri::command]
pub async fn player_play(
    host: State<'_, PlayerHost>,
    silo: State<'_, SiloState>,
    request: PlayRequest,
) -> CmdResult<()> {
    request.validate()?;
    let session = silo.with_session(&request.server_id, |s| async move { Ok(s) }).await?;
    let profile_id = session.profile().map(|p| p.id).ok_or(ApiError::ProfileRequired)?;
    tracing::info!(
        item_id = %request.item_id,
        file_id = %request.file_id,
        start = request.start_seconds,
        audio = ?request.audio_index,
        subtitle = ?request.subtitle_index,
        "play requested"
    );
    let args = StartArgs {
        item_id: request.item_id.clone(),
        file_id: request.file_id.clone(),
        profile_id,
        start_seconds: request.start_seconds,
        audio_index: request.audio_index,
        subtitle_index: request.subtitle_index,
    };
    let item = MediaItem {
        title: request.title.clone().unwrap_or_default(),
        subtitle: request.subtitle.clone().unwrap_or_default(),
        has_previous: request.has_previous,
        has_next: request.has_next,
    };
    let r = host
        .ask(|reply| Cmd::Play {
            session,
            args,
            item,
            reply,
        })
        .await
        .ok_or_else(|| CommandError::internal("player host stopped".into()))?;
    r.map_err(|(code, message)| CommandError::plain(&code, message))
}

#[tauri::command]
pub async fn player_stop(host: State<'_, PlayerHost>) -> CmdResult<()> {
    host.ask(|r| Cmd::Stop { reply: Some(r) }).await;
    Ok(())
}

#[tauri::command]
pub fn player_pause(host: State<'_, PlayerHost>, paused: Option<bool>) {
    host.send(Cmd::Pause(paused));
}

#[tauri::command]
pub async fn player_seek(host: State<'_, PlayerHost>, seconds: f64, relative: bool) -> CmdResult<()> {
    if !seconds.is_finite() {
        return Err(CommandError::invalid("seek target".into()));
    }
    host.ask(|reply| Cmd::Seek {
        seconds,
        relative,
        reply,
    })
    .await
    .unwrap_or_else(|| Err("player host stopped".into()))
    .map_err(host_error)
}

#[tauri::command]
pub async fn player_set(host: State<'_, PlayerHost>, name: String, value: String) -> CmdResult<()> {
    host.ask(|reply| Cmd::Set { name, value, reply })
        .await
        .unwrap_or_else(|| Err("player host stopped".into()))
        .map_err(host_error)
}

#[tauri::command]
pub fn player_audio(host: State<'_, PlayerHost>, index: u32) {
    host.send(Cmd::Audio(index));
}

#[tauri::command]
pub async fn player_subtitle(host: State<'_, PlayerHost>, choice: SubtitleChoice) -> CmdResult<()> {
    host.ask(|reply| Cmd::Subtitle { choice, reply })
        .await
        .unwrap_or_else(|| Err("player host stopped".into()))
        .map_err(host_error)
}

#[tauri::command]
pub async fn player_stats(host: State<'_, PlayerHost>) -> CmdResult<serde_json::Value> {
    Ok(host.ask(Cmd::Stats).await.unwrap_or_default())
}

#[tauri::command]
pub async fn player_state(host: State<'_, PlayerHost>) -> CmdResult<super::host::UiState> {
    Ok(host.ask(Cmd::State).await.unwrap_or_default())
}

/// Idle overlay: hide the WebView (and with it every static element) over the video; activity shows it again.
#[tauri::command]
pub fn player_overlay(app: AppHandle, visible: bool) -> CmdResult<()> {
    #[cfg(windows)]
    app.run_on_main_thread(move || super::video::set_overlay_visible(visible))
        .map_err(|e| CommandError::internal(e.to_string()))?;
    #[cfg(not(windows))]
    let _ = (app, visible);
    Ok(())
}

fn window_state(app: &AppHandle) -> CmdResult<WindowState> {
    let w = app
        .get_webview_window("main")
        .ok_or_else(|| CommandError::internal("main window missing".into()))?;
    let mini = app
        .try_state::<WindowModes>()
        .is_some_and(|m| m.mini.lock().is_ok_and(|g| g.is_some()));
    Ok(WindowState {
        fullscreen: w.is_fullscreen().unwrap_or(false),
        mini,
    })
}

fn emit_window(app: &AppHandle) -> CmdResult<WindowState> {
    let s = window_state(app)?;
    let _ = app.emit(EVENT_WINDOW, &s);
    Ok(s)
}

#[tauri::command]
pub fn player_fullscreen(app: AppHandle, fullscreen: Option<bool>) -> CmdResult<WindowState> {
    let w = app
        .get_webview_window("main")
        .ok_or_else(|| CommandError::internal("main window missing".into()))?;
    let next = fullscreen.unwrap_or(!w.is_fullscreen().unwrap_or(false));
    if next {
        // Fullscreen and the mini player exclude each other.
        let _ = player_mini(app.clone(), false);
    }
    w.set_fullscreen(next)
        .map_err(|e| CommandError::internal(e.to_string()))?;
    emit_window(&app)
}

/// The main window's minimum size (`tauri.conf.json` minWidth/minHeight), lifted while the mini player is shown.
const MIN_WINDOW: tauri::LogicalSize<f64> = tauri::LogicalSize {
    width: 960.0,
    height: 600.0,
};

/// Always-on-top mini player in the bottom-right corner of the current monitor; `false` restores the window.
#[tauri::command]
pub fn player_mini(app: AppHandle, enabled: bool) -> CmdResult<WindowState> {
    let w = app
        .get_webview_window("main")
        .ok_or_else(|| CommandError::internal("main window missing".into()))?;
    let modes = app
        .try_state::<WindowModes>()
        .ok_or_else(|| CommandError::internal("window modes missing".into()))?;
    let err = |e: tauri::Error| CommandError::internal(e.to_string());
    let mut saved = modes
        .mini
        .lock()
        .map_err(|_| CommandError::internal("window modes poisoned".into()))?;
    match (enabled, saved.is_some()) {
        (true, false) => {
            if w.is_fullscreen().unwrap_or(false) {
                w.set_fullscreen(false).map_err(err)?;
            }
            let maximized = w.is_maximized().unwrap_or(false);
            if maximized {
                w.unmaximize().map_err(err)?;
            }
            *saved = Some((
                w.outer_position().map_err(err)?,
                w.outer_size().map_err(err)?,
                maximized,
            ));
            let scale = w.scale_factor().unwrap_or(1.0);
            // The configured minimum size would clamp the mini player to 960x600 (phase-6 review M4).
            w.set_min_size(None::<tauri::Size>).map_err(err)?;
            w.set_size(PhysicalSize::new((480.0 * scale) as u32, (270.0 * scale) as u32))
                .map_err(err)?;
            // Place it by the size Windows actually applied (frame included), not the requested one.
            let size = w.outer_size().map_err(err)?;
            if let Ok(Some(m)) = w.current_monitor() {
                let area = m.work_area();
                let margin = (24.0 * scale) as i32;
                w.set_position(PhysicalPosition::new(
                    area.position.x + area.size.width as i32 - size.width as i32 - margin,
                    area.position.y + area.size.height as i32 - size.height as i32 - margin,
                ))
                .map_err(err)?;
            }
            w.set_always_on_top(true).map_err(err)?;
        }
        (false, true) => {
            if let Some((pos, size, maximized)) = *saved {
                w.set_always_on_top(false).map_err(err)?;
                w.set_size(size).map_err(err)?;
                w.set_position(pos).map_err(err)?;
                w.set_min_size(Some(MIN_WINDOW)).map_err(err)?;
                if maximized {
                    w.maximize().map_err(err)?;
                }
                // Forget the geometry only once it is back, so a failed restore can be retried.
                *saved = None;
            }
        }
        _ => {}
    }
    drop(saved);
    emit_window(&app)
}

#[tauri::command]
pub fn player_window(app: AppHandle) -> CmdResult<WindowState> {
    window_state(&app)
}
