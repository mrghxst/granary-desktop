# Review: Phase 6 Native playback & progress sync

- Date: 2026-11-07
- Scope: commits `31c58bd`, `8da66ae`, `4ed8496`, `037c1cc` (`git diff a1801af..037c1cc`, 89 files):
  - `crates/silo-api/src/{playback,caps,media,events,session,mock}.rs`, `tests/contract.rs` and the new fixtures;
  - `crates/player/src/{backend,state,options}.rs`;
  - `apps/desktop/src-tauri/src/playback/{controller,host,commands,relay,video,events,platform}.rs`, `lib.rs`,
    `build.rs`, `capabilities/main.json`;
  - `apps/desktop/src/{player/*,lib/player.ts,lib/errors.ts,i18n/*}` and the tests;
  - ADR-0008, `docs/research/silo-api.md` §13, `scripts/playback-evidence.mjs`, `docs/evidence/phase-6/`, README
    keyboard table, `MANUAL_TEST_CHECKLIST.md`, `THIRD_PARTY_NOTICES.md`.
- Checked against: `prompt.md` PART B (Non-negotiable rules, Phase 6, Gate 6), `CLAUDE.md`, `PROGRESS.md`,
  ADR-0002, ADR-0003, ADR-0004, ADR-0008, and upstream `.ref/silo-server@6791aac` (the OAS and
  `internal/playback/protocol_v3.go`, read as data only).
- Reviewer: fresh subagent (independent; did not write this code).
- Line numbers refer to `037c1cc` (`git show 037c1cc:<path>`). The working tree was being changed by Phase 7 work
  while this review ran, so current line numbers may differ.

**Verdict: PASS WITH FINDINGS.** No High finding. The phase is in good shape:
- every playback operation used exists in the OAS, and all 13 new §13 line citations are exact; every capability
  feature and claim name sent exists in `protocol_v3.go@6791aac`;
- no credential reaches mpv: mpv gets only `http://127.0.0.1:<port>/<128-bit route token>` from the loopback relay,
  which adds the bearer host-side, checks `Host`, allows only GET/HEAD and filters headers both ways;
- the stream URL is the plan's `original_http` URL with no transcode parameters, and the log records only the path
  and the query parameter *names* (`url_shape`);
- the controller follows protocol v3 closely: client-minted attempt id, `allow_alternate_versions: false`,
  strictly increasing sequence numbers, source-timeline positions, one `stop_id` reused on retry, `404/410` end the
  attempt, `409 progress_conflict` is not retried;
- no unjustified `unwrap()`/`expect()`, no TODO, en/de at parity (238/238), capabilities match the command list one
  to one;
- `cargo test --workspace --locked` is green, and the evidence log shows four direct-play sessions with
  `hwdec_current=d3d11va` and server-acknowledged progress and stops.

Five Medium findings should be fixed before Gate 6 is closed:
- **M1:** a recovery reload can restart at 0 and overwrite the resume point.
- **M2:** server calls block the single host task, so Stop and every control can hang for up to a minute.
- **M3:** audio/subtitle switching ignores non-direct plans.
- **M4:** the mini player cannot shrink below the main window's 960x600 minimum.
- **M5:** playback errors reach the user untranslated, and the new error codes show as "unknown".

