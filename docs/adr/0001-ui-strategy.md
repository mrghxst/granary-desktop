# ADR-0001: UI strategy

- Status: Accepted
- Date: 2026-11-06
- Inputs: [silo-web-ui.md](../research/silo-web-ui.md) (R2), [silo-api.md](../research/silo-api.md) (R1),
  [packaging-and-licensing.md](../research/packaging-and-licensing.md) (R5)

## Context
Granary must look and behave like Silo's own web app, hand all playback to a native mpv player, support
**en and de**, contain no Silo branding in name/title/icon/installer, and expose minimal remote-content/IPC surface.
Facts from research (upstream `silo-server@6791aac`):

- `web/` is React 19.2 + React Router 8.3 + TanStack Query 5 + Vite 6 + Tailwind 4/shadcn; **2,359 files,
  ~470k non-test LOC**, very high churn (HEAD is PR #3594).
- **No i18n framework at all**; all copy is hard-coded English. ~256 user-facing string literals contain "Silo".
- Brand assets in `web/public` (`silo-icon-1024.png`, `silo-wordmark-sidebar.png`, favicons) are all-rights-reserved.
- Strictly **same-origin**: root-relative API and artwork URLs, WebSocket URL from `location.host`, no base-URL
  setting, no server picker. `/api/v2` sends no CORS headers; the WS Origin check rejects `http://tauri.localhost`
  (Tauri 2's Windows default origin).
- `pnpm build` typechecks against `contracts/` and `docs/design/schemas` outside `web/`.
- The theme is one dark theme, `midnight-cinema`, defined as CSS variables (`web/src/app.css:523-590`), font
  Plus Jakarta Sans (OFL, via @fontsource). The player seam is `WatchPage` in `WatchPlaybackChrome.tsx:1236`.
- Upstream is AGPL-3.0-or-later, so copying code/tokens is license-compatible with attribution.

## Options and scoring (1 = poor, 5 = best)

| Criterion | A: remote server UI + injected bridge | B: vendor `web/` + desktop adapter | C: own UI with Silo design tokens |
|---|---|---|---|
| Visual parity | 5 (it *is* the web UI) | 5 | 3–4 (same tokens, font, layout patterns for the core screens) |
| Robustness to upstream churn | 1 (any deploy can break the bridge; version skew per server) | 2 (rebase 470k LOC + patches regularly) | 4 (depends only on the documented `/api/v2` contract) |
| Security (remote content + IPC) | 1 (remote server pages get IPC; Tauri remote-domain capabilities needed) | 3 (local code, but a huge surface incl. admin, plugins, CodeMirror) | 5 (small local bundle, least-privilege commands) |
| CORS / same-origin (API, images, WS) | 3 (same-origin works, but bridge must reach the native side) | 2 (needs custom-scheme `/api` proxy **and** patches to 4 WS builders + ~12 `location.origin` uses) | 4 (Rust-side HTTP/WS per ADR-0004; images via a custom-scheme proxy — no CORS involved) |
| en/de requirement | 1 (impossible) | 1 (retrofitting i18n onto ~470k LOC is not maintainable as a patch set) | 5 (i18n from day one) |
| Trademark hygiene | 1 (server UI shows Silo brand) | 2 (256 strings + assets to patch, re-patched on every sync) | 5 |
| Effort to first slice | 4 | 2 | 3 |

## Decision
**Option C: build Granary's own React/TypeScript UI that reuses Silo's design tokens** — the `midnight-cinema`
CSS variables (copied with attribution into `apps/desktop/src/styles/tokens.css`), Plus Jakarta Sans via
`@fontsource-variable/plus-jakarta-sans`, Tailwind 4 + Radix primitives in the same shadcn idiom, and the same
layout patterns (sidebar, hero, poster rows, details page) for the screens Granary needs: home rows, libraries,
search, item and series details, settings, plus the player overlay. Data comes only from `/api/v2` endpoints
documented in `silo-api.md`, through the Rust client (ADR-0004).

The prompt asked us to lean towards B unless A is clearly simpler and equally safe. Neither A nor B can satisfy the
en/de requirement or trademark hygiene without a permanent large patch set, and B's same-origin assumptions require
both a proxy *and* source patches. C is the option best supported by the evidence.

## Consequences
- Parity is "same design language", not pixel identity; admin/server-management screens are out of scope (users use
  the server's web UI for admin). Visual comparison against a live Silo web UI goes into `MANUAL_TEST_CHECKLIST.md`
  until a test server is available.
- We own UI maintenance, but only for ~10 screens; upstream churn affects us only through the API contract.
- The R2 seam (`WatchPage` props: content/file id, resume position, audio/subtitle index) defines our
  `PlayRequest` shape, so a future move to B remains possible.
- `THIRD_PARTY_NOTICES.md` credits the copied tokens (silo-server, AGPL-3.0-or-later) and the font (OFL-1.1).
- No Silo assets are copied; Granary ships an original icon.
