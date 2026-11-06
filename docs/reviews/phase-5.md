# Review: Phase 5 UI with web-app parity

- Date: 2026-11-07
- Scope: commit `bd2369b` (`git diff 11807e7..bd2369b`, 66 files):
  - `crates/silo-api/src/{catalog,api,session,mock,error}.rs`, `tests/contract.rs` + 6 new fixtures,
    `scripts/{gen-catalog-fixtures,extract-oas-subset}.mjs`;
  - `apps/desktop/src-tauri/src/{catalog,silo,settings,lib}.rs`, `build.rs`, `capabilities/main.json`,
    `tauri.conf.json` (CSP);
  - `apps/desktop/src/{App.tsx,components/*,routes/*,lib/*,i18n/*}` and the tests;
  - ADR-0004 amendment 3, `docs/research/silo-api.md` §13.
- Checked against: `prompt.md` PART B (Non-negotiable rules, Phase 5, Gate 5), `CLAUDE.md`, ADR-0001, ADR-0004
  (amendments 2–3), and upstream `.ref/silo-server@6791aac`. The upstream OAS and the `web/` sources were read as
  data only.
- Reviewer: fresh subagent (independent; did not write this code).

**Verdict: PASS WITH FINDINGS.** No High finding. The phase is in good shape:
- the 7 new operations exist in the OAS, and all §13 line citations are exact;
- the OAS subset regenerates byte-identically;
- no token crosses IPC, and artwork goes through a host-side scheme without credentials;
- every Play/Resume control goes through one `usePlay` → `player_play` path that carries the item id, the file id,
  the start position and the audio/subtitle choice;
- en/de are at parity;
- `verify.ps1` is green.

Four Medium findings should be fixed before Gate 5 is closed. Phase 6 builds its `PlayRequest` and
artwork handling directly on this code:
- **M1:** the automatic subtitle choice does not follow the web app's rules (default mode, `off` + forced
  subtitles, language matching).
- **M2:** the resume logic drops resume points the web app honours.
- **M3:** the `granary-img` scheme serves SVG from a Tauri *local* origin, with no CSP/sandbox on the response.
- **M4:** the 16 MiB artwork cap is not enforced for chunked bodies.

M5 (no parity or desktop-behaviour evidence) is a documentation and evidence gap, not a code defect.

