//! Player core for Granary (ADR-0002): a small safe layer over `libmpv2-sys`, the [`PlayerBackend`] seam, the
//! desktop-tuned option set and a pure state machine fed by mpv events.

pub mod backend;
pub mod events;
pub mod hevc;
pub mod mkv;
pub mod options;
pub mod range;
pub mod state;

#[cfg(windows)]
pub mod mpv;
#[cfg(windows)]
pub mod probe;
#[cfg(windows)]
pub mod thumbs;

pub use backend::{MockBackend, PlayerBackend};
pub use events::{EndReason, MpvEvent, PropValue};
pub use state::{Phase, PlayerState};

/// Errors from the player layer.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlayerError {
    #[error("libmpv is not available: {0}")]
    Unavailable(String),
    #[error("mpv error {code} ({message}) during {context}")]
    Mpv {
        code: i32,
        message: String,
        context: String,
    },
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}
