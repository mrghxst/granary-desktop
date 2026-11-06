# Windows display and audio pipeline for Granary (HDR, refresh rate, mpv/libplacebo, WASAPI)

- **Date:** 2026-11-06. Research agent R4.
- **Versions consulted:**
  - **mpv v0.42.0** (released 2026-11-05). Manual read from `DOCS/man/*.rst` at tag `v0.42.0`; https://mpv.io/manual/stable/ tracks the same release.
  - **libplacebo v7.387.0** (tag dated 2026-10-25). mpv 0.42.0's release notes require libplacebo ≥ 7.387.0 and FFmpeg ≥ 6.1.
  - **`windows` crate 0.62.2**: crates.io `max_stable`, published 2025-10-06. The source was read from the crate tarball.
  - **win32metadata `wingdi.h`**: main branch, latest release v71.0.45-preview.
  - **PresentMon v2.6.0**: console README.
- **How claims are tagged:**
  - **[Doc]**: stated by official documentation.
  - **[Src]**: read from source code at the stated tag.
  - **[Inference]**: my reasoning, not stated by a source.
  - **[Unofficial]**: third-party source only.
- **Constraints:** nothing was changed on this machine's displays. All facts were taken from the sources listed at the end.

## Summary

1. **HDR state and toggle** go through `DisplayConfigGetDeviceInfo` / `DisplayConfigSetDeviceInfo`.
   - The legacy pair is `GET_ADVANCED_COLOR_INFO` (9) and `SET_ADVANCED_COLOR_STATE` (10). Both are in `windows` 0.62.2.
   - The Windows 11 pair is `GET_ADVANCED_COLOR_INFO_2` (15) and `SET_HDR_STATE` (16). They are only in `wingdi.h` and are undocumented on Learn. Granary must declare them by hand with `#[repr(C)]`.
2. **HMONITOR or DXGI output to DisplayConfig path:**
   - Get the GDI name (`\\.\DISPLAYn`) from `MONITORINFOEXW.szDevice` or from `DXGI_OUTPUT_DESC1.DeviceName`.
   - Match it against `DISPLAYCONFIG_SOURCE_DEVICE_NAME.viewGdiDeviceName` for each active path.
   - Then use that path's `targetInfo.adapterId`/`id`. mpv does exactly this.
3. **SDR white:** `DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL` (11), where nits = `SDRWhiteLevel / 1000 * 80`.
   - mpv 0.42 already reads it per monitor (`--hdr-reference-white=auto`, the default).
4. **Panel peak:** `IDXGIOutput6::GetDesc1` → `MaxLuminance` (small area), `MaxFullFrameLuminance`, `MinLuminance`.
   - mpv feeds `MaxLuminance`, not full-frame, to libplacebo. That matches the OLED requirement.
   - Microsoft does not document whether the HDR Calibration profile changes these values.
5. **mpv's default is already correct for "Windows HDR on, stay in HDR":** `vo=gpu-next`, `gpu-api=d3d11`, `target-colorspace-hint=auto`, `target-colorspace-hint-mode=target`.
   - libplacebo creates an RGB10A2 flip swapchain tagged `G2084_NONE_P2020` with HDR10 metadata.
   - It tone-maps only when the source peak exceeds the display peak.
   - In HDR mode, SDR video is placed at the system SDR white.
6. **Refresh rate:**
   - `ChangeDisplaySettingsExW`/`DEVMODEW` are integer-Hz only.
   - `SetDisplayConfig` with `DISPLAYCONFIG_RATIONAL` (`targetVideoSignalInfo.vSyncFreq`) is the exact way to set 119.88/23.976. Use it as a temporary config (no `SDC_SAVE_TO_DATABASE`) and restore with `SDC_APPLY | SDC_USE_DATABASE_CURRENT`.
7. **VRR has no "is active" API.** The only documented hook is tearing support (`DXGI_FEATURE_PRESENT_ALLOW_TEARING`), and neither mpv nor libplacebo uses `ALLOW_TEARING`.
   - On 120–480 Hz desktops, the practical default is `video-sync=display-resample` at the current refresh. It needs `video-sync-max-factor` raised for 240/480 Hz.
   - Switch to a same-resolution multiple mode only for rates that cannot be fitted within 1 %, such as 25/50 fps.
8. **Independent flip/MPO:** hide (not just make transparent) the WebView2 overlay window. Verify the result with PresentMon's `PresentMode` column.
9. **Audio:** WASAPI, shared mode by default (`audio-exclusive=no`, `audio-channels=auto-safe`). The TV/receiver profile uses `audio-spdif=ac3,eac3,dts-hd,truehd` plus exclusive mode.
10. **Restore:** persist a JSON snapshot of the affected paths/modes and HDR state before any change, keyed by adapter LUID + target id + `monitorDevicePath`. Restore on exit, on panic, and on the next start if the marker still exists.

---

## 1. HDR query and toggle per display (DisplayConfig)

### 1.1 Device-info types

`DISPLAYCONFIG_DEVICE_INFO_TYPE` values, from `wingdi.h` in win32metadata; the Learn enum page lists the names [S1][S19]:

| Value | Name | In `windows` 0.62.2? |
|---|---|---|
| 1 | `DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME` | yes |
| 2 | `DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME` | yes |
| 3 | `DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE` | yes |
| 9 | `DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO` | yes |
| 10 | `DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE` | yes |
| 11 | `DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL` | yes |
| 15 | `DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2` | **no** |
| 16 | `DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE` | **no** |
| 17 | `DISPLAYCONFIG_DEVICE_INFO_SET_WCG_STATE` | **no** |

- **[Src]** These were checked by grepping `src/Windows/Win32/Devices/Display/mod.rs` in the 0.62.2 tarball [S18].
- **[Doc]** The Learn reference pages for `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` and `DISPLAYCONFIG_SET_HDR_STATE` currently return 404. The layouts below come from `wingdi.h` in win32metadata [S19].
- **Header guard:** types 15–17, their structs and `DISPLAYCONFIG_ADVANCED_COLOR_MODE` are declared under `#if NTDDI_VERSION >= NTDDI_WIN11_GA` [S19].

### 1.2 Struct layouts (`wingdi.h`)

**Legacy pair.**
- `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO` has:
  - `header`;
  - a `u32` bitfield: bit0 `advancedColorSupported`, bit1 `advancedColorEnabled`, bit2 `wideColorEnforced`, bit3 `advancedColorForceDisabled`, then 28 reserved bits;
  - `colorEncoding: DISPLAYCONFIG_COLOR_ENCODING`;
  - `bitsPerColorChannel: u32`.
- `DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE` has `header` and a bitfield whose bit0 is `enableAdvancedColor` [S19].

**Windows 11 pair.**
- `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` has:
  - `header`;
  - a `u32` bitfield:
    - bit0 `advancedColorSupported`
    - bit1 `advancedColorActive`
    - bit2 `reserved1`
    - bit3 `advancedColorLimitedByPolicy`
    - bit4 `highDynamicRangeSupported`
    - bit5 `highDynamicRangeUserEnabled`
    - bit6 `wideColorSupported`
    - bit7 `wideColorUserEnabled`
    - then 24 reserved bits;
  - `colorEncoding`;
  - `bitsPerColorChannel`;
  - `activeColorMode: DISPLAYCONFIG_ADVANCED_COLOR_MODE` [S19].
