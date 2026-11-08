# Review: Phase 9 Packaging & release workflow

- Date: 2026-11-07
- Scope: commits since `381e07c`: `309d2af` (bundled + user `mpv.conf`, uninstall data hook, install test, signing
  docs, changelog), `1257c5f` (About build commit), `950ca33` (relay token redaction, artwork local-host check, IPC
  contract test). Files read:
  - Rust: `crates/player/{mpv.conf,src/options.rs,src/mpv.rs,src/state.rs}`, `apps/desktop/src-tauri/build.rs`,
    `src/{about,redact,paths,lib}.rs`, `src/playback/{commands,output,relay}.rs`, `crates/silo-api/src/api.rs`;
  - packaging: `apps/desktop/src-tauri/tauri.conf.json`, `windows/hooks.nsh`, the generated
    `target/release/nsis/x64/installer.nsi` (20:43), `target/release/bundle/nsis/Granary_0.1.0_x64-setup.exe{,.sha256}`
    (20:44), version resources of `granary.exe` and the installer, `apps/desktop/dist/assets/*`;
  - scripts and CI: `scripts/{release-build,install-test,fetch-libmpv}.ps1`, `scripts/libmpv.pin.json`,
    `.github/workflows/release.yml`;
  - docs: ADR-0002 and ADR-0005 amendments, `docs/signing.md`, `CHANGELOG.md`, `MANUAL_TEST_CHECKLIST.md` "Packaging",
    `THIRD_PARTY_NOTICES.md`, `docs/research/packaging-and-licensing.md` (WebView2, signing, obligations checklist),
    `docs/evidence/phase-9/{install-test,mpv-conf}.txt`, `apps/desktop/src/ipc-contract.test.ts`.
- Checked against: `prompt.md` PART B (Non-negotiable rules, Phase 9, Gate 9), `CLAUDE.md`, `PROGRESS.md`.
- Reviewer: fresh subagent (independent; did not write this code). Nothing was installed, launched, built for
  release, committed or pushed.
- Line numbers refer to `950ca33` (HEAD). The working tree had uncommitted Phase 10 edits while this review ran
  (`options.rs` forces `ytdl=no`/`load-scripts=no`, `silo.rs`, README, CHANGELOG, `docs/security.md`, pnpm files).
  They are not reviewed here, but the cargo/vitest runs below compiled them.

**Verdict: FAIL.** One High finding. The mechanics of the gate look right: a per-user NSIS installer with libmpv,
`mpv.conf`, `LICENSE.txt` and the notices; a silent install → launch → `ready` → uninstall test that passed; a
drafts-only release workflow; tag `v0.1.0` on the remote. But:
- **H1:** the installer ships no third-party licence texts, while the installed `THIRD_PARTY_NOTICES.md` says the full
  texts "are generated into the installer at packaging time (Phase 9)". That claim is false. Phase 9 requires the
  installer to bundle "license files", and `libmpv-2.dll` is GPLv3, which needs the GPL text.
- **M1:** the evidence covers three different builds, and none of them is shown to be the released one. The draft
  release state is not recorded in the repo, and I could not check it myself (`gh` is not signed in on this host).
- **M2:** the "nothing left behind" check misses a registry key that the silent uninstall leaves behind (confirmed on
  this machine).

Everything else is sound:
- The `mpv.conf` layering is correct and tested; the bundled file and the code baseline cannot drift.
- The uninstall hook only acts when the box is ticked, and never in update mode.
- `release.yml` never publishes and never touches a published release.
- No "Silo" appears in the product name, binary, window title, version resources, NSIS script or language file.
- Notices match the libmpv pin (asset, SHA-256, mpv/FFmpeg commits).
- The security commit's redaction and artwork checks are tested.

## Summary of findings
- **H1** No third-party licence texts in the installer; the shipped notices claim they are there.
- **M1** Gate evidence spans three builds; the CI draft artifact was not install-tested and its draft state is not
  recorded; tag `v0.1.0` predates the security commit.
- **M2** Silent uninstall leaves `HKCU\Software\Granary contributors\Granary`; `install-test.ps1` does not check the
  registry.
