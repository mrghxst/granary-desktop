# PROGRESS
Last updated: 2026-11-08 | Current phase: done | Last commit: 6a14e97 before the Gate 10 commit (each Log line names its commit)

## Phases
- [x] 0 Preflight & private repo — DONE (commit a0e7d04) — gate: gh repo view visibility=PRIVATE, skeleton pushed, PROGRESS.md + CLAUDE.md present
- [x] 1 Research & ADRs — DONE — gate: 5 research docs with Sources, ADR-0001..0006, reviewer docs/reviews/phase-1.md PASS WITH FINDINGS, all 6 medium + 15 low resolved
- [x] 2 Risk spike: mpv + overlay + HDR — DONE (commit 9a1a87e) — gate: self-test 16/16 (SDR, video-sync=audio), HDR round-trip exit 0, restore-safety scenarios pass, reviewer docs/reviews/phase-2.md PASS WITH FINDINGS, 2 high + 5 medium + 8 low resolved
- [x] 3 Scaffold, CI, project skills — DONE (commit 508e26c) — gate: verify.ps1 exit 0 (11 steps), tauri dev launches (libmpv api 2.6, single-instance, owner-checked restore), CI green, reviewer docs/reviews/phase-3.md FAIL → 4 high + 6 medium + 8 low resolved
- [x] 4 Silo connectivity & auth — DONE (commit 6573af4 + review fixes) — gate: verify.ps1 exit 0, CI green, contract tests vs OAS subset @6791aac, mocked add → login → PIN → libraries flow (live test blocked: no .env.local), reviewer docs/reviews/phase-4.md FAIL → 1 high + 2 medium + 2 low resolved
- [x] 5 UI (web-app parity) — DONE (commit bd2369b + review fixes) — gate: verify.ps1 exit 0, CI green, vitest flow + host E2E test, ui-evidence.mjs 13/13 checks with 14 screenshots (docs/evidence/phase-5), reviewer docs/reviews/phase-5.md PASS WITH FINDINGS → 5 medium + 3 low resolved
- [x] 6 Native playback & progress sync — DONE — gate: verify.ps1 exit 0 (11 steps), playback evidence 17/17 (docs/evidence/phase-6, mock server + ffmpeg clips, d3d11va), progress/stop/recovery controller tests, reviewer docs/reviews/phase-6.md PASS WITH FINDINGS → 5 medium + 6 low resolved (L1–L3, L7–L9), L4–L6/L10 deferred below
- [x] 7 Display & audio (auto-HDR, refresh match, passthrough) — DONE — gate: verify.ps1 exit 0, display crate 36 + app 56 tests (mode selection, restore, leftover marker, refused HDR, live retune), display_roundtrip exit 0 (mains: HDR + refresh; battery: HDR refusal + refresh), output-evidence 17/17 (mains and battery runs), MANUAL_TEST_CHECKLIST A.1–A.15 / B.1–B.6, reviewer docs/reviews/phase-7.md PASS WITH FINDINGS → 7 medium + L1/L3/L6/L7 resolved, L2/L4/L5/L8 deferred below
- [x] 8 Polish — DONE — gate: verify.ps1 exit 0 (11 steps, vitest 47), docs/perf.md (cold start 778 ms median, idle 455 MiB / 7 processes, TTFF 1.46 s SDR / 1.61 s HDR10 4K), native-keys evidence 3/3 (German layout), reviewer docs/reviews/phase-8.md PASS WITH FINDINGS → 2 medium + L1/L5/L6 resolved, other lows re-targeted or accepted below
- [x] 9 Packaging & release workflow — DONE — gate: local installer (release-build.ps1) and CI installer (release run 42428189010 on dd9b7a8, draft release v0.1.0-dev, isDraft=true, SHA-256 a78a23e5…091d verified), install-test.ps1 PASSED on the CI asset (docs/evidence/phase-9/install-test-ci.txt), CI green, reviewer docs/reviews/phase-9.md FAIL → H1 + M1 + M2 + L1–L5 resolved
- [x] 10 Final verification & handoff — DONE — gate: clean clone of dd9b7a8 (pnpm install --frozen-lockfile, fetch-libmpv, verify.ps1 12 steps, release build) all exit 0 (docs/evidence/phase-10/clean-clone.txt), reviewer docs/reviews/final.md PASS WITH FINDINGS → M1 + L1–L5 resolved, security review docs/security.md (cargo audit 0 vulnerabilities, pnpm audit clean), docs complete, CI green, FINAL REPORT printed

