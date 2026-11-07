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

## Native playback (Phase 6, ADR-0008)
Automated evidence: `docs/evidence/phase-6/` (`scripts/playback-evidence.mjs` drives the real app against the mock
server over WebView2 DevTools: 2160p HDR10 HEVC and 1080p H.264 fixtures, direct play through the loopback relay,
`hwdec-current=d3d11va`, `gpu-next`, pause/seek/stop, next episode, progress acknowledged, one stop per session).
- [ ] Live server (needs `.env.local`): play a real library movie and an episode; the log line "playback plan
      accepted" shows `direct_play=true` and a stream path without transcode parameters; "mpv hardware decoding"
      shows `d3d11va`; stop half-way and check the resume point on the Silo web UI matches within 10 s.
- [ ] Live server: change a watched flag or resume point in the web UI while Granary shows the item; Granary's page
      updates within a few seconds (events socket).
- [ ] Transcode / HLS on a live server (pick a file the server will not direct-play, or cap the bitrate): the "Not
      direct play" notice names the reason, playback starts, seeking works, subtitles load.
- [ ] Idle cursor over the video (unlocked session): controls and cursor hide after ~2.5 s while playing; moving the
      mouse brings both back; keys act while hidden (Space pauses) without showing the controls.
- [ ] Paused: controls dim after 8 s and hide after 30 s.
- [ ] Media keys on a keyboard and the Windows media flyout (Win+A volume panel): play/pause, next/previous episode,
      title and series lines shown.
- [ ] Mini player (T) and fullscreen (f, double-click) round-trips keep the video aligned with the overlay.
- [ ] Mini player size (Gate 6 review M4): T shrinks the window to about 480x270 in the bottom-right corner of the
      current monitor, fully on screen and always on top; T again restores the previous size/position (and maximised
      state); stopping playback in mini or fullscreen mode returns to the normal window (L7).
- [ ] Track change on a transcode (live server, review M3): during a non-direct playback pick another audio track and
      an embedded subtitle; playback reloads at the same position with the new track (log: "tracks changed on the
      server; reloading"), no silent audio.
- [ ] Unreachable server during playback (pull the network cable or block the server): q closes the player at once;
      the log shows "server stop still pending; delivering in the background" (review M2).
- [ ] German UI: a refused file shows a German error card with the server reason in brackets (review M5).
- [ ] Up next on a real episode with a credits marker: the card appears at the marker, counts down 10 s and starts the
      next episode; Cancel keeps playing to the end; with autoplay off nothing happens.
- [ ] Trickplay previews on a server with trickplay generated: hover the seek bar; the thumbnail matches the frame
      after seeking there (tile arithmetic is UNVERIFIED, silo-api.md open question 10).
- [ ] Window background: with a slow-starting stream, the area behind the overlay is #141417 before the first frame,
      never white or a stale frame (Design A re-check, phase-3 review L2).

(Extended in Phase 7.)