## Phase 6 requirement coverage
| Requirement | Status | Evidence |
|---|---|---|
| Negotiation exactly per R1 | Met (L1, L2) | `caps.rs` declares `declared` evidence with the FFmpeg codec set; `controller.rs` `capabilities` L238-250, `negotiate` L273-301 (retry once on `installation_changed`), `start_body` with `allow_alternate_versions: false`. All 13 operations at the cited OAS lines. `connectEventsSocket` is used but missing from §13 (L2) |
| Direct-play everything mpv decodes; fall back only when required, show the reason | Met (M3) | Evidence: 4/4 sessions `delivery=original_http reason=direct_play`. Non-direct plans show `player.notDirect` with `decision_reason` (`PlayerView.tsx:445`). Track switching on non-direct plans is wrong (M3) |
| Overlay: play/pause, seek bar with chapters + trickplay, time/remaining, volume | Met | `PlayerView.tsx` seek bar (L165-280: chapter ticks, marker ranges, `getWatchTrickplay` tiles), volume, time |
| Audio/subtitle menus incl. external server subtitles, delay, size | Partly (M3, L10) | Menus exist; `sub-delay`/`sub-scale` (L314-315). External sidecars through the relay. Wrong on non-direct plans (M3); repeated external picks add duplicate tracks (L10) |
| Speed, aspect/zoom, fullscreen | Met | `validate_property` whitelist (`host.rs:40-68`), aspect cycle, `player_fullscreen` |
| Previous/next episode, skip intro/credits, autoplay-next with countdown | Met (L5, L10) | `PlayerView.tsx` skip buttons (L765-767), up-next card, `PlayerProvider.tsx` autoplay on `ended` |
| Stats overlay on `i` | Met | `stats` state + `player_stats`; evidence `03-stats.png`, `stats-*.json` |
| Progress: start, periodic, pause, stop per API; resume; watched threshold | Partly (M1, M2, L9) | Samples on start, pause/resume, seek, every 10 s playing and the paused heartbeat; stop once with the same `stop_id`. The watched threshold is applied by the server. Resume can be lost on recovery (M1) |
| Relevant WebSocket events | Met (L2, L10) | `events.rs` ticket + `silo.ticket.<t>` subprotocol, refreshes the library views |
| Recover from network drops and resume | Partly (M1, L1) | `schedule_recovery`/`recover` L645-699: reloads, then a new start. ADR-0008 says `failure_recovery` replan (L1) |
| mpv crash → restore UI, report error | Met (L4) | `host.rs` `BackendGone` L377-385 resets state, shows the overlay and emits a fatal notice. Untested (L4); message untranslated (M5) |
| Never leave the display in a switched HDR/refresh state | Not applicable to this diff | Phase 6 does not change display modes; Phase 7 work (`ff57e2a`, `01a6171`) is outside this scope |
| Keyboard: mpv-like defaults, README + in-app | Met (L6) | `lib/player.ts` KEYMAP L237-268 matches the README table and the help overlay. `keyFromVk` assumes a US layout (L6) |
| Hover-to-show, auto-hide 2–3 s with the overlay window actually hidden | Met (L5) | `video.rs` `set_overlay_visible` L247-257 (SW_HIDE of the WRY container). The evidence script checks CSS opacity only (L5) |
| Double-click fullscreen, wheel volume, right-click menu | Met | `PlayerView.tsx` L819-835 (WebView) and `onActivity` forwarding from the native window |
| Always-on-top mini player | Not met (M4) | `player_mini` sets 480x270 but the window minimum is 960x600; unchecked in `MANUAL_TEST_CHECKLIST.md` |
| SMTC | Met | `platform.rs` SMTC, media keys mapped in `PlayerView.tsx` |
| Display kept awake only while playing | Met | `PowerCreateRequest`/`PowerSetRequest(DisplayRequired)` toggled by `playing_changed` |
| No static elements; pause overlay dims | Met | `dimmed` state (`PlayerView.tsx:293`), opacity-40 while paused |

## Gate 6 coverage
| Requirement | Status | Evidence |
|---|---|---|
| Unit tests for the state machine and progress reporting, mock server + mocked mpv events | Met with gaps (L4) | `controller.rs` tests L792-1083 (direct play + progress, EOF, server-ended, reload/renegotiate, embedded subtitle), `host.rs` tests, `crates/player` state tests, silo-api playback/caps/media unit + contract tests. Missing: `installation_changed` retry, `seek_reanchor`, stop retry with the same `stop_id`, `409`, `BackendGone` |
| Real playback of the test clips with logged evidence | Met (L5) | `docs/evidence/phase-6/playback-log.txt`: sessions ps-5..ps-8 |
| Direct-play stream URL, no transcode parameters | Met | `direct_play=true relayed=true stream=/api/v2/stream/ps-N` (no query names) for all 4 sessions |
| `hwdec-current` active | Met (L5) | `hwdec_current=d3d11va` logged once per session (4/4) |
| Progress acknowledged by the server | Met, mock only (L5) | 9 progress acknowledgements and 4 `playback stopped on the server outcome=stopped` lines. All against the in-process `MockSiloApi`; no live server (no `.env.local`), which the gate permits |

