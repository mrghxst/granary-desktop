# ADR-0008: Native playback pipeline (relay, controller, host, overlay)

- Status: Accepted
- Date: 2026-11-07
- Inputs: prompt PART B Phase 6; ADR-0002 (in-process libmpv), ADR-0003 (Design A video child window), ADR-0004
  (Rust-side HTTP, tokens never leave the host); `docs/research/silo-api.md` §6–§10 and §13 (playback protocol v3,
  events socket, chapters/markers/trickplay, all `@6791aac`); Phase 6 evidence `docs/evidence/phase-6/`.

## Context
- The server negotiates every attempt (`startPlayback` → `PlaybackDecision` with a plan and media URLs), expects
  sequenced progress with the *source* position, a single stop with a client-minted `stop_id`, and replans for
  seeks/track changes/failures. Its media endpoints need bearer and profile headers (or self-authorising `st` URLs).
- ADR-0004 forbids handing tokens to anything outside the Rust host, and mpv's `http-header-fields` would put them in
  mpv's memory, logs and any redirect target.
- mpv events arrive on mpv's own thread; UI commands arrive from Tauri IPC; media keys from SMTC; timers from tokio.
  The protocol needs one strictly increasing sequence counter and exactly one stop per session.
- The overlay (WebView2, transparent) sits over the native video window (ADR-0003). A static overlay composited over
  the video breaks independent flip and costs power on OLED panels; the cursor must hide on an idle video.

## Decision
1. **Loopback media relay** (`playback/relay.rs`). Every server media URL of a plan is registered under a random
   128-bit route token; mpv opens `http://127.0.0.1:<ephemeral>/r/<token>/<name>`. The relay fetches upstream through
   the account's `Session` (bearer + profile headers, refresh on 401), forwards only range/conditional request headers
   and a fixed set of response headers, streams bodies, refuses any `Host` other than its own address (DNS rebinding),
   accepts only GET/HEAD, resolves relative HLS paths inside the registered directory only, follows no redirects and
   drops routes when playback stops. URLs on another origin go to mpv as-is, without credentials.
2. **One controller per attempt** (`playback/controller.rs`): capabilities (cached, refreshed once on
   `409 installation_changed`) → start with a client attempt id → load the plan (resume via `start`, audio/subtitle
   selection, external subtitles through the relay) → progress every 10 s while playing, on pause/resume, after seeks,
   every 5 min while paused (server liveness 45 s / 30 min) → exactly one stop (UUID `stop_id`, retried with the same
   id; `replayed` counts as success). `410`/`404` end the attempt; `409 progress_conflict` is never retried. Load
   failures replan with `failure_recovery` (at most 2 reloads and 1 renegotiation per session). The logged plan shows
   the stream URL **shape** only (path + query names), never query values.
3. **Single-task player host** (`playback/host.rs`): one tokio task owns the mpv core (created lazily, reused across
   files), the relay and the current controller; UI commands, mpv events, media keys and a 2 Hz tick arrive on one
   channel, so no playback state is shared between threads. The UI gets `player://state` (position throttled to
   4 Hz), `player://notice` (plan, error, ended, stopped) and `player://window`.
4. **mpv on a reused core**: `pause` is a global property and `keep-open` pauses at EOF, so the controller clears
   `pause` before every `loadfile`; precise seeks use `seek <t> absolute` (hr-seek default) instead of `+keyframes`,
   which snapped up to a GOP back in the evidence run. `hwdec-current` is logged once per session at the first frame
   (a reused core does not necessarily re-notify an unchanged value).
5. **Overlay hiding** (`playback/video.rs`, `PlayerView.tsx`): after 2.5 s of inactivity while playing (30 s while
   paused; dimmed to 40 % after 8 s paused) the UI calls `player_overlay(false)`; the host hides the WebView container
   (`SW_HIDE`) and the cursor over the video window. Mouse activity on the video window shows it again; keys typed
   while hidden are forwarded as virtual-key codes and mapped through the same keymap without waking the overlay.
   Window background `#141417` is kept for the non-video pages; `html[data-video-active]` makes the page transparent.
6. **Keyboard**: an mpv-like default map (`KEYMAP` in `src/lib/player.ts`, shown with `?`), plus SMTC media keys.
7. **Up next**: from the credits marker (or 20 s before the end, never later than end − 12 s) a 10 s countdown in
   media time starts the next episode (`listSeasonEpisodes` order; there is no next-up endpoint, silo-api.md open
   question 9). It is anchored at the first position seen while playing, so a resume inside the window gets the full
   countdown; a file that ends anyway hands over through the `ended` notice when autoplay is on.
8. **Events socket** (`playback/events.rs`): one socket per active server; event frames (`user_state.changed`,
   `catalog.item.changed`, ...) trigger a debounced in-place refetch of the visible page (data stays on screen when
   the refetch fails), `access_changed` returns to profile selection; reconnects with a fresh ticket after the 5-minute limit, with backoff on failures.
9. **Trickplay**: seek previews use `getWatchTrickplay` sheets loaded through the `granary-img` scheme; the tile
   arithmetic is UNVERIFIED (silo-api.md open question 10) and isolated in `trickplayTile`.

## Consequences
- mpv never sees a credential, and the relay is the single place that enforces same-origin media fetches; the cost is
  one extra loopback hop (direct play of the 2160p HDR10 HEVC fixture through the relay: `hwdec-current=d3d11va`, `frame-drop-count` 0 in
  `docs/evidence/phase-6/stats-hdr10.json`).
- The one-task host makes ordering trivial (sequence, stop-once) but every long operation inside it must be bounded;
  API calls carry client timeouts (6 s connect, 20 s per request; media reads 30 s).
- Reusing the mpv core keeps startup fast but requires resetting global state per file (pause today; the Phase 7
  display/HDR switching must follow the same rule).
- Against the in-process mock the events socket is unreachable (backoff warnings in the log, expected); a live server
  check needs `.env.local` (PROGRESS.md, Blocked / needs human).

## Amendment 1 (Gate 6 review, 2026-11-07)
The code and this record disagreed in places; this is what the code does:
- Capabilities are fetched at the start of each attempt (`Controller::begin`), not cached across attempts, and
  refreshed once on `409 installation_changed`. The `state` field of the capability is not checked.
- Recovery: up to 2 reloads of the same plan at the last confirmed position, then the old session is stopped and a
  **new start** is negotiated at that position (1 renegotiation). `failure_recovery` replans are not used.
- The resume position is the requested start until mpv has reported playback of the current load; a load that fails
  before the first frame reloads at the requested start, never at 0 (review M1).
- No server call blocks the host task for long (review M2): progress samples run in a spawned task with at most one
  in flight (a newer sample waits for it, so sequence order is kept); Stop unloads mpv at once and waits at most 4 s
  for the server stop, after which the delivery continues in the background with the same `stop_id`.
- Track menus: direct play switches `aid`/`sid` in mpv; remux/transcode plans ask the server for a
  `track_change` replan with `selected_tracks` (audio ordinal, subtitle `track_id` + `combined_index` from the
  plan inventory) and load the new plan at the current position (review M3).

## Alternatives considered
- **`http-header-fields` with the bearer token**: simplest, but leaks tokens into mpv (logs, redirects, crash dumps)
  and breaks ADR-0004. Rejected.
- **Custom mpv stream protocol (`stream_cb`)**: no loopback socket, but reimplements seeking/ranges/HLS in C callbacks
  on mpv's threads. Rejected for complexity; the relay gives the same isolation with standard HTTP semantics.
- **mpv's own OSC / a native overlay**: would not match the web-app look (ADR-0001) and duplicates UI logic.
