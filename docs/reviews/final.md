# Review: Final (Phase 10, step 2)

- Date: 2026-11-08
- Scope: the whole repository at `1c10c83` (HEAD = `origin/main`; working tree: only `PROGRESS.md` modified, the
  Gate 9 closure note). Inputs: PART B of the build specification and the repository. Nothing in `.ref/` was used.
- Files read in full or in the relevant parts:
  - Rust: `apps/desktop/src-tauri/src/{lib,paths,logging,catalog}.rs`, `src/playback/{relay,host,output}.rs`,
    `crates/display/src/{marker,session,win}.rs`, `crates/silo-api/src/{api,session,device}.rs`,
    `crates/player/src/options.rs`, workspace `Cargo.toml`;
  - config and packaging: `tauri.conf.json`, `capabilities/main.json`, `windows/hooks.nsh`, `.gitignore`,
    `.env.example`, `apps/desktop/package.json`;
  - scripts and CI: `scripts/{release-build,install-test}.ps1`, `.github/workflows/{ci,release}.yml`;
  - docs: `PROGRESS.md`, `README.md`, `CHANGELOG.md`, `THIRD_PARTY_NOTICES.md`, `docs/security.md`,
    `docs/ARCHITECTURE.md`, `CONTRIBUTING.md`, `MANUAL_TEST_CHECKLIST.md`, `docs/research/silo-api.md` (§13, endpoint
    table), `docs/reviews/phase-9.md`, `docs/evidence/phase-9/*`, `docs/evidence/phase-10/*`;
  - UI: `apps/desktop/src/lib/player.ts` (key map against the README table).
- Checked against: PART B (Mission, Primary target setup, Non-negotiable rules, Phases 0-10, Definition of done).
- Reviewer: fresh subagent; did not write this code. Nothing was installed, launched, committed or pushed, and no
  display setting was changed.

**Verdict: PASS WITH FINDINGS.** No High finding, one Medium.

The project meets PART B in substance:
- Every endpoint the client calls is in `silo-api.md` §13 and the endpoint table, with OAS lines and upstream
  paths at `@6791aac`.
- The display-restore design is sound: marker written before every change, non-persistent mode switches,
  restore on stop, exit and panic, owner-checked startup repair.
- Tokens stay in the host and in Credential Manager, and logs are redacted.
- The relay is loopback-only, Host-checked, uses random route tokens and stays inside the route's directory.
- No "Silo" appears in the product name, binary, window title, icon or installer.
- AGPL licence, generated licence texts and pinned libmpv notices are in place.
- No unjustified `unwrap()`/`expect()`, no TODOs.
- `verify.ps1` is green.

Findings:
- **M1:** the release workflow runs third-party build code (pnpm lifecycle scripts, crate build scripts) with a
  `contents: write` token. This contradicts the principle the CI workflow states, and `docs/security.md` does not
  cover it.
- **Lows:** small correctness and documentation issues.

Gates 9 and 10 are still open. Their missing evidence is listed under "Open gate items" below. Those items are not
findings, because PROGRESS.md states them honestly.

## Summary of findings
- **M1** `release.yml` exposes a repo-write `GITHUB_TOKEN` to `pnpm install` and `cargo build` (env `GH_TOKEN` plus
  persisted checkout credentials).
- **L1** Re-dispatching `release.yml` uploads new assets into an existing `-dev` draft but leaves its target commit
  and notes ("Draft build from <old sha>") stale.
- **L2** `setup` can fail on `silo_state(...)?` before the startup display repair runs. A leftover HDR/refresh change
  then survives that launch.
- **L3** Documentation drift:
  - `silo-api.md` §13 says next episode does not cross seasons, but it does since Gate 8;
  - `THIRD_PARTY_NOTICES.md` says the subset has 29 operations, but it has 30;
  - `docs/security.md` justifies `style-src 'unsafe-inline'` with Radix, which is not a dependency.
- **L4** PROGRESS.md format: `Last commit: (see git log)` instead of a SHA, and no Log lines for the Phase 9/10
  steps already committed.