## Current step
0.2 web-app parity round (user list of 8 items): detail page + player, library tabs/infinite scroll/A–Z rail/monochrome icon/poster tags, seek previews done; next for files without trickplay, README + Pages site.
All phases DONE (0.1.0); 0.1.1 fixes the hidden video under the overlay; 0.2.0 adds web-app parity features, local seek previews and the website. Remaining human steps: see "Blocked / needs human" and MANUAL_TEST_CHECKLIST.md.

## Decisions (link ADRs)
- [ADR-0001](docs/adr/0001-ui-strategy.md) UI: own React UI with Silo design tokens (option C).
- [ADR-0002](docs/adr/0002-player-integration.md) Player: in-process libmpv (libmpv2-sys 4.0.1) via `wid`.
- [ADR-0003](docs/adr/0003-window-overlay-architecture.md) Window: single Tauri window, video child HWND under WebView2; fallback owned overlay.
- [ADR-0004](docs/adr/0004-http-auth-token-storage.md) Rust-side HTTP, tokens in Credential Manager, origin-scoped credentials.
- [ADR-0005](docs/adr/0005-packaging-licensing-updates.md) shinchiro v3 libmpv pinned + private mirror, NSIS per-user, no auto-update.
- [ADR-0006](docs/adr/0006-display-refresh-hdr-restore-policy.md) Refresh/HDR policy and restore marker.
- [ADR-0007](docs/adr/0007-workspace-toolchain-ci.md) Workspaces, frontend toolchain (TS 6.0.3 for typescript-eslint), Rust 1.99.1 pin, CI/release workflows.
- [ADR-0008](docs/adr/0008-native-playback-pipeline.md) Playback: loopback media relay (no credentials in mpv), one controller per attempt, single-task player host, overlay hiding, up-next anchoring, events socket.
- [ADR-0009](docs/adr/0009-seek-previews.md) Seek previews without server trickplay: second headless libmpv handle on the relay URL (direct play only), keyframe seek + screenshot, lavfi tone-mapping for HDR; AIOStreams' approach researched (container index + libavcodec), not used because our FFmpeg is static.
- [ADR-0010](docs/adr/0010-artwork-cache.md) Bounded on-disk artwork cache (300 MiB, LRU, Continue Watching art kept, weekly revalidation, offline fallback).
- [ADR-0011](docs/adr/0011-seekr-previews.md) Seek-preview source chain server → Seekr → local (reorderable, each switchable); Seekr key in Credential Manager, sheets proxied by the host.
- [ADR-0012](docs/adr/0012-rendering-quality-and-gpu-video-processing.md) Per-title rendering options: quality levels (auto = balanced on UMA GPUs, high = mpv as configured, max = high-quality profile), opt-in d3d11vpp RTX/Intel VSR and RTX HDR for SDR, Dolby Vision switch (P7/P8 base layer).
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
- Tag `v0.1.0` (and its draft release) points to 1257c5f, from before the Gate 9 review fixes. Moving a pushed tag was
  refused by the session's safety policy, so the final build is the draft release `v0.1.0-dev`, built from main by the
  release workflow (workflow_dispatch). Before publishing 0.1.0: delete the old draft, then move the tag to the
  final commit:
  `git tag -f -a v0.1.0 -m "Granary 0.1.0" <sha>`, then `git push -f origin v0.1.0`. The release workflow then
  rebuilds the draft.

