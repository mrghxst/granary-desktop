---
name: fetch-libmpv
description: Download the pinned libmpv dev build, verify its SHA-256 and extract it to vendor-bin/libmpv (with a generated MSVC mpv.lib). Use before any build that links libmpv, or when bumping the libmpv pin.
---

# fetch-libmpv

Run from the repo root (Windows PowerShell 5.1 or pwsh 7):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-libmpv.ps1          # idempotent
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-libmpv.ps1 -Force   # re-download
```

Result: `vendor-bin/libmpv/{libmpv-2.dll, mpv.lib, mpv.def, include/mpv/*.h, .pinned}`. Exit 0 = ready.

## Bumping the pin
1. `curl -s "https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases?per_page=1"` — pick the
   `mpv-dev-x86_64-v3-<date>-git-<sha>.7z` asset; copy its `digest` (sha256) and `size`.
2. Edit `scripts/libmpv.pin.json` (tag, asset, size, sha256, mpv_commit).
3. Run the script with `-Force`, then update `THIRD_PARTY_NOTICES.md` (mpv/FFmpeg versions: run the app and read
   `mpv-version` / `ffmpeg-version` from the log).
4. The CI cache key is the sha256, so CI refetches automatically.
