# Review: Phase 2 risk spike (mpv + overlay + HDR)

- Date: 2026-11-06
- Scope: commit 1739d20 (`git diff 61e7356..1739d20`): `docs/spikes/mpv-overlay.md`, ADR-0003/0005/0006 amendments,
  `MANUAL_TEST_CHECKLIST.md`, `spikes/mpv-overlay/**` (Rust host, `hdr_roundtrip` bin, `tools/*.rs`, frontend,
  Tauri config). Checked against prompt.md PART B Phase 2 and Gate 2, plus the non-negotiable rules. The claims were
  cross-checked against the local, gitignored `spikes/mpv-overlay/spike.log` (549 lines, 3 self-test runs) and the
  `selftest-*.json` files.
- Reviewer: fresh subagent (independent; did not write this code)

**Verdict: PASS WITH FINDINGS.** The retained log supports the three Gate 2 conditions:
- native playback: `current-vo=gpu-next`, `current-gpu-context=d3d11`, `hwdec-current=d3d11va`, HDR clip
  `gamma=pq`/`primaries=bt.2020`;
- overlay controls: pause, seek and fullscreen through IPC in all 3 runs, and real keyboard → WebView → IPC in both
  SDR runs;
- HDR detection: DisplayConfig + DXGI state logged with HDR off and on, and the PQ swapchain windowed and fullscreen.

Phase 3 may start. However, two High findings must be fixed before this design and the restore tool go into
Phase 6/7, and before `hdr-roundtrip` is run unattended again:
- H1: the overlay does not stay hidden while the cursor rests over the video;
- H2: the restore marker can be lost or become unusable.

Several doc/ADR claims also need correcting (M1–M5). No trademark or secret problems were found:
- product name "Granary Spike", window title "Granary spike", binaries `granary-spike` and `hdr-roundtrip`,
  identifier `io.github.mrghxst.granary.spike`;
- no credentials appear in the diff.

## Phase 2 item 3 coverage (summary)
| Bullet | Doc status | Reviewer assessment |
|---|---|---|
| mpv in app-owned window, gpu-next/d3d11/d3d11va | ✅ | Supported (spike.log 24–28, 122–124) |
| Overlay pause/seek/fullscreen via IPC | ✅ | Supported in all runs |
| Overlay follows move/resize/fullscreen/minimize | ✅ | Supported (geometry checks) |
| Overlay follows DPI | ⚠️ partial → manual | Correctly marked. "Handler is wired" is overstated (L3) |
| Keyboard focus | ✅ (SDR run) | Supported (spike.log 115–116, 293–294). Skipped in the HDR-on run, correctly marked |
| Overlay truly hidden | ✅ (SDR run) | **Not supported for sustained hiding**: self-wake loop (H1) |
| Property readback, drops near 0 over 30 s | ✅ audio / ⚠️ display-resample | Only partly verifiable from retained artifacts (M1) |
| HDR state, DXGI luminance, SDR white per display | ✅ | Supported (spike.log 4, 182, 369) |
| HDR on: HDR clip windowed + fullscreen, HDR swapchain | ✅ | Supported (spike.log 423, 154/163 target `gamma=pq`) |
| HDR on: SDR clip with correct SDR white | ⚠️ partial | Correctly marked. The ADR does not reflect it (M5) |
| HDR toggle round-trip | ✅ | Tool output not retained (M1). Restore-path weaknesses (H2) |
| Independent flip | ⚠️ not measurable → manual | Correctly marked, but the ADR-0003 status overclaims (M2). H1 blocks the precondition |
| Multi-monitor move | ⏭ skipped → manual | Correctly marked |

## High

### H1: the overlay re-shows itself every ~3 s while the cursor rests over the video, so it is never "truly hidden"
- **Location:** `spikes/mpv-overlay/src-tauri/src/video.rs:43-58` (`video_proc`: any `WM_MOUSEMOVE` →
  `set_webview_visible(true)`), `spikes/mpv-overlay/src/main.ts:42-49,69-72` (the `video-activity` event re-arms the
  3 s auto-hide timer). Evidence: local `spike.log` 300–318 (run `sdr-desktop-sync-audio`).
