//! User settings (`%APPDATA%\Granary\settings.json`). Every field has a default, unknown fields are dropped and values
//! are clamped/normalised on load and save, so a hand-edited or older file can never put the player into an invalid
//! state. Defaults target the primary setup (OLED desktop HDR monitor); TV/receiver features are opt-in.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `system`, `en` or `de`.
    pub language: String,
    /// `midnight-cinema` or `true-black`.
    pub theme: String,
    pub player: PlayerSettings,
    pub display: DisplaySettings,
    pub audio: AudioSettings,
    pub subtitles: SubtitleSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayerSettings {
    /// mpv `hwdec`: `auto-safe`, `auto-copy-safe` or `no`.
    pub hwdec: String,
    pub autoplay_next: bool,
    /// Seconds per arrow-key seek (1..=120).
    pub seek_step: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DisplaySettings {
    /// Windows HDR switching: `off`, `content` (HDR content only, default) or `always` (ADR-0006).
    pub hdr_mode: String,
    /// Refresh strategy: `auto` (VRR → fits → exact multiple → resample, default), `match` (match content exactly,
    /// TVs) or `never`.
    pub refresh: String,
    /// Per-monitor preferences keyed by the reboot-stable monitor device path.
    pub monitors: BTreeMap<String, MonitorSettings>,
}

/// Preferences for one monitor (Phase 7). Missing values use DXGI and the global strategy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MonitorSettings {
    /// Friendly name when the entry was saved (shown in the UI; not used for matching).
    pub name: String,
    /// The monitor runs VRR (G-SYNC/FreeSync) for video: never switch its refresh (no Windows API reports VRR).
    pub vrr: bool,
    /// Peak luminance override in nits (80..=10000), e.g. from the HDR Calibration app.
    pub peak_nits: Option<f64>,
    /// Black level override in nits (0..=5); 0 = perfect black (OLED).
    pub black_nits: Option<f64>,
    /// `default` (global strategy), `auto`, `match` or `never`.
    pub refresh: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    pub passthrough: bool,
    /// Subset of `ac3`, `eac3`, `dts`, `dts-hd`, `truehd`.
    pub passthrough_codecs: Vec<String>,
    /// `auto` (device layout), `stereo` (downmix, default), `5.1` or `7.1`.
    pub channels: String,
    /// WASAPI exclusive mode (opt-in).
    pub exclusive: bool,
    /// mpv `audio-device` name; `auto` follows the Windows default device.
    pub device: String,
    /// Normalise the level of downmixes (5.1/7.1 → stereo) so they do not clip.
    pub normalize_downmix: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubtitleSettings {
    /// mpv `sub-scale` (0.5..=2.0).
    pub scale: f64,
    pub prefer_sdh: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            theme: "midnight-cinema".into(),
            player: PlayerSettings::default(),
            display: DisplaySettings::default(),
            audio: AudioSettings::default(),
            subtitles: SubtitleSettings::default(),
        }
    }
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            hwdec: "auto-safe".into(),
            autoplay_next: true,
            seek_step: 10,
        }
    }
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            hdr_mode: "content".into(),
            refresh: "auto".into(),
            monitors: BTreeMap::new(),
        }
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            passthrough: false,
            passthrough_codecs: vec!["ac3".into(), "eac3".into(), "dts".into()],
            channels: "stereo".into(),
            exclusive: false,
            device: "auto".into(),
            normalize_downmix: true,
        }
    }
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            scale: 1.0,
            prefer_sdh: false,
        }
    }
}

/// Per-monitor entries kept at most (keys beyond this, by sort order, are dropped).
const MAX_MONITORS: usize = 32;

fn one_of(value: &mut String, allowed: &[&str], fallback: &str) {
    if !allowed.contains(&value.as_str()) {
        *value = fallback.to_owned();
    }
}

impl Settings {
    /// Clamp every value into its valid range; invalid enum strings fall back to the default.
    pub fn normalized(mut self) -> Self {
        one_of(&mut self.language, &["system", "en", "de"], "system");
        one_of(&mut self.theme, &["midnight-cinema", "true-black"], "midnight-cinema");
        one_of(
            &mut self.player.hwdec,
            &["auto-safe", "auto-copy-safe", "no"],
            "auto-safe",
        );
        self.player.seek_step = self.player.seek_step.clamp(1, 120);
        one_of(&mut self.display.hdr_mode, &["off", "content", "always"], "content");
        one_of(&mut self.display.refresh, &["auto", "match", "never"], "auto");
        let valid_key = |k: &String| !k.is_empty() && k.len() <= 512 && !k.chars().any(char::is_control);
        self.display.monitors.retain(|k, _| valid_key(k));
        while self.display.monitors.len() > MAX_MONITORS {
            self.display.monitors.pop_last();
        }
        for m in self.display.monitors.values_mut() {
            m.name = m.name.chars().filter(|c| !c.is_control()).take(128).collect();
            m.peak_nits = m
                .peak_nits
                .filter(|p| p.is_finite() && (80.0..=10_000.0).contains(p))
                .map(f64::round);
            m.black_nits = m
                .black_nits
                .filter(|b| b.is_finite() && (0.0..=5.0).contains(b))
                .map(|b| (b * 10_000.0).round() / 10_000.0);
            one_of(&mut m.refresh, &["default", "auto", "match", "never"], "default");
        }
        one_of(&mut self.audio.channels, &["auto", "stereo", "5.1", "7.1"], "stereo");
        if self.audio.device.is_empty()
            || self.audio.device.len() > 512
            || self.audio.device.chars().any(char::is_control)
        {
            self.audio.device = "auto".into();
        }
        let codecs = ["ac3", "eac3", "dts", "dts-hd", "truehd"];
        let mut picked: Vec<String> = codecs
            .iter()
            .filter(|c| self.audio.passthrough_codecs.iter().any(|p| p == *c))
            .map(|c| (*c).to_owned())
            .collect();
        picked.dedup();
        self.audio.passthrough_codecs = picked;
        self.subtitles.scale = if self.subtitles.scale.is_finite() {
            (self.subtitles.scale.clamp(0.5, 2.0) * 10.0).round() / 10.0
        } else {
            1.0
        };
        self
    }
}