- **L1** User `mpv.conf` can override host-required options that are not forced (`idle`, `user-agent`, cache dir).
- **L2** The "user conf breaks mpv" fallback was never exercised, and the user is not told about it.
- **L3** `GRANARY_COMMIT` has no dirty marker, so About can link a source tree that is not what the binary was built
  from.
- **L4** Artwork local-host check covers literals only (DNS names that resolve to LAN/loopback pass; a few reserved ranges
  missing).
- **L5** `install-test.ps1` robustness: an unparsable log line aborts the script; run 1's failed uninstall is
  unexplained.

## Phase 9 requirement coverage
| Requirement | Status | Evidence |
|---|---|---|
| Installer: NSIS, per-user, no admin | Met (M2) | `tauri.conf.json:52,62` (`targets: ["nsis"]`, `installMode: "currentUser"`); generated `installer.nsi:41,105-107` (`INSTALLMODE "currentUser"` → `RequestExecutionLevel user`), `:538` (`$LOCALAPPDATA\Granary`); `install-test.txt` "PASS per-user install location (no admin)", "uninstall entry registered under HKCU" |
| Bundles the libmpv DLL | Met | `tauri.conf.json:56`; `installer.nsi:813` (uninstall list shows the installed resources); `install-test.txt` "PASS installed libmpv-2.dll" |
| Bundles `mpv.conf` defaults | Met (L1, L2) | `crates/player/mpv.conf` (18 options with a one-line reason each); `tauri.conf.json:57`; loaded with `include=` from the exe directory (`playback/commands.rs:123-137`, `options.rs:86-104`); test `bundled_conf_matches_the_code_baseline` keeps it equal to `desktop_baseline()`; `mpv-conf.txt` cases 1-3 (bundled-only, user override, invalid user lines); ADR-0002 Amendment 1 |
| Bundles licence files | **Partly (H1)** | `LICENSE.txt` (AGPL text, `tauri.conf.json:59`, also the installer licence page via `licenseFile`, `installer.nsi:169-171`) and `THIRD_PARTY_NOTICES.md` are installed. Missing: GPL-3.0 text for `libmpv-2.dll`, LGPL-2.1 (libplacebo, `libmpv2-sys`), OFL-1.1 (embedded font), and the MIT/Apache/ISC/BSD notices of the shipped Rust and JS dependencies. `THIRD_PARTY_NOTICES.md:47-48` claims they are generated in Phase 9; nothing generates them |
| Handles WebView2 per R5 | Met | R5 recommends `downloadBootstrapper` (`research/packaging-and-licensing.md:202`); `tauri.conf.json:63`; `installer.nsi:57,585-641` (registry `pv` check, skip when present, abort on download failure); clean-PC check is `MANUAL_TEST_CHECKLIST.md` "Packaging" item 3 |
| Original app icon, not Silo's | Met | `bundle.icon` points at `apps/desktop/src-tauri/icons/*` (ADR-0001:63, ADR-0005:52 "Original icon"); no Silo brand files referenced; `granary.exe` and installer version resources: ProductName/FileDescription "Granary", CompanyName "Granary contributors" |
| libmpv in CI via `fetch-libmpv.ps1`, hash-pinned | Met | `release.yml:33-37` → `release-build.ps1:34` runs `fetch-libmpv.ps1`; `scripts/libmpv.pin.json` (asset + SHA-256), `fetch-libmpv.ps1:65-68` (mismatch fatal on every source path) |
| Release: tag `v0.1.0` pushed; `release.yml` attaches the installer to a draft; verify with `gh release view` | **Partly (M1)** | Remote tag exists: `git ls-remote --tags origin` → `refs/tags/v0.1.0^{}` = `1257c5f` (HEAD is `950ca33`). `release.yml:43-53` creates with `--draft`, uploads only to a draft, refuses a published one. `gh release view` / `gh run list` could not be run here (`gh auth status` exit 1: not logged in; the orchestrator's credential pass-through was not available to the reviewer). No evidence file records the draft state, assets or run result |
| Local install test: `/S` install, launch, logs `ready`, `/S` uninstall, nothing left except user data | Partly (M1, M2, L5) | `scripts/install-test.ps1`; `install-test.txt` run 2: INSTALL TEST PASSED (ready line `cold_start_ms=515`, 6 installed files removed, shortcuts removed, user data kept). Gaps: registry leftover not checked (M2), and the tested installer was built at 20:44, before `309d2af` (20:50) |
| Code signing documented in `docs/signing.md` only, not configured | Met | `docs/signing.md` (status, SmartScreen/Smart App Control, options table, Artifact Signing steps, OIDC, updater keys separate; facts trace to research §4 S26-S29); no `signCommand`/`certificateThumbprint` in `tauri.conf.json`; no signing secret in `release.yml` |

## Gate 9 coverage
| Requirement | Status | Evidence |
|---|---|---|
| Installer built locally | Met | `target/release/bundle/nsis/Granary_0.1.0_x64-setup.exe` (39.8 MB, 20:44) with `.sha256` `07b4f972…7c6c7d`, equal to `install-test.txt` |
| Installer built in CI | Not verified (M1) | `release.yml` is correct by reading; no run ID, conclusion or asset hash in the repo, and `gh run list` was not available to the reviewer |
| Install → launch → uninstall test passes | Met with gaps (M1, M2) | `install-test.txt` runs 2 and 3 PASS, exit 0; done on a pre-commit local build, not on the release asset |
| Release is a draft in the private repo | Not verified (M1) | Tag on the remote; draft state not observable here and not in evidence |

## Other rules checked
- **Trademark (rule 7):** `productName` "Granary", `mainBinaryName` "granary", window `title` "Granary",
  `installer.nsi:37,48,86-87` and `English.nsh` have no "Silo"; `install-test.txt` "window title has no trademark".
  The internal crate `granary-silo-api` is not a user-visible name.
- **THIRD_PARTY_NOTICES vs pin (rule 8):** asset `mpv-dev-x86_64-v3-20261106-git-9028187538.7z` and SHA-256
  `a778b344…0683bb` match `libmpv.pin.json` and `vendor-bin/libmpv/.pinned`. mpv `9028187538` (v0.42.0-3), FFmpeg
  `9f690c6a4`, libplacebo and the recipe commit carry source links. The only gap is H1.
- **Drafts only (rule 4):** `release.yml:47` `--draft`, `:48-49` upload only when `isDraft == true`, `:50-53` hard
  failure on a published release. `workflow_dispatch` uses a `-dev` tag and is also a draft.
- **unwrap/expect/TODO (rule 11):** the added non-test Rust code has no `unwrap()`/`expect(`. `options.rs` tests carry
  `#[allow(clippy::unwrap_used)] // test code`. No new TODO.
- **Security commit `950ca33`:** `redact.rs:191-200` masks `/r/<32 hex>/` (the relay builds `uuid::new_v4().simple()`,
  `relay.rs:117`), with positive and negative tests. `api.rs:504,511-538` refuses foreign https artwork on
  localhost/loopback/private/link-local/CGNAT/ULA literals and keeps the server's own LAN origin (tested; see L4).
  `ipc-contract.test.ts` cross-checks build.rs `COMMANDS`, `generate_handler!`, the capability and UI argument names
  (runs in vitest below).
- **About build commit:** `build.rs:61-67,81-92`, `about.rs:18-31`; the link only uses a 40-hex value, otherwise the
  repo root (test `source_link_points_at_the_build_commit`). See L3.

## Checks run (this session)
- `cargo test --workspace --locked` → **exit 0**: all suites ok (58, 36, 16, 43, 13, 1 passed; 1 ignored = live test).
- `pnpm --filter desktop exec vitest run` → **exit 0**: 10 files, 51 tests passed (incl. `ipc-contract.test.ts`).
- `cargo fmt --all --check` → **exit 0**.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → **exit 0**.
- `pnpm --filter desktop exec tsc --noEmit` → **exit 0**.
- `git ls-remote --tags origin` → exit 0; `v0.1.0` → `1257c5f`; local `git rev-parse HEAD` = `950ca33`.
- `gh auth status` → **exit 1** (not logged in). So `gh release view v0.1.0 …` and `gh run list …` could not be run.
  The reviewer was not allowed to read the stored GitHub credential.
- `Test-Path "HKCU:\Software\Granary contributors\Granary"` → **True**, `(default) = C:\Users\mr\AppData\Local\Granary`,
  while the Uninstall key and `granary.exe` are gone (M2).
- `(Get-Item …).VersionInfo` of `target/release/granary.exe` and the installer: no "Silo"; FileDescription/ProductName
  "Granary".
- `grep -c "@license|MIT License|Copyright" apps/desktop/dist/assets/*.js` → 0 (the JS bundle keeps no licence
  comments; H1).
- Not run (per instructions): the installer, the app, `install-test.ps1`, `release-build.ps1`, `verify.ps1`.

## Findings

### High

- **H1 The installer ships no third-party licence texts, and the shipped notices claim it does.**
  `THIRD_PARTY_NOTICES.md:47-48`, `tauri.conf.json:55-60`, `docs/research/packaging-and-licensing.md:318-326`
  (obligations checklist).
  - **Mechanism:**
    - The installer holds exactly 6 files (`install-test.txt`): exe, DLL, `mpv.conf`, `LICENSE.txt` (AGPL-3.0 text),
      `THIRD_PARTY_NOTICES.md`, uninstaller.
    - The notices file lists components by name only, then states: "Full licence texts of all bundled Rust and
      JavaScript dependencies are generated into the installer at packaging time (Phase 9)." No script, resource or
      workflow step generates them (repo grep for cargo-about / licence export: none).
    - The Vite bundle strips licence comments (0 matches in `dist/assets/*.js`).
    - `libmpv-2.dll` is GPL-3.0-or-later (notices L11-13), and GPLv3 §4/§6 require the GPL text with the binary. The
      AGPL text is a different licence. LGPL-2.1 (libplacebo, `libmpv2-sys`), OFL-1.1 (Plus Jakarta Sans, embedded as
      woff2) and the MIT/Apache-2.0/ISC/BSD crates and npm packages also require their licence and copyright notices
      in binary copies.
  - **Failure scenario:** the owner hands the `v0.1.0` draft installer to a tester (that is what the
    `MANUAL_TEST_CHECKLIST` "Packaging" section prepares for). The tester gets a GPLv3 DLL without the GPL, dozens of
    MIT/Apache components without their notices, and an About → Licences page that says the texts are in the
    installer when they are not. The project rules (rule 8) and the Phase 9 bullet "bundles … license files" are not
    met.
  - **Fix:**
    - Generate a `THIRD_PARTY_LICENSES.txt` at build time: for example `cargo about generate` with an `about.toml`
      accepting the workspace's licences, plus a pnpm licence export for the production JS deps (or
      `rollup-plugin-license`).
    - Add the full GPL-3.0, LGPL-2.1 and OFL-1.1 texts.
    - Bundle the file as a resource in `tauri.conf.json`, run the generation from `release-build.ps1`, and check in
      `install-test.ps1` that it is installed.
    - Replace notices L47-48 with a pointer to that file.
    - If this is deferred, at minimum remove the false sentence and record it under "Blocked / needs human" next to
      the libmpv source-set gap.

