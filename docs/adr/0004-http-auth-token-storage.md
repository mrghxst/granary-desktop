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

Token storage: (a) plaintext config file; (b) Tauri store plugin; (c) **Windows Credential Manager via `keyring`**
(DPAPI-protected, per-user).

## Decision
- **Rust-side HTTP and WebSocket** in `crates/silo-api` (`reqwest` + rustls, `tokio-tungstenite` for
  `/api/v2/events/ws`). The UI calls typed Tauri commands (`servers_*`, `auth_*`, `library_*`, `item_*`,
  `playback_*`); it never sees access or refresh tokens.
- **TLS** (L3): `reqwest` with rustls using the **platform verifier** (Windows certificate store), so servers behind
  a private/LAN CA that the user trusts in Windows work; TLS errors are mapped to clear, specific messages (expired,
  untrusted issuer, name mismatch). Plain `http://` is allowed only after an explicit warning in the add-server flow.
- **Credential scoping** (M4): Bearer/profile headers are attached **only** to requests whose origin (scheme, host,
  port) equals the configured server origin. Redirects to other origins drop credentials (custom `reqwest` redirect
  policy). Stream and artwork URLs on other origins (proxy nodes, object storage) rely solely on their `st`
  signatures. For mpv, the host sets `http-header-fields` only when the stream URL is same-origin, and otherwise
  passes the signed URL without headers (mpv applies header fields to every request, including redirects).
- **Images** are served through a custom URI scheme handler named `granary-img`, which on Windows is reachable as
  `http://granary-img.localhost/<path>` (L9); the CSP lists it in `img-src`. Rust resolves the root-relative signed path
  against the active server, applies the credential-scoping rule above and streams bytes back. No CORS.
- **Login methods** (L4): password login only in v0.1. The device-code flow (`/api/v2/auth/device/start|poll`) and
  OAuth found in R2 are deferred; they must be documented in silo-api.md before use. `X-Profile-Token` (PIN-verified
  profile) is kept in memory only; the PIN is asked again after an app restart.
- **Tokens in Windows Credential Manager** via the `keyring` crate (service `Granary`, account
  `<server_id>/<user_id>`), storing the refresh token; the access token is kept in memory only. Server list and
  non-secret account metadata go to `%APPDATA%\Granary\servers.json`.
- **Refresh**: single-flight refresh on `token_refresh_required` or local expiry; on refresh failure with 401 the
  account is marked signed-out and the UI is told (`auth://revoked` event). Logout calls `POST /api/v2/auth/logout` (VERIFIED,
  silo-api.md) and deletes the credential.
- **Device identity**: random UUID v4 created on first run, stored in `%APPDATA%\Granary\device.json`; client name
  `Granary for Windows`, version from `CARGO_PKG_VERSION`, sent as `X-Silo-Device-Id/-Name/-Platform: windows`
  and `X-Silo-Client: Granary`, `X-Silo-Client-Version`, `X-Silo-Client-Family: desktop` (silo-api.md).
- **Redaction**: a `Secret<String>` newtype with a redacting `Debug`; a tracing layer masks `Authorization`,
  `X-Profile-Token`, `st=`, `token=`, `password` patterns in every log line.
- Endpoints/headers marked UNVERIFIED in silo-api.md are isolated behind the `SiloApi` trait, so a mock
  implementation backs tests and UI development without a server.

## Consequences
- The IPC surface is a fixed list of commands with typed arguments; capability files allow only those.
- mpv receives the stream URL (plus headers only when same-origin) directly from Rust; the WebView never handles them.
- Offline/no-server development uses `MockSiloApi` with sanitized fixtures recorded from upstream docs/tests.
- `keyring` behaviour is Windows-only here (target is Windows-only); tests use an in-memory credential store.