- `DISPLAYCONFIG_ADVANCED_COLOR_MODE` values, with the header comments [S19]:
  - `_SDR = 0`: "RGB888 composition, display-referred".
  - `_WCG = 1`: "FP16 scRGB, scene-referred color, display-referred luminance".
  - `_HDR = 2`: scene-referred luminance.
- `DISPLAYCONFIG_SET_HDR_STATE` has `header` and a bitfield whose bit0 is `enableHdr`. `DISPLAYCONFIG_SET_WCG_STATE` has bit0 `enableWcg` [S19].
- **Bit order [Inference]:** the bit positions assume MSVC's LSB-first bitfield allocation, which is the Windows ABI. The `windows` crate exposes these unions only as `_bitfield: u32`, with no accessor methods [S18], so Granary masks the bits by hand.

### 1.3 Exact `windows` 0.62.2 names (module `windows::Win32::Devices::Display`) [S18][S17]

**Functions:**
- `DisplayConfigGetDeviceInfo(requestpacket: *mut DISPLAYCONFIG_DEVICE_INFO_HEADER) -> i32`
- `DisplayConfigSetDeviceInfo(setpacket: *const DISPLAYCONFIG_DEVICE_INFO_HEADER) -> i32`
- `GetDisplayConfigBufferSizes(flags: QUERY_DISPLAY_CONFIG_FLAGS, numpatharrayelements: *mut u32, nummodeinfoarrayelements: *mut u32) -> WIN32_ERROR`
- `QueryDisplayConfig(flags, *mut u32, *mut DISPLAYCONFIG_PATH_INFO, *mut u32, *mut DISPLAYCONFIG_MODE_INFO, Option<*mut DISPLAYCONFIG_TOPOLOGY_ID>) -> WIN32_ERROR`
- `SetDisplayConfig(Option<&[DISPLAYCONFIG_PATH_INFO]>, Option<&[DISPLAYCONFIG_MODE_INFO]>, SET_DISPLAY_CONFIG_FLAGS) -> i32`

**Structs:**
- `DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type, size, adapterId: LUID, id }`
- `DISPLAYCONFIG_SOURCE_DEVICE_NAME { header, viewGdiDeviceName: [u16; 32] }`
- `DISPLAYCONFIG_TARGET_DEVICE_NAME { header, flags, outputTechnology, edidManufactureId, edidProductCodeId, connectorInstance, monitorFriendlyDeviceName: [u16; 64], monitorDevicePath: [u16; 128] }`
- `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO { header, Anonymous, colorEncoding: windows::Win32::Graphics::Gdi::DISPLAYCONFIG_COLOR_ENCODING, bitsPerColorChannel }`
- `DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE { header, Anonymous }`
- `DISPLAYCONFIG_SDR_WHITE_LEVEL { header, SDRWhiteLevel: u32 }`
- `DISPLAYCONFIG_PATH_INFO { sourceInfo, targetInfo, flags }`
- `DISPLAYCONFIG_MODE_INFO { infoType, id, adapterId, Anonymous }`
- `DISPLAYCONFIG_TARGET_MODE { targetVideoSignalInfo }`
- `DISPLAYCONFIG_RATIONAL { Numerator, Denominator }`

**Constants:**
- `QDC_ONLY_ACTIVE_PATHS` = 2, `QDC_VIRTUAL_REFRESH_RATE_AWARE` = 64.
- `SDC_APPLY` = 0x80, `SDC_USE_SUPPLIED_DISPLAY_CONFIG` = 0x20, `SDC_VALIDATE` = 0x40, `SDC_SAVE_TO_DATABASE` = 0x200, `SDC_ALLOW_CHANGES` = 0x400, `SDC_USE_DATABASE_CURRENT` = 0xF, `SDC_VIRTUAL_REFRESH_RATE_AWARE` = 0x20000.

**Cargo features:**
- `Win32_Devices_Display`.
- `Win32_Graphics_Gdi`, needed for `DISPLAYCONFIG_COLOR_ENCODING`, `ChangeDisplaySettingsExW`, `EnumDisplaySettingsExW`, `DEVMODEW`, `MonitorFromWindow`, `GetMonitorInfoW` and `MONITORINFOEXW`, which all live in `Win32::Graphics::Gdi` in 0.62.2.
- `Win32_Foundation`.
- `Win32_Graphics_Dxgi` and `Win32_Graphics_Dxgi_Common`.

**Docs hosting:** docs.rs does not render the `Win32::Devices::Display` module (404). Microsoft hosts the generated docs at microsoft.github.io/windows-docs-rs [S16][S17].

**Hand-declared types (sketch).** Names are Granary's own; the layout follows `wingdi.h` [S19]:

```rust
use windows::Win32::Devices::Display::*;
pub const GET_ADVANCED_COLOR_INFO_2: DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(15);
pub const SET_HDR_STATE:             DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(16);
#[repr(C)] pub struct AdvColorInfo2 {
    pub header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    pub bits: u32,                    // b0 supported, b1 active, b3 limitedByPolicy, b4 hdrSupported, b5 hdrUserEnabled, b6 wcgSupported, b7 wcgUserEnabled
    pub color_encoding: i32,          // DISPLAYCONFIG_COLOR_ENCODING
    pub bits_per_color_channel: u32,
    pub active_color_mode: i32,       // 0 SDR, 1 WCG, 2 HDR
}
#[repr(C)] pub struct SetHdrState { pub header: DISPLAYCONFIG_DEVICE_INFO_HEADER, pub bits: u32 /* b0 enableHdr */ }
// header.size = size_of::<Self>() as u32; header.adapterId/id = path.targetInfo.adapterId/id
```

### 1.4 Mapping HMONITOR or DXGI output to a DisplayConfig path

1. **Get the GDI name.** Call `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)`, then `GetMonitorInfoW` with a `MONITORINFOEXW` to read `szDevice` (`\\.\DISPLAY1`). Alternatively, use `DXGI_OUTPUT_DESC1.DeviceName[32]`, which pairs with `DXGI_OUTPUT_DESC1.Monitor` (the HMONITOR) [S4][S18].
2. **List active paths.** Call `GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS)`, then `QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)` [S9]. Retry on `ERROR_INSUFFICIENT_BUFFER`, because the topology can change between the two calls.
3. **Match the source.** For each path, call `DisplayConfigGetDeviceInfo` with `GET_SOURCE_NAME` on `sourceInfo.adapterId`/`sourceInfo.id`, and compare `viewGdiDeviceName` with the GDI name.
4. **Query the target.** Use `targetInfo.adapterId`/`targetInfo.id` of the matching path for all colour packets (9/10/11/15/16). Use `GET_TARGET_NAME` for `monitorDevicePath`, a stable key across reboots, and for `monitorFriendlyDeviceName`.
5. **Prior art:** mpv's `video/out/win32/displayconfig.c` does steps 2–3 to find the refresh rate and the SDR white level [S33] **[Src]**.
6. **Cloned displays [Inference]:** in clone mode one source has several targets. Apply the colour state to every target of the source.
7. **Which DXGI output owns the window:** the Advanced Color article says not to rely on `IDXGISwapChain::GetContainingOutput`, which can be stale.
   - Instead, enumerate outputs and take the one with the largest intersection with the window rectangle.
   - Re-check when `IDXGIFactory1::IsCurrent()` returns false (poll it) and on `WM_SIZE`/move. Win32 apps get no HDR-change event [S3] **[Doc]**.
   - mpv does a simplified version: it calls `MonitorFromWindow` on its window, recreates the DXGI factory when `!IsCurrent()`, matches `DXGI_OUTPUT_DESC1.Monitor`, then calls `GetDesc1` [S29] **[Src]**.

