# ADR-0004: HTTP, auth and token storage

- Status: Accepted
- Date: 2026-11-06
- Inputs: [silo-api.md](../research/silo-api.md) (R1), [silo-web-ui.md](../research/silo-web-ui.md) (R2), ADR-0001

## Context
- Silo `/api/v2` uses `POST /auth/login` → HS256 JWT access token (8 h) + refresh token (30 d); every call sends
  `Authorization: Bearer`; `POST /auth/refresh` rotates the pair. A 401 with `token_refresh_required` means refresh
  once and retry; 503 + `Retry-After` keeps tokens. Viewer routes need `X-Profile-Id` (+ `X-Profile-Token` for PIN
  profiles). Clients identify via `X-Silo-Device-*` / `X-Silo-Client*` headers (R1).
- `/api/v2` has no CORS headers; the WebSocket origin check rejects `http://tauri.localhost` (R2).
- Artwork is returned as root-relative signed paths; stream URLs carry `st` signatures and Android additionally sends
  Bearer + profile headers (R1).
- Tokens must never reach logs or the repo; the WebView is the least trusted component.

## Options
1. TS-side `fetch` from the WebView — blocked by missing CORS; tokens would live in WebView storage.
2. Tauri HTTP plugin from TS — avoids CORS but still exposes tokens to JS and needs broad URL scopes.
3. **Rust-side client** (`crates/silo-api`, `reqwest` with rustls) behind narrow Tauri commands; tokens stay in Rust.

Token storage: (a) plaintext config file; (b) Tauri store plugin; (c) **Windows Credential Manager via `keyring-core` + `windows-native-keyring-store`**
(DPAPI-protected, per-user).

## Decision
- **Rust-side HTTP and WebSocket** in `crates/silo-api` (`reqwest` + rustls, `tokio-tungstenite` for
  `/api/v2/events/ws`). The UI calls typed Tauri commands (`servers_*`, `auth_*`, `library_*`, `item_*`,
  `playback_*`); it never sees access or refresh tokens.
- **TLS** (L3): `reqwest` with rustls using the **platform verifier** (Windows certificate store), so servers behind
  a private/LAN CA that the user trusts in Windows work; TLS errors are mapped to clear, specific messages (expired,
  untrusted issuer, name mismatch). Plain `http://` is allowed only after an explicit warning in the add-server flow.
- **Credential scoping** (M4): Bearer/profile headers are attached **only** to requests whose origin (scheme, host,
  port) equals the configured server origin. Redirects are never followed (`reqwest` redirect policy `none`; a 3xx is
  reported as `redirected` so the user enters the final address). Stream and artwork URLs on other origins (proxy nodes, object storage) rely solely on their `st`
  signatures. For mpv, the host sets `http-header-fields` only when the stream URL is same-origin, and otherwise
  passes the signed URL without headers (mpv applies header fields to every request, including redirects).
- **Images** are served through a custom URI scheme handler named `granary-img`, which on Windows is reachable as
  `http://granary-img.localhost/<path>` (L9); the CSP lists it in `img-src`. Rust resolves the root-relative signed path
  against the active server, applies the credential-scoping rule above and streams bytes back. No CORS.
- **Login methods** (L4): password login only in v0.1. The device-code flow (`/api/v2/auth/device/start|poll`) and
  OAuth found in R2 are deferred; they must be documented in silo-api.md before use. `X-Profile-Token` (PIN-verified
  profile) is kept in memory only; the PIN is asked again after an app restart.
- **Tokens in Windows Credential Manager** via `keyring-core` 1.0 with `windows-native-keyring-store` 1.1 (service `Granary`, account
  `<server_id>/<user_id>`), storing the refresh token; the access token is kept in memory only. Server list and
  non-secret account metadata go to `%APPDATA%\Granary\servers.json`.
