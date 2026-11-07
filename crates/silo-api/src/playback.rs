//! Playback protocol v3 over `/api/v2/playback` (Phase 6, docs/research/silo-api.md §6–§10).
//!
//! Request bodies mirror the OAS schemas exactly (they are `additionalProperties: false` upstream, so nothing extra may
//! be sent; optional fields are omitted when `None`). Response models are lenient subsets: unknown fields are ignored
//! and optional fields default. Schema names and lines refer to `silo-server/contracts/api/v2/openapi.json@6791aac`.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The only playback protocol the server speaks (`playback-protocol-v3.md@6791aac` §2).
pub const PROTOCOL_VERSION: i64 = 3;

/// Delivery classes keyed in `client_playback_context.deliveries` (`protocol_v3.go@6791aac` L345-347).
pub const DELIVERY_ORIGINAL_HTTP: &str = "original_http";
pub const DELIVERY_PROGRESSIVE: &str = "progressive";
pub const DELIVERY_HLS: &str = "hls";

/// `PlaybackCapabilities` (OAS:L43970), `GET /api/v2/playback/capabilities`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackCapabilities {
    /// `available | disabled | not_configured | unsupported`.
    pub state: String,
    pub allowed: bool,
    pub revision: String,
    /// Must be echoed on every start, progress, stop and replan (`playback-api.md@6791aac` "Capabilities").
    pub installation_id: Option<String>,
    pub protocol_versions: Vec<i64>,
    pub features: Vec<String>,
    pub deliveries: Vec<String>,
}

impl PlaybackCapabilities {
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f.eq_ignore_ascii_case(feature))
    }

    /// Playback can be negotiated: allowed, available and speaking protocol 3, with an installation id to echo.
    pub fn usable(&self) -> bool {
        self.allowed
            && self.state == "available"
            && self.protocol_versions.contains(&PROTOCOL_VERSION)
            && self.installation_id.as_deref().is_some_and(|i| !i.is_empty())
    }
}

/// `HDRCapabilitiesV3` (OAS:L35528).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HdrCapabilities {
    pub hdr10: bool,
    pub hdr10_plus: bool,
    pub hlg: bool,
    pub dolby_vision_profiles: Vec<i64>,
}

/// `VideoDecodeCapabilityV3`. An omitted bound means "unconstrained" (`playback-protocol-v3.md@6791aac` §3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoDecodeCapability {
    pub codec: String,
    pub hardware: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bit_depths: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder_name: Option<String>,
}

/// `AudioPassthroughV3`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioPassthrough {
    pub passthrough_codecs: Vec<String>,
    pub max_channels: i64,
    pub spatializer_enabled: bool,
}

/// `ClientCodecCapabilitiesV3` (OAS:L28930).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientCodecCapabilities {
    /// `exact | platform_attested | declared`.
    pub video_evidence: String,
    pub audio_evidence: String,
    pub codecs_video: Vec<String>,
    pub codecs_video_hardware: Vec<String>,
    pub codecs_audio: Vec<String>,
    pub containers: Vec<String>,
    pub hdr: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_details: Option<HdrCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_decode: Vec<VideoDecodeCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_passthrough: Option<AudioPassthrough>,
}

/// `DeviceContextV3`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceContext {
    pub platform: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub platform_details: BTreeMap<String, String>,
}

/// `OutputDisplayV3`: the raw display probe and how it was obtained (`exact | unknown`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputDisplay {
    pub hdr_evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_types: Option<HdrCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_id: Option<String>,
}

/// `OutputContextV3`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_context_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sink_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_sink: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<OutputDisplay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_details: Option<HdrCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_passthrough: Option<AudioPassthrough>,
}

/// `NativeEmbeddedSubtitleCapabilityV3`: subtitle codecs the player selects in-process from one container.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NativeEmbeddedSubtitleCapability {
    pub container: String,
    pub codecs: Vec<String>,
    /// `ffmpeg_stream_index | container_track_id` (`protocol_v3.go@6791aac` L679-680).
    pub track_identity: String,
    pub ass_styling: bool,
    pub font_attachments: bool,
}

/// `DeliverySubtitleCapabilitiesV3` (OAS:L31550).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeliverySubtitleCapabilities {
    pub embedded_text: bool,
    pub embedded_bitmap: bool,
    pub sidecar_text: bool,
    pub sidecar_bitmap: bool,
    pub ass_styling: bool,
    pub font_attachments: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_embedded: Vec<NativeEmbeddedSubtitleCapability>,
}

/// `TransformationV3`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Transformation {
    pub name: String,
    pub executor: String,
    pub recipe_version: String,
    pub validated_claims: Vec<String>,
}