### Checks run (this session)
- `cargo test --workspace --locked` → **exit 0**. Results: granary_lib 35, display 33, player 13, silo-api 41 unit,
  contract 13, tls 1 passed; live 1 ignored. All Phase 6 tests passed (controller ×6, host ×2, relay, events,
  caps ×4, media ×2, playback ×4, session playback round trip, contract playback tests). The run finished before the
  concurrent Phase 7 edits touched the tree; at that point HEAD was `ff57e2a`, whose Phase 6 code equals `037c1cc`.
- `grep -n operationId` in the upstream OAS for the 13 §13 operations → all line citations exact.
- `grep` of every capability feature/claim name in `.ref/silo-server/internal/playback/protocol_v3.go` → all exist.
  `track_change` is a valid replan operation (`protocol_v3.go:843`).
- i18n parity and key-usage script (scratchpad, node) → en 238 / de 238 keys, no missing keys, no placeholder
  mismatch; every literal `t("…")` key exists.
- `grep` for `unwrap()`/`expect(` outside `#[cfg(test)]` in the diff → none without a justification comment.
- `grep -n TODO\|FIXME` over the diff → none.
- `capabilities/main.json` vs `build.rs` vs `generate_handler!` → 15 new commands, 1:1.
- tao 0.37.2 source (cargo registry) read for M4: `WM_GETMINMAXINFO` handling and `set_inner_size_constraints`.
- `git show`/`git diff` reads of all files in scope; `docs/evidence/phase-6/*` read.
- Not run: the app, `scripts/verify.ps1`, vitest, the live test. Nothing that changes displays was run.

## Findings

### High
None.

### Medium

- **M1 A recovery reload can restart at position 0 and overwrite the server resume point.** `controller.rs:218` (`last_source_position: 0.0`), `controller.rs:393-395`, `recover` L669-699.
  - **Mechanism:**
    - `last_source_position` starts at 0.0 and is never set to the requested start position.
    - On every event, L393 copies `source_position()` into it as soon as the phase is `Playing`/`Paused`. After `load_requested()` the position is 0, and `FileLoaded` already sets the settled phase, so the field becomes 0 before mpv has reported the first `time-pos` after the seek to the start.
    - `recover` reloads at `last_source_position`, or renegotiates with `start_position` = that value.
  - **Failure scenarios:**
    - Resume at 45:00. The first load fails before playback (relay returns 503, the network drops during open). `recover` reloads at 0, and the first progress sample tells the server the position is 0:00. The resume point is lost.
    - Playing at 45:00, the stream drops; the reload hits `FileLoaded`, then fails again before the first `time-pos`. The next recovery restarts at 0.
  - **Fix:** set `last_source_position = start_seconds` in `begin`/`negotiate` (and to the target in `seek` and `recover`), and update it only from `time-pos` after `PlaybackRestart`, not on `FileLoaded`. Add a controller test: start at 600, fail the load, assert the reload is at 600.

- **M2 Server calls block the single host task, so Stop, pause and seek can hang for up to a minute.** `controller.rs` `stop` L711-723, `stop_server_session` L726-770, `send_progress` L593-643 (awaited inline from `on_event` and `tick`); `host.rs` `run` L259-277.
  - **Mechanism:**
    - `stop` awaits `stop_server_session()` (up to 3 attempts, 20 s request timeout each, 0.5 s + 1 s back-off) *before* `relay.clear()` and `backend.stop()`.
    - Progress samples are awaited inside event handling. All commands go through one task and one mpsc queue.
  - **Failure scenarios:**
    - Server unreachable through a black hole (Wi-Fi gone, VPN down). The user presses `q`/Back: mpv keeps playing and the overlay keeps showing for up to ~61.5 s. Pause, volume and seek queue behind it.
    - A slow server (20 s per progress request) freezes pause/seek and state updates for 20 s at every sample.
    - On app exit, `shutdown_blocking` gives up after 3 s, so the stop is often not delivered at all.
  - **Fix:** in `stop`, stop mpv and clear the relay first, emit `Stopped` with outcome pending, then deliver the stop body (fixed `stop_id`, sequence and position captured first) in a spawned task. Send progress samples from a spawned task too, with a single in-flight sample and a short timeout (for example 5 s).

