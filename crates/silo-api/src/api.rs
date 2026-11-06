//! The [`SiloApi`] trait (one method per endpoint Granary uses, all listed in docs/research/silo-api.md) and its
//! HTTP implementation. Methods are stateless: tokens and refresh live in [`crate::session::Session`].
use crate::device::{CLIENT_FAMILY, CLIENT_NAME, DEVICE_PLATFORM, DeviceIdentity};
use crate::error::{ApiError, Result};
use crate::models::*;
use crate::secret::Secret;
use async_trait::async_trait;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;

/// Selected profile: `X-Profile-Id` and, for a PIN-locked profile, `X-Profile-Token` (silo-api.md §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileAuth {
    pub id: String,
    pub token: Option<Secret>,
}

/// Credentials for one authenticated request.
#[derive(Debug, Clone)]
pub struct Auth {
    pub access: Secret,
    pub profile: Option<ProfileAuth>,
}

#[async_trait]
pub trait SiloApi: Send + Sync {
    /// `GET /api/v2/system/info` (getSystemInfo), public.
    async fn system_info(&self, origin: &str) -> Result<SystemInfo>;
    /// `GET /api/v2/system/identity` (getServerIdentity), public.
    async fn server_identity(&self, origin: &str) -> Result<ServerIdentity>;
    /// `POST /api/v2/auth/login` (login), public; device headers recorded on the session.
    async fn login(&self, origin: &str, username: &str, password: &Secret) -> Result<TokenPair>;
    /// `POST /api/v2/auth/refresh` (refreshSession), public; rotates the pair.
    async fn refresh(&self, origin: &str, refresh_token: &Secret) -> Result<RefreshedTokens>;
    /// `POST /api/v2/auth/logout` (logout), bearer.
    async fn logout(&self, origin: &str, auth: &Auth) -> Result<()>;
    /// `GET /api/v2/account/me` (getCurrentUser), bearer.
    async fn current_user(&self, origin: &str, auth: &Auth) -> Result<Account>;
    /// `GET /api/v2/profiles` (listProfiles), bearer.
    async fn list_profiles(&self, origin: &str, auth: &Auth) -> Result<ProfileCollection>;
    /// `POST /api/v2/profiles/{id}/verify-pin` (verifyProfilePIN), bearer.
    async fn verify_profile_pin(
        &self,
        origin: &str,
        auth: &Auth,
        profile_id: &str,
        pin: &Secret,
    ) -> Result<ProfileVerification>;
    /// `GET /api/v2/user/libraries` (listUserLibraries), bearer + profile headers.
    async fn list_user_libraries(&self, origin: &str, auth: &Auth) -> Result<Vec<UserLibrary>>;
}

/// `reqwest` + rustls with the platform verifier (Windows certificate store, ADR-0004 L3).
pub struct HttpSiloApi {
    http: reqwest::Client,
    device: DeviceIdentity,
}

impl HttpSiloApi {
    pub fn new(device: DeviceIdentity) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("Granary/{} (Windows)", device.client_version))
            // Never follow redirects: authenticated /api/v2 routes do not redirect (api-contract.md@6791aac
            // "Discovery and capability detection"), and following one could carry Bearer/profile headers to another
            // origin. A redirect is reported to the user instead (ADR-0004 M4).
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(6))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok(Self { http, device })
    }

    fn request(&self, method: Method, origin: &str, path: &str) -> RequestBuilder {
        let d = &self.device;
        self.http
            .request(method, format!("{origin}{path}"))
            .header("Accept", "application/json, application/problem+json")
            .header("X-Silo-Device-Id", &d.id)
            .header("X-Silo-Device-Name", &d.name)
            .header("X-Silo-Device-Platform", DEVICE_PLATFORM)
            .header("X-Silo-Client", CLIENT_NAME)
            .header("X-Silo-Client-Version", &d.client_version)
            .header("X-Silo-Client-Family", CLIENT_FAMILY)
    }

    fn authed(&self, method: Method, origin: &str, path: &str, auth: &Auth) -> RequestBuilder {
        let mut rb = self.request(method, origin, path).bearer_auth(auth.access.expose());
        if let Some(p) = &auth.profile {
            rb = rb.header("X-Profile-Id", &p.id);
            if let Some(t) = &p.token {
                rb = rb.header("X-Profile-Token", t.expose());
            }
        }
        rb
    }
}