/// `DeliveryCapabilityV3` (OAS:L31459): one delivery class. Both `enabled` and `supported_on_device` must be true
/// for the class to be eligible (`playback-protocol-v3.md@6791aac` §4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeliveryCapability {
    pub enabled: bool,
    pub supported_on_device: bool,
    pub containers: Vec<String>,
    pub video_codecs: Vec<String>,
    pub audio_decode_codecs: Vec<String>,
    pub audio_passthrough_codecs: Vec<String>,
    pub subtitles: DeliverySubtitleCapabilities,
    pub features: Vec<String>,
    pub transformations: Vec<Transformation>,
    pub validated_claims: Vec<String>,
    pub auth_header_refresh: bool,
}

/// `ClientPlaybackContextV3` (OAS:L28993).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientPlaybackContext {
    pub protocol_version: i64,
    pub form_factor: String,
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_channel: Option<String>,
    pub device: DeviceContext,
    pub output: OutputContext,
    pub deliveries: BTreeMap<String, DeliveryCapability>,
}

/// `TrackIdentityV3`: stable track id plus the ordinal index (`index` is the audio/subtitle ordinal of the file,
/// `plan_v3.go@6791aac` L2409-2421).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackIdentity {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
}

/// `SelectedTracksV3`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelectedTracks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<TrackIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<TrackIdentity>,
}

/// `PlaybackStartBody` (OAS:L45534), `POST /api/v2/playback/start`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PlaybackStartBody {
    pub installation_id: String,
    pub protocol_version: i64,
    pub client_features: Vec<String>,
    pub file_id: String,
    pub profile_id: String,
    /// Client-minted idempotency key (`^[A-Za-z0-9][A-Za-z0-9._:-]{7,127}$`, `protocol_v3.go@6791aac` L1245).
    pub playback_attempt_id: String,
    /// `auto`, a resolution such as `1080p`, or a rung label from `available_qualities`.
    pub quality_preference: String,
    /// `preserve | compatible`.
    pub subtitle_fidelity_preference: String,
    pub client_capabilities: ClientCodecCapabilities,
    pub client_playback_context: ClientPlaybackContext,
    /// Seconds on the source timeline; omitted = the server's stored resume position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_position: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_track_index: Option<i64>,
    /// Omitting both subtitle fields means subtitles off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_track_index: Option<i64>,
    /// `false` pins the requested file through every replan (`fixed_media_file_v1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_alternate_versions: Option<bool>,
}

/// `TerminalV3`: why a decision is not playable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    pub reason: String,
    pub message: String,
    pub retryable: bool,
}

/// `StreamV3` (OAS:L51929).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stream {
    /// `uri-reference`: relative URLs resolve against the server origin.
    pub url: String,
    /// `http_progressive | hls`.
    pub protocol: String,
    pub headers: BTreeMap<String, String>,
    /// `none | session`.
    pub header_refresh: String,
    pub header_refresh_url: Option<String>,
    pub container: Option<String>,
    pub mime_type: Option<String>,
}

/// `TimelineV3` (OAS:L53946). Keep the four numbers distinct (`playback-protocol-v3.md@6791aac` §5).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timeline {
    pub source_start_seconds: f64,
    pub stream_origin_seconds: f64,
    /// Where the player seeks after load.
    pub player_start_seconds: f64,
    /// Added to the player position to give the source position (what progress reports).
    pub timeline_offset_seconds: f64,
    pub can_seek_anywhere: bool,
    pub seek_restoration: String,
    pub seek_window_start_seconds: Option<f64>,
    pub seek_window_end_seconds: Option<f64>,
}

impl Timeline {
    /// Source-timeline position for a player position.
    pub fn source_position(&self, player_seconds: f64) -> f64 {
        (player_seconds + self.timeline_offset_seconds).max(0.0)
    }

    /// Player position for a source-timeline position.
    pub fn player_position(&self, source_seconds: f64) -> f64 {
        (source_seconds - self.timeline_offset_seconds).max(0.0)
    }

    /// Can the player seek to this source position without a `seek_reanchor` replan?
    pub fn can_seek_to(&self, source_seconds: f64) -> bool {
        if self.can_seek_anywhere {
            return true;
        }
        let start = self.seek_window_start_seconds.unwrap_or(f64::NEG_INFINITY);
        let end = self.seek_window_end_seconds.unwrap_or(f64::INFINITY);
        source_seconds >= start && source_seconds <= end
    }
}