### 1.5 Recommended query and toggle logic

**Is HDR on?**
- **Documented check:** `DXGI_OUTPUT_DESC1.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020` means the output is in HDR mode. `..._G22_NONE_P709` means SDR, including WCG/ACM-on-SDR, which cannot be told apart from plain SDR [S3][S4] **[Doc]**.
- **Fine-grained check:** use packet 15 and read `activeColorMode == 2` and `highDynamicRangeSupported` [S19].
- **Fallback when packet 15 fails:** use packet 9 and treat `advancedColorEnabled && !wideColorEnforced` as HDR on. That rule is **[Inference]**: `wideColorEnforced` marks ACM/WCG on an SDR panel.

**Toggling.** Only when HDR is off, the panel supports it, and the user opted in:
1. Send `SET_HDR_STATE` (16) with `enableHdr = 1`.
2. If that fails with a non-zero return, fall back to `SET_ADVANCED_COLOR_STATE` (10) with `enableAdvancedColor = 1`.
3. **[Inference]** On Windows 11 machines with Auto Color Management, the legacy "advanced colour" bit may toggle WCG rather than HDR, so prefer packet 16 whenever packet 15 works.
4. After the toggle, re-check DXGI. The factory becomes non-current, so recreate it and call `GetDesc1` again [S3].

**[Inference]** Treat the toggle as persistent: it is the same state as the Settings switch. Restore it explicitly, as described under "Restore strategy".

**Requirements [Doc]:** the Advanced Color article targets WDDM 2.7+ drivers and Windows 10 2004+ for HDR in apps [S3].

**Uncertainty:** the minimum runtime build for packets 15–17 is unknown. The header guard says `NTDDI_WIN11_GA` [S19]. Always probe at runtime: a non-zero return means "unsupported", so use 9/10.

## 2. SDR content brightness (SDR white level)

- **API:** `DisplayConfigGetDeviceInfo` with `DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL` (11) and `DISPLAYCONFIG_SDR_WHITE_LEVEL { header, SDRWhiteLevel }`, addressed to the target.
  - `SDRWhiteLevel` is relative to 80 nits: 1000 means 80 nits, so nits = `SDRWhiteLevel / 1000 * 80` [S2] **[Doc]**.
- **Where the value matters:** the Advanced Color article says scRGB 1.0 = 80 nits, and users set an "SDR content brightness" that is typically around 200 nits on desktop HDR displays.
  - Apps that compose SDR UI into an HDR surface should scale linear values by `SdrWhiteLevel / 80`.
  - When SDR and HDR content are on separate swapchains or surfaces, Windows applies the scaling to the SDR ones automatically [S3] **[Doc]**.
- **What this means for Granary:**
  - The WebView2 overlay is an SDR surface, so DWM brightens it automatically.
  - The mpv HDR swapchain must place SDR video at that level itself.
  - mpv 0.42 does this: `--hdr-reference-white=<auto|10-10000>`, where `auto` queries the system (Windows only) and otherwise uses 203. The release notes add "vo_gpu_next: follow reference white system setting"; subtitles and the OSD follow it too [S21][S25] **[Doc]**.
- **Implementation [Src]:** `mp_dxgi_sdr_white_level_from_hwnd` → `mp_w32_displayconfig_get_sdr_white_level(szDevice)` returns `SDRWhiteLevel * 80 / 1000`, cached per monitor [S29][S33].
  - A source comment in `d3d11_helpers.c` notes that `IsCurrent()` also tracks reference-luminance changes [S29].
- **Uncertainty:** whether mpv's per-monitor cache refreshes when the user moves the slider during playback.
  - **Mitigation:** Granary can poll packet 11 (cheap) whenever `IsCurrent` goes false or the window gains focus, and set `hdr-reference-white=<nits>` explicitly if the value differs.

## 3. DXGI luminance values and the HDR Calibration profile

`IDXGIOutput6::GetDesc1()` returns `DXGI_OUTPUT_DESC1` [S4]. In the `windows` crate this is `IDXGIOutput6::GetDesc1(&self) -> Result<DXGI_OUTPUT_DESC1>` [S18]. Its fields:
- `DeviceName[32]`, `DesktopCoordinates`, `AttachedToDesktop`, `Rotation`, `Monitor`, `BitsPerColor`, `ColorSpace`.
- `RedPrimary`, `GreenPrimary`, `BluePrimary`, `WhitePoint`.
- `MinLuminance`, `MaxLuminance`, `MaxFullFrameLuminance`.

Microsoft's definitions [S4] **[Doc]**:
- `MaxLuminance` is "likely only valid for a small area of the panel".
- `MaxFullFrameLuminance` is valid for a colour that fills the whole panel.
- The values usually come from the EDID, sometimes from an override.

**What mpv does with them [Src]:**
- `mp_dxgi_desc_to_color_space` maps `max_luma = MaxLuminance`, `min_luma = MinLuminance` and `max_fall = MaxFullFrameLuminance`. In SDR mode it zeroes the luminance values.
- `vo_gpu_next` then drops `max_fall`, with the comment "maxFALL in display metadata is in fact MaxFullFrameLuminance" [S29][S31].
- So libplacebo tone-maps to the small-window peak, which is what an OLED with ABL wants.

**HDR Calibration app.**
- The support page says the app shows three HGIG-style test patterns: darkest visible detail, brightest visible detail, and maximum full-screen brightness. It saves the result as a colour profile, which can be removed under Color Management → All Profiles [S14] **[Doc]**.
- The Advanced Color ICC article says Windows colour-manages using the display's default profile, otherwise the EDID. Profiles carry standard/extended subtypes. Apps that understand advanced colour should use `IDXGIOutput6` [S5] **[Doc]**.
- **Not documented by Microsoft:** whether the calibration profile changes the numbers `GetDesc1` reports. Treat this as unverified.
- **Policy for Granary:**
  - Trust `GetDesc1`.
  - Show the reported peak, full-frame and black values in a per-monitor settings page.
  - Offer a per-monitor `target-peak` override keyed by `monitorDevicePath`. QA should compare `GetDesc1` with and without a calibration profile on the dev machine; that is not done here.

**"Don't tone-map if it fits" [Src]:**
- With `target-colorspace-hint-mode=target`, mpv merges the display's measured luminance into the target.
- libplacebo applies tone mapping only when the source peak (from mastering, MaxCLL or measured `hdr-compute-peak` data) exceeds the target peak.
- Source fitting inside the panel peak passes through untouched [S21][S31].
- **Edge case:** if `GetDesc1` reports 0 or nonsense, which is common with AV receivers, the hint mode falls back to `source` for each unknown property [S21]. Granary should then set `target-peak` from user settings.

## 4. Refresh-rate switching

### 4.1 Integer DEVMODE versus rational DisplayConfig

