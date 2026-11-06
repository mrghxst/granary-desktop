//! [`Secret`]: a string whose `Debug` never shows the value (ADR-0004 "Redaction").
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A credential (password, access/refresh token, PIN, profile token). `Debug` prints `Secret(***)`; there is no
/// `Display`, so `format!("{}")` cannot leak it by accident. Serialization is transparent because request bodies
/// must carry the value; never serialize a `Secret` into anything but a request body or the credential store.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value, for building a request or writing the credential store.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_value() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(format!("{:?}", Some(&s)), "Some(Secret(***))");
        assert_eq!(s.expose(), "hunter2");
    }

    #[test]
    fn serde_is_transparent() {
        let s: Secret = serde_json::from_str("\"abc\"").unwrap_or_default();
        assert_eq!(s.expose(), "abc");
        assert_eq!(serde_json::to_string(&s).ok().as_deref(), Some("\"abc\""));
    }
}
