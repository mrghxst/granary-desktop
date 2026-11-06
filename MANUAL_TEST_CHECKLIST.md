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
- [ ] Overlay auto-hide with an idle cursor (review H1): start playback, leave the mouse resting over the video, wait
      15 s; the controls must hide once and stay hidden. Move the mouse; they must reappear. Press a key while hidden;
      the key must act (e.g. Space pauses) and wake the controls. (Phase 2 review)
- [ ] Pacing with the overlay hidden (review M1): unlocked session, fullscreen and windowed, 60 s each with the controls
      hidden; record `frame-drop-count` / `vo-delayed-frame-count` from the log for display-resample and
      `video-sync=audio`. (Phase 2 review)

## Browse UI (Phase 5, ADR-0001)
Automated evidence: `docs/evidence/phase-5/` (screenshots of the real app against the mock server, captured over the
WebView2 DevTools protocol by `scripts/ui-evidence.mjs`: widths 960 and 1920 and 3840 css px, 150 %/200 % device scale,
reduced motion, true-black). The items below still need human eyes or a live server.
- [ ] Side-by-side with the Silo web UI on the same server (needs a test server): home rows and hero, library grid
      and sort, item details (movie, series with season tabs, episode), search. Spacing, type scale, colours and
      card proportions must read as the same product. Note differences here.
- [ ] Snap layouts: hover the maximise button and Win+Z; snap to halves/thirds; the sidebar must collapse to the icon
      rail below 1024 css px and no horizontal scroll bar may appear.
- [ ] Scaling: change Windows scaling 100 % → 150 % → 200 % with the app open; text and focus rings stay crisp, no
      clipped controls. Move the window to a monitor with different scaling.
- [ ] 4K at 100 % scaling: content stops at 2400 css px and stays centred.
- [ ] Settings → Accessibility → Visual effects → Animation effects off: card hover, row scroll and tab changes must
      not animate.
- [ ] Keyboard only: from launch, Tab through server list → sign-in → profile → home → library → item → Play using
      only Tab/Shift+Tab/Enter/Space/arrow keys/Esc; every focused control shows a ring; the skip link works.
- [ ] True-black theme on the OLED panel: background and sidebar are #000 (no grey glow in a dark room).

(Extended in Phase 7.)
