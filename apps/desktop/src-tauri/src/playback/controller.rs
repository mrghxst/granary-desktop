//! One playback attempt (ADR-0008): negotiate with the server, load the plan into mpv, report sequenced progress,
//! stop exactly once, and recover from dropped connections. Driven by one task (see `playback::runner`), so the
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
            stop_id: None,
            stopped: false,
            server_ended: false,
            reloads: 0,
            renegotiations: 0,
            retry_at: None,
            started_reported: false,
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
        self.stopped = false;
        self.server_ended = false;
        self.reloads = 0;
        self.apply_output();
        self.load_plan()
    }

    /// Display and audio output for this source, decided before the first frame (HDR switched on, refresh set,
    /// tone-mapping target and audio options in place before `loadfile`).
    fn apply_output(&mut self) {
        let sink = Arc::clone(&self.sink);
        let source = self.plan.source.clone();
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
        let before = self.state.clone();
        self.state.apply(ev);
        if self.state.position > 0.0 || matches!(self.state.phase, Phase::Playing | Phase::Paused) {
            self.last_source_position = self.source_position();
        }
        match ev {
            MpvEvent::FileLoaded => {
                self.apply_tracks();
                self.reloads_succeeded();
            }
            MpvEvent::PlaybackRestart => {
                // First frame of the load, or a seek finished.
                if !self.started_reported || before.phase == Phase::Seeking {
                    self.started_reported = true;
                    self.send_progress().await;
                }
            }
            MpvEvent::Property { name, .. } if name == "pause" => {
                if before.paused != self.state.paused && self.started_reported {
                    self.send_progress().await;
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
        if let Some(at) = self.retry_at
            && now >= at
        {
            self.retry_at = None;
            self.recover().await;
            return;
        }
        let interval = match self.state.phase {
            Phase::Playing | Phase::Buffering => PROGRESS_INTERVAL,
            Phase::Paused => PAUSED_HEARTBEAT,
            _ => return,
        };
        if self.started_reported && self.last_progress.is_none_or(|t| now.duration_since(t) >= interval) {
            self.send_progress().await;
        }
    }

    /// Select the plan's audio and subtitle tracks once mpv has the track list.
    fn apply_tracks(&mut self) {
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
                None => tracing::warn!(
                    stream_index = e.stream_index,
                    "embedded subtitle not in the mpv track list"
                ),
            },
            (_, _, Some(a)) if sub.mode != "off" && sub.mode != "burn_in" => {
                if let Err(e) = self.add_sidecar(&a.url.clone()) {
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

    fn add_sidecar(&self, raw: &str) -> Result<(), ControllerError> {
        let (url, _) = self.media_url(raw)?;
        self.backend
            .command(&["sub-add", &url, "select"])
            .map_err(|e| ControllerError::Player(e.to_string()))
    }

    /// UI subtitle menu.
    pub fn select_subtitle(&mut self, choice: &SubtitleChoice) -> Result<(), ControllerError> {
        match choice {
            SubtitleChoice::Off => self.set("sid", "no"),
            SubtitleChoice::Embedded { stream_index } => {
                let sid = self
                    .mpv_sub_id(*stream_index)
                    .ok_or_else(|| ControllerError::Player("subtitle track not in this stream".into()))?;
                self.set("sid", &sid);
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
                self.add_sidecar(&url)?;
            }
        }
        Ok(())
    }

    /// UI audio menu: mpv switches locally (direct play carries every track of the original file).
    pub fn select_audio(&mut self, index: u32) {
        self.args.audio_index = Some(index);
        self.set("aid", &(index + 1).to_string());
    }

    /// Seek to a source-timeline position.
    pub async fn seek(&mut self, source_seconds: f64) -> Result<(), ControllerError> {
        let t = &self.plan.timeline;
        if t.can_seek_to(source_seconds) {
            let target = t.player_position(source_seconds);
            return self
                .backend
                .seek_absolute(target)
                .map_err(|e| ControllerError::Player(e.to_string()));
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

    async fn send_progress(&mut self) {
        if self.stopped || self.server_ended {
            return;
        }
        self.sequence += 1;
        let body = PlaybackProgressBody {
            installation_id: self.installation_id.clone(),
            sequence: self.sequence,
            position: self.source_position(),
            is_paused: self.state.paused,
        };
        self.last_progress = Some(Instant::now());
        match self.session.playback_progress(&self.session_id, &body).await {
            Ok(m) => {
                tracing::info!(
                    sequence = body.sequence,
                    position = format!("{:.1}", body.position),
                    paused = body.is_paused,
                    outcome = %m.outcome,
                    "progress acknowledged"
                );
                self.sink.notice(Notice::Progress {
                    sequence: body.sequence,
                    position: body.position,
                    paused: body.is_paused,
                    outcome: m.outcome,
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
                        message: "The server was reconfigured; progress will be saved at the end.".into(),
                        fatal: false,
                    });
                }
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
        self.stop_server_session().await;
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

    /// Stop the attempt: final sample + stop (once, same stop id on retry), then unload.
    pub async fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.retry_at = None;
        let outcome = self.stop_server_session().await;
        self.relay.clear();
        if let Err(e) = self.backend.stop() {
            tracing::warn!(error = %e, "mpv stop failed");
        }
        self.sink.notice(Notice::Stopped { outcome });
    }

    /// Send the stop for the current server session. Returns the outcome, `None` if it could not be delivered.
    async fn stop_server_session(&mut self) -> Option<String> {
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
        for attempt in 1..=3 {
            match self.session.stop_playback(&self.session_id, &body).await {
                Ok(m) => {
                    tracing::info!(
                        outcome = %m.outcome,
                        position = ?body.position.map(|p| format!("{p:.1}")),
                        "playback stopped on the server"
                    );
                    self.server_ended = true;
                    return Some(m.outcome);
                }
                Err(e) if matches!(status_of(&e), Some(404 | 410)) => {
                    self.server_ended = true;
                    return None;
                }
                Err(e) if is_transient(&e) && attempt < 3 => {
                    tracing::warn!(code = e.code(), attempt, "stop failed; retrying with the same stop id");
                    tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
                }
                Err(e) => {
                    tracing::warn!(code = e.code(), "stop could not be delivered");
                    return None;
                }
            }
        }
        None
    }
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
        play_to(c, 35.5).await;
        c.on_event(&prop("pause", PropValue::Flag(true))).await;
        c.on_event(&prop("pause", PropValue::Flag(false))).await;
        // A seek reports once it has landed.
        c.seek(100.0).await.unwrap();
        c.on_event(&MpvEvent::Seek).await;
        play_to(c, 100.0).await;
        c.on_event(&MpvEvent::PlaybackRestart).await;
        // Periodic sample only after the interval.
        c.tick(Instant::now()).await;
        c.tick(Instant::now() + PROGRESS_INTERVAL).await;

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
        r.api.end_session(c.session_id());
        c.tick(Instant::now() + PROGRESS_INTERVAL).await;
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
    async fn embedded_subtitle_is_selected_by_stream_index() {
        let r = rig(StartArgs {
            subtitle_index: Some(3),
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
            .unwrap();
        assert!(r.backend.calls().contains(&"set sid=1".to_owned()));
        c.select_subtitle(&SubtitleChoice::Off).unwrap();
        assert!(
            c.select_subtitle(&SubtitleChoice::Embedded { stream_index: 9 })
                .is_err()
        );
        c.select_audio(0);
        assert_eq!(r.backend.calls().last().unwrap(), "set aid=1");
    }
}
