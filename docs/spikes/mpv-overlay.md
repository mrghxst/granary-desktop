# Phase 2 spike: libmpv + transparent WebView2 overlay + HDR (2026-11-06)

Code: `spikes/mpv-overlay/` (throwaway). Design under test: ADR-0003 **Design A**, a single Tauri window holding a
`GranaryVideo` Win32 child HWND at the bottom of the z-order, with libmpv rendering into it via `wid`
(`vo=gpu-next`, `gpu-api=d3d11`, `hwdec=d3d11va`, `d3d11-flip=yes`). Above it sits the `WRY_WEBVIEW` container, a
transparent WebView2 (`WEBVIEW2_DEFAULT_BACKGROUND_COLOR=00000000`, window `transparent: true`), hidden with
`SW_HIDE` when idle.

Machine: Intel Arc 140V laptop with one internal 2880×1800 HDR-capable panel at 175% scaling (DPI 168), 60 Hz
(120 Hz mode available), Windows HDR **off** by default. No second monitor, no VRR, no PresentMon. libmpv:
`mpv v0.42.0-3-g902818753`, FFmpeg `N-128504-g9f690c6a4`, libplacebo `v7.387.0`.

> **Corrections after the gate review** (`docs/reviews/phase-2.md`, 2026-11-06). These supersede the original text
> below where the two conflict:
> - **H1 fixed.** The overlay re-showed itself every ~3 s: hiding the WebView makes Windows send a `WM_MOUSEMOVE` to
>   `GranaryVideo` for the stationary cursor. `video_proc` now ignores mouse moves at an unchanged position. A new
>   check, "overlay stays hidden 10s without input", passes: `reshown=0`, and `overlay_visible_samples=0` across all
>   120 pacing samples (`evidence/selftest-sdr-h1fix-sync-audio.txt`). That run was taken with the **session locked**,
>   so the mouse-wake and keyboard checks were skipped by the safety guard, and its pacing numbers (41 drops) are not
>   representative. Both the input path and overlay-hidden pacing are re-verified with the real app in Phase 6, and
>   are listed in `MANUAL_TEST_CHECKLIST.md`.
> - **Pacing claims (M1).** Every pacing run in the table under finding 7 was taken *before* the H1 fix, so the
>   overlay was cycling between visible and hidden. Only the overlay `display-resample` run and the overlay
>   `video-sync=audio` runs survive in saved files (`evidence/selftest-*.json`). The standalone `tools/pace.rs` numbers
>   came from console output that was not saved. "The overlay adds no measurable cost" is **withdrawn** and is not
>   established. `tools/pace.rs` also does not use exactly the spike's options (it has no `wid` and no
>   input-related options).
> - **Restore tool (H2) hardened.** The marker is keyed by adapter LUID + target id **and** the reboot-stable monitor
>   device path. An unreadable marker is kept. Refresh restore is verified by re-query. Ctrl+C or closing the console
>   repairs first. The `--with` child has a timeout. Evidence: `evidence/restore-safety.txt` (corrupt marker kept;
>   stale LUID matched by path; unknown display keeps the marker; child timeout → restored; Ctrl+C → repaired, rc 130)
>   and `evidence/hdr-roundtrip.txt`.
> - **HDR detection (M4).** "HDR on" now means `advancedColorEnabled && !wideColorEnforced`, and "HDR capable" means
>   `advancedColorSupported && !wideColorEnforced` (R4 rule). An SDR panel running ACM is no longer toggled. The
>   round-trip prints `ROUNDTRIP OK` only when rc is 0.
> - **APIs actually exercised (M3).** HDR was toggled only with legacy packet 10 (`SET_ADVANCED_COLOR_STATE`) and read
>   with packet 9; refresh was changed only through whole-Hz `ChangeDisplaySettingsExW(CDS_FULLSCREEN)`. Packets 15/16
>   and the rational `SetDisplayConfig` path from ADR-0006 are **untested**; they are verified in Phase 7.
> - **Independent flip (M2)** is still unmeasured, and so is the ADR-0003 fallback trigger. The window is created with
>   `transparent: true`. ADR-0003 is now "provisionally confirmed".
> - **SDR white (M5).** ADR-0006 now sets `hdr-reference-white` explicitly from the display's SDR white level
>   (packet 11) whenever Windows HDR is on, instead of relying on `auto`.
> - **Smaller corrections.** The HDR-on self-test run exited with code 2: the child exited 1, and `--with` maps a
>   failing child to 2. Its log also showed audio underrun and A/V desync warnings, which are expected while the
>   session is locked. "`WM_DPICHANGED` handler is wired" means only that the message triggers a `displays()` re-read;
>   no DPI-specific resizing exists yet (Phase 6). The ADR-0003 16/16 result needed `video-sync=audio`.
>   `tools/*.rs` are standalone files built by hand:
>   `rustc -O --edition 2024 tools/pace.rs -L ../../vendor-bin/libmpv -l mpv -o target/pace.exe`, with
>   `libmpv-2.dll` copied next to the executable.

