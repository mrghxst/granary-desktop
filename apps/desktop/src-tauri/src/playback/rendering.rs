//! Per-title rendering options (ADR-0012): the rendering quality level, GPU video processing (NVIDIA RTX Video Super
//! Resolution / RTX HDR, Intel Video Super Resolution through mpv's `d3d11vpp` filter) and the Dolby Vision switch.
//!
//! Every load sets every key this module owns, so a change in the settings (or a lighter level for one title) never
//! leaks into the next one. The "high" level is mpv as configured (baseline, bundled and user `mpv.conf`): its values
//! are read back from mpv once after start-up ([`remember_configured`]), so a power user's `mpv.conf` keeps working.
use crate::settings::PlayerSettings;
use granary_silo_api::playback::PlaybackSource;
use serde::Serialize;
use std::sync::Mutex;

/// The mpv options the quality levels change (plus `vf`, which the video filters own).
pub const KEYS: [&str; 13] = [
    "scale",
    "dscale",
    "cscale",
    "scale-antiring",
    "dither",
    "correct-downscaling",
    "linear-downscaling",
    "sigmoid-upscaling",
    "deband",
    "hdr-compute-peak",
    "allow-delayed-peak-detect",
    "hdr-peak-percentile",
    "hdr-contrast-recovery",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Vendor {
    Nvidia,
    Intel,
    Amd,
    Other,
}

impl Vendor {
    pub fn from_pci(id: u32) -> Self {
        match id {
            0x10DE => Self::Nvidia,
            0x8086 => Self::Intel,
            0x1002 | 0x1022 => Self::Amd,
            _ => Self::Other,
        }
    }
}

/// What the default GPU can do (probed once, `platform::probe_gpu`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub name: String,
    pub vendor: Vendor,
    /// Integrated GPU sharing system memory (`D3D11_FEATURE_D3D11_OPTIONS2.UnifiedMemoryArchitecture`).
    pub uma: bool,
    /// The driver accepts the NVIDIA RTX Video Super Resolution extension on a video processor.
    pub rtx_vsr: bool,
    /// The driver offers the NVIDIA RTX Video HDR extension.
    pub rtx_hdr: bool,
}

impl Default for Gpu {
    fn default() -> Self {
        Self {
            name: String::new(),
            vendor: Vendor::Other,
            uma: false,
            rtx_vsr: false,
            rtx_hdr: false,
        }
    }
}

impl Gpu {
    /// The `d3d11vpp` scaling mode for Video Super Resolution on this GPU, if any. Intel VSR runs on Arc and Xe
    /// graphics; mpv falls back to the driver's plain scaler where the extension is missing, so older Intel chips
    /// only lose the AI part.
    pub fn vsr_mode(&self) -> Option<&'static str> {
        match self.vendor {
            Vendor::Nvidia if self.rtx_vsr => Some("nvidia"),
            Vendor::Intel => Some("intel"),
            _ => None,
        }
    }
}

/// Facts outside the settings that decide the options.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub gpu: Gpu,
    /// A user `mpv.conf` exists: "auto" then leaves the rendering to it.
    pub user_conf: bool,
}

/// One playback's inputs besides the settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct Target {
    /// Height of the monitor holding the player, in physical pixels.
    pub monitor_height: Option<i32>,
    /// The swapchain is HDR (Windows HDR on for this playback).
    pub hdr_output: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Fast,
    Balanced,
    High,
    Max,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Balanced => "balanced",
            Self::High => "high",
            Self::Max => "max",
        }
    }

    /// Changes from mpv as configured. `fast` is mpv's built-in `fast` profile without debanding or peak detection;
    /// `balanced` drops only the two costliest extras (debanding, same-frame peak detection), which keeps an
    /// integrated GPU cool at 4K; `max` is mpv's `high-quality` profile with debanding.
    fn changes(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Fast => &[
                ("scale", "bilinear"),
                ("dscale", "bilinear"),
                ("cscale", "bilinear"),
                ("dither", "no"),
                ("correct-downscaling", "no"),
                ("linear-downscaling", "no"),
                ("sigmoid-upscaling", "no"),
                ("deband", "no"),
                ("hdr-compute-peak", "no"),
                ("allow-delayed-peak-detect", "yes"),
            ],
            Self::Balanced => &[("deband", "no"), ("allow-delayed-peak-detect", "yes")],
            Self::High => &[],
            Self::Max => &[
                ("scale", "ewa_lanczossharp"),
                ("cscale", "ewa_lanczossharp"),
                ("scale-antiring", "0.6"),
                ("deband", "yes"),
                ("hdr-peak-percentile", "99.995"),
                ("hdr-contrast-recovery", "0.30"),
            ],
        }
    }
}

