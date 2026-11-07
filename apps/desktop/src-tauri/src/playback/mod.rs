//! Native playback (Phase 6, ADR-0008): the loopback media relay, the per-attempt controller and the host glue
//! that drives libmpv and the overlay.
pub mod commands;
pub mod controller;
pub mod events;
pub mod host;
pub mod output;
#[cfg(windows)]
pub mod platform;
pub mod relay;
#[cfg(windows)]
pub mod video;

/// Client-minted ids (attempt, replan, stop): canonical lowercase UUID v4 strings.
pub fn mint_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
