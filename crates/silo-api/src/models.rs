//! Wire models for the endpoints Granary uses. Shapes follow `silo-server/contracts/api/v2/openapi.json@6791aac`
//! (component names in each doc comment; see docs/research/silo-api.md §1–§4). Deserialization is lenient where the
//! client does not depend on a field (`#[serde(default)]`) and ignores unknown fields, because pre-1.0 servers add
//! members; the contract tests check every fixture against the upstream schema subset.
use crate::secret::Secret;
use serde::{Deserialize, Serialize};

/// `SystemInfo` (OAS:L53300), from `GET /api/v2/system/info`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SystemInfo {
    pub api_major: i64,
    pub server_version: String,
    pub contract_digest: String,
    pub links: SystemInfoLinks,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_origin: Option<String>,
}

/// `SystemInfoLinks`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SystemInfoLinks {
    pub openapi: String,
    pub capabilities: String,
    pub identity: String,
}

/// `ServerIdentity`, from `GET /api/v2/system/identity`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ServerIdentity {
    pub server_id: String,
}

/// `LoginInputBody`.
#[derive(Debug, Serialize)]
pub struct LoginRequest<'a> {
    pub username: &'a str,
    pub password: &'a Secret,
}

/// `TokenPair` (OAS:L54121), from `POST /api/v2/auth/login`.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenPair {
    pub access_token: Secret,
    pub refresh_token: Secret,
    /// Access token lifetime in seconds.
    pub expires_in: i64,
    pub user: Account,
}

/// `RefreshSessionInputBody`.
#[derive(Debug, Serialize)]
pub struct RefreshRequest<'a> {
    pub refresh_token: &'a Secret,
}

/// `RefreshedTokens`, from `POST /api/v2/auth/refresh`. The refresh token rotates: always persist the new one.
#[derive(Debug, Clone, Deserialize)]
pub struct RefreshedTokens {
    pub access_token: Secret,
    pub refresh_token: Secret,
    pub expires_in: i64,
}

/// `Account`, from `GET /api/v2/account/me` and inside `TokenPair`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Account {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub download_allowed: bool,
    #[serde(default)]
    pub password_change_required: bool,
}

/// `Profile` (OAS:L46528); only the members the client uses.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub has_pin: bool,
    #[serde(default)]
    pub is_child: bool,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// Preferred audio language (BCP 47); empty when unset.
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub subtitle_language: String,
    #[serde(default)]
    pub show_forced_subtitles: bool,
    #[serde(default)]
    pub auto_skip_intro: bool,
    #[serde(default)]
    pub auto_skip_credits: bool,
    #[serde(default)]
    pub auto_skip_recap: bool,
    #[serde(default)]
    pub max_playback_quality: String,
}

/// `ProfileCollection`, from `GET /api/v2/profiles`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ProfileCollection {
    pub items: Vec<Profile>,
}

/// `ProfilePINCheck`.
#[derive(Debug, Serialize)]
pub struct PinRequest<'a> {
    pub pin: &'a Secret,
}

/// `ProfileVerification` (OAS:L47478), from `POST /api/v2/profiles/{id}/verify-pin`.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileVerification {
    pub valid: bool,
    #[serde(default)]
    pub profile_token: Option<Secret>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// `UserLibrary` (OAS:L54514).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct UserLibrary {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub box_sets_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
}

/// `CollectionUserLibrary`, from `GET /api/v2/user/libraries` (bounded, unpaginated).
#[derive(Debug, Clone, Deserialize)]
pub struct UserLibraryCollection {
    pub items: Vec<UserLibrary>,
}

/// RFC 9457 `Problem` (OAS:L58829). Clients branch on [`Problem::id`], never on `detail`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct Problem {
    #[serde(rename = "type", default)]
    pub type_uri: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: i64,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub instance: String,
}

impl Problem {
    /// The stable problem identifier: the final path segment of `type`
    /// (`https://siloserver.org/docs/api/v2/problems/<id>`, `silo-server/internal/apiv2/problem.go@6791aac` L22).
    pub fn id(&self) -> &str {
        self.type_uri.rsplit('/').next().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_id_is_last_segment() {
        let p = Problem {
            type_uri: "https://siloserver.org/docs/api/v2/problems/token_refresh_required".into(),
            ..Problem::default()
        };
        assert_eq!(p.id(), "token_refresh_required");
        assert_eq!(Problem::default().id(), "");
    }
}