/// `SubtitleInventoryItemV3` (OAS:L52462).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleInventoryItem {
    pub track_id: String,
    pub combined_index: i64,
    pub source: String,
    pub delivery: String,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub label: Option<String>,
    pub forced: bool,
    pub hearing_impaired: bool,
    pub default: bool,
    pub unplayable: Option<bool>,
    pub url: Option<String>,
    pub stream_index: Option<i64>,
    pub container_track_id: Option<String>,
    pub native_selectable: Option<bool>,
}

/// `SubtitleArtifactV3`: a sidecar file for the selected subtitle.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleArtifact {
    pub url: String,
    pub format: String,
    pub mime_type: String,
    pub timing_origin_seconds: f64,
}

/// `EmbeddedSubtitleV3`: select this stream in-process.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddedSubtitle {
    pub stream_index: i64,
    pub container_track_id: Option<String>,
}

/// `SubtitleDecisionV3` (OAS:L52265). `mode` is `off | render | convert | burn_in`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleDecision {
    pub mode: String,
    pub track_id: Option<String>,
    pub inventory: Vec<SubtitleInventoryItem>,
    pub artifact: Option<SubtitleArtifact>,
    pub embedded: Option<EmbeddedSubtitle>,
}

/// `AvailableQualityV3`: a quality rung offered in the quality menu.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AvailableQuality {
    pub label: String,
    pub display_name: Option<String>,
    pub height: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    pub preserves_source: bool,
}

/// `DegradationWarningV3`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DegradationWarning {
    pub code: String,
    pub message: String,
}

/// `PlaybackSource` subset (stats overlay).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSource {
    pub media_file_id: String,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_channels: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frame_rate: Option<f64>,
    pub bitrate_kbps: Option<i64>,
    pub dynamic_range: Option<String>,
    pub dolby_vision_profile: Option<i64>,
    pub duration_seconds: Option<f64>,
}

/// `PlaybackPlan` (OAS:L44282) subset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackPlan {
    pub plan_id: String,
    pub plan_attempt_key: String,
    /// `original_http | server_remux_progressive | server_remux_hls | server_transcode_hls`.
    pub delivery: String,
    pub decision_reason: String,
    pub stream: Stream,
    pub timeline: Timeline,
    pub selected_tracks: SelectedTracks,
    pub subtitle: SubtitleDecision,
    pub available_qualities: Vec<AvailableQuality>,
    pub active_quality: Option<String>,
    pub degradation_warnings: Vec<DegradationWarning>,
    pub requested_media_file_id: String,
    pub effective_media_file_id: String,
    pub source: PlaybackSource,
    pub expires_at: Option<String>,
}

impl PlaybackPlan {
    /// The original file, byte for byte: the player owns tracks, subtitles and dynamic range.
    pub fn is_direct(&self) -> bool {
        self.delivery == "original_http"
    }
}

/// `PlaybackDecision` (OAS:L44188), from start and replan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackDecision {
    pub protocol_version: i64,
    /// `playable` or a terminal outcome such as `adaptation_unavailable`.
    pub outcome: String,
    pub session_id: Option<String>,
    pub server_features: Vec<String>,
    pub terminal: Option<Terminal>,
    pub playback_plan: Option<PlaybackPlan>,
}

impl PlaybackDecision {
    /// The playable session and plan, if the decision is playable.
    pub fn playable(&self) -> Option<(&str, &PlaybackPlan)> {
        match (&self.outcome[..], self.session_id.as_deref(), &self.playback_plan) {
            ("playable", Some(s), Some(p)) if !s.is_empty() => Some((s, p)),
            _ => None,
        }
    }
}

/// `PlaybackProgressBody` (OAS:L44515).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackProgressBody {
    pub installation_id: String,
    /// Positive, strictly increasing per session.
    pub sequence: i64,
    /// Seconds on the source timeline.
    pub position: f64,
    pub is_paused: bool,
}

/// `PlaybackStopBody` (OAS:L45666). `sequence` and `position` travel together or not at all.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackStopBody {
    pub installation_id: String,
    /// Client-minted UUID, reused on every retry.
    pub stop_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_paused: Option<bool>,
}

/// `PlaybackAccepted`: the latest committed sample.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackAccepted {
    pub sequence: i64,
    pub position: f64,
    pub is_paused: bool,
}

/// `PlaybackMutation` (OAS:L44251): progress and stop receipts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackMutation {
    /// `applied | stale_sample | replayed | stopped`.
    pub outcome: String,
    pub accepted: Option<PlaybackAccepted>,
    pub stop_id: Option<String>,
    pub history_id: Option<String>,
}

