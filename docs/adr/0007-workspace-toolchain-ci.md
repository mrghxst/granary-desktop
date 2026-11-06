# ADR-0007: Workspace, frontend toolchain and CI

- Status: Accepted
- Date: 2026-11-06
- Inputs: prompt PART B Phase 3; ADR-0001 (UI option C, own React UI), ADR-0003, ADR-0005; versions looked up with
  `npm view <pkg> version` / `cargo search` / `gh api repos/<action>/releases/latest` on 2026-11-06.

## Context
- Phase 3 needs a pnpm + Cargo workspace, a Tauri 2 app, least-privilege capabilities, strict CSP, rotated and
  redacted logs, single-instance/window-state, CI on `windows-latest` and a tag-triggered draft-release workflow.
- TypeScript 7.0.2 (native compiler) is the newest release, but `typescript-eslint` 8.72.1 declares
  `typescript >=4.8.4 <6.1.0` as its peer range; type-aware linting is part of the quality bar.
- Private-repo CI minutes are budgeted (ADR-0005).

## Decision
- **Workspaces**: root `Cargo.toml` (resolver 3, edition 2024, shared deps/lints; members `apps/desktop/src-tauri`,
  `crates/{player,display,silo-api}`; `spikes/` excluded) and `pnpm-workspace.yaml` (`apps/*`, `packages/*`).
  Rust toolchain pinned to 1.99.1 in `rust-toolchain.toml` so clippy `-D warnings` cannot drift between machines.
- **Frontend**: React 19.3.1, react-router 8.5.1 (HashRouter: no server-side routing inside Tauri), Vite 8.3.6,
  Tailwind CSS 4.3.4 (`@tailwindcss/vite`, tokens via `@theme inline`), i18next 26.7.2 / react-i18next 17.1.13 (en,
  de), Vitest 5.1.0 + jsdom 31 + Testing Library, ESLint 10.14 + typescript-eslint 8.72.1 (strict) + react-hooks,
  Prettier 3.9.10. **TypeScript 6.0.3** (latest within typescript-eslint's peer range) instead of 7.0.2; revisit when
  typescript-eslint supports TS 7.
- **pnpm minimum-release-age**: pnpm 12 refuses packages younger than its default minimum release age; i18next 26.7.2
  was excluded explicitly (`minimumReleaseAgeExclude` in `pnpm-workspace.yaml`) because it is the version current at
  time of use. Further exclusions need a log line in PROGRESS.md.
- **Host** (`apps/desktop/src-tauri`, package `granary`, binary `granary.exe`): startup order WebView2 transparent
  background env → logging → display repair from restore marker → libmpv probe → Tauri. Logs: daily rotation, 7 files,
  `%LOCALAPPDATA%\Granary\logs`, every line passed through `redact()` (bearer/refresh/profile tokens, passwords, PINs,
  API keys, bare JWTs). Config root `%APPDATA%\Granary`.
- **Least privilege**: `tauri_build::AppManifest` lists the app's own commands; `capabilities/main.json` grants only
  `core:event:default` and `allow-app-info` to window `main`. Plugins used Rust-side only (single-instance,
  window-state) get no WebView permissions. CSP: `default-src 'self'`, `script-src 'self'`, no remote scripts,
  `object-src 'none'`, `frame-ancestors 'none'`; `img-src` allows http/https for server artwork (Phase 5);
  `freezePrototype` on.
- **libmpv linking**: `/DELAYLOAD:libmpv-2.dll` only on the `granary` bin (its test harness is disabled; lib tests
  dead-strip mpv imports); `build.rs` copies the pinned DLL next to the dev exe; the bundle ships it as a resource in
  the install directory.
- **CI**: `ci.yml` on every push: pinned toolchain, `Swatinem/rust-cache@v2`, `pnpm/action-setup@v6`,
  `actions/setup-node@v7` (pnpm cache), `actions/cache@v6` for `vendor-bin/libmpv` keyed on the pin file, then
  `scripts/verify.ps1 -Install -SkipFetch` (same script as local). `permissions: contents: read`; a read-only token
  cannot list draft releases, so CI downloads from the upstream URL (SHA-256 verified) and caches it.
  `release.yml` on `v*` tags: `scripts/release-build.ps1` → NSIS → attached to a **draft** release
  (`contents: write`, which also lets it read the private libmpv mirror).
- Actions are referenced by major tag (checkout v7, setup-node v7, pnpm/action-setup v6, cache v6, rust-cache v2).

## Consequences
- One verify script for local and CI; a gate is green only if both are.
- When shinchiro prunes the pinned release, CI keeps working from its cache; a cold cache then needs the mirror
  (release workflow) or a pin bump.
- TS 6 → 7 migration is deferred, tracked in PROGRESS.md Known issues.
