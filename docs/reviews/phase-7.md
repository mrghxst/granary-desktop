# Review: Phase 7 Display & audio

- Date: 2026-11-07
- Scope: commits `01a6171`, `47e3f7f`, `a7778b3`, plus the `crates/display` code from `ff57e2a`:
  - `crates/display/src/{policy,session,marker,win,lib}.rs` and `examples/{display_roundtrip,hdr_probe}.rs`;
  - `apps/desktop/src-tauri/src/playback/{output,host,controller,commands}.rs`, `settings.rs`, `lib.rs`;
  - `apps/desktop/src/routes/Settings.tsx`, `lib/settings.ts`, `i18n/{en,de}.json`;
  - ADR-0006 (incl. Amendment 1), `MANUAL_TEST_CHECKLIST.md`, `docs/evidence/phase-7/`, `scripts/output-evidence.mjs`,
    `docs/research/windows-display-audio.md` (R4).
- Checked against: `prompt.md` PART B (Non-negotiable rules, Phase 7, Gate 7), `CLAUDE.md`, `PROGRESS.md`,
  ADR-0002, ADR-0003, ADR-0006, R4, and upstream `.ref/silo-server@6791aac` (`internal/playback/plan_v3.go`,
  `capabilities_v3.go`, `protocol_v3.go`; read as data only).
- Reviewer: fresh subagent (independent; did not write this code).
- Line numbers refer to `a7778b3` (`git show a7778b3:<path>`). The working tree had uncommitted Phase 8 edits
  (`lib.rs`, `host.rs`, `commands.rs` and others) while this review ran, so current line numbers may differ.

**Verdict: PASS WITH FINDINGS.** No High finding. The core is sound:
- the HDR decision follows the spec exactly (`policy.rs:132-144`): already on → never toggled; Off / content-only /
  always; unsupported or `wideColorEnforced` panels are never toggled;
- the restore marker is written atomically *before* each change, the first recorded original wins, an unparsable
  marker is kept, a live owner's marker is skipped, and startup repair runs before the window is created;
- the HDR switch reads back through both DisplayConfig and the DXGI colour space, and a failed read-back undoes the
  half-applied toggle and falls back to tone-mapped SDR (`session.rs:104-126`);
- SDR reference white is explicit per monitor, target peak is the per-monitor override or DXGI `MaxLuminance` read
  after the settle, and options are reset to neutral when nothing is decided, so a reused mpv core does not keep the
  previous file's state;
- audio mapping to mpv is correct (`output.rs:106-127`), and audio settings apply live;
- `cargo test` for `granary-display` (33) and `granary --lib` (53) is green, the hardware round trip exited 0 and the
  in-app evidence run shows a real HDR switch, an exact-multiple refresh switch and both restores.

Seven Medium findings should be fixed (or explicitly accepted in PROGRESS.md) before Gate 7 is closed:
- **M1:** the HDR decision uses the source file's dynamic range, not what the plan delivers.
- **M2:** exit restore can miss the 5 s budget, because the server stop runs first.
- **M3:** a leftover marker that startup repair could not finish is overwritten by the next playback.
- **M4:** the panic hook restores displays on *caught* panics, while playback continues.
- **M5:** display settings changed during a binge are ignored while the output is held.
- **M6:** a new HDR title on battery (and every HDR switch) blocks the host task for up to ~13 s.
- **M7:** the Gate 7 checklist misses three required items.

