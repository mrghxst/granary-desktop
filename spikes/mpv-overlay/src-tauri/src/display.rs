//! Spike: per-display HDR state, SDR white level and DXGI luminance (read), plus HDR toggle (legacy packet).
use serde::Serialize;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{HWND, LUID};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW,
    MonitorFromWindow,
};
use windows::core::Interface;

#[derive(Debug, Clone, Serialize)]
pub struct DisplayInfo {
    pub gdi_name: String,
    pub friendly_name: String,
    pub adapter_luid: (u32, i32),
    pub target_id: u32,
    pub refresh_hz: f64,
    pub refresh_rational: (u32, u32),
    pub hdr_supported: bool,
    pub hdr_enabled: bool,
    pub wide_color_enforced: bool,
    pub bits_per_color: i32,
    pub sdr_white_nits: f64,
    pub dxgi_max_luminance: f32,
    pub dxgi_max_full_frame_luminance: f32,
    pub dxgi_min_luminance: f32,
    pub dxgi_color_space: i32,
    pub dxgi_bits_per_color: u32,
    pub desktop_rect: (i32, i32, i32, i32),
}

fn wstr(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

struct Dxgi {
    name: String,
    desc: DXGI_OUTPUT_DESC1,
}

fn dxgi_outputs() -> Vec<Dxgi> {
    let mut v = Vec::new();
    // SAFETY: plain COM calls on interfaces we own; errors end the enumeration.
    unsafe {
        let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return v;
        };
        let mut ai = 0;
        while let Ok(a) = f.EnumAdapters1(ai) {
            let mut oi = 0;
            while let Ok(o) = a.EnumOutputs(oi) {
                if let Ok(o6) = o.cast::<IDXGIOutput6>()
                    && let Ok(d) = o6.GetDesc1()
                {
                    v.push(Dxgi {
                        name: wstr(&d.DeviceName),
                        desc: d,
                    });
                }
                oi += 1;
            }
            ai += 1;
        }
    }
    v
}

fn header<T>(
    t: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    luid: LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: t,
        size: std::mem::size_of::<T>() as u32,
        adapterId: luid,
        id,
    }
}

pub fn active_paths() -> Vec<DISPLAYCONFIG_PATH_INFO> {
    // SAFETY: buffers are sized from GetDisplayConfigBufferSizes and truncated to the returned counts.
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return vec![];
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return vec![];
        }
        paths.truncate(np as usize);
        paths
    }
}

pub fn displays() -> Vec<DisplayInfo> {
    let dx = dxgi_outputs();
    let mut out = Vec::new();
    for p in active_paths() {
        // SAFETY: each packet is a correctly sized, typed struct whose header we initialise.
        unsafe {
            let mut sn = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    p.sourceInfo.adapterId,
                    p.sourceInfo.id,
                ),
                ..Default::default()
            };
            DisplayConfigGetDeviceInfo(&mut sn.header);
            let mut tn = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                header: header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    p.targetInfo.adapterId,
                    p.targetInfo.id,
                ),
                ..Default::default()
            };
            DisplayConfigGetDeviceInfo(&mut tn.header);
            let mut ac = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
                header: header::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                    p.targetInfo.adapterId,
                    p.targetInfo.id,
                ),
                ..Default::default()
            };
            DisplayConfigGetDeviceInfo(&mut ac.header);
            let bits = ac.Anonymous.value;
            let mut sw = DISPLAYCONFIG_SDR_WHITE_LEVEL {
                header: header::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                    p.targetInfo.adapterId,
                    p.targetInfo.id,
                ),
                ..Default::default()
            };
            DisplayConfigGetDeviceInfo(&mut sw.header);
            let gdi = wstr(&sn.viewGdiDeviceName);
            let d = dx.iter().find(|d| d.name == gdi);
            let rr = p.targetInfo.refreshRate;
            out.push(DisplayInfo {
                friendly_name: wstr(&tn.monitorFriendlyDeviceName),
                adapter_luid: (
                    p.targetInfo.adapterId.LowPart,
                    p.targetInfo.adapterId.HighPart,
                ),
                target_id: p.targetInfo.id,
                refresh_hz: rr.Numerator as f64 / rr.Denominator.max(1) as f64,
                refresh_rational: (rr.Numerator, rr.Denominator),
                hdr_supported: bits & 1 != 0,
                hdr_enabled: bits & 2 != 0,
                wide_color_enforced: bits & 4 != 0,
                bits_per_color: ac.bitsPerColorChannel as i32,
                sdr_white_nits: sw.SDRWhiteLevel as f64 / 1000.0 * 80.0,
                dxgi_max_luminance: d.map(|d| d.desc.MaxLuminance).unwrap_or(0.0),
                dxgi_max_full_frame_luminance: d
                    .map(|d| d.desc.MaxFullFrameLuminance)
                    .unwrap_or(0.0),
                dxgi_min_luminance: d.map(|d| d.desc.MinLuminance).unwrap_or(0.0),
                dxgi_color_space: d.map(|d| d.desc.ColorSpace.0).unwrap_or(-1),
                dxgi_bits_per_color: d.map(|d| d.desc.BitsPerColor).unwrap_or(0),
                desktop_rect: d
                    .map(|d| {
                        let r = d.desc.DesktopCoordinates;
                        (r.left, r.top, r.right, r.bottom)
                    })
                    .unwrap_or_default(),
                gdi_name: gdi,
            });
        }
    }
    out
}

