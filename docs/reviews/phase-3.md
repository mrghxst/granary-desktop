# Review: Phase 3 scaffold, CI, project skills

- Date: 2026-11-06
- Scope: commit `f84b6b4` (`git diff 29586a4..f84b6b4`): root `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`,
  `package.json`, `pnpm-workspace.yaml`, `.gitignore`, `crates/{player,display,silo-api}/**`, `apps/desktop/**`
  (frontend, configs, `src-tauri/**` incl. `tauri.conf.json`, `capabilities/main.json`, `build.rs`, `src/*.rs`),
  `scripts/{verify,release-build}.ps1`, `scripts/fetch-libmpv.ps1` + `libmpv.pin.json` changes,
  `.github/workflows/{ci,release}.yml`, `.claude/skills/{verify,release-build}/SKILL.md`, `THIRD_PARTY_NOTICES.md`,
  `docs/adr/0007-workspace-toolchain-ci.md`. Checked against `prompt.md` PART B (Non-negotiable rules, Phase 3, with
  Phases 4–9 in view), `CLAUDE.md`, ADR-0001/0005/0006/0007, `docs/research/{silo-api,packaging-and-licensing,windows-display-audio}.md`.
- Reviewer: fresh subagent (independent; did not write this code)

**Verdict: FAIL.** The scaffold itself is solid. Workspaces, crate split, delay-loaded libmpv with a probe,
least-privilege capability, strict CSP, a single verify script shared by local runs and CI, and the draft-release
workflow are all in place, and every Rust/TS check I ran is green. Four High findings block the gate, though, and
all four are cheap to fix:
- the startup display repair runs before the single-instance check, so a second launch reverts a running instance's
  HDR/refresh state (H1);
- the log redactor panics on ordinary non-ASCII input (H2);
- the redactor leaks secrets in common `Debug` shapes (H3);
- the debug stderr log layer bypasses redaction entirely because of ANSI colour codes (H4).

The verdict becomes PASS WITH FINDINGS once H1–H4 are fixed and covered by tests.

## Phase 3 requirement coverage
| Requirement | Status | Evidence |
|---|---|---|
| Layout `apps/desktop`, `crates/{player,display,silo-api}`, `scripts/`, `docs/`, `.claude/skills/` | Met | Tree matches the prompt layout adapted to ADR-0001 C / ADR-0004. `packages/` is not needed (no TS API client); `.claude/skills/fetch-libmpv` predates this phase |
| pnpm workspace + Cargo workspace | Met | `pnpm-workspace.yaml` (`apps/*`, `packages/*`); root `Cargo.toml` resolver 3, 4 members, `spikes` excluded |
| Rust 2024 edition | Met | `Cargo.toml:9` `edition = "2024"`, toolchain pinned 1.99.1 (`rust-toolchain.toml`) |
| `tracing` logs with rotation in `%LOCALAPPDATA%\Granary\logs`, tokens redacted | Partial | `logging.rs` daily rotation, 7 files, `paths::log_dir()`. Redaction is incomplete and can panic (H2, H3); the stderr layer is unredacted (H4); buffered lines are lost at exit (M2) |
| Config in `%APPDATA%\Granary` | Partial (acceptable for scaffold) | `paths::config_dir()` exists; nothing persists config yet |
| Single-instance + window-state plugins | Met (with H1) | `lib.rs:59-68`. Single-instance is registered first, but display repair runs before it (H1) |
| Least-privilege capabilities per window | Met | `capabilities/main.json`: window `main`, `core:event:default` + `allow-app-info` only; `build.rs` AppManifest lists the one command. `core:event:default` is currently unused (L3) |
| Strict CSP | Met | `tauri.conf.json:27-38`: `default-src 'self'`, `script-src 'self'`, `object-src 'none'`, `frame-ancestors 'none'`, `connect-src` only IPC (fits ADR-0004 Rust-side HTTP), `freezePrototype`. Phase 5 artwork works via `img-src https: http:` (L3) |
| CI on windows-latest: fmt, clippy, tests, typecheck, lint, build, caching, no installer | Met (with M1, M5) | `ci.yml` runs `verify.ps1 -Install -SkipFetch`; rust-cache, pnpm cache, libmpv cache; no bundling |
| `release.yml` on `v*` → NSIS → DRAFT release | Met (with M4) | `release.yml` creates with `--draft`, but can `--clobber` a published release |
| `verify` and `release-build` skills | Met | `.claude/skills/{verify,release-build}/SKILL.md`, consistent with scripts |
| Rule 7 trademark (name/binary/title/icon/installer) | Met (L6) | productName/title `Granary`, binary `granary`, original icon, publisher "Granary contributors" |
| Rule 8 licensing / notices | Met | `THIRD_PARTY_NOTICES.md` matches `libmpv.pin.json`, `docs/spikes/libmpv-smoke.md` (mpv `v0.42.0-3-g902818753`, FFmpeg `N-128504-g9f690c6a4`) and spike evidence (libplacebo `v7.387.0-13-g9fabf6d`) (L8) |
| Rule 11 no `unwrap()/expect()` / no stray TODO | Met | grep: none in non-test code (`unwrap_or_default`/`unwrap_or_else` only); `clippy::unwrap_used/expect_used` warn + `-D warnings`; no TODO/FIXME |
| Gate 3 | Not verified here | Component checks green (below). I did not run `tauri dev`. The CI run cannot be observed offline. I did not run the full `verify.ps1`: frontend build and secret scan were skipped to avoid writing `dist/` |

