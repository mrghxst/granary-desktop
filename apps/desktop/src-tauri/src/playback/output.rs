//! Display and audio output for playback (Phase 7, ADR-0006 amendment 1).
//!
//! Before the first frame of a plan the controller asks [`Output::prepare`] for the mpv options of this playback:
//! the display policy (Windows HDR switch, refresh strategy, tone-mapping target, SDR white) is applied to the
//! monitor holding the player window through a [`DisplaySession`] (restore marker written before every change), and
//! the audio options come from the settings. The changes are held across back-to-back playbacks with the same needs
//! (no HDR flicker between episodes) and restored by [`Output::release`] once the player has been idle for a grace
//! period, at app exit, or when the next content needs something else. When the window moves to another monitor,
//! [`Output::monitor_moved`] restores the old monitor and applies the policy to the new one.
//!
//! All methods block (DisplayConfig calls, HDR read-back up to [`Timing::hdr_timeout`]); callers run them off the
//! async executor (see `run_blocking`).
use super::rendering;
use crate::settings::{AudioSettings, DisplaySettings, Settings};
use granary_display::DisplayController;
use granary_display::policy::{Content, DynamicRange, HdrDecision, HdrMode, MonitorPrefs, RefreshStrategy};
use granary_display::session::{DisplaySession, Policy, Timing};
use granary_silo_api::playback::PlaybackSource;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

/// The thread-safe display controller the app uses.
pub type Ctrl = Arc<dyn DisplayController + Send + Sync>;
/// GDI name (`\\.\DISPLAY1`) of the monitor that holds the player window now.
pub type MonitorOf = Box<dyn Fn() -> Option<String> + Send + Sync>;
/// Current settings snapshot.
pub type SettingsOf = Box<dyn Fn() -> Settings + Send + Sync>;

/// What one playback was prepared with (for reuse, monitor moves and the stats overlay).
#[derive(Debug, Clone)]
struct Active {
    gdi: String,
    content: Content,
    /// Settings-derived policy this was decided with: a change re-decides (phase-7 review M5).
    policy: Policy,
    /// Granary switched HDR on for this (held) output.
    hdr_switched: bool,
    /// Display-side mpv options of this monitor (audio options are added on every call).
    /// The swapchain is HDR for this playback.
    hdr_output: bool,
    /// Height of the monitor in physical pixels.
    monitor_height: Option<i32>,
    display_options: Vec<(String, String)>,
    summary: Vec<(String, String)>,
}

pub struct Output {
    ctrl: Option<Ctrl>,
    monitor: MonitorOf,
    settings: SettingsOf,
    render: rendering::Env,
    inner: Mutex<Inner>,
}

struct Inner {
    session: DisplaySession,
    active: Option<Active>,
    /// The title playing now (rendering options are decided again on settings changes and monitor moves).
    video: Option<PlaybackSource>,
    render_summary: Vec<(String, String)>,
    /// Rendering options set for the title playing now.
    render_options: Vec<(String, String)>,
}

/// The content facts the display policy needs, from the negotiated plan.
pub fn content_of(source: &PlaybackSource, audio: &AudioSettings) -> Content {
    let range = source
        .dynamic_range
        .as_deref()
        .and_then(DynamicRange::from_server)
        .or_else(|| source.dolby_vision_profile.map(|_| DynamicRange::DolbyVision))
        .unwrap_or(DynamicRange::Sdr);
    let passthrough = audio.passthrough
        && source.audio_codec.as_deref().is_some_and(|c| {
            let c = c.to_ascii_lowercase();
            audio.passthrough_codecs.iter().any(|p| match p.as_str() {
                // mpv's `dts-hd` also passes core DTS; the server names DTS-HD MA as `dts` with a profile.
                "dts-hd" => c == "dts" || c == "dca",
                "dts" => c == "dts" || c == "dca",
                other => other == c,
            })
        });
    Content {
        range,
        fps: source.frame_rate.filter(|f| f.is_finite() && *f > 1.0 && *f < 500.0),
        passthrough,
    }
}

