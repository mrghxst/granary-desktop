# PROGRESS
Last updated: 2026-11-06T13:45:00+01:00 | Current phase: 2 | Last commit: (see git log)

## Phases
- [x] 0 Preflight & private repo — DONE (commit a0e7d04) — gate: gh repo view visibility=PRIVATE, skeleton pushed, PROGRESS.md + CLAUDE.md present
- [x] 1 Research & ADRs — DONE — gate: 5 research docs with Sources, ADR-0001..0006, reviewer docs/reviews/phase-1.md PASS WITH FINDINGS, all 6 medium + 15 low resolved
- [ ] 2 Risk spike: mpv + overlay + HDR
- [ ] 3 Scaffold, CI, project skills
- [ ] 4 Silo connectivity & auth
- [ ] 5 UI (web-app parity)
- [ ] 6 Native playback & progress sync
- [ ] 7 Display & audio (auto-HDR, refresh match, passthrough)
- [ ] 8 Polish
- [ ] 9 Packaging & release workflow
- [ ] 10 Final verification & handoff

## Current step
Phase 2: spike app in spikes/mpv-overlay (Design A: GranaryVideo child HWND under transparent WebView2).

## Decisions (link ADRs)
- [ADR-0001](docs/adr/0001-ui-strategy.md) UI: own React UI with Silo design tokens (option C).
- [ADR-0002](docs/adr/0002-player-integration.md) Player: in-process libmpv (libmpv2-sys 4.0.1) via `wid`.
- [ADR-0003](docs/adr/0003-window-overlay-architecture.md) Window: single Tauri window, video child HWND under WebView2; fallback owned overlay.
- [ADR-0004](docs/adr/0004-http-auth-token-storage.md) Rust-side HTTP, tokens in Credential Manager, origin-scoped credentials.
- [ADR-0005](docs/adr/0005-packaging-licensing-updates.md) shinchiro v3 libmpv pinned + private mirror, NSIS per-user, no auto-update.
- [ADR-0006](docs/adr/0006-display-refresh-hdr-restore-policy.md) Refresh/HDR policy and restore marker.
- pnpm 12.18.0 via corepack shim installed to `%APPDATA%\npm` (corepack enable into Program Files needs admin).
- Toolchain (Phase 0): git 2.53.0, node v24.18.0, pnpm 12.18.0, rustc 1.99.1 (host x86_64-pc-windows-msvc),
  VS Build Tools 2022 17.14 (MSVC 14.44.35207, Windows SDK 10.0.26100), gh 2.103.3, 7-Zip 26.01, ffmpeg 8.1.2,
  WebView2 157.0.4341.42. No gitleaks/PresentMon/pwsh 7/python (Store stub only) on the host.
- gh CLI is not logged in; gh calls use the existing Git Credential Manager GitHub credential via a per-command
  `GH_TOKEN` (never printed or stored). Commits use repo-local identity `mrghxst <mrghxst@users.noreply.github.com>`.
- No `claude` CLI on PATH (desktop app), so code-intelligence plugins could not be installed; built-in skills
  (code-review, security-review, simplify, run) are used instead.

## Blocked / needs human
- No `.env.local` present: no live Silo server credentials, no TEST_MEDIA_DIR, no DISPLAY_NOTES. Live tests will be
  skipped/mocked; synthetic ffmpeg fixtures used instead.
- Test machine is a laptop (Intel Arc 140V, one HDR-capable internal panel 60/120 Hz): multi-monitor, VRR, OLED
  desktop monitor and AV-receiver checks go to MANUAL_TEST_CHECKLIST.md. PresentMon not installed (independent flip
  cannot be measured automatically).
- Distribution to third parties requires a complete libmpv dependency source set (ADR-0005 known gap).

## Known issues

## Log (newest first, one line per completed step)
- 2026-11-06 Phase 1 DONE: R1–R5 research (subagents), ADR-0001..0006, reviewer pass + fixes, libmpv pinned/fetched (sha256 verified), fixtures generated.
- 2026-11-06 Phase 0 DONE: tools installed (winget rustup/gh/VS Build Tools), repo PRIVATE, skeleton pushed (a0e7d04), obsolete branch claude/blissful-planck-27cqsw deleted at owner's request.
- 2026-11-06 Phase 0: skeleton files created (.gitignore, .gitattributes, LICENSE from gnu.org, .env.example, README, CLAUDE.md, PROGRESS.md, docs/).
