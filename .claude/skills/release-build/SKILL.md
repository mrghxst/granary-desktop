---
name: release-build
description: Build the Granary NSIS installer (per-user) with the pinned libmpv bundled, write its SHA-256, and optionally test install/uninstall. Use for Phase 9 packaging, before tagging a release, or to reproduce the release workflow locally.
---

# release-build

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-build.ps1           # build
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-build.ps1 -Install  # + pnpm install --frozen-lockfile
```

Output: `target/release/bundle/nsis/Granary_<version>_x64-setup.exe` and `<installer>.sha256`; the script prints
path, size and hash. It fetches/verifies libmpv first (`scripts/fetch-libmpv.ps1`).

Silent install/uninstall test (per-user, no admin):

```powershell
& target/release/bundle/nsis/Granary_<version>_x64-setup.exe /S
Test-Path "$env:LOCALAPPDATA\Granary\granary.exe"          # install dir is set by the NSIS template; check the log
& "$env:LOCALAPPDATA\Granary\uninstall.exe" /S
```

Releases: push a `v<version>` tag → `.github/workflows/release.yml` builds and attaches the installer to a **draft**
release in the private repo. Never publish a release; the owner reviews and publishes manually.
Before tagging: bump `apps/desktop/package.json` version (tauri.conf.json reads it), update CHANGELOG and
`THIRD_PARTY_NOTICES.md`.
