//! Secret redaction for log output (prompt rule: tokens never logged). Applied to every formatted log line by
//! [`RedactingWriter`], so a stray `?request` or error message cannot leak a bearer token, refresh token, profile
//! token, PIN, password or API key.
use std::borrow::Cow;
use std::io::{self, Write};

const MASK: &str = "[REDACTED]";

/// Identifier (lowercased) that names a secret value.
fn is_sensitive_key(id: &str) -> bool {
    id == "pin"
        || [
            "token",
            "password",
            "authorization",
            "api_key",
            "apikey",
            "secret",
            "cookie",
        ]
        .iter()
        .any(|s| id.ends_with(s))
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn is_value_end(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'&' | b'"' | b'\'' | b',' | b'}' | b')' | b']' | b';')
}

/// Replace secret values in `s`:
/// - `key=value`, `key: value`, `"key":"value"` where the key names a secret (`Authorization: Bearer x` included);
/// - bare JWTs (`eyJ…` with two dots) anywhere.
pub fn redact(s: &str) -> Cow<'_, str> {
    let b = s.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0; // bytes of `s` already transferred to `out`
    let mut i = 0;
    while i < b.len() {
        let at_word_start = i == 0 || !is_ident(b[i - 1]);
        if !(at_word_start && is_ident(b[i])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (is_ident(b[i]) || b[i] == b'.') {
            i += 1;
        }
        let word = &s[start..i];

        // Bare JWT.
        if word.starts_with("eyJ") && word.matches('.').count() >= 2 {
            let o = out.get_or_insert_with(String::new);
            o.push_str(&s[copied..start]);
            o.push_str(MASK);
            copied = i;
            continue;
        }

        // Key followed by a separator.
        let key = word.trim_end_matches('.');
        if !is_sensitive_key(&key.to_ascii_lowercase()) {
            continue;
        }
        let mut j = start + key.len();
        if j < b.len() && matches!(b[j], b'"' | b'\'') {
            j += 1;
        }
        while j < b.len() && b[j] == b' ' {
            j += 1;
        }
        if j >= b.len() || !matches!(b[j], b'=' | b':') {
            continue;
        }
        j += 1;
        while j < b.len() && b[j] == b' ' {
            j += 1;
        }
        if j < b.len() && matches!(b[j], b'"' | b'\'') {
            j += 1;
        }
        // Skip an auth scheme word ("Bearer ", "Basic ").
        for scheme in ["bearer ", "basic "] {
            if s.len() >= j + scheme.len() && s[j..j + scheme.len()].eq_ignore_ascii_case(scheme) {
                j += scheme.len();
            }
        }
        let vstart = j;
        while j < b.len() && !is_value_end(b[j]) {
            j += 1;
        }
        if j > vstart {
            let o = out.get_or_insert_with(String::new);
            o.push_str(&s[copied..vstart]);
            o.push_str(MASK);
            copied = j;
        }
        i = j.max(i);
    }
    match out {
        None => Cow::Borrowed(s),
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
        self.0.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_key_value_forms() {
        assert_eq!(redact("GET /x?token=abc123&y=1"), "GET /x?token=[REDACTED]&y=1");
        assert_eq!(
            redact(r#"{"access_token":"a.b.c","refresh_token": "zz"}"#),
            r#"{"access_token":"[REDACTED]","refresh_token": "[REDACTED]"}"#
        );
        assert_eq!(
            redact("Authorization: Bearer sa_live_123"),
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(redact("X-Profile-Token: p0t"), "X-Profile-Token: [REDACTED]");
        assert_eq!(
            redact(r#"Login { username: "me", password: "hunter2" }"#),
            r#"Login { username: "me", password: "[REDACTED]" }"#
        );
        assert_eq!(redact("pin=1234 ok"), "pin=[REDACTED] ok");
    }

    #[test]
    fn redacts_bare_jwts() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJ1c2VyX2lkIjoxfQ.sig-_x";
        assert_eq!(redact(&format!("refreshed {jwt} done")), "refreshed [REDACTED] done");
    }

    #[test]
    fn leaves_harmless_text_alone() {
        let s = "token_type=access spin=3 code=token_refresh_required expires_in: 28800";
        assert!(matches!(redact(s), Cow::Borrowed(_)));
        assert_eq!(redact("über ünicode token=ä"), "über ünicode token=[REDACTED]");
    }

    #[test]
    fn writer_redacts() -> io::Result<()> {
        let mut w = RedactingWriter(Vec::new());
        w.write_all(b"password=x\n")?;
        assert_eq!(String::from_utf8_lossy(&w.0), "password=[REDACTED]\n");
        Ok(())
    }
}