**ChangeDisplaySettingsExW** [S6][S7] **[Doc]**:
- `DEVMODEW.dmDisplayFrequency` is a `DWORD` holding the frequency in hertz, so it is integer-only. 23.976, 59.94 and 119.88 cannot be expressed.
- Drivers commonly list 23/59/119 Hz entries for the NTSC-rate modes. That convention is **[Unofficial]** and varies by driver [S38].
- **Flags** (`CDS_TYPE` in `windows::Win32::Graphics::Gdi`) [S6][S18]:
  - `0`: dynamic change for this session.
  - `CDS_FULLSCREEN` (4): "temporary in nature"; it is not reset when switching desktops.
  - `CDS_UPDATEREGISTRY` (1): persists to the user profile.
  - `CDS_TEST` (2): validates only.
  - `CDS_NORESET` with `CDS_UPDATEREGISTRY`: batches several monitors, then a final call with a NULL device applies them.
- **Returning to the default:** calling with `lpDevMode = NULL` and flags 0 is "the easiest way to return to the default mode after a dynamic mode change" [S6].
- **Return value:** `DISP_CHANGE_*`. The change broadcasts `WM_DISPLAYCHANGE` [S6].
- **Process exit is not documented.** The docs do not say a dynamic or `CDS_FULLSCREEN` mode reverts when the process exits. Do not rely on it.
- **`windows` 0.62.2 signature** [S18]: `ChangeDisplaySettingsExW(lpszdevicename, lpdevmode: Option<*const DEVMODEW>, hwnd: Option<HWND>, dwflags: CDS_TYPE, lparam: Option<*const c_void>) -> DISP_CHANGE`.
- **Enumerating modes** [S18]: `EnumDisplaySettingsExW(name, imodenum: ENUM_DISPLAY_SETTINGS_MODE, *mut DEVMODEW, flags) -> BOOL`, with `ENUM_CURRENT_SETTINGS` / `ENUM_REGISTRY_SETTINGS`.

**SetDisplayConfig (exact rational)** [S8][S9][S10][S11] **[Doc]**:
- `DISPLAYCONFIG_PATH_TARGET_INFO.refreshRate` is a `DISPLAYCONFIG_RATIONAL`.
  - If target mode info is supplied, the OS uses `DISPLAYCONFIG_TARGET_MODE.targetVideoSignalInfo.vSyncFreq` (also rational) instead.
  - `0/0` means "let the OS pick"; it requires `scanLineOrdering` = UNSPECIFIED.
- `DISPLAYCONFIG_VIDEO_SIGNAL_INFO` contains `pixelRate`, `hSyncFreq`, `vSyncFreq`, `activeSize`, `totalSize`, `videoStandard`/`vSyncFreqDivider` and `scanLineOrdering`.
- **Flags:**
  - Temporary change: `SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG`, without `SDC_SAVE_TO_DATABASE`. It is not saved to the persistence database.
  - Persistent change: add `SDC_SAVE_TO_DATABASE`.
  - Return from a temporary config: `SDC_APPLY | SDC_USE_DATABASE_CURRENT`.
  - Dry run: `SDC_VALIDATE | SDC_USE_SUPPLIED_DISPLAY_CONFIG`.
  - `SDC_ALLOW_CHANGES` lets the OS adjust the supplied modes. Without it, an unsatisfiable config fails with `ERROR_BAD_CONFIGURATION`.
  - `SDC_VIRTUAL_REFRESH_RATE_AWARE` / `QDC_VIRTUAL_REFRESH_RATE_AWARE` require Windows 11 [S8][S9].
- **Limitation:** `QueryDisplayConfig` returns only the current paths and modes, not the list of supported modes [S9]. Candidate enumeration therefore still uses `EnumDisplaySettingsExW` (integer) or `IDXGIOutput1::GetDisplayModeList1`, which returns `DXGI_MODE_DESC1` with a `DXGI_RATIONAL` `RefreshRate` [S18].

**Recommended algorithm for an exact multiple mode, such as 119.88 for 23.976 on a 120 Hz panel [Inference built on S8–S11]:**
1. Snapshot the current state with `QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)`. Persist it (see "Restore strategy").
2. Enumerate candidates at the **same resolution** with `GetDisplayModeList1` for the output (rational) and/or `EnumDisplaySettingsExW` (integer). Select N/1001 or N/1 rates that are integer multiples of the content rate, nearest to the current refresh, and never below 60 Hz unless the TV profile allows it.
3. Copy the snapshot arrays. In the target mode for the path, set `targetVideoSignalInfo.vSyncFreq = {120000, 1001}`, and adjust `hSyncFreq`/`pixelRate` from the candidate mode's timing if known.
4. Call `SetDisplayConfig(paths, modes, SDC_VALIDATE | SDC_USE_SUPPLIED_DISPLAY_CONFIG)`.
   - If that succeeds, apply with `SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG`.
   - If it fails, retry once with `| SDC_ALLOW_CHANGES`.
5. Re-run `QueryDisplayConfig` and read back the actual `vSyncFreq`. Accept the change only if it is within 0.05 % of the request; otherwise revert.
6. Tell mpv: it re-reads the rate on `WM_DISPLAYCHANGE` (see §8.5). Alternatively, set `display-fps-override` to the read-back rational.

### 4.2 VRR detection

- **Documented [Doc]:** the Microsoft VRR page defines app support through tearing.
  - Check `IDXGIFactory5::CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING)`.
  - Create the swapchain with `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING`.
  - Present with `DXGI_PRESENT_ALLOW_TEARING` and sync interval 0, windowed or borderless.
  - The constant `DXGI_FEATURE_PRESENT_ALLOW_TEARING` exists in `windows` 0.62.2 [S12][S18].
  - There is **no API that reports "this monitor has VRR enabled" or "VRR is active now"**.
- **Unofficial [Unofficial]:** the Windows "Variable refresh rate" toggle under Settings → Display → Graphics is reported to be stored in `HKCU\Software\Microsoft\DirectX\UserGpuPreferences`, value `DirectXUserGlobalSettings`, containing `VRROptimizeEnable=1;` [S37]. Read-only use at most; never write it.
- **mpv [Src]:**
  - Neither mpv's d3d11 context nor libplacebo's d3d11 swapchain uses `ALLOW_TEARING`.
  - mpv presents with `--d3d11-sync-interval` (default 1) and no present flags. It calls `MakeWindowAssociation(DXGI_MWA_NO_WINDOW_CHANGES | DXGI_MWA_NO_ALT_ENTER)` [S21][S28][S35].
- **[Inference]** With sync interval 1 the present cadence is locked to vblank. Whether G-Sync/FreeSync engages for a windowed flip-model app is up to the driver; it is mostly a driver-panel option for windowed mode.
- **Conclusion:** Granary cannot reliably *know* VRR is helping. Treat VRR as a user-declared, per-monitor capability ("This monitor uses VRR for video": off / auto / on), not as a detected one.
  - In "on" mode, keep the desktop refresh and use `video-sync=audio`.
  - Verify with PresentMon (§8.2) and the monitor's own refresh read-out. If `MsBetweenDisplayChange` follows the content rate at a fixed desktop refresh, VRR is working.

### 4.3 The policy the requirements ask for