- **Refresh**: single-flight refresh on `token_refresh_required` or local expiry; on refresh failure with 401 the
  account is marked signed-out and the UI is told (`auth://revoked` event). Logout calls `POST /api/v2/auth/logout` (VERIFIED,
  silo-api.md) and deletes the credential.
- **Device identity**: random UUID v4 created on first run, stored in `%APPDATA%\Granary\device.json`; client name
  `Granary for Windows`, version from the bundle (`tauri.conf.json`), sent as `X-Silo-Device-Id`, `X-Silo-Device-Name`
  (the computer name), `X-Silo-Device-Platform: windows`
  and `X-Silo-Client: Granary for Windows`, `X-Silo-Client-Version`, `X-Silo-Client-Family: desktop` (silo-api.md §13).
- **Redaction**: a `Secret<String>` newtype with a redacting `Debug`; a tracing layer masks `Authorization`,
  `X-Profile-Token`, `st=`, `token=`, `password` patterns in every log line.
- Endpoints/headers marked UNVERIFIED in silo-api.md are isolated behind the `SiloApi` trait, so a mock
  implementation backs tests and UI development without a server.

## Consequences
- The IPC surface is a fixed list of commands with typed arguments; capability files allow only those.
- mpv receives the stream URL (plus headers only when same-origin) directly from Rust; the WebView never handles them.
- Offline/no-server development uses `MockSiloApi` with sanitized fixtures recorded from upstream docs/tests.
- The credential store is Windows-only here (target is Windows-only); tests use an in-memory credential store, and debug
  builds started with `GRANARY_MOCK_SERVER=1` use the in-process `MockSiloApi` (user `alice`/`pw`, Kids PIN `1234`)
  with an in-memory store, for UI development and end-to-end tests. Release builds ignore the variable.

## Amendment (Phase 4, 2026-11-06)
- `keyring` 4.x no longer builds without picking a store through its `v1`/CLI features, so Granary depends on
  `keyring-core` and `windows-native-keyring-store` directly (same Credential Manager entries, fewer dependencies).
- `reqwest` 0.13 with `rustls` (aws-lc-rs provider, `rustls-platform-verifier`); TLS failures are classified by walking
  the error source chain down to `rustls::Error` (expired, not yet valid, untrusted issuer, name mismatch, revoked).
- Login `401` is reported as `invalid_credentials`; the server uses problem id `invalid_token` for it (silo-api.md §13).

### Amendment 2 (Gate 4 review, 2026-11-06)
- A stored account is bound to the origin it signed in at. When a saved `server_id` is reached at a different origin,
  Granary revokes the session at the old origin, deletes the credential and asks the user to sign in again. The
  `server_id` comes from a public endpoint and cannot prove identity.
- Profile headers (`X-Profile-Id`, `X-Profile-Token`) are sent only on profile-scoped operations; account-level ones
  (profile list, PIN verification, current user, logout) carry only the bearer token.
- Signing in on a server that already has an account first ends the previous session.

### Amendment 3 (Phase 5, 2026-11-06): artwork
- The WebView never loads images from a server directly. Artwork references from catalog responses are rendered as
  `http://granary-img.localhost/<serverId>?u=<reference>` (Tauri asynchronous URI scheme `granary-img`). The host
  resolves the reference against the saved server origin, accepts only same-origin URLs or `https` URLs without
  user-info, fetches them **without** bearer, profile or device headers (artwork links are presigned with
  `exp`/`sig`), requires an `image/*` content type, caps the body at 16 MiB and answers with `nosniff`.
- The CSP `img-src` is therefore narrowed from `https: http:` to `'self' data: blob: http://granary-img.localhost
  granary-img:` (phase-3 review L3).
- Phase 5 E2E: `tauri-driver` on Windows needs a separately downloaded `msedgedriver` matching WebView2, which is
  outside the downloads approved for this project; Gate 5 uses the vitest UI flow (fake host) plus a host-side Rust
  integration test (`catalog::tests::login_browse_details_play_handoff`), as prompt.md allows.
