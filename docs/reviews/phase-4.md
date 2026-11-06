# Review: Phase 4 Silo connectivity & auth

- Date: 2026-11-06
- Scope: commit `6573af4` (`git diff 508e26c..6573af4`): `crates/silo-api/src/{api,session,error,models,credentials,registry,device,secret,mock,lib}.rs`,
  `crates/silo-api/tests/{contract,tls,live}.rs` + `tests/fixtures/`, `apps/desktop/src-tauri/src/{silo,lib}.rs`, `build.rs`,
  `capabilities/main.json`, `apps/desktop/src/{App.tsx,lib/{silo,errors,nav}.ts,components/ui.tsx,routes/*.tsx,i18n/*.json}`
  and tests, `scripts/extract-oas-subset.mjs`, `docs/research/silo-api.md` §13, ADR-0004 amendment. Checked against
  `prompt.md` PART B (Non-negotiable rules, Phase 4), `CLAUDE.md`, ADR-0004, `docs/research/silo-api.md`, and upstream
  `.ref/silo-server@6791aac` (OAS + Go sources, read as data).
- Reviewer: fresh subagent (independent; did not write this code)

**Verdict: FAIL.** The phase is mostly in good shape:
- the typed client covers exactly the nine documented operations;
- the OAS subset regenerates byte-identically from the pinned upstream document;
- secrets are wrapped in a redacting `Secret`, and no token crosses IPC;
- redirects are never followed, and the TLS errors are classified;
- the refresh single-flight, 401 → refresh → retry-once, and 503-keeps-tokens rules are implemented and tested;
- every Rust and TS check I ran is green.

One High finding blocks the gate. Re-adding a "known" server at a new address moves the stored refresh token to that
address without a new sign-in (H1). The `server_id` that triggers this is public, so a look-alike server can collect the
token. The verdict becomes PASS WITH FINDINGS once H1 is fixed and covered by a test. M1 (a stale PIN-profile token
leaves the profile picker stuck until restart) should be fixed in the same pass.