## Phase 5 requirement coverage
| Requirement | Status | Evidence |
|---|---|---|
| UI per ADR-0001 (own UI, Silo tokens) | Met | `styles/tokens.css` (midnight-cinema from `web/src/app.css@6791aac`, credited), Plus Jakarta Sans, Tailwind 4, lucide icons (ISC, added to THIRD_PARTY_NOTICES) |
| Parity: theme, layout, typography, components | Partly evidenced (M5) | Sidebar (icon rail below `lg`, 260 px above), hero, poster rows, details page and season tabs follow the web app's arrangement. Not compared with a live web UI. No screenshots, and no `MANUAL_TEST_CHECKLIST.md` entry, although ADR-0001 says that comparison goes there |
| Screens: home rows, libraries, search, item + series details | Met | `ServerHome.tsx` (`listHomeSections`, hero = first featured item), `Library.tsx` (sort + cursor paging), `Search.tsx` (300 ms debounce, `type=video_with_episodes`, L4), `Item.tsx` (movie/episode PlayPanel, series/season EpisodeList, cast) |
| Desktop settings: server mgmt, player, display, audio, subtitles, language | Met (L6) | `Settings.tsx` + `settings.rs` (clamped, unknown fields dropped, corrupt file → `.bad`, atomic write). "Server management" is a link to the server list. Player/display/audio values are stored only; they take effect in Phases 6–7 |
| Mouse-first: hover states, context menus, scroll | Met (L11) | Card hover lift, `ContextMenu.tsx` (Play/Details, Esc/outside/blur close, arrow keys), Row wheel + arrow buttons |
| Keyboard navigation + focus rings | Met | `focus-visible` rings on links, cards, tabs and switches; skip link; `role=tab`/`switch`/`menu`. Season tabs have no arrow-key roving (acceptable) |
| Responsive small → 4K | Met (not verified at runtime) | `auto-fill` grids, rail/sidebar breakpoint, gutter `max-w-[2400px]`. `minWidth` 960. At 4K with 100 % scaling the content stops at 2400 css px |
| True-black theme | Met | `[data-theme="true-black"]` sets `--background`/`--sidebar` to `#000`; selectable; applied at startup (`App.tsx` `ApplySettings`) |
| Native window / snap layouts | Met | Native decorations kept (no custom title bar); window-state plugin unchanged. Not verified manually (M5) |
| DPI scaling, reduced motion | Partly (L5) | rem-based layout. `motion-reduce:` on cards and switches, and Row scroll honours `prefers-reduced-motion`, but there is no global rule (other transitions stay) |
| Play hand-off with item id, media source, start, audio/subtitle | Met (M1, M2, L3) | `usePlay.ts` is the single path (hero, card menu, grid, PlayPanel, episode list, SeriesPlay) → `getWatchState` → `PlayRequest{serverId,itemId,fileId,startSeconds,audioIndex,subtitleIndex}` → `player_play` (validated, logged, emits `player://handoff`) |
| Browser `<video>` path unreachable | Met (L10) | No media element in the source (`no-video.test.ts` scans the source tree); the flow test also asserts `document.querySelector("video") === null` |
| Rule 1 truth | Met (M1, M2) | All 7 ops exist at the cited lines (L142607, L150065, L152009, L155969, L156533, L179770, L234067). Every query parameter sent is checked against the OAS parameter list in `http_client_speaks_the_catalog_contract`. The `X-Profile-Id` "required" claims match the OAS. Two code comments state web-app behaviour that upstream does not have (M1 subtitle semantics, M2 "30 s matches the web app") |
| Rule 6 secrets / no token in WebView | Met | Catalog views carry no tokens. `fetch_image` builds from `self.http` directly, with no bearer, profile or device headers; the contract test asserts this. Artwork links are presigned (`exp`/`sig`), and exposing them to the WebView is intended |
| Rule 11 unwrap/expect/TODO | Met | grep of the diff: `unwrap()`/`expect(`/`panic!` only in `#[cfg(test)]` modules with `allow` + reason and in `tests/`. No TODO/FIXME |
| i18n en/de parity, plurals | Met | 173/173 keys, identical `{{placeholders}}`. `_one`/`_other` for `browse.count`, `item.minutes`, `settings.seconds`. Every literal `t("…")` key in the source exists. `errors.ts` covers `profile_required`, `bad_artwork_url`, `invalid_request` |
| Capabilities least privilege | Met | 9 new `allow-*` command permissions (generated by `build.rs`), matching `generate_handler!` one to one. No new core or plugin permission. `servers_activate` is still unused (phase-4 L8, deferred) |
| CSP | Met (M3, L10) | `img-src 'self' data: blob: http://granary-img.localhost granary-img:` is the correct WebView2 form (Tauri 2.12.5 serves custom schemes as `http://<scheme>.localhost` unless `useHttpsScheme`). `https:`/`http:` were removed |
| Gate 5 E2E (login → browse → details → play) | Met by the allowed substitute (L8) | Vitest `flow.test.tsx` covers connect → login → PIN → home, and library → details → audio pick → Resume → asserted `player_play` args. Rust `catalog::tests::login_browse_details_play_handoff` runs the host path end to end. The tauri-driver skip is justified: the approved download list (prompt.md L62) does not include `msedgedriver`, and Gate 5 explicitly allows component and integration tests |

### Checks run (this session)
- `cargo test --workspace --locked` → **exit 0**. Results: granary_lib 23, display 14, player 13, silo-api 26 unit,
  contract 8, tls 1 passed; live 1 ignored.
- `pnpm --filter desktop exec vitest run` → **exit 0** (6 files, 17 tests).
- `pnpm --filter desktop exec tsc --noEmit -p .` → **exit 0**.
- `pnpm --filter desktop run lint` → **exit 0**.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → **exit 0**.
- `scripts/verify.ps1` → **exit 0**, `VERIFY OK`. Steps: libmpv present, version sync, rustfmt, clippy, cargo test,
  typecheck, eslint, prettier, vitest, frontend build, secret scan.
- `extract-oas-subset.mjs` (copy writing to the scratchpad) on `.ref/silo-server@6791aac` → 16 operations, 75
  schemas. SHA-256 identical to the committed `openapi-subset.json`
  (`19B819E2…EE9B`).
- `grep operationId` in the upstream OAS → all 7 §13 line numbers are exact. The `catalog.rs` schema-line comments
  (L25578, L26983, L28026, L35329, L54986, L55124, L57950, L46528) are also exact.