/// The level for a setting: "auto" is "balanced" on an integrated GPU and "high" elsewhere, or whenever a user
/// `mpv.conf` decides.
pub fn level(setting: &str, env: &Env) -> Level {
    match setting {
        "fast" => Level::Fast,
        "balanced" => Level::Balanced,
        "high" => Level::High,
        "max" => Level::Max,
        _ if env.gpu.uma && !env.user_conf => Level::Balanced,
        _ => Level::High,
    }
}

static GPU: std::sync::OnceLock<Gpu> = std::sync::OnceLock::new();

/// The GPU, probed once with `probe` (later calls return the first result).
pub fn known_gpu(probe: impl FnOnce() -> Gpu) -> Gpu {
    GPU.get_or_init(probe).clone()
}

static CONFIGURED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Remember mpv's configured values of [`KEYS`] and `vf` right after start-up (before any title changed them).
pub fn remember_configured(get: impl Fn(&str) -> Option<String>) {
    let values: Vec<(String, String)> = KEYS
        .iter()
        .chain(&["vf"])
        .filter_map(|k| get(k).map(|v| ((*k).to_owned(), v)))
        .collect();
    tracing::debug!(?values, "configured rendering options");
    *CONFIGURED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = values;
}

fn configured() -> Vec<(String, String)> {
    CONFIGURED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// What the stats overlay shows about the rendering.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decision {
    pub options: Vec<(String, String)>,
    pub summary: Vec<(String, String)>,
}

/// Options for one title. `configured` is mpv's own configuration ([`remember_configured`]).
pub fn decide(
    player: &PlayerSettings,
    env: &Env,
    source: &PlaybackSource,
    target: Target,
    configured: &[(String, String)],
) -> Decision {
    let level = level(&player.render_quality, env);
    let mut options: Vec<(String, String)> = configured.iter().filter(|(k, _)| k != "vf").cloned().collect();
    for (k, v) in level.changes() {
        match options.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => (*v).clone_into(&mut slot.1),
            None => options.push(((*k).to_owned(), (*v).to_owned())),
        }
    }
    let mut summary = vec![(
        "render-quality".to_owned(),
        if player.render_quality == "auto" {
            format!(
                "{} (auto{})",
                level.name(),
                if env.gpu.uma { ", integrated GPU" } else { "" }
            )
        } else {
            level.name().to_owned()
        },
    )];

    let mut filters: Vec<String> = Vec::new();
    let configured_vf = configured.iter().find(|(k, _)| k == "vf").map(|(_, v)| v.as_str());
    if let Some(vf) = configured_vf.filter(|v| !v.is_empty()) {
        filters.push(vf.to_owned());
    }
    let (vpp, why) = video_processing(player, env, source, target);
    if let Some(f) = vpp {
        filters.push(f);
    }
    summary.push(("video-processing".to_owned(), why));
    // Profiles 7 and 8 carry an HDR10/HLG/SDR base layer that plays correctly without the RPU; profile 5 has none.
    let dv = source.dolby_vision_profile;
    if !player.dolby_vision && matches!(dv, Some(7 | 8)) {
        filters.push("format=dolbyvision=no".to_owned());
        summary.push(("dolby-vision".to_owned(), "off (base layer)".to_owned()));
    } else if !player.dolby_vision && dv == Some(5) {
        summary.push((
            "dolby-vision".to_owned(),
            "kept (profile 5 has no base layer)".to_owned(),
        ));
    }
    if configured_vf.is_some() || !filters.is_empty() {
        options.push(("vf".to_owned(), filters.join(",")));
    }
    Decision { options, summary }
}