/// Display policy from the settings for the monitor at `monitor_path`.
pub fn policy_of(display: &DisplaySettings, monitor_path: Option<&str>) -> Policy {
    let hdr = match display.hdr_mode.as_str() {
        "off" => HdrMode::Off,
        "always" => HdrMode::Always,
        _ => HdrMode::ContentOnly,
    };
    let strategy = |s: &str| match s {
        "match" => Some(RefreshStrategy::MatchContent),
        "never" => Some(RefreshStrategy::Never),
        "auto" => Some(RefreshStrategy::Auto),
        _ => None,
    };
    let prefs = monitor_path
        .and_then(|p| display.monitors.get(p))
        .map(|m| MonitorPrefs {
            vrr: m.vrr,
            peak_nits: m.peak_nits.map(|p| p as f32),
            black_nits: m.black_nits.map(|b| b as f32),
            refresh: strategy(&m.refresh),
        })
        .unwrap_or_default();
    Policy {
        hdr,
        refresh: strategy(&display.refresh).unwrap_or_default(),
        prefs,
    }
}

/// mpv audio options (R4 §7): WASAPI shared by default, `auto` device follows the Windows default, stereo downmix
/// with normalisation by default; exclusive mode, passthrough codecs and multichannel layouts are opt-in.
pub fn audio_options(a: &AudioSettings) -> Vec<(String, String)> {
    let channels = match a.channels.as_str() {
        "auto" => "auto-safe",
        "5.1" => "5.1,stereo",
        "7.1" => "7.1,5.1,stereo",
        _ => "stereo",
    };
    let spdif = if a.passthrough {
        a.passthrough_codecs.join(",")
    } else {
        String::new()
    };
    let flag = |b: bool| if b { "yes" } else { "no" }.to_owned();
    vec![
        ("audio-device".into(), a.device.clone()),
        ("audio-exclusive".into(), flag(a.exclusive)),
        ("audio-spdif".into(), spdif),
        ("audio-channels".into(), channels.into()),
        ("audio-normalize-downmix".into(), flag(a.normalize_downmix)),
        ("audio-fallback-to-null".into(), "yes".into()),
    ]
}