## Known issues
- Seek previews (ADR-0009) of Dolby Vision profile 5 files have wrong colours (libavfilter does not reshape DV); previews only, playback is unaffected.
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
- Phase 5 review lows: L7 done in Phase 8, L11 (ContextMenu refocus) fixed at Gate 8, L3 done in Phase 6.
  L8 IPC/capability-level test (`ipc-contract.test.ts`) and L12 artwork hosts limited to the server origin or public
  https hosts: fixed in the Phase 10 security review (docs/security.md).
  Accepted for 0.1 (Gate 8 review M2): L1 ItemGrid key omits serverId (a server switch remounts the route anyway);
  L2 sub-loader errors skip needsReauth (the session refreshes tokens itself; a revoked session is caught by the item
  load and the next navigation); L4 video_with_episodes without getCatalogSearchCapabilities (the server ignores an
  unsupported filter); L6 unserialised settings saves / seekStep options / preferSdh unused / unreadable settings read
  as defaults (single window, saves are tiny and last-write-wins is the intended result).
- Design A with window `backgroundColor #141417` (spike used `#00000000`; phase-3 review L2): playback works with
  it in the Phase 6 evidence; the pre-first-frame look on a slow stream is a manual check (MANUAL_TEST_CHECKLIST.md).
- Events socket against the in-process mock is unreachable by design (backoff warnings in the dev log).
- Next/previous episode across a season boundary: fixed at Gate 8 (`episodesAround`, listSeasons +
  listSeasonEpisodes; specials never cross; still no next-up endpoint upstream, silo-api.md open question 9).
- Phase 6 review lows: L6 (US layout for keys from the video window) fixed at Gate 8 (ToUnicode layout character).
  Accepted for 0.1 (Gate 8 review M2): L4 more controller tests (installation_changed retry, seek_reanchor, stop
  retry reusing stop_id, 409 not retried, BackendGone, shutdown mid-attempt; the paths exist and are exercised by the
  evidence runs); L5 evidence tightening (instant-EOF resume in the mock, auto-hide by CSS opacity, hwdec `.some()`);
  L10 duplicate sidecar on re-selecting the same external subtitle, events back-off reset on every connect,
  `Ended` while an up-next start is in flight (rare, harmless: mpv ignores the duplicate, the next start wins).
- Phase 8 review lows: L1, L5, L6 fixed at Gate 8; L4 relay-route token redaction and L9 About source link with the
  build commit fixed in Phases 9/10. Accepted for 0.1: L2 About shows CARGO_PKG_VERSION
  (verify.ps1 fails when it differs from the bundle version); L3 standalone /about and /settings have no back link
  (reached from the start screen only; a back link is a 0.2 item); L7 loading texts without role="status"; L8 log export to %USERPROFILE%\Downloads
  rather than the known folder; L10 onboarding shown when the server list fails to load.
- Track change on remux/transcode plans (`track_change` replan) is covered by controller tests against the mock only;
  the mock answers every plan as direct play, so a live transcode check is in MANUAL_TEST_CHECKLIST.md.

