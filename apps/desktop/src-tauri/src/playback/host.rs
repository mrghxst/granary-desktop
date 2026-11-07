//! The player host: one long-lived task owns the mpv instance, the media relay and the current [`Controller`]. UI
//! commands, mpv events (from the mpv event thread), media keys and a 2 Hz tick all arrive on one channel, so
//! playback state is never shared between threads.
use super::controller::{Controller, Environment, Notice, Sink, StartArgs, SubtitleChoice};
use super::output::{Output, run_blocking};
use super::relay::Relay;
use granary_player::{MpvEvent, Phase, PlayerBackend, PropValue};
use granary_silo_api::Session;
use granary_silo_api::playback::PlaybackSource;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

pub const EVENT_STATE: &str = "player://state";
pub const EVENT_NOTICE: &str = "player://notice";
pub const EVENT_MEDIA_KEY: &str = "player://media-key";
const TICK: Duration = Duration::from_millis(500);
/// Position updates to the UI at most this often (pause, phase and volume changes go out at once).
const POSITION_EMIT: Duration = Duration::from_millis(250);
/// Display changes made for playback are kept this long after the player goes idle, so the next episode (autoplay,
/// up next) does not switch HDR/refresh off and on again; then they are restored (ADR-0006 amendment 1).
const RELEASE_GRACE: Duration = Duration::from_secs(10);
/// Window move events are coalesced; the owning monitor is checked this long after the last one.
const MOVE_SETTLE: Duration = Duration::from_millis(400);

/// Extra mpv properties observed for the UI and the logs (the state machine observes `OBSERVED`).
pub const UI_PROPERTIES: &[(&str, granary_player::state::ObserveFormat)] = {
    use granary_player::state::ObserveFormat::*;
    &[
        ("speed", Double),
        ("hwdec-current", Str),
        ("current-vo", Str),
        ("sub-delay", Double),
        ("sub-scale", Double),
        ("aid", Str),
        ("sid", Str),
        ("video-aspect-override", Str),
        ("video-zoom", Double),
        ("video-params/gamma", Str),
        ("video-params/primaries", Str),
    ]
};

/// Properties the UI may set (name → validator). Anything else is refused.
fn validate_property(name: &str, value: &str) -> Result<String, String> {
    let num = |lo: f64, hi: f64| -> Result<String, String> {
        let v: f64 = value.parse().map_err(|_| format!("{name}: not a number"))?;
        if v.is_finite() && (lo..=hi).contains(&v) {
            Ok(format!("{v}"))
        } else {
            Err(format!("{name}: out of range {lo}..{hi}"))
        }
    };
    let flag = || match value {
        "yes" | "no" => Ok(value.to_owned()),
        _ => Err(format!("{name}: yes or no")),
    };
    match name {
        "volume" => num(0.0, 150.0),
        "speed" => num(0.25, 4.0),
        "sub-delay" | "audio-delay" => num(-600.0, 600.0),
        "sub-scale" => num(0.3, 3.0),
        "sub-pos" => num(0.0, 150.0),
        "video-zoom" => num(-2.0, 2.0),
        "panscan" => num(0.0, 1.0),
        "mute" | "sub-visibility" | "deband" => flag(),
        "video-aspect-override" => match value {
            "no" | "-1" | "16:9" | "4:3" | "1.85:1" | "2.35:1" | "2.39:1" | "21:9" => Ok(value.to_owned()),
            _ => Err("aspect: unsupported value".into()),
        },
        _ => Err(format!("property {name} cannot be set")),
    }
}