- **L5** The README feature list states hardware-dependent results as facts, without saying that they have not been
  verified on target hardware: DV direct play, OLED HDR correctness, smooth VRR, passthrough.

## Requirement coverage

### Non-negotiable rules
| Rule | Status | Evidence |
|---|---|---|
| 1 Truth over speed: every endpoint documented with path@SHA; UNVERIFIED isolated | Met (L3) | All 24 literal paths in `crates/silo-api/src/api.rs:257-441` plus media/stream/events map to `silo-api.md` §13 (`:676-713`, 30 operations, each with an OAS line) and the endpoint table (`:746 ff.`, with `router.go@6791aac`, `auth/service.go@6791aac` …). The OAS is pinned as `contracts/api/v2/openapi.json@6791aac` (`:15`). Subset fixture `openapi-subset.json` = 30 operations, 125 schemas (counted). Unconfirmed points (trickplay tile arithmetic, `video_range_type` vocabulary, `X-Silo-Client` handling) are marked UNVERIFIED and only drive presentation |
| 2 Verify, don't assume | Met | Every phase line in PROGRESS cites its gate command and exit code; reviews 1-9 present |
| 3 Look things up, ADRs | Met | ADR-0001…0008; versions pinned with dates in ADR-0007 |
| 4 Scope safety, drafts only | Met (M1) | Remote has only `main` and tag `v0.1.0` (`git ls-remote`). `release.yml:45-53` creates and uploads drafts only and refuses a published release |
| 5 Untrusted content | Met | No instruction-following from upstream found; `.ref/` gitignored |
| 6 Secrets | Met | `.gitignore` `.env*` / `!.env.example`; `.env.example` placeholders only; secret scan in `verify.ps1` (exit 0); `redact.rs` on every sink (`logging.rs:31,47`); refresh token in Credential Manager, access/profile tokens in memory (`session.rs:143-245`) |
| 7 Trademark | Met | `tauri.conf.json:3,4,18` (`Granary`, `granary`, title `Granary`), `index.html:6` `<title>Granary</title>`, publisher "Granary contributors", original icon set; "Silo" appears only nominatively in UI copy ("compatible with Silo media servers", "your Silo server"), README and About |
| 8 Licensing | Met (L3) | `LICENSE` (AGPL-3.0); `THIRD_PARTY_NOTICES.md` matches the pin (asset, SHA-256, mpv `9028187538`, FFmpeg `9f690c6a4`, libplacebo, recipe commit); `THIRD_PARTY_LICENSES.txt` generated by `gen-licenses.mjs`, checked by `verify.ps1` step `licenses` (exit 0), bundled (`tauri.conf.json:59`) and checked by `install-test.ps1:67` |
| 9 Resumability | Met (L4) | PROGRESS has the required sections; a commit for every step in `git log` |
| 10 Autonomy / blocked items | Met | "Blocked / needs human" covers no `.env.local`, the laptop-only hardware, the libmpv source-set gap and the stale `v0.1.0` tag, each with a workaround |
| 11 Quality bar; unwrap/expect; TODO | Met | `verify.ps1` exit 0 (below). Workspace lints `unwrap_used`/`expect_used = "warn"` with clippy `-D warnings` (`Cargo.toml:27-29`). All 146 `unwrap()`/`expect(` hits in `crates/` and `src-tauri/src` are below the file's `#[cfg(test)]` line (checked per file, e.g. `relay.rs` 225 → 241, `session.rs` 425 → 665). Each `allow(clippy::unwrap_used)` is on a test module. `git grep TODO|FIXME|XXX|HACK` finds no code hits |
| 12 End-to-end first | Met | Phases 4-6 built connect → browse → play → progress before Phase 7 deepening |
| 13 Gate evidence | Met for 0-8; open for 9-10 | PROGRESS phase lines |

