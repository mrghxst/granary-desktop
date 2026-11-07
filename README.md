# Granary

Granary is a native Windows desktop client **compatible with the [Silo](https://github.com/Silo-Server/silo-server)
media server**. It looks like the Silo web app and plays everything through mpv/libplacebo instead of a browser. That
gives it:

- direct play of 4K HDR10 and Dolby Vision remuxes with hardware decoding;
- correct HDR on OLED desktop monitors, also when Windows HDR is always on;
- smooth playback at high refresh rates and with VRR;
- opt-in audio passthrough for AV receivers.

> Granary is an independent project. It is **not affiliated with or endorsed by Silo Media L.L.C.**; "Silo" is their
> trademark. Granary is "compatible with Silo" and nothing more.

Status: **0.1.0, first private test release** (unsigned). See [`CHANGELOG.md`](CHANGELOG.md) and
[`PROGRESS.md`](PROGRESS.md).

## Features

**Connect and browse**
- Connect to your server: address check with clear TLS and plain-http warnings, password sign-in, and profiles with
  PIN.
- Sign-ins are kept in Windows Credential Manager. The UI never sees a token.
- Browse in the Silo web-app look: the server's home rows (such as continue watching), libraries with filters and
  sorting, search, and movie, series, season and episode pages with artwork.

**Playback** (libmpv in the app window, ADR-0002/0003/0008)
- `gpu-next` on D3D11 with d3d11va hardware decoding and libplacebo tone mapping to the panel's real peak brightness.
  The peak is read from DXGI and can be overridden per monitor.
- Server-side playback plans: direct play, remux or transcode.
- Progress reporting and resume points.
- Next episode, also across seasons.
- Intro and credits skip, chapters, and trickplay previews while seeking.
- Audio and subtitle track menus.
- Statistics overlay and mini player.
- Media keys and the Windows media overlay.

**Display handling for OLED HDR monitors** (ADR-0006)
- Optional automatic HDR switching and refresh-rate matching for the monitor the player is on.
- Every change is **always restored**, also after a crash, through a restore marker that is repaired at the next
  start.

**OLED-safe player**
- Controls and cursor hide on their own, and no static element stays on screen.

**Audio**
- Choice of output device, channel layout, exclusive mode, and opt-in passthrough (AC-3, E-AC-3, TrueHD, DTS, DTS-HD)
  for receivers.

**Everything else**
- English and German UI, keyboard navigation, and keyboard shortcuts that follow your keyboard layout.
- Onboarding, About (licences, exact source commit, update link), log viewer and redacted log export.
- An offline banner with automatic reload.
- Bundled mpv defaults (`mpv.conf`), plus an optional power-user `%APPDATA%\Granary\mpv.conf`.

## Install

**Requirements**
- Windows 11 x64 with a CPU that supports AVX2 (x86-64-v3). That covers Intel Haswell (2013) and AMD Excavator or
  Zen, and later.
- A GPU with D3D11 video decoding.
- The WebView2 runtime. It ships with Windows 11; the installer downloads it if it is missing.

**Steps**
1. Download `Granary_<version>_x64-setup.exe` and its `.sha256` from the
   [releases page](https://github.com/mrghxst/granary-desktop/releases). Check the hash:
   `Get-FileHash -Algorithm SHA256 .\Granary_0.1.0_x64-setup.exe`.
2. Run the installer. It installs per user, so no administrator rights are needed. The build is not code-signed yet
   (see [`docs/signing.md`](docs/signing.md)), so SmartScreen shows "Windows protected your PC". Choose
   **More info → Run anyway**.
3. Start Granary from the Start menu, add your server address, and sign in.

**Uninstall**
- Use Settings → Apps → Granary.
- Tick "Delete the application data" to also remove settings, logs and the shader cache.
- Saved sign-ins are removed with **Remove** on Granary's start screen.

## Build from source

Prerequisites:
- Windows 11 x64.
- Visual Studio 2022 Build Tools with "Desktop development with C++".
- [rustup](https://rustup.rs). The toolchain is pinned in `rust-toolchain.toml` (1.99.1).
- Node.js 24 with corepack, for pnpm.
- 7-Zip.
- Optional: ffmpeg, for the test media.

From PowerShell in the repository root:

```powershell
corepack enable
pnpm install --frozen-lockfile
./scripts/fetch-libmpv.ps1          # pinned libmpv, SHA-256 verified, into vendor-bin/
./scripts/verify.ps1                # fmt, clippy -D warnings, tests, typecheck, lint, vitest, build, secret scan
pnpm --filter desktop tauri dev     # run the app
./scripts/release-build.ps1         # NSIS installer in target/release/bundle/nsis/
```

Debug builds can run without a Silo server: set `GRANARY_MOCK_SERVER=1`, sign in as `alice` / `pw`, and use Kids PIN
`1234`. For real playback against the mock, generate the test clips first with `./scripts/gen-test-media.ps1`.

[`CONTRIBUTING.md`](CONTRIBUTING.md) covers the workflow and the rules.

## Architecture

```
┌──────────────────────── Granary main window (Tauri 2) ────────────────────────┐
│  WebView2 (React UI, transparent while playing)    ← controls, menus, library │
│  ─────────────────────────────────────────────────────────────────────────────│
│  GranaryVideo child window  ← libmpv renders here (gpu-next, D3D11 swapchain) │
└───────────────────────────────────────────────────────────────────────────────┘
        ▲ typed Tauri commands / events (no tokens)
┌───────┴────────────── Rust host (apps/desktop/src-tauri) ─────────────────────┐
│ silo.rs / catalog.rs ── crates/silo-api ── HTTPS ──► Silo server              │
│ playback/ host · controller · relay (127.0.0.1) ── crates/player ── libmpv    │
│ crates/display ── HDR / refresh / DXGI luminance / restore marker             │
│ settings · redacting logs · About                                             │
└───────────────────────────────────────────────────────────────────────────────┘
```

The details are in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the decision records in
[`docs/adr/`](docs/adr). The Silo API contract that Granary relies on, with upstream citations, is in
[`docs/research/silo-api.md`](docs/research/silo-api.md).

## Player keyboard shortcuts

The defaults are mpv-like. Press `?` (or F1) during playback for the full list. Keys follow the active keyboard
layout. Media keys (play/pause, next, previous, fast-forward, rewind) work through Windows' media controls.

| Keys | Action |
| --- | --- |
| Space, p, k | Play / pause |
| ← / → | Seek back / forward (seek step, Settings) |
| Shift+← / Shift+→ | Small seek back / forward |
| ↓ / ↑ | Large seek back / forward |
| PageDown / PageUp | Previous / next chapter |
| 9 or / · 0 or * | Volume down / up |
| m | Mute |
| f, Enter, double-click | Fullscreen |
| Esc | Leave fullscreen / close menus |
| q | Stop |
| [ / ] / Backspace | Slower / faster / normal speed |
| z / x | Subtitle delay − / + |
| v / j | Subtitles on-off / next subtitle track |
| a, # | Next audio track |
| A | Cycle aspect ratio |
| i | Playback statistics |
| T | Mini player |
| N or > / P or < | Next / previous episode |
| o | Show controls |
| ? , F1 | Keyboard shortcuts |

## Documentation

- [`MANUAL_TEST_CHECKLIST.md`](MANUAL_TEST_CHECKLIST.md): checks that need a person, real displays or a real server.
- [`docs/security.md`](docs/security.md): security review, residual risks and dependency audits.
- [`docs/perf.md`](docs/perf.md): cold start, memory and time to first frame.
- [`docs/signing.md`](docs/signing.md): code-signing plan.
- [`docs/reviews/`](docs/reviews): independent review of every phase.

## License

Granary is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE).

It bundles libmpv (GPL-3.0-or-later build) and FFmpeg. Versions, source links and the other third-party licences are
listed in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md). The About screen links to the exact source commit of each
build.
