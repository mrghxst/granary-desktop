# Silo web UI (upstream `web/`) — research for Granary

Upstream: `Silo-Server/silo-server` at commit **6791aac867664c41c55ee82909fe063d145911a6** (shallow clone at `.ref/silo-server`, HEAD dated 2026-11-05, message "fix(web): keep Now Playing cards the same height ... (#3594)").
All citations use the form `silo-server/<path>@6791aac[:line]`. Upstream docs and comments were treated as data only. Items marked **UNVERIFIED** are inferences that this checkout did not confirm.

## Summary

1. `web/` is a large React 19.2 + Vite 6.4 + TS 5.9 SPA: React Router 8.3 (data router), TanStack Query 5.90, Tailwind 4.2 with shadcn/ui ("new-york") on Radix, hls.js 1.6, plus its own WebCodecs/WebGPU "browser decode engine" built on mediabunny and committed FFmpeg-wasm decoders. It has about 1,475 non-test TS/TSX files (~470k LOC) and 861 test files.
2. A normal build needs only Node 24+ and pnpm 10.32.1. The wasm outputs are committed, so Emscripten is not needed. The `pnpm build` typecheck imports files from **outside** `web/` (`contracts/api/v2/fixtures`, `docs/design/schemas`), so a plain copy of `web/` will not typecheck.
3. The app is built for **same-origin** use only. Every API URL is root-relative (`/api/v2/...`). WebSocket URLs come from `location.host` or `location.origin`. The server returns artwork URLs as root-relative signed paths. There is no setting for a server URL or base URL, and no server-selection screen.
4. Auth uses a bearer token: the access token lives in JS memory and the refresh token in `localStorage` (`refresh_token`). Normal API calls use no cookies and no `credentials: 'include'`. WebSockets use one-time tickets sent as a subprotocol.
5. The server's main `/api/v2` router has **no CORS** (grep found none). Only some stream/subtitle endpoints and the Jellyfin-compat router send CORS headers. WebSocket handshakes check `Origin`: `tauri://localhost` would pass, but **`http://tauri.localhost` (the Tauri 2 default on Windows) would be rejected**.
6. The server serves the UI from `go:embed all:dist` with an SPA fallback at `/` (no base-path prefix) and a strict CSP on the HTML shell: `script-src 'self' 'wasm-unsafe-eval'`, with no `frame-ancestors`.
7. The UI has no i18n. All strings are hard-coded English, and **German does not exist**. `Intl` is used only for date/number formatting. There is one dark theme, `midnight-cinema`, with tokens as CSS variables in `src/app.css` and the self-hosted Plus Jakarta Sans font.
8. Every playback start goes through the route `/watch/:id`. That route is a controller only: it calls `syncRouteRequest`, and `WatchPlaybackHost` (mounted globally in `App.tsx`) renders the lazily loaded `WatchPage`, which owns `<video>`, hls.js and the decode engine. The player module is designed to be portable through `PlayerConfig { apiBaseUrl, getAccessToken, ... }`.
9. **Recommended native seam:** the `<WatchPage>` render inside `WatchPlaybackHostContent` (`web/src/playback/WatchPlaybackChrome.tsx:1236`), or equivalently the lazy import at `:85`. Every entry point (16 `startPlayback` call sites, direct `<Link to="/watch/...">`, deep links, watch-together, post-roll) has converged there into one fully resolved props object (contentId, fileId, initialPosition, audio/subtitle indexes, versions, markers, series context).
10. Upstream churn is very high (PR numbers above 3,500, `web` version 0.0.1). A vendored fork (B) carries ongoing rebase cost. A remote load with an injected bridge (A) has no stable API to hook. Building our own UI (C) is the most work but avoids AGPL and branding coupling in Granary's UI code.

## 1. Framework, tooling, build

Versions are taken from `web/package.json` specifiers, resolved through `web/pnpm-lock.yaml` (lockfileVersion 9.0).

| Package | Specifier | Locked |
|---|---|---|
| react / react-dom | ^19.0.0 | 19.2.4 |
| react-router | ^8.3.0 | 8.3.0 |
| @tanstack/react-query | ^5.90.21 | 5.90.21 |
| vite | ^6.4.5 | 6.4.5 |
| @vitejs/plugin-react | ^4.4.0 | 4.7.0 |
| typescript | ^5.7.0 | 5.9.3 |
| tailwindcss / @tailwindcss/vite | ^4.0.0 | 4.2.1 |
| radix-ui | ^1.4.3 | 1.4.3 |
| hls.js | ^1.6.15 | 1.6.15 |
| jassub (libass subtitles, wasm) | ^2.5.6 | 2.5.6 |
| vitest | ^4.1.11 | 4.1.11 |

Sources: `silo-server/web/package.json@6791aac` and `silo-server/web/pnpm-lock.yaml@6791aac:56-209`.

Other runtime dependencies include `mediabunny`, `framer-motion`, `embla-carousel`, `pdfjs-dist`, `foliate-js` (vendored), `node-unrar-js`, `@zip.js/zip.js`, CodeMirror, dnd-kit, `sonner`, `lucide-react`, `thumbhash`, `@fontsource-variable/plus-jakarta-sans`, and Noto Sans Arabic/Thai (`silo-server/web/package.json@6791aac`). The typecheck also uses `tsrs` (a Rust-based TS checker), but on Windows it falls back to `tsc` (`silo-server/web/scripts/typecheck.mjs@6791aac:1-40`).