## Phase 4 requirement coverage
| Requirement | Status | Evidence |
|---|---|---|
| URL normalization | Met | `lib.rs:30-49` (https default, lower case, default port dropped, path/query/userinfo rejected or stripped; same serialization as `request_origin`). Tests `lib.rs:100-123`. A host:port without a scheme on a plain-http server gives a generic TLS error (L7) |
| TLS errors shown clearly | Met | `error.rs:131-199` walks the chain into `io::Error` payloads to `rustls::Error`. Expired, not-yet-valid, untrusted, name mismatch and revoked each have a message (en/de). `tests/tls.rs` runs against a real self-signed server → `tls_untrusted` (passes) |
| Reachability check | Met | `probe()` (`lib.rs:68-92`): 404 or non-JSON → `not_silo`, connect/timeout → `unreachable`/`timeout`, 3xx → `redirected` (location not shown, L6) |
| Version detection + untested warning | Met | `api_major == 2` enforced. `contract_digest` compared with `TESTED_CONTRACT_DIGESTS`, a warning only, never a block. I verified the digest: `git show 6791aac:contracts/api/v2/openapi.json \| sha256sum` = `6a439ae5…b8b9`, and upstream computes it the same way (`internal/apiv2/system.go@6791aac` L48-52) |
| Multiple servers | Met (with H1, M2) | `registry.rs` keyed by `server_id`, sessions map per server, Home lists all servers. Re-adding a known server keeps its account (H1) |
| Login + profile selection | Met (with M1) | Password login and `listProfiles`. PIN via `verifyProfilePIN`, profile token in memory only (`session.rs:263-281`), PIN asked again after restart (test `silo.rs:508-514`). A stale profile token blocks the picker (M1) |
| Credential Manager | Met | `credentials.rs:57-99` (`keyring-core` + `windows-native-keyring-store`, service `Granary`, key `<server_id>/<user_id>`). Refresh token only. The real-store round trip test passes on this host. Falls back to memory with an error log when unavailable (`lib.rs` host) |
| Refresh / logout / revoked | Met | Proactive refresh 60 s before expiry; single-flight via `refresh_lock` + generation; 401/403 from refresh → `forget()` deletes the credential; host `settle()` clears the account and emits `auth://revoked`; UI `RevokedListener` → sign-in with "session ended". 503 + `Retry-After` keeps tokens (`session.rs:367-385`). Logout is best effort, then forgets |
| Device identity | Met | UUID v4 in `%APPDATA%\Granary\device.json` (recreated if corrupt). `X-Silo-Client: Granary for Windows`, `X-Silo-Client-Version` from `package_info()`, `X-Silo-Client-Family: desktop` (enum verified at `settings_values_seam.go@6791aac` L180-184), `X-Silo-Device-Platform: windows`, device name = `COMPUTERNAME` (≤120 chars). Non-ASCII names are accepted (`HeaderValue: TryFrom<&String>` uses `from_bytes`) |
| Typed client, exactly the used endpoints, documented | Met (L3, L8) | 9 ops in `SiloApi`; each is in the silo-api.md endpoint table and §13. `getCurrentUser` is never called by the app (L8) |
| Contract tests with sanitized fixtures | Met | `tests/contract.rs` validates 11 fixtures against the subset schemas (required, `additionalProperties:false`, enums). It serves them to the real `HttpSiloApi` and checks that every request maps to a documented op, bearer only on secured ops, and bodies against the request schemas. Fixtures are synthetic (`fixture-access-1`, `pvt_fixture`, `example.test`) |
| Live tests tagged / skipped in CI | Met | `tests/live.rs` is `#[ignore]`, reads `.env.local`, prints no credentials, and logs out at the end. I did not run it |
| Rule 1 truth | Met (L3) | All endpoints, fields, headers and problem ids used exist in the OAS or the cited code: `account_required`, `account_disabled`, `password_change_required`, `profile_verification_required`, `rate_limited`, `client_upgrade_required`, `provider_unavailable`, `invalid_token`, `session_expired`, `dependency_unavailable` (all in `internal/apiv2/problem.go@6791aac`). One citation path is wrong (L3) |
| Rule 6 secrets | Met (H1) | `Secret` has a redacting `Debug` and no `Display`; reqwest errors use `without_url()`; IPC views carry no tokens; vitest asserts the password reaches only `auth_login`. H1 sends the credential to an origin it was not issued for |
| Rule 7 trademark | Met | App name, title and binary unchanged (`Granary`). "Silo" appears only descriptively in UI text ("Found a Silo server") |
| Rule 11 unwrap/expect/TODO | Met | grep of the diff: `unwrap()`/`expect(`/`panic!` only in tests (each with an `allow` + reason); no TODO/FIXME |
| i18n en/de parity | Met | At `6573af4`: 85/85 keys, placeholders identical. `errors.test.ts` ties every Rust `code()` arm to both catalogues |
| Tauri capability scope | Met (L8) | 10 new `allow-*` command permissions (all generated by `build.rs`) plus `core:event:allow-listen`/`allow-unlisten`; no emit, no HTTP or fs plugin; CSP unchanged (`connect-src` IPC only) |
| Gate 4 | Not passed (H1) | Mocked coverage is complete and green. Live login is recorded as blocked (no `.env.local`) in PROGRESS.md, as the prompt allows |

### Checks run (this session)
- `cargo test -p granary-silo-api --locked` → exit 0 (22 unit + 7 contract + 1 TLS passed, 1 live ignored).
- `cargo test -p granary --locked --lib silo` → exit 0 (3 silo tests + 1 redact filter match).
- `pnpm exec vitest run` (apps/desktop) → exit 0, 5 files / 14 tests. The working tree also holds uncommitted Phase 5
  files; the Phase 4 test files themselves are unchanged from `6573af4`.
- `node scripts/extract-oas-subset.mjs` (copy writing to the scratchpad) → output identical to the committed
  `openapi-subset.json` (9 operations, 18 schemas).
- I did not run `verify.ps1`, the live test or `tauri dev`.

## Findings

### High

