//! One playback attempt (ADR-0008): negotiate with the server, load the plan into mpv, report sequenced progress,
//! stop exactly once, and recover from dropped connections. Driven by one task (see `playback::host`), so the
//! sequence counter and the stop id never race.
//!
//! Protocol rules followed here (silo-api.md §6 and §13, all `@6791aac`):
//! - start with the capability `installation_id` and a client-minted attempt id; refresh capabilities and retry once
//!   on `409 installation_changed`;
//! - progress samples carry strictly increasing sequence numbers and the **source** position
//!   (`Timeline::source_position`); a sample is sent when playback starts, on every pause/resume, after every seek,
//!   every [`PROGRESS_INTERVAL`] while playing and every [`PAUSED_HEARTBEAT`] while paused (server liveness: 45 s
//!   playing, 30 min paused);
//! - `410`/`404` end the attempt (the server stopped it); `409 progress_conflict` is never retried;
//! - stop is sent once with a UUID `stop_id` minted once and the final sample, retried on transient failures with the
//!   same id (a replay answers `replayed`).
use super::mint_id;
use super::relay::{MediaSource, Relay};
use granary_player::{EndReason, MpvEvent, Phase, PlayerBackend, PlayerState};
use granary_silo_api::caps::{HardwareDecoders, OutputFacts, client_capabilities, client_features, playback_context};
use granary_silo_api::media::{MediaTarget, resolve_media_url};
use granary_silo_api::playback::*;
use granary_silo_api::{ApiError, Session};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
pub const PAUSED_HEARTBEAT: Duration = Duration::from_secs(5 * 60);
/// Reloads of the same session after a dropped connection before a new attempt is negotiated.
const RELOADS_PER_SESSION: u32 = 2;
/// New attempts after the reloads are exhausted.
const RENEGOTIATIONS: u32 = 1;
/// How long `stop` waits for the server stop before the UI moves on; delivery continues in the background with the
/// same stop id (phase-6 review M2).
const STOP_WAIT: Duration = Duration::from_secs(4);

/// A progress sample in flight and its result (sent from a spawned task so a slow server never blocks the host).
type ProgressResult = (PlaybackProgressBody, Result<String, ApiError>);

/// What to play (from the UI's `PlayRequest`).
#[derive(Debug, Clone, PartialEq)]
pub struct StartArgs {
    pub item_id: String,
    pub file_id: String,
    pub profile_id: String,
    /// Source-timeline seconds; 0 = from the beginning.
    pub start_seconds: f64,
    /// Ordinal in the version's `audio_tracks`.
    pub audio_index: Option<u32>,
    /// `SubtitleTrack.index` (the probed stream index); `None` = off.
    pub subtitle_index: Option<i64>,
}

/// Facts about this machine for the capability declaration.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub app_version: String,
    pub os_version: String,
    pub output: OutputFacts,
    pub hardware: HardwareDecoders,
    /// Channels of the Windows mix format for an mpv `audio-device` (`platform::audio_mix_channels`).
    pub audio_mix_channels: Option<fn(&str) -> Option<u32>>,
}

/// What the UI is told about the negotiated plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanInfo {
    pub session_id: String,
    pub plan_id: String,
    /// `original_http | progressive | hls`.
    pub delivery: String,
    pub decision_reason: String,
    /// The original file is streamed untouched.
    pub direct_play: bool,
    /// The stream goes through the loopback relay (server origin) rather than straight to mpv.
    pub relayed: bool,
    pub protocol: String,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub dynamic_range: Option<String>,
    pub degradation_warnings: Vec<String>,
    pub subtitles: Vec<SubtitleInventoryItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notice {
    Plan(Box<PlanInfo>),
    Progress {
        sequence: i64,
        position: f64,
        paused: bool,
        outcome: String,
    },
    /// `fatal`: playback is over; otherwise informational (a reconnect, a refused sample).
    Error {
        code: String,
        message: String,
        fatal: bool,
    },
    /// The file played to its end.
    Ended,
    Stopped {
        outcome: Option<String>,
    },
}

pub trait Sink: Send + Sync {
    fn notice(&self, notice: Notice);
    /// Called once per negotiated plan, before mpv loads it (Phase 7): apply the display/audio output policy for
    /// this source and return the mpv options to set first. Blocking; the default sets nothing.
    fn before_load(&self, _source: &PlaybackSource) -> Vec<(String, String)> {
        Vec::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("the server does not offer playback protocol v3 ({0})")]
    Unsupported(String),
    #[error("not playable: {0}")]
    NotPlayable(String),
    #[error("player: {0}")]
    Player(String),
}

impl ControllerError {
    pub fn code(&self) -> String {
        match self {
            Self::Api(e) => e.code().to_owned(),
            Self::Unsupported(_) => "playback_unsupported".into(),
            Self::NotPlayable(_) => "not_playable".into(),
            Self::Player(_) => "player_error".into(),
        }
    }
}

/// Subtitle choice from the UI menu.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubtitleChoice {
    Off,
    /// An embedded track by probed stream index.
    Embedded {
        stream_index: i64,
    },
    /// A server sidecar from the plan's inventory.
    External {
        track_id: String,
    },
}

pub struct Controller {
    session: Arc<Session>,
    backend: Arc<dyn PlayerBackend>,
    relay: Arc<Relay>,
    sink: Arc<dyn Sink>,
    env: Environment,
    args: StartArgs,
    caps: PlaybackCapabilities,
    installation_id: String,
    attempt_id: String,
    session_id: String,
    plan: PlaybackPlan,
    stream_url: String,
    state: PlayerState,
    sequence: i64,
    last_progress: Option<Instant>,
    last_source_position: f64,
    /// Tracks applied for the current load.
    tracks_applied: bool,
    stop_id: Option<String>,
    stopped: bool,
    server_ended: bool,
    reloads: u32,
    renegotiations: u32,
    retry_at: Option<Instant>,
    started_reported: bool,
    /// At most one progress sample in flight (sequence order is kept).
    progress_inflight: Option<tokio::task::JoinHandle<ProgressResult>>,
    /// A sample was asked for while one was in flight; send it when that one finishes.
    progress_due: bool,
    /// An audio switch paused playback for its resync seek; resume when that seek lands.
    resume_after_switch: bool,
    /// After an audio switch resumed: watch A/V sync for a few seconds and resync again if audio is missing or
    /// drifting (the remedy a viewer would otherwise apply by seeking).
    sync_check: Option<SyncCheck>,
    /// Sidecar subtitles already handed to mpv for this load (server URL, mpv track id).
    sidecars: Vec<(String, String)>,
    /// Sidecar subtitle files, downloaded in the background as soon as the plan names them (by server URL).
    sub_files: SubFiles,
    /// `gapless-audio` value to put back once an audio switch that kept the open audio device has landed.
    gapless_restore: Option<String>,
}

/// How long selecting a sidecar subtitle waits for its background download before mpv is given the URL instead.
const SIDECAR_WAIT: Duration = Duration::from_secs(5);
/// Larger sidecar files are left to mpv (a subtitle file is rarely over a few MB, even heavily typeset ASS).
const SIDECAR_MAX: u64 = 16 << 20;

/// Sidecar subtitle downloads by server URL: one per file, shared by the background prefetch and a selection that
/// arrives while it runs. `None` inside means the download failed (mpv then reads the URL itself).
#[derive(Default, Clone)]
struct SubFiles(Arc<std::sync::Mutex<std::collections::HashMap<String, SubFile>>>);

type SubFile = Arc<tokio::sync::OnceCell<Option<Arc<[u8]>>>>;

impl SubFiles {
    fn cell(&self, raw: &str) -> SubFile {
        let mut map = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(map.entry(raw.to_owned()).or_default())
    }
}

/// Download a sidecar subtitle (through the loopback relay for server files, which adds the credentials).
async fn download_subtitle(url: String) -> Option<Arc<[u8]>> {
    static CLIENT: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()
                .ok()
        })
        .as_ref()?;
    let res = client.get(&url).send().await.ok()?.error_for_status().ok()?;
    if res.content_length().is_some_and(|n| n > SIDECAR_MAX) {
        return None;
    }
    let bytes = res.bytes().await.ok()?;
    if bytes.len() as u64 > SIDECAR_MAX {
        return None;
    }
    tracing::debug!(bytes = bytes.len(), "sidecar subtitle downloaded ahead");
    Some(Arc::from(&bytes[..]))
}

/// Lower-case hex, for mpv's `hex://` in-memory stream.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    out
}

