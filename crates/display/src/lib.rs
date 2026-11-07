//! Display state for Granary (ADR-0006): per-monitor HDR state, SDR white level, DXGI luminance, refresh rate,
//! temporary HDR/refresh changes and the persisted restore marker that guarantees every change is undone.
//!
//! The [`DisplayController`] trait is the seam: [`win::WindowsDisplayController`] talks to DisplayConfig/DXGI,
//! [`mock::MockDisplayController`] backs unit tests of the restore and selection logic.

pub mod marker;
pub mod mock;
pub mod policy;
pub mod power;
pub mod process;
pub mod session;
pub mod types;
#[cfg(windows)]
pub mod win;

pub use types::{DisplayId, DisplayInfo, Rational};

/// Errors from display queries and changes.
#[derive(Debug, thiserror::Error)]
pub enum DisplayError {
    #[error("display {0} not found")]
    NotFound(DisplayId),
    #[error("display API call failed: {0}")]
    Api(String),
    #[error("mode rejected: {0}")]
    ModeRejected(String),
    #[error("restore marker I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("restore marker format: {0}")]
    Format(#[from] serde_json::Error),
}

/// Read and change display state. Implementations must be cheap to query repeatedly.
pub trait DisplayController {
    /// All active displays.
    fn displays(&self) -> Result<Vec<DisplayInfo>, DisplayError>;
    /// Enable or disable Windows HDR (advanced colour) on one display.
    fn set_hdr(&self, id: &DisplayId, enable: bool) -> Result<(), DisplayError>;
    /// Temporarily set a refresh rate at the current resolution (not persisted to the registry).
    fn set_refresh(&self, id: &DisplayId, gdi_name: &str, refresh: Rational) -> Result<(), DisplayError>;
    /// Restore the persisted (registry) mode of one display after a temporary refresh change.
    fn restore_mode(&self, gdi_name: &str) -> Result<(), DisplayError>;
    /// Refresh rates available at the display's current resolution (rational where the driver reports them).
    fn modes(&self, _display: &DisplayInfo) -> Result<Vec<Rational>, DisplayError> {
        Ok(Vec::new())
    }

    /// Convenience: the display with this id.
    fn display(&self, id: &DisplayId) -> Result<DisplayInfo, DisplayError> {
        self.displays()?
            .into_iter()
            .find(|d| &d.id == id)
            .ok_or_else(|| DisplayError::NotFound(id.clone()))
    }
}