### Medium

- **M1 Gate evidence spans three different builds, and the released artifact is unverified.**
  `docs/evidence/phase-9/install-test.txt`, `.github/workflows/release.yml`, remote tag `v0.1.0`.
  - **Mechanism:**
    - The install-tested installer was built at 20:44 (file time; the hash matches `install-test.txt`, and the ready
      line is 19:47:54Z = 20:47 local). That is before `309d2af` (20:50), so it came from `381e07c` plus uncommitted
      Phase 9 edits.
    - The CI draft is built from the tag, `1257c5f` (About commit embedding, build.rs `git` call), which no local
      install test ran against.
    - HEAD `950ca33` (relay token redaction, artwork local-host refusal) is in neither build.
    - No file records `gh release view v0.1.0` (isDraft, asset names, sizes), the release run's ID and conclusion,
      or the CI installer's SHA-256. The reviewer could not query GitHub.
  - **Failure scenario:**
    - The build.rs change added in `1257c5f` breaks in CI (for example `git` missing or not trusting the checkout).
      Then About shows `unknown` and links the private repo root, or the CI asset has some other packaging
      difference. Gate 9 is still recorded as passed on a local installer that is not the shipped one.
    - Later the owner publishes the `v0.1.0` draft, which lacks the `950ca33` security fixes. CHANGELOG `[0.1.0]` does
      not say so.
  - **Fix:**
    - Add `docs/evidence/phase-9/release.txt` with the `gh release view v0.1.0 --json isDraft,assets,tagName,targetCommitish`
      output and `gh run view <id>` (conclusion, head SHA).
    - Download the CI asset and compare it with its `.sha256` asset. Run `scripts/install-test.ps1 -Installer <downloaded exe> -Report …`
      on it and save the report.
    - Decide the tag: either cut the draft from a commit that includes `950ca33` (new tag such as `v0.1.1`, or delete
      the unpublished draft and re-tag while it is still private and unpublished), or state in PROGRESS.md and the
      release notes that `v0.1.0` predates `950ca33`.

