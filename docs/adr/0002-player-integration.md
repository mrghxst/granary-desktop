# ADR-0002: Player integration — in-process libmpv

- Status: Accepted (confirmed by the Phase 2 spike)
- Date: 2026-11-06
- Inputs: [shell-and-player.md](../research/shell-and-player.md) (R3), [windows-display-audio.md](../research/windows-display-audio.md) (R4),
  [packaging-and-licensing.md](../research/packaging-and-licensing.md) (R5), ADR-0005

## Context
- HDR output with libplacebo requires `vo=gpu-next` on `gpu-api=d3d11` with mpv owning the swapchain, so Windows HDR
  signalling (`target-colorspace-hint`, `hdr-reference-white=auto`, DXGI peak) works. The libmpv **render API**,
  including the D3D11 render backend new in mpv 0.42.0, runs on legacy `vo_gpu` only (mpv issue #10810 open), so it
  cannot be used for HDR with gpu-next. Every HDR-capable mpv frontend (Stremio shell-ng, Plex HTPC, Jellyfin CEF
  rewrite, jellium) embeds by window instead.
- Options: (a) libmpv in-process via Rust bindings, embedded with `wid`; (b) bundled `mpv.exe` controlled via JSON IPC
  on a named pipe, embedded with `--wid`.
- Rust bindings: `libmpv2` 7.0.0 (2026-10-30, maintained, used by Stremio's Rust/WebView2 shell) with `libmpv2-sys`
  4.0.1 (headers at client API 2.5; mpv 0.42 is 2.6 — compatible for the client API).
- The pinned shinchiro build ships no MSVC import library; `fetch-libmpv.ps1` generates `mpv.lib` from the DLL exports.
- A pre-spike smoke test on this machine (raw FFI, mpv-owned window) confirmed `current-vo=gpu-next`,
  `current-gpu-context=d3d11`, `hwdec-current=d3d11va`, HDR10 clip `gamma=pq primaries=bt.2020`, 0 dropped frames.

| Criterion | (a) libmpv in-process | (b) mpv.exe + JSON IPC |
|---|---|---|
| Control latency / property observation | Direct calls, typed observers | Pipe round trips, JSON parsing |
| Crash isolation | mpv/driver crash kills the app | Isolated |
| Startup | Fast (DLL already loaded) | Process spawn per session |
| Packaging | One DLL | mpv.exe (+ DLL or static) |
| `wid` embedding | Same process, same input queue | Cross-process child window (input-queue coupling) |
| Future composition mode (`display-swapchain`) | Possible | Impossible |

## Decision
**(a) In-process libmpv**, embedded by `wid` into an app-owned child HWND (ADR-0003). We use `libmpv2-sys` 4.0.1
(raw bindings) wrapped by our own small safe layer in `crates/player` rather than the high-level `libmpv2` API, so
that we control threading, the property-observer model and the event loop precisely. (If the high-level crate proves
sufficient during implementation we may use it; the boundary below is unaffected.) Linking uses the generated
`mpv.lib` with a `build.rs` link-search path to `vendor-bin/libmpv`; `libmpv-2.dll` ships next to the exe.

- All mpv access goes through a `PlayerBackend` trait (`load`, `command`, `set_property`, `get_property`,
  `observe` → event stream). An `mpv.exe` IPC backend can be added later for crash isolation without UI changes.
- One dedicated event thread runs `mpv_wait_event`; commands come from Tauri async commands; property changes are
  pushed to the UI over a Tauri `Channel`, throttled (time-pos ≤ 4 Hz).
- `config=no` (user mpv.conf opt-in only), `input-default-bindings=no`, `input-vo-keyboard=no`, `osc=no`,
  `terminal=no`; Granary's own `mpv.conf` defaults (R4 baseline) are applied as options at init.

## Consequences
- A libmpv or GPU driver crash takes down the app; a persisted display-restore marker (ADR-0003/Phase 7) ensures HDR/
  refresh state is repaired at next start.
- Unit tests run against a `MockBackend` that replays mpv events; real-mpv tests are tagged and run locally.
- libmpv's GPLv3 licence applies to the combined binary distribution (compatible with AGPL, ADR-0005).