## Phase 7 requirement coverage
| Requirement | Status | Evidence |
|---|---|---|
| HDR already on → output HDR directly, no toggling | Met | `hdr_decision` L133-135 `AlreadyOn`; test `hdr_already_on_is_never_toggled` (`output.rs:426`); checklist A.2 |
| HDR off → setting Off / content only (default) / Always switches before the first frame and restores | Met (M1, M6) | `session.rs:104-126` runs in `before_load` before `loadfile` (`controller.rs:330-339`); evidence `checks.json` "HDR content: Windows HDR switched on", restored after the grace. Decision uses the source range, not the delivered one (M1) |
| SDR reference white follows each display's SDR level | Met (L8) | `mpv_options` L299-306 from packet 11; round trip shows `hdr-reference-white=200`. Not refreshed when the SDR slider moves during a held binge (L8) |
| Peak and black level from DXGI per display, per-monitor override | Met (L5) | `mpv_options` L277-298; `settings.rs:169-171` validates 80..10000. Black level has no DXGI source (DXGI exposes `MinLuminance`, unused) — the override or `auto` is used. Settings shows the pre-toggle SDR peak (L5) |
| Content within peak untouched, brighter tone-mapped, SDR mapping without HDR | Met | `target-peak` + mpv defaults (`tone-mapping=auto`, `hdr-compute-peak=auto`, R4 §5.2); `target-colorspace-hint=auto`; `stats-hdr10.json` `gamma=pq max-luma=1266` |
| Refresh: VRR → exact multiple near current → display-resample | Met (L1, L3, L4) | `refresh_plan` L188-262; VRR is user-declared (no API, R4 §4.2). The `0.9·R` rule rejects R4's own typical cases (L1) |
| Opt-in "Match content exactly" down to 23.976…59.94 | Met (L4) | `refresh_plan` MatchContent branch (lowest exact multiple); test `per_monitor_overrides_and_match_content`. Fractional rates rely on the unverified rational path (L4) |
| Always restore afterwards | Partly (M2, M3, M4, L7) | Stop + 10 s grace, monitor move, exit, panic hook, startup repair. Gaps in M2/M3/M4 |
| Monitor the window is on; re-evaluate on move | Met (M6) | `gdi_name_for_window`; `WindowMoved` → 400 ms settle → `monitor_moved` (`host.rs:462-466`, `housekeeping` L491-503); test `moving_to_another_monitor_restores_the_first`. Not exercised on hardware |
| Dolby Vision P5/P7/P8 per R4; DV profile in stats | Met | mpv/libplacebo defaults; stats show plan profile, container profile/level and "dolby-vision-rpu active" |
| Audio default: WASAPI shared, stereo downmix with normalisation | Met | `audio_options` defaults (`audio-exclusive=no`, `audio-channels=auto-safe`, `audio-normalize-downmix=yes`) |
| Output device selection, following the Windows default | Met | `audio-device=auto` follows the default (R4 §7); `player_audio_devices` list |
| Opt-in exclusive mode and passthrough (AC3, E-AC3, DTS, DTS-HD, TrueHD) | Met (L8) | `audio-exclusive`, `audio-spdif` from the codec list; `content_of` maps `dts`/`dca` to `dts-hd`. `audio-fallback-to-null` hides an exclusive-mode failure (L8) |
| Channel layout selection | Met | `auto-safe` / `5.1,stereo` / `7.1,5.1,stereo` / `stereo` |
| Settings persist and apply live where mpv allows | Partly (M5) | Persisted and normalised in `settings.rs`; audio applies live (`host.rs:453-461`). Display options never re-apply live, and not on the next video either while held (M5) |

## Gate 7 coverage
| Requirement | Status | Evidence |
|---|---|---|
| Unit tests for mode selection and restore logic | Met | `crates/display` policy/session/marker tests (33 total) incl. 25 fps/120 Hz, 23.976/165 Hz, VRR, MatchContent, marker merge, failed HDR read-back, repair with live owner; `output.rs` tests L376-520 (reuse, SDR-after-HDR restore, move, passthrough, no controller); host test `display_changes_are_held_after_stop_and_restored_at_shutdown` |
| HDR toggle + refresh switch round trip on this machine that restores | Met (L4) | `docs/evidence/phase-7/display-roundtrip.txt`: exit 0, HDR off→on→off, 60→120→60 Hz, restore verified by re-query, marker removed. The applied path (rational SetDisplayConfig vs CDS) is not logged, and 120 Hz is an integer rate (L4) |
| `MANUAL_TEST_CHECKLIST.md`, OLED desktop first, then TV/receiver | Partly (M7, L1) | A.1-A.12 and B.1-B.6 exist. Missing HDR10+, near-black gradients/banding, windowed vs fullscreen for HDR10/DV (M7). A.6 expects 119.88 Hz, which the policy never picks (L1) |