/// The `d3d11vpp` filter for this title, and a line for the stats overlay.
fn video_processing(
    player: &PlayerSettings,
    env: &Env,
    source: &PlaybackSource,
    target: Target,
) -> (Option<String>, String) {
    if !player.super_resolution && !player.rtx_hdr {
        return (None, "off".into());
    }
    if player.hwdec == "no" {
        return (None, "off (needs hardware decoding)".into());
    }
    let sdr = source.dolby_vision_profile.is_none()
        && source
            .dynamic_range
            .as_deref()
            .is_none_or(|r| r.eq_ignore_ascii_case("sdr"));
    if !sdr {
        return (None, "off (HDR source)".into());
    }
    let mut parts: Vec<String> = Vec::new();
    let mut what: Vec<&str> = Vec::new();
    if player.super_resolution
        && let Some(mode) = env.gpu.vsr_mode()
        && let (Some(src), Some(screen)) = (source.height.filter(|h| *h > 0), target.monitor_height)
    {
        let factor = (f64::from(screen) / src as f64).min(4.0);
        // Only when the video is upscaled noticeably; at about 1:1 the plain path is as sharp and cheaper.
        if factor >= 1.1 {
            parts.push(format!("scaling-mode={mode}:scale={factor:.2}"));
            what.push(if mode == "nvidia" {
                "RTX Video Super Resolution"
            } else {
                "Intel Video Super Resolution"
            });
        }
    }
    if player.rtx_hdr && env.gpu.rtx_hdr && target.hdr_output {
        parts.push("format=x2bgr10:nvidia-true-hdr".into());
        what.push("RTX Video HDR");
    }
    if parts.is_empty() {
        return (None, "off (not needed or not supported)".into());
    }
    (Some(format!("d3d11vpp={}", parts.join(":"))), what.join(" + "))
}