## Findings

### High

- **H1 Startup display repair runs before the single-instance check and has no owner check, so a second launch reverts a running instance's display changes.** `apps/desktop/src-tauri/src/lib.rs:50-51` (`repair_displays()` before `tauri::Builder` / single-instance at `:61`), `crates/display/src/marker.rs:203-219` (`repair_at_startup`), `marker.rs:24-28` (`RestoreMarker` has no pid/start time).
  - **Problem:** `run()` calls `marker::repair_at_startup(&WindowsDisplayController, …)` unconditionally. The single-instance plugin only gets a chance to exit the second process later, during `Builder::run`.
  - **Failure scenario (Phase 7):** instance A plays HDR10 content with Windows HDR toggled on (marker written). The user clicks the Start-menu shortcut again, or Windows re-activates the app. Instance B starts and finds the marker. It switches HDR off and restores the refresh rate under A's running video, deletes A's marker, and only then exits via single-instance. A now plays HDR content on an SDR desktop and no longer has a persisted marker. A crash of A after that leaves the display unrepaired at the next startup.
  - **Contract:** `docs/research/windows-display-audio.md:540` already specifies "Include `pid`, process start time" in the marker for exactly this case.
  - **Fix:**
    - Move the repair after the single-instance decision. Plugins are initialised in `Builder::build` before the `setup` hook. Set the `main` window to `"create": false`, run the repair in `.setup()`, then create the window from config, which keeps "before any window" (ADR-0006).
    - Also add `pid` + process creation time to `RestoreMarker`. Skip the repair (and log it) when that process is still alive.
    - Add a unit test with a live-owner marker.

- **H2 `redact()` panics on non-ASCII text after a sensitive key, so a log call can crash the app.** `apps/desktop/src-tauri/src/redact.rs:86`.
  - **Cause:** `s[j..j + scheme.len()].eq_ignore_ascii_case(scheme)` slices a `&str` at a byte offset that is only length-checked, not char-boundary-checked.
  - **Reproduced:** in a scratch copy of the function (outside the repo), `redact("token=äöüäöü")` panics with "end byte index 13 is not a char boundary". The same happens for any value whose 6th/7th byte falls inside a multi-byte character, e.g. `password: "ÄÄÄÄ"`. These values are realistic in an en/de app: error messages, user names, PINs typed with umlauts.
  - **Where it runs:** `RedactingWriter::write` is called synchronously on the logging thread. The panic propagates into whatever code logged, including Tauri/tao window-procedure callbacks, where a panic aborts the process. The panic hook then logs again through the same writer.
  - **Fix:** compare bytes instead: `b.get(j..j + n).is_some_and(|w| w.eq_ignore_ascii_case(scheme.as_bytes()))`. Add a test for non-ASCII values at offsets 1–8. Optionally wrap `redact` in `catch_unwind` inside the writer and emit a fixed `[REDACTION FAILED]` line.