- i18n parity and key-usage scripts (scratchpad) → no missing or extra keys, no placeholder mismatch.
- Tauri source `tauri-2.12.5/src/webview/mod.rs:1961-2002` (`is_local_url`): read for M3.
- Not run: `tauri dev` / a manual UI pass, and the live test (no `.env.local`).
- The working tree held an untracked `crates/silo-api/src/playback.rs` (Phase 6 work in progress). `lib.rs` does not
  reference it, so it did not affect the results above.

## Findings

### High
None.

### Medium

- **M1 The automatic subtitle choice does not follow the web app's rules.** `apps/desktop/src/lib/play.ts:28` (`lang`), `play.ts:30-60` (`defaultSubtitle`), test `play.test.ts` "follows the profile subtitle mode".
  - **Upstream behaviour** (`web/src/pages/watchRouteHelpers.ts@6791aac` L297-301 and `web/src/player/utils/subtitleSort.ts` `resolveSubtitleAutoSelect`, L219-275):
    - A missing `effective_subtitle_mode` is treated as **`auto`**, not "the file's default track".
    - **`off`** still selects the **forced** track in the *audio* language when `effective_show_forced_subtitles` is true (the default).
    - **`auto`** with no subtitle language, or with audio already in that language, selects only the forced track for the audio language, and only when forced subtitles are on. Otherwise it selects the best *full* track in the subtitle language: full beats forced, plain beats SDH, text beats bitmap.
    - Any other value (`default`, `forced_only`) selects **nothing**.
    - Languages are compared after `Intl.Locale` canonicalisation (`languageTags.ts`), so `eng` matches `en` and `ger` matches `de`.
  - **Granary today:**
    - It invents meanings for `default` and `forced_only`. The doc comment presents them as the OAS vocabulary's meaning, but `internal/apiv2/profiles.go@6791aac` L43 says the field is "free-form until the vocabulary is ratified (#135)". That is a rule 1 truth issue.
    - It ignores `effective_show_forced_subtitles` (it is not even deserialized in `catalog.rs` `WatchDetail`).
    - It compares only the primary subtag, so `eng` and `en` do not match. The OAS says tracks are ISO 639-1 "where one exists", but older scans may carry ISO 639-2, and Granary's own fixtures and mock use `eng`/`deu`.
  - **Failure scenarios:**
    - With a profile set to "Off", a German-dubbed film with forced German signs shows no subtitles in Granary; the web app shows the signs.
    - With the mode unset, Granary turns on any track the file flags as default, often a full SDH track, where the web app shows none.
    - With `effective_subtitle_language: "en"` and tracks tagged `eng`, Granary never selects the English track.
  - **Fix:**
    - Port `resolveSubtitleAutoSelect` semantics, including `effective_show_forced_subtitles` and `effective_subtitle_track_signature` (exact match first).
    - Default an absent mode to `auto`.
    - Compare languages through `Intl.Locale` canonicalisation, the same as `canonicalLanguageTag`/`normalizeLanguageCode`.
    - Treat unknown modes as "none".
    - Cite the upstream file in the comment, and add `play.test.ts` cases for `off` + forced, unset mode, and `eng`↔`en`.
    - `subtitles.preferSdh` (L6) can then slot into the tie-break.

- **M2 Resume ignores rewatches in progress and applies a 30 s floor that the web app does not have.** `play.ts:3-5`, `play.ts:63-68`.
  - **Upstream:** "Watched items store position 0, so any nonzero position is a live resume point — including a rewatch in flight (played stays true)" (`watchRouteHelpers.ts@6791aac` L376-378: `initialPosition = restart ? 0 : position_seconds ?? 0`). Thresholds are admin settings applied by the server (`playback.min_resume_threshold`, a percentage, `admin_settings.go@6791aac` L459).
  - **Granary:** `resumePosition` returns 0 when `played` is true, and for any position under 30 s. The comment claims this "matches the web app's behaviour", which is a rule 1 truth issue.
  - **Failure scenario:** the user rewatches a finished film, stops at 1:10:00, and opens it again. Granary offers only **Play** (from the start), while the web app offers Resume at 1:10:00. The server has accepted any position under 30 s as a resume point, but Granary discards it.
  - **Fix:**
    - Use `position_seconds > 0` regardless of `played`, and drop `MIN_RESUME_SECONDS` (or keep it only as a display decision with an honest comment).
    - Update the tests (`played: true, position_seconds: 754` → 754).
    - Also consider the version-specific position (L9).