- **M2 The silent uninstall leaves an installer registry key, and the test does not look for it.**
  `target/release/nsis/x64/installer.nsi:873-890,902-906` (template), `apps/desktop/src-tauri/windows/hooks.nsh:11-20`,
  `scripts/install-test.ps1:100-115`.
  - **Mechanism:** Tauri's template deletes `${MANUPRODUCTKEY}` (`HKCU\Software\Granary contributors\Granary`, the
    install location) only when "Delete the application data" is ticked. A plain or `/S` uninstall keeps it. On this
    machine after the test: the key exists with `(default) = C:\Users\mr\AppData\Local\Granary`, while the Uninstall
    key and `granary.exe` are gone. `install-test.ps1` checks files, shortcuts and the Uninstall key only.
  - **Failure scenario:** Phase 9 asks the test to confirm "nothing is left behind except user data". The report says
    PASS, but installer state (not user data) remains in the registry after every uninstall. A later install reads it
    back as the default location (`RestorePreviousInstallLocation`). The test would never notice a growing set of
    leftovers here.
  - **Fix:** In `NSIS_HOOK_POSTUNINSTALL`, when `$UpdateMode <> 1`, delete `HKCU\Software\Granary contributors\Granary`
    and remove `HKCU\Software\Granary contributors` if empty (or accept the Tauri behaviour explicitly in ADR-0005).
    Add a registry check to `install-test.ps1`: the Uninstall key, the manufacturer key, and the HKCU Run value.