- **Issue:** the only callers of `spike_overlay(false)` are the "Hide" button and the `poke()` timer. The selftest
  clicks "Hide" once at 18.693 s. After that, `cmd overlay visible=false` is logged at 21.758, 24.790, 27.817, 30.859,
  33.904, 36.934, 39.980, 43.013 and 46.057 s. That is a strictly periodic 3.03 s cycle (3000 ms timer + IPC) with no
  synthetic input. Each hide requires a preceding `poke()`, which comes from a DOM `mousemove` (webview visible) or
  from `video-activity`, and `video-activity` calls `set_webview_visible(true)`. So the overlay was shown again
  between every pair of hides. The likely trigger: when `WRY_WEBVIEW` is hidden under a stationary cursor, Windows
  sends `GranaryVideo` a `WM_MOUSEMOVE` without real movement, and `video_proc` treats it as user activity. The run 1
  log (spike.log 122–126) shows the same pattern more sparsely. The HDR-on run does not show it, because the cursor
  was over the lock screen.
- **Why it matters:**
  - The ✅ "Overlay truly hidden" row rests on a single sample taken 700 ms after hiding (`main.rs:426-434`).
  - In fullscreen the cursor almost always rests over the video. The controls would then keep reappearing, which
    means OLED burn-in exposure, and DWM composition would never end, so independent flip (Phase 2 bullet, ADR-0003
    fallback trigger) can never occur.
  - The 30 s pacing runs labelled "overlay hidden" were in fact taken with the overlay cycling (see M1).
- **Fix:**
  - In `video_proc`, ignore `WM_MOUSEMOVE` whose client coordinates equal the last seen position. Store the last
    `lParam` and compare with GET_X/Y_LPARAM semantics.
  - Optionally require a movement threshold of a few pixels, and ignore moves within ~100 ms of the hide.
  - Add a self-test check that samples `IsWindowVisible(WRY_WEBVIEW)` every 250 ms for ≥10 s with the cursor parked
    over the video, and expects it to stay hidden.
  - Re-run the pacing check afterwards, and downgrade the doc row to ⚠️ until it passes.
  - Carry the rule into ADR-0003 "Idle-state details".

### H2: display-state restore is not crash-, Ctrl+C- or reboot-safe, and the doc claims "deleted only after a verified restore"
- **Location:** `spikes/mpv-overlay/src-tauri/src/bin/hdr_roundtrip.rs`:
  - 43-47 (`state()` matches by adapter LUID + target id only);
  - 70-108 (`repair()`): 76 (`unwrap_or_default` on parse), 85-90 (refresh restore result ignored), 103-106 (marker
    deleted when `rc == 0`);
  - 110-130 (no console control handler);
  - 266-273 (child command run without timeout).
  - Doc: `docs/spikes/mpv-overlay.md:110-111`.
- **Issue:**
  1. **Corrupt marker:** if the marker JSON fails to parse, `repair()` gets `Value::Null`, so the loop runs zero
     times, `rc = 0`, and the marker is deleted with "display state repaired". Nothing was restored.
  2. **Refresh is never verified in repair:** `restore_registry_mode` errors are only printed, not added to `rc`,
     and the refresh rate is not re-read. A failed refresh restore still deletes the marker.
  3. **Stale LUID after reboot:** adapter LUIDs are only guaranteed unique until restart (Win32 LUID semantics), and
     the HDR state set through packet 10 is persistent. Scenario: the process dies while HDR is flipped, then the
     machine reboots. `state()` then finds no display with that LUID, `set_hdr` fails, `rc = 1`, and the marker is
     kept forever. Every later `hdr-roundtrip` run (main path 125-130, `--with` 223-225) then exits 1, and the
     monitor stays in the changed HDR state. ADR-0006:61-62 requires `monitorDevicePath` in the marker for this
     reason, but the spike marker omits it.
  4. **No automatic repair on interruption:** there is no `SetConsoleCtrlHandler`. Ctrl+C or closing the console
     during `--with hdr=on -- granary-spike.exe` (~55 s of playback) terminates `hdr-roundtrip` and leaves Windows
     HDR toggled until someone runs `--repair` manually. `granary-spike.exe` does not check the marker at startup
     either.
  5. **Hung child:** `Command::status()` (268) has no timeout, so a hung child keeps HDR on indefinitely.
- **Why it matters:** "display/HDR changes always restored" is a non-negotiable rule. The owner's approval to switch
  HDR was conditional on restoring every monitor. The doc's safety claim (110–111) is false for the repair path. The
  same marker design is the template for `crates/display` (Phase 7). All the actual runs today did restore correctly
  (the main and `--with` paths verify before deleting), which is why this does not invalidate Gate 2.