/// `FailureV3`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub classification: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `PlaybackReplanBody` (OAS:L44548), `POST /api/v2/playback/{session_id}/replan`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PlaybackReplanBody {
    pub installation_id: String,
    pub protocol_version: i64,
    /// `failure_recovery | seek_reanchor | seek_failure_recovery | track_change | quality_change | output_change`.
    pub operation: String,
    pub playback_attempt_id: String,
    /// Client-minted idempotency key for this replan.
    pub replan_request_id: String,
    pub failed_plan_id: String,
    /// Client-minted id of the plan load being replaced (Android mints a UUID per load).
    pub plan_attempt_id: String,
    pub plan_attempt_key: String,
    pub attempted_plan_keys: Vec<String>,
    /// 1..=8.
    pub attempt_count: i64,
    pub quality_preference: String,
    pub position_seconds: f64,
    pub selected_tracks: SelectedTracks,
    pub client_capabilities: ClientCodecCapabilities,
    pub client_playback_context: ClientPlaybackContext,
    pub client_features: Vec<String>,
    /// Required for failure recovery, forbidden for intent replans.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

/// `WatchTrickplaySheet`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrickplaySheet {
    pub index: i64,
    pub url: String,
}

/// `WatchTrickplay` (`getWatchTrickplay`, OAS:L234363): seek-preview sprite sheets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchTrickplay {
    pub file_id: String,
    pub interval_ms: i64,
    pub thumbnail_width: i64,
    pub thumbnail_height: i64,
    pub tile_columns: i64,
    pub tile_rows: i64,
    pub thumbnail_count: i64,
    pub expires_at: String,
    pub sheets: Vec<TrickplaySheet>,
}

/// `EventsSocketTicket` (OAS:L34928), `POST /api/v2/events/ws-ticket`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventsSocketTicket {
    pub ticket: String,
    pub expires_in: i64,
    pub max_connection_seconds: i64,
    pub protocol: String,
}

/// A fresh client-minted identifier (attempt, replan, stop and plan-load ids): a v4 UUID, which satisfies the server's
/// bounded-identifier pattern and the canonical-UUID rule for `stop_id`.
pub fn mint_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn timeline_maps_player_and_source_positions() {
        let t = Timeline {
            timeline_offset_seconds: 12.5,
            can_seek_anywhere: false,
            seek_window_start_seconds: Some(10.0),
            seek_window_end_seconds: Some(100.0),
            ..Timeline::default()
        };
        assert_eq!(t.source_position(7.5), 20.0);
        assert_eq!(t.player_position(20.0), 7.5);
        assert_eq!(t.player_position(3.0), 0.0);
        assert!(t.can_seek_to(50.0));
        assert!(!t.can_seek_to(150.0));
        assert!(
            Timeline {
                can_seek_anywhere: true,
                ..t
            }
            .can_seek_to(1e6)
        );
    }

    #[test]
    fn decision_playable_needs_session_and_plan() {
        let mut d: PlaybackDecision = serde_json::from_str(
            r#"{"protocol_version":3,"outcome":"playable","session_id":"s1","server_features":[],
                "playback_plan":{"plan_id":"p","delivery":"original_http","future":1}}"#,
        )
        .unwrap();
        assert_eq!(d.playable().map(|(s, p)| (s, p.is_direct())), Some(("s1", true)));
        d.session_id = None;
        assert!(d.playable().is_none());
        let t: PlaybackDecision = serde_json::from_str(
            r#"{"protocol_version":3,"outcome":"adaptation_unavailable","server_features":[],
                "terminal":{"reason":"source_unreadable","message":"m","retryable":false}}"#,
        )
        .unwrap();
        assert!(t.playable().is_none());
        assert_eq!(t.terminal.unwrap().reason, "source_unreadable");
    }

    #[test]
    fn optional_request_fields_are_omitted() {
        let stop = serde_json::to_value(PlaybackStopBody {
            installation_id: "i".into(),
            stop_id: "s".into(),
            sequence: None,
            position: None,
            is_paused: None,
        })
        .unwrap();
        assert_eq!(stop, serde_json::json!({"installation_id":"i","stop_id":"s"}));
        let start = serde_json::to_value(PlaybackStartBody::default()).unwrap();
        assert!(start.get("start_position").is_none());
        assert!(start.get("subtitle_track_index").is_none());
    }

    #[test]
    fn capabilities_usable() {
        let mut c = PlaybackCapabilities {
            state: "available".into(),
            allowed: true,
            installation_id: Some("inst".into()),
            protocol_versions: vec![3],
            features: vec!["Sequenced_Progress_V1".into()],
            ..PlaybackCapabilities::default()
        };
        assert!(c.usable() && c.has_feature("sequenced_progress_v1"));
        c.protocol_versions = vec![2];
        assert!(!c.usable());
    }
}
