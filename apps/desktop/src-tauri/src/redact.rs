//! Secret redaction for log output (rule 6: tokens never logged or printed). [`RedactingWriter`] passes every
//! formatted log line through [`redact`], so a stray `?request`, error message or response body cannot leak a bearer
//! token, refresh/profile token, stream signature, WebSocket ticket, PIN, password or API key. Secret-carrying types
//! should additionally have a redacting `Debug`; this filter is the backstop.
//!
//! All slicing happens at positions of ASCII bytes or at the string ends, so arbitrary UTF-8 can never panic here
//! (phase-3 review H2); the writer also catches panics as a last resort.
use std::borrow::Cow;
use std::io::{self, Write};

const MASK: &str = "[REDACTED]";

/// Identifier (lowercased) that names a secret value.
fn is_sensitive_key(id: &str) -> bool {
    // Exact names: short Silo query parameters (st = signed stream ref, sig = artwork signature) and PINs.
    matches!(id, "pin" | "st" | "sig" | "ticket" | "code_verifier")
        || [
            "token",
            "password",
            "passwd",
            "authorization",
            "api_key",
            "apikey",
            "api-key",
            "secret",
            "cookie",
            "ticket",
        ]
        .iter()
        .any(|s| id.ends_with(s))
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn starts_with_ci(b: &[u8], at: usize, pat: &str) -> bool {
    b.get(at..at + pat.len())
        .is_some_and(|w| w.eq_ignore_ascii_case(pat.as_bytes()))
}

/// Remove ANSI CSI sequences (`ESC [ … final`), which would otherwise split `key=value` (review H4).
fn strip_ansi(s: &str) -> Cow<'_, str> {
    if !s.contains('\x1b') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for f in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&f) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

#[derive(Clone, Copy, PartialEq)]
enum Quote {
    None,
    /// `"` or `'`: value ends at the same unescaped quote.
    Plain(u8),
    /// `\"` (a quote inside a Debug-escaped string): value ends at the next backslash.
    Escaped,
}

