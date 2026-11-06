//! Typed client for the Silo `/api/v2` (ADR-0004). Every endpoint used here must be documented in
//! `docs/research/silo-api.md` with its upstream path@SHA; unconfirmed ones stay behind the `SiloApi` trait and are
//! marked UNVERIFIED. The HTTP implementation, auth and token storage arrive in Phase 4.

/// Errors returned by the client.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),
}

/// Normalise a user-entered server address to an origin (`scheme://host[:port]`) without a trailing slash.
/// Plain `http://` is accepted here; the UI warns before using it (ADR-0004).
pub fn normalize_origin(input: &str) -> Result<String, ApiError> {
    let s = input.trim().trim_end_matches('/');
    let (scheme, rest) = s.split_once("://").ok_or_else(|| ApiError::InvalidUrl(input.into()))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(ApiError::InvalidUrl(input.into()));
    }
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() || host.contains(char::is_whitespace) || host.contains('@') {
        return Err(ApiError::InvalidUrl(input.into()));
    }
    Ok(format!("{scheme}://{}", host.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_origins() {
        assert_eq!(
            normalize_origin(" HTTPS://Media.Example.com:8443/ ").ok().as_deref(),
            Some("https://media.example.com:8443")
        );
        assert_eq!(
            normalize_origin("http://192.168.1.5:8080/web/").ok().as_deref(),
            Some("http://192.168.1.5:8080")
        );
        assert!(normalize_origin("ftp://x").is_err());
        assert!(normalize_origin("media.example.com").is_err());
        assert!(normalize_origin("https://user:pw@host").is_err());
        assert!(normalize_origin("https://").is_err());
    }
}