/// Stats overlay (`i`): mpv properties read on demand.
const STATS: &[&str] = &[
    "time-pos",
    "audio-pts",
    "core-idle",
    "estimated-frame-number",
    "file-format",
    "video-codec",
    "video-params/w",
    "video-params/h",
    "video-params/pixelformat",
    "video-params/primaries",
    "video-params/gamma",
    "video-params/colormatrix",
    "video-params/sig-peak",
    "video-params/max-cll",
    "video-params/max-fall",
    "current-tracks/video/dolby-vision-profile",
    "current-tracks/video/dolby-vision-level",
    "video-out-params/primaries",
    "video-out-params/gamma",
    "video-target-params/primaries",
    "video-target-params/gamma",
    "video-target-params/max-luma",
    "target-peak",
    "hdr-reference-white",
    "video-sync",
    "display-sync-active",
    "hwdec-current",
    "current-vo",
    "current-gpu-context",
    "container-fps",
    "estimated-vf-fps",
    "display-fps",
    "frame-drop-count",
    "decoder-frame-drop-count",
    "vo-delayed-frame-count",
    "avsync",
    "audio-codec-name",
    "audio-params/samplerate",
    "audio-params/channel-count",
    "audio-params/hr-channels",
    "current-ao",
    "audio-device",
    "audio-exclusive",
    "audio-spdif",
    "video-bitrate",
    "audio-bitrate",
    "demuxer-cache-duration",
    "cache-speed",
];

/// Snapshot sent to the UI on every relevant change.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    /// A playback attempt is in progress (the player view is up).
    pub active: bool,
    pub item_id: Option<String>,
    pub file_id: Option<String>,
    pub phase: Option<Phase>,
    /// Source-timeline seconds.
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub buffering: bool,
    pub volume: f64,
    pub muted: bool,
    /// Other observed properties (`speed`, `hwdec-current`, `sub-delay`, ...).
    pub props: BTreeMap<String, serde_json::Value>,
}

/// Where the host sends things (Tauri in the app, a recorder in tests).
pub trait Ui: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
    /// Overlay/cursor policy and media controls follow playback.
    fn playing_changed(&self, active: bool, playing: bool, item: Option<&MediaItem>);
    fn position_changed(&self, _playing: bool, _position: f64, _duration: f64) {}
}

/// What the Windows media flyout shows.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaItem {
    pub title: String,
    pub subtitle: String,
    pub has_previous: bool,
    pub has_next: bool,
}

/// Creates the mpv backend on first use; events must be sent into the given channel.
pub type BackendFactory =
    Box<dyn Fn(mpsc::UnboundedSender<Cmd>) -> Result<Arc<dyn PlayerBackend>, String> + Send + Sync>;

