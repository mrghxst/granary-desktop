# ADR-0006: Display, refresh-rate, HDR and restore policy

- Status: Accepted (round-trip behaviour validated in Phase 2/7 tests)
- Date: 2026-11-06
- Inputs: [windows-display-audio.md](../research/windows-display-audio.md) (R4) §1–§4, §8; prompt PART B "Primary
  target setup" items 1, 3, 6, 8 and Phase 7; ADR-0002, ADR-0003

## Context
- **Spec desktop order** for refresh: VRR if active → exact multiple mode at the same resolution near the current
  refresh (e.g. 119.88/239.76 Hz for 23.976 fps) → mpv `video-sync=display-resample`. Never drop the desktop to 24 Hz
  by default; full matching down to 23.976 is opt-in for TVs. Always restore every monitor afterwards.
- **R4 findings that constrain this:**
  - There is **no API that reports VRR as enabled/active** (R4 §4.2); VRR must be a user-declared per-monitor setting.
  - `ChangeDisplaySettingsExW` takes whole hertz only; fractional rates need `SetDisplayConfig` with a rational
    `vSyncFreq`, applied as a temporary config (no `SDC_SAVE_TO_DATABASE`) and undone with
    `SDC_APPLY | SDC_USE_DATABASE_CURRENT` (R4 §4.1).
  - `display-resample` at the current refresh already gives smooth playback when the refresh is a near-multiple of
    the content rate; `video-sync-max-factor` (default 5) must be raised for 240/480 Hz panels (R4 §4.3).
  - R4's own proposal was "never change modes" with display-resample before an exact-multiple switch. This ADR
    instead follows the **spec's order** and uses R4's facts to define *when* a switch is worthwhile.
  - Windows HDR: legacy `GET/SET_ADVANCED_COLOR_INFO/STATE` (packets 9/10) are in the `windows` crate; Windows 11
    24H2+ `GET_ADVANCED_COLOR_INFO_2`/`SET_HDR_STATE` (15/16) are not and need hand-declared `#[repr(C)]` structs;
    minimum build unknown → probe at runtime (R4 §1). SDR white: packet 11, nits = raw/1000×80; mpv 0.42 reads it per
    monitor via `hdr-reference-white=auto` (R4 §2). Peak: DXGI `MaxLuminance` (small-window peak, right for OLED ABL).
  - With `wid` embedding mpv may not notice a monitor change without a resize; the host must push
    `display-fps-override` and per-monitor overrides itself (R4 §8.4).

## Options
1. Never change display modes; display-resample only (R4 default).
2. **Spec order with a worthwhile-switch rule** (below).
3. Always match content exactly (TV behaviour) — rejected as a desktop default by the spec.

## Decision — option 2
**Refresh strategy per monitor** (setting: `Auto (desktop)` default | `Match content exactly` | `Never switch`):

`Auto (desktop)`, evaluated for the monitor holding the player window when playback starts and again when the window
moves to another monitor:
1. **VRR declared** for this monitor (per-monitor setting "This monitor uses VRR for video", default *off* because it
   cannot be detected) → no mode change, `video-sync=audio`.
2. **Current refresh already fits**: if `R / f` is within 0.1% of an integer k ≥ 1 (R = current refresh as a
   rational, f = content fps) → no mode change, `video-sync=display-resample`.
3. **Exact multiple available**: among modes at the **same resolution and colour format** whose refresh R′ satisfies
   `R′/f` within 0.05% of an integer and `R′ ≥ 0.9·R`, pick the R′ closest to R → apply as a temporary
   `SetDisplayConfig` (validate first, read back after) and record it in the restore marker.
4. Otherwise → no mode change, `video-sync=display-resample`, `video-sync-max-factor = ceil(R/f)+1`.

`Match content exactly` (opt-in, TV/HTPC): pick the lowest mode at the same resolution with `R′/f` integral (prefer
k = 1: 23.976/24/25/29.97/50/59.94), `video-sync=audio`. `Never switch`: steps 1, 2 and 4 only.

**HDR mode** (setting: `Off` | `HDR content only` default | `Always`):
- If Windows HDR is already **on** for the monitor: never toggle; HDR content is output as HDR10
  (`target-colorspace-hint=auto`, `target-colorspace-hint-mode=target`), SDR content uses `hdr-reference-white=auto`.
- If **off** and the setting is `HDR content only` and the content is PQ/HLG/DV, or the setting is `Always`: enable HDR
  on that monitor **before the first frame** (load with `pause=yes`, switch, wait for the DXGI colour space to report
  HDR, then unpause) and restore afterwards. Packets 16/15 are tried first and fall back to 10/9.
- Tone mapping: `target-peak` = per-monitor override if set, else DXGI `MaxLuminance`; `tone-mapping=auto`,
  `hdr-compute-peak=auto`; content whose mastering peak/MaxCLL ≤ target peak is not compressed (libplacebo behaviour
  per R4). Without any HDR output path, mpv maps to the SDR target.

**Restore guarantee** (referenced by ADR-0002/0003):
- Before any change, write `%LOCALAPPDATA%\Granary\display-restore.json` with, per affected monitor, adapter LUID +
  target id + `monitorDevicePath`, the original refresh rational and resolution, and the original HDR state.
  Written atomically (temp file + rename).
- Restore on: playback stop, player-window move to another monitor (restore old monitor first), app exit, panic hook,
  `WM_ENDSESSION`, and Ctrl-C/console close in dev. Restore = HDR state back to original, then
  `SetDisplayConfig(SDC_APPLY | SDC_USE_DATABASE_CURRENT)` for refresh (falls back to setting the recorded rational
  mode explicitly); then verify by re-query and delete the marker.
- On every app start, if the marker exists (crash, power loss), restore it before showing UI and log `display
  state repaired`.

## Consequences
- `crates/display` owns all of this behind a `DisplayController` trait with a mock for unit tests of the selection
  and restore logic (Phase 7 gate).
- VRR users must tick the per-monitor VRR box once; README and settings explain why (no Windows API).
- Mode switches cause a brief black screen on some monitors; the default only switches when no near-multiple exists.
- Hardware round-trip tests on this machine (single HDR-capable internal panel, 60/120 Hz) cover HDR toggle and
  refresh switching; multi-monitor and VRR behaviour go to `MANUAL_TEST_CHECKLIST.md`.

## Spike amendments (2026-11-06, [mpv-overlay.md](../spikes/mpv-overlay.md))
- HDR toggle via packet 10 verified on Windows 11 26200 (on in 7–82 ms, restored in 14–61 ms, verified through
  DisplayConfig and the DXGI colour space). The refresh switch to 120 Hz and its restore were also verified.
- DXGI `MaxLuminance` changed across toggles (500 → 200 → 400 → 1000). Luminance is therefore read with a **fresh
  DXGI factory after a ~1.5 s settle delay**, and the value used is logged; the per-monitor override wins.
- Wait-for-HDR before unpausing: poll DisplayConfig + DXGI colour space (12) with a 10 s timeout, as the spike tool does.
- On the iGPU test laptop, display-resample at a non-integer ratio (60/23.976) showed 0–4 drops per 30 s, while
  `video-sync=audio` showed 0; standalone mpv behaves the same, so this is GPU load, not the overlay. The order
  above stays. `video-sync` is additionally a per-monitor advanced setting, and drop/delayed counters are logged
  every 30 s during playback for later tuning on the primary target.