### Low

- **L1 The user `mpv.conf` can override host-required options that are not in `forced()`.** `options.rs:5-22,86-104`
  (at HEAD; the uncommitted Phase 10 edit adds `ytdl`/`load-scripts` to `forced()` but not `idle`),
  `playback/commands.rs:95-118`. The user include comes after the settings layer, which also carries host invariants:
  `idle=yes`, `user-agent`, `gpu-shader-cache-dir`, `stream-lavf-o`, `network-timeout`.
  - With `idle=no` in the power-user file, libmpv shuts down after the first title ends. mpv is created once per app
    run, so the player is gone until restart.
  - Per-title runtime properties (`output.rs:120-140`: `video-sync`, `target-peak`, `hdr-reference-white`, `audio-*`)
    silently override the user file. The `mpv.conf` header says this only about the bundled values.
  - Fix: move `idle` (and `user-agent` if the server relies on it) into `forced()`. Say in the checklist/ADR which keys
    the host sets per title and therefore always wins.
- **L2 The fallback for a broken user file is untested and silent.** `playback/commands.rs:130-138`. `mpv-conf.txt`
  case 3 shows mpv accepts bad lines, so the retry path never ran. The user only gets a log line. Fix: a unit test
  through a fake `Mpv::new` seam, or an evidence case with an include that mpv rejects. Optionally a one-time notice
  "your mpv.conf was ignored".
- **L3 The build commit has no dirty marker.** `build.rs:81-92`. A build from a modified tree, like the locally
  install-tested installer, reports HEAD, and About's AGPL source link (`about.rs:21-31`) then points at a tree that
  is not the binary's source. Fix: run `git status --porcelain` (or `git describe --dirty --always`) and either append
  `-dirty` (and fall back to the repo root link) or make `release-build.ps1` refuse a dirty tree.
