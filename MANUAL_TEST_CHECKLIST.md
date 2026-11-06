# Manual test checklist

Checks that need human eyes or hardware not available to the automated build (this machine: one HDR-capable internal
laptop panel, 60/120 Hz, no VRR, no second monitor, no AV receiver, no PresentMon). Fill in date/result per item.

## OLED desktop (primary target)
- [ ] Windows HDR Calibration profile: with a calibration profile applied, compare Granary's logged DXGI
      `MaxLuminance`/`MinLuminance` with the profile values; if they differ, set the per-monitor peak override and
      re-check highlight detail. (ADR-0003/0006, review phase-1 L14)
- [ ] Window straddling an SDR and an HDR monitor: drag the player window half-way; video must not flicker or show
      wrong colours; when released fully on one monitor, the logged display parameters must switch. (L14)
- [ ] Independent flip: fullscreen playback with controls hidden; run PresentMon (`PresentMon --process_name granary.exe`)
      and confirm `PresentMode` = `Hardware: Independent Flip` (or `Hardware Composed: Independent Flip`). Show the
      controls and confirm it drops to composed, then returns when they hide. (Phase 2 spike)
- [ ] SDR white under HDR: with Windows HDR on, set "SDR content brightness" to a clearly different value (e.g. 400
      nits); play an SDR title; its whites must match the Windows desktop's whites side by side (windowed). (Phase 2)
- [ ] DPI change: change scaling (e.g. 150% → 200%) during playback; the video must keep filling the window and the
      overlay must stay aligned. Move the window to a monitor with different scaling; same check. (Phase 2)
- [ ] Multi-monitor move during playback: drag the player to the other monitor; the log must show the new monitor's
      HDR state, peak luminance, SDR white and refresh decision; the old monitor's state must be restored. (Phase 2)

(Extended in Phase 7.)
