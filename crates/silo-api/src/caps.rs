//! What Granary declares to the server (playback protocol v3, ADR-0008): everything libmpv/FFmpeg decodes, so the
//! server direct-plays the original file wherever it can and remuxes/transcodes only when it must.
//!
//! - Evidence tier `declared`: flat codec lists grant direct play (`playback-protocol-v3.md@6791aac` §3). mpv decodes
//!   in software whatever D3D11VA cannot, so a flat "can decode" is the honest claim; hardware decoders are reported
//!   separately from the D3D11 decoder-profile probe.
//! - `original_http` claims `client_managed_dynamic_range_v1` (mpv/libplacebo tone-maps any HDR source to the active
//!   output) and `client_selected_audio_track_v1` (mpv selects the audio ordinal itself), `protocol_v3.go@6791aac`
//!   L145-160 and `capabilities_v3.go@6791aac` L515-534.
//! - Codec and container names are FFmpeg/ffprobe names as the server normalises them (`capabilities_v3.go@6791aac`
//!   `normalizeCodecV3`, `scanner/probe.go@6791aac` `detectContainer`).
use crate::playback::*;
use std::collections::BTreeMap;

/// Video codecs FFmpeg (as bundled with libmpv) decodes.
pub const VIDEO_CODECS: &[&str] = &[
    "h264",
    "hevc",
    "av1",
    "vp9",
    "vp8",
    "mpeg2video",
    "mpeg1video",
    "mpeg4",
    "vc1",
    "wmv3",
    "wmv2",
    "msmpeg4v3",
    "h263",
    "theora",
    "prores",
    "mjpeg",
    "dnxhd",
];

/// Audio codecs FFmpeg decodes (bitstream passthrough is a Phase 7 opt-in and is not declared here).
pub const AUDIO_CODECS: &[&str] = &[
    "aac",
    "ac3",
    "eac3",
    "truehd",
    "mlp",
    "dts",
    "flac",
    "opus",
    "vorbis",
    "mp3",
    "mp2",
    "alac",
    "pcm_s16le",
    "pcm_s24le",
    "pcm_s32le",
    "pcm_f32le",
    "pcm_s16be",
    "pcm_bluray",
    "pcm_dvd",
    "wmav2",
    "wmapro",
    "aac_latm",
];

/// Containers the server's scanner reports that FFmpeg demuxes.
pub const CONTAINERS: &[&str] = &[
    "mkv", "webm", "mp4", "mov", "avi", "ts", "m2ts", "flv", "ogg", "wmv", "mpeg",
];

/// Subtitle codecs mpv renders from inside a container, selected by FFmpeg stream index.
const MKV_SUBTITLES: &[&str] = &[
    "subrip",
    "ass",
    "ssa",
    "webvtt",
    "hdmv_pgs_subtitle",
    "dvd_subtitle",
    "dvb_subtitle",
];
const MP4_SUBTITLES: &[&str] = &["mov_text"];

/// Features Granary implements, sent only when the server advertises them (`playback_plan_v3` always).
const CLIENT_FEATURES: &[&str] = &[
    "playback_plan_v3",
    "sequenced_progress_v1",
    "device_quirks_v1",
    "seek_reanchor_v1",
    "direct_stream_resume_v1",
    "embedded_subtitles_v1",
    // Media requests go through the host relay, which adds the bearer and profile headers itself.
    "header_authenticated_media_v1",
    "plan_source_duration_v1",
    "output_display_evidence_v1",
];

/// Facts about the output the player window is on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputFacts {
    /// Stable id of the monitor (GDI device name), if known.
    pub display_id: Option<String>,
    /// Windows HDR (advanced colour) active on that monitor; `None` when it could not be probed.
    pub hdr_active: Option<bool>,
}

