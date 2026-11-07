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

/// Active paths and modes (for a temporary `SetDisplayConfig`). Retries once when the topology changed between the
/// size query and the query itself (R4 §1.4).
fn active_config() -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>), DisplayError> {
    let mut last = String::new();
    for _ in 0..2 {
        // SAFETY: buffers are sized from GetDisplayConfigBufferSizes and truncated to the returned counts.
        unsafe {
            let (mut np, mut nm) = (0u32, 0u32);
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm)
                .ok()
                .map_err(|e| DisplayError::Api(format!("GetDisplayConfigBufferSizes: {e}")))?;
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
            match QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                None,
            )
            .ok()
            {
                Ok(()) => {
                    paths.truncate(np as usize);
                    modes.truncate(nm as usize);
                    return Ok((paths, modes));
                }
                Err(e) => last = e.to_string(),
            }
        }
    }
    Err(DisplayError::Api(format!("QueryDisplayConfig: {last}")))
}

/// `DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE` (16, Windows 11 24H2+), not in `windows` 0.62.2 (R4 §1.3).
const SET_HDR_STATE: DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(16);

/// `DISPLAYCONFIG_SET_HDR_STATE` layout from `wingdi.h`: header + a bit field (bit 0 = enableHdr).
#[repr(C)]
struct SetHdrState {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    bits: u32,
}

/// Width and height of the display's current mode.
fn current_resolution(gdi_name: &str) -> Option<(u32, u32)> {
    let name = wide(gdi_name);
    let mut dm = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: DEVMODEW sized via dmSize; the NUL-terminated name outlives the call.
    let ok = unsafe { EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool() };
    ok.then_some((dm.dmPelsWidth, dm.dmPelsHeight))
}

/// Rational refresh rates DXGI lists for this output at `width`×`height` (fresh factory, R4 §4.1).
fn dxgi_refresh_rates(gdi_name: &str, width: u32, height: u32) -> Vec<Rational> {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
    let mut out = Vec::new();
    // SAFETY: COM calls on interfaces owned here; the mode buffer is sized from the first GetDisplayModeList1 call.
    unsafe {
        let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut ai = 0;
        while let Ok(a) = f.EnumAdapters1(ai) {
            let mut oi = 0;
            while let Ok(o) = a.EnumOutputs(oi) {
                oi += 1;
                let Ok(o6) = o.cast::<IDXGIOutput6>() else { continue };
                if o6.GetDesc1().map(|d| wstr(&d.DeviceName)).ok().as_deref() != Some(gdi_name) {
                    continue;
                }
                let mut n = 0u32;
                if o6
                    .GetDisplayModeList1(DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_ENUM_MODES(0), &mut n, None)
                    .is_err()
                    || n == 0
                {
                    continue;
                }
                let mut modes = vec![DXGI_MODE_DESC1::default(); n as usize];
                if o6
                    .GetDisplayModeList1(
                        DXGI_FORMAT_R8G8B8A8_UNORM,
                        DXGI_ENUM_MODES(0),
                        &mut n,
                        Some(modes.as_mut_ptr()),
                    )
                    .is_err()
                {
                    continue;
                }
                modes.truncate(n as usize);
                for m in modes.iter().filter(|m| m.Width == width && m.Height == height) {
                    let r = Rational::new(m.RefreshRate.Numerator, m.RefreshRate.Denominator.max(1));
                    out.push(r);
                }
            }
            ai += 1;
        }
    }
    out
}

