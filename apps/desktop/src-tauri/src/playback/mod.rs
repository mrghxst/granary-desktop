//! Native playback (Phase 6, ADR-0008): the loopback media relay, the per-attempt controller and the host glue
//! that drives libmpv and the overlay.
pub mod controller;
pub mod relay;

/// Client-minted ids (attempt, replan, stop): canonical lowercase UUID v4 strings.
pub fn mint_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