## How it was run
| Run | Command | Result |
|---|---|---|
| Overlay self-test, SDR desktop, `video-sync=display-resample` | `SPIKE_SELFTEST=1 SPIKE_LABEL=sdr-desktop pnpm tauri dev` | 15/16; only the 30 s pacing check failed (see Pacing) → exit 1 |
| Overlay self-test, SDR desktop, `video-sync=audio` | `SPIKE_SELFTEST=1 SPIKE_LABEL=sdr-desktop-sync-audio SPIKE_MPV_OPTS=video-sync=audio pnpm tauri dev` | **16/16 → exit 0** |
| HDR toggle round-trip | `hdr-roundtrip.exe --hold-secs 3` | **ROUNDTRIP OK → exit 0** |
| Overlay self-test with Windows HDR on (release build) | `hdr-roundtrip.exe --with hdr=on -- granary-spike.exe` (`SPIKE_LABEL=hdr-on-sync-audio`) | 14/16; the 2 input checks were **skipped by the safety guard** because Windows locked the session mid-run (`LockScreenBackstopFrame` under the cursor). HDR restored and verified → `RESTORED` |
| Standalone mpv pacing baseline (no WebView) | `tools/pace.rs` (raw FFI, same options), 30 s steady state | see Pacing |
| `/DELAYLOAD:libmpv-2.dll` | `granary-spike.exe --probe-mpv` with and without the DLL | exit 0 (`client api 0x20006`) / exit 3 (clean error) |
| After review: self-test with the H1 fix, `video-sync=audio` | `SPIKE_SELFTEST=1 SPIKE_LABEL=sdr-h1fix-sync-audio SPIKE_MPV_OPTS=video-sync=audio pnpm tauri dev` | 14/17. **"overlay stays hidden 10s" PASS**. The session was locked, so the 2 input checks were skipped and pacing is invalid |
| After review: restore safety | `hdr-roundtrip.exe` scenarios 1–5 | see `evidence/restore-safety.txt`; all behave as specified |

The self-test (`selftest()` in `src-tauri/src/main.rs`) clicks the overlay's real buttons by emitting
`selftest-click` to the WebView, which calls the Tauri commands over IPC. It sends real `SendInput` mouse and keyboard
events, but only after checking that the spike window is foreground and under the cursor, so synthetic input never
reaches another application.

## Results against the Phase 2 checklist
| Requirement | Status | Evidence |
|---|---|---|
| mpv renders into the app-owned window, gpu-next / d3d11 / d3d11va | ✅ | `current-vo=gpu-next`, `current-gpu-context=d3d11`, `hwdec-current=d3d11va` |
| Transparent overlay above the video; pause / seek / fullscreen through IPC | ✅ | checks `overlay pause`, `overlay seek +3s`, `overlay fullscreen on` |
| Overlay follows move / resize / fullscreen / minimize | ✅ | `video child fills client` after each step; video rect == webview rect == client rect |
| Overlay follows DPI changes | ⚠️ partial | Correct at 175% (DPI 168), and the `WM_DPICHANGED` handler is wired. Changing scaling or moving to another-DPI monitor needs a second monitor or a system-setting change → MANUAL_TEST_CHECKLIST |
| Keyboard focus works | ✅ (SDR run) | real `VK_SPACE` (with scan code) → WebView `keydown Space` → IPC → `pause` flipped `no→yes` |
| Overlay truly hidden and woken by input | ✅ after the H1 fix (hidden ≥ 10 s, locked session); wake by input verified only before the fix | `WRY_WEBVIEW` `IsWindowVisible=false`; a real mouse move over the video wakes it (pre-fix run). Re-verify unlocked in Phase 6 |
| `current-vo` / `hwdec-current` / HDR clip `gamma=pq`, `primaries=bt.2020` | ✅ | all runs, `max-cll=1000` |
| drops / delayed near 0 over 30 s | ✅ with `video-sync=audio`, ⚠️ with display-resample at 60 Hz on this iGPU | see Pacing |
| HDR state, DXGI luminance, SDR white of every display | ✅ | `displays()` JSON logged at start and on monitor change |
| Windows HDR already on: HDR clip windowed **and** fullscreen with an HDR swapchain | ✅ | windowed: `New swap chain configuration received from hint: ... RGB_FULL_G2084_NONE_P2020`; fullscreen: target `gamma=pq primaries=bt.2020 max-luma=400` |
| Windows HDR on: SDR clip with correct SDR white | ⚠️ partial | The SDR clip is mapped into the PQ target (`gamma=pq`, `hdr-reference-white=auto`). The applied white level is not observable through mpv properties, the debug log or `screenshot window` (screenshots are rendered to sRGB; all four `hdr-reference-white` variants auto/80/200/203 were byte-identical, `tools/white.rs`). Follow-up: Desktop Duplication capture (FP16) in the Phase 7 display tests (needs an unlocked session), plus the manual check |
| HDR off → on → original, verified | ✅ | round-trip below; also used by every `--with hdr=on` run (4 runs today, all `RESTORED`) |
| Independent flip with overlay hidden in fullscreen | ⚠️ not measurable here | No PresentMon. Checked: mpv `Using flip-model presentation`, `R10G10B10A2` flip swapchain, and the overlay HWND hidden (`SW_HIDE`), which are the preconditions → MANUAL_TEST_CHECKLIST (PresentMon `PresentMode`) |
| Two or more monitors: move during playback | ⏭ skipped | one monitor; the `on_moved` hook logs the monitor and re-reads `displays()` (logged once at start) → MANUAL_TEST_CHECKLIST |
| Fallback (ADR-0003 B′/B) | not triggered so far | Design A works functionally. The fallback trigger (no independent flip with the overlay hidden) has not been measured yet; Phase 7 / manual check |