- **H3 `redact()` leaks secrets in common `Debug` output shapes.** `redact.rs:29` (`is_value_end`), `redact.rs:91-99` (value scan), `redact.rs:68-76` (separator detection). Reproduced with the scratch copy:

  | Input (as `tracing` `?field` / derived `Debug` would render it) | Output |
  |---|---|
  | `refresh_token: Some("opaque-refresh-abc")` | `refresh_token: [REDACTED]"opaque-refresh-abc")` |
  | `api_key: Some("sa_live_secret")` | `api_key: [REDACTED]"sa_live_secret")` |
  | `profile_token: Token("ptok")` | `profile_token: [REDACTED]"ptok")` |
  | `body="{\"access_token\":\"opaque123\"}"` (Debug of a JSON `String`) | unchanged (leaks) |
  | `"password":"hun\"ter2"` | `"password":"[REDACTED]"ter2"` |
  | `Authorization: Bearer   x` (several spaces) | unchanged (leaks) |

  - **Why:** the value scan stops at the first `(` or `"` after `Some`. An escaped key (`\"access_token\":`) is never recognised because the byte after the key is `\`, not `"`, `:` or `=`.
  - **Exposure:** JWT access/refresh tokens are still caught by the bare-JWT rule. Non-JWT secrets are not: `sa_` API keys (silo-api.md §1.1), PINs, passwords, and the profile token (format unverified) leak whenever an `Option<String>`/newtype field or a response body is logged with `?`.
  - **Rule:** violates rule 6 ("tokens and passwords are never … logged").
  - **Fix:**
    - After the separator, also skip `Some(` / `Ok(` / identifier`(` wrappers and optional `\"` before the value.
    - Treat `\"` as a key terminator and value delimiter.
    - Skip runs of spaces after the auth scheme.
    - Add these strings as unit tests.
    - Longer term, give secret types a redacting `Debug` (e.g. a `Secret<String>` newtype printing `[REDACTED]`) so the line filter is a backstop, not the only defence.

- **H4 The debug stderr log layer emits ANSI escape codes that defeat redaction, so tokens are printed in `tauri dev`.** `apps/desktop/src-tauri/src/logging.rs:33-34`.
  - **Cause:** the stderr layer does not call `.with_ansi(false)`. `tracing-subscriber` 0.3.23 with default features colours field names, writing `ESC[3mtoken ESC[0m ESC[2m= ESC[0m value`. `redact()` then sees `token` followed by `ESC`, not `=`, and skips it.
  - **Reproduced:** scratch crate with the same tracing versions and redactor. `tracing::info!(token = %t, "refreshed")` printed `refreshed ^[[3mtoken^[[0m^[[2m=^[[0mopaque-secret-value`. ANSI stays on even when stderr is piped.
  - **Impact:** every structured secret field logged during `pnpm tauri dev` goes to the terminal in clear. That includes the agent-driven dev runs whose output is printed into the conversation. This violates rule 6 ("never … logged or printed").
  - **Scope:** the file layer is unaffected (`with_ansi(false)`).
  - **Fix:** `.with_ansi(false)` on the stderr layer. Alternatively, redact before formatting with a custom `FormatFields`, or strip `ESC[…m` sequences in `RedactingWriter` before redaction. Add a test that formats through the real stderr layer configuration.

### Medium

- **M1 `verify.ps1` reports a missing tool as a pass.** `scripts/verify.ps1:29-31`.
  - **Cause:** under `$ErrorActionPreference='Continue'`, `& $Exe` for a command that is not on PATH writes a `CommandNotFoundException` and continues. `$LASTEXITCODE` keeps the previous native exit code, or is `$null` and then mapped to `0`.
  - **Reproduced on PS 5.1:** after `cmd /c "exit 0"`, `Step 'x' 'nonexistent-tool'` yields `code=0`. On a machine or runner without `pnpm` on PATH, all six pnpm steps show `exit 0` and the script prints `VERIFY OK`, which is a false gate.
  - **Fix:** set `$global:LASTEXITCODE = $null` before each call, wrap the call in `try { … } catch { $code = 127 }`, and treat `$null` as failure. Alternatively, check `Get-Command $Exe` up front.

- **M2 Buffered log lines are lost at shutdown and on fatal errors.** `apps/desktop/src-tauri/src/lib.rs:45,59-75`, `logging.rs:24`.
  - **Cause:** `tauri::Builder::run` → `App::run` "never returns. When the application finishes, the process is exited directly using `std::process::exit`" (`tauri-2.12.5/src/app.rs:1364`). So `_log_guard` (the `tracing_appender` non-blocking `WorkerGuard`) is never dropped and the queued lines are never flushed. The error branch at `:74` likewise calls `process::exit(1)` with the guard alive, so the very "Tauri runtime failed" line can be lost. The same applies to a panic that ends in an abort.
  - **Phase 6/7 impact:** the "display restore" log on exit, which is the evidence ADR-0006 relies on, is exactly the kind of line that gets dropped.
  - **Fix:** use `Builder::build(ctx)` + `app.run(|_, e| if let RunEvent::Exit = e { drop guard })`. Keep the guard in a `Mutex<Option<WorkerGuard>>` and take it on `RunEvent::Exit` and before `process::exit`, or use a blocking writer for the final lines.

