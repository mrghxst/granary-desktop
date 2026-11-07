# PROGRESS
Last updated: 2026-11-07T20:30:00+01:00 | Current phase: 8 | Last commit: (see git log)

## Phases
- [x] 0 Preflight & private repo — DONE (commit a0e7d04) — gate: gh repo view visibility=PRIVATE, skeleton pushed, PROGRESS.md + CLAUDE.md present
- [x] 1 Research & ADRs — DONE — gate: 5 research docs with Sources, ADR-0001..0006, reviewer docs/reviews/phase-1.md PASS WITH FINDINGS, all 6 medium + 15 low resolved
- [x] 2 Risk spike: mpv + overlay + HDR — DONE (commit 9a1a87e) — gate: self-test 16/16 (SDR, video-sync=audio), HDR round-trip exit 0, restore-safety scenarios pass, reviewer docs/reviews/phase-2.md PASS WITH FINDINGS, 2 high + 5 medium + 8 low resolved
- [x] 3 Scaffold, CI, project skills — DONE (commit 508e26c) — gate: verify.ps1 exit 0 (11 steps), tauri dev launches (libmpv api 2.6, single-instance, owner-checked restore), CI green, reviewer docs/reviews/phase-3.md FAIL → 4 high + 6 medium + 8 low resolved
- [x] 4 Silo connectivity & auth — DONE (commit 6573af4 + review fixes) — gate: verify.ps1 exit 0, CI green, contract tests vs OAS subset @6791aac, mocked add → login → PIN → libraries flow (live test blocked: no .env.local), reviewer docs/reviews/phase-4.md FAIL → 1 high + 2 medium + 2 low resolved
- [x] 5 UI (web-app parity) — DONE (commit bd2369b + review fixes) — gate: verify.ps1 exit 0, CI green, vitest flow + host E2E test, ui-evidence.mjs 13/13 checks with 14 screenshots (docs/evidence/phase-5), reviewer docs/reviews/phase-5.md PASS WITH FINDINGS → 5 medium + 3 low resolved
- [x] 6 Native playback & progress sync — DONE — gate: verify.ps1 exit 0 (11 steps), playback evidence 17/17 (docs/evidence/phase-6, mock server + ffmpeg clips, d3d11va), progress/stop/recovery controller tests, reviewer docs/reviews/phase-6.md PASS WITH FINDINGS → 5 medium + 6 low resolved (L1–L3, L7–L9), L4–L6/L10 deferred below
- [x] 7 Display & audio (auto-HDR, refresh match, passthrough) — DONE — gate: verify.ps1 exit 0, display crate 36 + app 56 tests (mode selection, restore, leftover marker, refused HDR, live retune), display_roundtrip exit 0 (mains: HDR + refresh; battery: HDR refusal + refresh), output-evidence 17/17 (mains and battery runs), MANUAL_TEST_CHECKLIST A.1–A.15 / B.1–B.6, reviewer docs/reviews/phase-7.md PASS WITH FINDINGS → 7 medium + L1/L3/L6/L7 resolved, L2/L4/L5/L8 deferred below
- [ ] 8 Polish
- [ ] 9 Packaging & release workflow
- [ ] 10 Final verification & handoff

## Current step
Phase 8: polish. Done: About screen + update link (ADR-0005), log viewer/export, onboarding, offline banner,
jsx-a11y lint, Profiles back button / aria-pressed / Remove confirmation, performance in `docs/perf.md` (cold start
778 ms, idle 455 MiB/7 processes, TTFF 1.46 s SDR / 1.61 s HDR10 4K after the persistent shader cache). Next: Gate 8
review.

