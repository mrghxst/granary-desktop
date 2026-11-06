//! Windows implementation: DisplayConfig (HDR state, SDR white, refresh, names) + DXGI (luminance, colour space).
//! Validated in the Phase 2 spike on Windows 11 26200 (docs/spikes/mpv-overlay.md).
use crate::types::{DisplayId, DisplayInfo, Rational};
use crate::{DisplayController, DisplayError};
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::LUID;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::core::{Interface, PCWSTR};

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsDisplayController;

fn wstr(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct DxgiOutput {
    name: String,
    desc: DXGI_OUTPUT_DESC1,
}

/// Enumerate DXGI outputs with a **fresh factory** each time: a factory caches output descriptions, and the
/// spike showed luminance values change across HDR toggles.
fn dxgi_outputs() -> Vec<DxgiOutput> {
    let mut v = Vec::new();
    // SAFETY: plain COM calls on interfaces owned by this function; any error ends the enumeration.
    unsafe {
        let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return v;
        };
        let mut ai = 0;
        while let Ok(a) = f.EnumAdapters1(ai) {
            let mut oi = 0;
            while let Ok(o) = a.EnumOutputs(oi) {
                if let Ok(desc) = o.cast::<IDXGIOutput6>().and_then(|o6| o6.GetDesc1()) {
                    v.push(DxgiOutput {
                        name: wstr(&desc.DeviceName),
                        desc,
                    });
                }
                oi += 1;
            }
            ai += 1;
        }
    }
    v
}

fn header<T>(t: DISPLAYCONFIG_DEVICE_INFO_TYPE, luid: LUID, id: u32) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: t,
        size: std::mem::size_of::<T>() as u32,
        adapterId: luid,
        id,
    }
}

fn active_paths() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>, DisplayError> {
    // SAFETY: buffers are sized from GetDisplayConfigBufferSizes and truncated to the counts QueryDisplayConfig returns.
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm)
            .ok()
            .map_err(|e| DisplayError::Api(format!("GetDisplayConfigBufferSizes: {e}")))?;
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        )
        .ok()
        .map_err(|e| DisplayError::Api(format!("QueryDisplayConfig: {e}")))?;
        paths.truncate(np as usize);
        Ok(paths)
    }
}

fn luid(id: &DisplayId) -> LUID {
    LUID {
        LowPart: id.adapter_luid_low,
        HighPart: id.adapter_luid_high,
    }
}