### Checks run (this session)
- `cargo test -p granary-display --locked` → **exit 0**, 33 passed.
- `cargo test -p granary --lib --locked` → **exit 0**, 53 passed (working tree, including the uncommitted Phase 8
  edits). All 9 `playback::output::tests` and the host display hold/restore test passed.
- i18n parity script (scratchpad, node) → en 272 / de 272 keys, no missing keys, no placeholder mismatch.
- `git show a7778b3:<path>` reads of every file in scope; `docs/evidence/phase-7/*` read (round trip, output log,
  checks.json, stats JSON, monitors before/after).
- `grep` of `.ref/silo-server` (data only) for how plans set `effective_recipe.dynamic_range` and when
  `nativeOutputHDRV3` allows HDR (M1).
- `grep` for `unwrap()`/`expect(` and `TODO` in the Phase 7 Rust → none without justification.
- Not run (per instructions): the app, `display_roundtrip`, `hdr_probe`, `scripts/verify.ps1`. No display setting was
  changed.

## Findings

### High
None.

### Medium

- **M1 The HDR decision uses the source file's dynamic range, not what the plan delivers.** `output.rs:51-73`
  (`content_of`), `crates/silo-api/src/caps.rs:184-206` (`output`).
  - **Mechanism:**
    - `content_of` takes `source.dynamic_range`. The plan's `source` is the descriptor of the *original file*; what
      is delivered is `effective_recipe.dynamic_range`, which the client does not parse.
    - Upstream, a transcode plan is always SDR: `plan_v3.go:1458` sets `plan.EffectiveRecipe.DynamicRange =
      DynamicRangeSDRV3` (with a server-side tone map). Remux/direct keep the source range (`plan_v3.go:2409`).
    - `caps.rs` sends `hdr_details` only when Windows HDR is *already* on at negotiation. With the default
      "HDR content only" and HDR off, the server therefore sees an SDR-only output; `nativeOutputHDRV3`
      (`capabilities_v3.go:353-394`) then refuses native HDR for every non-`original_http` plan.
  - **Failure scenarios:**
    - A child profile's quality limit (or an unsupported codec) forces `server_transcode_hls` of a 4K HDR10 file.
      Windows HDR is switched on (black flash), `target-peak=1266` and the HDR reference white
      are applied, and the server-tone-mapped SDR stream is shown on an HDR desktop. It works visually only because
      mpv maps BT.709 into PQ, but the toggle is pointless, and the same happens on every episode.
    - The reverse: with HDR off and "HDR content only", an HDR remux plan is downgraded to an SDR transcode by the
      server, because the capability says "no HDR output", even though Granary would have switched HDR on.
  - **Fix:** parse `effective_recipe.dynamic_range` (document it in `silo-api.md` with the upstream path@SHA) and
    decide on the delivered range, falling back to `source` only when the recipe is absent. When the HDR mode is not
    Off and the panel is `hdr_supported`, declare HDR output capability in `caps.rs` (Granary will switch it on).
    Add an output test with a transcode plan of an HDR source.

- **M2 Exit restore can miss the 5 s budget, because the server stop runs first.** `host.rs:470-476`
  (`Cmd::Shutdown`), `controller.rs:34` (`STOP_WAIT = 4 s`), `controller.rs:882-894` (`stop`), `lib.rs:267-273`.
  - **Mechanism:** `Shutdown` awaits `self.stop()` (up to `STOP_WAIT` = 4 s for the server stop) *before*
    `run_blocking(output.release())`. The restore itself takes ≥1.6 s on this machine: packet 16 is accepted but
    not applied, so `set_hdr(false)` polls 1.5 s before falling back to packet 10 (`win.rs:417-428`); the round
    trip measured `RESTORE in 1643 ms`. `shutdown_blocking(5 s)` then gives up and the process exits.
  - **Failure scenario:** the server is unreachable (VPN dropped, Wi-Fi gone) while an HDR title plays on a monitor
    whose HDR Granary switched on. The user closes the window: 4 s server wait + ~1.6 s restore > 5 s, the process
    ends during or before the restore, and the desktop stays in HDR (and/or the switched refresh) until Granary is
    started again. Checklist A.11 ("restored immediately") would fail.
  - **Fix:** in `Shutdown`, stop mpv and release the display first (they are local and bounded), then deliver the
    server stop with the remaining budget. Alternatively give the release its own budget outside the 5 s.