## Decisions (link ADRs)
- [ADR-0001](docs/adr/0001-ui-strategy.md) UI: own React UI with Silo design tokens (option C).
- [ADR-0002](docs/adr/0002-player-integration.md) Player: in-process libmpv (libmpv2-sys 4.0.1) via `wid`.
- [ADR-0003](docs/adr/0003-window-overlay-architecture.md) Window: single Tauri window, video child HWND under WebView2; fallback owned overlay.
- [ADR-0004](docs/adr/0004-http-auth-token-storage.md) Rust-side HTTP, tokens in Credential Manager, origin-scoped credentials.
- [ADR-0005](docs/adr/0005-packaging-licensing-updates.md) shinchiro v3 libmpv pinned + private mirror, NSIS per-user, no auto-update.
- [ADR-0006](docs/adr/0006-display-refresh-hdr-restore-policy.md) Refresh/HDR policy and restore marker.
- [ADR-0007](docs/adr/0007-workspace-toolchain-ci.md) Workspaces, frontend toolchain (TS 6.0.3 for typescript-eslint), Rust 1.99.1 pin, CI/release workflows.
- [ADR-0008](docs/adr/0008-native-playback-pipeline.md) Playback: loopback media relay (no credentials in mpv), one controller per attempt, single-task player host, overlay hiding, up-next anchoring, events socket.
- pnpm 12.18.0 via corepack shim installed to `%APPDATA%\npm` (corepack enable into Program Files needs admin).
- Toolchain (Phase 0): git 2.53.0, node v24.18.0, pnpm 12.18.0, rustc 1.99.1 (host x86_64-pc-windows-msvc),
  VS Build Tools 2022 17.14 (MSVC 14.44.35207, Windows SDK 10.0.26100), gh 2.103.3, 7-Zip 26.01, ffmpeg 8.1.2,
  WebView2 157.0.4341.42. No gitleaks/PresentMon/pwsh 7/python (Store stub only) on the host.
- gh CLI is not logged in; gh calls use the existing Git Credential Manager GitHub credential via a per-command
  `GH_TOKEN` (never printed or stored). Commits use repo-local identity `mrghxst <mrghxst@users.noreply.github.com>`.
- No `claude` CLI on PATH (desktop app), so code-intelligence plugins could not be installed; built-in skills
  (code-review, security-review, simplify, run) are used instead.

## Blocked / needs human
- No `.env.local` present: no live Silo server credentials, no TEST_MEDIA_DIR, no DISPLAY_NOTES. Live tests will be
  skipped/mocked; synthetic ffmpeg fixtures used instead. Gate 4 live login/library listing is therefore recorded as
  blocked, with mocked coverage (contract tests against the OAS subset, wiremock HTTP tests, MockSiloApi session and
  UI flow tests). To run it later: create `.env.local` (SILO_URL, SILO_USERNAME, SILO_PASSWORD), then
  `cargo test -p granary-silo-api --test live -- --ignored`.
- Gate 6 "real library item" playback is not applicable without `.env.local` (prompt: "if credentials exist"); real
  playback evidence uses the ffmpeg test clips served by the in-process mock (direct play, `hwdec-current=d3d11va`,
  progress acknowledged). Live checks are listed in MANUAL_TEST_CHECKLIST.md (Native playback).
- Test machine is a laptop (Intel Arc 140V, one HDR-capable internal panel 60/120 Hz): multi-monitor, VRR, OLED
  desktop monitor and AV-receiver checks go to MANUAL_TEST_CHECKLIST.md. PresentMon not installed (independent flip
  cannot be measured automatically).
- Distribution to third parties requires a complete libmpv dependency source set (ADR-0005 known gap).

## Known issues
- One `cargo test --workspace` run inside verify.ps1 exited 101 on 2026-11-07 (right after `cargo fmt`); it was not
  reproduced in 7 later runs (5 × cargo test, 2 × verify.ps1). Watch for a flaky test; CI runs the same suite.
- Phase 7 accepted risk (review L7): `WM_ENDSESSION` (log-off/shutdown during playback) is not handled; the marker
  repairs HDR/refresh at the next start, so a switched monitor can stay in HDR until Granary runs again.