- **M3 `granary-img` responses come from a Tauri *local* origin and may be SVG, with no CSP/sandbox, so a server-supplied SVG would run with the main window's IPC permissions if it were ever loaded as a document.** `crates/silo-api/src/api.rs:288` (accepts any `image/*`, including `image/svg+xml`), `apps/desktop/src-tauri/src/catalog.rs:168-182` (response headers: only content type, cache control, `nosniff`), `lib.rs` scheme registration, no `on_navigation` guard.
  - **Mechanism:**
    - Tauri 2.12.5 `Webview::is_local_url` (`src/webview/mod.rs:1979-2001`) treats `http://<registered-scheme>.localhost` as local. A document from `http://granary-img.localhost/...` in the `main` window therefore gets the `main` capability: `auth_logout`, `servers_add`/`remove`, `settings_set`, `player_play`, and so on.
    - Tauri injects its CSP only into its own frontend assets, not into custom-scheme responses.
    - The bytes are chosen by the Silo server, or by any https host its artwork URLs name. A malicious or compromised server, or a poisoned metadata provider, can return an SVG with `<script>`.
  - **Today it is reachable only by a top-level navigation to such a URL.** `<img>` never runs SVG script, and wry's drop target swallows in-page drag/drop, so I found no trigger without user action. That is why this is Medium, not High. The mock serves SVG (`mock.rs:736`), so this content type is exercised as normal.
  - **Fix:**
    - Allow-list raster types (`image/jpeg|png|webp|avif|gif`) in `fetch_image` and reject `image/svg+xml`; switch the mock placeholder to a tiny PNG.
    - Add `Content-Security-Policy: default-src 'none'; sandbox` and `Cross-Origin-Resource-Policy: same-site` to every scheme response.
    - Add an `on_navigation` handler on the main window that allows only the app URL, so `granary-img`, `ipc` and remote origins can never become the top-level document.

- **M4 The 16 MiB artwork cap is not enforced for bodies without `Content-Length`.** `crates/silo-api/src/api.rs:291-298`.
  - **Mechanism:** the cap is checked against `content_length()` (absent for chunked or HTTP/2 streamed responses), then `resp.bytes().await` buffers the **whole** body before the length check. The only bound is the 20 s client timeout.
  - **Failure scenario:** a misbehaving or malicious server streams a chunked "image" for 20 s at LAN speed (hundreds of MB to over 1 GB). A home page fires one such request per visible poster, so host RSS spikes to several GB, or the process dies.
  - **Contradiction:** ADR-0004 amendment 3 (`docs/adr/0004-http-auth-token-storage.md:86`) states "caps the body at 16 MiB".
  - **Fix:** read with `resp.chunk()` in a loop, abort as soon as the total exceeds `MAX_IMAGE_BYTES`, and add a wiremock test with a chunked oversize body. Optionally bound concurrent artwork fetches with a semaphore (for example 8).

- **M5 There is no evidence for parity and desktop window behaviour, and no `MANUAL_TEST_CHECKLIST.md` entry, although ADR-0001 says the comparison goes there.** ADR-0001 "Consequences": "Visual comparison against a live Silo web UI goes into `MANUAL_TEST_CHECKLIST.md` until a test server is available". The Phase 5 diff does not touch the checklist.
  - **Unverified items:** parity, snap layouts, DPI change on the browse UI, 4K/small-window layout, reduced motion, keyboard-only navigation and the true-black theme. Tests and code reading cover none of them.
  - **Fix:**
    - Add a Phase 5 section to `MANUAL_TEST_CHECKLIST.md` covering side-by-side comparison with the web UI (home, library, details, series), Win+Z snap, 100/150/200 % scaling, 960 px and 4K widths, Windows "Animation effects" off, Tab-only walkthrough, and true-black on OLED.
    - Capture at least one `tauri dev` screenshot set (mock server) in `docs/` as Gate 5 evidence.

### Low

- **L1 The `ItemGrid` load key omits `serverId`, and the "more" error/loading state is not keyed.** `routes/Library.tsx:29` (`key = JSON.stringify(query)`), `:39-40`, `:52`.
  - **Effect:** the same `libraryId`/`sort` on another server would reuse the first page if the component stayed mounted. That is not reachable today, because switching servers leaves the Shell. A load-more error from an old query stays visible after the sort changes, and the new query's "More" button stays disabled until the old request finishes.
  - **Fix:** key on `${serverId}|${JSON.stringify(query)}`, and store `moreError`/`loading` inside the keyed `more` state.