### Primary target setup (design for OLED desktop first)
| Item | Status | Evidence |
|---|---|---|
| 1 HDR always-on: no toggle, SDR white, toggle only if off and enabled | Met (code, evidence on laptop panel) | ADR-0006; `session.rs` policy; `output.rs`; Phase 7 output evidence 17/17; checklist A.2/A.4 |
| 2 Windowed HDR via flip model; second SDR monitor | Met in code, hardware check manual | `options.rs` `d3d11-flip=yes`, `d3d11-exclusive-fs=no`; checklist A.2, A.10 |
| 3 Panel-aware tone mapping, DXGI GetDesc1, override, ABL | Met | `crates/display/src/win.rs`, per-monitor override in settings; checklist A.3 (calibration profile) |
| 4 Full-range, dithering, debanding, true-black theme | Met | `crates/player/mpv.conf`; checklist A.14; theme setting |
| 5 Burn-in care: auto-hide, no static, pause dim, keep-awake only while playing | Met | Overlay hidden (ADR-0008), `platform.rs` power request; checklist |
| 6 High refresh/VRR order, never 24 Hz by default | Met | `crates/display/src/policy.rs`, ADR-0006; checklist A.6/A.7 |
| 7 Overlay truly hidden for independent flip | Met in code; PresentMon check manual (blocked) | ADR-0003/0008; checklist "Independent flip" |
| 8 Multi-monitor re-apply and restore | Met in code; hardware manual | `lib.rs:221-232` `WindowMoved`, `host.rs:516-541`; checklist A.10 |
| 9 Desktop UX (hover, scroll, double-click, context menu, mini player, SMTC, window state per monitor) | Met | KEYMAP `player.ts:265-295` = README table; window-state plugin `lib.rs:205-209` |
| 10 Audio defaults shared/downmix, exclusive/passthrough opt-in | Met | Settings, checklist B.2 |

### Phases and gates
| Phase / gate | Status | Evidence |
|---|---|---|
| 0 Preflight, PRIVATE repo, skeleton | Met | PROGRESS phase 0 line; `.gitignore`, `.gitattributes`, `LICENSE`, `.env.example`, `CLAUDE.md` (< 120 lines) |
| 1 Research R1-R5 with Sources, ADR-0001…0005 | Met | `docs/research/*.md` (5 + README with SHAs); ADR-0001…0008; `reviews/phase-1.md` |
| 2 Spike: libmpv, overlay, HDR detection, round trip | Met | `docs/spikes/mpv-overlay.md`, `docs/spikes/evidence/*` |
| 3 Scaffold, CI, skills, capabilities, CSP, logs/config dirs | Met | `paths.rs:14-27`; `logging.rs` daily, 7 kept, redacted; single-instance + window-state; one capability for `main` (42 commands + event listen/unlisten); strict CSP `tauri.conf.json:28-42`; `.claude/skills/{verify,fetch-libmpv,release-build}`; `sync-upstream-web` not needed (ADR-0001 = C) |
| 4 Connectivity & auth | Met (live test blocked, recorded) | `session.rs` single-flight refresh, revoked → forget; contract + wiremock + TLS tests; `tests/live.rs` `#[ignore]` |
| 5 UI parity, settings, play hand-off, no `<video>` | Met | `no-video.test.ts`, `flow.test.tsx`, `docs/evidence/phase-5` |
| 6 Playback, progress, resilience, display never left switched | Met | Controller tests; relay (ADR-0008); `host.rs:495-510` exit order; evidence 17/17 |
| 7 Display & audio, round trip, checklist | Met (laptop) | display crate tests, `display_roundtrip` evidence, `MANUAL_TEST_CHECKLIST.md` §A (OLED first) / §B (TV) |
| 8 Polish, perf numbers | Met | `docs/perf.md` (778 ms cold start, TTFF); About with AGPL source link + build commit (`build.rs:80-92` dirty marker) |
| 9 Packaging & release | **Open** (see "Open gate items") | Installer config correct (`tauri.conf.json:50-66`, `hooks.nsh` registry cleanup); libmpv hash-pinned in CI (`release-build.ps1:31`); `docs/signing.md`; M1 |
| 10 Final verification & handoff | **Open** | `docs/security.md` + `docs/evidence/phase-10/cargo-audit.txt` present; this review; clean-clone build and FINAL REPORT pending |

