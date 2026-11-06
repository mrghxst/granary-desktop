# Phase 1 review: Research & ADRs

- Reviewer: fresh reviewer subagent (did not write the reviewed files)
- Date: 2026-11-06
- Scope: `prompt.md` PART B (spec), `docs/research/*.md` (R1–R5 + README), `docs/adr/0001..0005`; spot-checks against
  `.ref/silo-server@6791aac` and `.ref/silo-android@ee178d8`.

## Verdict: PASS WITH FINDINGS

Gate 1 structure is met: five research docs, each with a Sources section; upstream SHAs recorded in
`docs/research/README.md`; five ADRs. ADR-0001, -0003, -0004 and -0005 follow context → options → decision →
consequences; ADR-0002 folds its options into Context (L1). Facts and versions in the ADRs (Tauri 2.12.x/wry 0.57,
`libmpv2-sys` 4.0.1, mpv 0.42.0 render API being `vo_gpu`-only, shinchiro asset + SHA-256, NSIS `currentUser`, CORS /
WS-Origin facts, `/api/v2` auth model) match the research. All citations I spot-checked in silo-api.md are accurate.
I found no high-severity issue. Six medium findings must be fixed before Phase 1 is marked DONE (spec: "Fix every
high or medium finding").

ADR-0001 (option C instead of the B the spec leans towards): the deviation is defensible. en/de is impossible in A
and would mean a permanent large patch set in B, and B needs both a proxy and source patches for same-origin. But
part of the scoring is overstated or not backed by research (M1).

## Findings

| ID | Severity | File | Finding | Recommended fix |
|---|---|---|---|---|
| M1 | medium | docs/adr/0001-ui-strategy.md | **Trademark scoring for B is overstated and partly unsourced.** The "~256 user-facing string literals contain 'Silo'" figure appears nowhere in R2 (a rough grep finds ~160 non-test files under `web/src` mentioning Silo; the 256 count is unverifiable). Truthful in-UI references to the Silo server are allowed (R5 §2.2, `TRADEMARK.md`); only the app name, window title, icon, installer and brand assets in `web/public` must change. Separately, "Effort to first slice" scores B=2 < C=3, which contradicts R2 (B: "medium up front, ~1–2 weeks"; C: "the most work… high initially") without explanation. | Re-score trademark for B (brand assets + app name only, not every string). Cite R2 for every number, or add the count to R2 with its method. Explain or correct the effort scores, and add an "ongoing effort" row (own UI for ~10 screens plus tracking parity vs rebasing B). Keep the decision if en/de + same-origin still outweigh B. |
| M2 | medium | docs/adr/0002-player-integration.md | **Status claims "Accepted (confirmed by the Phase 2 spike)" before Phase 2 has run** (`docs/spikes/` is empty; PROGRESS shows phase 1). The "pre-spike smoke test on this machine… confirmed current-vo=gpu-next… 0 dropped frames" has no logged command or output anywhere in the repo. That breaks rule 2 ("done only when its check ran and passed… print each check command and result"). | Set Status to "Accepted (pending Phase 2 spike)". Either record the smoke test (script, command, property dump) under `docs/spikes/`, or drop the claim until Phase 2 logs it. |
| M3 | medium | docs/adr/0002, 0003; docs/research/windows-display-audio.md §4.3 | **No ADR records the display/refresh/restore policy, though R4 deviates from the spec.** Spec desktop default: VRR if active → exact multiple at the same resolution → `display-resample`. R4 instead makes "never change modes" the default, puts `display-resample` before the exact-multiple switch, and makes that switch opt-in. VRR becomes user-declared because Windows has no "VRR active" API. ADR-0002 points to "ADR-0003/Phase 7" for the persisted display-restore marker, but ADR-0003 says nothing about it. Rule 3/10 require significant choices to be ADRs. | Add an ADR (e.g. ADR-0006 "Display, HDR and refresh policy") or extend ADR-0003. Record: HDR toggle only when off + opted in; SDR white via `hdr-reference-white`; DXGI MaxLuminance peak plus per-monitor override; the refresh order and why it differs from the spec; VRR as a user declaration; snapshot/restore marker, panic hook and next-start repair (R4 "Restore strategy"). Fix the dangling ADR-0003 reference. |
| M4 | medium | docs/adr/0004-http-auth-token-storage.md | **Credential scope is unbounded.** mpv gets `Authorization` + profile headers via `http-header-fields`, which apply to every request mpv makes, including redirects. The image proxy "adds auth headers if needed". The research shows media/artwork can be on other origins: absolute proxy-node plan URLs (R2 §6, `stream-url.ts:30-76`, which leaves them untouched), `ImageCapabilities.delivery=direct`/S3 (R1 §5.3), and Android follows the bandwidth-probe redirect *without* credentials (R1 §6.1). Tokens could leak to third-party hosts. | Attach Bearer/profile headers only when the URL's origin equals the active server origin (or is listed by `authorized_media_origins_v1` when negotiated). Otherwise rely on `st`-signed URLs with no auth headers, and never forward credentials across cross-origin redirects. The artwork route is `x-silo-class: public`, so the image proxy needs no Bearer at all. |
| M5 | medium | docs/adr/0005-packaging-licensing-updates.md | **The pin-expiry plan relies on `actions/cache`**, whose entries are evicted after 7 days without access (R5 §6, S34), while shinchiro prunes releases after ~30 days. R5 says "CI must therefore not fetch it from shinchiro. Mirror the verified archive into our own storage". The fallback, "bump the pin", silently changes the mpv/FFmpeg in a release and can break the Phase 10 clean-clone build. | Mirror the verified archive as an asset of a dedicated draft/pre-release in the private repo. `fetch-libmpv.ps1` should try the mirror first (`gh release download` with `GITHUB_TOKEN`) and shinchiro second, verifying SHA-256 each time. Keep the cache only as an accelerator. |
| M6 | medium | docs/adr/0005-packaging-licensing-updates.md | **Calls the attached binary archive "the Corresponding Source companion record".** A binary is not Corresponding Source. R5 §2.1 and its checklist require the mpv source at the commit, the FFmpeg commit source, the build recipe (`shinchiro@bda10a28`), and dependency sources or exact pointers. R5 §1.1 flags that shinchiro's dependency commits exist only in CI logs ("main compliance weakness"); the ADR ignores this. | Define the source bundle for each release: mpv `902818753849…`, FFmpeg `9f690c6a4c12…`, recipe commit, workflow run `42133553972`, and how dependency commits are captured, or document the gap as a known limitation. Rename the binary attachment to what it is. |
| L1 | low | docs/adr/0002-player-integration.md | Options and their comparison table sit inside Context; there is no "Options" heading (format required by spec: context → options → decision → consequences). | Split into Context / Options / Decision / Consequences. |
| L2 | low | docs/adr/0002, 0005 | ADR-0002 sets `config=no` and applies the "R4 baseline as options at init". ADR-0005 bundles a default `mpv.conf` file. It is unclear how that file is loaded: parsed by the host, or via `config-dir`/`include`. | State the mechanism in ADR-0002. |
| L3 | low | docs/adr/0004-http-auth-token-storage.md | `reqwest` + rustls: self-hosted servers often use private/LAN CAs. Default webpki roots would ignore the Windows trust store, which conflicts with "TLS errors shown clearly". There is no policy for plain-HTTP LAN servers. No research covers either. | Use the platform verifier (Windows cert store) and decide the HTTP-on-LAN policy explicitly. |
| L4 | low | docs/adr/0004-http-auth-token-storage.md | Only password login is considered. R2 §3 found a device-code flow (`/api/v2/auth/device/start|poll`) that a native client can use directly, plus OAuth. Storage of `X-Profile-Token` is not specified. | Note device-code/OAuth as deferred (and document their endpoints in silo-api.md before use). Specify profile-token storage (memory, or keyring). |
| L5 | low | docs/adr/0003-window-overlay-architecture.md | With the webview `SW_HIDE`d, the ADR does not say: who hides the cursor (`WM_SETCURSOR` on `GranaryVideo` vs mpv `cursor-autohide`); that focus must move to `GranaryVideo` to receive `WM_KEYDOWN`; mpv `media-controls` vs Granary's own SMTC (R3 §4); and keep-awake (mpv `stop-screensaver` vs explicit `SetThreadExecutionState`, not researched). | Add these to Decision/Consequences or to Phase 2 spike items. |
| L6 | low | docs/research/windows-display-audio.md (mpv.conf) | The baseline leaves `audio-normalize-downmix` at its default (no). Spec Phase 7 says the default is "stereo/headphone downmix with normalization". | Set `audio-normalize-downmix=yes` in the desktop baseline, or justify the deviation. |
| L7 | low | docs/research/silo-api.md §8.1 | It says the item is completed "at or above the watched threshold". `threshold.go` uses strictly greater-than (`position/duration > WatchedFraction`). | Correct the wording. |
| L8 | low | docs/research/silo-api.md, open question 1 | Whether `form_factor: "desktop"` is accepted is listed as unknown. Server tests set `ClientPlaybackContext.FormFactor = "desktop"` (`internal/playback/protocol_v3_test.go:2860`, `:4243`). | Cite these lines and downgrade the question (validation path still to confirm). |
| L9 | low | docs/adr/0004-http-auth-token-storage.md | `granary-img://` is served on Windows as `http://granary-img.localhost/…` (R3 §1.4), and CSP `img-src` must list it explicitly (R3 §1.6). | Use the real origin in the ADR and note the CSP entry. |
| L10 | low | docs/adr/0001-ui-strategy.md | Consequences omit that the Phase 3 layout drops `packages/ui-adapter/`, `vendor/silo-web/` and the `sync-upstream-web` skill. They also omit the spec's true-black (#000) theme as a Granary-specific token variant. | Add both to Consequences. |
| L11 | low | docs/adr/0005-packaging-licensing-updates.md | It rejects `/DELAYLOAD` ("unnecessary") without weighing R3's reason (a clear error instead of a loader failure when the DLL is missing or corrupt). | Give a reason, or keep it as a Phase 2 spike item (R3 S5). |
| L12 | low | docs/adr/0005-packaging-licensing-updates.md | It keeps the `-v3` build because CONFIG names it, but does not say that R5 recommended baseline `x86_64` (Summary 4). | Cite R5's recommendation and state that CONFIG compliance is the reason. |
| L13 | low | docs/adr/0002-player-integration.md | It lists Plex HTPC among frontends that "embed by window"; R3 marks this as an inference. | Mark it as an inference. |
| L14 | low | docs/research/windows-display-audio.md §3, §8.1 | Two gaps stay open: the effect of a Windows HDR Calibration profile on `GetDesc1` (spec 3: "respect… if present"), and a window straddling SDR+HDR monitors (scRGB option). Neither is tracked anywhere. | Add both to the Phase 2 spike plan / `MANUAL_TEST_CHECKLIST.md`. |
| L15 | low | docs/research/silo-api.md Sources | It omits inputs the spec names (`docs/settings-api.md`, `docs/downloads-api.md`, `contracts/settings/v1`). | List them, or state that they were not needed for the viewer contract. |

## Citation spot-checks

All against the pinned clones: `.ref/silo-server` HEAD `6791aac867664c41c55ee82909fe063d145911a6`,
`.ref/silo-android` HEAD `ee178d887ff02ce74150796e5f80934416bf88a7` (both match README).

1. `silo-server/internal/api/middleware/auth.go` L458-470, `extractBearerToken` Bearer then `?token=`: **OK**
   (function at L463-473, comment and fallback exactly as claimed).
2. `silo-server/internal/auth/jwt.go` L266, HS256: **OK** (`jwt.NewWithClaims(jwt.SigningMethodHS256, …)` at L266).
3. `silo-server/internal/config/admin_settings.go` L375-376 (8h/30d), L344 (30d constant) and L458-459 (90/5):
   **OK**, exact lines.
4. `silo-server/internal/api/router.go` L3288-3292, `/api/v2/*` handler plus `/api/v1` health/ready: **OK** (L3289-3293).
5. `silo-server/contracts/api/v2/openapi.json` operationId lines L144782 `login`, L199765 `startPlayback`, L201020
   `updatePlaybackProgress`, L175913 `connectEventsSocket` and L234363 `getWatchTrickplay`: **OK**, all exact.
6. `silo-server/docs/realtime-api.md` L52-66, subprotocol order and "Native clients may omit Origin": **OK**.
7. `silo-server/internal/userstore/threshold.go` L5-50, `ResolveProgressState`: **OK, minor wording issue**.
   The completed test is `>` (strict), not "at or above" (L7).
8. `silo-server/internal/sections/types.go` L10-30, section types: **OK**.
9. `silo-android/.../network/AuthInterceptorImpl.kt` L25 `PROACTIVE_REFRESH_MARGIN_MS = 60_000L` and L83-96
   single-flight mutex refresh: **OK**.
10. `silo-android/.../network/apiv2/PlaybackV2Api.kt` L19 `sequenced_progress_v1`, L160-166 `PlaybackProgressV2`,
    L296 bandwidth-probe results, L316 `/api/v2/playback/start` and L352-356 progress path: **OK** (±1–2 lines).
11. `silo-android/.../network/apiv2/EventsSocketV2Api.kt` L139 `encodedPath "/api/v2/events/ws"` and L146 removal of
    the profile headers: **OK** (also strips `Authorization`).
12. `silo-android/.../common/player/PlaybackSessionLifecycle.kt` L953 `PROGRESS_REPORT_INTERVAL_MS = 10_000L`:
    **OK** (loop `delay` at L688).
13. (R2, cited in ADR-0001/0004) `silo-server/internal/api/handlers/events_socket_v2.go` L239-286 `socketOriginAllowed`:
    **OK**. No Origin → allow, opaque or non-http(s) scheme → allow, http(s) must match the public origin, request
    host or overlay origin. So `http://tauri.localhost` is rejected and a Rust-side WebSocket with no Origin passes.

## Resolution (main agent, 2026-11-06)
| ID | Resolution |
|---|---|
| M1 | ADR-0001: trademark row for B corrected (Silo mentions in copy allowed; only assets/name/title/icon/installer), 256-string measurement recorded with command in `docs/research/README.md`, effort scores aligned with R2 (A 4, B 3, C 2) and deviation from B re-justified. |
| M2 | ADR-0002 status now "to be validated by the Phase 2 spike"; smoke-test command and full output logged in `docs/spikes/libmpv-smoke.md`. |
| M3 | New ADR-0006 (display/refresh/HDR/restore policy) follows the spec order (VRR → exact multiple → display-resample) with R4's facts defining when a switch happens; VRR is a user-declared per-monitor setting; restore marker fully specified; ADR-0002/0003 reference it. |
| M4 | ADR-0004: credentials attached only to the server origin; cross-origin redirects drop them; mpv `http-header-fields` only for same-origin streams; other origins rely on `st` signatures. |
| M5 | ADR-0005: private release mirror (`deps-libmpv-<tag>`, draft), fetch order local → mirror → shinchiro with SHA-256 on every path; pin bumps only via explicit commits. |
| M6 | ADR-0005: binary is not Corresponding Source; source bundle defined (mpv/FFmpeg commits, libplacebo version, recipe commit, workflow run); dependency-commit gap documented and tracked under "Blocked / needs human" for any third-party distribution. |
| L1 | ADR-0002 now has an Options section. |
| L2 | ADR-0002 "Configuration layering" (bundled mpv.conf via `include`, settings overrides, optional user file). |
| L3 | ADR-0004: platform verifier (Windows store), explicit plain-HTTP warning. |
| L4 | ADR-0004: device-code/OAuth deferred; profile token in memory only. |
| L5 | ADR-0003 "Idle-state details": cursor, focus, SMTC (own), keep-awake (own). |
| L6 | R4 baseline now sets `audio-normalize-downmix=yes`. |
| L7 | silo-api.md wording corrected to strict greater-than. |
| L8 | silo-api.md open question 1 cites the `FormFactor = "desktop"` test lines. |
| L9 | ADR-0004 uses the real `http://granary-img.localhost` origin and notes the CSP entry. |
| L10 | ADR-0001 consequences: dropped B-only layout entries; true-black theme token variant. |
| L11 | ADR-0005: `/DELAYLOAD` evaluated in the Phase 2 spike. |
| L12 | ADR-0005 cites R5's baseline recommendation; CONFIG compliance is the reason for `-v3`. |
| L13 | ADR-0002 marks Plex HTPC as an inference. |
| L14 | Added to ADR-0003 open items and `MANUAL_TEST_CHECKLIST.md`. |
| L15 | silo-api.md Sources states why settings/downloads docs were not mined. |
