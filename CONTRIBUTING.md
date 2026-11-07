# Contributing to Granary

Thanks for helping. Granary is a small project with a few firm rules. They exist because the app changes the user's
display settings, holds their server credentials, and talks to an API it does not control.

## Setup

See "Build from source" in [`README.md`](README.md). In short, from PowerShell in the repository root:

```powershell
corepack enable
pnpm install --frozen-lockfile
./scripts/fetch-libmpv.ps1
./scripts/verify.ps1
pnpm --filter desktop tauri dev
```

You can develop without a Silo server. Set `$env:GRANARY_MOCK_SERVER = "1"` before `tauri dev`, sign in as
`alice` / `pw`, and use Kids PIN `1234`. Run `./scripts/gen-test-media.ps1` once to get real clips to play. The mock
works in debug builds only.

Logs are in `%LOCALAPPDATA%\Granary\logs`. Raise the log detail with `$env:GRANARY_LOG = "debug"` (EnvFilter syntax).

## Rules

1. **Never invent Silo API behaviour.**
   - Every endpoint and field Granary uses is documented in
     [`docs/research/silo-api.md`](docs/research/silo-api.md) with the upstream file path and commit.
   - To use something new, add it there first, with its citation.
   - If it cannot be confirmed upstream, mark it `UNVERIFIED` and keep it behind an interface.
2. **Display changes are always restored.** Any HDR or refresh change goes through `crates/display` `session`, which
   writes the restore marker *before* the change. Never change display state anywhere else.
3. **Tokens stay in Rust.**
   - The UI receives views without secrets.
   - Never log a token, password, PIN, signed URL or ticket. Test new log sites against `redact.rs`.
   - `.env.local` is never committed.
4. **Least privilege.** A new Tauri command needs all four of these, or `ipc-contract.test.ts` fails:
   - `#[tauri::command]` in the host;
   - an entry in `generate_handler!` (lib.rs);
   - an entry in `COMMANDS` (build.rs);
   - `allow-<name>` in `capabilities/main.json`.

   Do not add core or plugin permissions without an ADR.
5. **No browser playback.** Media plays only through libmpv. `no-video.test.ts` enforces this.
6. **Trademark.** "Silo" never appears in the app name, binary, window title, icon or installer. Only "compatible
   with Silo" is allowed, in the README and the About screen.
7. **Licensing.**
   - Granary is AGPL-3.0-or-later.
   - When a bundled binary changes (libmpv/FFmpeg), update `THIRD_PARTY_NOTICES.md` and `scripts/libmpv.pin.json`.
8. **Decisions.** Significant choices (a dependency, a protocol, a policy) get an ADR in `docs/adr/`, or an amendment
   to an existing one.

## Code style

**Rust**
- Edition 2024.
- `cargo fmt`, and `clippy -D warnings` using the workspace lints.
- Errors use `thiserror`; logs use `tracing`.
- No `unwrap()` or `expect()` outside tests without a justification comment. Test modules use
  `#[allow(clippy::unwrap_used)] // test code`.
- `unsafe` blocks need a `// SAFETY:` comment.

**TypeScript**
- Strict mode with `exactOptionalPropertyTypes`.
- ESLint (typescript-eslint strict, react-hooks, jsx-a11y) with zero warnings, and Prettier.
- Every user-visible string goes through i18next, in both `en.json` and `de.json`.

**General**
- No `TODO` without a matching entry in `PROGRESS.md`.

## Checks

`./scripts/verify.ps1` must be green before every commit that closes a piece of work. It runs these steps, which are
the same as CI:

- libmpv pin;
- version sync;
- rustfmt, clippy, cargo test;
- TypeScript typecheck, ESLint, Prettier, vitest;
- frontend build;
- secret scan.

Changes to playback, display or input also need a run in the real app. Where a person, real HDR panels, a receiver or
a real server are needed, add a step to [`MANUAL_TEST_CHECKLIST.md`](MANUAL_TEST_CHECKLIST.md).

## Commits and releases

**Commits**
- Use Conventional Commits (`feat(player): …`, `fix(display): …`, `docs: …`).
- Keep each commit to one logical change.
- Run `scripts/secret-scan.ps1` before pushing. `verify.ps1` includes it.

**Releases**
- Bump the version in `apps/desktop/package.json` and the workspace `Cargo.toml`. `verify.ps1` checks that they
  match.
- Update `CHANGELOG.md`, then tag `vX.Y.Z`.
- `release.yml` builds the installer and attaches it to a **draft** release. Publishing is a manual decision.