/// Settings file plus the in-memory copy commands read.
pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    /// Load, falling back to defaults for a missing file; a corrupt file is kept as `settings.json.bad`.
    pub fn load(path: PathBuf) -> Self {
        let current = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s.normalized(),
                Err(e) => {
                    tracing::warn!(error = %e, "settings.json unreadable; using defaults");
                    let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        };
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub async fn get(&self) -> Settings {
        self.current()
    }

    /// Synchronous snapshot (the player host reads it from blocking display code).
    pub fn current(&self) -> Settings {
        match self.current.lock() {
            Ok(s) => s.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub async fn set(&self, next: Settings) -> std::io::Result<Settings> {
        let next = next.normalized();
        let mut cur = match self.current.lock() {
            Ok(c) => c,
            Err(poisoned) => poisoned.into_inner(),
        };
        write_atomic(&self.path, &next)?;
        *cur = next.clone();
        Ok(next)
    }
}

fn write_atomic(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(s).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[tauri::command]
pub async fn settings_get(store: tauri::State<'_, SettingsStore>) -> Result<Settings, crate::silo::CommandError> {
    Ok(store.get().await)
}

#[tauri::command]
pub async fn settings_set(
    store: tauri::State<'_, SettingsStore>,
    host: tauri::State<'_, crate::playback::host::PlayerHost>,
    settings: Settings,
) -> Result<Settings, crate::silo::CommandError> {
    let saved = store.set(settings).await.map_err(|e| {
        tracing::error!(error = %e, "could not save settings");
        crate::silo::CommandError::storage(e.to_string())
    })?;
    // Audio options apply live where mpv allows; the display policy applies from the next playback.
    host.send(crate::playback::host::Cmd::SettingsChanged);
    Ok(saved)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_normalisation() {
        let s: Settings = serde_json::from_str(
            r#"{"language":"fr","theme":"true-black","player":{"seekStep":900,"hwdec":"vulkan"},
                "audio":{"passthroughCodecs":["truehd","bogus","ac3","ac3"],"channels":"9.2"},
                "subtitles":{"scale":7.77},"unknownFutureSetting":1}"#,
        )
        .unwrap();
        let s = s.normalized();
        assert_eq!(s.language, "system");
        assert_eq!(s.theme, "true-black");
        assert_eq!((s.player.hwdec.as_str(), s.player.seek_step), ("auto-safe", 120));
        assert!(s.player.autoplay_next, "missing nested fields keep their defaults");
        assert_eq!(s.audio.passthrough_codecs, vec!["ac3", "truehd"]);
        assert_eq!(s.audio.channels, "stereo");
        assert_eq!(s.audio.device, "auto");
        assert!(s.audio.normalize_downmix);
        assert_eq!(s.display.hdr_mode, "content");
        assert!((s.subtitles.scale - 2.0).abs() < f64::EPSILON);
        assert!(
            s.display.refresh == "auto" && !s.audio.passthrough && !s.audio.exclusive,
            "OLED-desktop defaults; receiver features opt-in"
        );
    }

    #[test]
    fn monitor_preferences_are_clamped() {
        let s: Settings = serde_json::from_str(
            r#"{"display":{"hdrMode":"sometimes","refresh":"match","monitors":{
                "MON-A":{"name":"OLED","vrr":true,"peakNits":1013.4,"blackNits":0,"refresh":"x"},
                "bad":{"peakNits":20000,"blackNits":-1},"":{"vrr":true}}},"audio":{"device":""}}"#,
        )
        .unwrap();
        let s = s.normalized();
        assert_eq!(s.display.hdr_mode, "content");
        assert_eq!(s.display.refresh, "match");
        assert_eq!(s.display.monitors.len(), 2, "empty key dropped");
        let oled = &s.display.monitors["MON-A"];
        assert_eq!(
            (oled.peak_nits, oled.black_nits, oled.refresh.as_str()),
            (Some(1013.0), Some(0.0), "default")
        );
        assert!(oled.vrr);
        let bad = &s.display.monitors["bad"];
        assert_eq!((bad.peak_nits, bad.black_nits), (None, None));
        assert_eq!(s.audio.device, "auto");
    }

    #[tokio::test]
    async fn store_round_trip_and_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("granary-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);
        let store = SettingsStore::load(path.clone());
        assert_eq!(store.get().await, Settings::default());
        let s = Settings {
            theme: "true-black".into(),
            ..Settings::default()
        };
        store.set(s.clone()).await.unwrap();
        assert_eq!(SettingsStore::load(path.clone()).get().await.theme, "true-black");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(SettingsStore::load(path.clone()).get().await, Settings::default());
        assert!(path.with_extension("json.bad").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
