//! Pure display policy (ADR-0006, Phase 7): whether to switch Windows HDR on, which refresh rate to use, and the
//! per-monitor mpv options (tone-mapping target, SDR white, video sync). No I/O here; [`crate::session`] applies the
//! decisions through a [`crate::DisplayController`] and records them in the restore marker.
use crate::{DisplayInfo, Rational};
use serde::{Deserialize, Serialize};

/// HDR output setting (spec Phase 7: Off / HDR content only (default) / Always).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrMode {
    /// Never switch Windows HDR on (HDR content is tone-mapped to SDR unless HDR is already on).
    Off,
    /// Switch Windows HDR on for HDR content only, and restore it afterwards.
    #[default]
    ContentOnly,
    /// Switch Windows HDR on for all video (SDR then plays at the SDR white level), and restore it afterwards.
    Always,
}

/// Refresh strategy per monitor (ADR-0006).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshStrategy {
    /// Desktop default: VRR if declared → current refresh if it already fits → exact multiple at the same resolution
    /// near the current refresh → `display-resample`.
    #[default]
    Auto,
    /// TV/HTPC opt-in: the lowest mode whose refresh is an exact multiple of the content rate (23.976/24/25/...).
    MatchContent,
    /// Never change the display mode.
    Never,
}

/// User preferences for one monitor (keyed by its reboot-stable monitor path in the settings).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MonitorPrefs {
    /// "This monitor uses VRR for video": there is no Windows API that reports VRR as active (R4 §4.2).
    pub vrr: bool,
    /// Peak luminance override in nits (wins over DXGI `MaxLuminance`, e.g. after HDR calibration).
    pub peak_nits: Option<f32>,
    /// Black level override in nits (sets the target contrast; OLED panels report ~0).
    pub black_nits: Option<f32>,
    /// Refresh strategy for this monitor; `None` uses the global setting.
    pub refresh: Option<RefreshStrategy>,
}

/// Dynamic range of the content, from the playback decision (`PlaybackSource.dynamic_range`, vocabulary
/// `sdr|hdr10|hdr10_plus|hlg|dolby_vision|hdr_unknown`, `internal/playback/protocol_v3.go@6791aac` L388-393) or from
/// mpv's `video-params/gamma` once the file is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicRange {
    Sdr,
    Hdr10,
    Hdr10Plus,
    Hlg,
    DolbyVision,
    HdrUnknown,
}

impl DynamicRange {
    /// Server vocabulary; empty or unknown strings are treated as unknown (`None`).
    pub fn from_server(s: &str) -> Option<Self> {
        Some(match s {
            "sdr" => Self::Sdr,
            "hdr10" => Self::Hdr10,
            "hdr10_plus" => Self::Hdr10Plus,
            "hlg" => Self::Hlg,
            "dolby_vision" => Self::DolbyVision,
            "hdr_unknown" => Self::HdrUnknown,
            _ => return None,
        })
    }

    /// From mpv `video-params/gamma` (`pq`, `hlg`, else SDR) and `video-params/colormatrix` (`dolbyvision`).
    pub fn from_mpv(gamma: &str, colormatrix: &str) -> Self {
        if colormatrix == "dolbyvision" {
            Self::DolbyVision
        } else {
            match gamma {
                "pq" => Self::Hdr10,
                "hlg" => Self::Hlg,
                _ => Self::Sdr,
            }
        }
    }

    pub fn is_hdr(self) -> bool {
        !matches!(self, Self::Sdr)
    }
}

/// What the content needs from the display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Content {
    pub range: DynamicRange,
    /// Frames per second (container rate); `None` when unknown.
    pub fps: Option<f64>,
    /// Audio is bitstreamed to a receiver (cannot be resampled: `video-sync=audio`).
    pub passthrough: bool,
}