- **M3 The install directory and the runtime data directory are the same folder.** `apps/desktop/src-tauri/src/paths.rs:16-22`, `crates/display/src/marker.rs:39-41`, `tauri.conf.json:58` (`installMode: currentUser`).
  - **The collision:** per `docs/research/packaging-and-licensing.md:14,169` and ADR-0005:50, the NSIS per-user install goes to `%LOCALAPPDATA%\<productName>` = `%LOCALAPPDATA%\Granary`. The logs (`…\Granary\logs`) and `display-restore.json` go to the same folder.
  - **Consequences:**
    - The uninstaller deletes only the files it installed. The install folder therefore survives with logs and the marker inside it, which makes the Phase 9 "nothing left behind except user data" check ambiguous.
    - The uninstaller's "Delete app data" option targets `%LOCALAPPDATA%\<identifier>` (`io.github.mrghxst.granary`), never the logs.
    - Any future installer step that clears `$INSTDIR` would delete a pending display-restore marker.
  - **Fix:** decide now, with an ADR note. Either install to `%LOCALAPPDATA%\Programs\Granary` (custom NSIS template or installer hook) and keep data in `%LOCALAPPDATA%\Granary` as the prompt mandates, or document the shared folder explicitly and make the uninstall test account for it.

- **M4 `release.yml` can overwrite assets of an already published release, and dispatch drafts point at the wrong commit.** `.github/workflows/release.yml:45-49`.
  - **Clobber:** if `gh release view $tag` succeeds, the workflow runs `gh release upload $tag --clobber` without checking `isDraft`. Two ways to hit this: a re-run of the tag workflow after the owner published `v0.1.0`, or `workflow_dispatch` from a tag ref. Either replaces the reviewed, published installer with a new unreviewed build. That runs against rule 4's "drafts only" intent.
  - **Wrong commit:** for `workflow_dispatch` the `-dev` draft is created without `--target $env:GITHUB_SHA`. A draft's tag is created at publish time from the default branch, not from the commit that was built.
  - **Fix:** `$isDraft = gh release view $tag --json isDraft -q .isDraft`; fail if the release exists and is not a draft. Add `--target $env:GITHUB_SHA` to `gh release create`.

- **M5 CI cannot read the libmpv mirror, so its fetch has no durable source.** `.github/workflows/ci.yml:8-9,51-55`; `scripts/fetch-libmpv.ps1:31-56`.
  - **Cause:** with `permissions: contents: read`, `gh api repos/…/releases` does not list draft releases (that needs push access). The mirror is a draft release, so CI always falls back to the shinchiro URL, as ADR-0007:41 admits.
  - **Contradiction:** ADR-0007:49 claims "CI keeps working from its cache". ADR-0005:36-39 states that `actions/cache` evicts after 7 days without access and is not a durable source, and that CI uses the mirror with `GITHUB_TOKEN`. The two ADRs disagree, and the code follows the weaker one.
  - **Failure scenario:** shinchiro prunes `20261106` (~30 days), then the cache key misses (7 idle days, or eviction). CI goes red on an unchanged commit.
  - **Fix:** fetch libmpv in a small separate job with `contents: write` and hand the verified archive to `verify` via `actions/upload-artifact`. Alternatively, grant `contents: write` to the CI job, which is broader. Align ADR-0005/0007.

- **M6 The version has two sources, and the app shows the stale one.** `apps/desktop/src-tauri/src/lib.rs:46` (`env!("CARGO_PKG_VERSION")` = workspace `Cargo.toml:8` `0.1.0`), `tauri.conf.json:5` (`"version": "../package.json"`), `.claude/skills/release-build/SKILL.md:26` ("bump `apps/desktop/package.json` version").
  - **Failure:** following the skill produces a `Granary_0.2.0_x64-setup.exe` whose UI footer, `app_info` and startup log say `0.1.0`. Phase 8's About screen and Phase 9's log check would report the wrong version.
  - **Fix:** take the version from `app.package_info().version` in `setup` (the source the bundler uses), or make the skill and script bump both and add a verify step asserting they are equal.

### Low

