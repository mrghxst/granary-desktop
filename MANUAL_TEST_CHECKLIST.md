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

## Display & audio output (Phase 7, ADR-0006 amendment 1)
Automated evidence: `docs/evidence/phase-7/` (unit tests for mode selection and restore in `crates/display` and
`playback/output.rs`; `display_roundtrip` on this laptop; `scripts/output-evidence.mjs` 17/17 in the real app).
Before each block: note every monitor's HDR state and refresh rate (Settings → System → Display → Advanced display),
so you can confirm they are back afterwards. Run on AC power: Windows will not enable HDR on battery.

### A. OLED desktop monitor (primary target), Windows HDR normally ON
1. [ ] Settings → Display: HDR mode "HDR content only", refresh "Automatic"; your monitor is listed with its current
       refresh, HDR state and DXGI peak. If the monitor runs VRR (G-SYNC/FreeSync) for video, tick "Uses VRR (G-SYNC/FreeSync) for video".
2. [ ] Play an HDR10 title. Press `i`: "output dynamic range" HDR, "target peak" ≈ the monitor's peak (or your
       override), "source dynamic range" hdr10. Windows HDR was already on, so it is **not** toggled (no black flash).
3. [ ] Highlights: compare a bright scene with the Windows HDR Calibration result; if clipped or dim, set the
       per-monitor peak override (e.g. the calibrated max nits), stop, play again; `target-peak` follows.
4. [ ] Play an SDR title with HDR on: whites match the desktop's SDR whites side by side (windowed);
       `hdr-reference-white` in the stats equals Windows "SDR content brightness".
5. [ ] Dolby Vision title (profile 5 or 8): stats show "Dolby Vision P5/P8" and "dolby-vision-rpu active"; colours
       are not green/purple (P5) and not washed out.
6. [ ] 23.976 fps title on a 120/144/165/240 Hz monitor with VRR **off**: the log line "display output decided" shows
       whether the current rate already fits or an exact multiple (e.g. 119.88 Hz) was chosen; no stutter in a slow
       pan (`frame-drop-count` stays 0 in the stats over 60 s).
7. [ ] Same with VRR ticked: no refresh change at all; stats show `video-sync=audio`.
8. [ ] Stop playback: settings stay for 10 s (start the next episode within that time: no second black flash), then
       the monitor returns to its original refresh/HDR state.
9. [ ] HDR off in Windows, mode "HDR content only": HDR10 title switches HDR on **before** the first frame (no SDR
       flash of the first picture), and HDR is off again 10 s after stopping. Mode "Off": HDR is never touched and the
       title is tone-mapped to SDR.
10. [ ] Two monitors (one SDR, one HDR): drag the playing window to the other monitor; within ~1 s the log shows the
        old monitor restored and a new decision for the new one; the picture adapts (tone-mapping target changes).
11. [ ] Quit Granary while a switched refresh/HDR state is active (close the window): the monitor is restored
        immediately. Kill it instead (Task Manager → End task): on the next start the log says "restored display
        state left over by a previous run" and the monitor is back before the window appears.
12. [ ] Audio: device list shows your outputs; switching device or channels in Settings during playback applies
        without restarting the file (Settings hint: "Audio settings apply immediately").

### B. TV / AV receiver (HTPC, opt-in features)
1. [ ] Settings → Display: refresh "Match content" (or per-monitor override for the TV only). Play 23.976, 25, 29.97
       and 50 fps clips: the TV's info banner shows 23.976/24, 25, 29.97/30 and 50 Hz respectively; the original mode
       comes back 10 s after stopping.
2. [ ] Passthrough on, codecs AC-3/E-AC-3/DTS (and TrueHD/DTS-HD only with exclusive mode on): the receiver display
       shows "Dolby Digital"/"DTS"/"Atmos" instead of PCM; stats show `audio-spdif` with the codec list. With an
       unsupported codec (e.g. AAC) playback falls back to PCM without silence.
3. [ ] Exclusive mode on: other apps' sounds are muted during playback; after stopping they play again.
4. [ ] Channels "5.1" / "7.1" with PCM: the receiver shows multichannel PCM; with stereo speakers on "Stereo" a 5.1
       title downmixes with dialogue audible (normalise downmix on) and without clipping.
5. [ ] HDR TV with Windows HDR off: an HDR title switches HDR on before the first frame; the TV shows its HDR badge;
       restored after stopping.
6. [ ] Receiver switched off during playback (device disappears): playback continues silently
       (`audio-fallback-to-null`), no crash; switching it on and re-selecting the device restores sound.