impl DisplayController for WindowsDisplayController {
    fn displays(&self) -> Result<Vec<DisplayInfo>, DisplayError> {
        let dx = dxgi_outputs();
        let mut out = Vec::new();
        for p in active_paths()? {
            let (src, tgt) = (&p.sourceInfo, &p.targetInfo);
            // SAFETY: each packet is a correctly sized, typed struct whose header is initialised for this path.
            let (gdi, friendly, path, bits, bpc, sdr_raw) = unsafe {
                let mut sn = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                    header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                        src.adapterId,
                        src.id,
                    ),
                    ..Default::default()
                };
                DisplayConfigGetDeviceInfo(&mut sn.header);
                let mut tn = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                    header: header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                        DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                        tgt.adapterId,
                        tgt.id,
                    ),
                    ..Default::default()
                };
                DisplayConfigGetDeviceInfo(&mut tn.header);
                let mut ac = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
                    header: header::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(
                        DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                        tgt.adapterId,
                        tgt.id,
                    ),
                    ..Default::default()
                };
                DisplayConfigGetDeviceInfo(&mut ac.header);
                let mut sw = DISPLAYCONFIG_SDR_WHITE_LEVEL {
                    header: header::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(
                        DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                        tgt.adapterId,
                        tgt.id,
                    ),
                    ..Default::default()
                };
                DisplayConfigGetDeviceInfo(&mut sw.header);
                (
                    wstr(&sn.viewGdiDeviceName),
                    wstr(&tn.monitorFriendlyDeviceName),
                    wstr(&tn.monitorDevicePath),
                    ac.Anonymous.value,
                    ac.bitsPerColorChannel,
                    sw.SDRWhiteLevel,
                )
            };
            let d = dx.iter().find(|d| d.name == gdi);
            let wce = bits & 4 != 0;
            let rr = tgt.refreshRate;
            out.push(DisplayInfo {
                id: DisplayId {
                    adapter_luid_low: tgt.adapterId.LowPart,
                    adapter_luid_high: tgt.adapterId.HighPart,
                    target_id: tgt.id,
                },
                friendly_name: friendly,
                monitor_path: path,
                refresh: Rational::new(rr.Numerator, rr.Denominator),
                hdr_supported: bits & 1 != 0 && !wce,
                hdr_enabled: bits & 2 != 0 && !wce,
                wide_color_enforced: wce,
                bits_per_color: bpc,
                sdr_white_nits: f64::from(sdr_raw) / 1000.0 * 80.0,
                max_luminance: d.map(|d| d.desc.MaxLuminance).unwrap_or(0.0),
                max_full_frame_luminance: d.map(|d| d.desc.MaxFullFrameLuminance).unwrap_or(0.0),
                min_luminance: d.map(|d| d.desc.MinLuminance).unwrap_or(0.0),
                dxgi_color_space: d.map(|d| d.desc.ColorSpace.0).unwrap_or(-1),
                desktop_rect: d
                    .map(|d| {
                        let r = d.desc.DesktopCoordinates;
                        (r.left, r.top, r.right, r.bottom)
                    })
                    .unwrap_or_default(),
                gdi_name: gdi,
            });
        }
        Ok(out)
    }

    /// Legacy `SET_ADVANCED_COLOR_STATE` packet (10); packets 15/16 (Windows 11 24H2+) are added in Phase 7.
    fn set_hdr(&self, id: &DisplayId, enable: bool) -> Result<(), DisplayError> {
        let mut s = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE {
            header: header::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>(
                DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
                luid(id),
                id.target_id,
            ),
            ..Default::default()
        };
        s.Anonymous.value = u32::from(enable);
        // SAFETY: correctly sized and initialised packet for an existing target.
        let rc = unsafe { DisplayConfigSetDeviceInfo(&s.header) };
        if rc == 0 {
            Ok(())
        } else {
            Err(DisplayError::Api(format!("SET_ADVANCED_COLOR_STATE rc={rc}")))
        }
    }

    /// Whole-hertz temporary mode change (`CDS_FULLSCREEN`, never persisted). Fractional rates via
    /// `SetDisplayConfig` are part of Phase 7 (ADR-0006).
    fn set_refresh(&self, _id: &DisplayId, gdi_name: &str, refresh: Rational) -> Result<(), DisplayError> {
        let name = wide(gdi_name);
        // SAFETY: DEVMODEW is sized via dmSize; the NUL-terminated name outlives both calls.
        unsafe {
            let mut dm = DEVMODEW {
                dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                ..Default::default()
            };
            if !EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool() {
                return Err(DisplayError::Api("EnumDisplaySettingsW failed".into()));
            }
            dm.dmDisplayFrequency = refresh.whole_hz();
            dm.dmFields = DM_DISPLAYFREQUENCY | DM_PELSWIDTH | DM_PELSHEIGHT;
            let test = ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&dm), None, CDS_TEST, None);
            if test != DISP_CHANGE_SUCCESSFUL {
                return Err(DisplayError::ModeRejected(format!(
                    "{} Hz on {gdi_name}: {}",
                    refresh.whole_hz(),
                    test.0
                )));
            }
            let r = ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&dm), None, CDS_FULLSCREEN, None);
            if r == DISP_CHANGE_SUCCESSFUL {
                Ok(())
            } else {
                Err(DisplayError::Api(format!("ChangeDisplaySettingsExW: {}", r.0)))
            }
        }
    }

    fn restore_mode(&self, gdi_name: &str) -> Result<(), DisplayError> {
        let name = wide(gdi_name);
        // SAFETY: a null DEVMODE with flags 0 restores the registry mode of this device.
        let r = unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), None, None, CDS_TYPE(0), None) };
        if r == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(DisplayError::Api(format!("restore mode: {}", r.0)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only query against the real machine; every Windows CI runner has at least one (virtual) display path,
    /// but headless runners may have none, so only the call itself is asserted.
    #[test]
    fn query_does_not_fail() {
        let r = WindowsDisplayController.displays();
        assert!(r.is_ok(), "{r:?}");
    }
}