- **M3 A leftover marker that startup repair could not finish is overwritten by the next playback.**
  `session.rs:62-68` (`DisplaySession::new`), `session.rs:75-87` (`record`), `marker.rs:66` (`save`),
  `marker.rs:223-247` (`repair_at_startup_with`).
  - **Mechanism:** a repair that fails keeps the marker on disk (correct). But `DisplaySession::new` starts with an
    empty `RestoreMarker::for_current_process()` and never loads the existing file; the first `record` merges into
    the empty in-memory marker and `save` replaces the file. After a successful `restore`, the file is removed.
  - **Failure scenario:** Granary crashes (or is killed) while HDR is switched on for monitor B. B is unplugged or off
    at the next start, so the repair of B fails and the marker is kept. The user plays an HDR title on monitor A:
    the marker is rewritten with A only, and deleted after the grace. When B is plugged back in, nothing remembers
    that its original state was HDR off; it stays in HDR for good (Windows persists the advanced-colour toggle).
  - **Fix:** at session creation (or on the first `record`), load the existing marker, adopt its entries under the
    current owner and merge (first original wins). Add a session test: leftover entry for B + record for A → both
    entries saved; restore of A keeps B.

- **M4 The panic hook restores displays on caught panics, while playback continues.** `lib.rs:24-39`
  (`restore_displays_now`), `lib.rs:175-178` (hook); `Cargo.toml` `[profile.release]` (no `panic = "abort"`);
  `redact.rs:237` (`catch_unwind`).
  - **Mechanism:** the panic hook runs for *every* panic, including ones that are caught: panics in spawned tokio
    tasks (`controller.rs:737` progress task, `controller.rs:916` stop task, relay tasks, events socket) are caught
    by the runtime, and `redact.rs` catches its own by design. The hook loads the marker, restores HDR and refresh
    and deletes the marker. The process keeps running; `Output` still believes it holds the changes.
  - **Failure scenario:** a relay connection task panics during HDR playback (any bug in a spawned task). Windows HDR
    is switched off mid-film; mpv keeps `target-peak=1266` and `hdr-reference-white=200`
    against an SDR output. The next episode reuses the held options (`output.rs:241-247`) without switching HDR back
    on, and the marker is gone, so a later crash cannot be repaired either.
  - **Fix:** set `panic = "abort"` in the release profile (the hook then always precedes process death), or restore
    in the hook only for panics on the main thread / host task. Either way, keep the marker until the restore is
    verified.