- **Fix:**
  - **Marker contents and matching:** store `monitorDevicePath` (from `DISPLAYCONFIG_TARGET_DEVICE_NAME`) plus the
    original refresh rational and resolution. In repair, resolve the target by device path and fall back to the LUID.
  - **Repair semantics:** treat a parse failure as an error. Keep the marker, log it, and never delete it silently.
    Include the refresh restore result and a refresh re-read in `rc`.
  - **Interruption handling:**
    - install a console control handler that restores before exit;
    - add a timeout for the `--with` child;
    - make the spike app run the repair at startup, as ADR-0006:68-69 requires.
  - **Doc:** correct lines 110–111 to describe the actual guarantees.

## Medium

### M1: pacing and round-trip numbers cannot be verified from retained artifacts; one data pattern looks duplicated; the conclusion is mis-scoped
- **Location:** `docs/spikes/mpv-overlay.md:67-88` (pacing table and conclusions), 100-115 (round-trip block),
  20 ("same options"). Related: `spikes/mpv-overlay/tools/pace.rs:24-26` vs `src-tauri/src/main.rs:43-71`, and
  `hdr_roundtrip.rs:132-135,196-199`.
- **Issue:**
  1. `spike.log` contains exactly **one** overlay run with `display-resample`: line 131, drops/vo-delayed/mistimed =
     4/9/14. The table reports 3 overlay runs (1/4/4, 2/9/9, 2/14/14) and two standalone runs (0/4, 4/9, 3/14).
     Three of the five display-resample entries carry the identical triplet 4/9/14, and the only retained one is
     among them. The standalone `pace.rs` output, the `hdr-roundtrip` round-trip and the four `--with` runs printed
     to stdout only. No file in the repo or the gitignored tree contains `BASELINE`, `ROUNDTRIP OK` or
     `WITH after` output.
  2. The round-trip block (101–109) is condensed: the tool prints pretty JSON for BEFORE/AFTER. Yet it is presented
     as log output.
  3. `pace.rs` does not use "the same options". It omits `target-colorspace-hint-mode=target`,
     `video-output-levels=full`, `d3d11-exclusive-fs=no`, `tone-mapping`, `hdr-compute-peak` and `dither-depth`,
     and it adds `ontop=yes`.
  4. "The overlay adds no measurable cost" was measured during a phase labelled "overlay hidden" (`main.rs:495`).
     Because of H1 the overlay was actually cycling, and the visible-overlay composition cost was never measured
     on purpose.
- **Why it matters:** rule 1 (truth over speed) and Gate 2 ("contains the logged property output"). ADR-0006:85-88
  builds on this conclusion ("this is GPU load, not the overlay").
- **Fix:**
  - Re-run the pacing matrix after H1 is fixed, with every tool appending to a gitignored log file (`spike.log` or
    `tools/*.log`). Report only the runs whose output is retained.
  - Mark abridged blocks as abridged.
  - Correct "same options" or align `pace.rs`.
  - Scope the overlay-cost conclusion to "overlay hidden/cycling". Add a separate visible-overlay measurement or a
    manual check.

