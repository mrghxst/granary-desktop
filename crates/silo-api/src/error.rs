//! [`ApiError`]: every failure the client reports, with a stable [`ApiError::code`] the UI translates.
use crate::models::Problem;

/// Why a TLS handshake failed, for a specific message in the add-server flow (ADR-0004 L3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsProblem {
    Expired,
    NotYetValid,
    UntrustedIssuer,
    NameMismatch,
    Revoked,
    Other,
}

impl TlsProblem {
    pub fn code(self) -> &'static str {
        match self {
            Self::Expired => "tls_expired",
            Self::NotYetValid => "tls_not_yet_valid",
            Self::UntrustedIssuer => "tls_untrusted",
            Self::NameMismatch => "tls_name_mismatch",
            Self::Revoked => "tls_revoked",
            Self::Other => "tls_error",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),
    #[error("TLS error ({}): {message}", kind.code())]
    Tls { kind: TlsProblem, message: String },
    #[error("server unreachable: {0}")]
    Unreachable(String),
    #[error("request timed out")]
    Timeout,
    #[error("server answered with a redirect to {location}")]
    Redirected { location: String },
    #[error("not a Silo server or the /api/v2 surface is missing (HTTP {status})")]
    NotSilo { status: u16 },
    #[error("server speaks API major {api_major}, Granary needs 2")]
    UnsupportedApi { api_major: i64 },
    #[error("HTTP {status}: {}", problem.as_deref().map(Problem::id).unwrap_or("no problem document"))]
    Http {
        status: u16,
        problem: Option<Box<Problem>>,
        /// Seconds from a `Retry-After` header (503/429).
        retry_after: Option<u64>,
    },
    /// Login answered 401 (`invalid_token`, apiv2/auth.go@6791aac L240-247).
    #[error("wrong username or password")]
    InvalidCredentials,
    #[error("signed out: the server rejected the stored session")]
    SignedOut,
    #[error("not signed in")]
    NotSignedIn,
    #[error("unknown server {0}")]
    UnknownServer(String),
    #[error("profile PIN required")]
    ProfileVerificationRequired,
    #[error("wrong PIN")]
    WrongPin,
    #[error("response could not be decoded: {0}")]
    Decode(String),
    #[error("credential store: {0}")]
    Credentials(String),
    #[error("internal: {0}")]
    Internal(String),
    #[error("storage: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;

impl ApiError {
    /// The problem identifier when the server sent a problem document.
    pub fn problem_id(&self) -> Option<&str> {
        match self {
            Self::Http { problem: Some(p), .. } => Some(p.id()),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http { status, .. } => Some(*status),
            Self::NotSilo { status } => Some(*status),
            _ => None,
        }
    }

    /// Stable machine-readable code for the UI (translated there; never parsed from `Display`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl(_) => "invalid_url",
            Self::Tls { kind, .. } => kind.code(),
            Self::Unreachable(_) => "unreachable",
            Self::Timeout => "timeout",
            Self::Redirected { .. } => "redirected",
            Self::NotSilo { .. } => "not_silo",
            Self::UnsupportedApi { .. } => "unsupported_api",
            Self::Http { status, .. } => match (self.problem_id(), status) {
                (Some("account_required"), _) => "account_required",
                (Some("provider_unavailable"), _) => "unavailable",
                (Some("password_change_required"), _) => "password_change_required",
                (Some("account_disabled"), _) => "account_disabled",
                (Some("profile_verification_required"), _) => "profile_verification_required",
                (Some("rate_limited"), _) | (_, 429) => "rate_limited",
                (Some("client_upgrade_required"), _) => "client_upgrade_required",
                (_, 401) => "unauthorized",
                (_, 403) => "forbidden",
                (_, 404) => "not_found",
                (_, 503) => "unavailable",
                (_, s) if *s >= 500 => "server_error",
                _ => "http_error",
            },
            Self::InvalidCredentials => "invalid_credentials",
            Self::SignedOut => "signed_out",
            Self::NotSignedIn => "not_signed_in",
            Self::UnknownServer(_) => "unknown_server",
            Self::ProfileVerificationRequired => "profile_verification_required",
            Self::WrongPin => "wrong_pin",
            Self::Decode(_) => "decode",
            Self::Credentials(_) => "credentials",
            Self::Storage(_) => "storage",
            Self::Internal(_) => "internal",
        }
    }

    /// Classify a transport error: TLS certificate problems first (they surface as connect errors), then timeouts
    /// and connection failures.
    pub(crate) fn from_transport(e: &reqwest::Error) -> Self {
        if let Some(kind) = tls_problem(e) {
            return Self::Tls {
                kind,
                message: error_chain(e),
            };
        }
        if e.is_timeout() {
            return Self::Timeout;
        }
        if e.is_decode() {
            return Self::Decode(error_chain(e));
        }
        Self::Unreachable(error_chain(e))
    }
}