## Findings
1. **Design A works on WebView2 157 + Tauri 2.12.5.** Window tree (from the self-test):
   `main → WRY_WEBVIEW → Chrome_WidgetWin_0 → Chrome_WidgetWin_1 → {Chrome_RenderWidgetHostHWND, Intermediate D3D Window}`
   above `main → GranaryVideo → mpv`. The video child stays below the WebView after every resize, fullscreen and
   minimize (`SetWindowPos(HWND_BOTTOM)` in the `WM_SIZE` subclass).
2. **mpv creates its own `mpv` child window inside `wid` and disables it itself** (`enabled=false`), so mouse input
   reaches `GranaryVideo`'s window procedure. No input hooks are needed.
3. **Keyboard while the overlay is hidden:** keys go to `GranaryVideo`, which wakes the overlay. In the product the
   first key must also be forwarded to the UI as an event, so it is not swallowed. Synthetic keys need a scan code
   (`wScan`), otherwise `KeyboardEvent.code` is empty.
4. **Swapchain colour space follows the hint dynamically:** with HDR on, libplacebo starts with G22/P709, switches to
   G2084/P2020 on the first HDR frame, briefly falls back to G22 during minimize/restore and returns to PQ afterwards.
   No action is needed, but the product's logs should record these transitions.
5. **DXGI `MaxLuminance` is unstable around HDR toggles:** before the toggle it read 500, right after enabling HDR
   200, during the HDR-on runs 400, and after restoring 1000 (`MaxFullFrameLuminance` 200/500). Consequence for
   ADR-0006: re-query luminance with a **fresh DXGI factory** only after the mode change has settled (~1.5 s), log
   the value used, and prefer the per-monitor override. mpv itself used 400 as `max-luma` in HDR mode.
6. **HDR toggle via the legacy packet (`SET_ADVANCED_COLOR_STATE`, 10) works on Windows 11 26200.** It is verified
   by DisplayConfig *and* the DXGI colour space (12 = `RGB_FULL_G2084_NONE_P2020`) in 7–82 ms, and restored in
   14–61 ms. SDR white reads 80 nits with HDR off and 200 nits with HDR on.
7. **Pacing** (HDR10 2160p HEVC Main10 clip, 23.976 fps, 10 s looping, window 1100×640, 30 s steady state,
   increments within 1 s of a loop restart excluded):

   | Setup | drops | vo-delayed | mistimed |
   |---|---|---|---|
   | standalone mpv, display-resample, 60 Hz (2 runs) | 0 / 4 | 4 / 9 | 3 / 14 |
   | **spike overlay**, display-resample, 60 Hz (3 runs) | 1 / 4 / 4 | 2 / 9 / 9 | 2 / 14 / 14 |
   | standalone mpv, `video-sync=audio`, 60 Hz (3 runs) | 0 / 0 / 0 | 0 / 0 / 0 | – |
   | **spike overlay**, `video-sync=audio`, 60 Hz | 0 | 0 | – |
   | standalone mpv, display-resample, `deband=no` (3 runs) | 0 / 0 / 0 | 0 / 2 / 0 | 0 / 2 / 0 |
   | standalone mpv, display-resample, **120 Hz** (5× multiple) | 0 | 80 | 38 |
   | standalone mpv, SDR 1080p 23.976, display-resample, 60 Hz | 2 | 22 | 41 |

   Conclusions:
   - ~~The overlay adds no measurable cost~~ (withdrawn, see the corrections at the top: these runs had the overlay
     cycling, and the standalone numbers were not saved).
   - The variance comes from this **iGPU laptop**. At a non-integer ratio (60/23.976 ≈ 2.5) display-resample
     misses vsync deadlines occasionally. At 120 Hz the iGPU cannot sustain gpu-next at 2880×1800 per vsync.
   - mpv does not count `vo-delayed`/`mistimed` the same way in `audio` mode, so the comparable figure across modes is
     `frame-drop-count` (0 in audio mode vs. 0–4 with display-resample).
   - ADR-0006 keeps its order. Phase 7 makes `video-sync` per-monitor configurable and logs these counters during
     playback, so the policy can be re-checked on the primary target (desktop dGPU + OLED).
   - The HDR-on pacing run (1 drop, 0 delayed) happened while the session was locked and is **not** representative.