/// Hardware video decoders found by the D3D11 decoder-profile probe (FFmpeg codec names).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HardwareDecoders {
    pub codecs: Vec<String>,
    /// HEVC Main10 / VP9 profile 2 / AV1 10-bit support (10-bit HDR in hardware).
    pub ten_bit: Vec<String>,
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// Decode-side HDR: libplacebo handles HDR10, HDR10+ and HLG, and Dolby Vision profiles 5 and 8 via the RPU
/// (Phase 7 refines DV per ADR-0006/R4).
fn decode_hdr() -> HdrCapabilities {
    HdrCapabilities {
        hdr10: true,
        hdr10_plus: true,
        hlg: true,
        dolby_vision_profiles: vec![5, 8],
    }
}

pub fn client_capabilities(hw: &HardwareDecoders) -> ClientCodecCapabilities {
    ClientCodecCapabilities {
        video_evidence: "declared".into(),
        audio_evidence: "declared".into(),
        codecs_video: strings(VIDEO_CODECS),
        codecs_video_hardware: hw.codecs.clone(),
        codecs_audio: strings(AUDIO_CODECS),
        containers: strings(CONTAINERS),
        hdr: true,
        hdr_details: Some(decode_hdr()),
        max_resolution: None,
        video_decode: Vec::new(),
        audio_passthrough: None,
    }
}

fn native_embedded() -> Vec<NativeEmbeddedSubtitleCapability> {
    [("mkv", MKV_SUBTITLES), ("mp4", MP4_SUBTITLES)]
        .into_iter()
        .map(|(container, codecs)| NativeEmbeddedSubtitleCapability {
            container: container.into(),
            codecs: strings(codecs),
            track_identity: "ffmpeg_stream_index".into(),
            ass_styling: true,
            font_attachments: true,
        })
        .collect()
}

fn subtitles(native: bool) -> DeliverySubtitleCapabilities {
    DeliverySubtitleCapabilities {
        embedded_text: true,
        embedded_bitmap: true,
        sidecar_text: true,
        sidecar_bitmap: true,
        ass_styling: true,
        font_attachments: true,
        native_embedded: if native { native_embedded() } else { Vec::new() },
    }
}

fn delivery(containers: &[&str], native_subtitles: bool, claims: &[&str]) -> DeliveryCapability {
    DeliveryCapability {
        enabled: true,
        supported_on_device: true,
        containers: strings(containers),
        video_codecs: strings(VIDEO_CODECS),
        audio_decode_codecs: strings(AUDIO_CODECS),
        audio_passthrough_codecs: Vec::new(),
        subtitles: subtitles(native_subtitles),
        features: Vec::new(),
        transformations: Vec::new(),
        validated_claims: strings(claims),
        // The relay re-reads the current tokens for every media request.
        auth_header_refresh: true,
    }
}

/// Output HDR as the server should see it: the raw display probe plus `hdr_details` (decoder ∩ display).
fn output(facts: &OutputFacts) -> OutputContext {
    let panel = match facts.hdr_active {
        Some(true) => Some(HdrCapabilities {
            hdr10: true,
            hdr10_plus: false,
            hlg: true,
            dolby_vision_profiles: Vec::new(),
        }),
        _ => None,
    };
    OutputContext {
        output_context_id: facts.display_id.clone(),
        sink_type: Some("display".into()),
        current_sink: None,
        display: Some(OutputDisplay {
            hdr_evidence: if facts.hdr_active.is_some() { "exact" } else { "unknown" }.into(),
            hdr_types: panel.clone(),
            display_id: facts.display_id.as_ref().map(|d| d.chars().take(64).collect()),
        }),
        hdr_details: panel,
        audio_passthrough: None,
    }
}

