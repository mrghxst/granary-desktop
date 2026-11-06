//! Events produced by the mpv event thread, independent of the FFI types so they can be replayed in tests.
use serde::Serialize;

/// A typed property value (observed with FLAG, DOUBLE, INT64 or STRING format).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PropValue {
    None,
    Flag(bool),
    Int(i64),
    Double(f64),
    Str(String),
}

impl PropValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Flag(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(d) => Some(*d),
            Self::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
}

/// Why a file stopped playing (`mpv_end_file_reason`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Eof,
    Stop,
    Quit,
    Error(String),
    Redirect,
    Unknown(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MpvEvent {
    StartFile,
    FileLoaded,
    PlaybackRestart,
    VideoReconfig,
    AudioReconfig,
    Seek,
    EndFile {
        reason: EndReason,
    },
    Property {
        name: String,
        value: PropValue,
    },
    Log {
        prefix: String,
        level: String,
        text: String,
    },
    Shutdown,
}