**Default: never change modes.**
1. **VRR.** If the user declared VRR on this monitor and it is verified, use `video-sync=audio` (as in §4.2).
2. **Display-resample at the current refresh.** Otherwise use `video-sync=display-resample` at the current refresh whenever the refresh is within `video-sync-max-video-change` (default 1 %) of a multiple of the content rate [S21].
   - 120, 240 and 480 Hz all fit 23.976, 24, 29.97, 30, 59.94 and 60 fps. The 0.1 % NTSC offset is absorbed by resampling the audio.
   - **Raise `video-sync-max-factor` on high-refresh panels.** Its default of 5 is the "maximum multiple for which to try to fit the video's FPS to the display's FPS" [S21]. 240/23.976 is about 10 and 480/23.976 about 20.
   - **[Inference]** Have the host set it to `ceil(display_fps / min(video_fps, 24)) + 1`, or simply 21. Then check `display-sync-active` and `vsync-ratio` [S22].
3. **Exact multiple mode.** Switch only when step 2 cannot fit. Typical cases are 25/50 fps on 120/240/480 Hz (ratios 4.8/9.6/19.2) or 23.976 on a 144 or 165 Hz panel.
   - Switch to an exact multiple **at the same resolution**, nearest the current refresh: 100/200 Hz for 25/50 fps, 119.88/239.76 for 23.976.
   - The switch uses the temporary `SetDisplayConfig` path from §4.1 and needs the user's opt-in for "auto mode switch (desktop)".
4. **Exact refresh matching** (24 Hz and similar, intended for TVs) belongs only to the TV/receiver profile, opt-in.

## 5. mpv / libplacebo HDR options (mpv 0.42.0, libplacebo 7.387.0)

### 5.1 Output side (the swapchain) [Src: S35, S28, S21]

**libplacebo's d3d11 `pick_format`:**
- Chooses **FP16** if alpha needs more than 8 bits, colour needs more than 10 bits, or the hint asks for scRGB.
- Otherwise **R10G10B10A2** for HDR, wide gamut, or 10-bit SDR.
- Otherwise RGBA8.

**`pick_colorspace`:**
- FP16 → `DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709` (scRGB).
- HDR hint plus RGB10A2 → `G2084_NONE_P2020` (HDR10).
- Wide gamut → `G22_NONE_P2020`.
- Otherwise `G22_NONE_P709`.

**HDR10 metadata:** set through `IDXGISwapChain4::SetHDRMetaData`, with the mastering and MaxCLL/MaxFALL values taken from the target.

**Selecting the output:**
- `--d3d11-output-format=<auto|rgba8|bgra8|rgb10_a2|rgba16f>`: a hint on gpu-next; `rgba16f` gives scRGB.
- `--d3d11-output-csp=<auto|srgb|linear|pq|bt.2020>`: `auto` uses the desktop's colour space. "Values other than srgb and pq have had issues in testing."
- `--target-trc=scrgb` is gpu-next only and new in 0.42 [S21][S25].
- Both are mostly unnecessary; see §8.1.

**Flip and exclusive fullscreen:**
- `--d3d11-flip` (default yes): flip model is required for HDR.
- `--d3d11-exclusive-fs` (default no) [S21].

### 5.2 Target and colour-space options [S21]

| Option | Default | Meaning / Granary use |
|---|---|---|
| `--target-colorspace-hint=<auto\|yes\|no>` | auto | Set swapchain colour metadata. "In auto mode, the target colorspace is only set if the current display parameters are known." Supported on D3D11. Requires gpu-next. |
| `--target-colorspace-hint-mode=<target\|source\|source-dynamic>` | target | `target` adapts the content to the display's measured capabilities; `source` is classic passthrough; `source-dynamic` sends per-scene HDR10 values, not real HDR10+/DV. Falls back to `source` per unknown property. |
| `--target-colorspace-hint-strict` | yes | Keep. |
| `--target-prim`, `--target-trc` | auto | Leave auto; the values come from the DXGI ColorSpace. |
| `--target-peak=<auto\|nits>` | auto | auto means the display-reported peak (203 for SDR). Override per monitor only if `GetDesc1` is wrong. |
| `--target-contrast=<auto\|10-1000000\|inf>` | auto | auto means 1000:1 for SDR and infinite for HDR; the manual notes `inf` suits OLED in practice. |
| `--target-gamut` | – | Limit the output gamut (for example to `dci-p3`) if desired. |
| `--hdr-reference-white=<auto\|10-10000>` | auto | System SDR white on Windows, else 203. Overrides `target-peak` for SDR targets. |
| `--tone-mapping` | auto → spline on gpu-next | Others: `bt.2390`, `bt.2446a`, `st2094-40`, `st2094-10`, `hable`, `mobius`, `reinhard`, `clip`, `linear`, `gamma`. |
| `--tone-mapping-*` (knee-adaptation 0.4, knee-minimum 0.1, knee-maximum 0.8, knee-default 0.4, knee-offset 1.0, slope-tuning 1.5, slope-offset 0.2, spline-contrast 0.5, reinhard-contrast 0.5, linear-knee 0.3, exposure 1.0) | – | gpu-next parameters. `--tone-mapping-param` affects only `vo=gpu`. |
| `--inverse-tone-mapping` | no | Keep off: SDR should stay at SDR white, not be stretched. |
| `--gamut-mapping-mode` | auto | auto/clip/perceptual/relative/saturation/absolute/desaturate/darken/warn/linear. |
| `--hdr-compute-peak=<auto\|yes\|no>` | auto | Measures frame peaks, so the tone mapping follows the real content. Related: `--allow-delayed-peak-detect` (no), `--hdr-peak-percentile` (100), `--hdr-peak-decay-rate` (20), `--hdr-scene-threshold-low/high` (1/3), `--hdr-contrast-recovery` (0), `--hdr-mastering-clip` (1.0). |
| `--sdr-adjust-gamma`, `--treat-srgb-as-power22` | auto | Leave auto. |
| `--icc-profile-auto` | no | Keep off in HDR. Windows already colour-manages, and the DXGI path is the documented route [S5]. |
| `--tone-mapping-visualize` | no | Debug aid. |

## 6. Dolby Vision (profiles 5/7/8), HDR10+ and hwdec

**What mpv does:**
- **Applying DV.** mpv carries DV RPU metadata on frames, and gpu-next hands it to libplacebo, which applies the reshaping and mapping. The default video filter `format` keeps it (`dolbyvision=yes`; `no` strips it) [S24].
- **Profile 7 FEL.** mpv 0.42 adds "Dolby Vision Profile 7 enhancement layer support in gpu-next" [S25]. It is controlled by `vf format:enhancement-layer=yes|no` (default yes: "Currently this controls Dolby Vision Profile 7 FEL application") [S24].
  - In source, `vo_gpu_next` keeps a separate `el_hwdec` and EL textures for the FEL [S31] **[Src]**.
- **Profiles 5 and 8.**
  - **[Inference]** Profile 5 has a non-HDR10-compatible IPTPQc2 base layer. It only looks right with the RPU applied, which gpu-next does; keep `dolbyvision=yes`.
  - **[Inference]** Profile 8.1 has an HDR10-compatible base layer, so it falls back gracefully if the RPU is lost.
  - **[Doc]** The output to Windows is always HDR10 (PQ/BT.2020) or scRGB, tone-mapped by libplacebo. mpv does not emit a DV signal: `source-dynamic` "does not send full HDR10+ or Dolby Vision metadata" [S21].