8. **`/DELAYLOAD:libmpv-2.dll` works with Rust** (`cargo:rustc-link-arg-bin=…=/DELAYLOAD:libmpv-2.dll` +
   `delayimp.lib`; dumpbin lists it under "delay load imports"). Without the DLL the exe starts and reports
   `The specified module could not be found (0x8007007E)` instead of a loader dialog. The explicit probe must use
   `LoadLibraryExW(…, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS)`: `LOAD_LIBRARY_SEARCH_APPLICATION_DIR` alone fails even
   with the DLL present, because libmpv's own imports (System32) are not searched. **Adopted** (ADR-0005 updated).
9. **Session lock is a real test hazard:** an unattended run can hit the lock screen. Every synthetic-input step
   checks `GetForegroundWindow()` and `WindowFromPoint()` first and skips (as FAIL with a reason) otherwise. The
   product's integration tests will keep this guard.
10. Tauri sync commands run on the main thread, so the video child (and anything else that calls `SetFocus`) is
    created and driven there; mpv's VO runs on its own thread (`tid` differs in the window tree), as expected.

## HDR toggle round-trip (`hdr-roundtrip.exe --hold-secs 3` → exit 0)
```
BEFORE: hdr_supported=true hdr_enabled=false sdr_white_nits=80 dxgi_max_luminance=500 dxgi_color_space=0 refresh=60.000085
marker written: C:\Users\mr\AppData\Local\Granary\spike-display-restore.json
---  (\\.\DISPLAY1) target 8388688: original hdr=false
flip to hdr=true -> Ok(()); verified=true after 81.6755ms; colorspace=Some(12) sdr_white=Some(200.0) max_lum=Some(200.0)
restore to hdr=false -> Ok(()); verified=true after 22.9169ms; colorspace=Some(0) sdr_white=Some(80.0)
ROUNDTRIP OK: all displays restored, marker deleted
AFTER:  hdr_enabled=false sdr_white_nits=80 dxgi_color_space=0 refresh=60.000085 (dxgi_max_luminance now reads 1000, see finding 5)
```
Marker: `%LOCALAPPDATA%\Granary\spike-display-restore.json`, written atomically (temp file + rename) before any
change; `--repair` applies it (crash path); deleted only after a verified restore.

`--with hz=120`: `set refresh 120 Hz -> Ok(()); now 120.000169` → after the command `restore refresh -> Ok(())`,
`WITH after: hdr=false refresh=60.0000845 -> RESTORED` (temporary `CDS_FULLSCREEN` change, restored with
`ChangeDisplaySettingsExW(name, NULL, 0)`).