### M2: ADR-0003 is marked "confirmed" and "fallback not needed" although its own fallback criterion was not tested
- **Location:** `docs/adr/0003-window-overlay-architecture.md:3`, `:49-52` (fallback trigger: "a child-HWND
  swapchain never reaches independent flip while the webview is hidden"), `:33` ("Unverified (child-HWND
  swapchain)"), `:84`; `docs/spikes/mpv-overlay.md:45,187`.
- **Issue:** independent flip with the overlay hidden is exactly the condition that selects between A and B′. It was
  not measured (no PresentMon), and H1 shows the hidden precondition does not hold yet. The spike also runs the
  top-level window with `transparent: true` (`spikes/mpv-overlay/src-tauri/tauri.conf.json:19`). Whether a
  transparent top-level window with a child flip swapchain can be promoted to independent flip is part of what the
  manual PresentMon check must answer.
- **Why it matters:** the ADR reads as settled. If the manual check later fails, Phase 6 will have built on A without
  a recorded open risk.
- **Fix:**
  - Set the status to "Accepted — rendering, overlay and HDR confirmed by the Phase 2 spike; independent-flip
    criterion open (MANUAL_TEST_CHECKLIST, PresentMon)".
  - Add the open risk to PROGRESS.md "Known issues".
  - Extend the checklist item to record whether the window is transparent, and the PresentMon result with the
    cursor parked over the video after the H1 fix.

### M3: the ADR-0006 amendment says HDR toggling and refresh switching were "verified", but the spike exercised different APIs than ADR-0006 decides
- **Location:** `docs/adr/0006-display-refresh-hdr-restore-policy.md:80-81` vs `:55` ("Packets 16/15 are tried
  first and fall back to 10/9"), `:42-44` and `:65-66` (temporary `SetDisplayConfig` with a rational `vSyncFreq`,
  restore via `SDC_APPLY | SDC_USE_DATABASE_CURRENT`); `spikes/mpv-overlay/src-tauri/src/display.rs:189-210`
  (packet 10 only), `:228-293` (`ChangeDisplaySettingsExW` whole-Hz `CDS_FULLSCREEN`, restored with a NULL
  DEVMODE).
- **Issue:**
  - The primary HDR path (hand-declared `#[repr(C)]` packets 15/16, whose layouts R4 says come from `wingdi.h`
    because the Learn pages 404) was not exercised at all, even though this machine (Windows 11 26200) is a valid
    target for it.
  - The refresh path verified was CDS whole-Hz, not the `SetDisplayConfig` rational path that ADR-0006 requires
    for 119.88/23.976 Hz.
- **Why it matters:** the riskiest parts of the Phase 7 display work remain unvalidated, but the ADR suggests
  otherwise.
- **Fix:** reword the amendment to "legacy packet 10/9 and CDS whole-Hz verified; packets 15/16 and the
  `SetDisplayConfig` rational path are unverified and are the first Phase 7 hardware tests". Optionally add a quick
  packet-15 read to the spike's `displays()` while the hardware is at hand.

### M4: HDR detection uses `advancedColorSupported/Enabled` and ignores `wideColorEnforced`, contrary to R4's rule
- **Location:** `spikes/mpv-overlay/src-tauri/src/display.rs:164-166`; `hdr_roundtrip.rs:136,227`;
  `docs/research/windows-display-audio.md:168-175` (fallback rule `advancedColorEnabled && !wideColorEnforced`;
  ACM may make the legacy bit toggle WCG rather than HDR).
- **Issue:** `hdr_supported = bit0` and `hdr_enabled = bit1`. On an SDR wide-gamut panel with Auto Color Management,
  both bits are set, so the code reports "HDR on". `hdr-roundtrip` would then pick that monitor as "HDR-capable" and
  toggle ACM. Its `wait_for` would fail on the colour-space check, it would report rc=1, but the run would still
  print "ROUNDTRIP OK", because `all_restored` (189-195) compares only `hdr_enabled`. On this machine
  `wide_color_enforced=false`, so today's evidence is valid. The detection logic itself is not.
- **Why it matters:** "HDR detection works" is a Gate 2 condition that carries into the product. ADR-0006:51 ("if
  Windows HDR is already on: never toggle") would misfire on ACM SDR monitors, which are common as secondary desktop
  monitors.
- **Fix:**
  - Use packet 15 (`activeColorMode`, `highDynamicRangeSupported`) when available. Otherwise use packet 9 with
    `enabled && !wideColorEnforced`, and require DXGI colour space 12 as the HDR-on confirmation.
  - In `hdr_roundtrip`, select only displays where HDR is supported by that rule, and make `all_restored` and the
    printed verdict agree with `rc`.

### M5: SDR white under Windows HDR is unverified, but ADR-0006 still relies on `hdr-reference-white=auto` without saying so
- **Location:** `docs/spikes/mpv-overlay.md:41`; `docs/adr/0006-display-refresh-hdr-restore-policy.md:23-24,52`
  (no amendment); `spikes/mpv-overlay/src-tauri/src/main.rs:53`.
- **Issue:** the spike honestly marks SDR white as partial: `auto` is not observable, and screenshots are
  sRGB-rendered. ADR-0006's spike amendments do not record this, and the ADR keeps `auto` as the mechanism for a
  primary-target requirement ("match SDR reference white to the Windows SDR content brightness"). The spike already
  reads the authoritative value per display: `sdr_white_nits` from packet 11 (80 with HDR off, 200 with HDR on).
- **Why it matters:** if mpv's `auto` does not follow the Windows slider, SDR video in an HDR desktop will be dim or
  washed out on the owner's setup, and nothing in the pipeline would show it.
- **Fix:**
  - Amend ADR-0006: set `hdr-reference-white` explicitly from packet 11 per monitor (re-applied on monitor change),
    with `auto` only as a fallback. This makes the value observable via the property and in logs.
  - Keep the FP16 Desktop Duplication check (Phase 7) and the manual side-by-side check.

## Low

### L1: `unsafe` blocks without SAFETY comments
- **Location:** `spikes/mpv-overlay/src-tauri/src/mpv.rs:413,418,422,435,452`; `tools/pace.rs` and
  `tools/white.rs` (whole `main` in one `unsafe`, plus `unwrap()`/`expect()`).
- **Issue / impact:** spike code, but `mpv.rs` is the template for `crates/player`. Rule 11 and the project
  convention require justification.
- **Fix:** add SAFETY comments when porting, and wrap each FFI call individually.

### L2: Win32 window calls from non-UI threads
- **Location:**
  - `main.rs:561`: `video::set_webview_visible(true)` (ShowWindow + `SetFocus`) from the selftest thread. `SetFocus`
    has no effect for a window owned by another thread.
  - `main.rs:131,137`: `disable_mpv_children()`, so `EnableWindow` runs on mpv's window from the mpv event thread,
    which is a cross-thread `SendMessage`.
- **Impact:** finding 10 in the doc states the right rule, but the spike breaks it. The `disable_mpv_children` call is
  redundant: the log shows "disabled mpv child windows: 0" on every call, because mpv disables its window itself,
  which supports doc finding 2.
- **Fix:** in the product, marshal all HWND show/hide/focus calls through `run_on_main_thread`. Drop the
  `EnableWindow` backstop, or run it on the UI thread.

### L3: the DPI claim is overstated
- **Location:** `docs/spikes/mpv-overlay.md:34` ("the `WM_DPICHANGED` handler is wired"); `video.rs:94-98`.
- **Issue:** `WM_DPICHANGED` only calls the `MOVED` logging callback, and that callback logs only on a monitor-name
  change. The child is resized by the `WM_SIZE` that follows tao's own DPI handling. ADR-0003 says the subclass
  resizes on `WM_DPICHANGED`.
- **Fix:** reword to "DPI change relies on the subsequent `WM_SIZE`; untested". Implement explicit handling in the
  product if the manual check shows a lag.

### L4: keyboard and cursor handling in `video_proc` is incomplete for the product
- **Location:** `video.rs:44-58`.
- **Issue:**
  - `WM_KEYDOWN` is swallowed. This is acknowledged in doc finding 3.
  - `WM_SYSKEYDOWN`/`WM_SYSCHAR` (Alt+Enter, F10, Alt+F4 routing) fall through to `DefWindowProcW`.
  - `WM_SETCURSOR` cursor hiding (ADR-0003 idle-state) is not implemented.
- **Fix:** cover these in Phase 6 with tests, and forward keys as `player://key`.

### L5: the HDR-on run's exit code and anomalies are not stated
- **Location:** `docs/spikes/mpv-overlay.md:19`; `hdr_roundtrip.rs:270-272`; spike.log 425–481.
- **Issue:**
  - With 14/16 checks the child exits 1, so `hdr-roundtrip --with` returns 2. The doc prints "RESTORED" but not the
    exit code (rule 2).
  - Audio underruns start at 5.2 s and an A/V desync warning appears at 13.9 s. Both come before the lock screen was
    detected at 16.9 s, so "locked" may not be the only reason this run is unrepresentative.
- **Fix:** state `exit 2`, and mention the warnings.

### L6: the ADR-0003 spike result omits a qualifier
- **Location:** `docs/adr/0003-window-overlay-architecture.md:79`.
- **Issue:** the "16/16 self-test checks on the SDR desktop" result needed `video-sync=audio`. With the ADR-0006
  default (`display-resample`) the result was 15/16.
- **Fix:** add the qualifier.

### L7: marker durability nits
- **Location:** `hdr_roundtrip.rs:13`, `:39-40`.
- **Issue:**
  - `marker_path()` falls back to the CWD when `LOCALAPPDATA` is unset.
  - The temp file is not flushed (`sync_all`) before the rename, so a power loss can leave an empty marker. Combined
    with H2.1, that empty marker is then silently deleted.
- **Fix:** fail when `LOCALAPPDATA` is missing, and call `sync_all` before the rename.

### L8: the `tools/*.rs` build is undocumented
- **Location:** `spikes/mpv-overlay/tools/`.
- **Issue:** `tools/pace.rs` and `tools/white.rs` are not Cargo targets, and neither the doc nor a script says how
  they were built and run. The results therefore cannot be reproduced.
- **Fix:** add `[[bin]]` entries, or a one-line build command in the doc.

### Note (outside the reviewed diff)
The working tree has an uncommitted modification to `scripts/fetch-libmpv.ps1` (mirror-first download). It is not
part of 1739d20 and was not reviewed. Rule 9 requires it to be committed with its own check result.

## Resolution (2026-11-06, author)
| ID | Sev. | Resolution | Evidence |
|---|---|---|---|
| H1 | High | **Fixed.** `video_proc` ignores `WM_MOUSEMOVE` at an unchanged position (the stationary-cursor resend after the WebView hides). New self-test check "overlay stays hidden 10s without input", plus `overlay_visible_samples` counted during the pacing window. The rule is recorded in ADR-0003 for the Phase 6 video module. Re-verification with an unlocked session (mouse wake, key wake) is in `MANUAL_TEST_CHECKLIST.md` and Phase 6 | `docs/spikes/evidence/selftest-sdr-h1fix-sync-audio.txt`: `reshown=0`, `overlay_visible_samples=0` over 30 s (session locked, so input checks were skipped) |
| H2 | High | **Fixed** in `hdr_roundtrip.rs`: an unparsable or unreadable marker is kept (rc 1); refresh restore is verified by re-query; entries also carry the reboot-stable monitor device path and are resolved by it when the LUID changed; Ctrl+C / console close / logoff run the repair via `SetConsoleCtrlHandler`; the `--with` child has `--timeout-secs` (default 900) and is killed on expiry; marker fsync before rename. The same identity, corrupt-marker and fsync rules went into `crates/display` (Phase 3 code, with unit tests `stale_adapter_luid_is_resolved_by_monitor_path` and `corrupt_marker_is_kept`) | `docs/spikes/evidence/restore-safety.txt` scenarios 1, 2c, 3, 4, 5 (5 = real Ctrl+C: repaired in 7 ms, rc 130, marker gone); `evidence/hdr-roundtrip.txt` |
| M1 | Med | **Fixed (docs).** The pacing claims are re-scoped in a corrections block; "overlay adds no measurable cost" is withdrawn; "same options" is corrected in `tools/pace.rs`. The raw self-test JSON for every run is now committed under `docs/spikes/evidence/`. Overlay-hidden pacing is re-measured in Phase 6/7 (manual checklist item added) | `docs/spikes/mpv-overlay.md` corrections block |
| M2 | Med | **Fixed (docs).** ADR-0003 status is now "provisionally confirmed"; the untested fallback trigger and `transparent: true` are stated; Phase 7 measures independent flip | ADR-0003 "Spike result" |
| M3 | Med | **Fixed (docs).** ADR-0006 states that only packets 10/9 and whole-Hz `CDS_FULLSCREEN` were exercised; packets 15/16 and the rational `SetDisplayConfig` path are verified in Phase 7 | ADR-0006 "Spike amendments" |
| M4 | Med | **Fixed.** HDR on = `enabled && !wideColorEnforced`, HDR capable = `supported && !wideColorEnforced` (spike and `crates/display`); `ROUNDTRIP OK` only when rc is 0 | `spikes/.../display.rs`, `crates/display/src/win.rs`; round-trip rerun exit 0 |
| M5 | Med | **Fixed (ADR).** ADR-0006 now sets `hdr-reference-white` explicitly from the per-display packet-11 SDR white level when Windows HDR is on, and updates it on monitor change; implemented in Phase 7 | ADR-0006 "HDR mode" |
| L1 | Low | Fixed: SAFETY comments on every unsafe block in `mpv.rs`; file-level SAFETY note in `tools/*.rs` | — |
| L2 | Low | Accepted for the throwaway spike; the Phase 6 video module drives all Win32 window calls from the UI thread (the mpv-thread `EnableWindow` backstop is dropped, as the log shows it is redundant) | PROGRESS.md known issues |
| L3 | Low | Fixed (docs): wording corrected in the corrections block | — |
| L4 | Low | Deferred to Phase 6: the production `video_proc` handles `WM_SYSKEYDOWN` and `WM_SETCURSOR` | PROGRESS.md known issues |
| L5 | Low | Fixed (docs): HDR-on run exit 2 and the underrun/desync warnings are now mentioned | — |
| L6 | Low | Fixed: ADR-0003 says 16/16 needed `video-sync=audio` (15/16 with display-resample) | — |
| L7 | Low | Fixed: no current-directory fallback; `sync_all` before rename (spike and `crates/display`) | — |
| L8 | Low | Fixed (docs): exact `rustc` build line for `tools/*.rs`, verified (`rustc pace → exit 0`, `rustc white → exit 0`) | — |