- **M3 Audio and subtitle switching ignores non-direct plans.** `controller.rs` `select_audio` L544-547, `select_subtitle` L519-540.
  - **Mechanism:** `apply_tracks` (L468-473) correctly sets `aid` only when `plan.is_direct()`, but `select_audio` sets `aid = index + 1` unconditionally. The doc comment says "direct play carries every track of the original file", which is not true for remux/transcode HLS. `select_subtitle` matches embedded tracks by `ff-index`, which belongs to the source file, not the HLS rendition. Neither sends a replan.
  - **Failure scenario:** a transcode plan (HEVC 10-bit to a client without that decoder, or an incompatible audio codec) carries only the negotiated audio track. Picking the second audio track sets `aid=2`, which does not exist, so playback goes silent. Picking an embedded subtitle fails with "subtitle track not in this stream", or matches an unrelated stream.
  - **Upstream:** protocol v3 has `track_change` for exactly this (`protocol_v3.go:843`, `silo-api.md:504`, "a `replan` with `operation: "track_change"` and `selected_tracks`").
  - **Fix:** on non-direct plans, send `replan_playback` with `track_change` and `selected_tracks`, then load the new plan at the current position. Until then, disable the track menus when `direct_play` is false. Add a controller test with a transcode plan.

- **M4 The mini player cannot shrink below 960x600, and it is positioned for 480x270.** `commands.rs` `player_mini` L351-406; `tauri.conf.json` L21-22 (`minWidth: 960`, `minHeight: 600`).
  - **Mechanism:** `set_size(480·scale × 270·scale)` is clamped by tao's `WM_GETMINMAXINFO` handler to the minimum track size (tao 0.37.2; `SetWindowPos` re-applies the constraints, per tao's `set_inner_size_constraints` comment). The position is then computed from the *requested* size, not the actual one.
  - **Failure scenario:** with mini mode on, the window stays at 960x600 and its top-left sits at work-area right − 480 − 24 px, so about half the window is off-screen to the right and bottom. It is always on top, so it covers other windows. `MANUAL_TEST_CHECKLIST.md` leaves the mini player unchecked, so this was never seen.
  - **Also:** in `(false, true)`, `saved.take()` runs before the fallible calls, so a failed `set_size` loses the saved geometry for good.
  - **Fix:** call `set_min_size(None)` before shrinking and restore `Some(960x600)` when leaving mini mode; compute the position from `outer_size()` after resizing; take the saved geometry only after the restore succeeds.

- **M5 Playback errors reach the user untranslated, and the new error codes show as "unknown".** `lib/errors.ts` (`ERROR_CODES`), `controller.rs:117-122` (`code()`), `PlayerProvider.tsx:81-82`, `PlayerView.tsx:459-464` (`fail`) and L1301 (error card).
  - **Mechanism:**
    - The host returns `not_playable`, `playback_unsupported`, `player_unavailable` and `player_error`, none of which is in `ERROR_CODES` or in `errors.*` in en/de. `errorMessage` therefore shows `errors.unknown` for every `player_play` failure from `usePlay`, and the server's terminal reason (`NotPlayable("<reason>: <message>")`) is lost.
    - Notices and the in-player `fail` toasts show the Rust/English `message` directly, for example "The server was reconfigured; …" or a raw `ControllerError` string, also in the German UI.
  - **Failure scenario:** a German user opens a file the server refuses with a terminal decision. The library shows "Unbekannter Fehler" with no reason; during playback a dropped connection shows an English toast.
  - **Fix:** add the four codes to `ERROR_CODES` and to en/de `errors.*`; give `Notice::Error` stable codes and map them to i18n keys in `PlayerProvider`; pass the terminal reason as a parameter (`errors.not_playable` with `{{reason}}`); run errors through `errorMessage` in `fail`.

### Low

- **L1 ADR-0008 and code documentation disagree with the code.**
  - ADR-0008 says recovery uses a `failure_recovery` replan; `recover` (L669-699) stops the session and sends a new start instead.
  - ADR-0008 says capabilities are cached; `capabilities` (L238-250) fetches them on every `begin`.
  - The `controller.rs` module doc (L2) refers to `playback::runner`, which does not exist (the driver is `playback::host`).
  - `capabilities` checks `allowed`, the protocol and the installation id, but not the capability `state`.
  - **Fix:** amend ADR-0008 to describe what the code does (or implement `failure_recovery`), and fix the module doc.