/// mpv options that undo any per-monitor display options (no display information available).
fn neutral_display_options() -> Vec<(String, String)> {
    [
        ("target-peak", "auto"),
        ("target-contrast", "auto"),
        ("hdr-reference-white", "auto"),
        ("video-sync", "display-resample"),
        ("video-sync-max-factor", "5"),
        ("display-fps-override", "0"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

impl Output {
    /// `ctrl = None` (non-Windows, or tests without displays) only sets audio and neutral display options.
    pub fn new(ctrl: Option<Ctrl>, marker: PathBuf, timing: Timing, monitor: MonitorOf, settings: SettingsOf) -> Self {
        Self {
            ctrl,
            monitor,
            settings,
            render: rendering::Env::default(),
            inner: Mutex::new(Inner {
                session: DisplaySession::new(marker, timing),
                active: None,
                video: None,
                render_summary: Vec::new(),
                render_options: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Decide and apply the display policy for this plan; returns every mpv option to set before loading.
    pub fn prepare(&self, source: &PlaybackSource) -> Vec<(String, String)> {
        let settings = (self.settings)();
        let content = content_of(source, &settings.audio);
        let mut inner = self.lock();
        let display = self.apply(&mut inner, &settings, content, None);
        inner.video = Some(source.clone());
        let rendering = self.rendering(&mut inner, &settings, true);
        display
            .into_iter()
            .chain(audio_options(&settings.audio))
            .chain(rendering)
            .collect()
    }

    /// The window moved: if it is on another monitor now, restore the old one and apply the policy to the new one.
    /// Returns the options to set, or `None` when nothing changed.
    pub fn monitor_moved(&self) -> Option<Vec<(String, String)>> {
        let mut inner = self.lock();
        let active = inner.active.clone()?;
        let now = (self.monitor)()?;
        if now.eq_ignore_ascii_case(&active.gdi) {
            return None;
        }
        tracing::info!(from = %active.gdi, to = %now, "player window moved to another monitor");
        let settings = (self.settings)();
        let mut options = self.apply(&mut inner, &settings, active.content, Some(now));
        options.extend(self.rendering(&mut inner, &settings, false));
        Some(options)
    }

    /// Restore everything changed for playback (idle grace elapsed, app exit).
    pub fn release(&self) {
        let mut inner = self.lock();
        Self::restore(&mut inner, self.ctrl.as_deref());
        inner.active = None;
        inner.video = None;
        inner.render_summary.clear();
        inner.render_options.clear();
    }

    /// Audio options from the current settings (applied live when settings change during playback).
    pub fn audio_now(&self) -> Vec<(String, String)> {
        audio_options(&(self.settings)().audio)
    }

    /// Settings were saved during playback: re-decide for the active monitor and return the display options to set
    /// live (`None` when nothing plays). Tone-mapping overrides apply without touching the display; a changed HDR
    /// mode, refresh strategy or VRR flag restores and re-applies at once (phase-7 review M5).
    pub fn settings_changed(&self) -> Option<Vec<(String, String)>> {
        let mut inner = self.lock();
        let active = inner.active.clone()?;
        let settings = (self.settings)();
        let mut options = self.apply(&mut inner, &settings, active.content, Some(active.gdi));
        options.extend(self.rendering(&mut inner, &settings, false));
        Some(options)
    }

    /// Lines for the stats overlay.
    pub fn summary(&self) -> Vec<(String, String)> {
        let inner = self.lock();
        let mut lines = inner.active.as_ref().map(|a| a.summary.clone()).unwrap_or_default();
        lines.extend(inner.render_summary.iter().cloned());
        lines
    }

    /// GPU and configuration facts for the rendering options (ADR-0012).
    #[must_use]
    pub fn with_render_env(mut self, env: rendering::Env) -> Self {
        self.render = env;
        self
    }

    /// Rendering options for the title playing now (none before the first title): all of them for a new title, the
    /// changed ones otherwise (setting `vf` again re-creates the filters).
    fn rendering(&self, inner: &mut Inner, settings: &Settings, all: bool) -> Vec<(String, String)> {
        let Some(source) = inner.video.as_ref() else {
            return Vec::new();
        };
        let target = rendering::Target {
            monitor_height: inner.active.as_ref().and_then(|a| a.monitor_height),
            hdr_output: inner.active.as_ref().is_some_and(|a| a.hdr_output),
        };
        let d = rendering::decide_now(&settings.player, &self.render, source, target);
        tracing::info!(summary = ?d.summary, "rendering options decided");
        inner.render_summary = d.summary;
        let changed = d
            .options
            .iter()
            .filter(|o| all || !inner.render_options.contains(o))
            .cloned()
            .collect();
        inner.render_options = d.options;
        changed
    }

    /// Something was changed and not restored yet.
    pub fn holding(&self) -> bool {
        !self.lock().session.changed().is_empty()
    }

    fn restore(inner: &mut Inner, ctrl: Option<&(dyn DisplayController + Send + Sync)>) {
        let Some(ctrl) = ctrl else { return };
        if let Some(report) = inner.session.restore(ctrl) {
            if report.all_ok() {
                tracing::info!(outcomes = ?report.outcomes, "display state restored");
            } else {
                tracing::error!(outcomes = ?report.outcomes, "display state not fully restored; marker kept");
            }
        }
    }

    fn apply(
        &self,
        inner: &mut Inner,
        settings: &Settings,
        content: Content,
        gdi: Option<String>,
    ) -> Vec<(String, String)> {
        let Some(ctrl) = self.ctrl.as_deref() else {
            return neutral_display_options();
        };
        let Some(gdi) = gdi.or_else(|| (self.monitor)()) else {
            tracing::warn!("no monitor for the player window; display policy skipped");
            Self::restore(inner, Some(ctrl));
            inner.active = None;
            return neutral_display_options();
        };
        let monitor_path = ctrl
            .displays()
            .ok()
            .and_then(|ds| ds.into_iter().find(|d| d.gdi_name.eq_ignore_ascii_case(&gdi)))
            .map(|d| d.monitor_path);
        let policy = policy_of(&settings.display, monitor_path.as_deref());
        let same_needs = inner
            .active
            .as_ref()
            .is_some_and(|a| a.gdi.eq_ignore_ascii_case(&gdi) && a.content == content);
        if same_needs && let Some(a) = inner.active.as_ref().filter(|a| a.policy == policy) {
            tracing::debug!("display output reused from the previous playback");
            return a.display_options.clone();
        }
        // Same monitor and content, only the tone-mapping overrides changed: re-decide on the display as it is now
        // (still switched), without a restore/re-switch flicker. The session keeps the original state recorded.
        let retune = same_needs
            && inner.active.as_ref().is_some_and(|a| {
                a.policy.hdr == policy.hdr
                    && a.policy.refresh == policy.refresh
                    && a.policy.prefs.vrr == policy.prefs.vrr
                    && a.policy.prefs.refresh == policy.prefs.refresh
            });
        if !retune {
            // Different needs (or another monitor): undo the previous changes first, so restore always returns to the
            // state before Granary touched anything.
            Self::restore(inner, Some(ctrl));
            inner.active = None;
        }
        let held = inner.active.take();
        match inner.session.prepare(ctrl, &gdi, &content, &policy) {
            Ok(a) => {
                tracing::info!(
                    monitor = %a.display.friendly_name,
                    gdi = %gdi,
                    range = ?content.range,
                    fps = ?content.fps,
                    hdr = ?a.hdr,
                    hdr_switched = a.hdr_switched,
                    hdr_output = a.hdr_output,
                    refresh_switched = a.refresh_switched,
                    refresh_hz = a.display.refresh.hz(),
                    reason = ?a.plan.reason,
                    options = ?a.options,
                    notes = ?a.notes,
                    "display output decided"
                );
                let switched_earlier = held.as_ref().is_some_and(|h| h.hdr_switched);
                let hdr_switched = a.hdr_switched || (switched_earlier && a.hdr == HdrDecision::AlreadyOn);
                let hdr = match a.hdr {
                    _ if hdr_switched => "HDR (switched on)".to_owned(),
                    HdrDecision::AlreadyOn => "HDR (already on)".to_owned(),
                    HdrDecision::Enable => "HDR (switched on)".to_owned(),
                    HdrDecision::StaySdr(r) => format!("SDR ({r:?})"),
                };
                let peak = a
                    .options
                    .iter()
                    .find(|(k, _)| k == "target-peak")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                let summary = vec![
                    ("output-monitor".into(), format!("{} ({gdi})", a.display.friendly_name)),
                    ("output-dynamic-range".into(), hdr),
                    ("output-target-peak".into(), peak),
                    (
                        "output-refresh".into(),
                        format!(
                            "{:.3} Hz{} · {:?} · video-sync {}",
                            a.display.refresh.hz(),
                            if a.refresh_switched { " (switched)" } else { "" },
                            a.plan.reason,
                            a.plan.video_sync
                        ),
                    ),
                ];
                inner.active = Some(Active {
                    gdi,
                    content,
                    policy,
                    hdr_switched,
                    hdr_output: a.hdr_output,
                    monitor_height: Some(a.display.desktop_rect.3 - a.display.desktop_rect.1).filter(|h| *h > 0),
                    display_options: a.options.clone(),
                    summary,
                });
                a.options
            }
            Err(e) => {
                tracing::warn!(error = %e, gdi = %gdi, "display policy could not be applied; playing with defaults");
                neutral_display_options()
            }
        }
    }
}

/// Run blocking display work without stalling the async executor (multi-threaded runtime: `block_in_place`; a
/// current-thread runtime, as in tests, runs it inline).
pub fn run_blocking<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(f),
        _ => f(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;
    use crate::settings::MonitorSettings;
    use granary_display::Rational;
    use granary_display::mock::MockDisplayController;
    use std::time::Duration;

    const GDI: &str = r"\\.\DISPLAY1";
    const MOCK_PATH: &str = r"\\?\DISPLAY#MOCK0001#1";

    fn source(range: &str, fps: f64) -> PlaybackSource {
        PlaybackSource {
            dynamic_range: Some(range.into()),
            frame_rate: Some(fps),
            audio_codec: Some("eac3".into()),
            ..PlaybackSource::default()
        }
    }

    fn rig(
        name: &str,
        hdr_on: bool,
        refresh: Rational,
        settings: Settings,
    ) -> (Arc<MockDisplayController>, Output, PathBuf) {
        let (m, out, marker, _) = rig_shared(name, hdr_on, refresh, settings);
        (m, out, marker)
    }

    /// Like `rig`, with settings the test can change between calls.
    fn rig_shared(
        name: &str,
        hdr_on: bool,
        refresh: Rational,
        settings: Settings,
    ) -> (Arc<MockDisplayController>, Output, PathBuf, Arc<Mutex<Settings>>) {
        let shared = Arc::new(Mutex::new(settings));
        let read = shared.clone();
        let m = Arc::new(MockDisplayController::single(hdr_on, refresh));
        let dir = std::env::temp_dir().join(format!("granary-output-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join(granary_display::marker::FILE_NAME);
        let out = Output::new(
            Some(m.clone() as Ctrl),
            marker.clone(),
            Timing {
                hdr_timeout: Duration::ZERO,
                settle: Duration::ZERO,
            },
            Box::new(|| Some(GDI.to_owned())),
            Box::new(move || read.lock().unwrap().clone()),
        );
        (m, out, marker, shared)
    }

    fn get<'a>(o: &'a [(String, String)], k: &str) -> Option<&'a str> {
        o.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn hdr_content_switches_hdr_on_and_release_restores_it() {
        let (m, out, marker) = rig("hdr", false, Rational::new(60, 1), Settings::default());
        let o = out.prepare(&source("hdr10", 23.976));
        assert!(m.display(&m.id()).unwrap().hdr_enabled);
        assert!(marker.exists(), "marker written while the change is held");
        assert_eq!(get(&o, "target-peak"), Some("1000"));
        assert_eq!(get(&o, "audio-channels"), Some("stereo"));
        assert_eq!(get(&o, "audio-normalize-downmix"), Some("yes"));
        assert_eq!(get(&o, "audio-exclusive"), Some("no"));
        assert_eq!(get(&o, "audio-spdif"), Some(""));
        assert!(out.holding());
        assert!(
            out.summary()
                .iter()
                .any(|(k, v)| k == "output-dynamic-range" && v.contains("switched on"))
        );
        out.release();
        assert!(!m.display(&m.id()).unwrap().hdr_enabled);
        assert!(!marker.exists());
        assert!(!out.holding());
    }

    #[test]
    fn next_episode_with_the_same_needs_reuses_the_output() {
        let (m, out, _) = rig("reuse", false, Rational::new(60, 1), Settings::default());
        out.prepare(&source("hdr10", 24.0));
        // A failing HDR switch would show up if prepare toggled again.
        m.fail_set_hdr(true);
        let o = out.prepare(&source("hdr10", 24.0));
        assert_eq!(get(&o, "target-peak"), Some("1000"));
        assert!(m.display(&m.id()).unwrap().hdr_enabled);
        m.fail_set_hdr(false);
        out.release();
        assert!(!m.display(&m.id()).unwrap().hdr_enabled);
    }

    #[test]
    fn tone_mapping_change_during_playback_retunes_without_a_restore() {
        let (m, out, marker, settings) = rig_shared("retune", false, Rational::new(60, 1), Settings::default());
        out.prepare(&source("hdr10", 24.0));
        assert!(m.display(&m.id()).unwrap().hdr_enabled);
        settings.lock().unwrap().display.monitors.insert(
            MOCK_PATH.into(),
            MonitorSettings {
                peak_nits: Some(650.0),
                ..MonitorSettings::default()
            },
        );
        // A restore/re-switch would hit the failing HDR switch.
        m.fail_set_hdr(true);
        let o = out.settings_changed().unwrap();
        assert_eq!(get(&o, "target-peak"), Some("650"), "new override applied live");
        assert!(m.display(&m.id()).unwrap().hdr_enabled, "display left as it was");
        assert!(marker.exists() && out.holding());
        assert!(
            out.summary()
                .iter()
                .any(|(k, v)| k == "output-dynamic-range" && v.contains("switched on")),
            "still reported as switched by Granary"
        );
        m.fail_set_hdr(false);
        out.release();
        assert!(
            !m.display(&m.id()).unwrap().hdr_enabled,
            "the original state is restored"
        );
        assert!(!marker.exists());
    }

    #[test]
    fn hdr_mode_change_during_playback_re_decides_at_once() {
        let (m, out, marker, settings) = rig_shared("mode", false, Rational::new(60, 1), Settings::default());
        out.prepare(&source("hdr10", 24.0));
        assert!(m.display(&m.id()).unwrap().hdr_enabled);
        settings.lock().unwrap().display.hdr_mode = "off".into();
        let o = out.settings_changed().unwrap();
        assert!(
            !m.display(&m.id()).unwrap().hdr_enabled,
            "restored when HDR switching is turned off"
        );
        assert_eq!(get(&o, "target-peak"), Some("auto"), "tone-mapped to SDR");
        assert!(!marker.exists() && !out.holding());
        out.release();
        assert!(out.settings_changed().is_none(), "nothing plays after release");
    }

    #[test]
    fn transcode_of_an_hdr_source_stays_sdr() {
        // Phase-7 review M1: upstream tone-maps every transcode to SDR (`plan_v3.go:1458`@6791aac).
        let (m, out, marker) = rig("transcode", false, Rational::new(60, 1), Settings::default());
        let plan = granary_silo_api::playback::PlaybackPlan {
            delivery: "server_transcode_hls".into(),
            source: source("hdr10", 23.976),
            effective_recipe: Some(granary_silo_api::playback::EffectiveRecipe {
                dynamic_range: Some("sdr".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let o = out.prepare(&plan.delivered());
        assert!(!m.display(&m.id()).unwrap().hdr_enabled, "no pointless HDR switch");
        assert_eq!(get(&o, "target-peak"), Some("auto"));
        assert!(!marker.exists());
    }

    #[test]
    fn sdr_after_hdr_restores_before_playing() {
        let (m, out, marker) = rig("sdr-after", false, Rational::new(60, 1), Settings::default());
        out.prepare(&source("hdr10", 24.0));
        let o = out.prepare(&source("sdr", 24.0));
        assert!(
            !m.display(&m.id()).unwrap().hdr_enabled,
            "content-only mode: SDR content plays with HDR off"
        );
        assert_eq!(get(&o, "target-peak"), Some("auto"));
        assert!(!marker.exists());
    }

    #[test]
    fn hdr_already_on_is_never_toggled() {
        let (m, out, marker) = rig("on", true, Rational::new(240, 1), Settings::default());
        m.fail_set_hdr(true);
        let o = out.prepare(&source("dolby_vision", 23.976));
        assert_eq!(get(&o, "hdr-reference-white"), Some("200"));
        assert!(!out.holding() && !marker.exists());
        out.release();
        assert!(m.display(&m.id()).unwrap().hdr_enabled);
    }

    #[test]
    fn per_monitor_overrides_and_match_content() {
        let mut s = Settings::default();
        s.display.refresh = "never".into();
        s.display.monitors.insert(
            MOCK_PATH.into(),
            MonitorSettings {
                peak_nits: Some(650.0),
                black_nits: Some(0.0),
                refresh: "match".into(),
                ..MonitorSettings::default()
            },
        );
        let (m, out, _) = rig("prefs", true, Rational::new(60, 1), s);
        m.set_modes(vec![Rational::new(24_000, 1001), Rational::new(60, 1)]);
        let monitor_path = m.display(&m.id()).unwrap().monitor_path;
        assert_eq!(monitor_path, MOCK_PATH, "monitor path is the settings key");
        let o = out.prepare(&source("hdr10", 24_000.0 / 1001.0));
        assert_eq!(get(&o, "target-peak"), Some("650"));
        assert_eq!(get(&o, "target-contrast"), Some("inf"));
        assert_eq!(m.display(&m.id()).unwrap().refresh, Rational::new(24_000, 1001));
        out.release();
        assert_eq!(m.display(&m.id()).unwrap().refresh, Rational::new(60, 1));
    }

    #[test]
    fn passthrough_audio_uses_audio_sync_and_spdif() {
        let mut s = Settings::default();
        s.audio.passthrough = true;
        s.audio.passthrough_codecs = vec!["ac3".into(), "eac3".into(), "truehd".into()];
        s.audio.exclusive = true;
        s.audio.channels = "7.1".into();
        let c = content_of(&source("sdr", 24.0), &s.audio);
        assert!(c.passthrough);
        let a = audio_options(&s.audio);
        assert_eq!(get(&a, "audio-spdif"), Some("ac3,eac3,truehd"));
        assert_eq!(get(&a, "audio-exclusive"), Some("yes"));
        assert_eq!(get(&a, "audio-channels"), Some("7.1,5.1,stereo"));
        let mut aac = source("sdr", 24.0);
        aac.audio_codec = Some("aac".into());
        assert!(!content_of(&aac, &s.audio).passthrough);
    }

    #[test]
    fn content_facts_from_the_plan() {
        let a = AudioSettings::default();
        let mut dv = PlaybackSource {
            dolby_vision_profile: Some(5),
            ..PlaybackSource::default()
        };
        assert_eq!(content_of(&dv, &a).range, DynamicRange::DolbyVision);
        dv.dynamic_range = Some("hlg".into());
        assert_eq!(content_of(&dv, &a).range, DynamicRange::Hlg);
        assert_eq!(content_of(&PlaybackSource::default(), &a).range, DynamicRange::Sdr);
        assert_eq!(content_of(&source("sdr", f64::NAN), &a).fps, None);
    }

    #[test]
    fn moving_to_another_monitor_restores_the_first() {
        let (m, out, _) = rig("move", false, Rational::new(60, 1), Settings::default());
        out.prepare(&source("hdr10", 24.0));
        assert!(out.monitor_moved().is_none(), "same monitor: nothing to do");
        // The monitor function always answers DISPLAY1, so simulate the move by asking for a monitor that does not
        // exist: the old changes are restored and defaults are used.
        let out2 = Output {
            monitor: Box::new(|| Some(r"\\.\DISPLAY9".to_owned())),
            ..out
        };
        let o = out2.monitor_moved().unwrap();
        assert_eq!(get(&o, "target-peak"), Some("auto"));
        assert!(!m.display(&m.id()).unwrap().hdr_enabled, "first monitor restored");
    }

    #[test]
    fn without_a_display_controller_only_neutral_and_audio_options() {
        let out = Output::new(
            None,
            std::env::temp_dir().join("granary-output-none.json"),
            Timing::default(),
            Box::new(|| None),
            Box::new(Settings::default),
        );
        let o = out.prepare(&source("hdr10", 24.0));
        assert_eq!(get(&o, "display-fps-override"), Some("0"));
        assert_eq!(get(&o, "audio-device"), Some("auto"));
        out.release();
    }

    #[test]
    fn rendering_options_follow_the_title_and_live_changes_send_only_differences() {
        let mut s = Settings::default();
        s.player.render_quality = "fast".into();
        let (_m, out, _marker, settings) = rig_shared("render", false, Rational::new(60, 1), s);
        let o = out.prepare(&source("sdr", 24.0));
        assert_eq!(get(&o, "deband"), Some("no"));
        assert_eq!(get(&o, "scale"), Some("bilinear"));
        assert!(out.summary().iter().any(|(k, v)| k == "render-quality" && v == "fast"));
        // Saving unrelated settings re-sends no rendering option (a new `vf` would re-create the filters).
        let o = out.settings_changed().unwrap();
        assert_eq!(get(&o, "deband"), None);
        assert_eq!(get(&o, "vf"), None);
        settings.lock().unwrap().player.render_quality = "max".into();
        let o = out.settings_changed().unwrap();
        assert_eq!(get(&o, "scale"), Some("ewa_lanczossharp"));
        assert_eq!(get(&o, "deband"), Some("yes"));
        out.release();
        assert!(!out.summary().iter().any(|(k, _)| k == "render-quality"));
    }
}