/// `source()` chain joined with `: `; URLs are stripped by reqwest's `without_url` at the call sites.
fn error_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut cur = e.source();
    while let Some(s) = cur {
        out.push_str(": ");
        out.push_str(&s.to_string());
        cur = s.source();
    }
    out
}

/// Find a `rustls::Error` in the chain. tokio-rustls wraps it in an `io::Error`, which hyper-util wraps in another one;
/// `io::Error::source()` skips the wrapped payload, so `io::Error::get_ref` is searched as well.
fn tls_problem(e: &(dyn std::error::Error + 'static)) -> Option<TlsProblem> {
    find_rustls(e, 0).map(classify_rustls)
}

/// Depth-first search through `source()` and `io::Error` payloads (observed chain: reqwest → hyper-util Connect →
/// io::Error(Other) → io::Error(InvalidData) → rustls::Error).
fn find_rustls<'a>(err: &'a (dyn std::error::Error + 'static), depth: u8) -> Option<&'a rustls::Error> {
    if depth > 16 {
        return None;
    }
    if let Some(r) = err.downcast_ref::<rustls::Error>() {
        return Some(r);
    }
    if let Some(io) = err.downcast_ref::<std::io::Error>()
        && let Some(inner) = io.get_ref()
        && let Some(r) = find_rustls(inner, depth + 1)
    {
        return Some(r);
    }
    find_rustls(err.source()?, depth + 1)
}

fn classify_rustls(e: &rustls::Error) -> TlsProblem {
    use rustls::CertificateError as C;
    match e {
        rustls::Error::InvalidCertificate(c) => match c {
            C::Expired | C::ExpiredContext { .. } => TlsProblem::Expired,
            C::NotValidYet | C::NotValidYetContext { .. } => TlsProblem::NotYetValid,
            C::UnknownIssuer => TlsProblem::UntrustedIssuer,
            C::NotValidForName | C::NotValidForNameContext { .. } => TlsProblem::NameMismatch,
            C::Revoked => TlsProblem::Revoked,
            _ => TlsProblem::Other,
        },
        _ => TlsProblem::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustls_errors_inside_io_errors_are_classified() {
        let io = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer),
        );
        assert_eq!(tls_problem(&io), Some(TlsProblem::UntrustedIssuer));
        let expired = rustls::Error::InvalidCertificate(rustls::CertificateError::Expired);
        assert_eq!(tls_problem(&expired), Some(TlsProblem::Expired));
        let other = std::io::Error::other("x");
        assert_eq!(tls_problem(&other), None);
    }

    #[test]
    fn codes_follow_problem_ids() {
        let e = ApiError::Http {
            status: 401,
            problem: Some(Box::new(Problem {
                type_uri: "https://siloserver.org/docs/api/v2/problems/password_change_required".into(),
                ..Problem::default()
            })),
            retry_after: None,
        };
        assert_eq!(e.code(), "password_change_required");
        let e = ApiError::Http {
            status: 503,
            problem: None,
            retry_after: Some(5),
        };
        assert_eq!(e.code(), "unavailable");
    }
}