/// [`decide`] with mpv's remembered configuration.
pub fn decide_now(player: &PlayerSettings, env: &Env, source: &PlaybackSource, target: Target) -> Decision {
    decide(player, env, source, target, &configured())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    fn get<'a>(o: &'a [(String, String)], k: &str) -> Option<&'a str> {
        o.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    fn configured() -> Vec<(String, String)> {
        [
            ("scale", "lanczos"),
            ("dscale", "hermite"),
            ("cscale", ""),
            ("deband", "yes"),
            ("allow-delayed-peak-detect", "no"),
            ("hdr-contrast-recovery", "0.000000"),
            ("vf", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
    }

    fn env(vendor: Vendor, uma: bool) -> Env {
        Env {
            gpu: Gpu {
                name: "test".into(),
                vendor,
                uma,
                rtx_vsr: vendor == Vendor::Nvidia,
                rtx_hdr: vendor == Vendor::Nvidia,
            },
            user_conf: false,
        }
    }

    fn sdr(height: i64) -> PlaybackSource {
        PlaybackSource {
            dynamic_range: Some("SDR".into()),
            height: Some(height),
            ..PlaybackSource::default()
        }
    }

    const SCREEN_1440: Target = Target {
        monitor_height: Some(1440),
        hdr_output: true,
    };

    #[test]
    fn auto_is_slightly_lighter_on_an_integrated_gpu_only() {
        let p = PlayerSettings::default();
        assert_eq!(p.render_quality, "auto");
        let igpu = decide(&p, &env(Vendor::Intel, true), &sdr(2160), SCREEN_1440, &configured());
        assert_eq!(get(&igpu.options, "deband"), Some("no"));
        assert_eq!(get(&igpu.options, "allow-delayed-peak-detect"), Some("yes"));
        assert_eq!(get(&igpu.options, "scale"), Some("lanczos"));
        assert_eq!(igpu.summary[0].1, "balanced (auto, integrated GPU)");
        let dgpu = decide(&p, &env(Vendor::Nvidia, false), &sdr(2160), SCREEN_1440, &configured());
        assert_eq!(get(&dgpu.options, "deband"), Some("yes"));
        assert_eq!(dgpu.summary[0].1, "high (auto)");
        // A user mpv.conf decides on its own.
        let mut e = env(Vendor::Intel, true);
        e.user_conf = true;
        assert_eq!(level("auto", &e), Level::High);
    }

    #[test]
    fn every_level_sets_every_key_it_touches_back_from_the_configuration() {
        let mut p = PlayerSettings {
            render_quality: "max".into(),
            ..PlayerSettings::default()
        };
        let e = env(Vendor::Amd, false);
        let max = decide(&p, &e, &sdr(1080), SCREEN_1440, &configured());
        assert_eq!(get(&max.options, "scale"), Some("ewa_lanczossharp"));
        assert_eq!(get(&max.options, "hdr-contrast-recovery"), Some("0.30"));
        p.render_quality = "high".into();
        let high = decide(&p, &e, &sdr(1080), SCREEN_1440, &configured());
        assert_eq!(get(&high.options, "scale"), Some("lanczos"));
        assert_eq!(get(&high.options, "hdr-contrast-recovery"), Some("0.000000"));
        assert_eq!(
            get(&high.options, "vf"),
            Some(""),
            "filters of the previous title are cleared"
        );
        p.render_quality = "fast".into();
        let fast = decide(&p, &e, &sdr(1080), SCREEN_1440, &configured());
        assert_eq!(get(&fast.options, "dscale"), Some("bilinear"));
        assert_eq!(get(&fast.options, "dither"), Some("no"));
    }

    #[test]
    fn super_resolution_upscales_sdr_to_the_monitor_on_supported_gpus() {
        let p = PlayerSettings {
            super_resolution: true,
            rtx_hdr: true,
            ..PlayerSettings::default()
        };
        let nv = decide(&p, &env(Vendor::Nvidia, false), &sdr(1080), SCREEN_1440, &configured());
        assert_eq!(
            get(&nv.options, "vf"),
            Some("d3d11vpp=scaling-mode=nvidia:scale=1.33:format=x2bgr10:nvidia-true-hdr")
        );
        assert_eq!(nv.summary[1].1, "RTX Video Super Resolution + RTX Video HDR");
        // SDR output: no RTX HDR; the Intel mode on Intel.
        let sdr_screen = Target {
            hdr_output: false,
            ..SCREEN_1440
        };
        let intel = decide(&p, &env(Vendor::Intel, true), &sdr(720), sdr_screen, &configured());
        assert_eq!(
            get(&intel.options, "vf"),
            Some("d3d11vpp=scaling-mode=intel:scale=2.00")
        );
        // Nothing to upscale, HDR sources, AMD and software decoding: no filter.
        let none = |p: &PlayerSettings, e: &Env, s: &PlaybackSource| {
            get(&decide(p, e, s, sdr_screen, &configured()).options, "vf").map(str::to_owned)
        };
        assert_eq!(none(&p, &env(Vendor::Nvidia, false), &sdr(1440)), Some(String::new()));
        let hdr = PlaybackSource {
            dynamic_range: Some("HDR10".into()),
            height: Some(1080),
            ..PlaybackSource::default()
        };
        assert_eq!(none(&p, &env(Vendor::Nvidia, false), &hdr), Some(String::new()));
        assert_eq!(none(&p, &env(Vendor::Amd, false), &sdr(720)), Some(String::new()));
        let soft = PlayerSettings {
            hwdec: "no".into(),
            ..p.clone()
        };
        assert_eq!(none(&soft, &env(Vendor::Nvidia, false), &sdr(720)), Some(String::new()));
    }

    #[test]
    fn dolby_vision_off_strips_profiles_7_and_8_only() {
        let p = PlayerSettings {
            dolby_vision: false,
            ..PlayerSettings::default()
        };
        let dv = |profile| PlaybackSource {
            dynamic_range: Some("DOVI".into()),
            dolby_vision_profile: Some(profile),
            height: Some(2160),
            ..PlaybackSource::default()
        };
        let e = env(Vendor::Nvidia, false);
        let p8 = decide(&p, &e, &dv(8), SCREEN_1440, &configured());
        assert_eq!(get(&p8.options, "vf"), Some("format=dolbyvision=no"));
        let p5 = decide(&p, &e, &dv(5), SCREEN_1440, &configured());
        assert_eq!(get(&p5.options, "vf"), Some(""));
        let on = decide(&PlayerSettings::default(), &e, &dv(8), SCREEN_1440, &configured());
        assert_eq!(get(&on.options, "vf"), Some(""));
    }

    #[test]
    fn a_configured_filter_chain_is_kept() {
        let mut c = configured();
        c.retain(|(k, _)| k != "vf");
        c.push(("vf".into(), "hflip".into()));
        let p = PlayerSettings {
            dolby_vision: false,
            ..PlayerSettings::default()
        };
        let src = PlaybackSource {
            dolby_vision_profile: Some(7),
            ..PlaybackSource::default()
        };
        let d = decide(&p, &env(Vendor::Amd, false), &src, SCREEN_1440, &c);
        assert_eq!(get(&d.options, "vf"), Some("hflip,format=dolbyvision=no"));
    }
}
