# ADR-0003: Window and overlay architecture

- Status: Accepted (to be confirmed by the Phase 2 spike; update this ADR with spike results)
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
creates a `GranaryVideo` child (`WS_CHILD|WS_VISIBLE|WS_CLIPSIBLINGS`, black brush, `CS_DBLCLKS`) of the main HWND,
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

## Consequences
- Multi-monitor moves need no position syncing; the host listens for `WM_MOVE`/`WM_DISPLAYCHANGE` and re-evaluates
  the monitor (HDR state, peak, SDR white, refresh strategy) when the window's `HMONITOR` changes.
- Independent flip verification needs PresentMon (not installed here) → recorded in `MANUAL_TEST_CHECKLIST.md`
  if it cannot be measured in the spike.
- While controls are visible, DWM composes the full-window transparent WebView2 over the video (GPU cost); controls
  therefore auto-hide after 2.5 s and avoid `backdrop-filter` over video.