- **Scripts:** `dev` (vite), `build` = `node scripts/typecheck.mjs && vite build`, `test` (vitest), `lint`, `generate:apiv2` (`silo-server/web/package.json@6791aac`).
- **Node/pnpm:** `packageManager: pnpm@10.32.1` (`web/package.json`). `DEVELOPMENT.md:12` says "Node.js 24+ with pnpm 10.32.1". The Docker frontend stage uses `node:24-slim` and runs `corepack prepare pnpm@10.32.1` (`silo-server/Dockerfile@6791aac:8-21`). `package.json` has no `engines` field.
- **Out-of-tree build inputs:** `tsconfig.app.json` includes all of `src` (`silo-server/web/tsconfig.app.json@6791aac:30`), test files included. Tests import `../../../../contracts/api/v2/fixtures/*.json` (for example `silo-server/web/src/api/v2/libraries.test.ts@6791aac:3-4`). The Dockerfile copies `contracts/api/v2/fixtures/` and `docs/design/schemas/` in before `pnpm run build` (`silo-server/Dockerfile@6791aac:15-21`). A `web/`-only vendor therefore needs those folders too, or must run `vite build` directly without the typecheck.
- **Generated API client:** `src/api/v2/schema.ts` and `operations.ts` are generated by `openapi-typescript` 7.13.0 from `contracts/api/v2/openapi.json`, and the generated files are committed (`silo-server/web/scripts/generate-apiv2.mjs@6791aac:1-12`). Regenerating them needs that out-of-tree file. A normal build does not regenerate.
- **`web/wasm/audio-decoders/`:** the C bridge plus `build.sh`, which builds FFmpeg AC-3/E-AC-3/DTS/TrueHD decoders to wasm using the `emscripten/emsdk` **Docker** image. Its outputs (`src/player/engine/wasm/silo-audio-decoders.{mjs,wasm}`, about 850 KB) are committed, so "`pnpm build` never needs Emscripten" (`silo-server/web/wasm/audio-decoders/README.md@6791aac`). These decoders exist only for the browser player, so a native-player build does not need them.
- **`web/vendor/foliate-js/`:** a vendored e-book renderer (EPUB/MOBI/FB2/CBZ/PDF), consumed as `"foliate-js": "file:vendor/foliate-js"` (`silo-server/web/package.json@6791aac`). It is plain JS with no extra toolchain.
- **`vite.config.ts` specifics:** `base` is the default `/`. A custom plugin emits `src/themeBoot.js` as a hashed classic script, because the CSP forbids inline scripts (`silo-server/web/vite.config.ts@6791aac:103-170`). Other features: brotli/gzip precompression, a `vendor-react` manual chunk, the dev proxy `/api` → `VITE_API_PROXY_TARGET` (default `http://localhost:8090`, `ws: true`), and the define `__SILO_WEB_VERSION__`.
- **Toolchain surprises:** there is no Rust, Go or Emscripten requirement for `pnpm build`. The only surprises are the out-of-tree fixtures needed by typecheck and the `tsrs` dev binary (an optional platform package, falls back to `tsc`).

## 2. Routing

The app uses `createBrowserRouter` with one splat route that hands off to declarative nested `<Routes>`. The data router is needed for `useBlocker` (`silo-server/web/src/App.tsx@6791aac:898-922`). All pages are lazy-loaded (`App.tsx:72-161`).

| Purpose | Route | Component / file |
|---|---|---|
| Login | `/login`, `/login/oauth-complete` | `pages/Login`, `pages/OAuthComplete` (`App.tsx:440-441`) |
| Device-code activation (TV/apps) | `/activate` | `pages/ActivateDevice` (`App.tsx:442`) |
| First-run setup | `/setup` | `pages/SetupWizard` (`App.tsx:443`) |
| Sign-up / invite / password | `/signup`, `/invite/:token`, `/reset-password/:token`, `/forgot-password` | `App.tsx:444-448` |
| Profile picker | `/profiles` | `pages/Profiles` (`App.tsx:458`) |
| **Player** | `/watch/:id` (query: `fileId`, `libraryId`, `restart=1`, `room_id`, `room_token`, `shuffle`; `location.state`: `audioTrackIndex`, `prePlaySubtitleMode`, `prePlaySubtitleSelection`, `versionPicked`, `watchReturnHref`) | `pages/WatchRoute` (`App.tsx:467-474`; `pages/WatchRoute.tsx:17-55`) |
| E-book reader | `/reader/ebook/:contentId` | `App.tsx:475-482` |
| Admin | `/admin/*` (about 40 sub-routes) | `App.tsx:484+` |
| Settings | `/settings/*` (playback, interface, accessibility, profiles, subtitle-appearance, devices, connect-apps, ...) | `App.tsx:579-616` |
| Home | `/` | `Home` inside Onboarding/TasteSeed gates (`App.tsx:623-632`) |
| Browse / search | `/catalog` (search is `/catalog?...`; `/search` and `/browse` are legacy redirects) | `App.tsx:634-637`, `373-387` |
| Library | `/library/:libraryId` | `App.tsx:635` |
| Item details (movie, series, season, episode alike) | `/item/:id` | `pages/ItemDetail` (`App.tsx:638`); sub-views `MovieContent`, `SeasonContent`, `EpisodeContent`, ... in `pages/ItemDetail/` |
| Person, box set | `/person/:id`, `/box-sets/:id` | `App.tsx:639-640` |
| Watch together | `/rooms`, `/rooms/:roomId`, `/rooms/join` | `App.tsx:450, 641-642` |
| Other | `/collections*`, `/requests*`, `/title/:mediaType/:tmdbId`, `/recommendations*`, `/calendar`, `/notifications`, favorites/watchlist/history redirects | `App.tsx:643-732` |

There is no separate series or season route: everything is `/item/:id`. There is **no server-selection route**, because the app assumes it is served by the server it talks to.

## 3. State management and API client