- **L2 §13 omits `connectEventsSocket`.** `docs/research/silo-api.md` §13 states that the client uses "exactly these operations", but `events.rs` uses `connectEventsSocket` (OAS:L175913), which is neither in the table nor in `openapi-subset.json`. The endpoint itself exists upstream, so this is a documentation gap, not an invented endpoint. **Fix:** add the row and regenerate the subset.

- **L3 The `installation_changed` progress notice is misleading.** `controller.rs:634-640`. It says "progress will be saved at the end", but the stop is sent with the same, now stale, `installation_id`, so it will most likely be rejected the same way. **Fix:** refresh capabilities and renegotiate at the current position (as `negotiate` does), or drop the promise from the message.

- **L4 Gate 6 test gaps.** No test for: the `installation_changed` retry in `negotiate`, `seek_reanchor`, stop retry reusing the `stop_id`, `409 progress_conflict` not being retried, `BackendGone` restoring the UI, and host shutdown during an attempt. These are the paths most likely to regress. **Fix:** add mock-server cases for each; the controller test harness already supports scripted responses.

- **L5 Evidence weaknesses.** `scripts/playback-evidence.mjs`, `docs/evidence/phase-6/playback-log.txt` L21-28.
  - The "N starts the next episode e1 → e3" check passed for the wrong reason. N started e2 at `start=600.0` on a 60 s clip; e2 hit EOF at once (`position=Some("60.0")`, 0.2 s later), was reported as fully watched, and autoplay moved on to e3. The mock's e2 resume point is longer than the clip.
  - The auto-hide check measures CSS opacity, not the `SW_HIDE` of the overlay window that the requirement asks for.
  - The hwdec check uses `.some()`, so one session with hardware decoding passes even if others fall back to software. The log does show d3d11va for all four.
  - All runs use the in-process mock (allowed by the gate, but worth stating in PROGRESS.md).
  - **Fix:** clamp the mock resume point below the duration and assert that e2 actually played; check `IsWindowVisible` of the WRY container; use `.every()`.

- **L6 Key forwarding from the native window assumes a US keyboard layout.** `lib/player.ts` `keyFromVk` L283-320. On a German layout the `#` key is `VK_OEM_2` (0xBF) and maps to `/` (volume down, KEYMAP), Shift+3 (`§`) maps to `#` (audio cycle), and `?` (Shift+ß) cannot be produced. This only affects keys pressed while the native video window has focus. **Fix:** send the translated character from the Win32 side (`ToUnicode`/`WM_CHAR`) instead of the virtual-key code for punctuation.

- **L7 Fullscreen, mini mode and always-on-top are not undone when playback ends.** `PlayerView.tsx:1290-1293` only restores the overlay. After Stop or EOF in mini mode, the library UI stays in an always-on-top window of the mini geometry (once M4 is fixed). **Fix:** call `player_mini(false)` and `player_fullscreen(false)` when `state.active` becomes false.

- **L8 The relay accept loop busy-spins on persistent errors.** `relay.rs:79-81`. `listener.accept()` errors (for example `EMFILE`/WSAEMFILE under handle exhaustion) `continue` immediately, burning a core. **Fix:** log once and sleep briefly (for example 50 ms) before retrying.

- **L9 Progress state is not fully reset across re-plans.** `negotiate` (L273-301) resets the sequence and stop id but not `started_reported`/`last_progress`; after a `seek_reanchor` reload (`seek` L550-569) no immediate sample is sent; old relay routes are not cleared on reanchor, so routes accumulate for the session. **Fix:** reset both fields in `negotiate`/`load_plan`, send a sample after the reanchored `PlaybackRestart`, and replace the route on every `load_plan`.

- **L10 Smaller robustness nits.**
  - Choosing the same external subtitle again runs `sub-add … select` again and adds a duplicate track (`select_subtitle` L529-537).
  - `events.rs` resets its failure counter on every successful connect, so a server that accepts and closes at once causes a reconnect every 250 ms.
  - If the up-next card has already started the next episode when `Ended` arrives, `PlayerProvider.tsx:84-90` can call `start` a second time.
  - **Fix:** remember added sidecars and reselect by id; reset the back-off only after the socket stayed up for a while; ignore `Ended` when a start is in flight.