- **M5 Display settings changed during a binge are ignored while the output is held.** `output.rs:241-247` (reuse
  check), `host.rs:453-461` (`SettingsChanged`), `settings.rs:278`; en `settings.audioLiveHint`.
  - **Mechanism:** the reuse key is the monitor name plus `Content`; neither the settings nor the resulting `Policy`
    is compared. `SettingsChanged` re-applies audio options only.
  - **Failure scenarios:**
    - During an HDR binge the user sets the peak override from 1266 to the calibrated 800 nits (checklist A.3: "stop,
      play again; `target-peak` follows"). The next episode starts within 10 s, the output is reused and
      `target-peak` stays 1266. The UI hint promises "display settings from the next video". A.3 passes only if the
      user waits more than 10 s.
    - The user switches HDR mode to Off, or ticks VRR, between episodes: the switched HDR/refresh state is kept for
      the rest of the binge.
    - `target-peak`, `target-contrast`, `hdr-reference-white` and `video-sync` are runtime properties in mpv, so the
      spec's "apply live where mpv allows" is not met for them.
  - **Fix:** include the `Policy` (or the settings hash) in the reuse key; on `SettingsChanged`, re-run the decision
    for the active monitor and set the live display options (a mode/HDR change can stay "next video" if documented).

- **M6 A new HDR title on battery (and every HDR switch) blocks the host task for up to ~13 s.** `session.rs:106-126`,
  `session.rs:19-25` (`hdr_timeout` 10 s, `settle` 1.5 s), `win.rs:417-428`, `output.rs:319-324` (`run_blocking`),
  `controller.rs:336-339`, `host.rs:481-503`.
  - **Mechanism:** the HDR switch runs inside `block_in_place` on the single host task: packet 16 poll (1.5 s on
    this machine) + `wait_for_hdr` (10 s timeout) + settle (1.5 s on success). Commands queue behind it. ADR-0006
    Amendment 1 documents that Windows refuses HDR on battery and that the call still "succeeds".
  - **Failure scenarios:**
    - On battery with the default "HDR content only", every HDR title that is not a reused next episode freezes for
      ~11.5 s before the first frame; Back/Stop pressed meanwhile is not handled until it ends.
    - Moving a playing window to a second HDR-capable monitor runs the same switch from `housekeeping`: video plays
      on, but pause, seek, volume and Stop do not respond for up to 13 s.
    - `Shutdown` queued behind it loses most of its 5 s budget (compounds M2).
  - **Fix:** remember a failed read-back per monitor for the power state (or check `GetSystemPowerStatus` and skip
    the switch on battery with a logged reason); cut the HDR wait to ~3 s when packet 10 was used; run monitor-move
    switches off the host task and apply the options when done.

- **M7 The Gate 7 checklist misses three required items.** `MANUAL_TEST_CHECKLIST.md:79-126` (at `a7778b3`).
  - **Mechanism:** Gate 7 requires, for the OLED desktop: "HDR10, DV P5/P8 and HDR10+ windowed and fullscreen with
    Windows HDR on" and "Near-black gradients: no raised blacks, no banding". The section has no HDR10+ step, no
    near-black/banding step, and A.2/A.5 do not ask for windowed *and* fullscreen.
  - **Failure scenario:** the gate is signed off without ever checking HDR10+ dynamic metadata, raised blacks from a
    wrong `target-contrast` on OLED, or the windowed (composed) HDR path vs fullscreen independent flip — exactly the
    cases most likely to differ on the primary target.
  - **Fix:** add a step for an HDR10+ clip; a near-black gradient step (e.g. a 0-5 % ramp, with and without a black
    override; expected `target-contrast=auto` (infinite for HDR targets) on OLED); and "windowed and fullscreen" to A.2 and A.5.
    `scripts/gen-test-media.ps1` can generate the ramp.

### Low

- **L1 The `0.9·R` rule rejects R4's own typical switches, and checklist A.6 contradicts it.** `policy.rs:248`;
  ADR-0006 Decision step 3; R4 §4.3; checklist A.6.
  - R4 §4.3 lists 100/200 Hz for 25/50 fps on 120/240 Hz and 119.88/239.76 Hz for 23.976 on 144/165 Hz as the typical
    worthwhile switches. With `m.hz() >= 0.9 * r` none of them qualifies (the tests assert this at L435 and L447),
    so on common 120/144/165 Hz OLEDs Auto practically never switches and always resamples.
  - A.6 tells the tester to expect "an exact multiple (e.g. 119.88 Hz)" on 120/144/165/240 Hz monitors; on 144/165 Hz
    the policy never picks it, so a correct build looks like a failure.
  - Fix: decide the rule deliberately (e.g. `R′ ≥ 0.6·R` or "highest exact multiple ≤ R"), update the ADR, tests and
    A.6 together.

- **L2 The reuse key is the raw content, not the decision.** `output.rs:241-247`.
  - `Content` contains the range enum, the `f64` fps and the passthrough flag. HDR10 → HDR10+ or DV → HDR10 episodes,
    23.976 vs 23.976023 fps metadata, or a different audio codec under passthrough all differ, so the held state is
    restored and switched again (two black flashes) although the decision is identical.
  - Fix: compare the resulting `HdrDecision` + `RefreshPlan` (+ policy, see M5) instead of the inputs.

- **L3 The refresh plan uses the snapshot taken before the HDR toggle.** `session.rs:135`.
  - `refresh_plan(d.refresh, …)` uses `d` read before `set_hdr`. On a bandwidth-limited link (HDMI 2.0 at 4K), enabling
    HDR can lower the refresh or change the mode list; the plan then compares against a stale rate (e.g. "already
    fits 120" while the panel now runs 60). `after` (L156) is read only for `display-fps-override`.
  - Fix: re-query the display (and modes) after a successful HDR switch.

- **L4 The fractional-rate path is unverified on hardware, and the ADR overstates restore.** `win.rs:146-224`,
  `win.rs:253-312`, `win.rs:453-491`, `win.rs:495-509`; ADR-0006 "Restore guarantee".
  - The round trip used an integer 120 Hz and did not log whether `set_refresh_rational` or the CDS fallback applied,
    so 23.976/29.97/59.94/119.88 switching (MatchContent, B.1) has no evidence.
  - `set_refresh_rational` changes only `vSyncFreq` (pixel rate/hSync untouched) and reads back only the refresh,
    not the resolution, after an `SDC_ALLOW_CHANGES` apply. The CDS fallback has no read-back. GDI modes are whole Hz,
    so 23/29/59 entries misrepresent NTSC rates when DXGI enumeration fails.
  - ADR-0006 says the marker records the resolution and restore falls back to the recorded rational mode; neither is
    implemented (`MarkerEntry` has no resolution; `restore_mode` only uses the database + CDS reset).
  - Fix: log the applied path; add a fractional round trip (59.94 if offered) to the evidence or a capability skip;
    read back the resolution; align the ADR with the code or implement the fallback.

- **L5 Settings show the pre-toggle peak; out-of-range numbers are silently dropped.** `playback/commands.rs:280`,
  `Settings.tsx:133`, `Settings.tsx:368`.
  - `display_list` reports DXGI `MaxLuminance` in the current state. With HDR off this is the SDR value (500 nits on
    this panel) while playback uses 1266 after the toggle; the override hint shows the wrong reference.
  - `NumberField` ignores input outside min/max (e.g. a 50-nit peak) without feedback; the field keeps showing the
    typed value although nothing was saved.
  - Fix: label the value "current (SDR)" or omit it while HDR is off; reset or flag invalid input.

- **L6 Several evidence checks do not test what their names say.** `scripts/output-evidence.mjs:217-249`;
  `docs/evidence/phase-7/checks.json`.
  - "audio settings apply live" only checks `phase === "playing"`; the `live` stats it fetches are never compared
    (`audio-normalize-downmix` should read `no`).
  - "stats: output dynamic range" passes for HDR *or* SDR, and "stats: tone-mapping target peak" passes for `auto`.
  - "held during the idle grace (next episode)" plays no next episode and passes unconditionally when HDR cannot be
    switched.
  - Exit restore, panic restore, startup repair and monitor move have no hardware evidence (only unit tests and
    checklist steps).
  - Fix: assert the actual values; mark capability skips as skips; add an exit-restore check (close the app while
    switched, re-read the monitor state).

- **L7 `WM_ENDSESSION` is not handled.** ADR-0006 Amendment 1. A log-off or shutdown while switched leaves HDR on
  until Granary next starts (Windows persists the advanced-colour state; the refresh change is temporary). Documented
  in the amendment, but the original ADR still lists `WM_ENDSESSION`; accepted residual risk should be in PROGRESS.md.

- **L8 Smaller items.**
  - `displays()` (`win.rs:322-405`) ignores `DisplayConfigGetDeviceInfo` return codes; a transient failure of the
    advanced-colour query yields `hdr_enabled=false` on an HDR-on panel, which would then be "switched on" and later
    "restored" to off.
  - The explicit `hdr-reference-white` is captured once; moving the Windows SDR brightness slider during a held binge
    is not followed (the ADR says it is updated on monitor change only, which is met).
  - `audio-fallback-to-null=yes` hides a failed exclusive open or a wrong device: playback runs silently with only a
    log line; a notice would help (B.6 relies on it for the receiver-off case, which is fine).
  - `DynamicRange::from_mpv` is used only in tests; when the server omits `dynamic_range` and no DV profile is present,
    content is treated as SDR without consulting mpv's `video-params`.
  - `Output::holding()` is used only in tests.