- **Server state:** TanStack Query (`QueryClientProvider` in `App.tsx`, about 260 hook files under `src/hooks/queries/**`). App state uses React contexts: `AuthProvider`, `ThemeProvider`, `BrandingProvider`, `WatchPlaybackProvider`, `AudiobookPlaybackProvider`, `RealtimeEventsProvider` (`silo-server/web/src/App.tsx@6791aac:860-895`). Playback host state is a reducer (`src/playback/watchPlaybackReducer.ts`).
- **Typed v2 boundary:** `v2("GET /api/v2/progress", {...})` picks an operation by its literal `METHOD /path` string and infers types from the generated `paths` (`silo-server/web/src/api/v2/request.ts@6791aac:1-15`). `buildUrl` only substitutes path parameters into the route literal, so URLs are **root-relative, same-origin** (`request.ts:365-393`, `:460`). Every request also sends the headers `X-Silo-Client: Silo Web` and `X-Silo-Client-Version` (`request.ts:345-346`).
- **Session layer:** `fetchWithSession` in `src/api/client.ts` adds Authorization/profile/device headers, waits for the boot-time session restore, and refreshes then retries once on a 401. The single `fetch` call is `silo-server/web/src/api/client.ts@6791aac:646`. The refresh request goes to the relative URL `fetchImpl("/api/v2/auth/refresh", ...)` (`client.ts:557`).
- **Base URL:** this is **not configurable** for the main app. The only origin notion is `currentServerOrigin()` = `location.origin`, which fences stale requests (`client.ts:218-220`). The player module is the exception: it takes `PlayerConfig.apiBaseUrl`, which the host hard-codes to `"/api/v2"` (`silo-server/web/src/playback/WatchPlaybackChrome.tsx@6791aac:705-714`). The player strips `/api/vN` to derive its origin (`silo-server/web/src/player/player-v2.ts@6791aac:101-108`). A comment says "Older embedding hosts may still provide the bridge base URL", which suggests a past embedding use (**UNVERIFIED**).
- **Token storage:**
  - The access token is kept only in a module variable (`client.ts:103-117`).
  - The refresh token, profile id and device id are in **localStorage**, under the keys `refresh_token`, `profile_id`, `silo-device-id` and others (`silo-server/web/src/utils/storage.ts@6791aac:1-48`; `client.ts:119-129`, `302-311`).
  - Per-tab profile choices go to sessionStorage (`storage.ts:56-125`).
  - Normal API auth uses no cookies. The server sets HttpOnly cookies only for the plugin-launch flow (`silo_plugin_access`, `Path=/api/v1`, `silo-server/internal/api/handlers/auth.go@6791aac:493-500`; `internal/apiv2/plugin_launch.go:26`) and for OAuth browser binding (`internal/apiv2/oauth_handshakes.go:249`).
  - No `credentials:` option is set anywhere in non-test code (grep).
- **Device identity:** `detectDevicePlatform()` reports values like "Windows Web" from the UA (`client.ts:313-326`). The player's capability context reports `platform: "web"` (`silo-server/web/src/player/client-context-v3.ts@6791aac:282-299`).
- **WebSockets:** every socket first mints a ticket over HTTP, then connects with the ticket as a subprotocol.
  - Events: `buildEventsUrl` = `${protocol}//${location.host}/api/v2/events/ws` (`silo-server/web/src/components/RealtimeEventsProvider.tsx@6791aac:137`, `:1009`; ticket in `src/api/v2/eventsSocket.ts:14`).
  - Playback control: `playbackControlSocketURL(sessionId, origin = window.location.origin)` (`src/api/v2/playbackControlSocket.ts:44-69`). The player passes `playerV2Origin(config) || window.location.origin` (`src/player/hooks/usePlaybackRealtime.ts:83, 190`).
  - Watch together: `src/api/v2/watchTogetherSocket.ts:24-42`.
  - Admin logs: `src/api/v2/adminLogsSocket.ts:63-69`.
- **Login options:** password login, OAuth through a full-page redirect to `/api/v2/auth/oauth/{id}/start` (`silo-server/web/src/lib/externalSignIn.ts@6791aac:86-100`), and a **device-code flow** (`POST /api/v2/auth/device/start|poll`, `request.ts:40-50`, page `/activate`). The device-code flow is directly usable by a native client.

## 4. Theme, design tokens, i18n

- **Tailwind v4, CSS-first:** there is no `tailwind.config.*`. `src/app.css` starts with `@import "tailwindcss"` and maps semantic variables into the theme through `@theme inline { --color-background: var(--background); ... }` (`silo-server/web/src/app.css@6791aac:1-60`). shadcn is configured with style `new-york`, baseColor `zinc`, `cssVariables: true` and css `src/app.css` (`silo-server/web/components.json@6791aac`).
- **Single theme:** `index.html` has `<html lang="en" data-theme="midnight-cinema">` (`silo-server/web/index.html@6791aac:2`). `themeBoot.js` says "Silo has one base theme, fixed by index.html's data-theme" (`silo-server/web/src/themeBoot.js@6791aac:1-20`). The theme is dark only: there is no light theme and no `prefers-color-scheme` handling in `app.css` (grep). Admins can customize it (theme editor, accent colour, `src/lib/accentColor.ts`, `hooks/useTheme.tsx`). There is also a high-contrast mode driven by `--*-base` aliases (`app.css:496-517`).
- **Core `midnight-cinema` tokens** (`silo-server/web/src/app.css@6791aac:523-590`):
  - Shape and font: `--radius: 0.75rem`; `--font-body` and `--font-display` are both "Plus Jakarta Sans".
  - Base colours: `--background #141417`, `--foreground #e8e8ec`, `--card #1c1c20`, `--popover #18181c`.
  - Primary and secondary: `--primary #e8e8ec` with `--primary-foreground #141417`; `--secondary #232328`; `--muted-foreground-base #a4a4ae`.
  - Lines and states: `--border-base #28282e`, `--ring #e8e8ec`, `--destructive #ef4444`.
  - Sidebar: `--sidebar #0f0f12`.
  - Surfaces: `--surface-base #1c1c20`, `--surface-hover-base #232328`, `--surface-raised-base #28282e`.
  - Charts: `--chart-1..5`.
  - Status colours (oklch) are in `:root` (`app.css:487-494`).