/// Whole-hertz refresh rates `EnumDisplaySettingsExW` lists at `width`×`height` (fallback when DXGI lists none).
fn gdi_refresh_rates(gdi_name: &str, width: u32, height: u32) -> Vec<Rational> {
    let name = wide(gdi_name);
    let mut out = Vec::new();
    let mut i = 0u32;
    loop {
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        // SAFETY: DEVMODEW sized via dmSize; the name outlives the call; enumeration ends when it returns FALSE.
        let ok = unsafe {
            EnumDisplaySettingsExW(
                PCWSTR(name.as_ptr()),
                ENUM_DISPLAY_SETTINGS_MODE(i),
                &mut dm,
                ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
            .as_bool()
        };
        if !ok {
            break;
        }
        if dm.dmPelsWidth == width && dm.dmPelsHeight == height && dm.dmDisplayFrequency > 1 {
            out.push(Rational::new(dm.dmDisplayFrequency, 1));
        }
        i += 1;
    }
    out
}

/// Distinct rates (within 0.01 Hz), sorted ascending.
fn dedup_rates(mut v: Vec<Rational>) -> Vec<Rational> {
    v.sort_by(|a, b| a.hz().total_cmp(&b.hz()));
    v.dedup_by(|a, b| (a.hz() - b.hz()).abs() < 0.01);
    v
}

/// GDI device name (`\\.\DISPLAY1`) of the monitor that holds most of the window.
pub fn gdi_name_for_window(hwnd: isize) -> Option<String> {
    use windows::Win32::Foundation::HWND;
    // SAFETY: MonitorFromWindow tolerates any HWND value (DEFAULTTONEAREST); MONITORINFOEXW is sized via cbSize.
    unsafe {
        let mon = MonitorFromWindow(HWND(hwnd as *mut core::ffi::c_void), MONITOR_DEFAULTTONEAREST);
        if mon.is_invalid() {
            return None;
        }
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        GetMonitorInfoW(mon, &mut mi.monitorInfo)
            .as_bool()
            .then(|| wstr(&mi.szDevice))
    }
}

/// Temporary rational refresh change through `SetDisplayConfig` (never saved to the database, R4 §4.1): first with
/// the current target mode's `vSyncFreq` replaced, then letting the OS pick the timing for the path's
/// `refreshRate`. Validated before applying, read back afterwards.
fn set_refresh_rational(id: &DisplayId, refresh: Rational) -> Result<(), DisplayError> {
    let (paths, modes) = active_config()?;
    let Some(pi) = paths.iter().position(|p| {
        p.targetInfo.adapterId.LowPart == id.adapter_luid_low
            && p.targetInfo.adapterId.HighPart == id.adapter_luid_high
            && p.targetInfo.id == id.target_id
    }) else {
        return Err(DisplayError::NotFound(id.clone()));
    };
    let want = DISPLAYCONFIG_RATIONAL {
        Numerator: refresh.numerator,
        Denominator: refresh.denominator.max(1),
    };
    let mut attempts: Vec<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>, &str)> = Vec::new();
    // SAFETY: reading/writing plain-data unions of structures QueryDisplayConfig filled in.
    unsafe {
        let idx = paths[pi].targetInfo.Anonymous.modeInfoIdx as usize;
        if idx < modes.len() && modes[idx].infoType == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET {
            let (mut p, mut m) = (paths.clone(), modes.clone());
            p[pi].targetInfo.refreshRate = want;
            m[idx].Anonymous.targetMode.targetVideoSignalInfo.vSyncFreq = want;
            attempts.push((p, m, "target mode vSyncFreq"));
        }
        let mut p = paths.clone();
        p[pi].targetInfo.refreshRate = want;
        p[pi].targetInfo.Anonymous.modeInfoIdx = DISPLAYCONFIG_PATH_MODE_IDX_INVALID;
        attempts.push((p, modes.clone(), "path refreshRate"));
    }
    let mut last = String::from("no attempt");
    for (p, m, how) in attempts {
        for extra in [SET_DISPLAY_CONFIG_FLAGS(0), SDC_ALLOW_CHANGES] {
            // SAFETY: arrays come from QueryDisplayConfig (modified only in plain-data fields) and outlive the call.
            let v = unsafe {
                SetDisplayConfig(
                    Some(&p),
                    Some(&m),
                    SDC_VALIDATE | SDC_USE_SUPPLIED_DISPLAY_CONFIG | extra,
                )
            };
            if v != 0 {
                last = format!("{how}: validate rc={v}");
                continue;
            }
            // SAFETY: as above.
            let r =
                unsafe { SetDisplayConfig(Some(&p), Some(&m), SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | extra) };
            if r != 0 {
                last = format!("{how}: apply rc={r}");
                continue;
            }
            let now = WindowsDisplayController.display(id)?.refresh;
            if ((now.hz() - refresh.hz()) / refresh.hz()).abs() <= 0.0005 {
                tracing::info!(how, hz = now.hz(), "refresh switched (SetDisplayConfig)");
                return Ok(());
            }
            last = format!("{how}: read back {:.3} Hz", now.hz());
        }
    }
    Err(DisplayError::ModeRejected(last))
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

    /// `SET_HDR_STATE` (packet 16, Windows 11 24H2+) first: on machines with Auto Color Management the legacy bit may
    /// toggle WCG instead of HDR (R4 §1.5). Falls back to `SET_ADVANCED_COLOR_STATE` (10) when 16 is refused.
    fn set_hdr(&self, id: &DisplayId, enable: bool) -> Result<(), DisplayError> {
        let s16 = SetHdrState {
            header: header::<SetHdrState>(SET_HDR_STATE, luid(id), id.target_id),
            bits: u32::from(enable),
        };
        // SAFETY: correctly sized, initialised packet (wingdi.h layout); a non-zero return only means "unsupported".
        let rc16 = unsafe { DisplayConfigSetDeviceInfo(&s16.header) };
        if rc16 == 0 {
            // Some drivers accept packet 16 without acting on it (seen on Windows 11 26200 / Intel Arc 140V):
            // accept it only if the state reads back within 1.5 s, else use the legacy packet.
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_millis(1500) {
                if self.display(id).is_ok_and(|d| d.hdr_enabled == enable) {
                    tracing::debug!(enable, "HDR set via SET_HDR_STATE");
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            tracing::debug!(
                enable,
                "SET_HDR_STATE accepted but not applied; using SET_ADVANCED_COLOR_STATE"
            );
        }
        tracing::debug!(rc16, "SET_HDR_STATE refused; using SET_ADVANCED_COLOR_STATE");
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

    /// Temporary refresh change: exact rational via `SetDisplayConfig` first; whole-hertz `CDS_FULLSCREEN` (never
    /// persisted, verified in the Phase 2 spike) as the fallback for integer rates.
    fn set_refresh(&self, id: &DisplayId, gdi_name: &str, refresh: Rational) -> Result<(), DisplayError> {
        match set_refresh_rational(id, refresh) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let whole = f64::from(refresh.whole_hz());
                if (refresh.hz() - whole).abs() > 0.001 {
                    return Err(e);
                }
                tracing::debug!(error = %e, "SetDisplayConfig refresh change failed; trying whole-Hz CDS");
            }
        }
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

    /// Back to the persisted configuration: `SetDisplayConfig(SDC_APPLY | SDC_USE_DATABASE_CURRENT)` (Granary never
    /// saves to the database), then the CDS reset for this device, which also undoes a `CDS_FULLSCREEN` change.
    fn restore_mode(&self, gdi_name: &str) -> Result<(), DisplayError> {
        // SAFETY: no arrays; the flags select the persisted database configuration.
        let rc = unsafe { SetDisplayConfig(None, None, SDC_APPLY | SDC_USE_DATABASE_CURRENT) };
        if rc != 0 {
            tracing::debug!(rc, "SetDisplayConfig(USE_DATABASE_CURRENT) failed; CDS reset only");
        }
        let name = wide(gdi_name);
        // SAFETY: a null DEVMODE with flags 0 restores the registry mode of this device.
        let r = unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), None, None, CDS_TYPE(0), None) };
        if r == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(DisplayError::Api(format!("restore mode: {}", r.0)))
        }
    }

    fn modes(&self, display: &DisplayInfo) -> Result<Vec<Rational>, DisplayError> {
        Ok(Self::refresh_rates(&display.gdi_name))
    }
}

impl WindowsDisplayController {
    /// Refresh rates at the current resolution: DXGI rationals, or GDI whole-hertz modes when DXGI lists none.
    pub fn refresh_rates(gdi_name: &str) -> Vec<Rational> {
        let Some((w, h)) = current_resolution(gdi_name) else {
            return Vec::new();
        };
        let dx = dxgi_refresh_rates(gdi_name, w, h);
        dedup_rates(if dx.is_empty() {
            gdi_refresh_rates(gdi_name, w, h)
        } else {
            dx
        })
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

    #[test]
    fn mode_list_is_sorted_and_distinct() {
        let v = dedup_rates(vec![
            Rational::new(60, 1),
            Rational::new(120_000, 1001),
            Rational::new(60_000, 1000),
            Rational::new(24, 1),
        ]);
        assert_eq!(
            v,
            vec![Rational::new(24, 1), Rational::new(60, 1), Rational::new(120_000, 1001)]
        );
        // Read-only query: every listed rate is positive.
        for d in WindowsDisplayController.displays().unwrap_or_default() {
            assert!(
                WindowsDisplayController::refresh_rates(&d.gdi_name)
                    .iter()
                    .all(|r| r.hz() > 0.0)
            );
        }
    }
}