## Logged property output (self-test excerpt, `spikes/mpv-overlay/spike.log`, runs 2 and 3)
```
selftest[sdr-desktop-sync-audio] displays: [{"gdi_name":"\\\\.\\DISPLAY1","friendly_name":"","adapter_luid":[67926,0],"target_id":8388688,"refresh_hz":60.0000845508658,"refresh_rational":[70963300,1182720],"hdr_supported":true,"hdr_enabled":false,"wide_color_enforced":false,"bits_per_color":10,"sdr_white_nits":80.0,"dxgi_max_luminance":500.0,"dxgi_max_full_frame_luminance":500.0,"dxgi_min_luminance":0.0004,"dxgi_color_space":0,"dxgi_bits_per_color":10,"desktop_rect":[0,0,2880,1800]}]
mpv[vo/gpu-next/d3d11:v] Queried output: \\.\DISPLAY1, 2880x1800 @ 10 bits, colorspace: RGB_FULL_G22_NONE_P709 (0)
mpv[vo/gpu-next/libplacebo:v] Initial swap chain configuration: format: R10G10B10A2_UNORM, color space: RGB_FULL_G22_NONE_P709.
selftest[sdr-desktop-sync-audio] HDR props: {"current-gpu-context":"d3d11","current-vo":"gpu-next","display-fps":"60.000085","display-names":"\\\\.\\DISPLAY1","estimated-display-fps":"<unavailable>","ffmpeg-version":"N-128504-g9f690c6a4","hwdec-current":"d3d11va","libplacebo-version":"v7.387.0 (v7.387.0-13-g9fabf6d-dirty)","mpv-version":"mpv v0.42.0-3-g902818753","video-params":"{\"pixelformat\":\"d3d11\",\"hw-pixelformat\":\"p010\",\"average-bpp\":24,\"w\":3840,\"h\":2160,\"dw\":3840,\"dh\":2160,\"crop-x\":0,\"crop-y\":0,\"crop-w\":3840,\"crop-h\":2160,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":1.000000,\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"bt.2020-ncl\",\"colorlevels\":\"limited\",\"primaries\":\"bt.2020\",\"gamma\":\"pq\",\"sig-peak\":4.926108,\"light\":\"display\",\"chroma-location\":\"mpeg2/4/h264\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"none\",\"min-luma\":0.005000,\"max-luma\":1000.000000,\"max-cll\":1000.000000,\"max-fall\":400.000000,\"prim-red-x\":0.680000,\"prim-red-y\":0.320000,\"prim-green-x\":0.265000,\"prim-green-y\":0.690000,\"prim-blue-x\":0.150000,\"prim-blue-y\":0.060000,\"prim-white-x\":0.312700,\"prim-white-y\":0.329000}","video-target-params":"{\"pixelformat\":\"rgb10a2\",\"w\":2240,\"h\":1260,\"dw\":2240,\"dh\":1260,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":\"-nan(ind)\",\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"rgb\",\"colorlevels\":\"full\",\"primaries\":\"bt.709\",\"gamma\":\"gamma2.2\",\"sig-peak\":0.394089,\"light\":\"auto\",\"chroma-location\":\"unknown\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"premul\",\"min-luma\":0.000400,\"max-luma\":80.000000,\"max-cll\":0.000000,\"max-fall\":0.000000}"}
CHECK PASS current-vo=gpu-next "gpu-next"
CHECK PASS hwdec-current!=no "d3d11va"
CHECK PASS hdr gamma=pq primaries=bt.2020 {"gamma":"pq","max-cll":1000.0,"primaries":"bt.2020"}
CHECK PASS video child below webview {"client":[2240,1260],"dpi":168,"hmonitor":65537,"main_rect":[352,352,2264,1324],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[364,404,2240,1260],"video_visible":true,"webview_rect":[364,404,2240,1260],"webview_visible":true}
CHECK PASS video child fills client {"client":[2240,1260],"dpi":168,"hmonitor":65537,"main_rect":[352,352,2264,1324],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[364,404,2240,1260],"video_visible":true,"webview_rect":[364,404,2240,1260],"webview_visible":true}
CHECK PASS overlay pause -> pause=yes "yes"
CHECK PASS overlay seek +3s {"delta_mod10":3.8854699999999998,"t0":3.837,"t1":7.72247}
CHECK PASS overlay fullscreen on + video fills {"client":[2880,1800],"dpi":168,"hmonitor":65537,"main_rect":[0,0,2880,1800],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[0,0,2880,1800],"video_visible":true,"webview_rect":[0,0,2880,1800],"webview_visible":true}
selftest[sdr-desktop-sync-audio] fullscreen HDR target: {"display-fps":"60.000085","frame-drop-count":"0","video-target-params":"{\"pixelformat\":\"rgb10a2\",\"w\":2880,\"h\":1620,\"dw\":2880,\"dh\":1620,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":\"-nan(ind)\",\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"rgb\",\"colorlevels\":\"full\",\"primaries\":\"bt.709\",\"gamma\":\"gamma2.2\",\"sig-peak\":0.394089,\"light\":\"auto\",\"chroma-location\":\"unknown\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"premul\",\"min-luma\":0.000400,\"max-luma\":80.000000,\"max-cll\":0.000000,\"max-fall\":0.000000}","vo-delayed-frame-count":"0"}
CHECK PASS fullscreen off {"client":[2240,1260],"dpi":168,"hmonitor":65537,"main_rect":[352,352,2264,1324],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[364,404,2240,1260],"video_visible":true,"webview_rect":[364,404,2240,1260],"webview_visible":true}
CHECK PASS resize 1100x640 -> video follows {"client":[1100,640],"dpi":168,"hmonitor":65537,"main_rect":[120,90,1124,704],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[132,142,1100,640],"video_visible":true,"webview_rect":[132,142,1100,640],"webview_visible":true}
CHECK PASS minimize/restore -> video follows {"client":[1100,640],"dpi":168,"hmonitor":65537,"main_rect":[120,90,1124,704],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[132,142,1100,640],"video_visible":true,"webview_rect":[132,142,1100,640],"webview_visible":false}
CHECK PASS overlay hidden (WRY_WEBVIEW SW_HIDE) {"client":[1100,640],"dpi":168,"hmonitor":65537,"main_rect":[120,90,1124,704],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[132,142,1100,640],"video_visible":true,"webview_rect":[132,142,1100,640],"webview_visible":false}
tree: 0x5e099c parent=0x680998 class=WRY_WEBVIEW visible=false enabled=true tid=31856 rect=(132, 142, 1100, 640)
tree: 0x7d04ac parent=0x5e099c class=Chrome_WidgetWin_0 visible=false enabled=true tid=31856 rect=(132, 142, 1100, 640)
tree: 0x220a62 parent=0x7d04ac class=Chrome_WidgetWin_1 visible=false enabled=true tid=1956 rect=(132, 142, 1100, 640)
tree: 0x3a096a parent=0x220a62 class=Chrome_RenderWidgetHostHWND visible=false enabled=true tid=1956 rect=(132, 142, 1101, 641)
tree: 0x96070c parent=0x220a62 class=Intermediate D3D Window visible=false enabled=false tid=19360 rect=(132, 142, 2240, 1260)
tree: 0x5f0960 parent=0x680998 class=GranaryVideo visible=true enabled=true tid=31856 rect=(132, 142, 1100, 640)
tree: 0xa307a2 parent=0x5f0960 class=mpv visible=true enabled=false tid=33468 rect=(132, 142, 1100, 640)
window under video centre: 0x5f0960 GranaryVideo (ours=true)
CHECK PASS mouse move wakes overlay {"client":[1100,640],"dpi":168,"hmonitor":65537,"main_rect":[120,90,1124,704],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[132,142,1100,640],"video_visible":true,"webview_rect":[132,142,1100,640],"webview_visible":true}
CHECK PASS keyboard space via WebView toggles pause {"after":"yes","before":"no"}
CHECK PASS 30s steady-state drops/delayed near 0 {"at_loop_restart":{"drops":0,"mistimed":0,"vo_delayed":0},"ctx":{"current-vo":"gpu-next","display-fps":"60.000085","estimated-vf-fps":"23.976024","video-sync":"audio"},"drops":0,"loops":3,"mistimed":0,"vo_delayed":0}
selftest[sdr-desktop-sync-audio] SDR props: {"hdr-reference-white":"auto","target-peak":"auto","video-params":"{\"pixelformat\":\"d3d11\",\"hw-pixelformat\":\"nv12\",\"average-bpp\":12,\"w\":1920,\"h\":1080,\"dw\":1920,\"dh\":1080,\"crop-x\":0,\"crop-y\":0,\"crop-w\":1920,\"crop-h\":1080,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":1.000000,\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"bt.709\",\"colorlevels\":\"limited\",\"primaries\":\"bt.709\",\"gamma\":\"bt.1886\",\"sig-peak\":0.000000,\"light\":\"display\",\"chroma-location\":\"mpeg2/4/h264\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"none\"}","video-target-params":"{\"pixelformat\":\"rgb10a2\",\"w\":1100,\"h\":618,\"dw\":1100,\"dh\":618,\"aspect\":1.779935,\"aspect-name\":\"16:9\",\"par\":\"-nan(ind)\",\"sar\":1.779935,\"sar-name\":\"16:9\",\"colormatrix\":\"rgb\",\"colorlevels\":\"full\",\"primaries\":\"bt.709\",\"gamma\":\"bt.1886\",\"sig-peak\":0.394089,\"light\":\"auto\",\"chroma-location\":\"unknown\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"premul\",\"min-luma\":0.000400,\"max-luma\":80.000000,\"max-cll\":0.000000,\"max-fall\":0.000000}"}
CHECK PASS sdr clip target params available {"alpha":"premul","aspect":1.779935,"aspect-name":"16:9","chroma-location":"unknown","colorlevels":"full","colormatrix":"rgb","dh":618,"dw":1100,"gamma":"bt.1886","h":618,"light":"auto","max-cll":0.0,"max-fall":0.0,"max-luma":80.0,"min-luma":0.0004,"par":"-nan(ind)","pixelformat":"rgb10a2","primaries":"bt.709","rotate":0,"sar":1.779935,"sar-name":"16:9","sig-peak":0.394089,"stereo-in":"mono","w":1100}
SELFTEST[sdr-desktop-sync-audio] SUMMARY 16/16 passed
selftest[hdr-on-sync-audio] displays: [{"gdi_name":"\\\\.\\DISPLAY1","friendly_name":"","adapter_luid":[67926,0],"target_id":8388688,"refresh_hz":60.0000845508658,"refresh_rational":[70963300,1182720],"hdr_supported":true,"hdr_enabled":true,"wide_color_enforced":false,"bits_per_color":10,"sdr_white_nits":200.0,"dxgi_max_luminance":400.0,"dxgi_max_full_frame_luminance":200.0,"dxgi_min_luminance":0.0001,"dxgi_color_space":12,"dxgi_bits_per_color":10,"desktop_rect":[0,0,2880,1800]}]
mpv[vo/gpu-next/d3d11:v] Queried output: \\.\DISPLAY1, 2880x1800 @ 10 bits, colorspace: RGB_FULL_G2084_NONE_P2020 (12)
mpv[vo/gpu-next/libplacebo:v] Initial swap chain configuration: format: R10G10B10A2_UNORM, color space: RGB_FULL_G22_NONE_P709.
mpv[vo/gpu-next/libplacebo:v] New swap chain configuration received from hint: format: R10G10B10A2_UNORM, color space: RGB_FULL_G2084_NONE_P2020.
selftest[hdr-on-sync-audio] HDR props: {"current-gpu-context":"d3d11","current-vo":"gpu-next","display-fps":"60.000085","display-names":"\\\\.\\DISPLAY1","estimated-display-fps":"<unavailable>","ffmpeg-version":"N-128504-g9f690c6a4","hwdec-current":"d3d11va","libplacebo-version":"v7.387.0 (v7.387.0-13-g9fabf6d-dirty)","mpv-version":"mpv v0.42.0-3-g902818753","video-params":"{\"pixelformat\":\"d3d11\",\"hw-pixelformat\":\"p010\",\"average-bpp\":24,\"w\":3840,\"h\":2160,\"dw\":3840,\"dh\":2160,\"crop-x\":0,\"crop-y\":0,\"crop-w\":3840,\"crop-h\":2160,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":1.000000,\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"bt.2020-ncl\",\"colorlevels\":\"limited\",\"primaries\":\"bt.2020\",\"gamma\":\"pq\",\"sig-peak\":4.926108,\"light\":\"display\",\"chroma-location\":\"mpeg2/4/h264\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"none\",\"min-luma\":0.005000,\"max-luma\":1000.000000,\"max-cll\":1000.000000,\"max-fall\":400.000000,\"prim-red-x\":0.680000,\"prim-red-y\":0.320000,\"prim-green-x\":0.265000,\"prim-green-y\":0.690000,\"prim-blue-x\":0.150000,\"prim-blue-y\":0.060000,\"prim-white-x\":0.312700,\"prim-white-y\":0.329000}","video-target-params":"{\"pixelformat\":\"rgb10a2\",\"w\":2240,\"h\":1260,\"dw\":2240,\"dh\":1260,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":\"-nan(ind)\",\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"rgb\",\"colorlevels\":\"full\",\"primaries\":\"bt.2020\",\"gamma\":\"pq\",\"sig-peak\":1.970443,\"light\":\"auto\",\"chroma-location\":\"unknown\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"premul\",\"min-luma\":0.000100,\"max-luma\":400.000000,\"max-cll\":400.000000,\"max-fall\":0.000000,\"prim-red-x\":0.682634,\"prim-red-y\":0.315810,\"prim-green-x\":0.244992,\"prim-green-y\":0.714006,\"prim-blue-x\":0.140968,\"prim-blue-y\":0.050250,\"prim-white-x\":0.312701,\"prim-white-y\":0.329002}"}
CHECK PASS current-vo=gpu-next "gpu-next"
CHECK PASS hwdec-current!=no "d3d11va"
CHECK PASS hdr gamma=pq primaries=bt.2020 {"gamma":"pq","max-cll":1000.0,"primaries":"bt.2020"}
CHECK PASS video child below webview {"client":[2240,1260],"dpi":168,"hmonitor":65537,"main_rect":[88,88,2264,1324],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[100,140,2240,1260],"video_visible":true,"webview_rect":[100,140,2240,1260],"webview_visible":true}
CHECK PASS video child fills client {"client":[2240,1260],"dpi":168,"hmonitor":65537,"main_rect":[88,88,2264,1324],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[100,140,2240,1260],"video_visible":true,"webview_rect":[100,140,2240,1260],"webview_visible":true}
CHECK PASS overlay pause -> pause=yes "yes"
CHECK PASS overlay seek +3s {"delta_mod10":3.8693209999999993,"t0":3.295,"t1":7.164321}
CHECK PASS overlay fullscreen on + video fills {"client":[2880,1800],"dpi":168,"hmonitor":65537,"main_rect":[0,0,2880,1800],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[0,0,2880,1800],"video_visible":true,"webview_rect":[0,0,2880,1800],"webview_visible":true}
selftest[hdr-on-sync-audio] fullscreen HDR target: {"display-fps":"60.000085","frame-drop-count":"0","video-target-params":"{\"pixelformat\":\"rgb10a2\",\"w\":2880,\"h\":1620,\"dw\":2880,\"dh\":1620,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":\"-nan(ind)\",\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"rgb\",\"colorlevels\":\"full\",\"primaries\":\"bt.2020\",\"gamma\":\"pq\",\"sig-peak\":1.970443,\"light\":\"auto\",\"chroma-location\":\"unknown\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"premul\",\"min-luma\":0.000100,\"max-luma\":400.000000,\"max-cll\":400.000000,\"max-fall\":0.000000,\"prim-red-x\":0.682634,\"prim-red-y\":0.315810,\"prim-green-x\":0.244992,\"prim-green-y\":0.714006,\"prim-blue-x\":0.140968,\"prim-blue-y\":0.050250,\"prim-white-x\":0.312701,\"prim-white-y\":0.329002}","vo-delayed-frame-count":"0"}
CHECK PASS fullscreen off {"client":[2240,1260],"dpi":168,"hmonitor":65537,"main_rect":[88,88,2264,1324],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[100,140,2240,1260],"video_visible":true,"webview_rect":[100,140,2240,1260],"webview_visible":true}
CHECK PASS resize 1100x640 -> video follows {"client":[1100,640],"dpi":168,"hmonitor":65537,"main_rect":[120,90,1124,704],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[132,142,1100,640],"video_visible":true,"webview_rect":[132,142,1100,640],"webview_visible":true}
mpv[vo/gpu-next/libplacebo:v] New swap chain configuration received from hint: format: R10G10B10A2_UNORM, color space: RGB_FULL_G22_NONE_P709.
mpv[vo/gpu-next/libplacebo:v] New swap chain configuration received from hint: format: R10G10B10A2_UNORM, color space: RGB_FULL_G2084_NONE_P2020.
CHECK PASS minimize/restore -> video follows {"client":[1100,640],"dpi":168,"hmonitor":65537,"main_rect":[120,90,1124,704],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[132,142,1100,640],"video_visible":true,"webview_rect":[132,142,1100,640],"webview_visible":true}
CHECK PASS overlay hidden (WRY_WEBVIEW SW_HIDE) {"client":[1100,640],"dpi":168,"hmonitor":65537,"main_rect":[120,90,1124,704],"monitor":"\\\\.\\DISPLAY1","video_below_webview":true,"video_rect":[132,142,1100,640],"video_visible":true,"webview_rect":[132,142,1100,640],"webview_visible":false}
tree: 0x1a0990 parent=0x5d0108 class=WRY_WEBVIEW visible=false enabled=true tid=34776 rect=(132, 142, 1100, 640)
tree: 0x3b096a parent=0x1a0990 class=Chrome_WidgetWin_0 visible=false enabled=true tid=34776 rect=(132, 142, 1100, 640)
tree: 0x500a9e parent=0x3b096a class=Chrome_WidgetWin_1 visible=false enabled=true tid=28372 rect=(132, 142, 1100, 640)
tree: 0x230964 parent=0x500a9e class=Chrome_RenderWidgetHostHWND visible=false enabled=true tid=28372 rect=(132, 142, 1101, 641)
tree: 0x1a0314 parent=0x500a9e class=Intermediate D3D Window visible=false enabled=false tid=26612 rect=(132, 142, 1100, 640)
tree: 0x1b0946 parent=0x5d0108 class=GranaryVideo visible=true enabled=true tid=34776 rect=(132, 142, 1100, 640)
tree: 0x65071e parent=0x1b0946 class=mpv visible=true enabled=false tid=28900 rect=(132, 142, 1100, 640)
window under video centre: 0x430658 LockScreenBackstopFrame (ours=false)
CHECK FAIL mouse move wakes overlay "skipped: another window covers the video centre"
CHECK FAIL keyboard space via WebView toggles pause "skipped: spike window is not foreground"
CHECK PASS 30s steady-state drops/delayed near 0 {"at_loop_restart":{"drops":0,"mistimed":0,"vo_delayed":0},"ctx":{"current-vo":"gpu-next","display-fps":"60.000085","estimated-vf-fps":"23.976024","video-sync":"audio"},"drops":1,"loops":3,"mistimed":0,"vo_delayed":0}
selftest[hdr-on-sync-audio] SDR props: {"hdr-reference-white":"auto","target-peak":"auto","video-params":"{\"pixelformat\":\"d3d11\",\"hw-pixelformat\":\"nv12\",\"average-bpp\":12,\"w\":1920,\"h\":1080,\"dw\":1920,\"dh\":1080,\"crop-x\":0,\"crop-y\":0,\"crop-w\":1920,\"crop-h\":1080,\"aspect\":1.777778,\"aspect-name\":\"16:9\",\"par\":1.000000,\"sar\":1.777778,\"sar-name\":\"16:9\",\"colormatrix\":\"bt.709\",\"colorlevels\":\"limited\",\"primaries\":\"bt.709\",\"gamma\":\"bt.1886\",\"sig-peak\":0.000000,\"light\":\"display\",\"chroma-location\":\"mpeg2/4/h264\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"none\"}","video-target-params":"{\"pixelformat\":\"rgb10a2\",\"w\":1100,\"h\":618,\"dw\":1100,\"dh\":618,\"aspect\":1.779935,\"aspect-name\":\"16:9\",\"par\":\"-nan(ind)\",\"sar\":1.779935,\"sar-name\":\"16:9\",\"colormatrix\":\"rgb\",\"colorlevels\":\"full\",\"primaries\":\"bt.2020\",\"gamma\":\"pq\",\"sig-peak\":1.970443,\"light\":\"auto\",\"chroma-location\":\"unknown\",\"stereo-in\":\"mono\",\"rotate\":0,\"alpha\":\"premul\",\"min-luma\":0.000100,\"max-luma\":400.000000,\"max-cll\":400.000000,\"max-fall\":0.000000,\"prim-red-x\":0.682634,\"prim-red-y\":0.315810,\"prim-green-x\":0.244992,\"prim-green-y\":0.714006,\"prim-blue-x\":0.140968,\"prim-blue-y\":0.050250,\"prim-white-x\":0.312701,\"prim-white-y\":0.329002}"}
CHECK PASS sdr clip target params available {"alpha":"premul","aspect":1.779935,"aspect-name":"16:9","chroma-location":"unknown","colorlevels":"full","colormatrix":"rgb","dh":618,"dw":1100,"gamma":"pq","h":618,"light":"auto","max-cll":400.0,"max-fall":0.0,"max-luma":400.0,"min-luma":0.0001,"par":"-nan(ind)","pixelformat":"rgb10a2","prim-blue-x":0.140968,"prim-blue-y":0.05025,"prim-green-x":0.244992,"prim-green-y":0.714006,"prim-red-x":0.682634,"prim-red-y":0.31581,"prim-white-x":0.312701,"prim-white-y":0.329002,"primaries":"bt.2020","rotate":0,"sar":1.779935,"sar-name":"16:9","sig-peak":1.970443,"stereo-in":"mono","w":1100}
SELFTEST[hdr-on-sync-audio] SUMMARY 14/16 passed
```

## Decisions carried forward
- ADR-0003: Design A **provisionally confirmed** (functional checks pass; independent flip still to be measured).
  B′/B stay documented as fallbacks.
- ADR-0005: `/DELAYLOAD:libmpv-2.dll` + `delayimp.lib` adopted, with a `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS` probe at startup.
- ADR-0006: luminance is re-queried with a fresh DXGI factory after a settle delay; `video-sync` becomes configurable
  per monitor; drop counters are logged during playback.
- New manual checks: independent flip (PresentMon), DPI change and monitor move, SDR white under HDR (visual).