/// Parse `<sep> <wrappers> <quote> <scheme>` after a sensitive key ending at `j`; returns the value start and quoting.
fn value_start(b: &[u8], mut j: usize) -> Option<(usize, Quote, bool)> {
    // Closing quote of the key itself: `"key"`, `'key'`, `\"key\"`.
    if b.get(j) == Some(&b'\\') && b.get(j + 1) == Some(&b'"') {
        j += 2;
    } else if matches!(b.get(j), Some(b'"' | b'\'')) {
        j += 1;
    }
    while matches!(b.get(j), Some(b' ' | b'\t')) {
        j += 1;
    }
    let encoded = if matches!(b.get(j), Some(b'=' | b':')) {
        j += 1;
        false
    } else if starts_with_ci(b, j, "%3D") || starts_with_ci(b, j, "%3A") {
        j += 3;
        true
    } else {
        return None;
    };
    while matches!(b.get(j), Some(b' ' | b'\t')) {
        j += 1;
    }
    // Debug wrappers and opening quotes: `Some(`, `Ok(`, `Token(`, `"`, `\"`.
    let mut quote = Quote::None;
    for _ in 0..4 {
        match b.get(j) {
            Some(b'\\') if b.get(j + 1) == Some(&b'"') => {
                quote = Quote::Escaped;
                j += 2;
            }
            Some(&q @ (b'"' | b'\'')) => {
                quote = Quote::Plain(q);
                j += 1;
            }
            Some(c) if c.is_ascii_alphabetic() && quote == Quote::None => {
                let mut k = j;
                while b.get(k).is_some_and(|&c| is_ident(c)) {
                    k += 1;
                }
                if b.get(k) == Some(&b'(') {
                    j = k + 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    // Auth scheme ("Bearer   x", "Basic x").
    for scheme in ["bearer", "basic"] {
        if starts_with_ci(b, j, scheme) && matches!(b.get(j + scheme.len()), Some(b' ' | b'\t')) {
            j += scheme.len();
            while matches!(b.get(j), Some(b' ' | b'\t')) {
                j += 1;
            }
        }
    }
    Some((j, quote, encoded))
}

fn value_end(b: &[u8], mut j: usize, quote: Quote, encoded: bool) -> usize {
    while let Some(&c) = b.get(j) {
        let end = match quote {
            Quote::Plain(q) => (c == q && b[j - 1] != b'\\') || c == b'\n' || c == b'\r',
            Quote::Escaped => c == b'\\' || c == b'\n' || c == b'\r',
            Quote::None => {
                c.is_ascii_whitespace()
                    || matches!(c, b'&' | b'"' | b'\'' | b'\\' | b',' | b'}' | b')' | b']' | b';')
                    || (encoded && c == b'%')
            }
        };
        if end {
            break;
        }
        j += 1;
    }
    j
}

/// Replace secret values in `s` (ANSI colour codes are removed first):
/// - `key=value`, `key: value`, `"key":"value"`, `\"key\":\"value\"`, `key: Some("value")`, `key%3Dvalue` where the
///   key names a secret (`Authorization: Bearer x` included);
/// - bare JWTs (`eyJ…` with two dots) and `silo.ticket.<ticket>` WebSocket subprotocols anywhere;
/// - loopback relay route tokens (`/r/<32 hex>/`, playback/relay.rs), which stand in for a signed stream URL.
pub fn redact(input: &str) -> Cow<'_, str> {
    let stripped = strip_ansi(input);
    let s: &str = &stripped;
    let b = s.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mask = |out: &mut Option<String>, copied: &mut usize, from: usize, to: usize| {
        let o = out.get_or_insert_with(String::new);
        o.push_str(&s[*copied..from]);
        o.push_str(MASK);
        *copied = to;
    };
    let mut i = 0;
    while i < b.len() {
        let at_word_start = i == 0 || !(is_ident(b[i - 1]) || b[i - 1] == b'.');
        if !(at_word_start && is_ident(b[i])) {
            i += 1;
            continue;
        }
        // WebSocket ticket subprotocol.
        if starts_with_ci(b, i, "silo.ticket.") {
            let v = i + "silo.ticket.".len();
            let e = value_end(b, v, Quote::None, false);
            if e > v {
                mask(&mut out, &mut copied, v, e);
            }
            i = e.max(i + 1);
            continue;
        }
        let start = i;
        while i < b.len() && is_ident(b[i]) {
            i += 1;
        }
        // Relay route token: `/r/<token>/` with a 128-bit hex token (phase-8 review L4).
        if start >= 3
            && &b[start - 3..start] == b"/r/"
            && i - start == 32
            && b.get(i) == Some(&b'/')
            && b[start..i].iter().all(u8::is_ascii_hexdigit)
        {
            mask(&mut out, &mut copied, start, i);
            continue;
        }
        // Bare JWT: header.payload.signature.
        if b[start..].starts_with(b"eyJ") {
            let mut k = start;
            let mut dots = 0;
            while let Some(&c) = b.get(k) {
                if c == b'.' {
                    dots += 1;
                } else if !is_ident(c) {
                    break;
                }
                k += 1;
            }
            if dots >= 2 {
                mask(&mut out, &mut copied, start, k);
                i = k;
                continue;
            }
        }
        if !is_sensitive_key(&s[start..i].to_ascii_lowercase()) {
            continue;
        }
        let Some((v, quote, encoded)) = value_start(b, i) else {
            continue;
        };
        let e = value_end(b, v, quote, encoded);
        if e > v {
            mask(&mut out, &mut copied, v, e);
        }
        i = e.max(i);
    }
    match out {
        None => stripped,
        Some(mut o) => {
            o.push_str(&s[copied..]);
            Cow::Owned(o)
        }
    }
}

/// `io::Write` adapter that redacts each buffer before passing it on. `tracing-subscriber`'s fmt layer formats a
/// whole event into one buffer and writes it in one call, so secrets are never split across writes.
pub struct RedactingWriter<W: Write>(pub W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        // Never let a redaction bug take the app down from inside a log call (review H2). Release builds abort on
        // panic (phase-7 review M4), so there this only guards debug builds; redact() never slices inside a UTF-8
        // character and its tests try every offset.
        match std::panic::catch_unwind(|| redact(&text).into_owned()) {
            Ok(clean) => self.0.write_all(clean.as_bytes())?,
            Err(_) => self.0.write_all(b"[REDACTION FAILED: log line dropped]\n")?,
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        redact(s).into_owned()
    }

    #[test]
    fn redacts_key_value_forms() {
        assert_eq!(r("GET /x?token=abc123&y=1"), "GET /x?token=[REDACTED]&y=1");
        assert_eq!(
            r(r#"{"access_token":"a.b.c","refresh_token": "zz"}"#),
            r#"{"access_token":"[REDACTED]","refresh_token": "[REDACTED]"}"#
        );
        assert_eq!(
            r("Authorization: Bearer sa_live_123"),
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(r("Authorization:Bearer   sa_x"), "Authorization:Bearer   [REDACTED]");
        assert_eq!(r("X-Profile-Token: p0t"), "X-Profile-Token: [REDACTED]");
        assert_eq!(
            r(r#"Login { username: "me", password: "hunter2" }"#),
            r#"Login { username: "me", password: "[REDACTED]" }"#
        );
        assert_eq!(r("pin=1234 ok"), "pin=[REDACTED] ok");
    }

    #[test]
    fn redacts_debug_shapes_from_review() {
        assert_eq!(
            r(r#"refresh_token: Some("opaque-refresh-abc")"#),
            r#"refresh_token: Some("[REDACTED]")"#
        );
        assert_eq!(
            r(r#"api_key: Some("sa_live_secret")"#),
            r#"api_key: Some("[REDACTED]")"#
        );
        assert_eq!(
            r(r#"profile_token: Token("ptok")"#),
            r#"profile_token: Token("[REDACTED]")"#
        );
        assert_eq!(
            r(r#"body="{\"access_token\":\"opaque123\"}""#),
            r#"body="{\"access_token\":\"[REDACTED]\"}""#
        );
        assert_eq!(r(r#""password":"hun\"ter2""#), r#""password":"[REDACTED]""#);
    }

    #[test]
    fn redacts_silo_specific_credentials() {
        assert_eq!(
            r("GET https://h/api/v2/stream/x?st=abc.def&exp=9"),
            "GET https://h/api/v2/stream/x?st=[REDACTED]&exp=9"
        );
        assert_eq!(
            r("/api/v2/artwork/k?exp=1&sig=deadbeef"),
            "/api/v2/artwork/k?exp=1&sig=[REDACTED]"
        );
        assert_eq!(
            r("Sec-WebSocket-Protocol: silo.events.v2, silo.ticket.T1ck3t"),
            "Sec-WebSocket-Protocol: silo.events.v2, silo.ticket.[REDACTED]"
        );
        assert_eq!(r(r#"{"ticket":"t-1"}"#), r#"{"ticket":"[REDACTED]"}"#);
        assert_eq!(
            r("next=%2Fx%3Ftoken%3Dsecret%26a%3D1"),
            "next=%2Fx%3Ftoken%3D[REDACTED]%26a%3D1"
        );
    }

    #[test]
    fn redacts_bare_jwts() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJ1c2VyX2lkIjoxfQ.sig-_x";
        assert_eq!(r(&format!("refreshed {jwt} done")), "refreshed [REDACTED] done");
    }

    #[test]
    fn redacts_relay_route_tokens() {
        assert_eq!(
            r("mpv loadfile http://127.0.0.1:50123/r/0123456789abcdef0123456789abcdef/master.m3u8"),
            "mpv loadfile http://127.0.0.1:50123/r/[REDACTED]/master.m3u8"
        );
        // Not a route token: wrong length, not hex, or no trailing segment.
        for s in [
            "/r/abc/x",
            "/r/0123456789abcdef0123456789abcdeg/x",
            "/r/0123456789abcdef0123456789abcdef",
        ] {
            assert_eq!(r(s), s);
        }
    }

    #[test]
    fn strips_ansi_before_matching() {
        assert_eq!(
            r("refreshed \x1b[3mtoken\x1b[0m\x1b[2m=\x1b[0mopaque-secret"),
            "refreshed token=[REDACTED]"
        );
    }

    #[test]
    fn leaves_harmless_text_alone() {
        let s = "token_type=access spin=3 code=token_refresh_required expires_in: 28800 X-Silo-Device-Id: 1234";
        assert!(matches!(redact(s), Cow::Borrowed(_)));
        assert_eq!(r("über ünicode token=ä"), "über ünicode token=[REDACTED]");
    }

    #[test]
    fn never_panics_on_unicode_at_any_offset() {
        let keys = [
            "token=",
            "password: \"",
            "Authorization: ",
            "pin=",
            "st=",
            "silo.ticket.",
            "x ",
        ];
        let fill = ["ä", "ö€", "😀", "a", "\"", "\\", "(", "%3D", " "];
        for k in keys {
            for pad in 0..9 {
                for f in fill {
                    let s = format!("{}{k}{}", "é".repeat(pad), f.repeat(pad + 1));
                    let _ = redact(&s);
                    let _ = redact(&s[..s.len() - f.len()]);
                }
            }
        }
        assert_eq!(r("token=äöüäöü"), "token=[REDACTED]");
        assert_eq!(r(r#"password: "ÄÄÄÄ""#), r#"password: "[REDACTED]""#);
    }

    #[test]
    fn writer_redacts() -> io::Result<()> {
        let mut w = RedactingWriter(Vec::new());
        w.write_all(b"password=x\n")?;
        assert_eq!(String::from_utf8_lossy(&w.0), "password=[REDACTED]\n");
        Ok(())
    }
}