pub enum Cmd {
    Play {
        session: Arc<Session>,
        args: StartArgs,
        item: MediaItem,
        reply: oneshot::Sender<Result<(), (String, String)>>,
    },
    Stop {
        reply: Option<oneshot::Sender<()>>,
    },
    Pause(Option<bool>),
    Seek {
        seconds: f64,
        relative: bool,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Set {
        name: String,
        value: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Audio(u32),
    Subtitle {
        choice: SubtitleChoice,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Stats(oneshot::Sender<serde_json::Value>),
    State(oneshot::Sender<UiState>),
    MediaKey(&'static str),
    Event(MpvEvent),
    /// mpv's event thread ended (the core shut down).
    BackendGone,
    /// Settings were saved: apply the audio options live.
    SettingsChanged,
    /// The main window moved or changed DPI: re-check the monitor it is on.
    WindowMoved,
    /// `audio-device-list` (name, description) for the settings page.
    AudioDevices(oneshot::Sender<Vec<AudioDevice>>),
    /// App exit: stop playback and restore the display state at once.
    Shutdown(oneshot::Sender<()>),
}

/// One mpv audio output device.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

#[derive(Clone)]
pub struct PlayerHost {
    tx: mpsc::UnboundedSender<Cmd>,
}

struct UiSink {
    ui: Arc<dyn Ui>,
    output: Arc<Output>,
}

impl Sink for UiSink {
    fn notice(&self, notice: Notice) {
        self.ui
            .emit(EVENT_NOTICE, serde_json::to_value(&notice).unwrap_or_default());
    }

    fn before_load(&self, source: &PlaybackSource) -> Vec<(String, String)> {
        self.output.prepare(source)
    }
}

struct Runner {
    ui: Arc<dyn Ui>,
    factory: BackendFactory,
    tx: mpsc::UnboundedSender<Cmd>,
    backend: Option<Arc<dyn PlayerBackend>>,
    relay: Option<Arc<Relay>>,
    env: Arc<dyn Fn() -> Environment + Send + Sync>,
    controller: Option<Controller>,
    output: Arc<Output>,
    item: MediaItem,
    state: UiState,
    last_emit: Instant,
    last_emitted: UiState,
    hwdec_logged: Option<String>,
    /// The player went idle at this moment (display changes are released after [`RELEASE_GRACE`]).
    idle_since: Option<Instant>,
    /// Re-check the window's monitor at this moment (coalesced move events).
    move_check_at: Option<Instant>,
}

impl PlayerHost {
    /// Start the host task on the current tokio runtime.
    pub fn start(
        ui: Arc<dyn Ui>,
        factory: BackendFactory,
        env: Arc<dyn Fn() -> Environment + Send + Sync>,
        output: Arc<Output>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let runner = Runner {
            ui,
            factory,
            tx: tx.clone(),
            backend: None,
            relay: None,
            env,
            controller: None,
            output,
            item: MediaItem::default(),
            state: UiState::default(),
            last_emit: Instant::now(),
            last_emitted: UiState::default(),
            hwdec_logged: None,
            idle_since: None,
            move_check_at: None,
        };
        tauri::async_runtime::spawn(runner.run(rx));
        Self { tx }
    }

    pub fn send(&self, cmd: Cmd) {
        if self.tx.send(cmd).is_err() {
            tracing::error!("player host task is gone");
        }
    }

    pub async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> Option<T> {
        let (reply, rx) = oneshot::channel();
        self.send(make(reply));
        rx.await.ok()
    }

    /// Stop playback (server stop included), restore the display state and wait for it, at most `timeout`. Used on
    /// exit; a restore that does not finish in time is repaired from the marker at the next start.
    pub fn shutdown_blocking(&self, timeout: Duration) {
        let (reply, rx) = oneshot::channel();
        self.send(Cmd::Shutdown(reply));
        let done = tauri::async_runtime::block_on(async { tokio::time::timeout(timeout, rx).await.is_ok() });
        if !done {
            tracing::warn!("playback did not stop in time at exit");
        }
    }
}

impl Runner {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Cmd>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    Some(cmd) => self.handle(cmd).await,
                    None => break,
                },
                _ = tick.tick() => {
                    if let Some(c) = self.controller.as_mut() {
                        c.tick(Instant::now()).await;
                    }
                    self.after_controller();
                    self.housekeeping(Instant::now());
                    self.emit_state(false);
                }
            }
        }
    }

    fn backend(&mut self) -> Result<Arc<dyn PlayerBackend>, String> {
        if let Some(b) = &self.backend {
            return Ok(b.clone());
        }
        let b = (self.factory)(self.tx.clone())?;
        self.backend = Some(b.clone());
        Ok(b)
    }

    async fn relay(&mut self) -> Result<Arc<Relay>, String> {
        if let Some(r) = &self.relay {
            return Ok(r.clone());
        }
        let r = Arc::new(Relay::start().await.map_err(|e| e.to_string())?);
        self.relay = Some(r.clone());
        Ok(r)
    }