- **Design doc:** `web/docs/design-system.md` (principles "Cinematic Darkness", surface/text hierarchy, typography) and `src/lib/design-system.ts` (601 lines).
- **Fonts:** Plus Jakarta Sans is a variable font (wght 200-800), self-hosted from `@fontsource-variable` and declared in `src/fonts.css` under the plain family name. Noto Sans Arabic and Thai are included for metadata text (`silo-server/web/src/fonts.css@6791aac:1-25`).
- **i18n: none.** No i18n library or locale files exist (grep for `i18n|useTranslation|locales` found only an unrelated error boundary). Labels are literal English, for example `playLabel = "Play"` and "Resume" in `src/pages/ItemDetail/components/ActionBar.tsx:303, 388`. **There is no German UI.** About 155 `Intl`/`toLocale*` uses do locale formatting only. A server-side "on-view translation" feature translates metadata descriptions, not the UI (`src/hooks/useOnViewTranslation.ts:39-45`).

## 5. How the server serves the UI

- `web/embed.go`: `//go:embed all:dist` → `var DistFS embed.FS` (`silo-server/web/embed.go@6791aac`).
- Root mux: `/metrics` → 404, `/api/` → API router, `/` → `server.FrontendHandler()` (`silo-server/cmd/silo/root_handler.go@6791aac:40-50`). The UI is mounted at `/` with **no base-path option** (none found).
- `frontendHandler.ServeHTTP` (`silo-server/internal/server/frontend.go@6791aac:115-198`):
  - Precompressed sidecars are never served directly. With branding enabled, `/site.webmanifest` and `/favicon.ico` are dynamic.
  - Existing files are served directly. `/assets/*` gets `max-age=31536000, immutable` plus br/gz negotiation. Other files get `no-cache` plus an ETag.
  - A missing `/assets/*` returns 404, not the shell.
  - Everything else falls back to the SPA: a **branded** `index.html`, rendered per request by `brandedShell` (`:352-377`), sent with `Content-Security-Policy` and `Cache-Control: no-cache`.