- **L1 The redactor misses Silo-specific credentials and nested encodings.**
  - **Missing keys:** `st=` (signed stream reference), `sig=` (artwork/trickplay signature), `ticket` and the `Sec-WebSocket-Protocol: …, silo.ticket.<ticket>` form (silo-api.md §5.3, §8, §9.1) are not redacted.
  - **Nested encodings:** URL-encoded nested queries (`…%3Ftoken%3Dx`) pass through unchanged.
  - **Why it matters:** Gate 6 logs a stream URL as evidence.
  - **Fix:** add `st`, `sig`, `ticket` (exact-match keys) and a `silo.ticket.` prefix rule.
- **L2 The unsafe `set_var` is redundant and its SAFETY text is imprecise.** `lib.rs:41-43`.
  - wry 0.57 already makes the WebView2 default background transparent when the window or webview is `transparent` (`wry/src/webview2/mod.rs:133-137,473-474`). The env var, and the only `unsafe` in the host crate, can go.
  - If kept, the justification should be that `std::env::set_var` is documented as always safe on Windows. "Before any other thread exists" is not strictly true on Windows: the loader's worker threads exist before `main`.
  - Separately, the window uses `backgroundColor: "#141417"` where the validated Phase 2 spike used `#00000000`. tao paints this colour on the parent surface under the child HWNDs. That should be fine with `WS_CLIPCHILDREN`, but re-verify Design A in Phase 6.
- **L3 CSP and capability nits.**
  - `img-src https: http:` lets any host serve images. It is justified because artwork can come from S3 when `delivery: direct`, but it deserves a one-line note in the config or ADR.
  - `style-src 'unsafe-inline'` works today only because the built `index.html` has no inline `<style>`. If one is ever added, Tauri appends a `'nonce-…'` to `style-src` (`tauri-2.12.5/src/manager/mod.rs:96-104`). The browser then ignores `'unsafe-inline'` and React `style={…}` attributes break in release builds only.
  - `core:event:default` is granted while the frontend uses no events yet.
- **L4 ESLint is not type-aware, although ADR-0007:12 uses type-aware linting to justify pinning TypeScript 6.** `apps/desktop/eslint.config.js:10` uses `tseslint.configs.strict`, not `strictTypeChecked` with `parserOptions.projectService`. Either enable it or correct the ADR rationale.
- **L5 window-state restores fullscreen and visibility.** `lib.rs:65` uses `tauri_plugin_window_state::Builder::default()` (`StateFlags::all()`), so quitting during fullscreen playback reopens the home screen fullscreen. Use `.with_state_flags(StateFlags::all() - StateFlags::FULLSCREEN - StateFlags::VISIBLE)`.
- **L6 The "Compatible with Silo" tagline appears on the Home screen.** `apps/desktop/src/routes/Home.tsx:26`, `src/i18n/en.json:4`. The prompt allows it in the README and About screen, and CLAUDE.md rule 7 says "only" there. ADR-0001 permits truthful UI references. The safer reading is to move it to About (Phase 8), or to note the interpretation in CLAUDE.md.
- **L7 `normalize_origin` accepts query/fragment in the host part.** `crates/silo-api/src/lib.rs:21`: `https://host?x=1` → `https://host?x=1`, because only `/` ends the host. It should also stop at `?`/`#`, and should validate the authority (or use the `url` crate) before Phase 4 relies on it.
- **L8 Notices and path nits.**
  - The libplacebo entry links to the repository, not the commit `9fabf6d`. The build reports `-dirty` (patched by the recipe), which is worth stating.
  - `marker::default_path()` duplicates `paths::local_dir()` with a different fallback (`None` vs temp dir). Keep one source for `%LOCALAPPDATA%\Granary`.

## Checks run
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0
- `cargo test --workspace --locked` → exit 0 (granary_lib 6, granary_display 11, granary_player 13, granary_silo_api 1)
- `cargo fmt --all -- --check` → exit 0
- `pnpm -r typecheck` (PowerShell) → exit 0
- `pnpm -r test` → exit 0 (2 files, 3 tests)
- `pnpm -r lint` → exit 0
- `pnpm -r --if-present format:check` → exit 0
- Scratch harness, outside the repo, with a verbatim copy of `redact()` (`cargo run`) → exit 0. It demonstrated the H2 panic and the H3 leak table.
- Scratch crate with `tracing 0.1.44` / `tracing-subscriber 0.3.23` + `RedactingWriter` (`cargo run --offline`) → exit 0. Output showed the token unredacted with ANSI on (H4).
- PS 5.1 snippet replicating `verify.ps1` `Step` with a missing executable → reported `code=0` (M1).
- Not run:
  - `pnpm --filter desktop build`, to avoid writing `dist/`.
  - `scripts/secret-scan.ps1`.
  - `tauri dev` / `tauri build`, per instructions.
  - The CI run, since there is no network access.

