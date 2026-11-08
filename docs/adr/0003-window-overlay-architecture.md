# ADR-0003: Window and overlay architecture

- Status: Accepted — provisionally confirmed by the Phase 2 spike ([docs/spikes/mpv-overlay.md](../spikes/mpv-overlay.md)); the fallback trigger (independent flip) is still unmeasured
- Date: 2026-11-06
- Inputs: [shell-and-player.md](../research/shell-and-player.md) (R3), [windows-display-audio.md](../research/windows-display-audio.md) (R4), ADR-0002

## Context
- mpv must own a D3D11 flip-model swapchain in an HWND we provide (`wid`) for HDR (windowed, DWM-composed, and
  fullscreen). Controls are web UI in WebView2 and must sit above the video, be transparent, follow every window
  change (move/resize/minimize/fullscreen/DPI/monitor), and be **truly hidden** when idle so the video swapchain can
  get independent flip / MPO (OLED burn-in care and frame pacing).
- Tauri 2.12 (wry 0.57) hosts WebView2 in *windowed* mode inside a `WRY_WEBVIEW` child HWND; it exposes
  `WebviewWindow::hwnd()`, `transparent`, `owner`, `skip_taskbar`, `no_redirection_bitmap`.
- Prior art: Stremio shell-ng (Rust + WebView2 + libmpv2, HDR with gpu-next) uses **one top-level window**: mpv via
  `wid`, a WebView2 windowed controller with background alpha 0 on top. jellium uses an mpv-owned top-level window plus
  an overlay composed over it.

## Options
- **A — Single Tauri window, sibling children.** Our `GranaryVideo` Win32 child HWND at the bottom of the z-order,
  `WRY_WEBVIEW` (WebView2, background alpha 0) above it; mpv `wid=GranaryVideo`.
- **B — Owned transparent overlay window.** Video top-level window + a Tauri `WebviewWindow` owned by it
  (`transparent`, `skip_taskbar`, `no_redirection_bitmap`, not topmost), position synced on `WM_WINDOWPOSCHANGED`.
  **B′**: mpv creates its own top-level window (no `wid`) and the overlay is owned by it.
- **C — DirectComposition**: `d3d11-output-mode=composition` + `display-swapchain` bound to a DComp visual under the
  webview. Loses mpv's automatic HDR target detection; unproven.

| Aspect | A | B / B′ | C |
|---|---|---|---|
| Proven in production | Yes (Stremio) | Partially (jellium) | No |
| Alignment on move/resize/DPI | Automatic + one synchronous child resize | Manual sync, 1-frame lag | Manual |
| Taskbar / Alt+Tab / snap / minimize | Native | Owner only, activation handling | Native |
| Overlay fully hideable | Yes (`WRY_WEBVIEW` SW_HIDE / controller `IsVisible=false`) | Yes (`SW_HIDE`) | Yes |
| Independent flip with overlay hidden | Unverified (child-HWND swapchain) | B′: classic top-level case | Unverified |
| HDR auto-detection by mpv | Yes | Yes | No |
| Code size | Small | Medium | Medium–large |

## Decision
**Primary: Design A.** One Tauri main window (native decorations → snap layouts work). On player start the Rust host
creates a `GranaryVideo` child (`WS_CHILD|WS_VISIBLE|WS_CLIPCHILDREN`, no `WS_CLIPSIBLINGS`, see Amendment 1; black brush, `CS_DBLCLKS`) of the main HWND,
places it at `HWND_BOTTOM` beneath `WRY_WEBVIEW`, and passes it to mpv as `wid`. The main window is subclassed
(`SetWindowSubclass`) so `WM_SIZE`/`WM_DPICHANGED` resize the video child synchronously. The page background is
transparent while the player is active (`WEBVIEW2_DEFAULT_BACKGROUND_COLOR=00000000` + transparent body). Fullscreen uses
Tauri `set_fullscreen` (borderless), never mpv's fullscreen, `d3d11-exclusive-fs=no`. When controls auto-hide, the
`WRY_WEBVIEW` container is hidden with `ShowWindow(SW_HIDE)`; the `GranaryVideo` window procedure catches
`WM_MOUSEMOVE`/`WM_KEYDOWN`/`WM_LBUTTONDBLCLK`, re-shows the webview, restores focus and forwards the key.
The library UI and the player share the one WebView (the player overlay is a React route), so no second window or
capability set is needed.

**Fallback: Design B′ → B.** If the spike shows that the transparent WebView2 cannot show the video beneath it, or
that a child-HWND swapchain never reaches independent flip while the webview is hidden, switch to an owned overlay
window (B), with mpv-owned top-level video window (B′) if child swapchains cannot independent-flip. The
`OverlayHost` abstraction in the desktop app (show/hide/align) keeps this switch local.

**Research only: Design C**, revisited only if A and B both fail.