    async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play {
                session,
                args,
                item,
                reply,
            } => {
                let r = self.play(session, args, item).await;
                let _ = reply.send(r);
            }
            Cmd::Stop { reply } => {
                self.stop().await;
                if let Some(r) = reply {
                    let _ = r.send(());
                }
            }
            Cmd::Pause(p) => {
                if let Some(b) = self.active_backend() {
                    let paused = p.unwrap_or(!self.state.paused);
                    if let Err(e) = b.set_pause(paused) {
                        tracing::warn!(error = %e, "pause not applied");
                    }
                }
            }
            Cmd::Seek {
                seconds,
                relative,
                reply,
            } => {
                let r = match self.controller.as_mut() {
                    Some(c) => {
                        let target = if relative {
                            c.source_position() + seconds
                        } else {
                            seconds
                        };
                        let target = match c.state().duration.map(|d| c.timeline().source_position(d)) {
                            Some(end) if end > 1.0 => target.clamp(0.0, end - 0.5),
                            _ => target.max(0.0),
                        };
                        c.seek(target).await.map_err(|e| e.to_string())
                    }
                    None => Err("nothing is playing".into()),
                };
                let _ = reply.send(r);
            }
            Cmd::Set { name, value, reply } => {
                let r = match (validate_property(&name, &value), self.active_backend()) {
                    (Ok(v), Some(b)) => b.set_property(&name, &v).map_err(|e| e.to_string()),
                    (Err(e), _) => Err(e),
                    (_, None) => Err("nothing is playing".into()),
                };
                let _ = reply.send(r);
            }
            Cmd::Audio(index) => {
                if let Some(c) = self.controller.as_mut()
                    && let Err(e) = c.select_audio(index).await
                {
                    tracing::warn!(code = %e.code(), "audio track change failed");
                    let notice = Notice::Error {
                        code: e.code(),
                        message: e.to_string(),
                        fatal: false,
                    };
                    self.ui
                        .emit(EVENT_NOTICE, serde_json::to_value(&notice).unwrap_or_default());
                }
            }
            Cmd::Subtitle { choice, reply } => {
                let r = match self.controller.as_mut() {
                    Some(c) => c.select_subtitle(&choice).await.map_err(|e| e.to_string()),
                    None => Err("nothing is playing".into()),
                };
                let _ = reply.send(r);
            }
            Cmd::Stats(reply) => {
                let _ = reply.send(self.stats());
            }
            Cmd::State(reply) => {
                let _ = reply.send(self.state.clone());
            }
            Cmd::MediaKey(k) => match k {
                "play" => self.handle_pause(Some(false)),
                "pause" => self.handle_pause(Some(true)),
                "stop" => self.stop().await,
                other => self.ui.emit(EVENT_MEDIA_KEY, serde_json::json!(other)),
            },
            Cmd::Event(ev) => self.on_event(ev).await,
            Cmd::BackendGone => {
                tracing::error!("the player core shut down; it will be restarted for the next playback");
                if let Some(c) = self.controller.as_mut() {
                    c.on_event(&MpvEvent::Shutdown).await;
                }
                self.backend = None;
                self.after_controller();
                self.emit_state(true);
            }
            Cmd::SettingsChanged => {
                if let Some(b) = self.active_backend() {
                    for (name, value) in self.output.audio_now() {
                        if let Err(e) = b.set_property(&name, &value) {
                            tracing::warn!(option = %name, error = %e, "audio option not applied live");
                        }
                    }
                }
            }
            Cmd::WindowMoved => {
                if self.controller.is_some() {
                    self.move_check_at = Some(Instant::now() + MOVE_SETTLE);
                }
            }
            Cmd::AudioDevices(reply) => {
                let _ = reply.send(self.audio_devices());
            }
            Cmd::Shutdown(reply) => {
                self.stop().await;
                self.idle_since = None;
                let output = self.output.clone();
                run_blocking(move || output.release());
                let _ = reply.send(());
            }
        }
    }

    /// Idle release of held display changes and coalesced monitor checks.
    fn housekeeping(&mut self, now: Instant) {
        if self.controller.is_none()
            && let Some(since) = self.idle_since
            && now.duration_since(since) >= RELEASE_GRACE
        {
            self.idle_since = None;
            let output = self.output.clone();
            run_blocking(move || output.release());
        }
        if let Some(at) = self.move_check_at
            && now >= at
        {
            self.move_check_at = None;
            if let Some(b) = self.active_backend() {
                let output = self.output.clone();
                if let Some(options) = run_blocking(move || output.monitor_moved()) {
                    for (name, value) in options {
                        if let Err(e) = b.set_property(&name, &value) {
                            tracing::warn!(option = %name, error = %e, "display option not applied after move");
                        }
                    }
                }
            }
        }
    }

    fn audio_devices(&mut self) -> Vec<AudioDevice> {
        let Ok(b) = self.backend() else {
            return Vec::new();
        };
        b.get_property("audio-device-list")
            .and_then(|json| serde_json::from_str::<Vec<AudioDevice>>(&json).ok())
            .unwrap_or_default()
    }

    fn handle_pause(&mut self, paused: Option<bool>) {
        if let Some(b) = self.active_backend() {
            let _ = b.set_pause(paused.unwrap_or(!self.state.paused));
        }
    }

    fn active_backend(&self) -> Option<Arc<dyn PlayerBackend>> {
        self.controller.as_ref().and(self.backend.clone())
    }

    async fn play(&mut self, session: Arc<Session>, args: StartArgs, item: MediaItem) -> Result<(), (String, String)> {
        // Stop the previous attempt but keep its display changes: the new plan reuses them when it has the same needs.
        if let Some(mut c) = self.controller.take() {
            c.stop().await;
        }
        self.idle_since = None;
        let fail = |code: &str, e: String| (code.to_owned(), e);
        let backend = self.backend().map_err(|e| fail("player_unavailable", e))?;
        let relay = self.relay().await.map_err(|e| fail("internal", e))?;
        let env = (self.env)();
        self.state = UiState {
            active: true,
            item_id: Some(args.item_id.clone()),
            file_id: Some(args.file_id.clone()),
            phase: Some(Phase::Loading),
            position: args.start_seconds,
            volume: self.state.volume,
            muted: self.state.muted,
            props: self.state.props.clone(),
            ..UiState::default()
        };
        self.item = item;
        self.ui.playing_changed(true, false, Some(&self.item));
        self.emit_state(true);
        let sink = Arc::new(UiSink {
            ui: self.ui.clone(),
            output: self.output.clone(),
        });
        match Controller::begin(session, backend, relay, sink, env, args).await {
            Ok(c) => {
                self.controller = Some(c);
                self.hwdec_logged = None;
                Ok(())
            }
            Err(e) => {
                tracing::warn!(code = %e.code(), error = %e, "playback could not start");
                self.state = UiState {
                    volume: self.state.volume,
                    muted: self.state.muted,
                    ..UiState::default()
                };
                self.ui.playing_changed(false, false, None);
                self.emit_state(true);
                self.idle_since = Some(Instant::now());
                Err((e.code(), e.to_string()))
            }
        }
    }

    async fn stop(&mut self) {
        if let Some(mut c) = self.controller.take() {
            c.stop().await;
        }
        self.after_controller();
    }

    /// Clean up once the controller finished (stop, EOF, fatal error).
    fn after_controller(&mut self) {
        if self.controller.as_ref().is_some_and(Controller::is_finished) {
            self.controller = None;
        }
        if self.controller.is_none() && self.state.active {
            self.state.active = false;
            self.idle_since = Some(Instant::now());
            self.state.phase = Some(Phase::Idle);
            self.ui.playing_changed(false, false, None);
            self.emit_state(true);
        }
    }

    async fn on_event(&mut self, ev: MpvEvent) {
        match &ev {
            MpvEvent::Log { prefix, level, text } => {
                let t = text.trim_end();
                match level.as_str() {
                    "fatal" | "error" => tracing::error!(target: "mpv", "[{prefix}] {t}"),
                    "warn" => tracing::warn!(target: "mpv", "[{prefix}] {t}"),
                    _ if prefix == "vd" || t.contains("hardware decoding") || t.contains("Using ") => {
                        tracing::info!(target: "mpv", "[{prefix}] {t}")
                    }
                    _ => {}
                }
                return;
            }
            MpvEvent::Property { name, value } => {
                if UI_PROPERTIES.iter().any(|(n, _)| n == name) {
                    let v = match value {
                        PropValue::Flag(b) => serde_json::json!(b),
                        PropValue::Int(i) => serde_json::json!(i),
                        PropValue::Double(d) => serde_json::json!(d),
                        PropValue::Str(s) => serde_json::json!(s),
                        PropValue::None => serde_json::Value::Null,
                    };
                    if name == "hwdec-current"
                        && let PropValue::Str(s) = value
                        && self.hwdec_logged.as_deref().is_some_and(|l| l != s)
                    {
                        tracing::info!(hwdec_current = %s, "mpv hardware decoding changed");
                        self.hwdec_logged = Some(s.clone());
                    }
                    self.state.props.insert(name.clone(), v);
                }
            }
            MpvEvent::FileLoaded | MpvEvent::VideoReconfig => {
                #[cfg(windows)]
                {
                    let n = super::video::disable_mpv_children();
                    if n > 0 {
                        tracing::debug!(n, "mpv child windows disabled for input routing");
                    }
                }
            }
            // Once per session, at the first frame: the decoder of this file (a reused core keeps the property
            // value, so a change notification may never come).
            MpvEvent::PlaybackRestart if self.hwdec_logged.is_none() && self.controller.is_some() => {
                let hw = self
                    .state
                    .props
                    .get("hwdec-current")
                    .and_then(|v| v.as_str())
                    .unwrap_or("no");
                tracing::info!(hwdec_current = %hw, "mpv hardware decoding");
                self.hwdec_logged = Some(hw.to_owned());
            }
            _ => {}
        }
        if let Some(c) = self.controller.as_mut() {
            c.on_event(&ev).await;
            let s = c.state();
            let duration = s.duration.map(|d| c.timeline().source_position(d));
            self.state.phase = Some(s.phase);
            self.state.position = c.source_position();
            self.state.duration = duration;
            self.state.paused = s.paused;
            self.state.buffering = s.buffering;
            self.state.volume = s.volume;
            self.state.muted = s.muted;
        }
        self.after_controller();
        self.emit_state(false);
    }

    fn stats(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        if let Some(b) = self.active_backend() {
            for p in STATS {
                if let Some(v) = b.get_property(p) {
                    m.insert((*p).to_owned(), serde_json::json!(v));
                }
            }
        }
        if let Some(c) = &self.controller {
            m.insert("session".into(), serde_json::json!(c.session_id()));
            let src = c.source();
            if let Some(r) = &src.dynamic_range {
                m.insert("source-dynamic-range".into(), serde_json::json!(r));
            }
            // Dolby Vision profile as the server probed it (container value from mpv is listed separately).
            if let Some(p) = src.dolby_vision_profile {
                m.insert("dolby-vision-profile".into(), serde_json::json!(format!("P{p}")));
            }
            if let Some(f) = src.frame_rate {
                m.insert("source-frame-rate".into(), serde_json::json!(format!("{f:.3}")));
            }
        }
        if m.get("video-params/colormatrix").and_then(|v| v.as_str()) == Some("dolbyvision") {
            m.insert("dolby-vision-rpu".into(), serde_json::json!("active"));
        }
        for (k, v) in self.output.summary() {
            m.insert(k, serde_json::json!(v));
        }
        serde_json::Value::Object(m)
    }

    fn emit_state(&mut self, force: bool) {
        let s = &self.state;
        let o = &self.last_emitted;
        let important = s.active != o.active
            || s.phase != o.phase
            || s.paused != o.paused
            || s.buffering != o.buffering
            || s.volume != o.volume
            || s.muted != o.muted
            || s.duration != o.duration
            || s.props != o.props;
        let moved = s.position != o.position && self.last_emit.elapsed() >= POSITION_EMIT;
        if !(force || important || moved) {
            return;
        }
        let playing = s.active && s.phase == Some(Phase::Playing);
        if s.active && (s.paused != o.paused || s.phase != o.phase) {
            self.ui.playing_changed(true, playing, None);
        }
        self.ui.position_changed(playing, s.position, s.duration.unwrap_or(0.0));
        self.ui.emit(EVENT_STATE, serde_json::to_value(s).unwrap_or_default());
        self.last_emitted = s.clone();
        self.last_emit = Instant::now();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;
    use granary_player::MockBackend;
    use granary_silo_api::Secret;
    use granary_silo_api::credentials::MemoryStore;
    use granary_silo_api::mock::MockSiloApi;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<(String, serde_json::Value)>>,
        playing: Mutex<Vec<(bool, bool)>>,
    }

    impl Ui for Recorder {
        fn emit(&self, event: &str, payload: serde_json::Value) {
            self.events.lock().unwrap().push((event.into(), payload));
        }
        fn playing_changed(&self, active: bool, playing: bool, _item: Option<&MediaItem>) {
            self.playing.lock().unwrap().push((active, playing));
        }
    }

    fn no_output() -> Arc<Output> {
        Arc::new(Output::new(
            None,
            std::env::temp_dir().join(format!("granary-host-none-{}.json", std::process::id())),
            granary_display::session::Timing::default(),
            Box::new(|| None),
            Box::new(crate::settings::Settings::default),
        ))
    }

    async fn demo_session() -> Session {
        let api = Arc::new(MockSiloApi::demo());
        let (session, _) = Session::login(
            api,
            Arc::new(MemoryStore::default()),
            "https://media.example.test",
            "srv",
            "alice",
            &Secret::new("pw"),
        )
        .await
        .unwrap();
        session.select_profile("1", None).await.unwrap();
        session
    }

    #[tokio::test]
    async fn display_changes_are_held_after_stop_and_restored_at_shutdown() {
        use granary_display::mock::MockDisplayController;
        use granary_display::{DisplayController, Rational};
        let ctrl = Arc::new(MockDisplayController::single(false, Rational::new(60, 1)));
        let dir = std::env::temp_dir().join(format!("granary-host-display-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut settings = crate::settings::Settings::default();
        settings.display.hdr_mode = "always".into(); // the demo plan is SDR
        let output = Arc::new(Output::new(
            Some(ctrl.clone() as super::super::output::Ctrl),
            dir.join(granary_display::marker::FILE_NAME),
            granary_display::session::Timing {
                hdr_timeout: Duration::ZERO,
                settle: Duration::ZERO,
            },
            Box::new(|| Some(r"\\.\DISPLAY1".to_owned())),
            Box::new(move || settings.clone()),
        ));
        let backend = Arc::new(MockBackend::default());
        let b2 = backend.clone();
        let host = PlayerHost::start(
            Arc::new(Recorder::default()),
            Box::new(move |_tx| Ok(b2.clone() as Arc<dyn PlayerBackend>)),
            Arc::new(Environment::default),
            output,
        );
        let session = Arc::new(demo_session().await);
        let args = StartArgs {
            item_id: "m1".into(),
            file_id: "m1-f1".into(),
            profile_id: "1".into(),
            start_seconds: 0.0,
            audio_index: None,
            subtitle_index: None,
        };
        let r = host
            .ask(|reply| Cmd::Play {
                session,
                args,
                item: MediaItem::default(),
                reply,
            })
            .await
            .unwrap();
        assert!(r.is_ok(), "{r:?}");
        assert!(
            ctrl.display(&ctrl.id()).unwrap().hdr_enabled,
            "HDR on before the first frame"
        );
        let calls = backend.calls();
        let opt = calls.iter().position(|c| c.starts_with("set audio-channels=stereo"));
        let load = calls.iter().position(|c| c.starts_with("cmd loadfile"));
        assert!(opt.is_some() && opt < load, "{calls:?}");
        host.ask(|r| Cmd::Stop { reply: Some(r) }).await.unwrap();
        assert!(
            ctrl.display(&ctrl.id()).unwrap().hdr_enabled,
            "held during the idle grace (next episode)"
        );
        host.ask(Cmd::Shutdown).await.unwrap();
        assert!(!ctrl.display(&ctrl.id()).unwrap().hdr_enabled, "restored at exit");
        assert!(!dir.join(granary_display::marker::FILE_NAME).exists());
    }

    #[test]
    fn only_whitelisted_properties_with_valid_values() {
        assert_eq!(validate_property("volume", "80").unwrap(), "80");
        assert!(validate_property("volume", "151").is_err());
        assert!(validate_property("volume", "NaN").is_err());
        assert_eq!(validate_property("mute", "yes").unwrap(), "yes");
        assert!(validate_property("mute", "1").is_err());
        assert!(validate_property("video-aspect-override", "16:9").is_ok());
        assert!(validate_property("video-aspect-override", "7:1").is_err());
        assert!(validate_property("sub-delay", "-1.5").is_ok());
        for bad in [
            "script",
            "stream-open-filename",
            "input-ipc-server",
            "screenshot-directory",
        ] {
            assert!(validate_property(bad, "x").is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn host_plays_reports_and_stops() {
        let api = Arc::new(MockSiloApi::demo());
        let (session, _) = Session::login(
            api.clone(),
            Arc::new(MemoryStore::default()),
            "https://media.example.test",
            "srv",
            "alice",
            &Secret::new("pw"),
        )
        .await
        .unwrap();
        session.select_profile("1", None).await.unwrap();
        let ui = Arc::new(Recorder::default());
        let backend = Arc::new(MockBackend::default());
        let b2 = backend.clone();
        let host = PlayerHost::start(
            ui.clone(),
            Box::new(move |_tx| Ok(b2.clone() as Arc<dyn PlayerBackend>)),
            Arc::new(Environment::default),
            no_output(),
        );
        let args = StartArgs {
            item_id: "m1".into(),
            file_id: "m1-f1".into(),
            profile_id: "1".into(),
            start_seconds: 0.0,
            audio_index: None,
            subtitle_index: None,
        };
        let r = host
            .ask(|reply| Cmd::Play {
                session: Arc::new(session),
                args,
                item: MediaItem::default(),
                reply,
            })
            .await
            .unwrap();
        assert!(r.is_ok(), "{r:?}");
        for ev in [
            MpvEvent::FileLoaded,
            MpvEvent::PlaybackRestart,
            MpvEvent::Property {
                name: "time-pos".into(),
                value: PropValue::Double(4.0),
            },
            MpvEvent::Property {
                name: "hwdec-current".into(),
                value: PropValue::Str("d3d11va".into()),
            },
        ] {
            host.send(Cmd::Event(ev));
        }
        let state = host.ask(Cmd::State).await.unwrap();
        assert!(state.active);
        assert_eq!(state.position, 4.0);
        assert_eq!(state.props["hwdec-current"], "d3d11va");
        let r = host
            .ask(|reply| Cmd::Set {
                name: "script".into(),
                value: "x.lua".into(),
                reply,
            })
            .await
            .unwrap();
        assert!(r.is_err());
        host.ask(|r| Cmd::Stop { reply: Some(r) }).await.unwrap();
        let state = host.ask(Cmd::State).await.unwrap();
        assert!(!state.active);
        let log = api.playback_log();
        assert_eq!(log.progress.len(), 1);
        assert_eq!(log.stops.len(), 1);
        let events = ui.events.lock().unwrap();
        assert!(events.iter().any(|(e, p)| e == EVENT_NOTICE && p["kind"] == "plan"));
        assert!(events.iter().any(|(e, p)| e == EVENT_NOTICE && p["kind"] == "stopped"));
        assert_eq!(ui.playing.lock().unwrap().last(), Some(&(false, false)));
    }
}