## Resolution (main session, 2026-11-06)

| ID | Resolution | Evidence |
|----|------------|----------|
| H1 | Plugins first (single-instance exits a second launch during plugin init); display repair runs in `setup` before the main window is created (`"create": false`). `RestoreMarker.owner = {pid, creation FILETIME}`; a marker whose owner is alive is never repaired (`crates/display/src/process.rs`, `marker::repair_at_startup_with`). | Tests `marker_of_running_owner_is_left_alone`, `old_marker_without_owner_still_loads`; live: second launch with a live-owned marker → exit 0, marker untouched; after killing the owner the next start logged "display state repaired" and deleted the marker. |
| H2 | `redact()` rewritten byte-safe (no `str` slicing at arbitrary offsets). | Unicode sweep test (no panic for any prefix/suffix of multibyte samples). |
| H3 | Value detection handles `"key": "v"`, `key: Some("v")`, `Ident("v")`, escaped quotes, `%3D`/`%3A`, `Bearer`/`Basic` with spaces; quote-aware value end; suffix keys (`*token`, `*password`, `*secret`, `*cookie`, …). | Each leak-table row from the review is a unit test. |
| H4 | All fmt layers built by one `logging::fmt_layer` with `.with_ansi(false)`; redactor also strips ANSI. | `real_layer_configuration_redacts_structured_fields` (real layer config, structured fields, asserts no `\x1b`). |
| M1 | `Step` resets `$LASTEXITCODE`, checks `Get-Command` (127 when missing), try/catch, null exit → 1. | `verify.ps1` → exit 0 with all 11 steps listed. |
| M2 | Worker guard parked in a static; `logging::flush()` on `RunEvent::Exit`, panic hook and fatal error paths. | Graceful close → exit 0, last log line "Granary exiting" present on disk. |
| M3 | Accepted and documented (ADR-0007 "Install vs data folder"): uninstaller removes only installed files; logs and a pending marker survive by design; Phase 9 uninstall test treats them as user data. | ADR-0007. |
| M4 | `release.yml` creates drafts with `--target $GITHUB_SHA`, uploads with `--clobber` only when `isDraft`, errors on a published release. | Workflow review; first exercised in Phase 9. |
| M5 | CI split: `libmpv` job (`contents: write`, only checkout + fetch script) fills `actions/cache` keyed on the pin; `verify` job (`contents: read`) restores it with `fail-on-cache-miss` and runs `verify.ps1 -SkipFetch`. | CI run of the fix commit (see Gate 3 block). |
| M6 | Version only in `apps/desktop/package.json`; host reads `package_info().version`; `verify.ps1` fails if the Cargo workspace version differs. | verify "version sync" step. |
| L1 | Exact keys `st`, `sig`, `ticket`, `pin`, `code_verifier`; `silo.ticket.` prefix and bare JWTs masked; `%3D` separators. | Unit tests. |
| L2 | Kept the env var (belt and braces with wry's own transparent default), SAFETY text corrected to "documented safe on Windows". `backgroundColor #141417` vs spike `#00000000`: re-verify Design A in Phase 6 (PROGRESS Known issues). | — |
| L3 | `img-src` note in ADR-0007 (narrowed to `granary-img` in Phase 4/5), `style-src` nonce caveat documented, `core:event:default` removed (capability = `allow-app-info` only). | `capabilities/main.json`. |
| L4 | ESLint now `strictTypeChecked` with `projectService`. | `pnpm -r lint` → exit 0. |
| L5 | window-state uses `StateFlags::all() - FULLSCREEN - VISIBLE`. | `lib.rs`. |
| L6 | Tagline removed from Home; "Compatible with Silo" stays in README and the Phase 8 About screen. | `Home.tsx`. |
| L7 | Host ends at `/`, `?` or `#`; test added. | `normalizes_origins`. |
| L8 | libplacebo notice links commit `9fabf6d` and states `-dirty` (recipe patches); `marker::default_path()` removed, `paths::local_dir()` is the single source. | THIRD_PARTY_NOTICES.md. |
