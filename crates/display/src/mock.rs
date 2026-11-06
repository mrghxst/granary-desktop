//! In-memory [`DisplayController`] for tests (no hardware, deterministic).
use crate::marker::MarkerEntry;
use crate::types::DXGI_COLOR_SPACE_HDR10;
use crate::{DisplayController, DisplayError, DisplayId, DisplayInfo, Rational};
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct MockDisplayController {
    state: Mutex<Vec<DisplayInfo>>,
    registry_refresh: Mutex<Vec<(String, Rational)>>,
    fail_set_hdr: Mutex<bool>,
}

impl MockDisplayController {
    /// One HDR-capable display with the given state.
    pub fn single(hdr_enabled: bool, refresh: Rational) -> Self {
        let d = DisplayInfo {
            id: DisplayId {
                adapter_luid_low: 0x10956,
                adapter_luid_high: 0,
                target_id: 1,
            },
            gdi_name: "\\\\.\\DISPLAY1".into(),
            friendly_name: "Mock OLED".into(),
            monitor_path: r"\\?\DISPLAY#MOCK0001#1".into(),
            refresh,
            hdr_supported: true,
            hdr_enabled,
            wide_color_enforced: false,
            bits_per_color: 10,
            sdr_white_nits: if hdr_enabled { 200.0 } else { 80.0 },
            max_luminance: 1000.0,
            max_full_frame_luminance: 250.0,
            min_luminance: 0.0001,
            dxgi_color_space: if hdr_enabled { DXGI_COLOR_SPACE_HDR10 } else { 0 },
            desktop_rect: (0, 0, 3840, 2160),
        };
        Self::with(vec![d])
    }

    pub fn with(displays: Vec<DisplayInfo>) -> Self {
        let reg = displays.iter().map(|d| (d.gdi_name.clone(), d.refresh)).collect();
        Self {
            state: Mutex::new(displays),
            registry_refresh: Mutex::new(reg),
            fail_set_hdr: Mutex::new(false),
        }
    }

    /// Id of the first display.
    pub fn id(&self) -> DisplayId {
        self.lock_state().first().map(|d| d.id.clone()).unwrap_or(DisplayId {
            adapter_luid_low: 0,
            adapter_luid_high: 0,
            target_id: 0,
        })
    }

    /// Marker entry describing the first display's current state.
    pub fn entry(&self, hdr_changed: bool, refresh_changed: bool) -> MarkerEntry {
        let s = self.lock_state();
        let d = &s[0];
        MarkerEntry {
            id: d.id.clone(),
            gdi_name: d.gdi_name.clone(),
            monitor_path: d.monitor_path.clone(),
            original_hdr: d.hdr_enabled,
            hdr_changed,
            original_refresh: d.refresh,
            refresh_changed,
        }
    }

    pub fn fail_set_hdr(&self, fail: bool) {
        if let Ok(mut f) = self.fail_set_hdr.lock() {
            *f = fail;
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, Vec<DisplayInfo>> {
        // A poisoned lock only happens after a panic in another test thread; recover the data.
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl DisplayController for MockDisplayController {
    fn displays(&self) -> Result<Vec<DisplayInfo>, DisplayError> {
        Ok(self.lock_state().clone())
    }

    fn set_hdr(&self, id: &DisplayId, enable: bool) -> Result<(), DisplayError> {
        if self.fail_set_hdr.lock().map(|f| *f).unwrap_or(false) {
            return Err(DisplayError::Api("mock: set_hdr failure".into()));
        }
        let mut s = self.lock_state();
        let d = s
            .iter_mut()
            .find(|d| &d.id == id)
            .ok_or_else(|| DisplayError::NotFound(id.clone()))?;
        d.hdr_enabled = enable;
        d.dxgi_color_space = if enable { DXGI_COLOR_SPACE_HDR10 } else { 0 };
        d.sdr_white_nits = if enable { 200.0 } else { 80.0 };
        Ok(())
    }

    fn set_refresh(&self, id: &DisplayId, _gdi_name: &str, refresh: Rational) -> Result<(), DisplayError> {
        let mut s = self.lock_state();
        let d = s
            .iter_mut()
            .find(|d| &d.id == id)
            .ok_or_else(|| DisplayError::NotFound(id.clone()))?;
        d.refresh = refresh;
        Ok(())
    }

    fn restore_mode(&self, gdi_name: &str) -> Result<(), DisplayError> {
        let reg = self
            .registry_refresh
            .lock()
            .map_err(|_| DisplayError::Api("mock registry poisoned".into()))?
            .iter()
            .find(|(n, _)| n == gdi_name)
            .map(|(_, r)| *r)
            .ok_or_else(|| DisplayError::Api(format!("unknown device {gdi_name}")))?;
        for d in self.lock_state().iter_mut().filter(|d| d.gdi_name == gdi_name) {
            d.refresh = reg;
        }
        Ok(())
    }
}
