//! Streaming media requests for the host relay (ADR-0008): `getPlaybackMedia`, `getPlaybackManifest`,
//! `getPlaybackSegment` and `getPlaybackSubtitle` (silo-api.md §13). Bodies are streamed, never buffered whole.
use crate::api::Auth;
use bytes::Bytes;
use futures_core::Stream;
use std::pin::Pin;

/// Request headers the relay forwards from mpv to the server (conditional and range requests; OAS @6791aac
/// `getPlaybackMedia`/`getPlaybackSegment` parameters).
pub const FORWARDED_REQUEST_HEADERS: &[&str] = &[
    "range",
    "if-range",
    "if-none-match",
    "if-modified-since",
    "if-match",
    "if-unmodified-since",
    "accept",
];

/// Response headers the relay passes back to mpv.
pub const FORWARDED_RESPONSE_HEADERS: &[&str] = &[
    "content-type",
    "content-length",
    "content-range",
    "accept-ranges",
    "etag",
    "last-modified",
    "cache-control",
];

/// One media request. `url` is absolute; `auth` adds the bearer, profile and device headers (only ever for the
/// saved server origin, `header_authenticated_media_v1`).
#[derive(Debug, Clone)]
pub struct MediaRequest {
    pub url: String,
    pub auth: Option<Auth>,
    pub head: bool,
    /// Lower-case names from [`FORWARDED_REQUEST_HEADERS`].
    pub headers: Vec<(String, String)>,
}

pub type MediaBody = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>;

/// A 2xx/304/416 media response. Other statuses are returned as [`crate::ApiError::Http`] so the session can refresh
/// on 401 and the controller can react to 404/410.
pub struct MediaResponse {
    pub status: u16,
    /// Lower-case names from [`FORWARDED_RESPONSE_HEADERS`].
    pub headers: Vec<(String, String)>,
    pub body: MediaBody,
}

impl std::fmt::Debug for MediaResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

impl MediaResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Where a media URL from a playback plan lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaTarget {
    /// On the saved server origin: fetched by the host relay with the account's credentials.
    Server(String),
    /// On another origin (e.g. a proxy or CDN the server projected): handed to the player as-is, never with
    /// credentials.
    Foreign(String),
}

/// Resolve a media URL the way the web app does after the server's v2 projection
/// (`internal/apiv2/playback.go@6791aac` `playbackV2MediaURL`): `/api/...` is used as-is, a legacy root-relative
/// `/stream/...` or `/playback/transcode/...` gets the `/api/v2` prefix, absolute URLs are compared to `origin`
/// (scheme, host and port, case-insensitive). Anything else (other relative forms, non-HTTP schemes,
/// protocol-relative `//host`) is rejected.
pub fn resolve_media_url(origin: &str, raw: &str) -> crate::Result<MediaTarget> {
    let raw = raw.trim();
    let bad = || crate::ApiError::Internal("unsupported media URL in the playback plan".into());
    if raw.starts_with("//") || raw.chars().any(|c| c.is_control() || c == '\\') {
        return Err(bad());
    }
    if raw.starts_with("/api/") {
        return Ok(MediaTarget::Server(format!("{origin}{raw}")));
    }
    if raw.starts_with("/stream/") || raw.starts_with("/playback/transcode/") {
        return Ok(MediaTarget::Server(format!("{origin}/api/v2{raw}")));
    }
    let url = url::Url::parse(raw).map_err(|_| bad())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(bad());
    }
    let same = url::Url::parse(origin)
        .map(|o| o.origin() == url.origin())
        .unwrap_or(false);
    if same {
        Ok(MediaTarget::Server(url.to_string()))
    } else {
        Ok(MediaTarget::Foreign(url.to_string()))
    }
}

/// A parsed single `Range: bytes=` request (the only form mpv/FFmpeg send).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    /// Inclusive end, clamped to the resource.
    pub end: u64,
}

/// Parse `bytes=a-b`, `bytes=a-` or `bytes=-n` against a resource of `len` bytes. `None` for a missing or
/// multi-range header (serve the whole body); `Some(Err(()))` when unsatisfiable (416).
#[allow(clippy::result_unit_err)]
pub fn parse_range(header: Option<&str>, len: u64) -> Option<Result<ByteRange, ()>> {
    let spec = header?.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let range = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        if n == 0 || len == 0 {
            return Some(Err(()));
        }
        ByteRange {
            start: len.saturating_sub(n),
            end: len - 1,
        }
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { u64::MAX } else { b.parse().ok()? };
        if start >= len || end < start {
            return Some(Err(()));
        }
        ByteRange {
            start,
            end: end.min(len - 1),
        }
    };
    Some(Ok(range))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(
            parse_range(Some("bytes=0-99"), 1000),
            Some(Ok(ByteRange { start: 0, end: 99 }))
        );
        assert_eq!(
            parse_range(Some("bytes=900-"), 1000),
            Some(Ok(ByteRange { start: 900, end: 999 }))
        );
        assert_eq!(
            parse_range(Some("bytes=-100"), 1000),
            Some(Ok(ByteRange { start: 900, end: 999 }))
        );
        assert_eq!(
            parse_range(Some("bytes=990-5000"), 1000),
            Some(Ok(ByteRange { start: 990, end: 999 }))
        );
        assert_eq!(parse_range(Some("bytes=1000-"), 1000), Some(Err(())));
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 1000), None);
        assert_eq!(parse_range(None, 1000), None);
        assert_eq!(parse_range(Some("items=0-1"), 1000), None);
    }

    #[test]
    fn media_urls_resolve_like_the_web_app() {
        let o = "https://media.example.test:8443";
        let r = |u: &str| resolve_media_url(o, u).ok();
        assert_eq!(
            r("/api/v2/stream/ps-1"),
            Some(MediaTarget::Server(format!("{o}/api/v2/stream/ps-1")))
        );
        assert_eq!(
            r("/stream/ps-1"),
            Some(MediaTarget::Server(format!("{o}/api/v2/stream/ps-1")))
        );
        assert_eq!(
            r("/playback/transcode/t/master.m3u8"),
            Some(MediaTarget::Server(format!(
                "{o}/api/v2/playback/transcode/t/master.m3u8"
            )))
        );
        assert_eq!(
            r("HTTPS://Media.Example.Test:8443/api/v2/stream/x?st=1"),
            Some(MediaTarget::Server(format!("{o}/api/v2/stream/x?st=1")))
        );
        // Same host, other port or scheme → another origin: never gets credentials.
        assert_eq!(
            r("https://media.example.test/api/v2/stream/x"),
            Some(MediaTarget::Foreign(
                "https://media.example.test/api/v2/stream/x".into()
            ))
        );
        assert_eq!(
            r("http://media.example.test:8443/x"),
            Some(MediaTarget::Foreign("http://media.example.test:8443/x".into()))
        );
        for bad in [
            "//evil.test/x",
            "file:///c:/x",
            "stream/x",
            "/other/x",
            "javascript:alert(1)",
            "/api/\\x",
        ] {
            assert_eq!(r(bad), None, "{bad}");
        }
    }
}
