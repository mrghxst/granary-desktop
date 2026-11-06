//! Pure player state machine. mpv events go in, a serialisable snapshot comes out; no FFI, fully unit-tested.
use crate::events::{EndReason, MpvEvent, PropValue};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Nothing loaded.
    Idle,
    /// `loadfile` issued, waiting for FILE_LOADED.
    Loading,
    Playing,
    Paused,
    /// Playing but stalled on network cache (`paused-for-cache`).
    Buffering,
    /// Seek in flight, until PLAYBACK_RESTART.
    Seeking,
    /// Reached the end (`eof-reached` with keep-open, or END_FILE eof).
    Ended,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerState {
    pub phase: Phase,
    /// Seconds.
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub buffering: bool,
    pub volume: f64,
    pub muted: bool,
    pub error: Option<String>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            position: 0.0,
            duration: None,
            paused: false,
            buffering: false,
            volume: 100.0,
            muted: false,
            error: None,
        }
    }
}

/// Properties the state machine needs; the backend observes these with the given formats.
pub const OBSERVED: &[(&str, ObserveFormat)] = &[
    ("pause", ObserveFormat::Flag),
    ("paused-for-cache", ObserveFormat::Flag),
    ("eof-reached", ObserveFormat::Flag),
    ("time-pos", ObserveFormat::Double),
    ("duration", ObserveFormat::Double),
    ("volume", ObserveFormat::Double),
    ("mute", ObserveFormat::Flag),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveFormat {
    Flag,
    Int,
    Double,
    Str,
}

impl PlayerState {
    fn active(&self) -> bool {
        matches!(
            self.phase,
            Phase::Playing | Phase::Paused | Phase::Buffering | Phase::Seeking | Phase::Ended
        )
    }

    fn settled(&self) -> Phase {
        if self.paused {
            Phase::Paused
        } else if self.buffering {
            Phase::Buffering
        } else {
            Phase::Playing
        }
    }

    /// A `loadfile` was issued by the host.
    pub fn load_requested(&mut self) {
        *self = Self {
            phase: Phase::Loading,
            volume: self.volume,
            muted: self.muted,
            ..Self::default()
        };
    }

    /// Apply one event. Returns `true` if the snapshot changed (callers throttle emission on top of this).
    pub fn apply(&mut self, ev: &MpvEvent) -> bool {
        let before = self.clone();
        match ev {
            MpvEvent::StartFile => {
                if self.phase != Phase::Loading {
                    self.load_requested();
                }
            }
            MpvEvent::FileLoaded => {
                if matches!(self.phase, Phase::Loading | Phase::Idle) {
                    self.phase = self.settled();
                }
            }
            MpvEvent::Seek => {
                if self.active() {
                    self.phase = Phase::Seeking;
                }
            }
            MpvEvent::PlaybackRestart => {
                if self.active() || self.phase == Phase::Loading {
                    self.phase = self.settled();
                }
            }
            MpvEvent::EndFile { reason } => match reason {
                EndReason::Eof => self.phase = Phase::Ended,
                EndReason::Error(e) => {
                    self.phase = Phase::Error;
                    self.error = Some(e.clone());
                }
                // A redirect (playlist/ytdl) immediately starts another file.
                EndReason::Redirect => {}
                EndReason::Stop | EndReason::Quit | EndReason::Unknown(_) => {
                    // A stop caused by a new loadfile keeps the Loading phase of the new file.
                    if self.phase != Phase::Loading {
                        self.phase = Phase::Idle;
                    }
                }
            },
            MpvEvent::Shutdown => self.phase = Phase::Idle,
            MpvEvent::Property { name, value } => self.apply_property(name, value),
            MpvEvent::VideoReconfig | MpvEvent::AudioReconfig | MpvEvent::Log { .. } => {}
        }
        *self != before
    }

    fn apply_property(&mut self, name: &str, value: &PropValue) {
        match name {
            "pause" => {
                if let Some(p) = value.as_bool() {
                    self.paused = p;
                    if matches!(self.phase, Phase::Playing | Phase::Paused | Phase::Buffering) {
                        self.phase = self.settled();
                    }
                }
            }
            "paused-for-cache" => {
                if let Some(b) = value.as_bool() {
                    self.buffering = b;
                    if matches!(self.phase, Phase::Playing | Phase::Buffering) {
                        self.phase = self.settled();
                    }
                }
            }
            "eof-reached" => {
                if value.as_bool() == Some(true) && self.active() {
                    self.phase = Phase::Ended;
                } else if value.as_bool() == Some(false) && self.phase == Phase::Ended {
                    self.phase = self.settled();
                }
            }
            "time-pos" => {
                if let Some(t) = value.as_f64() {
                    self.position = t.max(0.0);
                }
            }
            "duration" => self.duration = value.as_f64().filter(|d| d.is_finite() && *d > 0.0),
            "volume" => {
                if let Some(v) = value.as_f64() {
                    self.volume = v;
                }
            }
            "mute" => {
                if let Some(m) = value.as_bool() {
                    self.muted = m;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(name: &str, value: PropValue) -> MpvEvent {
        MpvEvent::Property {
            name: name.into(),
            value,
        }
    }

    fn playing() -> PlayerState {
        let mut s = PlayerState::default();
        s.load_requested();
        s.apply(&MpvEvent::FileLoaded);
        s.apply(&MpvEvent::PlaybackRestart);
        s
    }

    #[test]
    fn load_to_playing() {
        let s = playing();
        assert_eq!(s.phase, Phase::Playing);
    }

    #[test]
    fn pause_and_resume() {
        let mut s = playing();
        assert!(s.apply(&prop("pause", PropValue::Flag(true))));
        assert_eq!(s.phase, Phase::Paused);
        s.apply(&prop("pause", PropValue::Flag(false)));
        assert_eq!(s.phase, Phase::Playing);
    }

    #[test]
    fn starting_paused_stays_paused() {
        let mut s = PlayerState::default();
        s.load_requested();
        s.apply(&prop("pause", PropValue::Flag(true)));
        assert_eq!(s.phase, Phase::Loading);
        s.apply(&MpvEvent::FileLoaded);
        assert_eq!(s.phase, Phase::Paused);
    }

    #[test]
    fn buffering_round_trip() {
        let mut s = playing();
        s.apply(&prop("paused-for-cache", PropValue::Flag(true)));
        assert_eq!(s.phase, Phase::Buffering);
        s.apply(&prop("paused-for-cache", PropValue::Flag(false)));
        assert_eq!(s.phase, Phase::Playing);
    }

    #[test]
    fn seek_until_restart() {
        let mut s = playing();
        s.apply(&MpvEvent::Seek);
        assert_eq!(s.phase, Phase::Seeking);
        s.apply(&MpvEvent::PlaybackRestart);
        assert_eq!(s.phase, Phase::Playing);
    }

    #[test]
    fn eof_with_keep_open_and_seek_back() {
        let mut s = playing();
        s.apply(&prop("eof-reached", PropValue::Flag(true)));
        assert_eq!(s.phase, Phase::Ended);
        s.apply(&prop("eof-reached", PropValue::Flag(false)));
        assert_eq!(s.phase, Phase::Playing);
    }

    #[test]
    fn end_file_error_is_reported() {
        let mut s = PlayerState::default();
        s.load_requested();
        s.apply(&MpvEvent::EndFile {
            reason: EndReason::Error("loading failed".into()),
        });
        assert_eq!(s.phase, Phase::Error);
        assert_eq!(s.error.as_deref(), Some("loading failed"));
        s.load_requested();
        assert_eq!(s.error, None);
    }

    #[test]
    fn stop_from_new_load_keeps_loading() {
        let mut s = playing();
        s.load_requested();
        s.apply(&MpvEvent::EndFile {
            reason: EndReason::Stop,
        });
        assert_eq!(s.phase, Phase::Loading);
        let mut t = playing();
        t.apply(&MpvEvent::EndFile {
            reason: EndReason::Stop,
        });
        assert_eq!(t.phase, Phase::Idle);
    }

    #[test]
    fn position_duration_volume() {
        let mut s = playing();
        s.apply(&prop("time-pos", PropValue::Double(12.5)));
        s.apply(&prop("duration", PropValue::Double(3600.0)));
        s.apply(&prop("volume", PropValue::Double(55.0)));
        s.apply(&prop("mute", PropValue::Flag(true)));
        assert_eq!(
            (s.position, s.duration, s.volume, s.muted),
            (12.5, Some(3600.0), 55.0, true)
        );
        s.apply(&prop("duration", PropValue::None));
        assert_eq!(s.duration, None);
        // volume survives a new load
        s.load_requested();
        assert_eq!(s.volume, 55.0);
    }

    #[test]
    fn unchanged_returns_false() {
        let mut s = playing();
        assert!(!s.apply(&prop("pause", PropValue::Flag(false))));
        assert!(!s.apply(&prop("unrelated", PropValue::Int(3))));
    }
}
