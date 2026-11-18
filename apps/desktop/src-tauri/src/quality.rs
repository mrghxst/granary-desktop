//! Streaming quality settings (ADR-0014): the profile's `playback.preferred_quality`, `playback.max_bitrate_kbps` and
//! `playback.buffering_quality_mode` as they resolve on this computer, written at `profile_device` scope, and this
//! device's connection reading with its learned limit (docs/research/silo-api.md §4.1l).
use crate::silo::{CommandError, SiloState};
use granary_silo_api::ApiError;
use granary_silo_api::playback::PlaybackConnection;
use serde::Serialize;
use tauri::State;

type CmdResult<T> = Result<T, CommandError>;

const PREFERRED_QUALITY: &str = "playback.preferred_quality";
const MAX_BITRATE: &str = "playback.max_bitrate_kbps";
const BUFFERING_MODE: &str = "playback.buffering_quality_mode";
/// The scope Granary writes: this profile on this computer, so a slow laptop link does not cap the living room.
const SCOPE: &str = "profile_device";

/// The resolved streaming-quality settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualitySettings {
    /// `auto`, `480p`, `720p`, `1080p` or `2160p` (`original` reads as `auto`).
    pub preferred_quality: String,
    /// `None`: no cap.
    pub max_bitrate_kbps: Option<i64>,
    /// `never`, `ask` or `always`; `None` when this server's settings contract does not define the key (the
    /// controller then runs `always` and the row is hidden).
    pub buffering_mode: Option<String>,
    /// Which of the three values this computer overrides (`profile_device` won).
    pub device_overrides: Vec<String>,
}

fn quality_value(v: &serde_json::Value) -> String {
    match v.as_str() {
        Some(q @ ("480p" | "720p" | "1080p" | "2160p")) => q.to_owned(),
        _ => "auto".to_owned(),
    }
}

fn mode_value(v: &serde_json::Value) -> String {
    match v.as_str() {
        Some(m @ ("never" | "ask")) => m.to_owned(),
        _ => "always".to_owned(),
    }
}

fn is_unknown_key(e: &ApiError) -> bool {
    e.status() == Some(422)
}

/// The three settings as they resolve for the selected profile on this computer.
#[tauri::command]
pub async fn quality_settings(silo: State<'_, SiloState>, server_id: String) -> CmdResult<QualitySettings> {
    silo.with_session(&server_id, |s| async move {
        let (items, has_mode) = match s
            .settings_effective(&[PREFERRED_QUALITY, MAX_BITRATE, BUFFERING_MODE])
            .await
        {
            Ok(c) => (c.items, true),
            // A server before settings revision 23 does not define the buffering mode (a 422 for the whole read).
            Err(e) if is_unknown_key(&e) => (
                s.settings_effective(&[PREFERRED_QUALITY, MAX_BITRATE]).await?.items,
                false,
            ),
            Err(e) => return Err(e),
        };
        let mut out = QualitySettings {
            preferred_quality: "auto".into(),
            buffering_mode: has_mode.then(|| "always".to_owned()),
            ..QualitySettings::default()
        };
        for item in items {
            match item.key.as_str() {
                PREFERRED_QUALITY => out.preferred_quality = quality_value(&item.value),
                MAX_BITRATE => out.max_bitrate_kbps = item.value.as_i64().filter(|k| (100..=200_000).contains(k)),
                BUFFERING_MODE => out.buffering_mode = Some(mode_value(&item.value)),
                _ => continue,
            }
            if item.source == SCOPE {
                out.device_overrides.push(item.key);
            }
        }
        Ok(out)
    })
    .await
}

/// Whether `value` is one Granary writes for `key` (`contracts/settings/v1/manifest.json` @749db96a).
fn valid(key: &str, value: &serde_json::Value) -> bool {
    match key {
        PREFERRED_QUALITY => value
            .as_str()
            .is_some_and(|v| ["auto", "480p", "720p", "1080p", "2160p"].contains(&v)),
        MAX_BITRATE => value.is_null() || value.as_i64().is_some_and(|k| (100..=200_000).contains(&k)),
        BUFFERING_MODE => value.as_str().is_some_and(|v| ["never", "ask", "always"].contains(&v)),
        _ => false,
    }
}

/// Store one of the three for this profile on this computer (`null` is a value: "no cap" for the bitrate); `clear`
/// removes this computer's value instead, so the profile's (or the default) applies again.
#[tauri::command]
pub async fn quality_setting_set(
    silo: State<'_, SiloState>,
    server_id: String,
    key: String,
    value: Option<serde_json::Value>,
    clear: Option<bool>,
) -> CmdResult<QualitySettings> {
    let clear = clear.unwrap_or(false);
    let value = value.unwrap_or(serde_json::Value::Null);
    if !clear && !valid(&key, &value) {
        return Err(CommandError::invalid("quality setting".into()));
    }
    if ![PREFERRED_QUALITY, MAX_BITRATE, BUFFERING_MODE].contains(&key.as_str()) {
        return Err(CommandError::invalid("quality setting".into()));
    }
    silo.with_session(&server_id, |s| async move {
        if clear {
            match s.setting_delete(&key, SCOPE).await {
                // Nothing stored on this computer: already the profile's value.
                Err(e) if e.status() == Some(404) => Ok(()),
                other => other,
            }
        } else {
            s.setting_set(&key, SCOPE, &value).await
        }
    })
    .await?;
    quality_settings(silo, server_id).await
}

/// This computer on the connection it uses now; `None` when the server has no connection state
/// (`buffering_quality_v1` missing).
#[tauri::command]
pub async fn playback_connection(
    silo: State<'_, SiloState>,
    server_id: String,
) -> CmdResult<Option<PlaybackConnection>> {
    silo.with_session(&server_id, |s| async move {
        let caps = s.playback_capabilities().await?;
        if !caps.has_feature(granary_silo_api::playback::BUFFERING_QUALITY_FEATURE) {
            return Ok(None);
        }
        s.playback_connection().await.map(Some)
    })
    .await
}

/// Forget the limit learned on this connection after buffering.
#[tauri::command]
pub async fn playback_learned_limit_reset(silo: State<'_, SiloState>, server_id: String) -> CmdResult<()> {
    silo.with_session(&server_id, |s| async move { s.playback_learned_limit_reset().await })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_follow_the_manifest() {
        assert!(valid(PREFERRED_QUALITY, &"720p".into()));
        assert!(!valid(PREFERRED_QUALITY, &"720p-medium".into()));
        assert!(valid(MAX_BITRATE, &serde_json::Value::Null));
        assert!(valid(MAX_BITRATE, &8000.into()));
        assert!(!valid(MAX_BITRATE, &50.into()));
        assert!(valid(BUFFERING_MODE, &"ask".into()));
        assert!(!valid("ui.theme", &"dark".into()));
        assert_eq!(quality_value(&"original".into()), "auto");
        assert_eq!(mode_value(&serde_json::Value::Null), "always");
    }
}