- **H1 Re-adding a known server at a new address moves its stored refresh token to that address without a new sign-in. A look-alike server can trigger this, because `server_id` is public.** `apps/desktop/src-tauri/src/silo.rs:187-205` (`add`), `crates/silo-api/src/registry.rs:78-91` (`upsert` keeps `account`), `silo.rs:280-291` (`session()` resumes with `record.origin`).
  - **Problem:** `add()` re-probes the new address and upserts by the `server_id` the new address reports.
    - For an existing record, `upsert` overwrites `origin` but keeps `account` (and the refresh token in Credential Manager). `add()` also drops the live session.
    - The next authenticated command resumes the session from the store and POSTs the refresh token to the new origin (`Session::resume` → `current()` → `api.refresh(&self.origin, …)`).
    - Nothing asks the user to sign in again.
    - `getServerIdentity` is unauthenticated (OAS `security` absent; `api.rs:30-31`), so anyone who can reach the real server can read its `server_id` and serve it.
  - **Failure scenario 1 (malicious):** the user is signed in to `https://media.example.com`. They are told the server "moved" and enter `https://media-example.net`. That host answers `/api/v2/system/info` with `api_major: 2` and `/api/v2/system/identity` with the real `server_id`.
    - The UI says only "This server is already saved; adding it again updates its address" and offers **Update address**.
    - Home still shows "Signed in as alice". Clicking **Open** sends the refresh token to `media-example.net/api/v2/auth/refresh`.
    - Upstream refresh tokens are stateless JWTs with no reuse detection (`internal/auth/service.go@6791aac` L1183-1225). The attacker can mint access tokens for up to 30 days, or until the session is revoked.
  - **Failure scenario 2 (benign):** the user re-adds the same server by its LAN address `http://192.168.1.5:8096`. From then on, every refresh sends the 30-day refresh token in clear text. The http warning mentions only "your password".
  - **Rule:** this breaks the ADR-0004 M4 guarantee ("credentials only to the configured origin"). The token was issued for one origin and is silently re-scoped to another. It also breaks rule 6.
  - **Fix:** when an upsert changes the `origin` of a record that has an account, end that account:
    - best-effort `logout` against the **old** origin;
    - `store.delete(<server_id>/<user_id>)`;
    - `account = None`, `last_profile_id = None`.
    - Say so in the UI (`addServer.known` → "…updates its address; you will need to sign in again"). The add flow already navigates to login.
    - Alternatively, store the origin with the credential and refuse to resume when it differs.
    - Add a `silo.rs` test: add + login, re-add the same `server_id` at another origin, then assert that the store is empty, `account` is `None`, and `libraries` returns `not_signed_in`.

### Medium

- **M1 Account-level calls carry the selected profile's headers, so a stale PIN-profile token makes the profile picker and PIN unlock fail until the app restarts.** `crates/silo-api/src/api.rs:88-97` (`authed` adds `X-Profile-Id`/`X-Profile-Token` to every authenticated call), `session.rs:256-260` (`profiles`), `session.rs:263-275` (`select_profile` → `verify_profile_pin` with the current profile), `apps/desktop/src/routes/Profiles.tsx`.
  - **Upstream behaviour:**
    - `listProfiles` and `verifyProfilePIN` are `ClassProfileScoped` with `ProfileOptional` (`internal/apiv2/profiles.go@6791aac` L190-197, L278-292).
    - Their gate chain still runs `RequireViewerAccess` (`gates.go` L118-124). That middleware resolves whatever `X-Profile-Id`/`X-Profile-Token` the request carries and answers 403 when the token no longer verifies (`internal/api/middleware/viewer_access.go@6791aac` L44-68).
    - v2 rewrites that answer as `profile_verification_required` (`gates.go` L266-270).
    - The OAS says the profile token "stops being accepted when … the profile's PIN changes" (OAS ~L47495).
  - **Failure scenario:** the Kids profile is unlocked on the desktop. A household manager changes the Kids PIN in the web app.
    - The next `libraries_list` gets 403 → `Libraries.tsx` navigates to Profiles.
    - `profiles_list` sends the same stale `X-Profile-Id: kids` + token → 403 → `ProfileVerificationRequired`. The page shows "This profile needs its PIN." with no profile list.
    - Profiles has no back button. Even with a list, re-entering the new PIN would send the stale headers to `verify-pin` and get the same 403.
    - Only an app restart clears the in-memory profile. A deleted profile gives the same dead end with 404.
  - **Fix:**
    - Send profile headers only on viewer operations: give `list_profiles`, `verify_profile_pin`, `current_user` and `logout` an `Auth` with `profile: None`, or add a `bearer_only` builder.
    - On `ProfileVerificationRequired` or a profile 404 in `Session::call`, clear the profile token (`set_profile(None)` or keep the id without the token).
    - Make `MockSiloApi::list_profiles`/`verify_profile_pin` apply `check_profile` when a header is present, as upstream does, and add a regression test: unlock Kids → invalidate its tokens → `profiles()` and `select_profile(kids, new_pin)` still succeed.