## Log (newest first, one line per completed step)
- 2026-11-09 0.4.3: preview scheduler rework (newest request wins, 3 fetchers + 1 renderer, frames pushed via player://preview, one adaptive range read per keyframe, undimmed stand-ins; ADR-0009 rev 5); external subtitles prefetched and added from memory (sub-add hex://); demuxer cache from installed RAM; rendering quality, RTX/Intel VSR, RTX HDR, Dolby Vision switch (ADR-0012); stale preview command permissions fixed. Unit, libmpv integration and GPU filter tests green on Intel Arc 140V; previews not checked against the real server; RTX paths not run (no NVIDIA GPU here).
- 2026-11-08 0.4.2: version bump, tag v0.4.2; release published (latest) at the owner's request.
- 2026-11-08 0.4.2 fixes (0fe187c + this commit): embedded subtitle switch seeks exactly in place (mpv refresh seek dropped the read-ahead and waited for cache-pause-wait); audio switch keeps the open AO via temporary gapless-audio=yes when the track loses nothing (WASAPI mix format); local previews in software from memory (mpv_stream_cb_add_ro, screenshot-raw, jpeg-encoder) 0.12 s vs 0.86 s per 4K HDR frame, parallel keyframe fetch, 3 requests in flight (ADR-0009 rev 4); FFmpeg reconnect_on_http_error 408/429/5xx; docs/research/reference-clients.md (AIOStreams, Nuvio, Stremio). Unit and libmpv integration tests green; not checked against the real server.
- 2026-11-08 0.4.1: version bump, tag v0.4.1; release published (latest) at the owner's request.
- 2026-11-08 0.4.1 fixes: card context menu portalled to <body> (root cause of the broken page: fixed menu inside a transformed, overflow-clipped card), menu actions Play / from beginning / watched / unwatched / remove from Continue Watching or Next Up (dismissHomeItem); bounded artwork cache (ADR-0010); Seekr preview provider and reorderable source chain (ADR-0011). Unit tests green; Seekr not exercised against the live API (no key in this environment).
- 2026-11-08 Post-0.4 fixes: subtitle index mapping catalog→protocol ordinal (externals first; root cause of wrong-language start subtitles), forced_only mode, unmatched embedded subtitle → sid=no; audio-switch sync watchdog (resync when audio-pts/avsync drift >0.35 s after the switch, 2 retries); spinner kept over hidden controls; overlay forced visible during launch (next-episode see-through); seek previews revision 3 (cache-only nearest frame, exact with priority, paced coarse-to-fine background grid, ADR-0009); fixed-size clamped preview tile; search type filters. Unit and integration tests green; not checked against the real server.
- 2026-11-08 0.4.0 public beta: version bump, site screenshots + pointer-tilt hero, tag v0.4.0; release published at the owner's request, old drafts/pre-releases removed (libmpv mirror draft kept, the release workflow downloads from it).
- 2026-11-08 0.4 fixes: audio switch pauses, switches, exact-seeks and resumes on PlaybackRestart (no display-resample speed-up); external subtitles deduped; bigger demuxer cache + cache-pause-wait; seek previews from the Matroska keyframe index (granary_player::mkv + range, single-frame files decoded locally, cache per keyframe, ADR-0009 revision 2, integration test asserts index mode); detail hero redesign (no side poster, blurred poster fallback, fixed logo slot, aligned credits); searchable long filter facets; Back moved into the Shell for item/person routes, scroll restore via ResizeObserver; mock-mode dev builds skip single-instance and display repair. Verified in the running app against the mock server.
- 2026-11-08 0.3.0: per-series audio/subtitle memory (lib/trackMemory.ts, matched by description), epilogue-aware credits (isFinalCredits, Skip credits + Next episode, Next button after a cancelled countdown), icon play mark moved to the kernel centroid; version bump, tag v0.3.0 for the draft release.
- 2026-11-08 UI and player fix round (14 owner reports): poster back on detail pages, menus above the cast row, Back button + scroll restore, drag-scroll rows, themed scrollbars + docked A–Z rail, web-app card hover, 9 more library filters via catalog `groups` rules (allowlisted host-side), player menus restyled and auto-close, per-context fullscreen memory, launch screen with poster until frames advance, audio switch resync (exact seek), seek previews at 30 s+ steps with a settle delay (ADR-0009 revised), new icon. verify.ps1 green before commit.
- 2026-11-08 Repo made public at the owner's request (full-history secret scan clean, commit emails noreply only); GitHub Pages enabled (workflow source), `pages` run build+deploy green, https://mrghxst.github.io/granary-desktop/ checked live (landing, docs, search index, images). Draft releases stay unpublished.
- 2026-11-08 0.2.0: version bump (package.json, Cargo.toml, CHANGELOG), verify.ps1 12/12 green, tag v0.2.0 pushed so the release workflow builds the draft release.
- 2026-11-08 0.2 step 4: website + docs in site/ (Next 16 static export, Fumadocs 16, own pnpm workspace; landing with scroll animations, 14 docs pages, static search), `pages` workflow, README rewrite. Site typecheck + build OK, checked in the browser (landing, docs, search index, basePath assets). Pages not enabled (private repo, see Blocked).
- 2026-11-08 0.2 step 3: seek-bar previews for direct-played files without trickplay (ADR-0009): granary_player::thumbs (vo=null handle, keyframe seek, screenshot-to-file, zscale+tonemap for PQ/HLG), player_preview command with 2 s bucket cache, overlay hook with one request in flight, setting player.seekPreviews (default on). Integration test against the pinned DLL (SDR+HDR JPEGs); in the running app: open 242 ms, new frame ~50 ms, cached 4 ms.
- 2026-11-08 0.2 step 2: library tabs (Recommended rows, Library grid, Collections), infinite scroll (IntersectionObserver, button fallback), A–Z rail (namePrefix), genre/decade filters, sort popover, shuffle; poster quality tags setting (poster_tags, default off); monochrome icon regenerated with tauri icon. Verified in the running app against the mock server; verify.ps1 12/12.
- 2026-11-08 0.2 step 1: 16 new OAS operations (flags, rating, person, library sections/collections, similar, filters) in silo-api with contract tests (14/14), docs/research/silo-api.md §13 + endpoint table; detail page redesign, person route, language-first track pickers (detail + player), launch screen until first frame, open_trailer command (YouTube key only). Verified in the running app against the mock server (screenshots not committed).
- 2026-11-08 Fix 0.1.1 (user report on 0.1.0): video hidden while the overlay is shown. Cause: `WS_CLIPSIBLINGS` on the video child clipped it under the full-client WRY container. Removed it; on-screen pixel check (scripts/overlay-pixels.ps1) added to playback-evidence (shown 0 → 5 colour families, hidden 5); ADR-0003 Amendment 1; evidence docs/evidence/overlay-fix (19/19). Draft release v0.1.1 from 3436cac, CI asset SHA-256 6b45f2f2…5f5b verified; install-test not run because the user has 0.1.0 installed (manual upgrade).
- 2026-11-08 Gate 10 PASSED: clean-clone build of dd9b7a8 green, final review resolved, every phase DONE, FINAL REPORT printed.
- 2026-11-08 Gate 9 PASSED: CI installer from dd9b7a8 (draft v0.1.0-dev) SHA-256 verified and install-tested (install-location key written, then removed; the install-test registry path is fixed), CI green, phase-9 review resolved.
- 2026-11-08 Final review (docs/reviews/final.md, PASS WITH FINDINGS): M1 release.yml split into libmpv / installer (read-only, no persisted credentials) / draft jobs; L1 draft target and notes follow re-runs; L2 display repair before anything that can fail at startup; L3 doc corrections; L4 this header and log; L5 README states what is untested. Clean clone found a parallel-test race in crates/player/tests/libmpv_options.rs (DLL copy once per process).
- 2026-11-08 Phase 10 (1c10c83, d1336dd, 950ca33): security review in docs/security.md (CSP, capabilities, IPC contract test, artwork host check, relay token and command-error redaction, cargo audit 0 vulnerabilities, pnpm audit clean), ARCHITECTURE.md, CONTRIBUTING.md, README rewrite, CHANGELOG 0.1.0.
- 2026-11-08 Gate 9 review fixes (5f563ae): H1 bundled THIRD_PARTY_LICENSES.txt (gen-licenses.mjs, verify step), M2 uninstall removes the HKCU product key, L1 forced host options, L2 test, L3 dirty build marker, L5 install-test robustness.
- 2026-11-07 Phase 9 (309d2af, 1257c5f): bundled + user mpv.conf, NSIS uninstall data hook, install-test.ps1, docs/signing.md, About build commit; tag v0.1.0 pushed, draft release built by release.yml.
- 2026-11-07 Gate 8: review M1 (retry tick on `online` + Retry button, ServerHome clears its error), M2 (ToUnicode layout characters from the video window, ContextMenu focus, next/previous episode across seasons, deferred items re-targeted), L1/L5/L6; verify.ps1 exit 0; GATE 8 PASSED.
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