async fn send_raw(rb: RequestBuilder) -> Result<reqwest::Response> {
    let resp = rb
        .send()
        .await
        .map_err(|e| ApiError::from_transport(&e.without_url()))?;
    let status = resp.status();
    if status.is_redirection() {
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        return Err(ApiError::Redirected { location });
    }
    if status.is_success() {
        return Ok(resp);
    }
    Err(error_from_response(resp).await)
}

async fn send_json<T: DeserializeOwned>(rb: RequestBuilder) -> Result<T> {
    let resp = send_raw(rb).await?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ApiError::from_transport(&e.without_url()))?;
    serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

async fn error_from_response(resp: reqwest::Response) -> ApiError {
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let problem = resp
        .bytes()
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<Problem>(&b).ok())
        .map(Box::new);
    ApiError::Http {
        status,
        problem,
        retry_after,
    }
}

#[async_trait]
impl SiloApi for HttpSiloApi {
    async fn system_info(&self, origin: &str) -> Result<SystemInfo> {
        send_json(self.request(Method::GET, origin, "/api/v2/system/info")).await
    }

    async fn server_identity(&self, origin: &str) -> Result<ServerIdentity> {
        send_json(self.request(Method::GET, origin, "/api/v2/system/identity")).await
    }

    async fn login(&self, origin: &str, username: &str, password: &Secret) -> Result<TokenPair> {
        let rb = self
            .request(Method::POST, origin, "/api/v2/auth/login")
            .json(&LoginRequest { username, password });
        match send_json(rb).await {
            Err(ApiError::Http { status, .. }) if status == StatusCode::UNAUTHORIZED.as_u16() => {
                Err(ApiError::InvalidCredentials)
            }
            other => other,
        }
    }

    async fn refresh(&self, origin: &str, refresh_token: &Secret) -> Result<RefreshedTokens> {
        let rb = self
            .request(Method::POST, origin, "/api/v2/auth/refresh")
            .json(&RefreshRequest { refresh_token });
        send_json(rb).await
    }

    async fn logout(&self, origin: &str, auth: &Auth) -> Result<()> {
        send_raw(self.authed(Method::POST, origin, "/api/v2/auth/logout", auth))
            .await
            .map(drop)
    }

    async fn current_user(&self, origin: &str, auth: &Auth) -> Result<Account> {
        send_json(self.authed(Method::GET, origin, "/api/v2/account/me", auth)).await
    }

    async fn list_profiles(&self, origin: &str, auth: &Auth) -> Result<ProfileCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/profiles", auth)).await
    }

    async fn verify_profile_pin(
        &self,
        origin: &str,
        auth: &Auth,
        profile_id: &str,
        pin: &Secret,
    ) -> Result<ProfileVerification> {
        let path = format!("/api/v2/profiles/{}/verify-pin", url_segment(profile_id));
        send_json(self.authed(Method::POST, origin, &path, auth).json(&PinRequest { pin })).await
    }

    async fn list_user_libraries(&self, origin: &str, auth: &Auth) -> Result<Vec<UserLibrary>> {
        let c: UserLibraryCollection =
            send_json(self.authed(Method::GET, origin, "/api/v2/user/libraries", auth)).await?;
        Ok(c.items)
    }
}

/// Percent-encode an opaque identifier for use as one path segment.
fn url_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_are_encoded() {
        assert_eq!(url_segment("12"), "12");
        assert_eq!(url_segment("a/b?c"), "a%2Fb%3Fc");
    }
}