- **L2 Sub-loaders show re-auth errors inline.** `routes/Item.tsx:86` (PlayPanel) and `:217-218` (EpisodeList).
  - **Effect:** a `profile_verification_required`/`signed_out` from `getWatchState`, `listSeriesSeasons` or `listSeasonEpisodes` renders "This profile needs its PIN." instead of going to the picker. This happens when the parent `getCatalogItem` succeeded just before, for example after a PIN change.
  - **Fix:** route them through `needsReauth`/`goReauth`, as the parent does.

- **L3 `player_play` validation is minimal.** `src-tauri/src/catalog.rs:61-70`.
  - **Gaps:** `server_id` is not checked against the registry, there are no length caps on the ids, `subtitle_index` may be negative, `audio_index` is unbounded, and `start_seconds` has no upper bound (`1e300` passes).
  - **Fix:** fine for a stub. Phase 6 must re-resolve the request against `getWatchState` (file belongs to item, track indices exist) before building any stream URL. Add the cheap checks now.

- **L4 Search sends `type=video_with_episodes` without the capability check that the OAS asks for.** `routes/Search.tsx:29`.
  - **OAS** (`listCatalogItems` `type`): "check getCatalogSearchCapabilities.video_with_episodes_scope first".
  - **Fix:** document the assumption for @6791aac in §13, or call the capability once and fall back to `type=video`.

- **L5 Reduced motion is only partly honoured.** `Shell.tsx:23` (`transition-colors`), `Item.tsx:233,263`, `Row.tsx:16` (`transition`).
  - **Fix:** add a global `@media (prefers-reduced-motion: reduce) { *, ::before, ::after { transition-duration: 0s !important; animation-duration: 0s !important; scroll-behavior: auto !important } }` in `styles/index.css`.

- **L6 Settings details.**
  - **Out-of-order saves:** `useSettings.update` (`lib/settings.ts:99-109`) fires one unserialised `settings_set` per change. Dragging the subtitle-scale slider issues dozens of writes. Async commands are not ordered, so the file, and the UI after `setSettings(saved)`, can end on an earlier value. Fix: serialise or debounce the saves.
  - **seekStep:** the select offers 5/10/15/30, while the store accepts 1–120, so a hand-edited 20 shows as 5.
  - **preferSdh:** stored but not used by the subtitle defaults (see M1).
  - **Read errors:** `SettingsStore::load` (`settings.rs:163`) treats any read error as "no file", so the next save overwrites the real file. This is the same pattern that phase-4 L2 fixed for the registry.

- **L7 Profiles still has no way back.** `routes/Profiles.tsx`.
  - **Tracking:** phase-4 L9 was recorded as "superseded by the Phase 5 shell", but Profiles is outside the Shell. The Shell's "Profile" link now leads users there, where the only exits are picking a profile or the browser back gesture.
  - **Fix:** add a "Back"/"Servers" control and `aria-pressed` on the tiles, or render Profiles inside the Shell.

- **L8 The E2E substitute is adequate but split, and it skips the IPC/ACL layer.**
  - **Coverage today:** vitest test 1 stops at the server home; test 4 starts already signed in at a library. The Rust test calls `SiloState` methods directly. Nothing exercises `generate_handler!` registration, argument names (`serverId`/`itemId`/`request`) or the capability file together.
  - **Gap:** a missing `allow-*` entry or a renamed argument would pass every test and fail only in the app.
  - **Fix:**
    - Chain test 1 into library → details → play.
    - Add one `tauri::test::mock_builder()` test that loads the real capability and invokes `catalog_browse` and `player_play` through `get_ipc_response`.
    - Add coverage for the search, episode-list and SeriesPlay play paths.

- **L9 Version default and resume position ignore upstream hints.**
  - `defaultVersion` (`play.ts:14-17`) falls back to `versions[0]` with the comment "the server orders them", which is unverified. Upstream uses `effective_version_{resolution,hdr,codec_video,edition_key}` and `user_data.last_*` as hints (`watchRouteHelpers.ts@6791aac` L380-402).
  - The item-level `position_seconds` is applied to whichever version the user picks.
  - **Fix:** deserialize the `effective_version_*` fields and use them when `last_file_id` is absent.

- **L10 The "no `<video>` path" guarantee rests on a source grep.**
  - **Gap:** the CSP has no `media-src`, so `default-src 'self'` would allow `self` media, and there is no navigation guard (see M3).
  - **Fix:** add `"media-src": "'none'"` to the CSP, which makes the browser playback path unreachable at policy level too.