### Phase 10 documentation
| Doc | Status | Notes |
|---|---|---|
| README | Met (L5) | What it is, features, install, build from source, architecture, keyboard shortcuts (match `KEYMAP`), "not affiliated" disclaimer (`README.md:12-13`). No screenshots (optional, "if available"; PNGs exist in `docs/evidence/phase-5`) |
| ARCHITECTURE, CONTRIBUTING, CHANGELOG | Met | Accurate against `lib.rs` startup order and persistent data locations |
| MANUAL_TEST_CHECKLIST | Met | Covers every Gate 7 item (HDR10, DV P5/P8, HDR10+ windowed + fullscreen, highlights, near-black, SDR on HDR, auto-hide, VRR, independent flip, multi-monitor; TrueHD/DTS-HD, 23.976) plus packaging |
| THIRD_PARTY_NOTICES | Met (L3) | — |
| security.md | Met (M1, L3) | Capabilities/CSP, IPC, remote content, tokens, audits; no section on CI token scope |

### PROGRESS.md honesty
Honest. It records:
- the missing `.env.local` and what is mocked instead;
- the laptop-only hardware, with the checks moved to the manual checklist;
- the libmpv source-set gap;
- the stale `v0.1.0` tag, with the exact commands to fix it;
- a one-off unreproduced `cargo test` failure;
- `WM_ENDSESSION` as an accepted risk;
- every deferred review Low.

No claim was found that the code or evidence contradicts, apart from the stale wording in L3 (`silo-api.md`, not
PROGRESS) and the format items in L4.

## Open gate items (not findings; PROGRESS states them)
- Gate 9: the repo has no record of the CI-built installer:
  - no `gh release view` output, run ID or asset SHA-256;
  - no install test of the CI asset (phase-9 review M1).
- `docs/evidence/phase-9/install-test.txt` predates `5f563ae`. The committed evidence therefore does not show the
  registry cleanup or the installed `THIRD_PARTY_LICENSES.txt` (`install-test.ps1:67,114` now check both).
- Phase 10 step 1: no clean-clone build evidence yet. Phase 10 step 5: the FINAL REPORT is not printed yet.
- Phases 9 and 10 are not ticked yet.

## Checks run (this session)
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1` (PATH prefixed with `%APPDATA%\npm` and
  `%USERPROFILE%\.cargo\bin`) → **exit 0**, "VERIFY OK". Step exits, all 0:
  - libmpv (`fetch-libmpv.ps1`);
  - version sync (0.1.0 = 0.1.0);
  - licenses (`gen-licenses.mjs --check`);
  - `cargo fmt --all -- --check`;
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`;
  - `cargo test --workspace --locked`;
  - typecheck;
  - eslint;
  - prettier;
  - vitest (10 files, 51 tests);
  - frontend build;
  - secret scan ("clean", built-in scanner).
- `pnpm audit --prod` → **exit 0**, "No known vulnerabilities found".
- `pnpm audit` → **exit 0**, "No known vulnerabilities found".
- `cargo audit` (cargo-audit 0.22.2) → **exit 0**, 5 allowed warnings (glib, proc-macro-error, zbus/zvariant: the
  Linux-only set listed in `docs/security.md` §5).
- `git ls-remote origin` → exit 0. `main` = `1c10c83` = local HEAD; `v0.1.0^{}` = `1257c5f`.
- `git status --short` → only `M PROGRESS.md`.
- Greps:
  - `unwrap()`/`expect(` in non-test Rust, checked per file against its `#[cfg(test)]` line;
  - `allow(clippy::unwrap_used|expect_used)` placement;
  - `TODO|FIXME|XXX|HACK`;
  - "Silo" in `tauri.conf.json`, `index.html`, NSIS hooks and package manifests;
  - API path literals in `crates/silo-api/src` against `silo-api.md`.
- `node -e` count of `openapi-subset.json` → 30 operations, 125 schemas.
- Not run (per instructions): the app, the installer, `install-test.ps1`, `release-build.ps1`, `gh`.

## Findings

### Medium

