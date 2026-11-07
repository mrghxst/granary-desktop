# ADR-0002: Player integration — in-process libmpv

- Status: Accepted; to be validated by the Phase 2 spike (`docs/spikes/mpv-overlay.md`)
- Date: 2026-11-06
- Inputs: [shell-and-player.md](../research/shell-and-player.md) (R3), [windows-display-audio.md](../research/windows-display-audio.md) (R4),
  [packaging-and-licensing.md](../research/packaging-and-licensing.md) (R5), ADR-0005, ADR-0006

## Context
- HDR output with libplacebo requires `vo=gpu-next` on `gpu-api=d3d11` with mpv owning the swapchain, so Windows HDR
  signalling (`target-colorspace-hint`, `hdr-reference-white=auto`, DXGI peak) works. The libmpv **render API**,
  including the D3D11 render backend new in mpv 0.42.0, runs on legacy `vo_gpu` only (mpv issue #10810 open), so it
  cannot be used for HDR with gpu-next. HDR-capable mpv frontends embed by window instead: Stremio shell-ng (`wid`),
  the Jellyfin CEF rewrite and jellium (mpv-owned window); Plex HTPC very likely too (R3 marks this as an inference
  from its gpu-next/d3d11 support).
- Rust bindings: `libmpv2` 7.0.0 (2026-10-30, maintained, used by Stremio's Rust/WebView2 shell) with `libmpv2-sys`
  4.0.1 (pregenerated bindings, no libclang needed; headers at client API 2.5, mpv 0.42 is 2.6 — compatible for the
  client API).
- The pinned shinchiro build ships no MSVC import library; `fetch-libmpv.ps1` generates `mpv.lib` from the DLL exports.
- Pre-spike evidence: a raw-FFI smoke test against the pinned DLL on this machine (mpv-owned window) reported
  `current-vo=gpu-next`, `current-gpu-context=d3d11`, `hwdec-current=d3d11va`, HDR10 clip `gamma=pq
  primaries=bt.2020`, 0 dropped/delayed frames. Command and full output: [docs/spikes/libmpv-smoke.md](../spikes/libmpv-smoke.md).
  This proves the DLL and decode path, not the embedding; the embedding is what the Phase 2 spike validates.

## Options
1. **libmpv in-process** via Rust bindings, embedded with `wid` into an app-owned HWND.
2. **Bundled `mpv.exe`** controlled via JSON IPC on a named pipe, embedded with `--wid`.
3. libmpv **render API** into our own surface — rejected up front: `vo_gpu` only, loses gpu-next/libplacebo, DV and
   colorspace hinting (Context).

| Criterion | (1) libmpv in-process | (2) mpv.exe + JSON IPC |
|---|---|---|
| Control latency / property observation | Direct calls, typed observers | Pipe round trips, JSON parsing |
| Crash isolation | mpv/driver crash kills the app | Isolated |
| Startup | Fast (DLL already loaded) | Process spawn per session |
| Packaging | One DLL | mpv.exe (+ DLL or static) |
| `wid` embedding | Same process, same input queue | Cross-process child window (input-queue coupling) |
| Future composition mode (`display-swapchain`) | Possible | Impossible |

## Decision
**(1) In-process libmpv**, embedded by `wid` into an app-owned child HWND (ADR-0003). We use `libmpv2-sys` 4.0.1
(raw bindings) wrapped by our own small safe layer in `crates/player` rather than the high-level `libmpv2` API, so
that we control threading, the property-observer model and the event loop precisely. Linking uses the generated
`mpv.lib` with a `build.rs` link-search path to `vendor-bin/libmpv`; `libmpv-2.dll` ships next to the exe.

- All mpv access goes through a `PlayerBackend` trait (`load`, `command`, `set_property`, `get_property`,
  `observe` → event stream). An `mpv.exe` IPC backend can be added later for crash isolation without UI changes.
- One dedicated event thread runs `mpv_wait_event`; commands come from Tauri async commands; property changes are
  pushed to the UI over a Tauri `Channel`, throttled (time-pos ≤ 4 Hz).
- **Configuration layering** (L2): `config=no` (mpv never reads `%APPDATA%\mpv`). At init the host applies, in order:
  (a) the bundled defaults file via the `include=<resource dir>/mpv.conf` option (the R4 desktop baseline, shipped by
  the installer per ADR-0005); (b) settings-driven options computed in Rust (display/audio/subtitle settings,
  ADR-0006); (c) an optional power-user file `%APPDATA%\Granary\mpv.conf` via a second `include` if it exists.
  Always forced by the host regardless of files: `input-default-bindings=no`, `input-vo-keyboard=no`, `osc=no`,
  `terminal=no`, `wid=<video hwnd>`.

## Consequences
- A libmpv or GPU driver crash takes down the app; the persisted display-restore marker (ADR-0006) ensures HDR/
  refresh state is repaired at next start.
- Unit tests run against a `MockBackend` that replays mpv events; real-mpv tests are tagged and run locally.
- libmpv's GPLv3 licence applies to the combined binary distribution (compatible with AGPL, ADR-0005).

## Amendment 1 (2026-11-07, Phase 9): configuration layering as implemented
- The bundled defaults file is `crates/player/mpv.conf`, installed next to `granary.exe` (a bundle resource, also
  copied next to the debug exe by `tauri dev`). `options::desktop_baseline()` holds the same options and a unit test
  keeps them equal, so a missing file changes nothing.
- Order at `mpv_create` (`options::layered`): code baseline → `include=<exe dir>\mpv.conf` → settings-driven options →
  `include=%APPDATA%\Granary\mpv.conf` (only if it exists) → forced options (`wid`, `config=no`, input, OSC,
  terminal, media controls, screensaver). mpv parses an `include` as soon as it is set, so each step overrides the
  earlier ones and the forced options always win.
- mpv skips invalid lines of an included file and keeps the valid ones. If mpv still refuses to start with the user
  file, the host logs a warning and starts again without it. Evidence: `docs/evidence/phase-9/mpv-conf.txt`.