- Phase 7 review lows deferred: L2 reuse key is the content, not the decision (HDR10 → HDR10+ episode re-switches);
  L4 fractional-rate switching unverified on hardware (no 59.94 mode on this panel), applied path not logged, marker has
  no resolution (ADR restore wording); L5 Settings shows the SDR-state DXGI peak while HDR is off, out-of-range numbers
  dropped silently; L8 small items (exclusive-mode failure notice, `from_mpv` fallback, test-only `holding()`).
- Phase 6 (from phase-2 review L2/L4): production video module drives all Win32 window calls from the UI thread and
  handles `WM_SYSKEYDOWN`/`WM_SETCURSOR`; idle-cursor auto-hide (H1 rule) and key-wake re-verified in an unlocked session.
- Phase 7: independent flip with the overlay hidden (ADR-0003 fallback trigger), overlay-hidden pacing, packets 15/16 and
  rational `SetDisplayConfig` refresh switching, explicit `hdr-reference-white` from packet 11 (ADR-0006 M3/M5).
- Unattended GUI tests can hit the Windows lock screen; input checks then skip by design (seen in 3 runs).
- TypeScript 7 migration deferred until typescript-eslint supports it (ADR-0007).
- pnpm minimumReleaseAgeExclude: i18next@26.7.2 (current at time of use; ADR-0007).
- Phase 4 review lows deferred: L1 double resume after restart (single-flight per Session only), L4 specific login
  403 codes, L5 show Retry-After, L6 show redirect target origin, L7 "enter http://" hint on TLS failure without
  scheme, L8 unused getCurrentUser / servers_activate surface, L9 Profiles back button / aria-pressed / live region /
  Remove confirmation (Phase 5 shell + Phase 8 polish), L10 Rust-side auth://revoked emit test, L11 request_origin
  mismatch as warning tone + ADR note.
- Phase 5 review lows deferred: L1 ItemGrid key omits serverId / unkeyed load-more state, L2 sub-loader errors skip
  needsReauth, L4 video_with_episodes without getCatalogSearchCapabilities, L6 unserialised settings saves / seekStep
  options / preferSdh unused / settings read errors treated as missing, L7 Profiles back button (also phase-4 L9),
  L8 IPC/capability-level test (tauri::test get_ipc_response), L11 ContextMenu refocus on re-render, L12 artwork host
  allow-list — Phase 8 polish; L3 player_play validation → Phase 6 controller.
- Design A with window `backgroundColor #141417` (spike used `#00000000`; phase-3 review L2): playback works with
  it in the Phase 6 evidence; the pre-first-frame look on a slow stream is a manual check (MANUAL_TEST_CHECKLIST.md).
- Events socket against the in-process mock is unreachable by design (backoff warnings in the dev log).
- Next episode does not cross a season boundary (neighbours come from `listSeasonEpisodes` of the current season;
  no next-up endpoint upstream, silo-api.md open question 9) — Phase 8 polish.
- Phase 6 review lows deferred: L4 more controller tests (installation_changed retry, seek_reanchor, stop retry
  reusing stop_id, 409 not retried, BackendGone, shutdown mid-attempt); L5 evidence tightening (mock resume point
  beyond clip length made "N → e3" pass via instant EOF, overlay auto-hide measured by CSS opacity, hwdec `.some()`);
  L6 punctuation keys from the native video window assume a US layout (send translated chars via ToUnicode);
  L10 duplicate sidecar on re-selecting the same external subtitle, events back-off reset on every connect,
  `Ended` while an up-next start is in flight — Phase 8 polish.
- Track change on remux/transcode plans (`track_change` replan) is covered by controller tests against the mock only;
  the mock answers every plan as direct play, so a live transcode check is in MANUAL_TEST_CHECKLIST.md.