**Idle-state details** (L5):
- *Cursor*: when controls hide, the host hides the cursor by answering `WM_SETCURSOR` on `GranaryVideo` with
  `SetCursor(NULL)`; mpv's own `cursor-autohide` is set to `no` so only one component owns the cursor.
- *Focus*: when the webview is hidden the host calls `SetFocus(GranaryVideo)` so `WM_KEYDOWN` arrives there; on wake
  focus returns to the WebView2 container and the key is forwarded to the UI as a `player://key` event.
- *SMTC*: Granary implements System Media Transport Controls itself (title/artwork come from Silo metadata, not file
  tags); mpv `media-controls=no`.
- *Keep-awake*: the host calls `SetThreadExecutionState(ES_CONTINUOUS|ES_DISPLAY_REQUIRED)` only while playing and
  clears it on pause/stop; mpv `stop-screensaver=no`.
- Display/refresh/HDR changes and their restore marker are governed by ADR-0006.

## Consequences
- Multi-monitor moves need no position syncing; the host listens for `WM_MOVE`/`WM_DISPLAYCHANGE` and re-evaluates
  the monitor (HDR state, peak, SDR white, refresh strategy) when the window's `HMONITOR` changes.
- Open items carried into the spike / `MANUAL_TEST_CHECKLIST.md` (L14): effect of a Windows HDR Calibration profile on
  DXGI `GetDesc1` values; a window straddling an SDR and an HDR monitor (DWM down-conversion, only the monitor holding
  the window centre is reported).
- Independent flip verification needs PresentMon (not installed here) → recorded in `MANUAL_TEST_CHECKLIST.md`
  if it cannot be measured in the spike.
- While controls are visible, DWM composes the full-window transparent WebView2 over the video (GPU cost); controls
  therefore auto-hide after 2.5 s and avoid `backdrop-filter` over video.

## Spike result (2026-11-06)
Design A worked functionally on WebView2 157 / Tauri 2.12.5 (16/16 self-test checks on the SDR desktop **with `video-sync=audio`** — 15/16 with display-resample; HDR swapchain
windowed and fullscreen with Windows HDR on). Details and logs: [mpv-overlay.md](../spikes/mpv-overlay.md).
- mpv creates and **disables** its own `mpv` child inside `wid`, so mouse input reaches `GranaryVideo` without hooks.
- The `GranaryVideo` child must be created on the UI (main) thread; Tauri sync commands run there.
- The first key pressed while the overlay is hidden wakes it and must be forwarded as `player://key` (as decided above).
- The fallback B′/B has **not been triggered, but its trigger was not tested**: independent flip with the overlay hidden
  was not measured (no PresentMon), and the review found the overlay re-showing itself (fixed, H1). The window is
  created with `transparent: true`, which may itself affect the presentation mode. Phase 7 measures independent flip
  (PresentMon if obtainable, else the manual checklist); if it never engages in fullscreen with the overlay hidden,
  B′ is re-evaluated. DPI changes and multi-monitor moves remain manual checks.
- Review fix H1 (docs/reviews/phase-2.md): Windows re-sends `WM_MOUSEMOVE` for a stationary cursor when the window
  above it is hidden; the video window procedure must ignore moves at an unchanged position, or the overlay wakes
  itself every auto-hide cycle.

## Amendment 1 (2026-11-08): no WS_CLIPSIBLINGS on the video child
**Problem.** The first user of 0.1.0 reported that the movie disappeared whenever the controls were shown and came
back only when they hid. The window showed the desktop behind it instead of the picture.

**Cause.** `GranaryVideo` was created with `WS_CLIPSIBLINGS`. The `WRY_WEBVIEW` container is a sibling that covers the
whole client area, so the video was clipped everywhere while the container was shown. The transparent page then
showed the transparent main window, which is the desktop. With the container hidden (`SW_HIDE`), the video was
visible again. The spike and phase 6 missed this: the spike only checked `IsWindowVisible`, and the phase 6
screenshots came from CDP `Page.captureScreenshot`, which captures the WebView alone, never the composed screen.

**Fix.** The video child is created with `WS_CHILD|WS_VISIBLE|WS_CLIPCHILDREN`. It stays at `HWND_BOTTOM` beneath the
container, so the overlay still draws on top.

**Evidence.** `scripts/overlay-pixels.ps1` captures the composed screen (GDI `CopyFromScreen`) inside the client area
and counts saturated colour families on one row of the colour-bar clip (≥ 4 means the picture is visible; the desktop
gives 0). `scripts/playback-evidence.mjs` runs it with the overlay shown and hidden.

| Build | Overlay shown | Overlay hidden |
| --- | --- | --- |
| 0.1.0 (`WS_CLIPSIBLINGS`) | 0 families (fail) | 5 families |
| 0.1.1 (fix) | 5 families | 5 families |

The run with the fix is in [docs/evidence/overlay-fix](../evidence/overlay-fix) (19/19 checks). An earlier theory, alpha
from `d3d11-output-format`, was tested and ruled out.