- **CSP** (on the HTML shell only, `frontend.go:53-65`): `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline' blob:; img-src 'self' blob: data: http: https:; font-src 'self' blob: data:; media-src 'self' blob: http: https:; connect-src 'self' ws: wss: http: https:; worker-src 'self' blob:; frame-src 'self' blob: https://www.youtube-nocookie.com; object-src 'none'; base-uri 'self'; form-action 'self'`. It has **no `frame-ancestors`**, and no `X-Frame-Options` was found in the Go code (grep). The CSP explicitly forbids inline scripts, but injected WebView2 initialization scripts are not subject to page CSP (**UNVERIFIED** for Tauri's `initialization_script` on remote pages).
- `X-Content-Type-Options: nosniff` is set on every response (`frontend.go:116`).

## 6. Player and its exact boundary

The call chain:

```
UI action (16 startPlayback call sites, or <Link>/navigate to "/watch/:id")
  -> WatchPlaybackProvider.startPlayback()       playback/WatchPlaybackChrome.tsx:339-405
       (normalizes, then navigate(buildWatchHref(request), {state}) at :379)
  -> route /watch/:id -> WatchRoute (controller only, renders null)   pages/WatchRoute.tsx:10-76
       -> controller.syncRouteRequest(request)   reducer SYNC_ROUTE_REQUEST (watchPlaybackReducer.ts:28, 87)
  -> WatchPlaybackHost (always mounted)           App.tsx:886; WatchPlaybackChrome.tsx:554-560
       -> WatchPlaybackHostContent: useWatchDetail() = GET /api/v2/watch/{id}   hooks/queries/items.ts:42, 53
       -> buildWatchPageProps(request, item, ...)  pages/watchRouteHelpers.ts:274-445
       -> <PlayerConfigProvider config={apiBaseUrl:"/api/v2", ...}>  WatchPlaybackChrome.tsx:705-714, 1233
       -> <WatchPage {...props}/> (lazy, :85-86)   WatchPlaybackChrome.tsx:1236-1258
            -> usePlaybackSession(...)              player/components/WatchPage.tsx:206
                 -> startPlaybackV2 = GET /api/v2/playback/capabilities + POST /api/v2/playback/start
                    player/hooks/usePlaybackSession.ts:891; player/start-v2.ts:120-175
            -> <VideoPlayer> (5,678 lines)          player/components/WatchPage.tsx:654
                 -> native <video>, hls.js (lazy import, VideoPlayer.tsx:182, 2752-2761),
                    or the browser decode engine (player/engine/*, mediabunny + WebCodecs/WebGPU + FFmpeg-wasm audio)
```

- **What the player needs** (`DerivedWatchPageProps`, `pages/watchRouteHelpers.ts:405-445`):
  - Identity and versions: `contentId`, `fileId?`, `versionPicked`, `libraryId?`, `versions`, `playbackVariants`.
  - Position: `initialPosition` (`restart ? 0 : item.user_data.position_seconds`, `:383`) and `forceInitialPosition`.
  - Quality: `qualityPreference`.
  - Audio and subtitles: `explicitAudioTrackIndex`, `initialSubtitleTrackIndexByFileId`, `initialBitmapSubtitleTrackIndexByFileId`, `preferredSubtitleLanguage`/`Signature`, `subtitleMode`, `showForcedSubtitles`, `subtitles`.
  - Markers: `intro`, `credits`, `recap`, `preview` and the skip settings.
  - Context: `seriesContext` (episodes for next/prev), `resumeHints`, `watchTogetherRoomId`/`Token`.
  - The host adds `maxBitrateKbps`, `bufferingMode`, callbacks (`onEnded`, `onExit`, `onMinimize`, `onNavigateEpisode`, `onPlaybackStateChange`, ...), `displayMode` and `seekIntervals` (`WatchPlaybackChrome.tsx:1236-1258`).
- **Stream acquisition:** this is protocol v3. `POST /api/v2/playback/start` returns a `playback_plan` whose `stream.url` the server has "fully anchored" (seek position, stream token). For API-relative paths the client only appends `?token=<access token>`, and absolute proxy-node URLs are left untouched (`silo-server/web/src/player/stream-url.ts@6791aac:30-76`). The start body includes client capabilities probed with `MediaSource.isTypeSupported`/`canPlayType` (`player/client-context-v3.ts:1-13, 125-299`). A native player would need its own, more capable declaration.
- **Lifecycle endpoints** (`src/api/v2/operations.ts:84-85, 468-474, 846-852`):
  - Session: `POST /api/v2/playback/{session_id}/progress`, `.../replan`, `DELETE /api/v2/playback/{session_id}` (stop).
  - Reporting: `POST /api/v2/playback/route-events`.
  - Transcode: `GET /api/v2/playback/transcode/{session_id}/master.m3u8`.
  - Control: the WS `GET /api/v2/playback/sessions/{session_id}/control/ws`.
- **Portability:** `player/index.ts` says the module "has ZERO imports from app-specific code" and exports `WatchPage`, `PlayerConfigProvider` and the types (`silo-server/web/src/player/index.ts@6791aac:1-30`). `PlayerConfig` is called "the portability contract" (`player/context/PlayerConfigContext.tsx:5-10`).
- **Audiobooks** use a separate player (`AudiobookPlaybackProvider`, `pages/audiobooks/player/useAudiobookPlayback.ts:598` also calls `startPlaybackV2`). This document does not cover audiobooks in detail.

## Native-player seam

### Every call site that starts video playback (non-test code)

These call `useWatchPlaybackController().startPlayback(...)`:

- `web/src/components/CardPlayOverlay.tsx:53` — poster card play button (built from `buildMediaPlayHref` at :31, parsed at :48)
- `web/src/components/ContinueWatchingCard.tsx:217` — Continue Watching (resume)
- `web/src/components/MediaItemMenu.tsx:1097` — context menu Play. The same menu's "play from start" instead calls `navigate(buildMediaPlayHref(...restart:true))` at `:1094`.
- `web/src/pages/ItemDetail/components/ActionBar.tsx:450` — item-detail Play/Resume from href (`startPlaybackFromHref`). This covers `playHref`/`restartHref` built in `MovieContent.tsx:222, 290`, `EpisodeContent.tsx:266, 365`, `SeasonContent.tsx:170`, `itemDetailLayout.ts:120`.
- `web/src/pages/ItemDetail/components/ActionBar.tsx:984` — Play with an explicitly selected version
- `web/src/pages/ItemDetail/components/ExtrasSection.tsx:41` — extras/trailers
- `web/src/hooks/queries/shuffles.ts:39` — shuffle start
- `web/src/playback/ShufflePlayingNext.tsx:88` — shuffle post-roll next
- `web/src/playback/WatchPlaybackChrome.tsx:884` — next episode (post-roll / auto-play)
- `web/src/playback/WatchPlaybackChrome.tsx:983` — next part of a multi-part file
- `web/src/player/components/WatchPage.tsx:424` — watch-together room changed its selection
- `web/src/pages/watchtogether/WatchTogetherRoomPage.tsx:178`, `:225` — room start/join
- `web/src/components/HeroBanner.tsx:335-350` — hero Play. It is a plain link to `playHref` (`/watch/...`). The click handler only calls `markPlaybackIntent` and then lets the link navigate, so it belongs with the direct-link entries below.

These navigate straight to `/watch/:id` without calling `startPlayback`:

- `web/src/pages/BoxSetPage.tsx:88` (`<ViewTransitionLink to={buildMediaPlayHref(...)}>`)
- `web/src/components/profiles/HouseholdStreamsPanel.tsx:52` (`buildWatchHref`)
- Any external deep link or browser URL to `/watch/...`

All the hrefs come from `lib/mediaNavigation.ts:46-70` (`buildMediaPlayHref`) and `pages/watchRouteHelpers.ts:143-160` (`buildWatchHref`/`parseWatchHref`).

### Candidate seams, smallest robust one first

1. **Recommended: `WatchPlaybackHostContent` → `<WatchPage>`** (`web/src/playback/WatchPlaybackChrome.tsx:1236`; lazy import `:85-86`; config `:705-714`).
   - Every entry above, including direct links (HeroBanner, BoxSetPage, HouseholdStreamsPanel) and automatic starts, passes through here, and at this point the props are fully resolved: version/file, resume position, audio and subtitle choices, markers, next-episode context.
   - A desktop adapter can replace `WatchPage` with a `NativeWatchPage` that has the same props. It would `invoke('play', props)` into Rust/libmpv and translate mpv events back into the host callbacks (`onEnded`, `onExit`, `onPlaybackStateChange`, `onNavigateEpisode`). Post-roll, next-episode and the background bar keep working through the existing reducer.
   - The swap can be done with a Vite `resolve.alias` for `@/player/components/WatchPage` to keep the upstream patch tiny.
   - The native side then owns the v3 start/progress/stop protocol (`player/start-v2.ts`, `player/hooks/usePlaybackSession.ts`, `player/session-mutations.ts`). Those files can be ported, or reused from JS while mpv only renders.
2. **The route controller `WatchRoute`** (`web/src/pages/WatchRoute.tsx:17-55`) or reducer `SYNC_ROUTE_REQUEST` (`playback/watchPlaybackReducer.ts:87`). Every entry converges here too, but the request is still raw (contentId, fileId, restart, audioTrackIndex, subtitle selection). The adapter would have to repeat `useWatchDetail` and `buildWatchPageProps` to get the resume position and stream choices.
3. **`WatchPlaybackProvider.startPlayback`** (`WatchPlaybackChrome.tsx:339-405`). It looks like a single function but **misses** BoxSetPage, HouseholdStreamsPanel, `MediaItemMenu:1094` and deep links. Not sufficient on its own.
4. **Inside the player at `startPlaybackV2`** (`player/start-v2.ts:120`) or `VideoPlayer` media attach. This sits too deep: the 5.7k-line `VideoPlayer` and the decode engine would still mount and own the UI.

For option A (no source patch), the only practical runtime hooks are history/route interception of `/watch/` (the `history.pushState` URL plus `history.state.usr` for `audioTrackIndex`) together with blocking the web player. That means overriding `fetch` for `/api/v2/playback/start` or stubbing `HTMLMediaElement`. Both are fragile; see the implications below.

## 7. Same-origin assumptions that break under a custom scheme talking to a remote server

Assume the vendored UI is served from `http://tauri.localhost` (the Windows default; `tauri://localhost` elsewhere) and calls `https://silo.example`:

| Concern | Where | Breaks? |
|---|---|---|
| Relative API URLs | `api/v2/request.ts:365-393, 460`; refresh `api/client.ts:557`; OAuth start `lib/externalSignIn.ts:86-95` | Yes. Requests go to the app's own origin. They need rewriting to an absolute base, or a Tauri custom-protocol or Rust proxy. |
| CORS | No CORS middleware on `/api/` (grep of `internal/api/router.go`, `internal/apiv2`). CORS exists only on `internal/jellycompat/router.go:51`, `internal/proxy/server.go:259`, and `Access-Control-Allow-Origin: *` on `internal/api/handlers/stream.go:797` and `stream_subtitle_offset.go:23`. | Yes. Requests carrying `Authorization` are preflighted and the preflight fails. The webview `fetch` must be routed through Rust (Tauri HTTP plugin or a local proxy protocol), or an upstream CORS change is needed. |
| Cookies / `credentials` | Not used for API auth (grep). Cookies exist only for plugin-launch (`auth.go:493-500`) and OAuth binding (`oauth_handshakes.go:249`). | Mostly no. OAuth/plugin redirect flows would break (full-page navigation to the server origin). |
| Token storage | `localStorage` scoped to the webview origin (`utils/storage.ts:32-48`) | Works, but it is bound to the app origin, not the server. Multi-server would need namespacing. Better to keep the refresh token in the Windows Credential Manager through an adapter. |
| Image URLs | The server signs artwork as root-relative `/api/v2/artwork/<key>?exp=&sig=` (`internal/artworkurl/artworkurl.go:147-148`). Used directly as `<img src>` (for example `components/HeroBanner.tsx:121-141`). | Yes. Root-relative image URLs resolve against the app origin. They need a rewrite or proxy. Absolute TMDB/S3 URLs still work. |
| WebSocket URLs | Built from `location.host`/`location.origin` (`RealtimeEventsProvider.tsx:137`, `playbackControlSocket.ts:64`, `watchTogetherSocket.ts:39-42`, `adminLogsSocket.ts:63-69`) | Yes, they point at the app origin. |
| WebSocket Origin check | `socketOriginAllowed`: no Origin → allow; an opaque or non-http(s) scheme ("Packaged TV and desktop apps load their pages from the device") → allow; http(s) only if it matches the public origin, the request host, or overlay origins (`internal/api/handlers/events_socket_v2.go:239-286`; v1 `ws_shared.go:35`) | `tauri://localhost` passes. **`http://tauri.localhost` fails.** A Rust-side WS (no Origin) passes. |
| Service worker | `/sw.js`, used only for Web Push (`public/sw.js`; `lib/webPush.ts:93`) | Push would not work. Not needed on desktop. |
| CSP | Sent by the server only on the shell (`frontend.go:188`). | For B the app sets its own CSP (Tauri config). It must allow the server origin in `connect-src`/`img-src`/`media-src` and keep `'wasm-unsafe-eval'` if JASSUB or the reader is kept. |
| `location.origin` as the server URL | `pages/setup-wizard/steps/ServerStep.tsx:15`, `pages/settings/ConnectAppsSettings.tsx:292`, invite/watch-party deep links (`pages/InviteClaim.tsx:139`, `lib/watchTogether.ts:273`), `api/v2/webhookSync.ts:11-42`, `pages/admin/autoscan/webhookURL.ts:17`, `pages/settings/*:258/494` | Yes. These would show or emit `http://tauri.localhost` as the server URL. |
| Stale-request fence | `currentServerOrigin()` = `location.origin` (`api/client.ts:218-220`) | Harmless, but it would never change on a server switch, so a server switch must force a reload or reset. |
| Branding | The server injects branding into `index.html` (`frontend.go:352-377`). | A locally served shell has no branding. `BrandingProvider` fetches some of it through the API (**UNVERIFIED** how much). |

## 8. Size, churn, embedding hooks

- **Size:** 2,359 files in `web/src`. 1,475 are non-test `.ts/.tsx` files (~469,578 lines) and 861 are tests. The largest directories are `lib` (187), `pages` (179), `components` (178), `api/v2` (162) and `hooks/queries` (140 + 121 admin). `VideoPlayer.tsx` alone is 5,678 lines and `app.css` 2,640 lines.
- **Churn:** the clone is shallow (`git rev-parse --is-shallow-repository` = true). The HEAD commit is PR **#3594**, dated 2026-11-05, and `web/package.json` is still at `version: 0.0.1`. Much of the codebase carries version suffixes and compatibility layers (`protocol-v3`, `start-v2`, "server below revision 20" gates in `WatchPlaybackChrome.tsx:591-640`), which points to fast-moving server/client contracts. The exact commit rate is **UNVERIFIED** (no history).
- **Embedding hooks:** **none** for a native shell. There are no `window.chrome.webview`, `__TAURI__`, `messageHandlers` or `ReactNativeWebView` references (grep). Related facts:
  - The `PlayerConfig` portability contract exists (above).
  - Native apps are known through `silo://` deep links (`lib/appDeepLink.ts:1-15`, Android app `silo-android`) and device-code login.
  - The server already accepts opaque/app-scheme WebSocket origins for "Packaged TV and desktop apps" (`events_socket_v2.go:252-262`).
  - `TRADEMARK.md` mentions an official "SiloMac" product name (`silo-server/TRADEMARK.md@6791aac:20`).
  - The server also hosts a Jellyfin-compatible API with CORS (`internal/jellycompat/router.go:51`), and the Dockerfile builds jellyfin-web. That is an alternative surface, not studied here.
- **License and branding:** `web/` is `AGPL-3.0-or-later` (`web/package.json`), with a "Platform Library" exception (`silo-server/PLATFORM-EXCEPTION.md@6791aac`). `TRADEMARK.md` forbids distributing a **modified version under the Silo name** and limits use of the logo (`TRADEMARK.md:28-70`). Vendoring (B) makes Granary's bundled UI AGPL, so its source must be offered, and Silo wordmark assets (`public/silo-wordmark-sidebar.png`, icons) must be removed or replaced. The legal reading is **UNVERIFIED**; this is not legal advice.

## Implications for A/B/C

### A — load the server's own UI remotely and inject a bridge

- **Pros:**
  - Zero maintenance of UI code, always in sync with the server, and full feature parity (admin, requests, settings).
  - Same-origin, so auth, CORS, images and WebSockets all just work.
  - No AGPL code bundled in Granary.
- **Cons and risks:**
  - There is no hook point to call into. Intercepting playback means monkeypatching `history.pushState`/`popstate` for `/watch/` (reading `history.state.usr` for `audioTrackIndex`/subtitle selection), suppressing the web player (which also POSTs `/playback/start`, opens a session and starts hls.js), and reimplementing `buildWatchPageProps` logic (resume position, version choice) natively. Each upstream release can silently break this.
  - Tauri 2 IPC from a remote origin needs a `remote` capability scope. Exposing IPC to whatever page the server returns is a security exposure (the server is user-controlled but remote).
  - The mpv window under a transparent overlay needs our own controls. The web player chrome cannot be reused because its video is not mpv.
- **Effort:** low to start, medium/high to make robust. **Risk:** high (fragile and silent breakage).

### B — vendor `web/` at a pinned commit and add a small desktop adapter

What the adapter must patch, with minimal-diff strategies:

1. **Base URL:**
   - Wrap `fetch` at the single site `api/client.ts:646` (plus `client.ts:557` refresh, `player/player-fetch.ts:151` and `start-v2.ts` through `PlayerConfig.apiBaseUrl` at `WatchPlaybackChrome.tsx:707`).
   - Simplest is to route everything through a Tauri custom protocol or local proxy that forwards `/api/*` to the selected server. That avoids CORS entirely and fixes root-relative artwork URLs and the OAuth start link.
   - Without a proxy you must rewrite URLs **and** solve CORS (Rust-side HTTP), because the server has no CORS on `/api/v2`.
2. **WebSockets:** rewrite the 4 URL builders (section 3) to the server origin. Either serve the app from a non-http scheme, or proxy WS through Rust, because `http://tauri.localhost` fails the Origin check.
3. **Auth storage:** `utils/storage.ts` is one choke point (localStorage get/set). Redirect `refresh_token` to the OS credential store through `invoke`, or accept localStorage. Add a server-selection screen in front of `/login`, which upstream lacks, and namespace storage per server.
4. **Player hand-off:**
   - Alias `@/player/components/WatchPage` to a native implementation (seam 1), and set `X-Silo-Client` (`request.ts:345`) to Granary.
   - Disable PiP and the background `WatchPlaybackBar` paths, or map them to mpv window states.
   - Send a desktop capability declaration (platform `windows-desktop`, mpv codecs) instead of `client-context-v3.ts` browser probes.
5. **Remove the browser `<video>` path:** with the alias, `VideoPlayer`, hls.js, the decode engine, JASSUB and the wasm decoders become unreachable and tree-shake away (**UNVERIFIED**; check bundle output).
6. **Origin-as-server-URL spots** (section 7) need a `getServerOrigin()` replacement. That is about 12 files, all cosmetic or admin.
7. **Build:** pin Node 24 and pnpm 10.32.1. Vendor `contracts/api/v2/fixtures` and `docs/design/schemas`, or build with `vite build` and skip the typecheck. Strip Silo trademark assets. Comply with AGPL.

- **Effort:** medium up front (about 1-2 weeks for adapter, proxy and native WatchPage; **UNVERIFIED** estimate). **Ongoing:** the pinned UI must be compatible with each server version. The v2 contract and settings capability gates suggest decent backward compatibility, but very high churn makes rebases frequent.
- **Risk:** medium. The patch surface is small and well-localized (fetch, storage, 4 WS builders, 1 component alias), but there is AGPL/branding obligation and contract drift.

### C — our own UI reusing Silo design tokens

- **Pros:**
  - Clean desktop architecture: Rust owns HTTP, WS and auth (no CORS or Origin issues), and the native player is first-class.
  - German i18n from day one, since upstream has none.
  - Only the routes we need (home, library, catalog/search, item, season/episode, settings, login with device-code and server selection).
  - Tokens are easy to replicate: CSS variables in `app.css:523-590`, Tailwind 4 `@theme inline` mapping, Plus Jakarta Sans and shadcn new-york/zinc.
- **Cons:**
  - We must re-implement version/resume/subtitle-selection logic (`watchRouteHelpers.ts:274-445`) and the v3 playback protocol, but that logic is needed in A and B anyway for the native player.
  - Feature parity (admin, requests, watch-together) is out of reach, but users can open the server's web UI in a browser for admin.
  - Copying `app.css` verbatim would bring AGPL in. Re-expressing token values (colours, radius, font) is likely fine (**UNVERIFIED**, legal).
- **Effort:** high initially. **Risk:** low technical risk and the lowest upstream coupling.

### Hybrid note

B's adapter work and C's native player share the same core: Rust HTTP/WS proxy, credential storage, and the v3 start/progress/stop client built from `player/start-v2.ts` and `player/hooks/usePlaybackSession.ts`. Building that core first keeps both options open.

## Sources

Upstream (all `@6791aac`, i.e. 6791aac867664c41c55ee82909fe063d145911a6):

Web app root and build:
- `silo-server/web/package.json`, `silo-server/web/pnpm-lock.yaml`, `silo-server/web/vite.config.ts`, `silo-server/web/tsconfig.app.json`, `silo-server/web/components.json`, `silo-server/web/index.html`, `silo-server/web/embed.go`
- `silo-server/web/scripts/typecheck.mjs`, `silo-server/web/scripts/generate-apiv2.mjs`
- `silo-server/web/wasm/audio-decoders/README.md`, `silo-server/web/vendor/foliate-js/`, `silo-server/web/public/sw.js`, `silo-server/web/docs/design-system.md`

Web app source:
- `silo-server/web/src/App.tsx`, `silo-server/web/src/main.tsx`, `silo-server/web/src/app.css`, `silo-server/web/src/fonts.css`, `silo-server/web/src/themeBoot.js`
- `silo-server/web/src/api/client.ts`, `silo-server/web/src/api/v2/request.ts`, `silo-server/web/src/api/v2/operations.ts`, `silo-server/web/src/api/v2/eventsSocket.ts`, `silo-server/web/src/api/v2/playbackControlSocket.ts`, `silo-server/web/src/api/v2/watchTogetherSocket.ts`, `silo-server/web/src/api/v2/adminLogsSocket.ts`, `silo-server/web/src/utils/storage.ts`
- `silo-server/web/src/components/RealtimeEventsProvider.tsx`, `silo-server/web/src/components/HeroBanner.tsx`, `silo-server/web/src/components/CardPlayOverlay.tsx`, `silo-server/web/src/components/ContinueWatchingCard.tsx`, `silo-server/web/src/components/MediaItemMenu.tsx`, `silo-server/web/src/components/profiles/HouseholdStreamsPanel.tsx`
- `silo-server/web/src/pages/WatchRoute.tsx`, `silo-server/web/src/pages/watchRouteHelpers.ts`, `silo-server/web/src/pages/BoxSetPage.tsx`, `silo-server/web/src/pages/ItemDetail/components/ActionBar.tsx`, `silo-server/web/src/pages/ItemDetail/components/ExtrasSection.tsx`, `silo-server/web/src/pages/watchtogether/WatchTogetherRoomPage.tsx`
- `silo-server/web/src/playback/WatchPlaybackChrome.tsx`, `silo-server/web/src/playback/watchPlaybackContext.ts`, `silo-server/web/src/playback/watchPlaybackReducer.ts`, `silo-server/web/src/playback/ShufflePlayingNext.tsx`
- `silo-server/web/src/player/index.ts`, `silo-server/web/src/player/context/PlayerConfigContext.tsx`, `silo-server/web/src/player/components/WatchPage.tsx`, `silo-server/web/src/player/components/VideoPlayer.tsx`, `silo-server/web/src/player/start-v2.ts`, `silo-server/web/src/player/player-v2.ts`, `silo-server/web/src/player/player-fetch.ts`, `silo-server/web/src/player/stream-url.ts`, `silo-server/web/src/player/client-context-v3.ts`, `silo-server/web/src/player/hooks/usePlaybackSession.ts`, `silo-server/web/src/player/hooks/usePlaybackRealtime.ts`
- `silo-server/web/src/lib/mediaNavigation.ts`, `silo-server/web/src/lib/externalSignIn.ts`, `silo-server/web/src/lib/appDeepLink.ts`, `silo-server/web/src/lib/buildPluginHref.ts`, `silo-server/web/src/lib/webPush.ts`, `silo-server/web/src/hooks/queries/items.ts`, `silo-server/web/src/hooks/queries/shuffles.ts`

Server (Go):
- `silo-server/internal/server/frontend.go`, `silo-server/cmd/silo/root_handler.go`
- `silo-server/internal/api/handlers/events_socket_v2.go`, `silo-server/internal/api/handlers/ws_shared.go`, `silo-server/internal/api/handlers/auth.go`, `silo-server/internal/api/handlers/stream.go`, `silo-server/internal/api/handlers/stream_subtitle_offset.go`
- `silo-server/internal/apiv2/plugin_launch.go`, `silo-server/internal/apiv2/oauth_handshakes.go`, `silo-server/internal/artworkurl/artworkurl.go`
- `silo-server/internal/jellycompat/router.go`, `silo-server/internal/proxy/server.go`

Repository root:
- `silo-server/Dockerfile`, `silo-server/Makefile`, `silo-server/DEVELOPMENT.md`, `silo-server/PLATFORM-EXCEPTION.md`, `silo-server/TRADEMARK.md`

External (general knowledge, not verified in this session): Tauri 2 on Windows serves app assets from `http://tauri.localhost` by default (`https://` with `useHttpsScheme`), and from `tauri://localhost` on macOS/Linux. See https://v2.tauri.app/ (**UNVERIFIED** for the exact current default).