- **L4 The artwork local-host check covers literals only.** `crates/silo-api/src/api.rs:511-538`. A foreign https
  host name that resolves to 127.0.0.1 or a LAN address (DNS rebinding, or a hostile server pointing at `nas.lan`) is
  fetched. Unclassified: multicast (`224/4`, `ff00::/8`), `198.18/15`, `240/4`, NAT64 `64:ff9b::/96` and 6to4
  `2002::/16` with an embedded private v4. Impact is small: raster-only, no redirects, no credentials sent. Fix:
  resolve and check the peer address in a custom reqwest `dns_resolver` for foreign artwork hosts, and add the missing
  ranges; or document the literal-only scope in `docs/security.md` (Phase 10).
- **L5 `install-test.ps1` robustness.**
  - L86 calls `[datetime]::Parse` inside `Where-Object` with `$ErrorActionPreference = 'Stop'`, on any matching line
    of 27+ characters. One malformed or ANSI-prefixed log line aborts the whole test with an exception instead of a
    FAIL line. Use `[datetime]::TryParse`.
  - Run 1's silent uninstall left the program in place and is recorded as unexplained (`install-test.txt`). A likely
    cause is `CheckIfAppIsRunning` racing an app that had not fully exited. Make the test wait for the `granary`
    process (not just `HasExited` of the launched PID) before uninstalling, and log the uninstaller's exit, so a
    recurrence can be diagnosed.

## Disposition
| Finding | Disposition |
|---|---|
| H1 | Fix before Gate 9 (or remove the false claim and record the gap as "Blocked / needs human") |
| M1, M2 | Fix (or accept explicitly in PROGRESS.md / ADR-0005) before Gate 9 |
| L1-L5 | Fix or record in PROGRESS.md "Known issues" |

## Resolution (main session, 2026-11-08)
| Finding | Resolution |
|---|---|
| H1 | Fixed (5f563ae). `scripts/gen-licenses.mjs` generates `THIRD_PARTY_LICENSES.txt`: GPL-3.0 for libmpv/FFmpeg, LGPL-2.1, and the licence files of 293 Rust crates and 15 JS packages. The installer bundles it, `verify.ps1` step `licenses` keeps it current, and the install test checks that it is installed. |
| M1 | Fixed. The final installer was built by the release workflow from main `dd9b7a8` (run 42428189010, workflow_dispatch; jobs libmpv/installer/draft all succeeded) into draft release `v0.1.0-dev`. Its isDraft is `true`, its target is `dd9b7a8` and its assets are the exe and `.sha256` (`docs/evidence/phase-9/draft-release.txt`). The downloaded asset's SHA-256 matches its `.sha256`, `a78a23e5…091d`. `scripts/install-test.ps1` passed against that CI asset (`docs/evidence/phase-9/install-test-ci.txt`). CI on `dd9b7a8` is green. Tag `v0.1.0` stays on 1257c5f, because the session's safety policy refused to force-move a pushed tag. Moving it before publishing is recorded in PROGRESS "Blocked / needs human". |
| M2 | Fixed (5f563ae). `NSIS_HOOK_POSTUNINSTALL` deletes the product key outside update mode. While re-checking, a broken path in `install-test.ps1` was found: a shell edit had stripped its backslashes, so the "left behind" check could never fail. It now uses `HKCU:\Software\Granary contributors`, and a new check that the key exists after install proves it can fail. Both pass on the CI installer. |
| L1 | Fixed: `idle`, `keep-open`, `ytdl`, `load-scripts` are forced; real-libmpv test `real_libmpv_accepts_every_option`. |
| L2 | Documented (ADR-0002 Amendment 2) and tested: `broken_user_conf_does_not_stop_mpv` shows libmpv only warns on bad user-conf lines. |
| L3 | Fixed: build.rs emits `<sha>-dirty`; About shows it. |
| L4 | Accepted and documented in docs/security.md: DNS names are not resolved; no credentials are ever sent with artwork requests and redirects are not followed. |
| L5 | Fixed: log timestamps parsed with `TryParse`. The earlier uninstall failure was caused by a leftover registry key from a previous test run, which the pre-flight check now rejects. |
