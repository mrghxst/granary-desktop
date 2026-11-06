---
name: verify
description: Run Granary's full local check suite (cargo fmt --check, clippy -D warnings, cargo test, TS typecheck, eslint, prettier, vitest, frontend build, secret scan). Use before every commit that closes a step, before every gate, and to reproduce CI locally.
---

# verify

From the repo root (Windows PowerShell 5.1 or pwsh 7):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1            # normal
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1 -Install   # fresh clone / after lockfile change
```

- Each step prints `==> <name> : <command>` and `<== <command> -> exit <code>`; a summary table follows.
- Exit 0 and `VERIFY OK` = green. Any non-zero step → `VERIFY FAILED: <steps>` and exit 1.
- The script calls `scripts/fetch-libmpv.ps1` first (idempotent); pass `-SkipFetch` if `vendor-bin/libmpv` is
  already prepared (CI does this after its cache step).
- CI (`.github/workflows/ci.yml`) runs the same script: `scripts/verify.ps1 -Install -SkipFetch`.

When reporting a gate, quote the command and its exit code, e.g. `scripts/verify.ps1 → exit 0`.

Common fixes:
- `rustfmt` failed → `cargo fmt --all`.
- `prettier` failed → `pnpm --filter desktop format`.
- `clippy` lint drift → toolchain is pinned in `rust-toolchain.toml`; fix the code, do not allow the lint globally.