/// How long after an audio switch the sync watchdog runs, and how often it may resync.
const SYNC_CHECK_WINDOW: Duration = Duration::from_secs(8);
const SYNC_CHECK_RETRIES: u8 = 2;
/// Audio this far from the video clock (s) after a switch counts as out of sync.
const SYNC_TOLERANCE: f64 = 0.35;

#[derive(Debug, Clone, Copy, Default)]
struct SyncCheck {
    /// Set on the first tick after the resume.
    until: Option<Instant>,
    /// Consecutive out-of-sync samples.
    bad: u8,
    retries: u8,
}

/// Outcome of delivering a stop.
enum StopDelivery {
    Stopped(String),
    /// 404/410: the server already ended the session.
    Gone,
    Failed,
}

fn is_transient(e: &ApiError) -> bool {
    match e {
        ApiError::Http { status, .. } => *status >= 500 || *status == 408 || *status == 429,
        _ => !matches!(
            e,
            ApiError::SignedOut
                | ApiError::NotSignedIn
                | ApiError::ProfileRequired
                | ApiError::ProfileVerificationRequired
        ),
    }
}

fn status_of(e: &ApiError) -> Option<u16> {
    match e {
        ApiError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

impl Controller {
    /// Negotiate and load. On success mpv is loading the plan's stream.
    pub async fn begin(
        session: Arc<Session>,
        backend: Arc<dyn PlayerBackend>,
        relay: Arc<Relay>,
        sink: Arc<dyn Sink>,
        env: Environment,
        args: StartArgs,
    ) -> Result<Self, ControllerError> {
        let caps = Self::capabilities(&session).await?;
        let installation_id = caps.installation_id.clone().unwrap_or_default();
        let mut c = Self {
            session,
            backend,
            relay,
            sink,
            env,
            args,
            caps,
            installation_id,
            attempt_id: String::new(),
            session_id: String::new(),
            plan: PlaybackPlan::default(),
            stream_url: String::new(),
            state: PlayerState::default(),
            sequence: 0,
            last_progress: None,
            last_source_position: 0.0,
            tracks_applied: false,
            resume_after_switch: false,
            sync_check: None,
            sidecars: Vec::new(),
            sub_files: SubFiles::default(),
            gapless_restore: None,
            stop_id: None,
            stopped: false,
            server_ended: false,
            reloads: 0,
            renegotiations: 0,
            retry_at: None,
            started_reported: false,
            progress_inflight: None,
            progress_due: false,
        };
        // `pause` is global in mpv and keep-open pauses at EOF: a reused core would start the next file paused while
        // the state machine (and the UI) believe it plays.
        if let Err(e) = c.backend.set_pause(false) {
            tracing::warn!(error = %e, "could not clear pause before loading");
        }
        let start = c.args.start_seconds;
        c.negotiate(start).await?;
        Ok(c)
    }

    async fn capabilities(session: &Session) -> Result<PlaybackCapabilities, ControllerError> {
        let caps = session.playback_capabilities().await?;
        if !caps.allowed || !caps.protocol_versions.contains(&PROTOCOL_VERSION) {
            return Err(ControllerError::Unsupported(format!(
                "state {}, protocols {:?}",
                caps.state, caps.protocol_versions
            )));
        }
        if caps.installation_id.as_deref().is_none_or(str::is_empty) {
            return Err(ControllerError::Unsupported("no installation id".into()));
        }
        Ok(caps)
    }

    fn start_body(&self, start_seconds: f64) -> PlaybackStartBody {
        PlaybackStartBody {
            installation_id: self.installation_id.clone(),
            protocol_version: PROTOCOL_VERSION,
            client_features: client_features(&self.caps),
            file_id: self.args.file_id.clone(),
            profile_id: self.args.profile_id.clone(),
            playback_attempt_id: self.attempt_id.clone(),
            quality_preference: "auto".into(),
            subtitle_fidelity_preference: "preserve".into(),
            client_capabilities: client_capabilities(&self.env.hardware),
            client_playback_context: playback_context(&self.env.app_version, &self.env.os_version, &self.env.output),
            start_position: Some(start_seconds.max(0.0)),
            audio_track_index: self.args.audio_index.map(i64::from),
            subtitle_track_index: self.args.subtitle_index,
            // The user picked this version; never switch it behind their back (`fixed_media_file_v1`).
            allow_alternate_versions: Some(false),
        }
    }

    /// Start a new attempt at `start_seconds` and load it.
    async fn negotiate(&mut self, start_seconds: f64) -> Result<(), ControllerError> {
        self.attempt_id = mint_id();
        let decision = match self.session.start_playback(&self.start_body(start_seconds)).await {
            Err(e) if e.problem_id() == Some("installation_changed") => {
                tracing::warn!("installation changed; refreshing playback capabilities");
                self.caps = Self::capabilities(&self.session).await?;
                self.installation_id = self.caps.installation_id.clone().unwrap_or_default();
                self.attempt_id = mint_id();
                self.session.start_playback(&self.start_body(start_seconds)).await?
            }
            other => other?,
        };
        let Some((session_id, plan)) = decision.playable() else {
            let why = decision
                .terminal
                .as_ref()
                .map(|t| format!("{}: {}", t.reason, t.message))
                .unwrap_or_else(|| decision.outcome.clone());
            return Err(ControllerError::NotPlayable(why));
        };
        self.session_id = session_id.to_owned();
        self.plan = plan.clone();
        self.sequence = 0;
        self.stop_id = None;
        self.started_reported = false;
        self.last_progress = None;
        self.progress_due = false;
        self.stopped = false;
        self.server_ended = false;
        self.reloads = 0;
        // Until mpv reports playback, the position to resume from (and to report) is the requested start.
        self.last_source_position = start_seconds.max(0.0);
        self.apply_output();
        self.load_plan()
    }

    /// Display and audio output for this source, decided before the first frame (HDR switched on, refresh set,
    /// tone-mapping target and audio options in place before `loadfile`).
    fn apply_output(&mut self) {
        let sink = Arc::clone(&self.sink);
        let source = self.plan.delivered();
        let options = super::output::run_blocking(move || sink.before_load(&source));
        for (name, value) in options {
            if let Err(e) = self.backend.set_property(&name, &value) {
                tracing::warn!(option = %name, value = %value, error = %e, "output option not applied");
            }
        }
    }

    /// The negotiated source (stats overlay: dynamic range, Dolby Vision profile).
    pub fn source(&self) -> &PlaybackSource {
        &self.plan.source
    }

    /// Resolve the stream URL (relay for the server origin, direct otherwise) and hand it to mpv.
    /// The log shows the plan URL's path and query *names* only (evidence of direct play without signatures).
    fn load_plan(&mut self) -> Result<(), ControllerError> {
        let server_url = self.plan.stream.url.clone();
        let (url, relayed) = self.media_url(&server_url)?;
        if !self.plan.stream.headers.is_empty() {
            // Upstream always sends an empty map today (plan_v3.go@6791aac); nothing else would forward them.
            tracing::warn!(count = self.plan.stream.headers.len(), "plan stream headers ignored");
        }
        self.stream_url = url;
        self.tracks_applied = false;
        self.resume_after_switch = false;
        self.sync_check = None;
        self.sidecars.clear();
        self.prefetch_sidecars();
        let info = self.plan_info(relayed);
        tracing::info!(
            session = %self.session_id,
            delivery = %info.delivery,
            reason = %info.decision_reason,
            direct_play = info.direct_play,
            relayed,
            stream = %url_shape(&server_url),
            "playback plan accepted"
        );
        self.sink.notice(Notice::Plan(Box::new(info)));
        let start = self.plan.timeline.player_start_seconds;
        self.state.load_requested();
        self.backend
            .load(&self.stream_url, Some(start))
            .map_err(|e| ControllerError::Player(e.to_string()))
    }

    fn media_url(&self, raw: &str) -> Result<(String, bool), ControllerError> {
        match resolve_media_url(self.session.origin(), raw)? {
            MediaTarget::Server(u) => {
                let source: Arc<dyn MediaSource> = Arc::new(Arc::clone(&self.session));
                Ok((self.relay.register(source, &u)?, true))
            }
            MediaTarget::Foreign(u) => Ok((u, false)),
        }
    }

    fn plan_info(&self, relayed: bool) -> PlanInfo {
        let p = &self.plan;
        PlanInfo {
            session_id: self.session_id.clone(),
            plan_id: p.plan_id.clone(),
            delivery: p.delivery.clone(),
            decision_reason: p.decision_reason.clone(),
            direct_play: p.is_direct(),
            relayed,
            protocol: p.stream.protocol.clone(),
            container: p.stream.container.clone(),
            video_codec: p.source.video_codec.clone(),
            audio_codec: p.source.audio_codec.clone(),
            dynamic_range: p.source.dynamic_range.clone(),
            degradation_warnings: p.degradation_warnings.iter().map(|w| w.code.clone()).collect(),
            subtitles: p.subtitle.inventory.clone(),
        }
    }

    pub fn state(&self) -> &PlayerState {
        &self.state
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The stream a seek-preview handle may open (ADR-0009): the original file over the local relay, direct play
    /// only (a transcode's timeline is produced on demand and seeking it would start server work).
    pub fn preview_url(&self) -> Option<&str> {
        (self.plan.is_direct() && !self.stopped && !self.stream_url.is_empty()).then_some(self.stream_url.as_str())
    }

    pub fn stream_url(&self) -> &str {
        &self.stream_url
    }

    pub fn timeline(&self) -> &Timeline {
        &self.plan.timeline
    }

    /// Source-timeline position of the player right now.
    pub fn source_position(&self) -> f64 {
        self.plan.timeline.source_position(self.state.position)
    }

    pub fn is_finished(&self) -> bool {
        self.stopped
    }

    /// Feed one mpv event.
    pub async fn on_event(&mut self, ev: &MpvEvent) {
        if self.stopped {
            return;
        }
        self.drain_progress().await;
        if self.stopped {
            return;
        }
        let before = self.state.clone();
        self.state.apply(ev);
        // Only positions mpv reported after the first frame of this load count (phase-6 review M1): right after a
        // (re)load the position is 0 until the seek to the start lands.
        if self.started_reported && matches!(self.state.phase, Phase::Playing | Phase::Paused) {
            self.last_source_position = self.source_position();
        }
        match ev {
            MpvEvent::FileLoaded => {
                self.restore_gapless();
                self.apply_tracks().await;
                self.reloads_succeeded();
            }
            MpvEvent::PlaybackRestart => {
                // The new audio chain is set up by now: the switch no longer needs the open device kept.
                self.restore_gapless();
                if std::mem::take(&mut self.resume_after_switch) {
                    self.set("pause", "no");
                    let retries = self.sync_check.map_or(0, |c| c.retries);
                    self.sync_check = Some(SyncCheck {
                        retries,
                        ..SyncCheck::default()
                    });
                }
                // First frame of the load, or a seek finished.
                if !self.started_reported || before.phase == Phase::Seeking {
                    self.started_reported = true;
                    self.last_source_position = self.source_position();
                    self.request_progress();
                }
            }
            MpvEvent::Property { name, .. } if name == "pause" => {
                if before.paused != self.state.paused && self.started_reported {
                    self.request_progress();
                }
            }
            MpvEvent::EndFile {
                reason: EndReason::Error(e),
            } => {
                tracing::warn!(error = %e, "stream ended with an error");
                self.schedule_recovery(e);
            }
            MpvEvent::EndFile { reason: EndReason::Eof } => self.finish_eof().await,
            MpvEvent::Shutdown => {
                self.sink.notice(Notice::Error {
                    code: "player_shutdown".into(),
                    message: "The player stopped unexpectedly.".into(),
                    fatal: true,
                });
                self.stop().await;
            }
            _ => {}
        }
        if before.phase != Phase::Ended && self.state.phase == Phase::Ended {
            self.finish_eof().await;
        }
    }

    fn reloads_succeeded(&mut self) {
        if self.retry_at.is_none() && self.reloads > 0 {
            tracing::info!(reloads = self.reloads, "stream reconnected");
        }
    }

    /// Called about twice a second by the runner.
    pub async fn tick(&mut self, now: Instant) {
        if self.stopped {
            return;
        }
        self.drain_progress().await;
        if self.stopped {
            return;
        }
        if let Some(at) = self.retry_at
            && now >= at
        {
            self.retry_at = None;
            self.recover().await;
            return;
        }
        self.check_sync(now);
        let interval = match self.state.phase {
            Phase::Playing | Phase::Buffering => PROGRESS_INTERVAL,
            Phase::Paused => PAUSED_HEARTBEAT,
            _ => return,
        };
        if self.started_reported && self.last_progress.is_none_or(|t| now.duration_since(t) >= interval) {
            self.request_progress();
        }
    }

    /// Select the plan's audio and subtitle tracks once mpv has the track list.
    async fn apply_tracks(&mut self) {
        if self.tracks_applied {
            return;
        }
        self.tracks_applied = true;
        if let Some(i) = self.args.audio_index
            && self.plan.is_direct()
        {
            // mpv numbers audio tracks 1.. in demux order, the server's `audio_tracks` order.
            self.set("aid", &(i + 1).to_string());
        }
        let sub = self.plan.subtitle.clone();
        match (&sub.mode[..], &sub.embedded, &sub.artifact) {
            (_, Some(e), _) if sub.mode != "off" && sub.mode != "burn_in" => match self.mpv_sub_id(e.stream_index) {
                Some(sid) => self.set("sid", &sid),
                None => {
                    tracing::warn!(
                        stream_index = e.stream_index,
                        "embedded subtitle not in the mpv track list"
                    );
                    // Never leave mpv's own pick (a default-flagged track in any language) on screen.
                    self.set("sid", "no");
                }
            },
            (_, _, Some(a)) if sub.mode != "off" && sub.mode != "burn_in" => {
                if let Err(e) = self.add_sidecar(&a.url.clone()).await {
                    tracing::warn!(error = %e, "subtitle sidecar could not be added");
                }
            }
            // Burned in (or off): never show a second copy.
            _ => self.set("sid", "no"),
        }
    }

    fn set(&self, name: &str, value: &str) {
        if let Err(e) = self.backend.set_property(name, value) {
            tracing::warn!(name, error = %e, "mpv property not applied");
        }
    }

    /// mpv's `sid` for the subtitle track whose FFmpeg stream index is `stream_index`.
    fn mpv_sub_id(&self, stream_index: i64) -> Option<String> {
        let count: usize = self.backend.get_property("track-list/count")?.parse().ok()?;
        (0..count).find_map(|n| {
            let get = |k: &str| self.backend.get_property(&format!("track-list/{n}/{k}"));
            (get("type").as_deref() == Some("sub") && get("ff-index").as_deref() == Some(&stream_index.to_string()))
                .then(|| get("id"))
                .flatten()
        })
    }

    /// Start downloading every sidecar subtitle the plan names, so selecting one later only parses it.
    fn prefetch_sidecars(&mut self) {
        let sub = &self.plan.subtitle;
        let raws: Vec<String> = sub
            .inventory
            .iter()
            .filter_map(|t| t.url.clone())
            .chain(sub.artifact.as_ref().map(|a| a.url.clone()))
            .collect();
        for raw in raws {
            let Ok((url, _)) = self.media_url(&raw) else { continue };
            let cell = self.sub_files.cell(&raw);
            if cell.initialized() {
                continue;
            }
            tokio::spawn(async move {
                cell.get_or_init(|| download_subtitle(url)).await;
            });
        }
    }

    /// Show the sidecar subtitle at `raw` (a server URL): from memory when it was downloaded ahead, so mpv only parses
    /// it; a track added before is selected again instead of stacking another copy.
    async fn add_sidecar(&mut self, raw: &str) -> Result<(), ControllerError> {
        if let Some((_, sid)) = self.sidecars.iter().find(|(r, _)| r == raw) {
            let sid = sid.clone();
            self.set("sid", &sid);
            return Ok(());
        }
        let (url, _) = self.media_url(raw)?;
        let cell = self.sub_files.cell(raw);
        let fetch = url.clone();
        // Normally done long before; a download still running is waited for briefly, then mpv reads the URL itself.
        let file = tokio::time::timeout(SIDECAR_WAIT, cell.get_or_init(|| download_subtitle(fetch)))
            .await
            .ok()
            .cloned()
            .flatten();
        let target = match &file {
            Some(bytes) => format!("hex://{}", hex(bytes)),
            None => url,
        };
        self.backend
            .command(&["sub-add", &target, "select"])
            .map_err(|e| ControllerError::Player(e.to_string()))?;
        if let Some(sid) = self.backend.get_property("sid").filter(|s| s != "no") {
            self.sidecars.push((raw.to_owned(), sid));
        }
        Ok(())
    }

    /// UI subtitle menu. Direct play switches in mpv; a remux/transcode only carries the negotiated tracks, so the
    /// server re-plans with `track_change` (phase-6 review M3, `silo-api.md` §8).
    pub async fn select_subtitle(&mut self, choice: &SubtitleChoice) -> Result<(), ControllerError> {
        if !self.plan.is_direct() {
            let subtitle = self.subtitle_identity(choice)?;
            let audio = self.plan.selected_tracks.audio.clone();
            return self.track_change(SelectedTracks { audio, subtitle }).await;
        }
        match choice {
            SubtitleChoice::Off => self.set("sid", "no"),
            SubtitleChoice::Embedded { stream_index } => {
                let sid = self
                    .mpv_sub_id(*stream_index)
                    .ok_or_else(|| ControllerError::Player("subtitle track not in this stream".into()))?;
                // Re-selecting the current track would make mpv refill it from the stream for nothing.
                if self.backend.get_property("sid").as_deref() != Some(sid.as_str()) {
                    self.set("sid", &sid);
                    // mpv's own refresh for a newly selected stream drops the whole read-ahead, re-reads from 11 s
                    // back and then waits for a 3 s buffer, so on a network stream the line on screen shows late or
                    // only after the viewer seeks. Seeking to the current position restarts every stream from the
                    // index instead (Matroska subtitle preroll finds the line already on screen), without that wait.
                    self.seek_in_place();
                }
            }
            SubtitleChoice::External { track_id } => {
                let url = self
                    .plan
                    .subtitle
                    .inventory
                    .iter()
                    .find(|t| &t.track_id == track_id)
                    .and_then(|t| t.url.clone())
                    .ok_or_else(|| ControllerError::Player("no such subtitle".into()))?;
                self.add_sidecar(&url).await?;
            }
        }
        Ok(())
    }

    /// UI audio menu: direct play carries every track of the original file, so mpv switches locally; other plans
    /// re-plan with `track_change` (phase-6 review M3).
    pub async fn select_audio(&mut self, index: u32) -> Result<(), ControllerError> {
        if !self.plan.is_direct() {
            let current = self.plan.selected_tracks.audio.clone();
            let audio = Some(TrackIdentity {
                // Keep the server's id only when it names the same ordinal; an empty id selects by index.
                id: current
                    .filter(|t| t.index == Some(i64::from(index)))
                    .map(|t| t.id)
                    .unwrap_or_default(),
                index: Some(i64::from(index)),
            });
            let subtitle = self.plan.selected_tracks.subtitle.clone();
            self.track_change(SelectedTracks { audio, subtitle }).await?;
            self.args.audio_index = Some(index);
            return Ok(());
        }
        let aid = (index + 1).to_string();
        if self.args.audio_index == Some(index) && self.backend.get_property("aid").as_deref() == Some(aid.as_str()) {
            return Ok(());
        }
        self.args.audio_index = Some(index);
        self.sync_check = None;
        if self.keep_audio_output(&aid) {
            let current = self.backend.get_property("gapless-audio");
            if current.as_deref() != Some("yes") {
                self.set("gapless-audio", "yes");
                self.gapless_restore = Some(current.unwrap_or_else(|| "weak".into()));
            }
        }
        self.resync_audio(Some(&aid));
        tracing::info!(index, "audio track switched in the player");
        Ok(())
    }

    /// A new audio track on a network stream makes mpv refill only that track while video runs on; audio then
    /// joins late and display-resample pulls it back with up to 12.5 % faster audio (heard as a speed-up), or it
    /// stays silent on slow sources. Instead: hold playback, (switch,) seek exactly to the same position so both
    /// streams preroll from one point, and resume when that seek lands (`PlaybackRestart`).
    fn resync_audio(&mut self, aid: Option<&str>) {
        let pos = self
            .backend
            .get_property("time-pos")
            .and_then(|p| p.parse::<f64>().ok());
        let was_playing = self.backend.get_property("pause").as_deref() == Some("no");
        if was_playing && pos.is_some() {
            self.set("pause", "yes");
        }
        if let Some(aid) = aid {
            self.set("aid", aid);
        }
        match pos.map(|p| self.backend.seek_absolute(p)) {
            Some(Ok(())) => self.resume_after_switch |= was_playing,
            Some(Err(e)) => {
                tracing::warn!(error = %e, "resync after the audio switch failed");
                if was_playing {
                    self.set("pause", "no");
                }
            }
            None => {}
        }
    }

    /// Re-seek to the current position (precise), for track changes mpv would otherwise refill slowly.
    fn seek_in_place(&self) {
        if !matches!(self.state.phase, Phase::Playing | Phase::Paused | Phase::Buffering) {
            return;
        }
        let Some(pos) = self
            .backend
            .get_property("time-pos")
            .and_then(|p| p.parse::<f64>().ok())
        else {
            return;
        };
        if let Err(e) = self.backend.seek_absolute(pos) {
            tracing::warn!(error = %e, "seek after the track switch failed");
        }
    }

    /// Whether the audio device already open can carry track `aid` as well as a new one would. With mpv's default
    /// (`gapless-audio=weak`) a track with another channel layout closes and reopens the Windows audio device: up to
    /// seconds of WASAPI setup, and a fresh stream that HDMI sinks and some drivers start late, heard as a gap after
    /// the picture is already moving. Keeping the device is lossless when the track has no more channels than the
    /// device was opened with, or when the device is already as wide as the Windows speaker setup (shared mode mixes
    /// everything down to that anyway). Bitstream passthrough and exclusive mode keep mpv's behaviour.
    fn keep_audio_output(&self, aid: &str) -> bool {
        let get = |k: &str| self.backend.get_property(k);
        let num = |k: &str| get(k).and_then(|v| v.parse::<u32>().ok());
        if get("audio-out-params/format").is_none_or(|f| f.starts_with("spdif")) {
            return false;
        }
        let Some(open) = num("audio-out-params/channel-count") else {
            return false;
        };
        let wanted = (0..num("track-list/count").unwrap_or(0)).find_map(|n| {
            let field = |k: &str| format!("track-list/{n}/{k}");
            (get(&field("type")).as_deref() == Some("audio") && get(&field("id")).as_deref() == Some(aid))
                .then(|| num(&field("demux-channel-count")))
                .flatten()
        });
        let Some(wanted) = wanted else { return false };
        if wanted <= open {
            return true;
        }
        get("audio-exclusive").as_deref() != Some("yes")
            && self
                .env
                .audio_mix_channels
                .and_then(|mix| mix(&get("audio-device").unwrap_or_default()))
                .is_some_and(|mix| open >= mix)
    }

    fn restore_gapless(&mut self) {
        if let Some(v) = self.gapless_restore.take() {
            self.set("gapless-audio", &v);
        }
    }

    /// Sync watchdog after an audio switch (see `sync_check`): two out-of-sync samples in a row (about a second)
    /// trigger another exact resync, at most `SYNC_CHECK_RETRIES` times within `SYNC_CHECK_WINDOW`.
    fn check_sync(&mut self, now: Instant) {
        let Some(mut c) = self.sync_check else { return };
        // The first tick after the resume only arms the window: audio needs a moment to start.
        let Some(until) = c.until else {
            c.until = Some(now + SYNC_CHECK_WINDOW);
            self.sync_check = Some(c);
            return;
        };
        if now >= until || self.resume_after_switch {
            self.sync_check = if now >= until { None } else { Some(c) };
            return;
        }
        if self.state.phase != Phase::Playing {
            self.sync_check = Some(c);
            return;
        }
        let num = |k: &str| self.backend.get_property(k).and_then(|v| v.parse::<f64>().ok());
        let audio_on = self
            .backend
            .get_property("aid")
            .is_some_and(|a| a != "no" && a != "false");
        if !audio_on {
            self.sync_check = None;
            return;
        }
        let off = match (num("time-pos"), num("audio-pts"), num("avsync")) {
            (Some(v), Some(a), sync) => (v - a).abs().max(sync.unwrap_or(0.0).abs()) > SYNC_TOLERANCE,
            // Playing with an audio track selected but no audio clock: audio never started.
            _ => true,
        };
        c.bad = if off { c.bad + 1 } else { 0 };
        if c.bad >= 2 {
            if c.retries >= SYNC_CHECK_RETRIES {
                tracing::warn!("audio still out of sync after resyncs; leaving it to the player");
                self.sync_check = None;
                return;
            }
            c.retries += 1;
            c.bad = 0;
            c.until = Some(now + SYNC_CHECK_WINDOW);
            tracing::warn!(attempt = c.retries, "audio out of sync after a track switch; resyncing");
            self.sync_check = Some(c);
            self.resync_audio(None);
            return;
        }
        self.sync_check = Some(c);
    }

    /// The protocol identity of a subtitle menu choice, from the plan's inventory (`None` = off).
    fn subtitle_identity(&self, choice: &SubtitleChoice) -> Result<Option<TrackIdentity>, ControllerError> {
        let inventory = &self.plan.subtitle.inventory;
        let item = match choice {
            SubtitleChoice::Off => return Ok(None),
            SubtitleChoice::Embedded { stream_index } => {
                inventory.iter().find(|t| t.stream_index == Some(*stream_index))
            }
            SubtitleChoice::External { track_id } => inventory.iter().find(|t| &t.track_id == track_id),
        };
        let item = item.ok_or_else(|| ControllerError::Player("no such subtitle".into()))?;
        Ok(Some(TrackIdentity {
            id: item.track_id.clone(),
            index: Some(item.combined_index),
        }))
    }

    /// Ask the server for a plan with other tracks and load it at the current position.
    async fn track_change(&mut self, selected: SelectedTracks) -> Result<(), ControllerError> {
        let position = self.source_position();
        let mut body = self.replan_body("track_change", position);
        body.selected_tracks = selected;
        let decision = self.session.replan_playback(&self.session_id, &body).await?;
        let Some((_, plan)) = decision.playable() else {
            return Err(ControllerError::NotPlayable(decision.outcome));
        };
        tracing::info!(
            position = format!("{position:.1}"),
            "tracks changed on the server; reloading"
        );
        self.plan = plan.clone();
        self.args.subtitle_index = self.plan.selected_tracks.subtitle.as_ref().and_then(|t| t.index);
        self.last_source_position = position.max(0.0);
        self.started_reported = false;
        self.load_plan()
    }

    /// Seek to a source-timeline position.
    pub async fn seek(&mut self, source_seconds: f64) -> Result<(), ControllerError> {
        let t = &self.plan.timeline;
        if t.can_seek_to(source_seconds) {
            let target = t.player_position(source_seconds);
            self.backend
                .seek_absolute(target)
                .map_err(|e| ControllerError::Player(e.to_string()))?;
            self.last_source_position = source_seconds.max(0.0);
            return Ok(());
        }
        // Outside the seek window (transcodes): ask the server to re-anchor and reload there.
        let decision = self
            .session
            .replan_playback(&self.session_id, &self.replan_body("seek_reanchor", source_seconds))
            .await?;
        let Some((_, plan)) = decision.playable() else {
            return Err(ControllerError::NotPlayable(decision.outcome));
        };
        self.plan = plan.clone();
        self.last_source_position = source_seconds.max(0.0);
        self.started_reported = false;
        self.load_plan()
    }

    fn replan_body(&self, operation: &str, position: f64) -> PlaybackReplanBody {
        PlaybackReplanBody {
            installation_id: self.installation_id.clone(),
            protocol_version: PROTOCOL_VERSION,
            operation: operation.into(),
            playback_attempt_id: self.attempt_id.clone(),
            replan_request_id: mint_id(),
            failed_plan_id: self.plan.plan_id.clone(),
            plan_attempt_id: mint_id(),
            plan_attempt_key: self.plan.plan_attempt_key.clone(),
            attempted_plan_keys: vec![self.plan.plan_attempt_key.clone()],
            attempt_count: 1,
            quality_preference: "auto".into(),
            position_seconds: position.max(0.0),
            selected_tracks: self.plan.selected_tracks.clone(),
            client_capabilities: client_capabilities(&self.env.hardware),
            client_playback_context: playback_context(&self.env.app_version, &self.env.os_version, &self.env.output),
            client_features: client_features(&self.caps),
            failure: None,
        }
    }

    /// Ask for a progress sample. It is sent from a spawned task; while one is in flight the next is marked due and
    /// sent as soon as the first finishes (phase-6 review M2).
    fn request_progress(&mut self) {
        if self.stopped || self.server_ended {
            return;
        }
        if self.progress_inflight.is_some() {
            self.progress_due = true;
            return;
        }
        self.progress_due = false;
        self.sequence += 1;
        let body = PlaybackProgressBody {
            installation_id: self.installation_id.clone(),
            sequence: self.sequence,
            position: self.source_position(),
            is_paused: self.state.paused,
        };
        self.last_progress = Some(Instant::now());
        let session = Arc::clone(&self.session);
        let session_id = self.session_id.clone();
        self.progress_inflight = Some(tokio::spawn(async move {
            let r = session.playback_progress(&session_id, &body).await.map(|m| m.outcome);
            (body, r)
        }));
    }

    /// Handle a finished progress sample (and send a due one).
    async fn drain_progress(&mut self) {
        if self.progress_inflight.as_ref().is_some_and(|h| h.is_finished())
            && let Some(h) = self.progress_inflight.take()
        {
            match h.await {
                Ok(r) => self.progress_done(r).await,
                Err(e) => tracing::warn!(error = %e, "progress task failed"),
            }
        }
        if self.progress_due && self.progress_inflight.is_none() && self.started_reported {
            self.request_progress();
        }
    }

    async fn progress_done(&mut self, (body, result): ProgressResult) {
        match result {
            Ok(outcome) => {
                tracing::info!(
                    sequence = body.sequence,
                    position = format!("{:.1}", body.position),
                    paused = body.is_paused,
                    outcome = %outcome,
                    "progress acknowledged"
                );
                self.sink.notice(Notice::Progress {
                    sequence: body.sequence,
                    position: body.position,
                    paused: body.is_paused,
                    outcome,
                });
            }
            Err(e) if matches!(status_of(&e), Some(404 | 410)) => {
                tracing::warn!(code = e.code(), "the server ended the playback session");
                self.server_ended = true;
                self.sink.notice(Notice::Error {
                    code: "playback_session_ended".into(),
                    message: "The server ended this playback session.".into(),
                    fatal: true,
                });
                self.stop().await;
            }
            Err(e) => {
                // 409 progress_conflict must not be retried; transient failures catch up with the next sample.
                tracing::warn!(code = e.code(), sequence = body.sequence, "progress sample not applied");
                if let Some("installation_changed") = e.problem_id() {
                    self.sink.notice(Notice::Error {
                        code: "installation_changed".into(),
                        message: "The server was reconfigured; progress may not be saved for this playback.".into(),
                        fatal: false,
                    });
                }
            }
        }
    }

    /// Tests: wait for every in-flight (and due) progress sample to be handled.
    #[cfg(test)]
    pub async fn settle(&mut self) {
        loop {
            if let Some(h) = self.progress_inflight.take() {
                if let Ok(r) = h.await {
                    self.progress_done(r).await;
                }
            } else if self.progress_due && self.started_reported && !self.stopped {
                self.request_progress();
            } else {
                break;
            }
        }
    }

    fn schedule_recovery(&mut self, error: &str) {
        if self.stopped || self.retry_at.is_some() {
            return;
        }
        let attempt = self.reloads + self.renegotiations;
        if self.reloads >= RELOADS_PER_SESSION && self.renegotiations >= RENEGOTIATIONS {
            self.sink.notice(Notice::Error {
                code: "stream_failed".into(),
                message: format!("Playback failed: {error}"),
                fatal: true,
            });
            self.retry_at = Some(Instant::now());
            self.reloads = u32::MAX;
            return;
        }
        let delay = Duration::from_secs(2u64 << attempt.min(4));
        self.sink.notice(Notice::Error {
            code: "reconnecting".into(),
            message: format!("Connection lost; retrying in {} s.", delay.as_secs()),
            fatal: false,
        });
        self.retry_at = Some(Instant::now() + delay);
    }

    async fn recover(&mut self) {
        if self.reloads == u32::MAX {
            self.stop().await;
            return;
        }
        let resume = self.last_source_position;
        if self.reloads < RELOADS_PER_SESSION {
            self.reloads += 1;
            tracing::info!(
                attempt = self.reloads,
                resume = format!("{resume:.1}"),
                "reloading the stream"
            );
            self.started_reported = false;
            self.state.load_requested();
            let at = self.plan.timeline.player_position(resume);
            if let Err(e) = self.backend.load(&self.stream_url, Some(at)) {
                self.schedule_recovery(&e.to_string());
            }
            return;
        }
        self.renegotiations += 1;
        tracing::info!(resume = format!("{resume:.1}"), "negotiating a new playback attempt");
        self.stop_server_session(STOP_WAIT).await;
        self.relay.clear();
        self.started_reported = false;
        if let Err(e) = self.negotiate(resume).await {
            self.reloads = RELOADS_PER_SESSION;
            self.schedule_recovery(&e.to_string());
        }
    }

    async fn finish_eof(&mut self) {
        if self.stopped {
            return;
        }
        tracing::info!("played to the end");
        self.stop().await;
        self.sink.notice(Notice::Ended);
    }

    /// Stop the attempt: unload mpv at once, then the final sample + stop (once, same stop id on retry). The UI waits
    /// at most [`STOP_WAIT`] for the server; delivery continues in the background after that.
    pub async fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.retry_at = None;
        if let Err(e) = self.backend.stop() {
            tracing::warn!(error = %e, "mpv stop failed");
        }
        let outcome = self.stop_server_session(STOP_WAIT).await;
        self.relay.clear();
        self.sink.notice(Notice::Stopped { outcome });
    }

    /// Send the stop for the current server session, waiting at most `wait`. Returns the outcome, `None` if it was
    /// not delivered (yet). A delivery still running after `wait` keeps going with the same stop id.
    async fn stop_server_session(&mut self, wait: Duration) -> Option<String> {
        if self.server_ended || self.session_id.is_empty() {
            return None;
        }
        let stop_id = self.stop_id.get_or_insert_with(mint_id).clone();
        let (sequence, position) = if self.started_reported || self.state.position > 0.0 {
            self.sequence += 1;
            (Some(self.sequence), Some(self.last_source_position.max(0.0)))
        } else {
            (None, None)
        };
        let body = PlaybackStopBody {
            installation_id: self.installation_id.clone(),
            stop_id,
            sequence,
            position,
            is_paused: sequence.map(|_| self.state.paused),
        };
        let mut task = tokio::spawn(deliver_stop(Arc::clone(&self.session), self.session_id.clone(), body));
        // This attempt's server session is over from the client's side either way.
        self.server_ended = true;
        match tokio::time::timeout(wait, &mut task).await {
            Ok(Ok(StopDelivery::Stopped(outcome))) => Some(outcome),
            Ok(Ok(StopDelivery::Gone | StopDelivery::Failed)) => None,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "stop task failed");
                None
            }
            Err(_) => {
                tracing::warn!("server stop still pending; delivering in the background");
                None
            }
        }
    }
}

