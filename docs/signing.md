# Code signing

**Status: not configured.** Granary 0.1 installers are unsigned (ADR-0005). Nothing in the repository holds a
certificate, key or signing secret. This page describes the options and the steps to turn signing on later.
Sources and dates are in [research/packaging-and-licensing.md](research/packaging-and-licensing.md) §4.

## What unsigned means for testers
- **SmartScreen:** the installer shows "Windows protected your PC". Click **More info → Run anyway**. Reputation
  resets with every unsigned version, so this appears for every release.
- **Smart App Control** (Windows 11, on by default on some clean installs) blocks unsigned executables with no
  reputation outright. Testers must turn it off in Windows Security → App & browser control, or wait for a signed
  build. On some Windows versions it cannot be turned back on without resetting Windows, so check that before
  telling testers to turn it off.
- Enterprise policy can block unsigned software entirely.
- Integrity still holds without a signature: every installer ships with a `.sha256` file (`scripts/release-build.ps1`),
  and the release is only reachable to people with access to the private repository.

## Options

| Option | Cost | CI-friendly | Notes |
|---|---|---|---|
| **Azure Artifact Signing** (formerly Trusted Signing) | from $9.99/month, needs a paid Azure subscription | Yes | Keys stay in Microsoft HSMs. Validates organisations and individuals. Short-lived certificates, so a timestamp is mandatory. Recommended before any public build. |
| OV certificate on a hardware token or cloud HSM | varies by issuer (not researched) | Only with a cloud HSM | Since 2023-06-01 private keys must live on an HSM or token. |
| EV certificate | varies by issuer, above OV | Only with a cloud HSM | Since 2024 EV gives **no** instant SmartScreen reputation, so it brings nothing extra here. |

Both OV and EV build SmartScreen reputation the same way, through download volume over time.

## Turning it on (Artifact Signing)
1. Create the Artifact Signing account, finish identity validation, and create a certificate profile.
2. Install the signing tool on the build machine or runner: `cargo install artifact-signing-cli`.
3. Add a `signCommand` to `apps/desktop/src-tauri/tauri.conf.json`, under `bundle.windows`:
   ```json
   "signCommand": "artifact-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <profile> -d Granary %1"
   ```
   Tauri runs it for `granary.exe` and for the NSIS installer. `%1` is the file to sign.
4. Sign `libmpv-2.dll` too. Check whether the bundler signs resources. If it does not, sign the DLL in
   `vendor-bin/libmpv/` before `tauri build`, in `scripts/release-build.ps1`.
5. In CI, give the runner an Azure identity through GitHub OIDC (federated credentials), not a stored secret. Add
   `id-token: write` to `release.yml` permissions.
6. Never change a file after it has been signed. The SHA-256 file is written after signing, as it is today.
7. Check the result: `Get-AuthenticodeSignature <file>` must report `Valid` for the exe, the DLL and the installer.

Alternative for a certificate in the Windows certificate store: set `certificateThumbprint`, `digestAlgorithm: "sha256"`
and `timestampUrl` in `bundle.windows` instead of `signCommand`. Tauri then calls signtool.

## Updater signatures are separate
A future Tauri auto-updater needs its own minisign key (`TAURI_SIGNING_PRIVATE_KEY`). That is independent of
Authenticode signing. It is not used in 0.1, which only links to the releases page (ADR-0005).