## Log (newest first, one line per completed step)
- 2026-11-07 Phase 8: docs/perf.md, `scripts/measure-ttff.mjs`, persistent libplacebo shader cache (TTFF −12 %/−22 %), release-build TTFF checklist item; verify.ps1 exit 0.
- 2026-11-07 Gate 7 PASSED: review M1 (delivered range from `effective_recipe`, HDR declared when switchable), M2 (exit restores before the server stop), M3 (leftover marker entries kept), M4 (`panic = "abort"`), M5 (policy in reuse key, live retune), M6 (5 s HDR wait, refused switch remembered per power state, monitor move off the host task), M7 (checklist HDR10+/near-black/windowed+fullscreen); L1, L3, L6, L7; verify.ps1 exit 0, round trip + evidence 17/17 on battery.
- 2026-11-07 Phase 8 (part): About, log viewer, onboarding, offline banner, a11y lint, perf measurement (median ready 778 ms, idle 455 MiB working set over 7 processes).
- 2026-11-07 Gate 6 PASSED: review M1 (resume position from the requested start until playback), M2 (progress from a spawned task, one in flight; Stop unloads mpv first, server stop bounded to 4 s then background), M3 (`track_change` replan on non-direct plans), M4 (mini player lifts min size, positions by actual size, keeps geometry until restored), M5 (player error codes + notices translated en/de); L1 ADR-0008 amendment, L2 connectEventsSocket in §13 + subset (30 ops), L3, L7, L8, L9; verify.ps1 exit 0.
- 2026-11-07 Phase 7: output policy applied before the first frame (HDR/refresh/tone mapping/audio), idle grace + restore, settings UI, mock clip facts, evidence 17/17 + round trip exit 0.
- 2026-11-07 Phase 6: player overlay (keyboard map, menus, stats, trickplay, up next, media keys), events socket live refresh, precise seeks + unpause on reused core, per-session hwdec log, ADR-0008, playback evidence 17/17 (`scripts/playback-evidence.mjs`); verify.ps1 exit 0.
- 2026-11-07 Gate 5: review fixes M1–M5 (web-app subtitle/resume/version rules, raster-only sandboxed artwork scheme + navigation guard, streamed 16 MiB cap, UI evidence via WebView2 DevTools), L5/L9/L10; verify.ps1 exit 0.
- 2026-11-06 Phase 5: UI web-app parity screens, catalog commands + mock catalog, granary-img scheme, settings store, contract tests for 6 new operations, vitest browse → details → play flow + host-side E2E test; verify.ps1 exit 0.
- 2026-11-06 Gate 4: review fixes (H1 credential never follows a server id to a new origin, M1 account-level calls without profile headers, M2 re-login ends previous session, L2, L3); verify.ps1 exit 0.
- 2026-11-06 Phase 4: silo-api crate (9 OAS ops, contract tests vs OAS subset, TLS + wiremock tests, live test `#[ignore]`), Tauri commands, connect UI + vitest flow; verify.ps1 exit 0.
- 2026-11-06 Phase 3 DONE: scaffold, verify.ps1, CI (libmpv cache job + verify job), release workflow (drafts only), skills, ADR-0007; review H1–H4/M1–M6/L1–L8 resolved.
- 2026-11-06 Phase 2 DONE: spike (Design A works), review H1/H2/M1–M5/L1–L8 resolved, evidence in docs/spikes/evidence/.
- 2026-11-06 Phase 1 DONE: R1–R5 research (subagents), ADR-0001..0006, reviewer pass + fixes, libmpv pinned/fetched (sha256 verified), fixtures generated.
- 2026-11-06 Phase 0 DONE: tools installed (winget rustup/gh/VS Build Tools), repo PRIVATE, skeleton pushed (a0e7d04), obsolete branch claude/blissful-planck-27cqsw deleted at owner's request.
- 2026-11-06 Phase 0: skeleton files created (.gitignore, .gitattributes, LICENSE from gnu.org, .env.example, README, CLAUDE.md, PROGRESS.md, docs/).
