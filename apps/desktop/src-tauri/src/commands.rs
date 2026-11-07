//! Tauri commands exposed to the UI. Every command is listed in `build.rs` (`COMMANDS`) and granted explicitly in
//! `capabilities/main.json`; nothing else is reachable from the WebView.
use serde::Serialize;

/// Result of the startup libmpv probe, as the UI sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibmpvStatus {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl LibmpvStatus {
    pub fn from_probe(r: Result<u64, String>) -> Self {
        match r {
            // MPV_CLIENT_API_VERSION is (major << 16) | minor.
            Ok(v) => Self {
                ok: true,
                api_version: Some(format!("{}.{}", v >> 16, v & 0xffff)),
                error: None,
            },
            Err(e) => Self {
                ok: false,
                api_version: None,
                error: Some(e),
            },
        }
    }
}

/// Static host facts, mirrored by `AppInfo` in `src/lib/host.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub libmpv: LibmpvStatus,
}

/// The UI calls this once its first screen is interactive: logs `ready` with the cold-start time (Phase 8/9 checks).
#[tauri::command]
pub fn app_ready() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let ms = crate::STARTED.get().map_or(0, |t| t.elapsed().as_millis() as u64);
        tracing::info!(cold_start_ms = ms, "ready");
    });
}

#[tauri::command]
pub fn app_info(info: tauri::State<'_, AppInfo>) -> AppInfo {
    info.inner().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libmpv_status_serialises_like_the_ts_union() -> Result<(), serde_json::Error> {
        let ok = serde_json::to_value(LibmpvStatus::from_probe(Ok((2 << 16) | 5)))?;
        assert_eq!(ok, serde_json::json!({"ok": true, "apiVersion": "2.5"}));
        let err = serde_json::to_value(LibmpvStatus::from_probe(Err("missing".into())))?;
        assert_eq!(err, serde_json::json!({"ok": false, "error": "missing"}));
        Ok(())
    }
}