- **L11 The ContextMenu effect re-subscribes and re-focuses the first item on every parent render.** `components/ContextMenu.tsx:25-41`, with `onClose` passed as an inline arrow from `PosterCard.tsx:92`.
  - **Effect:** a re-render while the menu is open (for example a `usePlay` busy toggle) steals focus back to the first item.
  - **Fix:** focus once in a mount-only effect, and keep `onClose` in a ref.

- **L12 The artwork scheme can be pointed at any https host, including loopback and private addresses.** `api.rs:311-323`.
  - **Exposure:** the WebView, or server-supplied data, can make the host send credential-less GETs to, for example, `https://192.168.1.1/...`. Only `image/*` bodies come back, so this is blind and low impact, and comparable to what a browser `<img>` would do.
  - **Fix (optional):** allow only the server origin plus known metadata CDNs, or reject literal private/loopback IPs for foreign hosts.

## Resolution (orchestrator, before Gate 5)

| Finding | Status | Change |
|---|---|---|
| M1 | Fixed | `play.ts` `defaultSubtitle` ports `resolveSubtitleAutoSelect` (`subtitleSort.ts@6791aac`): absent mode → `auto`, `off` + `effective_show_forced_subtitles` → forced track for the audio language, `always` honours `effective_subtitle_track_signature` first, unknown modes → none, Intl.Locale canonicalisation (`eng`↔`en`, `ger`↔`de`), full > forced, plain > SDH, text > bitmap, embedded > external. `WatchDetail` (TS + Rust) gains `effective_show_forced_subtitles`, `effective_subtitle_track_signature`, `effective_version_edition_key`. Tests: off + forced, unset mode, explicit empty language, signature, ISO 639-2 tags, SDH/bitmap ranking |
| M2 | Fixed | `resumePosition` = any `position_seconds > 0`, also when `played` (rewatch); 30 s floor removed; comment cites `watchRouteHelpers.ts@6791aac` L376-378 |
| M3 | Fixed | `fetch_image` accepts only `image/jpeg|png|webp|avif|gif` (`api::raster_image_type`); the scheme re-checks the type and sends `Content-Security-Policy: default-src 'none'; sandbox` + `nosniff` on every response (also 404s); main window `on_navigation` allows only `tauri://localhost`, `http(s)://tauri.localhost` and (debug) the dev server; mock artwork is now a generated PNG. `Cross-Origin-Resource-Policy: same-site` was **not** added: `granary-img.localhost` and the page origin are not guaranteed to be same-site, which would block every poster; the sandbox CSP is the protection. Tests: `scheme_never_serves_svg_and_always_sandboxes`, `only_the_app_origin_is_navigable`, and a live check in `scripts/ui-evidence.mjs` (navigating to the scheme is blocked) |
| M4 | Fixed | Body read chunk by chunk with `resp.chunk()`, aborted once `MAX_IMAGE_BYTES` would be exceeded. Test `artwork_is_capped_without_content_length_and_must_be_raster` streams 64 MiB chunked without Content-Length |
| M5 | Fixed | `MANUAL_TEST_CHECKLIST.md` "Browse UI (Phase 5)" section (web-UI side-by-side, snap, scaling, 4K, animation effects, keyboard-only, true-black). `scripts/ui-evidence.mjs` drives the real app (mock server) over the WebView2 DevTools protocol: 14 screenshots in `docs/evidence/phase-5/` and 13 checks in `checks.json` (artwork decodes, no horizontal overflow at 960/1920/3840 px and 150 %/200 % scale, icon rail below lg, 2400 px cap, no `<video>`, visible focus ring, reduced motion, true-black #000, navigation guard), all PASS |
| L5 | Fixed | Global `prefers-reduced-motion` rule in `styles/index.css` (verified by ui-evidence) |
| L9 | Fixed | `defaultVersion`: last file, then `last_edition_key`/`effective_version_edition_key`, then a version without edition, then the first (`usePlaybackSession.ts@6791aac` `selectFileId`) |
| L10 | Fixed | CSP `media-src 'none'` |
| L3 | Deferred → Phase 6 | `player_play` is replaced by the playback controller, which re-resolves the request against `getWatchState` |
| L1, L2, L4, L6, L7, L8, L11, L12 | Deferred | Recorded in PROGRESS.md "Known issues" (Phase 8 polish unless noted) |