/// Deliver one stop body: up to three attempts on transient failures, same stop id (a replay answers `replayed`).
async fn deliver_stop(session: Arc<Session>, session_id: String, body: PlaybackStopBody) -> StopDelivery {
    for attempt in 1..=3u64 {
        match session.stop_playback(&session_id, &body).await {
            Ok(m) => {
                tracing::info!(
                    outcome = %m.outcome,
                    position = ?body.position.map(|p| format!("{p:.1}")),
                    "playback stopped on the server"
                );
                return StopDelivery::Stopped(m.outcome);
            }
            Err(e) if matches!(status_of(&e), Some(404 | 410)) => return StopDelivery::Gone,
            Err(e) if is_transient(&e) && attempt < 3 => {
                tracing::warn!(code = e.code(), attempt, "stop failed; retrying with the same stop id");
                tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
            }
            Err(e) => {
                tracing::warn!(code = e.code(), "stop could not be delivered");
                return StopDelivery::Failed;
            }
        }
    }
    StopDelivery::Failed
}

/// `/path?exp&sig`: the path and the query parameter names of a URL, never the values (signatures, tokens).
fn url_shape(raw: &str) -> String {
    let without_origin = raw.find("://").map_or(raw, |i| {
        let rest = &raw[i + 3..];
        rest.find('/').map_or("/", |p| &rest[p..])
    });
    let (path, query) = without_origin.split_once('?').unwrap_or((without_origin, ""));
    let names: Vec<&str> = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| kv.split_once('=').map_or(kv, |(k, _)| k))
        .collect();
    if names.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", names.join("&"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;
    use granary_player::{MockBackend, PropValue};
    use granary_silo_api::Secret;
    use granary_silo_api::credentials::MemoryStore;
    use granary_silo_api::mock::MockSiloApi;
    use std::sync::Mutex;

    #[test]
    fn url_shape_keeps_path_and_query_names_only() {
        assert_eq!(
            url_shape("https://h.example/api/v3/playback/media/f1?exp=1&sig=secret"),
            "/api/v3/playback/media/f1?exp&sig"
        );
        assert_eq!(url_shape("/media/x"), "/media/x");
        assert_eq!(url_shape("http://h"), "/");
    }

    #[derive(Default)]
    struct Notices(Mutex<Vec<Notice>>);

    impl Sink for Notices {
        fn notice(&self, n: Notice) {
            self.0.lock().unwrap().push(n);
        }
        fn before_load(&self, source: &PlaybackSource) -> Vec<(String, String)> {
            vec![("target-peak".into(), format!("src-{}", source.media_file_id.len()))]
        }
    }

    impl Notices {
        fn all(&self) -> Vec<Notice> {
            self.0.lock().unwrap().clone()
        }
    }

    struct Rig {
        api: Arc<MockSiloApi>,
        backend: Arc<MockBackend>,
        notices: Arc<Notices>,
        relay: Arc<Relay>,
        controller: Controller,
    }

    async fn rig(args: StartArgs) -> Rig {
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
        let backend = Arc::new(MockBackend::default());
        let notices = Arc::new(Notices::default());
        let relay = Arc::new(Relay::start().await.unwrap());
        let controller = Controller::begin(
            Arc::new(session),
            backend.clone(),
            relay.clone(),
            notices.clone(),
            Environment {
                app_version: "0.1.0".into(),
                os_version: "10.0.26200".into(),
                ..Environment::default()
            },
            args,
        )
        .await
        .unwrap();
        Rig {
            api,
            backend,
            notices,
            relay,
            controller,
        }
    }

    fn args() -> StartArgs {
        StartArgs {
            item_id: "m1".into(),
            file_id: "m1-f1".into(),
            profile_id: "1".into(),
            start_seconds: 30.0,
            audio_index: Some(1),
            subtitle_index: None,
        }
    }

    fn prop(name: &str, value: PropValue) -> MpvEvent {
        MpvEvent::Property {
            name: name.into(),
            value,
        }
    }

    async fn play_to(c: &mut Controller, pos: f64) {
        c.on_event(&prop("time-pos", PropValue::Double(pos))).await;
    }

    #[tokio::test]
    async fn negotiates_direct_play_through_the_relay_and_reports_progress() {
        let mut r = rig(args()).await;
        let calls = r.backend.calls();
        let load = calls.iter().find(|c| c.starts_with("cmd loadfile")).unwrap();
        // A reused core may still be paused from keep-open at the previous EOF: unpause before loading.
        let unpause = calls.iter().position(|c| c == "set pause=no");
        let load_at = calls.iter().position(|c| c.starts_with("cmd loadfile"));
        assert!(unpause.is_some() && unpause < load_at, "{calls:?}");
        // The output policy (HDR, refresh, tone mapping, audio) is in place before the first frame.
        let output = calls.iter().position(|c| c.starts_with("set target-peak=src-"));
        assert!(output.is_some() && output < load_at, "{calls:?}");
        assert!(
            load.contains(&format!("http://127.0.0.1:{}/r/", r.relay.port())),
            "{load}"
        );
        assert!(load.ends_with("start=30.000"), "{load}");
        assert!(!load.contains("api/v2"), "server path never reaches mpv: {load}");
        let Notice::Plan(plan) = &r.notices.all()[0] else {
            panic!("no plan notice")
        };
        assert!(plan.direct_play && plan.relayed && plan.delivery == "original_http");
        // The start body asked for the requested file, pinned, at the resume position.
        let log = r.api.playback_log();
        assert_eq!(log.starts[0].file_id, "m1-f1");
        assert_eq!(log.starts[0].start_position, Some(30.0));
        assert_eq!(log.starts[0].allow_alternate_versions, Some(false));

        let c = &mut r.controller;
        c.on_event(&MpvEvent::StartFile).await;
        c.on_event(&MpvEvent::FileLoaded).await;
        assert!(
            r.backend.calls().contains(&"set aid=2".to_owned()),
            "audio ordinal 1 → aid 2"
        );
        assert!(r.backend.calls().contains(&"set sid=no".to_owned()));
        play_to(c, 30.0).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        c.settle().await;
        play_to(c, 35.5).await;
        c.on_event(&prop("pause", PropValue::Flag(true))).await;
        c.settle().await;
        c.on_event(&prop("pause", PropValue::Flag(false))).await;
        c.settle().await;
        // A seek reports once it has landed.
        c.seek(100.0).await.unwrap();
        c.on_event(&MpvEvent::Seek).await;
        play_to(c, 100.0).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        c.settle().await;
        // Periodic sample only after the interval.
        c.tick(Instant::now()).await;
        c.settle().await;
        c.tick(Instant::now() + PROGRESS_INTERVAL).await;
        c.settle().await;

        let progress = r.api.playback_log().progress;
        let seqs: Vec<i64> = progress.iter().map(|p| p.1).collect();
        assert_eq!(seqs, [1, 2, 3, 4, 5]);
        assert!(progress.iter().all(|p| p.4 == "applied"));
        assert!(progress[1].3, "pause sample");
        assert!((progress[3].2 - 100.0).abs() < 1e-9, "post-seek position");

        c.stop().await;
        c.stop().await;
        let stops = r.api.playback_log().stops;
        assert_eq!(stops.len(), 1, "stop is sent once");
        assert_eq!(stops[0].1.sequence, Some(6));
        assert_eq!(stops[0].1.position, Some(100.0));
        assert_eq!(r.relay.route_count(), 0);
        assert!(r.backend.calls().last().unwrap() == "cmd stop");
        assert!(matches!(r.notices.all().last(), Some(Notice::Stopped { outcome: Some(o) }) if o == "stopped"));
    }

    #[tokio::test]
    async fn eof_stops_with_the_final_position_and_reports_ended() {
        let mut r = rig(StartArgs {
            start_seconds: 0.0,
            ..args()
        })
        .await;
        let c = &mut r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        play_to(c, 9.96).await;
        c.on_event(&prop("eof-reached", PropValue::Flag(true))).await;
        assert!(c.is_finished());
        let stops = r.api.playback_log().stops;
        assert_eq!(stops.len(), 1);
        assert!((stops[0].1.position.unwrap() - 9.96).abs() < 1e-9);
        let n = r.notices.all();
        assert!(matches!(n[n.len() - 2], Notice::Stopped { .. }) && n[n.len() - 1] == Notice::Ended);
    }

    #[tokio::test]
    async fn server_ended_session_stops_without_a_stop_call() {
        let mut r = rig(args()).await;
        let c = &mut r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        c.settle().await;
        r.api.end_session(c.session_id());
        c.tick(Instant::now() + PROGRESS_INTERVAL).await;
        c.settle().await;
        assert!(c.is_finished());
        assert!(r.api.playback_log().stops.is_empty());
        assert!(
            r.notices
                .all()
                .iter()
                .any(|n| matches!(n, Notice::Error { code, fatal: true, .. } if code == "playback_session_ended"))
        );
    }

    #[tokio::test]
    async fn dropped_connection_reloads_then_renegotiates_at_the_last_position() {
        let mut r = rig(args()).await;
        let c = &mut r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        play_to(c, 42.0).await;
        let first_session = c.session_id().to_owned();
        let fail = MpvEvent::EndFile {
            reason: EndReason::Error("network".into()),
        };
        let far = || Instant::now() + Duration::from_secs(120);
        for _ in 0..RELOADS_PER_SESSION {
            c.on_event(&fail).await;
            c.tick(far()).await;
        }
        let loads: Vec<String> = r
            .backend
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("cmd loadfile"))
            .collect();
        assert_eq!(loads.len(), 3);
        assert!(loads[1].ends_with("start=42.000") && loads[2].ends_with("start=42.000"));
        // Third failure: the old session is stopped and a new attempt starts at 42 s.
        c.on_event(&fail).await;
        c.tick(far()).await;
        assert_ne!(c.session_id(), first_session);
        let log = r.api.playback_log();
        assert_eq!(log.starts.len(), 2);
        assert_eq!(log.starts[1].start_position, Some(42.0));
        assert_eq!(log.stops[0].0, first_session);
        // A further failure after everything is used up is fatal.
        c.on_event(&MpvEvent::FileLoaded).await;
        for _ in 0..=RELOADS_PER_SESSION {
            c.on_event(&fail).await;
            c.tick(far()).await;
        }
        assert!(c.is_finished());
        assert!(
            r.notices
                .all()
                .iter()
                .any(|n| matches!(n, Notice::Error { code, fatal: true, .. } if code == "stream_failed"))
        );
    }

    #[tokio::test]
    async fn a_load_that_fails_before_playback_reloads_at_the_requested_start() {
        // Phase-6 review M1: the resume point must not fall back to 0 when the first load fails.
        let mut r = rig(StartArgs {
            start_seconds: 600.0,
            ..args()
        })
        .await;
        let c = &mut r.controller;
        c.on_event(&MpvEvent::StartFile).await;
        c.on_event(&MpvEvent::FileLoaded).await;
        // mpv reports position 0 before the start seek lands, then the connection drops.
        c.on_event(&prop("time-pos", PropValue::Double(0.0))).await;
        c.on_event(&MpvEvent::EndFile {
            reason: EndReason::Error("network".into()),
        })
        .await;
        c.tick(Instant::now() + Duration::from_secs(120)).await;
        let loads: Vec<String> = r
            .backend
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("cmd loadfile"))
            .collect();
        assert_eq!(loads.len(), 2, "{loads:?}");
        assert!(loads[1].ends_with("start=600.000"), "{}", loads[1]);
    }

    #[tokio::test]
    async fn stop_unloads_mpv_before_waiting_for_the_server() {
        // Phase-6 review M2: the picture goes away at once; the server stop follows.
        let mut r = rig(args()).await;
        let c = &mut r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        play_to(c, 31.0).await;
        c.settle().await;
        c.stop().await;
        let calls = r.backend.calls();
        assert!(calls.iter().any(|c| c == "cmd stop"), "{calls:?}");
        assert_eq!(r.api.playback_log().stops.len(), 1);
    }

    #[tokio::test]
    async fn embedded_subtitle_is_selected_by_stream_index() {
        // Protocol ordinal 1 = the second embedded track (no externals), stream index 3 in the mock's version.
        let r = rig(StartArgs {
            subtitle_index: Some(1),
            ..args()
        })
        .await;
        for (k, v) in [
            ("track-list/count", "3"),
            ("track-list/0/type", "video"),
            ("track-list/1/type", "sub"),
            ("track-list/1/ff-index", "2"),
            ("track-list/1/id", "1"),
            ("track-list/2/type", "sub"),
            ("track-list/2/ff-index", "3"),
            ("track-list/2/id", "2"),
        ] {
            r.backend.properties.lock().unwrap().push((k.into(), v.into()));
        }
        let mut c = r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        assert!(
            r.backend.calls().contains(&"set sid=2".to_owned()),
            "{:?}",
            r.backend.calls()
        );
        c.select_subtitle(&SubtitleChoice::Embedded { stream_index: 2 })
            .await
            .unwrap();
        assert!(r.backend.calls().contains(&"set sid=1".to_owned()));
        c.select_subtitle(&SubtitleChoice::Off).await.unwrap();
        assert!(
            c.select_subtitle(&SubtitleChoice::Embedded { stream_index: 9 })
                .await
                .is_err()
        );
        c.select_audio(0).await.unwrap();
        assert_eq!(r.backend.calls().last().unwrap(), "set aid=1");
        assert!(r.api.playback_log().replans.is_empty(), "direct play switches locally");
    }

    #[tokio::test]
    async fn unmatched_embedded_subtitle_turns_subtitles_off() {
        let r = rig(StartArgs {
            subtitle_index: Some(1),
            ..args()
        })
        .await;
        r.backend
            .properties
            .lock()
            .unwrap()
            .push(("track-list/count".into(), "0".into()));
        let mut c = r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        assert_eq!(r.backend.calls().last().unwrap(), "set sid=no");
    }

    #[tokio::test]
    async fn audio_switch_resyncs_again_when_audio_does_not_start() {
        let r = rig(args()).await;
        let mut c = r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        r.backend.set_property("time-pos", "50.000").unwrap();
        r.backend.set_property("pause", "no").unwrap();
        c.on_event(&prop("pause", PropValue::Flag(false))).await;
        play_to(&mut c, 50.0).await;
        let seeks = |r: &MockBackend| r.calls().iter().filter(|c| c.starts_with("cmd seek")).count();

        c.select_audio(0).await.unwrap();
        let calls = r.backend.calls();
        assert_eq!(
            &calls[calls.len() - 3..],
            ["set pause=yes", "set aid=1", "cmd seek 50.000 absolute"]
        );
        c.on_event(&MpvEvent::Seek).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        assert_eq!(
            r.backend.calls().last().unwrap(),
            "set pause=no",
            "resumes when the seek lands"
        );

        // Audio never starts (no audio clock): the watchdog arms, then resyncs after two bad samples.
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        c.tick(t0).await;
        c.tick(t0 + ms(500)).await;
        assert_eq!(seeks(&r.backend), 1, "one bad sample is not enough");
        c.tick(t0 + ms(1000)).await;
        assert_eq!(seeks(&r.backend), 2, "resynced");
        c.on_event(&MpvEvent::Seek).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;

        // Now in sync: no further resyncs, and the window ends.
        r.backend.set_property("audio-pts", "50.020").unwrap();
        r.backend.set_property("avsync", "0.010").unwrap();
        for i in 0..30 {
            c.tick(t0 + ms(1500 + 500 * i)).await;
        }
        assert_eq!(seeks(&r.backend), 2);
        assert!(c.sync_check.is_none(), "watchdog window closed");
    }

    #[tokio::test]
    async fn embedded_subtitle_switch_seeks_in_place_while_playing() {
        let r = rig(args()).await;
        for (k, v) in [
            ("track-list/count", "2"),
            ("track-list/0/type", "sub"),
            ("track-list/0/ff-index", "2"),
            ("track-list/0/id", "1"),
            ("track-list/1/type", "sub"),
            ("track-list/1/ff-index", "3"),
            ("track-list/1/id", "2"),
        ] {
            r.backend.set_property(k, v).unwrap();
        }
        let mut c = r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        r.backend.set_property("time-pos", "42.500").unwrap();
        play_to(&mut c, 42.5).await;
        c.select_subtitle(&SubtitleChoice::Embedded { stream_index: 3 })
            .await
            .unwrap();
        let calls = r.backend.calls();
        assert_eq!(&calls[calls.len() - 2..], ["set sid=2", "cmd seek 42.500 absolute"]);
        // The same track again: nothing to refill, no seek.
        let n = r.backend.calls().len();
        c.select_subtitle(&SubtitleChoice::Embedded { stream_index: 3 })
            .await
            .unwrap();
        assert_eq!(r.backend.calls().len(), n);
        // Off is instant in mpv: no seek either.
        c.select_subtitle(&SubtitleChoice::Off).await.unwrap();
        assert_eq!(r.backend.calls().last().unwrap(), "set sid=no");
    }

    #[tokio::test]
    async fn audio_switch_keeps_the_open_device_when_it_loses_nothing() {
        let r = rig(StartArgs {
            audio_index: Some(0),
            ..args()
        })
        .await;
        for (k, v) in [
            ("track-list/count", "3"),
            ("track-list/0/type", "audio"),
            ("track-list/0/id", "1"),
            ("track-list/0/demux-channel-count", "6"),
            ("track-list/1/type", "audio"),
            ("track-list/1/id", "2"),
            ("track-list/1/demux-channel-count", "2"),
            ("track-list/2/type", "audio"),
            ("track-list/2/id", "3"),
            ("track-list/2/demux-channel-count", "8"),
            ("audio-out-params/format", "floatp"),
            ("audio-out-params/channel-count", "6"),
            ("gapless-audio", "weak"),
            ("time-pos", "10.000"),
            ("pause", "no"),
        ] {
            r.backend.set_property(k, v).unwrap();
        }
        let mut c = r.controller;
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        play_to(&mut c, 10.0).await;

        // 5.1 device, stereo track: kept for the switch, mpv's setting back once the seek lands.
        c.select_audio(1).await.unwrap();
        let calls = r.backend.calls();
        assert_eq!(
            &calls[calls.len() - 4..],
            [
                "set gapless-audio=yes",
                "set pause=yes",
                "set aid=2",
                "cmd seek 10.000 absolute"
            ]
        );
        c.on_event(&MpvEvent::PlaybackRestart).await;
        assert!(r.backend.calls().contains(&"set gapless-audio=weak".to_owned()));
        r.backend.set_property("gapless-audio", "weak").unwrap();

        // 7.1 track on a 5.1-opened device: reopening gains channels unless Windows mixes to 5.1 anyway.
        let gapless = |r: &MockBackend| r.calls().iter().filter(|c| *c == "set gapless-audio=yes").count();
        c.select_audio(2).await.unwrap();
        assert_eq!(gapless(&r.backend), 1, "no mix format known: let mpv reopen");
        c.on_event(&MpvEvent::PlaybackRestart).await;
        c.env.audio_mix_channels = Some(|_| Some(6));
        c.select_audio(0).await.unwrap();
        c.on_event(&MpvEvent::PlaybackRestart).await;
        c.select_audio(2).await.unwrap();
        assert_eq!(gapless(&r.backend), 3, "speaker setup is 5.1: nothing to gain");

        // Passthrough must match the bitstream: mpv decides.
        c.on_event(&MpvEvent::PlaybackRestart).await;
        r.backend.set_property("audio-out-params/format", "spdif-ac3").unwrap();
        c.select_audio(1).await.unwrap();
        assert_eq!(gapless(&r.backend), 3);
    }

    #[tokio::test]
    async fn track_switches_on_a_transcode_replan_with_track_change() {
        // Phase-6 review M3: a remux/transcode carries only the negotiated tracks.
        let mut r = rig(args()).await;
        let c = &mut r.controller;
        c.plan.delivery = "server_transcode_hls".into();
        c.plan.subtitle.inventory = vec![SubtitleInventoryItem {
            track_id: "emb-3".into(),
            combined_index: 4,
            stream_index: Some(3),
            ..SubtitleInventoryItem::default()
        }];
        c.on_event(&MpvEvent::FileLoaded).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        play_to(c, 50.0).await;
        let loads = |r: &Rig| {
            r.backend
                .calls()
                .iter()
                .filter(|c| c.starts_with("cmd loadfile"))
                .count()
        };
        let before = loads(&r);

        let c = &mut r.controller;
        c.select_audio(1).await.unwrap();
        let log = r.api.playback_log();
        assert_eq!(log.replans.len(), 1);
        let body = &log.replans[0];
        assert_eq!(body.operation, "track_change");
        assert!(body.failure.is_none());
        assert_eq!(body.selected_tracks.audio.as_ref().and_then(|t| t.index), Some(1));
        assert!((body.position_seconds - 50.0).abs() < 1e-9);
        assert_eq!(loads(&r), before + 1, "the new plan is loaded");
        assert!(
            !r.backend.calls().contains(&"set aid=2".to_owned()),
            "no local aid switch"
        );

        // The new plan from the mock is direct; force another transcode and pick the embedded subtitle.
        let c = &mut r.controller;
        c.plan.delivery = "server_transcode_hls".into();
        c.plan.subtitle.inventory = vec![SubtitleInventoryItem {
            track_id: "emb-3".into(),
            combined_index: 4,
            stream_index: Some(3),
            ..SubtitleInventoryItem::default()
        }];
        c.select_subtitle(&SubtitleChoice::Embedded { stream_index: 3 })
            .await
            .unwrap();
        let log = r.api.playback_log();
        let sub = log.replans[1].selected_tracks.subtitle.clone().unwrap();
        assert_eq!((sub.id.as_str(), sub.index), ("emb-3", Some(4)));
        assert_eq!(
            log.replans[1].selected_tracks.audio.as_ref().and_then(|t| t.index),
            Some(1),
            "audio choice kept"
        );
        // Unknown tracks fail without a request.
        let c = &mut r.controller;
        c.plan.delivery = "server_transcode_hls".into();
        assert!(
            c.select_subtitle(&SubtitleChoice::Embedded { stream_index: 9 })
                .await
                .is_err()
        );
        assert_eq!(r.api.playback_log().replans.len(), 2);
    }

    #[tokio::test]
    async fn sidecar_subtitles_are_downloaded_ahead_and_added_from_memory() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&served);
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let body = "1\r\n00:00:01,000 --> 00:00:02,000\r\nHi\r\n";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(format!("{head}{body}").as_bytes()).await;
            }
        });
        let mut r = rig(args()).await;
        let c = &mut r.controller;
        c.plan.subtitle.inventory = vec![SubtitleInventoryItem {
            track_id: "ext-1".into(),
            url: Some(format!("http://127.0.0.1:{port}/sub.srt")),
            ..SubtitleInventoryItem::default()
        }];
        c.prefetch_sidecars();
        r.backend.set_property("sid", "3").unwrap();
        let c = &mut r.controller;
        c.select_subtitle(&SubtitleChoice::External {
            track_id: "ext-1".into(),
        })
        .await
        .unwrap();
        let calls = r.backend.calls();
        let add = calls.iter().find(|c| c.starts_with("cmd sub-add")).unwrap();
        assert!(add.starts_with("cmd sub-add hex://310d0a"), "{add}");
        assert!(add.ends_with(" select"), "{add}");
        // Picked again: the loaded track is selected, nothing is downloaded or added again.
        let c = &mut r.controller;
        c.select_subtitle(&SubtitleChoice::External {
            track_id: "ext-1".into(),
        })
        .await
        .unwrap();
        let adds = r
            .backend
            .calls()
            .iter()
            .filter(|c| c.starts_with("cmd sub-add"))
            .count();
        assert_eq!(adds, 1);
        assert!(r.backend.calls().ends_with(&["set sid=3".to_owned()]));
        assert_eq!(
            served.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one download, shared"
        );
    }
}