/// Enable/disable Windows HDR (advanced color) on one target using the documented legacy packet.
pub fn set_hdr(luid: (u32, i32), target_id: u32, enable: bool) -> Result<(), String> {
    let mut s = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE {
        header: header::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>(
            DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
            LUID {
                LowPart: luid.0,
                HighPart: luid.1,
            },
            target_id,
        ),
        ..Default::default()
    };
    s.Anonymous.value = u32::from(enable);
    // SAFETY: correctly sized and initialised packet.
    let rc = unsafe { DisplayConfigSetDeviceInfo(&s.header) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("DisplayConfigSetDeviceInfo rc={rc}"))
    }
}

/// GDI device name (e.g. `\\.\DISPLAY1`) of the monitor the window is on.
pub fn monitor_of(hwnd: HWND) -> (HMONITOR, String) {
    // SAFETY: MonitorFromWindow never fails with DEFAULTTONEAREST; MONITORINFOEXW is sized correctly.
    unsafe {
        let hm = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let _ = GetMonitorInfoW(hm, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO);
        (hm, wstr(&mi.szDevice))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Temporarily set a whole-hertz refresh rate at the current resolution (CDS_FULLSCREEN = not persisted).
pub fn set_refresh_hz(gdi_name: &str, hz: u32) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::*;
    let name = wide(gdi_name);
    // SAFETY: DEVMODEW is sized via dmSize; the name buffer is NUL-terminated and outlives the calls.
    unsafe {
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if !EnumDisplaySettingsW(
            windows::core::PCWSTR(name.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut dm,
        )
        .as_bool()
        {
            return Err("EnumDisplaySettingsW failed".into());
        }
        dm.dmDisplayFrequency = hz;
        dm.dmFields = DM_DISPLAYFREQUENCY | DM_PELSWIDTH | DM_PELSHEIGHT;
        let test = ChangeDisplaySettingsExW(
            windows::core::PCWSTR(name.as_ptr()),
            Some(&dm),
            None,
            CDS_TEST,
            None,
        );
        if test != DISP_CHANGE_SUCCESSFUL {
            return Err(format!("mode {hz} Hz rejected by CDS_TEST: {}", test.0));
        }
        let r = ChangeDisplaySettingsExW(
            windows::core::PCWSTR(name.as_ptr()),
            Some(&dm),
            None,
            CDS_FULLSCREEN,
            None,
        );
        if r == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(format!("ChangeDisplaySettingsExW: {}", r.0))
        }
    }
}

/// Restore the registry (persisted) mode of one display after a temporary change.
pub fn restore_registry_mode(gdi_name: &str) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::*;
    let name = wide(gdi_name);
    // SAFETY: a null DEVMODE with flags 0 means "restore the registry settings" for this device.
    let r = unsafe {
        ChangeDisplaySettingsExW(
            windows::core::PCWSTR(name.as_ptr()),
            None,
            None,
            CDS_TYPE(0),
            None,
        )
    };
    if r == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        Err(format!("restore: {}", r.0))
    }
}
