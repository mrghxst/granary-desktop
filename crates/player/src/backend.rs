//! The [`PlayerBackend`] seam (ADR-0002): the in-process libmpv backend implements it, tests use [`MockBackend`].
use crate::PlayerError;
use std::sync::Mutex;

pub trait PlayerBackend: Send + Sync {
    /// Run an mpv command (`loadfile`, `seek`, `stop`, ...).
    fn command(&self, args: &[&str]) -> Result<(), PlayerError>;
    fn set_property(&self, name: &str, value: &str) -> Result<(), PlayerError>;
    fn get_property(&self, name: &str) -> Option<String>;

    /// Load a URL or path, optionally starting at `start` seconds.
    fn load(&self, url: &str, start: Option<f64>) -> Result<(), PlayerError> {
        if url.is_empty() {
            return Err(PlayerError::InvalidArgument("empty url".into()));
        }
        match start.filter(|s| s.is_finite() && *s > 0.0) {
            // loadfile <url> <flags> <index> <options> (mpv >= 0.38 argument order)
            Some(s) => self.command(&["loadfile", url, "replace", "-1", &format!("start={s:.3}")]),
            None => self.command(&["loadfile", url, "replace"]),
        }
    }

    fn set_pause(&self, paused: bool) -> Result<(), PlayerError> {
        self.set_property("pause", if paused { "yes" } else { "no" })
    }

    fn seek_absolute(&self, seconds: f64) -> Result<(), PlayerError> {
        if !seconds.is_finite() {
            return Err(PlayerError::InvalidArgument("non-finite seek target".into()));
        }
        // Precise (hr-seek default for absolute seeks): `+keyframes` snaps back to the previous keyframe, so with long
        // GOPs a relative +10 s from the UI could land seconds behind where it started.
        self.command(&["seek", &format!("{:.3}", seconds.max(0.0)), "absolute"])
    }

    fn stop(&self) -> Result<(), PlayerError> {
        self.command(&["stop"])
    }
}

/// Records every call; `get_property` answers from a preset table.
#[derive(Debug, Default)]
pub struct MockBackend {
    pub calls: Mutex<Vec<String>>,
    pub properties: Mutex<Vec<(String, String)>>,
}

impl MockBackend {
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    fn record(&self, s: String) {
        if let Ok(mut c) = self.calls.lock() {
            c.push(s);
        }
    }
}

impl PlayerBackend for MockBackend {
    fn command(&self, args: &[&str]) -> Result<(), PlayerError> {
        self.record(format!("cmd {}", args.join(" ")));
        Ok(())
    }

    fn set_property(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        self.record(format!("set {name}={value}"));
        if let Ok(mut p) = self.properties.lock() {
            p.retain(|(n, _)| n != name);
            p.push((name.into(), value.into()));
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> Option<String> {
        self.properties
            .lock()
            .ok()?
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_with_and_without_start() -> Result<(), PlayerError> {
        let m = MockBackend::default();
        m.load("https://h/v.mkv", None)?;
        m.load("https://h/v.mkv", Some(61.25))?;
        m.load("https://h/v.mkv", Some(f64::NAN))?;
        assert_eq!(
            m.calls(),
            [
                "cmd loadfile https://h/v.mkv replace",
                "cmd loadfile https://h/v.mkv replace -1 start=61.250",
                "cmd loadfile https://h/v.mkv replace",
            ]
        );
        assert!(m.load("", None).is_err());
        Ok(())
    }

    #[test]
    fn pause_seek_stop() -> Result<(), PlayerError> {
        let m = MockBackend::default();
        m.set_pause(true)?;
        m.seek_absolute(-3.0)?;
        m.stop()?;
        assert!(m.seek_absolute(f64::INFINITY).is_err());
        assert_eq!(m.calls(), ["set pause=yes", "cmd seek 0.000 absolute", "cmd stop"]);
        assert_eq!(m.get_property("pause").as_deref(), Some("yes"));
        Ok(())
    }
}
