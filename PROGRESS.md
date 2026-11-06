# PROGRESS
Last updated: 2026-11-06T10:20:00+01:00 | Current phase: 0 | Last commit: (pending)

## Phases
- [ ] 0 Preflight & private repo
- [ ] 1 Research & ADRs
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
Phase 0: tool installation (rustup, gh, VS Build Tools via winget) and skeleton commit.

## Decisions (link ADRs)
- pnpm 12.18.0 via corepack shim installed to `%APPDATA%\npm` (corepack enable into Program Files needs admin).

## Blocked / needs human
- No `.env.local` present: no live Silo server credentials, no TEST_MEDIA_DIR, no DISPLAY_NOTES. Live tests will be
  skipped/mocked; synthetic ffmpeg fixtures used instead.

## Known issues

## Log (newest first, one line per completed step)
- 2026-11-06 Phase 0: skeleton files created (.gitignore, .gitattributes, LICENSE from gnu.org, .env.example, README, CLAUDE.md, PROGRESS.md, docs/).