- **M1 The release workflow hands a repo-write token to third-party build code.**
  `.github/workflows/release.yml:8-10,19,32-36`, `scripts/release-build.ps1:31-32`, contrast `.github/workflows/ci.yml:17-19`.
  - **Mechanism:**
    - The whole release job has `permissions: contents: write`.
    - `actions/checkout@v7` runs with the default `persist-credentials: true`, so that token is written into
      `.git/config`.
    - The "Build NSIS installer" step also exports it as `GH_TOKEN`, then runs `release-build.ps1 -Install`. That
      script runs `pnpm install --frozen-lockfile`, which executes dependency lifecycle scripts, and
      `pnpm tauri build`, which compiles and runs every crate's `build.rs` and proc macros.
    - `ci.yml:17-19` states the opposite design: "No third-party code (pnpm/cargo deps) runs with this token". There,
      the write token is confined to a job that only runs `fetch-libmpv.ps1`.
    - `docs/security.md` does not mention CI token scope.
  - **Failure scenario:**
    - One compromised npm package or crate version reaches the lockfile through a routine update. The lockfile
      pins versions, but it does not vet them.
    - On the next `v*` tag or dispatch, its install or build script reads `GH_TOKEN` or `.git/config`. With
      `contents: write`, it can push to `main`, move tags, or replace the draft installer's assets with a trojaned
      build.
    - The owner later publishes that draft. This is the supply-chain path that the `ci.yml` split was built to close.
  - **Fix:** Use the `ci.yml` pattern. Split `release.yml` into three jobs:
    1. A `libmpv` job with `contents: write` that runs only `fetch-libmpv.ps1` and hands the archive over through
       `actions/cache` or an artifact.
    2. A `build` job with `contents: read`, `actions/checkout` with `persist-credentials: false`, and no `GH_TOKEN`.
       It runs `release-build.ps1 -Install` (which then finds the verified libmpv and needs no token) and uploads
       the installer and `.sha256` as an artifact.
    3. A `draft` job with `contents: write` that runs no repository build code. It downloads the artifact and runs
       the existing `gh release create/upload` block.

    Add a "CI and release tokens" paragraph to `docs/security.md`.

### Low

- **L1 Re-dispatching the release workflow leaves a stale draft target and stale notes.** `release.yml:43-49`.
  - **Mechanism:**
    - On `workflow_dispatch`, the tag is `v<version>-dev`.
    - The first run creates the draft with `--target $GITHUB_SHA` and notes "Draft build from <sha>".
    - Every later dispatch takes the `isDraft == true` branch and only runs `gh release upload --clobber`. The
      target commit and the notes keep the first run's SHA, while the assets come from the new commit.
  - **Impact:** the `v0.1.0-dev` draft is the final installer of this release (PROGRESS "Blocked / needs human").
    If it was dispatched more than once, its metadata names the wrong source commit, and publishing it would tag
    that wrong commit.
  - **Fix:** in the draft branch, also run
    `gh release edit $tag --target $env:GITHUB_SHA --notes "Draft build from $env:GITHUB_SHA. …"`. Alternatively,
    delete and recreate a `-dev` draft on every dispatch.
- **L2 Startup display repair can be skipped when `setup` fails earlier.** `apps/desktop/src-tauri/src/lib.rs:118,128`
  (`app.manage(silo_state(&version)?)` runs before `repair_displays()`), `lib.rs:99-100`.
  - **Mechanism:** `silo_state` returns an error when `device.json` cannot be written
    (`DeviceIdentity::load_or_create` → `write_atomic`, `crates/silo-api/src/device.rs:42`) or when
    `HttpSiloApi::new` fails. Examples: an `%APPDATA%\Granary` that cannot be written (roaming-profile or redirected
    folder problems), or a TLS verifier that fails to initialise.
  - **Impact:** `setup` returns early, the app exits with "Tauri runtime failed", and a leftover restore marker from
    a crashed run is not applied. The monitor stays in HDR or the switched refresh until a later launch succeeds.
    That weakens the ADR-0006 promise ("repaired at the next start"), and the ARCHITECTURE startup order implies
    repair comes first.
  - **Fix:** call `repair_displays()` as the first statement of `setup`, before any fallible step. Its own errors are
    already only logged.