- **HDR10+.** Dynamic metadata is kept by default (`vf format:hdr10plus=yes`) [S24]. It is used by `--tone-mapping=st2094-40` (HDR10+ Annex B) and can drive per-scene output metadata through `source-dynamic` [S21].
- **hwdec.**
  - `--hwdec` defaults to `no`. `d3d11va` is zero-copy and needs gpu/gpu-next with a d3d11 context; `d3d11va-copy` is the copy variant [S21].
  - In `vo_gpu_next.c`, when the hwdec mapper is reused, `repr.dovi` and `color.hdr` are copied from the frame parameters [S31] **[Src]**. So DV and HDR10+ metadata survive d3d11va zero-copy.
  - 0.42 adds d3d11va zero-copy for 4:4:4 and high bit depth [S25].
  - **Recommendation:** `hwdec=d3d11va` (or `auto-safe`), with a "software decode" fallback toggle for debugging DV issues.
- **Reading DV/HDR fields for a stats overlay (properties)** [S22]:
  - `track-list/N/dolby-vision-profile` and `track-list/N/dolby-vision-level` (int64). These come from the container and may be missing.
  - `video-params/` gives `pixelformat`, `hw-pixelformat`, `colormatrix` (reads `dolbyvision` for DV), `colorlevels`, `primaries`, `gamma`, `light`, `min-luma`, `max-luma`, `max-cll`, `max-fall`, `scene-max-r/g/b` (HDR10+), `max-pq-y` and `avg-pq-y` (measured or dynamic), and `prim-*`.
  - `video-target-params/*`: what the VO actually outputs, including target peak, primaries and transfer.
  - Also `video-out-params`, `hwdec-current`, `current-gpu-context`, `display-names`, `display-fps`, `estimated-display-fps`, `vsync-jitter`, `display-sync-active`, `vsync-ratio`, `mistimed-frame-count`, `frame-drop-count`, `vo-passes`, and the new `libplacebo-version`.
  - **Overlay idea [Inference]:** show "DV P{profile}.{level} (container)" from `track-list`, and "DV RPU active" when `video-params/colormatrix == dolbyvision`.

## 7. WASAPI and audio options [S21][S23][S25]

**Device selection:**
- `ao=wasapi` is the Windows default.
- `--audio-device` defaults to `auto`, which follows the system default device. Enumerate devices through the `audio-device-list` property (entries are `{name, description}`) and persist `name`.

**Shared versus exclusive:**
- `--audio-exclusive=yes|no` is supported by wasapi.
- `--wasapi-exclusive-buffer=<default|min|1-2000000>`.
- Default to shared mode on the desktop, so system sounds and other apps keep playing and the Windows volume works.

**Channels:**
- `--audio-channels=auto-safe` (default) uses the system's preferred layout, falling back to stereo.
- For HDMI/receivers, the manual recommends an explicit list such as `7.1,5.1,stereo`.
- 0.42 can request unknown layouts as DirectOut.

**Passthrough:**
- `--audio-spdif=ac3,eac3,dts-hd,truehd` is the receiver list. `dts` plus `dts-hd` behaves as `dts-hd`.
- `dsd` (new DoP in 0.42) requires wasapi exclusive mode.

**Other options:**
- `--audio-normalize-downmix` defaults to no. Enable it for stereo desktops if clipping on 5.1→2.0 downmix is reported.
- `--audio-fallback-to-null` keeps video playing when the device disappears.

**Interaction with display sync:**
- `display-resample` changes audio speed by up to about 1 % plus 0.125 % [S21].
- **[Inference]** Bitstreamed passthrough cannot be resampled. With `audio-spdif` active, use `video-sync=audio` or match the refresh exactly (TV profile), and expect occasional repeated frames otherwise.
- **[Inference]** Changing the display mode over HDMI often renegotiates the link, and AV receivers may mute for one or two seconds. Do mode switches before playback starts, while paused.

## 8. Desktop OLED specifics

### 8.1 scRGB versus HDR10 [S3][S35]

- **Microsoft's general recommendation [Doc]:** FP16 scRGB (`R16G16B16A16_FLOAT` plus `G10_NONE_P709`) is recommended in general and required for alpha. `R10G10B10A2` plus `G2084_NONE_P2020` is allowed only on HDR displays, D3D11/12 only, with no alpha, and "can perform better" for video [S3].
- **For Granary:** stay on libplacebo's default (RGB10A2/PQ) when Windows HDR is on.
  - The video swapchain needs no alpha, because the overlay is a separate window.
  - PQ at 10 bits is the most efficient buffer and is eligible for MPO on some GPUs **[Inference]**.
- **When to choose scRGB:**
  - Use `--d3d11-output-format=rgba16f` or `--target-trc=scrgb` only when the window straddles mixed SDR/HDR monitors and DWM clipping is visible. The article notes that a window straddling two displays reports the display containing its centre, and DWM clips on the SDR one [S3].
  - Also consider it for GPUs with poor PQ swapchain support.

### 8.2 Independent flip, MPO and PresentMon [S13][S36]

- **Flip-model presentation modes:**
  - DirectFlip: the buffers match the screen and the client area covers it.
  - DirectFlip with panel fitters: within a scaling factor.
  - DirectFlip with MPO: a dedicated hardware plane over an alpha-blended sub-region.
- **Composition behaviour:**
  - Independent flip lets DWM sleep.
  - If content appears on top, DWM falls back to composition ("reverse compose") or uses MPO to stay independent.
  - `FLIP_DISCARD` enables reverse composition.
  - HDR formats are not clamped unless they are composed onto an SDR desktop.
  - `IDXGIOutput6::CheckHardwareCompositionSupport` reports MPO capability; in `windows` 0.62.2 it returns `Result<u32>` [S13][S18].
- **Granary rules [Inference from S13]:**
  - When controls are idle, call `ShowWindow(SW_HIDE)` on the WebView2 overlay window. An alpha-0 layered or transparent window still counts as content above the swapchain.
  - Avoid any other window or tooltip over the video.
  - In fullscreen, make the mpv child window cover the monitor exactly.