/// Outcome of the HDR decision for one display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdrDecision {
    /// Windows HDR is already on: output HDR directly, never toggle.
    AlreadyOn,
    /// Switch Windows HDR on before the first frame and restore it afterwards.
    Enable,
    /// Stay SDR (HDR content is tone-mapped to SDR).
    StaySdr(SdrReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdrReason {
    /// The panel cannot do HDR (or ACM/WCG on an SDR panel, `wideColorEnforced`).
    Unsupported,
    /// The setting is Off.
    SettingOff,
    /// "HDR content only" and the content is SDR.
    SdrContent,
}

impl HdrDecision {
    /// HDR output (PQ swapchain) after the decision is applied.
    pub fn hdr_output(self) -> bool {
        matches!(self, Self::AlreadyOn | Self::Enable)
    }
}

pub fn hdr_decision(display: &DisplayInfo, mode: HdrMode, content: DynamicRange) -> HdrDecision {
    if display.hdr_enabled {
        return HdrDecision::AlreadyOn;
    }
    if !display.hdr_supported {
        return HdrDecision::StaySdr(SdrReason::Unsupported);
    }
    match mode {
        HdrMode::Off => HdrDecision::StaySdr(SdrReason::SettingOff),
        HdrMode::ContentOnly if !content.is_hdr() => HdrDecision::StaySdr(SdrReason::SdrContent),
        HdrMode::ContentOnly | HdrMode::Always => HdrDecision::Enable,
    }
}

/// Refresh decision: an optional temporary mode switch plus the mpv timing options for the resulting refresh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefreshPlan {
    pub switch_to: Option<Rational>,
    pub video_sync: &'static str,
    pub max_factor: u32,
    pub reason: RefreshReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshReason {
    UnknownFps,
    Vrr,
    AlreadyFits,
    ExactMultiple,
    MatchContent,
    NoExactMode,
    NeverSwitch,
}

/// Relative tolerance for "the current refresh already fits": covers the 0.1 % NTSC offset (e.g. 240 Hz for
/// 23.976 fps), which `display-resample` absorbs by resampling audio (R4 §4.3).
const FITS: f64 = 0.0015;
/// Relative tolerance for an exact multiple mode (ADR-0006: 0.05 %).
const EXACT: f64 = 0.0005;

/// `ratio` is within `tolerance` (relative) of an integer k ≥ 1.
fn integral(ratio: f64, tolerance: f64) -> bool {
    let k = ratio.round();
    k >= 1.0 && ((ratio - k) / k).abs() <= tolerance
}

/// mpv's `video-sync-max-factor` for this refresh: room for the display/content ratio, never below mpv's default 5.
pub fn max_factor(refresh_hz: f64, fps: f64) -> u32 {
    if fps <= 0.0 {
        return 5;
    }
    ((refresh_hz / fps).ceil() as u32 + 1).max(5)
}

/// ADR-0006 refresh selection. `modes` are the refresh rates available at the current resolution and colour format
/// (rational where the driver reports them); `current` is the refresh now.
pub fn refresh_plan(
    current: Rational,
    modes: &[Rational],
    content: &Content,
    strategy: RefreshStrategy,
    vrr: bool,
) -> RefreshPlan {
    let resample = if content.passthrough {
        "audio"
    } else {
        "display-resample"
    };
    let keep = |reason, fps: f64| RefreshPlan {
        switch_to: None,
        video_sync: resample,
        max_factor: max_factor(current.hz(), fps),
        reason,
    };
    let Some(fps) = content.fps.filter(|f| f.is_finite() && *f > 1.0) else {
        return RefreshPlan {
            max_factor: 21,
            ..keep(RefreshReason::UnknownFps, 0.0)
        };
    };
    if vrr && strategy != RefreshStrategy::MatchContent {
        return RefreshPlan {
            video_sync: "audio",
            ..keep(RefreshReason::Vrr, fps)
        };
    }
    let r = current.hz();
    match strategy {
        RefreshStrategy::MatchContent => {
            // Lowest refresh that is an exact multiple (k = 1 first: 23.976/24/25/29.97/50/59.94).
            let best = modes
                .iter()
                .copied()
                .filter(|m| integral(m.hz() / fps, EXACT))
                .min_by(|a, b| a.hz().total_cmp(&b.hz()));
            match best {
                Some(m) if (m.hz() - r).abs() < 0.001 => RefreshPlan {
                    video_sync: "audio",
                    ..keep(RefreshReason::AlreadyFits, fps)
                },
                Some(m) => RefreshPlan {
                    switch_to: Some(m),
                    video_sync: "audio",
                    max_factor: max_factor(m.hz(), fps),
                    reason: RefreshReason::MatchContent,
                },
                None if integral(r / fps, FITS) => keep(RefreshReason::AlreadyFits, fps),
                None => keep(RefreshReason::NoExactMode, fps),
            }
        }
        _ if integral(r / fps, FITS) => keep(RefreshReason::AlreadyFits, fps),
        RefreshStrategy::Never => keep(RefreshReason::NeverSwitch, fps),
        RefreshStrategy::Auto => {
            let best = modes
                .iter()
                .copied()
                .filter(|m| integral(m.hz() / fps, EXACT) && m.hz() >= 0.9 * r)
                .min_by(|a, b| (a.hz() - r).abs().total_cmp(&(b.hz() - r).abs()));
            match best {
                Some(m) => RefreshPlan {
                    switch_to: Some(m),
                    video_sync: resample,
                    max_factor: max_factor(m.hz(), fps),
                    reason: RefreshReason::ExactMultiple,
                },
                None => keep(RefreshReason::NoExactMode, fps),
            }
        }
    }
}

/// Per-monitor mpv options for the display the video is on. `hdr_output` is the state after the HDR decision.
///
/// - `target-peak`: the per-monitor override, else DXGI `MaxLuminance` (small-window peak, right for OLED ABL),
///   else `auto`. Content mastered within this peak is not compressed (libplacebo with `hdr-compute-peak=auto`);
///   brighter content is tone-mapped to it with mpv's default curve (`tone-mapping=auto`, spline on gpu-next; R4 §5.2).
/// - `target-contrast`: from the black-level override, else `auto` (infinite for HDR targets, R4 §5.2).
/// - `hdr-reference-white`: the monitor's Windows SDR content brightness, set explicitly (ADR-0006 M5).
/// - Without HDR output mpv maps HDR content to the SDR target (`target-colorspace-hint=auto`).
pub fn mpv_options(
    display: &DisplayInfo,
    prefs: &MonitorPrefs,
    hdr_output: bool,
    plan: &RefreshPlan,
) -> Vec<(String, String)> {
    let mut o: Vec<(String, String)> = Vec::new();
    let peak = prefs
        .peak_nits
        .filter(|p| *p >= 10.0)
        .or(Some(display.max_luminance).filter(|p| hdr_output && *p >= 80.0));
    o.push((
        "target-peak".into(),
        match peak {
            Some(p) if hdr_output => format!("{}", p.round() as u32),
            _ => "auto".into(),
        },
    ));
    let contrast = match (prefs.black_nits, peak) {
        (Some(b), Some(p)) if b > 0.0 && hdr_output => format!("{}", ((p / b).round() as u32).clamp(10, 1_000_000)),
        (Some(b), _) if b <= 0.0 && hdr_output => "inf".into(),
        _ => "auto".into(),
    };
    o.push(("target-contrast".into(), contrast));
    o.push((
        "hdr-reference-white".into(),
        if hdr_output && display.sdr_white_nits >= 10.0 {
            format!("{}", display.sdr_white_nits.round() as u32)
        } else {
            "auto".into()
        },
    ));
    o.push(("video-sync".into(), plan.video_sync.into()));
    o.push(("video-sync-max-factor".into(), plan.max_factor.to_string()));
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DisplayId;

    fn display(hdr_supported: bool, hdr_enabled: bool) -> DisplayInfo {
        DisplayInfo {
            id: DisplayId {
                adapter_luid_low: 1,
                adapter_luid_high: 0,
                target_id: 1,
            },
            gdi_name: r"\\.\DISPLAY1".into(),
            friendly_name: "OLED".into(),
            monitor_path: "p".into(),
            refresh: Rational::new(240, 1),
            hdr_supported,
            hdr_enabled,
            wide_color_enforced: false,
            bits_per_color: 10,
            sdr_white_nits: 240.0,
            max_luminance: 1000.0,
            max_full_frame_luminance: 250.0,
            min_luminance: 0.0,
            dxgi_color_space: if hdr_enabled { 12 } else { 0 },
            desktop_rect: (0, 0, 2560, 1440),
        }
    }

    fn content(fps: f64) -> Content {
        Content {
            range: DynamicRange::Hdr10,
            fps: Some(fps),
            passthrough: false,
        }
    }

    const NTSC_FILM: f64 = 24_000.0 / 1001.0;

    #[test]
    fn hdr_on_is_never_toggled() {
        for mode in [HdrMode::Off, HdrMode::ContentOnly, HdrMode::Always] {
            assert_eq!(
                hdr_decision(&display(true, true), mode, DynamicRange::Sdr),
                HdrDecision::AlreadyOn
            );
        }
    }

    #[test]
    fn hdr_off_follows_the_setting() {
        let d = display(true, false);
        assert_eq!(
            hdr_decision(&d, HdrMode::ContentOnly, DynamicRange::Hdr10),
            HdrDecision::Enable
        );
        assert_eq!(
            hdr_decision(&d, HdrMode::ContentOnly, DynamicRange::DolbyVision),
            HdrDecision::Enable
        );
        assert_eq!(
            hdr_decision(&d, HdrMode::ContentOnly, DynamicRange::Sdr),
            HdrDecision::StaySdr(SdrReason::SdrContent)
        );
        assert_eq!(
            hdr_decision(&d, HdrMode::Always, DynamicRange::Sdr),
            HdrDecision::Enable
        );
        assert_eq!(
            hdr_decision(&d, HdrMode::Off, DynamicRange::Hdr10),
            HdrDecision::StaySdr(SdrReason::SettingOff)
        );
        assert_eq!(
            hdr_decision(&display(false, false), HdrMode::Always, DynamicRange::Hdr10),
            HdrDecision::StaySdr(SdrReason::Unsupported)
        );
    }

    #[test]
    fn server_and_mpv_ranges() {
        assert_eq!(
            DynamicRange::from_server("dolby_vision"),
            Some(DynamicRange::DolbyVision)
        );
        assert_eq!(DynamicRange::from_server("hdr10_plus"), Some(DynamicRange::Hdr10Plus));
        assert_eq!(DynamicRange::from_server(""), None);
        assert_eq!(DynamicRange::from_mpv("pq", "bt.2020-ncl"), DynamicRange::Hdr10);
        assert_eq!(DynamicRange::from_mpv("pq", "dolbyvision"), DynamicRange::DolbyVision);
        assert_eq!(DynamicRange::from_mpv("bt.1886", "bt.709"), DynamicRange::Sdr);
        assert!(DynamicRange::HdrUnknown.is_hdr());
    }

    #[test]
    fn auto_keeps_a_refresh_that_already_fits() {
        // 240 Hz fits 23.976 within 0.1 % (10.01): display-resample absorbs the NTSC offset; ratio ~10 → factor 12.
        let p = refresh_plan(
            Rational::new(240, 1),
            &[],
            &content(NTSC_FILM),
            RefreshStrategy::Auto,
            false,
        );
        assert_eq!(p.switch_to, None);
        assert_eq!(p.reason, RefreshReason::AlreadyFits);
        assert_eq!(p.video_sync, "display-resample");
        assert_eq!(p.max_factor, 12);
    }

    #[test]
    fn auto_switches_to_the_nearest_exact_multiple_at_or_near_the_current_refresh() {
        // A 165 Hz panel offering 150 Hz plays 25 fps at 150 Hz (150/25 = 6 and 150 ≥ 0.9·165).
        let modes = [
            Rational::new(60, 1),
            Rational::new(120, 1),
            Rational::new(150, 1),
            Rational::new(165, 1),
        ];
        let p = refresh_plan(
            Rational::new(165, 1),
            &modes,
            &content(25.0),
            RefreshStrategy::Auto,
            false,
        );
        assert_eq!(p.switch_to, Some(Rational::new(150, 1)));
        assert_eq!(p.reason, RefreshReason::ExactMultiple);
        assert_eq!(p.video_sync, "display-resample");
        // 23.976 on 165 Hz (6.88): 119.88 is below 0.9·165, so no switch; resample with a raised factor.
        let modes = [Rational::new(120_000, 1001), Rational::new(165, 1)];
        let p = refresh_plan(
            Rational::new(165, 1),
            &modes,
            &content(NTSC_FILM),
            RefreshStrategy::Auto,
            false,
        );
        assert_eq!(p.switch_to, None);
        assert_eq!(p.reason, RefreshReason::NoExactMode);
        assert_eq!(p.max_factor, 8);
        // 25 fps on 120 Hz (4.8): 100 Hz is exact but below 0.9·120, so keep and resample.
        let p = refresh_plan(
            Rational::new(120, 1),
            &[Rational::new(100, 1), Rational::new(120, 1)],
            &content(25.0),
            RefreshStrategy::Auto,
            false,
        );
        assert_eq!(p.switch_to, None);
    }

    #[test]
    fn vrr_keeps_the_desktop_refresh_and_syncs_to_audio() {
        let p = refresh_plan(
            Rational::new(165, 1),
            &[Rational::new(150, 1)],
            &content(25.0),
            RefreshStrategy::Auto,
            true,
        );
        assert_eq!(p.switch_to, None);
        assert_eq!(p.video_sync, "audio");
        assert_eq!(p.reason, RefreshReason::Vrr);
    }

    #[test]
    fn match_content_picks_the_lowest_exact_mode() {
        let modes = [
            Rational::new(24_000, 1001),
            Rational::new(24, 1),
            Rational::new(60_000, 1001),
            Rational::new(60, 1),
            Rational::new(120, 1),
        ];
        let p = refresh_plan(
            Rational::new(60, 1),
            &modes,
            &content(NTSC_FILM),
            RefreshStrategy::MatchContent,
            false,
        );
        assert_eq!(p.switch_to, Some(Rational::new(24_000, 1001)));
        assert_eq!(p.video_sync, "audio");
        // 24.000 fps content must not pick 23.976 (0.1 % off is not exact).
        let p = refresh_plan(
            Rational::new(60, 1),
            &modes,
            &content(24.0),
            RefreshStrategy::MatchContent,
            false,
        );
        assert_eq!(p.switch_to, Some(Rational::new(24, 1)));
        // 50 fps with no 50 Hz mode: keep and resample.
        let p = refresh_plan(
            Rational::new(60, 1),
            &modes,
            &content(50.0),
            RefreshStrategy::MatchContent,
            false,
        );
        assert_eq!((p.switch_to, p.reason), (None, RefreshReason::NoExactMode));
    }

    #[test]
    fn never_and_unknown_fps_do_not_switch() {
        let modes = [Rational::new(150, 1)];
        let p = refresh_plan(
            Rational::new(165, 1),
            &modes,
            &content(25.0),
            RefreshStrategy::Never,
            false,
        );
        assert_eq!((p.switch_to, p.reason), (None, RefreshReason::NeverSwitch));
        let c = Content {
            fps: None,
            ..content(25.0)
        };
        let p = refresh_plan(Rational::new(165, 1), &modes, &c, RefreshStrategy::Auto, false);
        assert_eq!(
            (p.switch_to, p.reason, p.max_factor),
            (None, RefreshReason::UnknownFps, 21)
        );
    }

    #[test]
    fn passthrough_never_resamples() {
        let c = Content {
            passthrough: true,
            ..content(NTSC_FILM)
        };
        let p = refresh_plan(Rational::new(240, 1), &[], &c, RefreshStrategy::Auto, false);
        assert_eq!(p.video_sync, "audio");
    }

    #[test]
    fn tone_mapping_target_follows_the_panel_or_the_override() {
        let d = display(true, true);
        let plan = refresh_plan(Rational::new(240, 1), &[], &content(24.0), RefreshStrategy::Auto, false);
        let get = |o: &[(String, String)], k: &str| o.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let o = mpv_options(&d, &MonitorPrefs::default(), true, &plan);
        assert_eq!(get(&o, "target-peak").as_deref(), Some("1000"));
        assert_eq!(get(&o, "target-contrast").as_deref(), Some("auto"));
        assert_eq!(get(&o, "hdr-reference-white").as_deref(), Some("240"));
        assert_eq!(get(&o, "video-sync").as_deref(), Some("display-resample"));
        let prefs = MonitorPrefs {
            peak_nits: Some(650.0),
            black_nits: Some(0.0),
            ..MonitorPrefs::default()
        };
        let o = mpv_options(&d, &prefs, true, &plan);
        assert_eq!(get(&o, "target-peak").as_deref(), Some("650"));
        assert_eq!(get(&o, "target-contrast").as_deref(), Some("inf"));
        let prefs = MonitorPrefs {
            black_nits: Some(0.05),
            ..MonitorPrefs::default()
        };
        assert_eq!(
            get(&mpv_options(&d, &prefs, true, &plan), "target-contrast").as_deref(),
            Some("20000")
        );
        // SDR output: mpv maps to the SDR target itself.
        let o = mpv_options(&display(true, false), &MonitorPrefs::default(), false, &plan);
        assert_eq!(get(&o, "target-peak").as_deref(), Some("auto"));
        assert_eq!(get(&o, "hdr-reference-white").as_deref(), Some("auto"));
    }
}
