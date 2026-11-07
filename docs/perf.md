# Performance (Phase 8)

Measured on the development machine: Windows 11 Pro 26200, Intel Core Ultra laptop with Intel Arc 140V
graphics, internal 2880×1800 panel at 60/120 Hz. Every number has a script that reproduces it, and the raw
results are in `docs/evidence/phase-8/`.

## Results

| Metric | Target | Result | How |
|---|---|---|---|
| Cold start to the first interactive screen | < ~2 s | **778 ms** median wall clock (566–913 ms over 5 runs); host's own `cold_start_ms` median 721 ms | `scripts/measure-perf.ps1 -Exe target/release/granary.exe -Runs 5` → `perf.json` |
| Idle memory, after 30 s on the home screen | — | **455 MiB** working set / **227 MiB** private bytes over 7 processes; `granary.exe` itself is 58 MiB | same run |
| Time to first frame, SDR H.264 1080p25 | — | **1463 ms** median (first play 1648 ms) | `scripts/measure-ttff.mjs` (5 runs per clip) → `ttff.json` |
| Time to first frame, HDR10 HEVC Main10 2160p23.976 | — | **1607 ms** median (first play 2160 ms) | same run |

### Definitions
- **Cold start:** from process start to the `ready cold_start_ms=…` log line. The UI emits it once the first
  screen (home or onboarding) has rendered and accepts input.
- **Time to first frame:** from the play request reaching the host to mpv's first `playback-restart`. It includes
  plan negotiation with the server, the loopback relay, `loadfile`, demuxer probing and decoder/renderer start.
  - The host logs it as `first frame time_to_first_frame_ms=…`.
  - It also logs `file loaded since_play_ms=…`; the TTFF median for that step was 210–250 ms.
- The TTFF run uses the debug build against the in-process mock server, because release builds have no mock. Two
  settings isolate the player: HDR mode "off" and refresh "never". Display switching is a separate cost (below).
- The machine was on battery during the TTFF runs; the power state during the cold-start runs was not recorded.

## Display switching (part of the first frame when it happens)
Granary changes the display before `loadfile`, so the first frame is already in the final mode (ADR-0006). These
costs come on top of TTFF only when a switch is needed. The next episode within 10 s reuses the state, so it pays
nothing.

| Switch | Cost | Evidence |
|---|---|---|
| HDR on, refresh change and 1.5 s luminance settle (mains power) | 3.2 s | `docs/evidence/phase-7/display-roundtrip.txt` (`APPLIED in 3166 ms`) |
| HDR refused on battery: packet-16 poll, then 5 s read-back wait | 6.8 s, **once per run and power state** (later titles skip the attempt) | `docs/evidence/phase-7/display-roundtrip-battery.txt` |
| Restore at exit / after the idle grace | 0.1–1.6 s | both round-trip files |

## Hotspots and what was done
1. **Shader compilation on every playback (fixed).** With `config=no`, mpv had no cache directory. libplacebo
   therefore compiled its D3D11 shaders again on every `loadfile`, after the renderer was torn down at the previous
   stop.
   - Fix: `gpu-shader-cache-dir=%LOCALAPPDATA%\Granary\shader-cache`.
   - Effect on the TTFF median: SDR 1663 → 1463 ms (−12 %), HDR10 2160p 2070 → 1607 ms (−22 %).
   - The first play after installation still compiles: 1648 / 2160 ms.
   - Before/after data: `ttff-before-shader-cache.json`, `ttff.json`.
2. **HDR wait on battery (bounded, Gate 7 review M6).** Windows refuses HDR on battery, but the API call still
   succeeds. The wait was cut from 10 s to 5 s, and a refused switch is not tried again in the same power state.
3. **Cold start: no change needed.** At 0.78 s it is well under the 2 s budget. The frontend is a single 440 kB JS
   chunk; route splitting would save little at this size and would add a loading state to every first navigation.
4. **Idle memory: dominated by WebView2.** Six of the seven processes belong to WebView2 (browser, GPU, network,
   storage, utility, renderer), which is the cost of ADR-0001/0003. The Rust host is 58 MiB. mpv is created lazily
   at the first playback, so it adds nothing at idle.

## Remaining time to first frame
About 1.2 s of the remaining TTFF falls between mpv choosing the decoder (`Using hardware decoding (d3d11va)`) and
the first `playback-restart`. That window is renderer and swapchain setup for the child window, audio-output start
and the first decoded frame. Keeping the VO alive between files (`force-window`) could remove part of it. It would
also leave a black mpv surface behind the UI while idle, and it changes the overlay design (ADR-0003). So it is not
done for 0.1. Release-build numbers against a real server go in `MANUAL_TEST_CHECKLIST.md` (Polish).

## Reproduce
```powershell
pnpm --filter desktop tauri build --no-bundle          # target/release/granary.exe
./scripts/measure-perf.ps1 -Exe target/release/granary.exe -Runs 5
$env:GRANARY_MOCK_SERVER = "1"; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
pnpm --filter desktop tauri dev                        # in another terminal
node scripts/measure-ttff.mjs docs/evidence/phase-8 9333 5
```