- **L3 Documentation drift.**
  - `docs/research/silo-api.md:676,733-734`: the §13 heading still says "Phases 4–6", and the text says next
    episode "does not cross a season boundary yet". Since Gate 8 it does (`episodesAround`; PROGRESS Known issues,
    `README.md:31`, `CHANGELOG.md:15`). Fix: update §13 and its heading.
  - `THIRD_PARTY_NOTICES.md:60`: says "29 operations". The subset and `silo-api.md:678` have 30 (counted). Fix:
    change it to 30.
  - `docs/security.md:40`: says `style-src 'unsafe-inline'` "is needed by Radix/Tailwind runtime styles". Radix is not
    a dependency (`apps/desktop/package.json`). The real need is React `style={…}` attributes (10 uses in
    `apps/desktop/src`). Fix: state that reason.
- **L4 PROGRESS.md format and log.** `PROGRESS.md:2,107-121`.
  - The header reads `Last commit: (see git log)`. PART B's format asks for `<sha>`.
  - The Log ("one line per completed step") has no lines for Phase 9 and 10 steps that are already committed and
    pushed: `309d2af` packaging, `1257c5f` About build commit, `950ca33` security, `5f563ae` Gate 9 review fixes,
    `d1336dd` security review/docs, `1c10c83` changelog.
  - Fix: fill both when closing Gates 9 and 10.
- **L5 The README states hardware-dependent results as facts.** `README.md:6-10,15-16`.
  - **Mechanism:** the README promises these outright:
    - direct play of 4K HDR10 and Dolby Vision remuxes;
    - correct HDR on OLED desktop monitors;
    - smooth playback with VRR;
    - receiver passthrough.

    PROGRESS records that none of them could be verified here: laptop panel only, no VRR, no receiver, no OLED
    monitor, no DV sample, no live server. The matching `MANUAL_TEST_CHECKLIST.md` items (A.2-A.7, A.13, B.2) are
    unticked.
  - **Fix:** add one sentence to "Status". For example: "Verified on a laptop HDR panel with synthetic clips and a
    mock server; OLED/VRR/multi-monitor, Dolby Vision, receiver passthrough and a live Silo server are not verified
    yet — see MANUAL_TEST_CHECKLIST.md."

## Disposition
| Finding | Disposition |
|---|---|
| M1 | Fix before Gate 10 (rule: fix every high/medium), then re-run the Phase 10 clean-clone build |
| L1 | Fix (small) or record in PROGRESS "Known issues"; check the `v0.1.0-dev` draft's target against the commit that built its assets |
| L2 | Fix (one-line move) or record in "Known issues" |
| L3, L4, L5 | Fix at Gate 9/10 closure (docs only) |
| Open gate items | Complete before ticking Phases 9 and 10 (not review findings) |

## Resolution (main session, 2026-11-08)
| Finding | Resolution |
|---|---|
| M1 | Fixed. `release.yml` now has three jobs: `libmpv` (write token, no third-party code, fetches into the actions cache), `installer` (read-only token, `persist-credentials: false`, builds and install-tests, uploads an artifact) and `draft` (write token, no checkout or build, attaches to the draft). `ci.yml` checkouts also use `persist-credentials: false`. |
| L1 | Fixed. A re-run on an existing draft calls `gh release edit --target <sha> --notes …` after the upload. |
| L2 | Fixed. `repair_displays()` is now the first step of `setup`, before `silo_state`. |
| L3 | Fixed. `silo-api.md` §13 describes the cross-season behaviour; THIRD_PARTY_NOTICES says 30 operations; `security.md` justifies `unsafe-inline` with React inline `style` attributes; ADR-0001 Amendment 1 and ARCHITECTURE record that Radix is not used. |
| L4 | Fixed. The PROGRESS header names the last commit, and the Log has the Phase 9/10 lines. |
| L5 | Fixed. The README says what the automated tests covered and what is still a manual check. |
| Also found | The clean-clone build and CI on 1c10c83 failed in `crates/player/tests/libmpv_options.rs`: the two tests copied the DLL in parallel (sharing violation, os error 32). The copy now runs once per process (`OnceLock`), and 3 runs from a deleted DLL pass. |