- **M2 Signing in on a server that already has an account neither revokes nor deletes the previous credential.** `apps/desktop/src-tauri/src/silo.rs:238-270` (`login`), `silo.rs:197-198` (`add` drops the session but keeps credential and account), `silo.rs:207-217` (`remove` deletes only the current account's key), `apps/desktop/src/routes/AddServer.tsx:36` (always navigates to login, also for a known server).
  - **Problem:** `login()` stores the new refresh token and overwrites `record.account`. It never calls `logout` on the previous session and never deletes the previous `<server_id>/<old_user_id>` entry.
  - **Reachable path:** Add server → known server → **Update address** → login screen. Sign in as a different user (for example a family member's account).
  - **Failure scenario:** the old user's refresh token stays in Credential Manager indefinitely. No later action deletes it, because "Sign out" and "Remove" only touch the account now in the registry. Its server login session also stays live until it expires, and it shows up in the old user's session list.
  - **Same user:** with the same user the key is overwritten, but the previous server session is still never revoked.
  - **Fix:** in `login()`, before storing the new credential:
    - if the record has an account, run the existing logout path (`session(server_id)?.logout()` best effort);
    - delete that account's key, even when the user id is the same, after the revoke.
    - Add a test: login as A, login as B, then assert that A's key is gone.

### Low

- **L1 Concurrent commands after a restart can resume two `Session`s for one server, each with its own refresh lock.** `apps/desktop/src-tauri/src/silo.rs:280-291`.
  - **Cause:** `session()` releases the `sessions` lock between the lookup and the insert. Two commands that arrive together each `resume()` and each refresh with the same refresh token, defeating single-flight; the second insert wins.
  - **Trigger:** `StrictMode` double-runs the Profiles effect in dev, and Phase 5 screens will issue parallel commands.
  - **Impact today:** harmless, because upstream refresh has no reuse detection, so both refreshes succeed (one wasted request). If the server ever adds rotation reuse detection, this signs the user out.
  - **Fix:** hold the `sessions` lock across the lookup, the `resume` (synchronous) and the insert (read the registry first, then lock), or use `entry().or_insert_with`.

- **L2 `Registry::load` treats every read failure as "no file", and the next save overwrites the real file.** `crates/silo-api/src/registry.rs:51-63`.
  - **Problem:** `read_to_string(...).ok()` maps a sharing violation (AV scanner, OneDrive/roaming redirection), a permission error or invalid UTF-8 to `Registry::default()`. No `.bad` copy is made, contrary to the doc comment. The next `add`/`login` writes an empty or near-empty registry over it, losing every server and orphaning the stored credentials.
  - **Also:** `version` is never checked, so a newer-format file is silently down-converted on save.
  - **Fix:** return `Self::default()` only for `ErrorKind::NotFound`. Read bytes and treat UTF-8/JSON errors as corrupt (`.bad`). For other I/O errors, start in memory with persistence disabled (or retry), and log either case.

- **L3 Documentation drift in silo-api.md.**
  - §13 (`docs/research/silo-api.md:677`) cites `internal/api/apiv2/auth.go` L240-247. The file is `internal/apiv2/auth.go`; the lines are correct (`loginProblem`).
  - §1.4's Granary note (`:90`) still says `X-Silo-Client: Granary`, while code, ADR-0004 and §13 say `Granary for Windows`.
  - **Fix:** correct the path; update or strike the §1.4 note.

- **L4 Login refusals other than 401 all show the generic "You are not allowed to do this."** `crates/silo-api/src/error.rs:103-117`.
  - **Upstream:** login 403s are `local_login_disabled`, `password_expired`, `not_permitted`, and `permission_denied` for a disabled account (`internal/apiv2/auth.go@6791aac` L90-98; `serviceProblem` → `TypeForStatus(403)`). The `account_disabled` arm is never produced by login; upstream uses it for identity linking.
  - **Fix:** map `local_login_disabled`, `password_expired` and `not_permitted` to specific codes, and `permission_denied` on `login` to `account_disabled`.

- **L5 `Retry-After` is never shown to the user.** `apps/desktop/src/lib/errors.ts` passes `retryAfter`, but `errors.rate_limited` and `errors.unavailable` (en/de) have no `{{retryAfter}}`.
  - **Why it matters:** the fifth wrong PIN locks the profile for five minutes, and every check (even with the right PIN) is refused with 429 + `Retry-After` (OAS `verifyProfilePIN` description). The user only sees "Wait a moment".
  - **Fix:** use a variant with the seconds or minutes when `retryAfter` is present.

- **L6 A redirect error does not tell the user where to go.** `error.rs:38-39` keeps `location`, but the UI only shows `errors.redirected` ("Enter the final address instead").
  - **Fix:** pass the redirect target origin (not the full URL) as a `CommandError` field and show it.

- **L7 A host:port without a scheme on a plain-http server gives only "A secure connection could not be established."** `crates/silo-api/src/lib.rs:36-40` defaults to `https://`.
  - **Mechanism:** a ClientHello sent to an HTTP listener fails with a non-certificate rustls error → `TlsProblem::Other` → `tls_error`.
  - **Fix:** for `TlsProblem::Other` (or a rustls `InvalidMessage`) on an input that had no explicit scheme, add the hint "If this server only offers http, enter http:// explicitly". Do not auto-fall back to http.

- **L8 Surface wider than used.** Prompt: "typed client for exactly the endpoints the app uses"; least-privilege capabilities.
  - `SiloApi::current_user` / `getCurrentUser` is called only from tests, never by the host.
  - `servers_activate` is registered, allowed in `capabilities/main.json` and wrapped in `lib/silo.ts`, but no screen calls it.
  - **Fix:** either use them (for example `current_user` to validate a resumed session, `activate` on Open) or drop them for now.

- **L9 Accessibility and navigation basics on the new screens.**
  - `Profiles.tsx` has no way back to the server list. Any error (network, M1) leaves the user on a dead page.
  - The selected profile is shown only by a border colour, with no `aria-pressed`.
  - "Loading…" texts are not in a live region.
  - Home's **Remove** (logout + delete) runs without confirmation.
  - **Fix:** add a back/"Servers" button to Profiles. Add `aria-pressed` on the profile tiles and `role="status"` on loading text. Confirm Remove.

- **L10 The Rust side of `auth://revoked` is untested.** `apps/desktop/src-tauri/src/silo.rs:370-382` (`emit_if_revoked`).
  - **Gap:** `revoked_session_clears_account` checks the state change, and the vitest flow fakes the event. Nothing checks that the commands emit it.
  - **Fix:** a small test with `tauri::test::mock_app()` that revokes via the mock, calls `libraries_list` and asserts that the event is received.

- **L11 `request_origin` mismatch is advisory only (design note, uncertain severity).** `crates/silo-api/src/lib.rs:83`, `AddServer.tsx`.
  - **Contract:** "A client compares it with its configured server origin before sending credentials" (`internal/apiv2/system.go@6791aac` L26).
  - **Granary today:** shows an info-tone note at probe time, then lets the user sign in. It never re-checks before login or refresh.
  - **Assessment:** acceptable for reverse-proxy setups without trusted-proxy config, where a mismatch is normal.
  - **Suggestion:** use warning tone instead of info, and record the decision in ADR-0004.

## Resolution (orchestrator, before Gate 4)

| Finding | Status | Change |
|---|---|---|
| H1 | Fixed | `SiloState::add` signs out at the old origin (revoke + delete credential + clear account) when a known `server_id` appears at a new origin; `Registry::upsert` also drops the account on an origin change (defence in depth). Tests: `silo::tests::new_address_signs_out_and_forgets_credential`, `registry::tests::upsert_dedupes_by_server_id_and_drops_account_on_new_origin`. |
| M1 | Fixed | `Session::call_account` sends only the bearer token; `profiles`, `verify-pin`, `current_user` and logout use it. A `profile_verification_required` answer drops the stored profile token. Mock now validates supplied profile tokens on every route like upstream. Test: `session::tests::stale_profile_token_does_not_block_account_calls`. |
| M2 | Fixed | `SiloState::login` ends the previous session (revoke, delete credential, clear account) before signing in. Test: `silo::tests::signing_in_again_ends_previous_session`. |
| L2 | Fixed | `Registry::load` keeps unreadable / newer-version files as `servers.json.bad` instead of overwriting them. |
| L3 | Fixed | §13 path corrected to `internal/apiv2/auth.go`; §1.4 note now says `Granary for Windows`. |
| L1, L4–L11 | Deferred | Tracked in PROGRESS.md "Known issues" (L9 navigation is superseded by the Phase 5 shell). |
