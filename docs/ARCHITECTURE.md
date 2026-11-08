# Granary architecture

Granary is a Windows 11 x64 desktop client for the self-hosted Silo media server. It has two halves:

- a **Tauri 2 Rust host**, which owns everything that touches the network, the display or the player;
- a **React/TypeScript UI** in WebView2, which only renders and sends typed commands.

Playback goes through **libmpv**, rendered into a window that the app owns. While playing, the WebView becomes a
transparent overlay that draws the controls.

The decision records are the source of truth. This page is a map of them:

| ADR | Decision |
| --- | --- |
| [0001](adr/0001-ui-strategy.md) | Own React UI on Silo's design tokens (midnight-cinema, Plus Jakarta Sans, Tailwind 4; Radix was planned but the 0.1 menus are own components) |
| [0002](adr/0002-player-integration.md) | In-process libmpv via `libmpv2-sys`, own safe layer, option layering |
| [0003](adr/0003-window-overlay-architecture.md) | Video child window under a transparent WebView2 in one main window |
| [0004](adr/0004-http-auth-token-storage.md) | Rust-side HTTP/WebSocket, tokens in Windows Credential Manager, credential scoping |
| [0005](adr/0005-packaging-licensing-updates.md) | Pinned libmpv, AGPL, NSIS per-user installer, no auto-updater |
| [0006](adr/0006-display-refresh-hdr-restore-policy.md) | HDR/refresh policy per monitor, restore marker |
| [0007](adr/0007-workspace-toolchain-ci.md) | Cargo/pnpm workspaces, pinned toolchain, CI |
| [0008](adr/0008-native-playback-pipeline.md) | Loopback relay, per-attempt controller, single-task player host |

## Repository layout

```
apps/desktop/                 Tauri app
  src/                        React UI: routes/, components/, player/ (overlay), lib/ (typed IPC wrappers), i18n/
  src-tauri/src/              Rust host
    lib.rs                    startup order, plugins, artwork scheme, navigation guard, command registration
    silo.rs, catalog.rs       server/account/profile and browse commands (views without tokens)
    playback/                 host (player task), controller (one attempt), relay (127.0.0.1), video (child HWND),
                              output (display + audio policy), platform (SMTC, keep-awake, decoder probe), events
    settings.rs               settings.json, clamped and normalised
    about.rs, logging.rs, redact.rs, paths.rs
  src-tauri/capabilities/     least-privilege capability (one allow-* per command)
  src-tauri/windows/hooks.nsh NSIS uninstall hook ("delete app data")
crates/player/                libmpv FFI wrapper, PlayerBackend seam + mock, option layering, pure state machine
crates/display/               HDR / SDR white / refresh / DXGI luminance, policy, session, restore marker
crates/silo-api/              typed Silo /api/v2 client, sessions, credential store, events socket, mock server
scripts/                      fetch-libmpv, verify, release-build, install-test, secret-scan, test media, measurements
docs/                         ADRs, research (API contract with upstream citations), reviews, evidence, perf, security
spikes/                       Phase 2 risk spikes (excluded from the workspace)
```

## Process and window model

```
Granary main window (native decorations, Snap layouts)
├── WRY / WebView2 container      transparent background (WEBVIEW2_DEFAULT_BACKGROUND_COLOR=00000000)
└── GranaryVideo child HWND       HWND_BOTTOM, no WS_CLIPSIBLINGS*; mpv renders here via `wid` (D3D11 flip swapchain)
```

* The WRY container is a sibling over the whole client area. With `WS_CLIPSIBLINGS` the video was clipped wherever
the overlay was shown, so the transparent page showed the desktop behind the window (fixed in 0.1.1, ADR-0003
Amendment 1). `scripts/overlay-pixels.ps1` checks the composed screen pixels in the playback evidence run.

**Pages and layering**
- The library pages are opaque.
- While playing, the UI clears its background and shows only the overlay: controls, menus and statistics.
- The overlay hides itself after a short idle time. The host then gives keyboard focus to the video window and
  forwards keys back to the UI as `player://activity`, with the layout character resolved by `ToUnicode`.

**Window behaviour**
- Fullscreen and the mini player are host-side window operations (`player_fullscreen`, `player_mini`). The main
  window is subclassed for double-click, cursor and DPI handling.
- Only one instance runs: a second launch focuses the first (single-instance plugin).

## Startup order (`lib.rs`)

1. WebView2 transparency environment variable, set before WebView2 exists.
2. Redacted, rotated logging.
3. Panic hook: in release builds, panics abort, so the hook restores the displays first.
4. libmpv probe. The DLL is delay-loaded, so a missing or broken DLL becomes a UI error, not a loader dialog.
5. Tauri builder:
   - single-instance plugin first;
   - window state, without the fullscreen or visible flags;
   - `granary-img` scheme;
   - `setup`, which does the following:
     - repairs any leftover display restore marker;
     - creates the main window with the navigation guard;
     - installs the player host.
6. `app_ready` from the UI logs `ready cold_start_ms=…` (docs/perf.md).

## Data flow: browse

```
UI route ─invoke("catalog_browse", {serverId, query})─► catalog.rs ─► SiloState session ─► silo-api HttpSiloApi
                                                                            │  bearer + X-Profile-Token, refresh on 401
UI <──────── typed view (no tokens) ◄───────────────────────────────────────┘
<img src="http://granary-img.localhost/<serverId>?u=<ref>"> ─► scheme handler ─► resolve_artwork_url ─► fetch_image
```