pub fn playback_context(app_version: &str, os_version: &str, facts: &OutputFacts) -> ClientPlaybackContext {
    let mut deliveries = BTreeMap::new();
    deliveries.insert(
        DELIVERY_ORIGINAL_HTTP.to_owned(),
        delivery(
            CONTAINERS,
            true,
            &["client_managed_dynamic_range_v1", "client_selected_audio_track_v1"],
        ),
    );
    deliveries.insert(
        DELIVERY_PROGRESSIVE.to_owned(),
        delivery(&["mp4", "mkv", "matroska", "webm"], false, &[]),
    );
    deliveries.insert(DELIVERY_HLS.to_owned(), delivery(&["hls", "m3u8"], false, &[]));
    ClientPlaybackContext {
        protocol_version: PROTOCOL_VERSION,
        form_factor: "desktop".into(),
        app_version: app_version.chars().take(64).collect(),
        app_channel: None,
        device: DeviceContext {
            platform: "windows".into(),
            manufacturer: None,
            model: None,
            os_version: Some(os_version.chars().take(128).collect()),
            platform_details: BTreeMap::from([("player".to_owned(), "libmpv".to_owned())]),
        },
        output: output(facts),
        deliveries,
    }
}

/// Granary's features intersected with the server's (`playback_plan_v3` is always sent).
pub fn client_features(server: &PlaybackCapabilities) -> Vec<String> {
    CLIENT_FEATURES
        .iter()
        .filter(|f| **f == "playback_plan_v3" || server.has_feature(f))
        .map(|f| (*f).to_owned())
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn declared_capabilities_cover_mpv_and_respect_server_limits() {
        let c = client_capabilities(&HardwareDecoders {
            codecs: vec!["hevc".into()],
            ten_bit: vec![],
        });
        // Server bounds (protocol_v3.go@6791aac L1443): at most 64 entries per list.
        for list in [&c.codecs_video, &c.codecs_audio, &c.containers] {
            assert!(list.len() <= 64 && list.iter().all(|s| s == &s.to_lowercase()));
        }
        for codec in ["hevc", "av1", "vc1", "h264"] {
            assert!(c.codecs_video.iter().any(|v| v == codec));
        }
        for codec in ["truehd", "dts", "eac3", "flac"] {
            assert!(c.codecs_audio.iter().any(|v| v == codec));
        }
        assert_eq!(c.video_evidence, "declared");
    }

    #[test]
    fn original_http_claims_client_managed_range_and_audio_selection() {
        let ctx = playback_context("0.1.0", "10.0.26200", &OutputFacts::default());
        let o = &ctx.deliveries["original_http"];
        assert!(o.enabled && o.supported_on_device);
        assert!(
            o.validated_claims
                .contains(&"client_managed_dynamic_range_v1".to_owned())
        );
        assert!(
            o.validated_claims
                .contains(&"client_selected_audio_track_v1".to_owned())
        );
        assert!(!o.subtitles.native_embedded.is_empty());
        assert!(ctx.deliveries["hls"].validated_claims.is_empty());
        assert!(ctx.form_factor.len() <= 32 && ctx.device.platform.len() <= 32);
        // Unknown HDR state is reported as such, never as an SDR panel.
        assert_eq!(ctx.output.display.as_ref().unwrap().hdr_evidence, "unknown");
        assert!(ctx.output.hdr_details.is_none());
    }

    #[test]
    fn hdr_output_is_exact_when_probed() {
        let ctx = playback_context(
            "0.1.0",
            "10",
            &OutputFacts {
                display_id: Some(r"\\.\DISPLAY1".into()),
                hdr_active: Some(true),
            },
        );
        let d = ctx.output.display.unwrap();
        assert_eq!(d.hdr_evidence, "exact");
        assert_eq!(d.hdr_types, ctx.output.hdr_details);
        assert!(ctx.output.hdr_details.unwrap().hdr10);
        let sdr = playback_context(
            "0.1.0",
            "10",
            &OutputFacts {
                display_id: None,
                hdr_active: Some(false),
            },
        );
        assert_eq!(sdr.output.display.unwrap().hdr_types, None);
    }

    #[test]
    fn features_are_intersected_with_the_server() {
        let server = PlaybackCapabilities {
            features: vec![
                "seek_reanchor_v1".into(),
                "header_authenticated_media_v1".into(),
                "x".into(),
            ],
            ..PlaybackCapabilities::default()
        };
        assert_eq!(
            client_features(&server),
            ["playback_plan_v3", "seek_reanchor_v1", "header_authenticated_media_v1"]
        );
    }
}