- **Uncertainty:** whether a swapchain on a `WS_CHILD` HWND (mpv's `--wid` child) is eligible for independent flip is not documented. Verify with PresentMon.
- **Verifying:** run `PresentMon.exe --process_name granary.exe --timed 20 --terminate_after_timed --output_file pm.csv`, then read the `PresentMode` column [S36]. The values are:
  - `Hardware: Independent Flip`
  - `Hardware Composed: Independent Flip` (MPO)
  - `Composed: Flip`
  - `Hardware: Legacy Flip`
  - `Hardware: Legacy Copy to front buffer`
  - `Composed: Copy with GPU GDI`
  - `Composed: Copy with CPU GDI`

  With libmpv embedded, the presenting process is granary.exe itself.

### 8.3 VRR with mpv; interpolation

- **VRR:** see §4.2. mpv's d3d11 path presents at sync interval 1 without tearing, so VRR behaviour depends on the driver. It is not under mpv's control [S28][S35].
- **`--interpolation`** needs a `display-*` video-sync. `--interpolation-threshold` defaults to 0.01, and `--tscale` defaults to `oversample` [S21].
  - At 120 Hz and above, judder from a 5:5 cadence is already invisible.
  - **[Inference]** Keep interpolation off by default and offer it as an opt-in for 60 Hz or 144/165 Hz panels where the cadence does not divide evenly (`interpolation=yes`, `tscale=oversample`).

### 8.4 Range, dithering and debanding [S21]

- **Range:** `--video-output-levels=full`. auto already equals full; set it explicitly as documentation. The manual advises fixing limited-range TVs with the driver's range setting rather than in mpv.
- **Dither depth:** `--dither-depth=auto` uses the detected depth on gpu-next.
  - The wire depth is detectable only with `gpu-api=d3d11`.
  - mpv takes the smaller of the swapchain format depth and `DXGI_OUTPUT_DESC1.BitsPerColor` [S29] **[Src]**.
  - 0.42 reverted the "auto means 8 bpc for SDR when undetected" behaviour [S25].
- **Dither method:** `--dither=fruit` is the default. `error-diffusion` needs compute shaders and falls back to fruit.
- **Deband:** `--deband=yes`, with defaults iterations 1, threshold 48, range 16, grain 32. The manual calls it "virtually always an improvement - the only reason to disable it would be for performance."

### 8.5 Moving the window between monitors [Src: S28, S29, S30, S32]

- **Colour space follows automatically.** `vo_gpu_next` queries the swapchain target colour space every frame. On d3d11 this goes through `mp_dxgi_output_desc_from_hwnd`, which calls `MonitorFromWindow` on mpv's window and recreates the factory if `!IsCurrent`. So HDR↔SDR, peak and SDR white follow the window.
- **Display FPS:**
  - mpv reads it from DisplayConfig as a rational `vSyncFreq`, falling back to GDI. `update_display_info` runs on `WM_MOVE`, `WM_DISPLAYCHANGE`, `WM_DPICHANGED` and the get-display-fps control.
  - `vo.c` re-reads it only when the window-state event is raised.
  - With `--wid`, mpv creates its own `WS_CHILD` window and hooks the parent only for resize (`WM_WINDOWPOSCHANGED` / `EVENT_OBJECT_LOCATIONCHANGE` → `resize_child_win`).
  - **Risk [Inference]:** moving the parent to another monitor without a size change may not refresh `display-fps`.
- **Granary mitigation:**
  1. On the host's `WM_WINDOWPOSCHANGED`/`WM_DISPLAYCHANGE`/`WM_DPICHANGED`, recompute the owning monitor (largest intersection, §1.4).
  2. When it changes, re-query DisplayConfig. Set `display-fps-override` to the exact rational, `hdr-reference-white` if needed, the per-monitor `target-peak` override, and the `video-sync-max-factor` / video-sync policy.
  3. Apply any per-monitor exact-mode switch only if the opt-in is set.
  4. Restore the previous monitor's mode if Granary changed it.

## Recommended mpv.conf

Baseline (desktop OLED HDR). Each option has a one-line justification:

```ini
# --- renderer / swapchain ---
vo=gpu-next                         # libplacebo renderer: required for colorspace hint, DV, HDR10+, scRGB, ref-white tracking [S21][S25]
gpu-api=d3d11                       # DXGI flip swapchain + HDR colorspace signalling on Windows; only API that detects wire bit depth [S21]
gpu-context=d3d11                   # explicit D3D11 context (needed for d3d11va zero-copy) [S21]
d3d11-flip=yes                      # flip model: required for windowed HDR and independent flip/MPO [S3][S13][S21]
d3d11-exclusive-fs=no               # stay composed/independent-flip; no FSE mode changes [S21]
hwdec=d3d11va                       # zero-copy HW decode; DV/HDR metadata survive the mapper [S21][S31]
# --- HDR / colour ---
target-colorspace-hint=auto         # tag swapchain PQ/BT.2020 when Windows HDR is on, sRGB otherwise [S21][S35]
target-colorspace-hint-mode=target  # tone-map to the measured panel (GetDesc1), not blind passthrough [S21]
hdr-reference-white=auto            # SDR video at Windows "SDR content brightness" [S2][S21][S25]
tone-mapping=auto                   # spline on gpu-next; only engaged when source peak > panel peak [S21][S31]
gamut-mapping-mode=auto             # libplacebo default perceptual handling for BT.2020 -> panel gamut [S21]
hdr-compute-peak=auto               # measure real scene peaks so in-range content is left untouched [S21]
inverse-tone-mapping=no             # never expand SDR beyond SDR white [S21]
icc-profile-auto=no                 # Windows/DXGI already colour-manage HDR output [S5][S21]
# --- signal quality ---
video-output-levels=full            # PC monitors expect full-range RGB (auto==full; explicit for clarity) [S21]
dither-depth=auto                   # dither to min(swapchain depth, BitsPerColor) [S21][S29]
dither=fruit                        # default high-quality ordered noise; works without compute [S21]
deband=yes                          # "virtually always an improvement" [S21]
# --- timing ---
video-sync=display-resample         # lock frames to vsync on fixed-refresh 120-480 Hz; host flips to 'audio' for VRR/passthrough [S21]
video-sync-max-factor=21            # allow 23.976 fps to fit 240/480 Hz (default 5 is too low) [S21]
interpolation=no                    # cadence at >=120 Hz is already even; opt-in only [S21]
# --- audio ---
ao=wasapi                           # native Windows output [S23]
audio-device=auto                   # follow the Windows default device [S21]
audio-exclusive=no                  # shared mode on desktop (system sounds, volume) [S21]
audio-channels=auto-safe            # system layout, stereo fallback [S21]
audio-fallback-to-null=yes          # keep video going if the device vanishes [S21]
audio-normalize-downmix=yes         # spec Phase 7: stereo/headphone downmix with normalization (prevents 5.1->2.0 clipping) [S21]
```

The host sets these at runtime for each monitor: `display-fps-override`, `video-sync` (`audio` when VRR is declared or passthrough is active), `target-peak` (user override only), and `hdr-reference-white` (only if mpv's cache is stale).

Opt-in TV / AV-receiver profile:

```ini
[tv-receiver]
profile-desc=HTPC -> AVR -> TV (opt-in)
audio-exclusive=yes                 # bit-exact bitstream, no system mixer [S21]
audio-spdif=ac3,eac3,dts-hd,truehd  # bitstream lossless/lossy formats to the receiver [S21]
audio-channels=7.1,5.1,stereo       # explicit HDMI layouts as the manual recommends [S21]
video-sync=audio                    # passthrough can't be resampled; pair with exact refresh match [S21][Inference]
video-sync-max-factor=5             # TVs run near content rate; default fit range [S21]
target-colorspace-hint-mode=target  # still map to reported peak; host sets target-peak if TV EDID lies [S21]
deband=yes                          # TV panels band too [S21]
# Host-side (not mpv options): exact refresh match (23.976 -> 23.976 Hz via SetDisplayConfig rational),
# HDR auto-enable via SET_HDR_STATE if user opted in, both restored on exit.
```

## Restore strategy

**1. Snapshot before any change.**
- For each affected monitor, record:
  - adapter LUID, target id, source id, `monitorDevicePath` and `viewGdiDeviceName`;
  - the full `QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)` path and mode arrays;
  - HDR state from packet 15 (or 9): `activeColorMode`/`advancedColorEnabled` and `highDynamicRangeUserEnabled`.
- Write `%LOCALAPPDATA%\Granary\display-restore.json` with an fsync **before** calling `SetDisplayConfig` or `DisplayConfigSetDeviceInfo`. Include `pid`, process start time and a version field.

**2. Make every change temporary.**
- Refresh changes use `SetDisplayConfig` without `SDC_SAVE_TO_DATABASE`, so the persistent database keeps the user's real mode [S8].
- Never use `CDS_UPDATEREGISTRY`. Avoid CDS for changes, since its behaviour at process exit is undocumented [S6].
- The HDR toggle has no temporary variant, so the marker must record "Granary enabled HDR on target X".

**3. Normal restore.**
- **When:** on playback stop, on app exit, and when the window leaves a monitor Granary modified.
- **Refresh:** call `SetDisplayConfig(NULL, NULL, SDC_APPLY | SDC_USE_DATABASE_CURRENT)`, which returns to the persisted configuration [S8].
  - If that does not restore exactly (verify with `QueryDisplayConfig`), re-apply the saved arrays with `SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES`.
- **HDR:** turn it off (packet 16 `enableHdr = 0`, or packet 10) **only** if the snapshot shows it was off and Granary turned it on.
- **Marker:** delete it when the restore is done.

**4. Crash safety.**
- **In-process hooks:**
  - Install a Rust panic hook that runs the restore. Use `std::panic::set_hook` and keep `panic = "unwind"`.
  - Handle Tauri's `RunEvent::Exit`/`ExitRequested`.
  - Install `SetConsoleCtrlHandler` for console builds.
- **On the next start:** if `display-restore.json` exists and its pid is not alive, run the restore from the file before creating any window, then delete it.
- **Optional watchdog [Inference]:** a tiny `granary-display-guard.exe` child that waits on the parent's process handle and runs the same restore. This covers hard kills.

**5. Topology drift.**
- On `WM_DISPLAYCHANGE`, check whether the snapshot's targets still exist; match by `monitorDevicePath`, not by GDI name.
- If a monitor was unplugged, drop its entry. If the adapter LUID changed after a reboot, re-resolve it by `monitorDevicePath`.

**6. Never touch unaffected monitors.** Restore only the targets listed in the marker. `SDC_USE_DATABASE_CURRENT` resets all monitors to the database state, which is safe because Granary never saved to it.

## Sources

**Microsoft Learn / Support**
- [S1] DISPLAYCONFIG_DEVICE_INFO_TYPE: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ne-wingdi-displayconfig_device_info_type
- [S2] DISPLAYCONFIG_SDR_WHITE_LEVEL: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_sdr_white_level
- [S3] Using DirectX with high dynamic range displays and Advanced Color: https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range
- [S4] DXGI_OUTPUT_DESC1: https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_6/ns-dxgi1_6-dxgi_output_desc1
- [S5] ICC profile behavior with Advanced Color: https://learn.microsoft.com/en-us/windows/win32/wcs/advanced-color-icc-profiles
- [S6] ChangeDisplaySettingsExW: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-changedisplaysettingsexw
- [S7] DEVMODEW: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-devmodew
- [S8] SetDisplayConfig: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setdisplayconfig
- [S9] QueryDisplayConfig: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-querydisplayconfig
- [S10] DISPLAYCONFIG_VIDEO_SIGNAL_INFO: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_video_signal_info
- [S11] DISPLAYCONFIG_PATH_TARGET_INFO: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_path_target_info
- [S12] Variable refresh rate displays: https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/variable-refresh-rate-displays
- [S13] For best performance, use DXGI flip model: https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model
- [S14] Calibrate your HDR display using the Windows HDR Calibration app: https://support.microsoft.com/en-us/windows/hardware/display-graphics/calibrate-your-hdr-display-using-the-windows-hdr-calibration-app

**windows crate / Win32 metadata**
- [S15] crates.io `windows` (max_stable 0.62.2): https://crates.io/crates/windows
- [S16] docs.rs `windows` (Display module not rendered): https://docs.rs/crate/windows/latest
- [S17] windows-docs-rs, Win32::Devices::Display: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/Display/index.html
- [S18] `windows` 0.62.2 source tarball (Devices/Display, Graphics/Dxgi, Graphics/Dxgi/Common, Graphics/Gdi `mod.rs`): https://static.crates.io/crates/windows/windows-0.62.2.crate
- [S19] win32metadata RecompiledIdlHeaders `wingdi.h`: https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/RecompiledIdlHeaders/um/wingdi.h

**mpv v0.42.0**
- [S20] Release v0.42.0: https://github.com/mpv-player/mpv/releases/tag/v0.42.0
- [S21] options.rst: https://github.com/mpv-player/mpv/blob/v0.42.0/DOCS/man/options.rst (rendered at https://mpv.io/manual/stable/)
- [S22] input.rst (properties): https://github.com/mpv-player/mpv/blob/v0.42.0/DOCS/man/input.rst
- [S23] ao.rst: https://github.com/mpv-player/mpv/blob/v0.42.0/DOCS/man/ao.rst
- [S24] vf.rst: https://github.com/mpv-player/mpv/blob/v0.42.0/DOCS/man/vf.rst
- [S25] RELEASE_NOTES: https://github.com/mpv-player/mpv/blob/v0.42.0/RELEASE_NOTES
- [S26] mpv manual (stable): https://mpv.io/manual/stable/
- [S27] libmpv render_d3d11.h (D3D11 render API, new in 0.42; an alternative to `--wid` if Granary ever owns the swapchain): https://github.com/mpv-player/mpv/blob/v0.42.0/include/mpv/render_d3d11.h
- [S28] video/out/d3d11/context.c: https://github.com/mpv-player/mpv/blob/v0.42.0/video/out/d3d11/context.c
- [S29] video/out/gpu/d3d11_helpers.c: https://github.com/mpv-player/mpv/blob/v0.42.0/video/out/gpu/d3d11_helpers.c
- [S30] video/out/w32_common.c: https://github.com/mpv-player/mpv/blob/v0.42.0/video/out/w32_common.c
- [S31] video/out/vo_gpu_next.c: https://github.com/mpv-player/mpv/blob/v0.42.0/video/out/vo_gpu_next.c
- [S32] video/out/vo.c: https://github.com/mpv-player/mpv/blob/v0.42.0/video/out/vo.c
- [S33] video/out/win32/displayconfig.c: https://github.com/mpv-player/mpv/blob/v0.42.0/video/out/win32/displayconfig.c

**libplacebo v7.387.0**
- [S34] Tag v7.387.0: https://code.videolan.org/videolan/libplacebo/-/tags/v7.387.0
- [S35] src/d3d11/swapchain.c: https://code.videolan.org/videolan/libplacebo/-/blob/v7.387.0/src/d3d11/swapchain.c

**PresentMon**
- [S36] PresentMon console application README (v2.6.0): https://github.com/GameTechDev/PresentMon/blob/main/README-ConsoleApplication.md

**Unofficial (labelled where used)**
- [S37] ElevenForum, VRR toggle registry value `VRROptimizeEnable`: https://www.elevenforum.com/t/enable-or-disable-variable-refresh-rate-for-games-in-windows-11.12052
- [S38] Blur Busters forum, fractional refresh listings vary by driver: https://forums.blurbusters.com/viewtopic.php?p=84915