**Endpoints.** Every Silo endpoint is listed in `docs/research/silo-api.md` with its upstream path@SHA (6791aac).
Models are lenient subsets: unknown fields are ignored and optional fields have defaults.

**Artwork.**
- The host fetches artwork without credentials.
- It is limited to the server origin or public `https` hosts, and redirects are never followed.
- Only raster types are served, with a sandbox CSP (see docs/security.md).

**Live updates.** The events socket (`playback/events.rs`) forwards `user_state.changed` and `catalog.item.changed` as
`server://event`. `useLoad` then reloads in place. Loads also retry when the network comes back (`online`).

## Data flow: playback (ADR-0008)

```
UI Play ─► player_play(request) ─► PlayerHost task ─► Controller
   Controller: negotiate plan (playback protocol v3, caps.rs) ─► register media URLs in Relay
               ─► Output::prepare (display policy + audio) ─► mpv loadfile http://127.0.0.1:<p>/r/<token>/<name>
   mpv ─► Relay ─► silo-api media.rs (Session headers only for same-origin URLs) ─► server
   mpv events (event thread) ─► host channel ─► state machine ─► player://state ─► UI overlay
   2 Hz tick ─► sequenced progress reports; stop exactly once (end, stop, switch, exit)
```

**One task owns the player.** A single long-lived Tokio task owns mpv, the relay and the current controller. UI
commands, mpv events, media keys and the tick all arrive on one channel, so playback state is never shared between
threads.

**The relay is mpv's only network.** mpv never sees a token or a signed server URL. The relay binds 127.0.0.1, checks
`Host`, and serves only registered 128-bit route tokens. It forwards only range and conditional headers.

**mpv options** are layered in this order:
1. the code baseline;
2. the bundled `mpv.conf`;
3. options driven by the settings;
4. the user `%APPDATA%\Granary\mpv.conf`;
5. `forced`: `wid`, `config=no`, no default bindings, no OSC, no scripts and no ytdl.

If the user file breaks mpv startup, Granary retries without it.

## Display and HDR (ADR-0006, `crates/display`)

**Policy.** `policy.rs` holds pure functions that decide the following for the monitor holding the player window:
- whether to switch Windows HDR on;
- which refresh rate to use: Auto (desktop), Match content, or Never;
- which mpv options to use: tone-mapping target peak (from DXGI, or overridden per monitor), SDR white and video sync.

**Session.** `session.rs` applies those decisions through `DisplayController`, implemented by `win.rs` with
DisplayConfig and DXGI. Before each change, it writes the original state to the **restore marker** (`marker.rs`,
atomic write).

**Restore.**
- It happens at stop, at exit, from the panic hook, and when the window moves to another monitor.
- The marker is deleted only after a read-back has verified the restore.
- After a crash, the next start repairs from the marker. It first checks that the owning process, identified by pid
  plus creation time, is really gone.

**Power.** On battery, Windows refuses to enable HDR. `power.rs` remembers that refusal per monitor and power source.

## UI (`apps/desktop/src`)

- **Routing:** hash routes (`#/servers/<id>/…`): `AddServer`, `Login`, `Profiles`, `ServerHome`, `Library`, `Item`,
  `Search`, `Settings` and `About`.
- **IPC:** `lib/silo.ts`, `catalog.ts`, `player.ts`, `settings.ts` and `host.ts` are the only places that call
  `invoke`. `ipc-contract.test.ts` checks their command and argument names against the Rust signatures, the
  capability and `build.rs`.
- **Player overlay:** `player/PlayerProvider.tsx` holds the playback context, next-episode logic and media keys.
  `PlayerView.tsx` holds the controls, menus, trickplay, the shortcuts dialog and the statistics.
- **Languages:** en and de through i18next. A parity test keeps the key sets identical.
- **Accessibility:** `eslint-plugin-jsx-a11y`, a skip link, labelled controls and live regions for status and errors.

## Persistent data

| Location | Contents |
| --- | --- |
| `%APPDATA%\Granary\` | `settings.json`, `servers.json` (no secrets), `device.json`, optional `mpv.conf` |
| `%LOCALAPPDATA%\Granary\` | install folder (per-user NSIS), `logs\` (daily, 7 kept, redacted), `shader-cache\`, `display-restore.json` while a display change is active |
| Windows Credential Manager | refresh token per account (`<server>/<user>.Granary`) |

## Testing

**Rust** (`cargo test --workspace`):
- Unit tests sit next to the code.
- The silo-api contract tests run against canned upstream responses.
- The host tests run against the in-process mock server (`silo-api::mock`): login, browse, details and the play
  hand-off.
- The display policy and the player state machine are pure functions with tests.

**UI** (vitest + Testing Library):
- route flows and components;
- i18n parity;
- the no-`<video>` rule;
- the IPC contract.

**Measured and manual evidence**
- Scripts drive the real app over the WebView2 debugging port: `measure-ttff.mjs`, `native-keys-evidence.mjs` and
  `measure-perf.ps1`. Results go to `docs/evidence/`.
- `install-test.ps1` checks a silent install and uninstall.
- Checks that need a person (real HDR panels, receivers, a real server) are in `MANUAL_TEST_CHECKLIST.md`.

**CI** (`.github/workflows/ci.yml`, Windows):
- The same steps as `scripts/verify.ps1`.
- `release.yml` builds the installer for `v*` tags and attaches it to a **draft** release.
