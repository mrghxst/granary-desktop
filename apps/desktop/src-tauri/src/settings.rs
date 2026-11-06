//! User settings (`%APPDATA%\Granary\settings.json`). Every field has a default, unknown fields are dropped and values
//! are clamped/normalised on load and save, so a hand-edited or older file can never put the player into an invalid
//! state. Defaults target the primary setup (OLED desktop HDR monitor); TV/receiver features are opt-in.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

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
    pub auto_hdr: bool,
    pub match_refresh_rate: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    pub passthrough: bool,
    /// Subset of `ac3`, `eac3`, `dts`, `dts-hd`, `truehd`.
    pub passthrough_codecs: Vec<String>,
    /// `auto`, `stereo`, `5.1` or `7.1`.
    pub channels: String,
    pub exclusive: bool,
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
            auto_hdr: true,
            match_refresh_rate: true,
        }
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            passthrough: false,
            passthrough_codecs: vec!["ac3".into(), "eac3".into(), "dts".into()],
            channels: "auto".into(),
            exclusive: false,
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
        one_of(&mut self.audio.channels, &["auto", "stereo", "5.1", "7.1"], "auto");
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
        self.current.lock().await.clone()
    }

    pub async fn set(&self, next: Settings) -> std::io::Result<Settings> {
        let next = next.normalized();
        let mut cur = self.current.lock().await;
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
    settings: Settings,
) -> Result<Settings, crate::silo::CommandError> {
    store.set(settings).await.map_err(|e| {
        tracing::error!(error = %e, "could not save settings");
        crate::silo::CommandError::storage(e.to_string())
    })
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
        assert_eq!(s.audio.channels, "auto");
        assert!((s.subtitles.scale - 2.0).abs() < f64::EPSILON);
        assert!(
            s.display.auto_hdr && !s.audio.passthrough,
            "OLED-desktop defaults; receiver features opt-in"
        );
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
