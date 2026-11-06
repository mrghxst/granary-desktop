//! Plain data types shared by the controller implementations and the restore marker.
use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identity of a display target: adapter LUID + DisplayConfig target id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DisplayId {
    pub adapter_luid_low: u32,
    pub adapter_luid_high: i32,
    pub target_id: u32,
}

impl fmt::Display for DisplayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:08x}:{:08x}/{}",
            self.adapter_luid_high, self.adapter_luid_low, self.target_id
        )
    }
}

/// A refresh rate as an exact rational (e.g. 120000/1001 for 119.88 Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rational {
    pub numerator: u32,
    pub denominator: u32,
}

impl Rational {
    pub const fn new(numerator: u32, denominator: u32) -> Self {
        Self { numerator, denominator }
    }

    /// Value in hertz; a zero denominator is treated as 1.
    pub fn hz(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator.max(1))
    }

    /// Nearest whole hertz (what `ChangeDisplaySettingsExW` accepts).
    pub fn whole_hz(self) -> u32 {
        self.hz().round() as u32
    }
}

/// Snapshot of one active display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: DisplayId,
    /// GDI device name, e.g. `\\.\DISPLAY1`.
    pub gdi_name: String,
    pub friendly_name: String,
    /// Monitor device interface path (`\\?\DISPLAY#...`): survives reboots, unlike the adapter LUID.
    pub monitor_path: String,
    pub refresh: Rational,
    /// `advancedColorSupported && !wideColorEnforced` (ACM on an SDR panel is not HDR; R4, review M4).
    pub hdr_supported: bool,
    /// `advancedColorEnabled && !wideColorEnforced`.
    pub hdr_enabled: bool,
    pub wide_color_enforced: bool,
    pub bits_per_color: u32,
    /// Windows "SDR content brightness" in nits (packet 11: raw / 1000 × 80).
    pub sdr_white_nits: f64,
    /// DXGI `MaxLuminance` (small-window peak). Unstable for ~1.5 s after an HDR toggle (Phase 2 spike).
    pub max_luminance: f32,
    pub max_full_frame_luminance: f32,
    pub min_luminance: f32,
    /// DXGI colour space of the output (12 = RGB_FULL_G2084_NONE_P2020 i.e. HDR10, 0 = sRGB).
    pub dxgi_color_space: i32,
    /// Desktop rectangle (left, top, right, bottom) in physical pixels.
    pub desktop_rect: (i32, i32, i32, i32),
}

impl DisplayInfo {
    /// HDR is on and DXGI already reports the PQ/BT.2020 colour space.
    pub fn hdr_active(&self) -> bool {
        self.hdr_enabled && self.dxgi_color_space == DXGI_COLOR_SPACE_HDR10
    }
}

/// `DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020`.
pub const DXGI_COLOR_SPACE_HDR10: i32 = 12;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_hz() {
        assert!((Rational::new(120_000, 1001).hz() - 119.88).abs() < 0.001);
        assert_eq!(Rational::new(120_000, 1001).whole_hz(), 120);
        assert_eq!(Rational::new(60, 0).hz(), 60.0);
    }

    #[test]
    fn display_id_formats_stably() {
        let id = DisplayId {
            adapter_luid_low: 0x10956,
            adapter_luid_high: 0,
            target_id: 8_388_688,
        };
        assert_eq!(id.to_string(), "00000000:00010956/8388688");
    }
}
