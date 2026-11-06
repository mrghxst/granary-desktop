//! Typed client for the Silo `/api/v2` (ADR-0004). Every endpoint used here is documented in
//! `docs/research/silo-api.md` with its upstream path@SHA; the HTTP implementation sits behind the [`SiloApi`] trait so
//! tests and UI development can use [`mock::MockSiloApi`].
pub mod api;
pub mod credentials;
pub mod device;
pub mod error;
pub mod mock;
pub mod models;
pub mod registry;
pub mod secret;
pub mod session;

pub use api::{Auth, HttpSiloApi, ProfileAuth, SiloApi};
pub use error::{ApiError, Result, TlsProblem};
pub use models::{Account, Profile, SystemInfo, UserLibrary};
pub use secret::Secret;
pub use session::Session;

/// `contract_digest` values (SHA-256 of the served OpenAPI artifact) Granary was built and tested against:
/// `silo-server/contracts/api/v2/openapi.json@6791aac867664c41c55ee82909fe063d145911a6`
/// (`git show 6791aac:contracts/api/v2/openapi.json | sha256sum`). Any other digest is an untested server
/// version: the add-server flow warns but continues, because the digest is diagnostic only and must never gate
/// features (`silo-server/internal/apiv2/system.go@6791aac` L15-25).
pub const TESTED_CONTRACT_DIGESTS: &[&str] = &["6a439ae59ca9e14e1c0edea0af872fc076fde5ddec60a2e978ee2dd74a9fb8b9"];

/// Normalise a user-entered server address to an origin (`scheme://host[:port]`, lower case, default port omitted,
/// no path): the same serialisation as the server's `request_origin`. A missing scheme means `https://`. Plain
/// `http://` is accepted here; the UI warns before using it (ADR-0004).
pub fn normalize_origin(input: &str) -> Result<String> {
    let trimmed = input.trim();
    let invalid = || ApiError::InvalidUrl(trimmed.to_owned());
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return Err(invalid());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let url = url::Url::parse(&with_scheme).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid());
    }
    Ok(url.origin().ascii_serialization())
}

/// Result of checking a server address before it is saved.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeResult {
    pub origin: String,
    pub server_id: String,
    pub info: SystemInfo,
    /// `contract_digest` is not in [`TESTED_CONTRACT_DIGESTS`].
    pub untested: bool,
    /// Plain `http://`: credentials would travel unencrypted.
    pub insecure: bool,
    /// The server's `request_origin` when it differs from the configured origin (proxy or redirect; the
    /// discovery contract asks clients to compare before signing in). Shown as a warning.
    pub reported_origin: Option<String>,
}

/// Add-server check: normalise, `GET /api/v2/system/info` (requires `api_major == 2`), then
/// `GET /api/v2/system/identity` for the de-duplication key (silo-api.md §2; Android `ApiV2Probe.kt@ee178d8`).
pub async fn probe(api: &dyn SiloApi, input: &str) -> Result<ProbeResult> {
    let origin = normalize_origin(input)?;
    let info = match api.system_info(&origin).await {
        Ok(info) => info,
        // A 404 (v1-only alpha server or another web app) or a non-JSON 200 means this is not a v2 Silo server.
        Err(ApiError::Http { status: 404, .. }) => return Err(ApiError::NotSilo { status: 404 }),
        Err(ApiError::Decode(_)) => return Err(ApiError::NotSilo { status: 200 }),
        Err(e) => return Err(e),
    };
    if info.api_major != 2 {
        return Err(ApiError::UnsupportedApi {
            api_major: info.api_major,
        });
    }
    let identity = api.server_identity(&origin).await?;
    let reported_origin = info.request_origin.clone().filter(|r| !r.is_empty() && *r != origin);
    Ok(ProbeResult {
        insecure: origin.starts_with("http://"),
        untested: !TESTED_CONTRACT_DIGESTS.contains(&info.contract_digest.as_str()),
        server_id: identity.server_id,
        reported_origin,
        info,
        origin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock::MockSiloApi;

    #[test]
    fn normalizes_origins() {
        let ok = |s: &str| normalize_origin(s).ok();
        assert_eq!(
            ok(" HTTPS://Media.Example.com:8443/ ").as_deref(),
            Some("https://media.example.com:8443")
        );
        assert_eq!(
            ok("http://192.168.1.5:8080/web/").as_deref(),
            Some("http://192.168.1.5:8080")
        );
        assert_eq!(ok("media.example.com").as_deref(), Some("https://media.example.com"));
        assert_eq!(
            ok("https://h:443/x").as_deref(),
            Some("https://h"),
            "default port dropped"
        );
        assert_eq!(ok("http://[::1]:8096").as_deref(), Some("http://[::1]:8096"));
        assert_eq!(ok("https://h:1?x=1#f").as_deref(), Some("https://h:1"));
        assert!(normalize_origin("ftp://x").is_err());
        assert!(normalize_origin("https://user:pw@host").is_err());
        assert!(normalize_origin("https://").is_err());
        assert!(normalize_origin("").is_err());
        assert!(normalize_origin("my server").is_err());
    }

    #[tokio::test]
    async fn probe_accepts_v2_and_flags_warnings() {
        let api = MockSiloApi::demo();
        let p = probe(&api, "http://Mock.Example:8096/").await;
        let Ok(p) = p else { panic!("probe failed: {p:?}") };
        assert_eq!(p.origin, "http://mock.example:8096");
        assert!(p.insecure && !p.untested && p.reported_origin.is_none());
        assert_eq!(p.server_id, api.server_id);

        let mut api = MockSiloApi::demo();
        api.info.contract_digest = "other".into();
        api.info.request_origin = Some("http://internal:8096".into());
        let p = probe(&api, "https://mock.example").await.ok();
        assert!(p.as_ref().is_some_and(|p| p.untested && !p.insecure));
        assert_eq!(
            p.and_then(|p| p.reported_origin).as_deref(),
            Some("http://internal:8096")
        );

        let mut api = MockSiloApi::demo();
        api.info.api_major = 3;
        assert!(matches!(
            probe(&api, "https://x").await,
            Err(ApiError::UnsupportedApi { api_major: 3 })
        ));
    }
}
