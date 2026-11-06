//! Saved servers and non-secret account metadata: `%APPDATA%\Granary\servers.json` (ADR-0004).
//! Servers are keyed by `server_id` (`GET /api/v2/system/identity`), so the same deployment reached at another
//! address updates its record instead of creating a duplicate.
use crate::error::{ApiError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRecord {
    pub user_id: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerRecord {
    pub server_id: String,
    /// Normalised origin (`scheme://host[:port]`).
    pub origin: String,
    pub server_version: String,
    pub contract_digest: String,
    /// The signed-in account; its refresh token lives in the credential store under `<server_id>/<user_id>`.
    #[serde(default)]
    pub account: Option<AccountRecord>,
    /// Last selected profile (the PIN, if any, is asked again after a restart).
    #[serde(default)]
    pub last_profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registry {
    pub version: u32,
    #[serde(default)]
    pub servers: Vec<ServerRecord>,
    #[serde(default)]
    pub active: Option<String>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: 1,
            servers: Vec::new(),
            active: None,
        }
    }
}

impl Registry {
    /// Load, or start empty when the file is missing. A corrupt file is kept as `servers.json.bad` and replaced by an
    /// empty registry, so the app still starts.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "servers.json unreadable; starting with no servers");
                let _ = std::fs::rename(path, path.with_extension("json.bad"));
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| ApiError::Storage(e.to_string()))?;
        write_atomic(path, &json)
    }

    pub fn get(&self, server_id: &str) -> Option<&ServerRecord> {
        self.servers.iter().find(|s| s.server_id == server_id)
    }

    pub fn get_mut(&mut self, server_id: &str) -> Option<&mut ServerRecord> {
        self.servers.iter_mut().find(|s| s.server_id == server_id)
    }

    /// Insert or update by `server_id`, keeping the account of an existing record. Makes it the active server.
    pub fn upsert(&mut self, record: ServerRecord) -> &ServerRecord {
        let id = record.server_id.clone();
        if let Some(existing) = self.get_mut(&id) {
            existing.origin = record.origin;
            existing.server_version = record.server_version;
            existing.contract_digest = record.contract_digest;
        } else {
            self.servers.push(record);
        }
        self.active = Some(id.clone());
        let idx = self.servers.iter().position(|s| s.server_id == id).unwrap_or_default();
        &self.servers[idx]
    }

    pub fn remove(&mut self, server_id: &str) -> Option<ServerRecord> {
        let idx = self.servers.iter().position(|s| s.server_id == server_id)?;
        let removed = self.servers.remove(idx);
        if self.active.as_deref() == Some(server_id) {
            self.active = self.servers.first().map(|s| s.server_id.clone());
        }
        Some(removed)
    }
}

/// Write via a temporary file and rename, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| ApiError::Storage(e.to_string()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| ApiError::Storage(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| ApiError::Storage(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, origin: &str) -> ServerRecord {
        ServerRecord {
            server_id: id.into(),
            origin: origin.into(),
            server_version: "abc".into(),
            contract_digest: "d".into(),
            account: None,
            last_profile_id: None,
        }
    }

    #[test]
    fn upsert_dedupes_by_server_id_and_keeps_account() {
        let mut r = Registry::default();
        r.upsert(rec("s1", "https://a"));
        if let Some(s) = r.get_mut("s1") {
            s.account = Some(AccountRecord {
                user_id: "1".into(),
                username: "alice".into(),
            });
        }
        r.upsert(rec("s2", "https://b"));
        r.upsert(rec("s1", "https://a2"));
        assert_eq!(r.servers.len(), 2);
        assert_eq!(r.get("s1").map(|s| s.origin.as_str()), Some("https://a2"));
        assert!(r.get("s1").is_some_and(|s| s.account.is_some()));
        assert_eq!(r.active.as_deref(), Some("s1"));
        r.remove("s1");
        assert_eq!(r.active.as_deref(), Some("s2"));
    }

    #[test]
    fn save_load_round_trip_and_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("granary-reg-{}", uuid::Uuid::new_v4()));
        let path = dir.join("servers.json");
        let mut r = Registry::default();
        r.upsert(rec("s1", "https://a"));
        assert!(r.save(&path).is_ok());
        assert_eq!(Registry::load(&path), r);
        let _ = std::fs::write(&path, "garbage");
        assert_eq!(Registry::load(&path), Registry::default());
        assert!(path.with_extension("json.bad").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
