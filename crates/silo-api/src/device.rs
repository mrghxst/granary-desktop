//! Device identity sent with every request (silo-api.md §1.4, ADR-0004 "Device identity").
use crate::error::{ApiError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Value of `X-Silo-Client`: the client name the prompt asks for (`<APP_NAME> for Windows`).
pub const CLIENT_NAME: &str = "Granary for Windows";
/// Value of `X-Silo-Client-Family`; one of `tv|mobile|tablet|desktop|web`
/// (`silo-server/internal/api/handlers/settings_values_seam.go@6791aac` L183).
pub const CLIENT_FAMILY: &str = "desktop";
/// Value of `X-Silo-Device-Platform` (free-form, ≤40 chars).
pub const DEVICE_PLATFORM: &str = "windows";

/// Identity headers of this installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    /// Random UUID v4 created on first run and persisted.
    pub id: String,
    /// `X-Silo-Device-Name` (≤120 chars): the computer name, so the server's device list is recognisable.
    pub name: String,
    pub client_version: String,
}

#[derive(Serialize, Deserialize)]
struct DeviceFile {
    device_id: String,
}

impl DeviceIdentity {
    /// Load `device.json` from `path`, creating it with a new UUID when it is missing or unreadable.
    pub fn load_or_create(path: &Path, client_version: &str) -> Result<Self> {
        let existing = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<DeviceFile>(&s).ok())
            .filter(|f| uuid::Uuid::parse_str(&f.device_id).is_ok());
        let id = match existing {
            Some(f) => f.device_id,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                let json = serde_json::to_string_pretty(&DeviceFile { device_id: id.clone() })
                    .map_err(|e| ApiError::Storage(e.to_string()))?;
                crate::registry::write_atomic(path, json.as_bytes())?;
                id
            }
        };
        Ok(Self {
            id,
            name: device_name(),
            client_version: client_version.to_owned(),
        })
    }

    /// Identity for tests and the mock server.
    pub fn fixed(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: "test-pc".into(),
            client_version: "0.0.0-test".into(),
        }
    }
}

fn device_name() -> String {
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(120).collect();
    if name.is_empty() { "Windows PC".into() } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_created_once_and_reused() {
        let dir = std::env::temp_dir().join(format!("granary-device-{}", uuid::Uuid::new_v4()));
        let path = dir.join("device.json");
        let a = DeviceIdentity::load_or_create(&path, "1.0.0");
        let b = DeviceIdentity::load_or_create(&path, "1.0.0");
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (a.ok(), b.ok());
        assert!(a.is_some());
        assert_eq!(a.as_ref().map(|d| &d.id), b.as_ref().map(|d| &d.id));
        assert!(a.is_some_and(|d| uuid::Uuid::parse_str(&d.id).is_ok() && d.name.len() <= 120));
    }

    #[test]
    fn corrupt_device_file_is_replaced() {
        let dir = std::env::temp_dir().join(format!("granary-device-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("device.json");
        let _ = std::fs::write(&path, "{not json");
        let d = DeviceIdentity::load_or_create(&path, "1.0.0").ok();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(d.is_some_and(|d| uuid::Uuid::parse_str(&d.id).is_ok()));
    }
}
