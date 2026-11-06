# Packaging and licensing research (R5)

Date: 2026-11-06. Researcher: subagent R5. Status: research only; nothing was downloaded, installed or committed.
Every claim below is backed by an entry in **Sources** at the end (S-numbers). Version facts were looked up today from official sources (GitHub API, npm, crates.io, vendor docs). Statements marked "(inference)" are my reasoning and do not come from a source.

## Summary

1. **There are two libmpv candidates now.** One is shinchiro's daily build `mpv-dev-x86_64-20261106-git-9028187538.7z` (31,801,743 bytes, GPLv3). The other is the **official mpv v0.42.0 release**, which since 2026-11-05 ships `libmpv-v0.42.0-x86_64-w64-mingw32-{gpl,lgpl}.zip` with PGP `.asc` files [S1–S8].
2. **shinchiro keeps only the newest 30 releases** (a pruning step in its workflow). Any shinchiro asset we pin disappears after about 30 days, so we must mirror it ourselves [S3].
3. **The GitHub API shows a SHA-256 `digest` for every release asset**, in the form `sha256:…` [S1, S6]. Every digest in this document was read from the API.
4. **Avoid the `-v3` builds.** They need an x86-64-v3 CPU (AVX2/BMI2/FMA), which Windows 11 does not require. Use the baseline `x86_64` build [S4, S9, S10].
5. **License verdict: compatible.** An AGPL-3.0-or-later app may ship next to a GPLv3 or LGPL libmpv. AGPLv3 §13 and GPLv3 §13 each allow combining with the other, and LGPL works fit into both. Shipping binaries means we must provide the Corresponding Source of libmpv and its FFmpeg build, plus our own source [S11–S15].
6. **Silo upstream (silo-server, silo-android, silo-apple) is AGPL-3.0-or-later and adds a "Platform Library" exception.** We may vendor `silo-server/web` (AGPL) under the AGPL. We may **not** use the Silo name, logo or icons as our identity, and we must strip or replace the brand assets in `web/public` [S20–S24].
7. **Tauri 2 (CLI 2.12.4, bundler 2.10.4, `tauri` 2.12.5).** Use NSIS with `installMode: "currentUser"` (the default), which installs to `%LOCALAPPDATA%\<productName>`. Put `libmpv-2.dll` through the `bundle.resources` map so it lands next to the exe. For WebView2 keep `downloadBootstrapper` (the default) or choose `embedBootstrapper` [S16–S19, S25].
8. **Signing:** Azure "Artifact Signing" (formerly Trusted Signing) starts at $9.99/month and needs a paid Azure subscription. Since 2024, EV certificates no longer skip SmartScreen. Smart App Control on Windows 11 **blocks unsigned executables** outright unless they have reputation [S26–S29].
9. **Updates:** the Tauri updater needs a reachable HTTPS endpoint, and the assets of a private repo need a token. Shipping a token is unacceptable, so: no auto-update for now, a manual "check for updates" link, and later a public feed (which also means publishing source, per the AGPL) [S30, S31].
10. **CI:** private repos now get *monetary* included compute: $12/month on Free, $18 on Pro. A Windows 2-core runner costs $0.010/min, so that is about 1,200 Windows minutes on Free and 1,800 on Pro. Current action majors: checkout@v7, setup-node@v7, pnpm/action-setup@v6, dtolnay/rust-toolchain@stable, Swatinem/rust-cache@v2, tauri-action@v1, softprops/action-gh-release@v3 [S32–S40].

---

## 1. libmpv Windows builds

### 1.1 shinchiro/mpv-winbuild-cmake: latest release (queried 2026-11-06)

The API (`/releases?per_page=5`) lists the latest release as tag **`20261106`**, published 2026-11-06T00:56:29Z. The tags before it are 20261105, 20261104, 20261103 and 20261102, one build per day [S1]. The release body contains only a downloads badge and a link to workflow run `42133553972` (workflow `mpv_clang.yml`, repo head `bda10a2816ce9908d419b3f4b564227330457e76`) [S1, S2].

| Asset | Size (bytes) | `digest` (from API) |
|---|---|---|
| `mpv-dev-x86_64-20261106-git-9028187538.7z` | 31,801,743 | `sha256:b5a3cc91b5dabf88ad5bb21d000834da4fd0bbda30f4584d39c082127bd62414` |
| `mpv-dev-x86_64-v3-20261106-git-9028187538.7z` | 33,062,771 | `sha256:a778b3444b0b8b6d93cb00ca49f2acf62a02d0b61249229fc9944672f80683bb` |
| `mpv-dev-aarch64-20261106-git-9028187538.7z` | 26,292,512 | `sha256:7144f07844d1e537710e18c091adc3fbde07d19e1a7ff2d6b6f0a091a22eafc3` |
| `mpv-dev-i686-20261106-git-9028187538.7z` | 29,575,474 | `sha256:1a004e872bb86ab9eccfa136d47aed438b7e942a49870bf485900653331cec93` |

Download URL pattern: `https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/20261106/<asset>` [S1].

- **Digest field:** yes. Every asset object in the REST response has a `digest` field of the form `sha256:<hex>` [S1, S6].
- **Retention, important:** the release job has a step "Pruning tags" (`# Keep latest 30 tags/releases`) that deletes every older release and tag [S3]. The API returned exactly 30 releases, the oldest being `20260926` [S1]. **A pinned shinchiro URL stops working after about 30 days.** CI must therefore not fetch it from shinchiro. Mirror the verified archive into our own storage, for example as an asset on a draft or pre-release in our repo, and check the SHA-256 again on every fetch (inference).
- **Versions built:** the file name embeds the mpv short hash `9028187538`, which resolves to `902818753849d9ad8f0096fad6ca5aeaa37113b9` (2026-11-05). That commit is **3 commits after the `v0.42.0` tag**, on master [S5, S7]. FFmpeg is git master `9f690c6a4` = `9f690c6a4c124b9fc60588860ac66ae3fc808525` (2026-11-05), the hash in the matching `ffmpeg-x86_64-git-9f690c6a4.7z` asset name. It is 3,197 commits ahead of the `n9.1-dev` tag, so it is pre-9.1 development code [S1, S5]. I inferred the FFmpeg commit from the sibling asset of the same run. Confirm it at runtime with the libmpv property `ffmpeg-version` (inference).
- **What "v3" means:** the workflow sets `-DGCC_ARCH=x86-64-v3` for the `x86_64_v3` matrix leg [S3, S4]. x86-64-v3 is the psABI microarchitecture level that adds AVX, AVX2, BMI1/BMI2, F16C, FMA, LZCNT and MOVBE on top of v2 [S9]. Windows 11 24H2 only needs roughly v2 (SSE4.2 + POPCNT) [S10]. Microsoft's supported-CPU lists include Tremont-class Atom/Celeron/Pentium Silver parts, and those lack AVX2 (inference from [S10] plus Intel's microarchitecture history). A v3 build would crash with an illegal instruction on such CPUs. **Pin the baseline `x86_64` build.** The v3 build at most gives small speedups in software paths, and for us decoding is mostly hardware-accelerated (inference).

**What the dev archive contains.** The `mpv.cmake` packaging step copies the following into `mpv-dev/` [S4]:
- `libmpv-2.dll`: libmpv. Built `--default-library=shared --prefer-static`, so FFmpeg, libplacebo, libass and the other dependencies are **statically linked into this one DLL**.
- `libmpv.dll.a`: MinGW import library.
- `include/mpv/client.h`, `render.h`, `render_gl.h`, `stream_cb.h`.

There is **no `mpv.def` and no MSVC `.lib`** in the current packaging [S4]. For the `x86_64-pc-windows-msvc` Rust target, we can either load the DLL at runtime (libloading-style), or generate a `.def` from the DLL's exports (`dumpbin /exports` or `gendef`) and then run `lib /def:mpv.def /machine:x64 /out:mpv.lib`. Validate this in the libmpv spike (inference). Current crates for reference: `libmpv2` 7.0.0 and `libmpv2-sys` 4.0.1 [S25].

**GPL or LGPL?** shinchiro builds are **GPL, specifically GPLv3**:
- `packages/ffmpeg.cmake` configures FFmpeg with `--enable-gpl --enable-version3` and enables GPL libraries such as `--enable-libx264 --enable-libx265 --enable-librubberband` [S4].
- `packages/mpv.cmake` does not pass `-Dgpl=false`. mpv's meson option `gpl` defaults to `true` ("GPL (version 2 or later) build") [S4, S7b].
- mpv's `Copyright` file says mpv as a whole is GPLv2+ by default, and LGPLv2.1+ only when built without the GPL-only files [S7b].
- `--enable-version3` moves FFmpeg to (L)GPLv3, so the combined DLL is effectively **GPLv3-or-later**. No `--enable-nonfree` flag is present [S4]. OpenSSL is built from a pinned commit of `openssl/openssl` [S4]. Since OpenSSL 3, that is Apache-2.0, which is compatible with GPLv3 (inference).

**Corresponding source for shinchiro builds:**
- mpv at `902818753849d9ad8f0096fad6ca5aeaa37113b9` and FFmpeg at `9f690c6a4c124b9fc60588860ac66ae3fc808525` [S5].
- The build recipe: `shinchiro/mpv-winbuild-cmake` at `bda10a2816ce9908d419b3f4b564227330457e76`, which includes the patches it applies (e.g. `openssl-*.patch`) [S2, S4].
- Most other dependencies (libplacebo, libass, dav1d, harfbuzz, …) are git-HEAD clones taken at build time. Their exact commits are **only in the CI logs**, not in the release [S3, S4]. This is the main compliance weakness of the shinchiro route (inference).

### 1.2 Official mpv v0.42.0 release (new; strong alternative)

mpv v0.42.0 was published 2026-11-05T00:56:42Z. The notes say "This release requires FFmpeg 6.1 or newer and libplacebo 7.387.0 or newer". New features include a **"D3D11 render API backend for libmpv"**, which matters for how we embed the video surface [S6].

Unlike shinchiro's, this release is permanent (not pruned) and every asset has a PGP signature. libmpv assets [S6]:

| Asset | Size (bytes) | `digest` |
|---|---|---|
| `libmpv-v0.42.0-x86_64-w64-mingw32-lgpl.zip` | 56,277,779 | `sha256:8e5f77430efc53ef175804ba6618527e3af0448a0dcabf04d3cc72659d1a6fce` |
| `libmpv-v0.42.0-x86_64-w64-mingw32-lgpl.zip.asc` | 228 | `sha256:7bdac86cf22b4c9825292b0e5f0fcc83f2f07fb2163f244abc81438fc8bd4863` |
| `libmpv-v0.42.0-x86_64-w64-mingw32-gpl.zip` | 65,742,725 | `sha256:b99e0d183024ae479d1b36a93fa262c0297dfb025b99715b98da377ffb6c2913` |
| `libmpv-v0.42.0-x86_64-w64-mingw32-gpl.zip.asc` | 228 | `sha256:10bd2d452a3d799f46216ecbd843542363ce469af5153b8662f74bebff1ff226` |

How it is built (CI job `mingw64-full` and `ci/build-mingw64-full.sh` at the tag) [S8]:
- **Build environment:** the container is `ghcr.io/btbn/ffmpeg-builds/win64-{gpl,lgpl}:latest`.
- **FFmpeg:** cloned at **master HEAD** (`git clone --depth=1`) and built static.
- **mpv:** built with `-Dgpl=$gpl`, `--prefer-static --default-library=shared`, and `-Dd3d11=enabled` among other options.
- **GPL-only features:** `dvda`, `dvdnav` and `rubberband` are enabled only in the GPL variant.
- **Package contents:** `libmpv*.dll`, `libmpv*.dll.a`, `LICENSE.LGPL` or `LICENSE.GPL`, `include/mpv/*.h` and `libbluray-*.jar`.
- **Platform:** baseline x86-64; there is no v3 variant.

The CI run was `41936479105`, head `b0841e8c4d156b8403dc1876c4cbd9011eb583f3`, created 2026-11-04T23:47Z [S8]. The tag object is `b985c3634ef0…`; the release notes link to that run [S6]. The FFmpeg commit is not in the asset name, so read it from `ffmpeg-version` at runtime, or from the CI log (inference).

**LGPL variant:** mpv in LGPL mode, plus FFmpeg and its dependencies from BtbN's LGPL image. Obligations are lighter than GPL (dynamic linking plus source), and nothing we need for a media-server client is missing (DVD/Blu-ray menus and rubberband are not needed) (inference from [S7b, S8]).

### 1.3 Recommendation summary (details in "Pinned libmpv recommendation")

- **Prefer:** official `libmpv-v0.42.0-x86_64-w64-mingw32-lgpl.zip`. It is a tagged release with a signature, a permanent URL, a stable mpv tag for source, LGPL, a baseline CPU target, and D3D11 enabled.
- **If we stay with shinchiro, as the main plan assumed:** pin `mpv-dev-x86_64-20261106-git-9028187538.7z` and mirror it immediately.

---

## 2. Licensing

### 2.1 Granary (AGPL-3.0-or-later) + libmpv (GPLv3 or LGPL)

- **AGPL plus GPLv3 is allowed.** AGPLv3 §13 and GPLv3 §13 each permit linking or combining with work under the other license. The GNU FAQ entry `#AGPLGPL` says: "Each of these licenses explicitly permits linking with code under the other license" [S11, S12, S13].
  - The combined work is conveyed with each part under its own license.
  - The AGPL network-interaction requirement (§13) applies to the AGPL-covered part (inference from S11).
- **LGPL is fine too.** LGPLv3 is GPLv3 plus additional permissions, and LGPLv2.1+ can be used under GPLv3+, so an LGPL libmpv imposes nothing beyond what the AGPL/GPL already require [S14].
  - Because we load `libmpv-2.dll` dynamically and the user can replace the DLL, the LGPL "Combined Work" relink condition (LGPLv3 §4(d)(1)) is met (inference from [S14]).
- **Our app's own AGPL §13 duty:** if a *modified* Granary interacts with users over a network, we must offer them its source. A desktop client mostly does not trigger this, but we will publish source anyway when we distribute (see §5) [S11, S13].
- **Shipping binaries (GPLv3/AGPLv3 §6):** object code must come with the Corresponding Source, or with a written offer valid for at least three years. Alternatively we can offer equivalent access from a designated place, which may be a third-party server, as long as we keep clear directions and **remain responsible for it staying available** [S12, S13].
  - In practice, attach the source to the release: the Granary source tarball, a "libmpv corresponding source" bundle or exact upstream tags/commits plus build recipe, and the license texts (inference).
  - Relying on shinchiro's or BtbN's repos alone is risky, because shinchiro prunes releases and its dependency commits are unpinned [S3].
- **FFmpeg's own checklist (written for LGPL use):**
  - Distribute the matching source.
  - Document the configure line.
  - Credit FFmpeg in About and the EULA.
  - Do not rename the DLLs to hide them.
  - Remove any anti-reverse-engineering clauses [S15].
  - Apply the same items to the GPL build; `--enable-gpl` makes all of FFmpeg GPL [S15].

### 2.2 Silo upstream licenses (read from `.ref/`)

- **License:** all three repos (`silo-server@6791aac`, `silo-android@ee178d8`, `silo-apple@77def09`) are **GNU AGPL v3** (`LICENSE`). Their `PLATFORM-EXCEPTION.md` files are byte-identical [S20, S21].
  - **What the exception adds** (AGPLv3 §7): permission to link or combine with "Platform Libraries" published by an OS, device or app-store vendor. Examples include the Windows App SDK. Platform Library source is not required, every other AGPL obligation still applies, and anyone who conveys the code may remove the exception [S21].
  - For Granary, WebView2 and Windows APIs are system or platform libraries either way. If we vendor Silo code, we may keep this exception on it or remove it (inference).
- **Contributions:** `silo-server/CONTRIBUTING.md` says contributions are licensed AGPL *including* the exception, and contributors keep their copyright [S22].
- **App Store / DRM exception:** `silo-apple/APPSTORE-EXCEPTION.md` covers app-store or DRM distribution of Silo Apple only. It is irrelevant to us, and it states that trademark rights are separate [S23].
- **Vendoring `silo-server/web`:** `web/package.json` declares `"license": "AGPL-3.0-or-later"` (name `silo-web`) [S20]. That is compatible with Granary's AGPL-3.0-or-later. Obligations:
  - Keep the copyright and license notices.
  - Mark our modifications with relevant dates (AGPLv3 §5(a)) [S11].
  - Carry over the third-party notices of what we copy, from `silo-server/THIRD_PARTY_NOTICES.md` and others [S24]:
    - `web/src/player/engine/wasm/silo-audio-decoders.wasm`: FFmpeg, LGPL-2.1+. The source tag is in `web/wasm/audio-decoders/build.sh` and the commit in `silo-audio-decoders.ffmpeg-revision`.
    - Mediabunny: MPL-2.0.
    - `web/vendor/foliate-js`: MIT, its own `LICENSE`.
    - `web/public/vendor/pdfjs`: carry its upstream license.
    - Swagger UI under `internal/apiv2/docsui/NOTICE`: only if we copy it.
- **Brand assets (`web/public/NOTICE`):** the logo, wordmark and app icons are "Copyright (c) 2026 Silo Media L.L.C. All rights reserved", are **not** under the AGPL, and "If you redistribute or fork this software, you MUST remove or replace these brand assets" [S24].
  - Concretely, `web/public/silo-icon-1024.png`, `silo-wordmark-sidebar.png`, `favicon.ico`, `apple-touch-icon.png`, `web-app-icon-*.png`, `maskable-icon-512.png` and `site.webmanifest` branding must not ship as Granary's identity (inference from the directory listing plus the NOTICE).
- **Trademark policy (`TRADEMARK.md`):** synced across all three repos; only the plugin example name differs [S20].
  - **Allowed:** truthful reference ("compatible with Silo", "a Silo client"). Allowed: showing the **unmodified** Silo logo only to refer to Silo, e.g. a "Connect to Silo" button or a server-type label.
  - **Not allowed:** using it as our logo or app icon.
  - **Needs written permission:** names where "Silo" stands alone, comes first ("Silo Client", "Silo Player"), or forms "Silo[word]" / "[word]Silo" names. Also needs permission: app-store publishing under a Silo identity.
  - **Forks:** must rebrand.
  - "Granary" contains no Silo mark, so the name is fine. Recommended tagline: "Granary, a desktop client compatible with Silo". Never imply that Silo Media made or endorses it [S20].

---

## 3. Tauri 2 bundling on Windows

**Current versions** (2026-11-06) [S25]:
- npm: `@tauri-apps/cli` 2.12.4, `@tauri-apps/api` 2.12.4, `@tauri-apps/plugin-updater` 2.14.1.
- crates: `tauri` 2.12.5, `tauri-cli` 2.12.4, `tauri-bundler` 2.10.4, `tauri-plugin-updater` 2.14.1.

### 3.1 Shipping `libmpv-2.dll` and other files

- **`bundle.resources`** takes either a list, which keeps directory structure under `$RESOURCE`, or a **map `"source": "target"`** [S16]. Sources are relative to `tauri.conf.json`. A glob in map form flattens into the target, and `"dir/**"` is an error [S16].
- **Windows placement:** in the bundler's NSIS template (`tauri-bundler-v2.10.4`), each resource is copied with `File /a "/oname={{target}}"` relative to `$INSTDIR`, the folder that holds the main exe. The uninstaller deletes the same paths [S17].
  - So a map entry whose target is a bare file name puts the DLL **next to `granary.exe`**. The Windows loader searches the application directory first, so no PATH changes or `SetDllDirectory` are needed (loader inference).
  - Example (verify with a first bundle):
    ```json
    "bundle": {
      "resources": {
        "../vendor/libmpv/libmpv-2.dll": "libmpv-2.dll",
        "../THIRD_PARTY_NOTICES.md": "THIRD_PARTY_NOTICES.md",
        "../licenses/": "licenses/"
      }
    }
    ```
- **At runtime**, resolve files with `app.path().resolve(.., BaseDirectory::Resource)` in Rust or `resolveResource()` in JS [S16].
- **During `tauri dev`:** check that the DLL also sits next to `target/debug/granary.exe`. If not, copy it in `build.rs` (inference).
- **Alternative: `bundle.externalBin` (sidecar)** is meant for executables, not DLLs. Use resources (inference).

### 3.2 NSIS vs MSI

- **MSI** is built with WiX Toolset v3, **only on Windows**, and needs the VBSCRIPT optional feature [S18].
- **NSIS** can also be cross-built (not needed for us).
- **NSIS install modes** (`bundle.windows.nsis.installMode`) [S19]:
  - `currentUser` (default): no admin, installs to `%LOCALAPPDATA%\<productName>`, registry entries under HKCU.
  - `perMachine`: Program Files, admin, HKLM.
  - `both`: the user chooses, and admin is needed even for per-user installs [S17, S18, S19].
- **Recommendation: NSIS only, `installMode: "currentUser"`**, with `bundle.targets: ["nsis"]` to skip WiX and save CI time (inference).
- **NSIS fields** (`bundle.windows.nsis`) [S19]:
  - Images: `template`, `headerImage` (150×57), `sidebarImage` (164×314), `uninstallerHeaderImage`.
  - Icons: `installerIcon`, `uninstallerIcon`.
  - Install mode: `installMode`.
  - Languages: `languages` (default `["English"]`), `customLanguageFiles`, `displayLanguageSelector`.
  - `compression`: `zlib`, `bzip2`, `lzma` (default) or `none`.
  - `startMenuFolder`.
  - `installerHooks`: an `.nsh` file defining `NSIS_HOOK_PREINSTALL`, `NSIS_HOOK_POSTINSTALL`, `NSIS_HOOK_PREUNINSTALL` and `NSIS_HOOK_POSTUNINSTALL`.
  - `minimumWebview2Version` here is deprecated; use `bundle.windows.minimumWebview2Version`.
- **Other `bundle.windows` defaults:** `allowDowngrades: true`, `bundleVCRuntime: false`, `signCommand: null`, `certificateThumbprint: null` [S19].
- **Silent install and uninstall** (from the template) [S17]:
  - The installer supports `/S` (silent), `/P` (passive), `/NS` (no shortcuts), `/R` (run after install, silent or passive only), `/UPDATE` (updater mode) and NSIS's standard `/D=` install directory.
  - The uninstaller is `$INSTDIR\uninstall.exe` and is registered under `Software\Microsoft\Windows\CurrentVersion\Uninstall\<productName>` in HKCU for per-user installs. It accepts `/S` too.
- **What uninstall leaves behind** [S17]:
  - `%APPDATA%\<identifier>` and `%LOCALAPPDATA%\<identifier>` are removed **only** if the user ticks the "Delete app data" checkbox, and never during `/UPDATE`. A silent `/S` uninstall leaves app data behind.
  - That data includes the WebView2 user-data folder, which Tauri keeps under the identifier in LocalAppData (inference).
  - The uninstall also removes the HKCU `Run` autostart value.
  - Anything else we write, such as an mpv cache or logs elsewhere, needs a `NSIS_HOOK_POSTUNINSTALL` hook (inference).

### 3.3 WebView2 install modes (`bundle.windows.webviewInstallMode`) [S18, S19]

| Mode | Size added | Notes |
|---|---|---|
| `downloadBootstrapper` (default, `silent: true`) | 0 MB | Downloads Microsoft's bootstrapper and needs internet. Runs only if WebView2 is missing (registry check). |
| `embedBootstrapper` | ~1.8 MB | Bootstrapper embedded, still downloads the runtime. |
| `offlineInstaller` | ~127 MB | Full offline runtime installer. |
| `fixedRuntime` (+`path`) | ~180 MB | Private fixed runtime. We must service it ourselves. |
| `skip` | 0 MB | Not recommended. The app fails without a runtime. |

Windows 11 includes the WebView2 runtime, and the NSIS section skips installation when the runtime's `pv` registry value exists [S17, S18]. **Recommendation: keep `downloadBootstrapper`.** For Windows 11-only targets, the safety net is free (inference).

---

## 4. Code signing (documentation only)

- **Tauri hooks** (`bundle.windows`) [S26]:
  - `certificateThumbprint` + `digestAlgorithm` (`sha256`) + `timestampUrl`, for a certificate in the Windows store via signtool.
  - Or `signCommand` with `%1` as the file placeholder, e.g. `relic` for Azure Key Vault, or `artifact-signing-cli` (`cargo install artifact-signing-cli`): `"signCommand": "artifact-signing-cli -e https://wus2.codesigning.azure.net -a MyAccount -c MyProfile -d MyApp %1"`.
  - Tauri's guide for OV `.pfx` import only applies to OV certificates issued before 2023-06-01. Newer OV and EV certificates live on HSMs or tokens, so follow the issuer's process [S26].
- **Azure Artifact Signing** (the renamed Trusted Signing) [S27, S28]:
  - Managed service; keys are in FIPS 140-3 Level 3 HSMs.
  - Basic and Premium SKUs, from $9.99/month. Billing is not pro-rated.
  - Requires a **paid** Azure subscription; free, trial and sponsored subscriptions are refused.
  - Identity validation exists for organizations **and individuals** (Verified ID plus government ID). Supported countries are listed in the quickstart prerequisites, not checked here.
  - Certificates are short-lived (3-day certificates are cited), so a timestamp is essential: `http://timestamp.acs.microsoft.com`.
  - It does not issue EV certificates.
- **OV and EV:** since 2024, EV gives **no** instant SmartScreen reputation. OV and EV build reputation the same way [S26, S29].
- **SmartScreen without signing** [S29]:
  - The user sees "Windows protected your PC" and must click "Run anyway". Enterprise policy can block it outright.
  - Reputation resets with every unsigned version.
  - **Smart App Control** on Windows 11 "will block execution of unsigned files unless the file has a positive reputation". This affects testers on clean Windows 11 installs where Smart App Control is on.
- **Practical guidance:**
  - Fine for an internal or private alpha: unsigned, with tester instructions.
  - Before any public distribution: Artifact Signing (cheapest CI-friendly option).
  - Sign the exe, `libmpv-2.dll` and the installer, and never modify files after signing [S29]. Check whether tauri-bundler signs resource DLLs; if not, sign them in a pre-bundle step (inference).

## 5. Updates

- **Updater signatures:** the Tauri updater requires its own minisign-style signature, which cannot be disabled. The key comes from `TAURI_SIGNING_PRIVATE_KEY` at build time, and `pubkey` sits in the config [S30].
- **Endpoint:** `bundle.createUpdaterArtifacts: true`. TLS is enforced in production. URL variables are `{{target}}`, `{{arch}}` and `{{current_version}}`. A static JSON can look like `https://github.com/<o>/<r>/releases/latest/download/latest.json`, which tauri-action can generate [S30, S36].
- **Request headers:** `check()` and the Rust builder accept custom headers such as `Authorization` [S30].
- **Windows install mode:** `plugins.updater.windows.installMode` is `passive` (default), `basicUi` or `quiet` [S30].
- **Private repo problem:** release assets of a private repo can only be downloaded with an authenticated API request [S31]. Embedding a token, even read-only, in a shipped binary leaks repo access and is unacceptable (inference).
- **Options** (inference):
  1. **Now:** no auto-update. Help > "Check for updates" opens the GitHub releases page in the browser. Testers have repo access and are signed in.
  2. **Later:** a separate **public** feed, e.g. a public `granary-releases` repo or a static site with `latest.json` and installers, signed with the Tauri updater key.
     - Public binaries put AGPL §6 into effect for the public: we must offer the Corresponding Source to every recipient. That means publishing Granary's source (and libmpv's) alongside, which effectively makes the code public.
  3. A self-hosted dynamic endpoint (204 when there is no update) also works [S30].

## 6. GitHub Actions (private repo)

- **Included usage is a dollar amount.** "included compute usage is expressed as a monetary amount, not a fixed number of minutes" [S32]:

  | Plan | Included compute | Artifact storage (shared with Packages) | Cache storage per repo |
  |---|---|---|---|
  | Free | $12/month | 500 MB | 10 GB |
  | Pro | $18/month | 1 GB | 10 GB |

- **Rates** [S32, S33]:
  - Linux 2-core $0.006/min (1-core $0.002).
  - **Windows 2-core $0.010/min**.
  - macOS $0.062/min.
  - The docs no longer describe minute multipliers. Some third-party pages still quote the old "2,000 min, 2x Windows" model; trust the official docs [S32, S33].
  - That gives about **1,200 Windows minutes/month on Free and about 1,800 on Pro** (inference: $12 / $0.010).
  - Public repos get standard runners free.
  - Overage: cache $0.07/GB-month, artifacts $0.25/GB-month. Without a payment method, usage stops once the included amount runs out [S32].
- **Caches** [S34]:
  - Entries not accessed for 7 days are evicted. The limit is 10 GB per repo by default.
  - A run can restore caches from its own branch or the default branch (and from the base branch for PRs).
- **Limits** [S35]:
  - A job can run 6 hours at most.
  - Concurrency: Free 20 jobs, Pro 40.
- **Current majors** (GitHub API `releases/latest`, 2026-11-06) [S36–S40]:
  - `actions/checkout@v7` (v7.0.1)
  - `actions/setup-node@v7` (v7.1.0)
  - `pnpm/action-setup@v6` (v6.1.1)
  - `dtolnay/rust-toolchain@stable`: branch-based refs, the only release is `v1` from 2022
  - `Swatinem/rust-cache@v2` (v2.9.3)
  - `tauri-apps/tauri-action@v1` (action-v1.0.0, 2026-06-29)
  - `softprops/action-gh-release@v3` (v3.0.4)
  - also `actions/cache@v6` and `actions/upload-artifact@v7`
- **Caching recipe:**
  - pnpm: `pnpm/action-setup@v6`, either with no `version` (it reads `packageManager`) and `cache: true`, or followed by `actions/setup-node@v7` with `cache: pnpm`. setup-node needs pnpm installed first. Use one store cache, not both [S37, S38].
  - Rust: `Swatinem/rust-cache@v2` after the toolchain step, with `workspaces: "src-tauri -> target"` and `save-if: ${{ github.ref == 'refs/heads/main' }}` so PR runs restore but do not write. It caches `~/.cargo` plus dependency artifacts in `target`, not workspace crates, and sets `CARGO_INCREMENTAL=0` [S39].
- **Draft release:**
  - `tauri-action@v1` with `releaseDraft: true`, `tagName: v__VERSION__` and `permissions: contents: write`. Or omit `tagName` to build only, then upload with `actions/upload-artifact` or `uploadWorkflowArtifacts` [S36].
  - Alternatively, run `gh release create --draft` or `softprops/action-gh-release@v3` with `draft: true` [S40].
  - `uploadUpdaterJson` defaults to true. Turn it off while there is no public feed [S36].
- **Keeping `release.yml` cheap** (inference):
  - Run it on tag push or `workflow_dispatch` only, never on every push.
  - Use a single `windows-latest` job and `bundle.targets: ["nsis"]` (no WiX).
  - Restore rust-cache and the pnpm cache; fetch the mirrored libmpv archive and check its SHA-256.
  - Set `retention-days` of 1–7 on workflow artifacts to stay within 500 MB / 1 GB [S32].
  - Run lint and tests on `ubuntu-latest` at 60% of the Windows rate, and only build the Windows bundle where needed.
  - Rough estimate: 8–15 Windows minutes per release build with a warm cache (unmeasured).

---

## Pinned libmpv recommendation

**Primary: official mpv release (LGPL)**
- Asset: `libmpv-v0.42.0-x86_64-w64-mingw32-lgpl.zip`
- URL: `https://github.com/mpv-player/mpv/releases/download/v0.42.0/libmpv-v0.42.0-x86_64-w64-mingw32-lgpl.zip`
- Size: 56,277,779 bytes
- Digest: `sha256:8e5f77430efc53ef175804ba6618527e3af0448a0dcabf04d3cc72659d1a6fce`
- Signature: `libmpv-v0.42.0-x86_64-w64-mingw32-lgpl.zip.asc` (`sha256:7bdac86cf22b4c9825292b0e5f0fcc83f2f07fb2163f244abc81438fc8bd4863`). Verify it against mpv's release signing key (key not researched here).
- mpv: tag `v0.42.0`; CI head `b0841e8c4d156b8403dc1876c4cbd9011eb583f3`.
- FFmpeg: master HEAD as of CI run `41936479105` (2026-11-04T23:47Z), built in BtbN's `win64-lgpl` image. **Record the exact commit from `ffmpeg-version` after download.**
- License: LGPLv2.1+ (mpv in LGPL mode) plus LGPL FFmpeg. The package ships `LICENSE.LGPL`.
- Expected contents, per the build script (verify after extracting): `libmpv-2.dll`, `libmpv.dll.a`, `include/mpv/*.h`, `LICENSE.LGPL`, `libbluray-*.jar`. The jar is not needed; leave it out.
- Why: permanent tagged URL, PGP-signed, stable source tag, lighter obligations, baseline x86-64 (no AVX2), `-Dd3d11=enabled`.

**Fallback: shinchiro (the originally planned source)**
- Asset: `mpv-dev-x86_64-20261106-git-9028187538.7z` (baseline x86-64; **not** `-v3`)
- URL: `https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/20261106/mpv-dev-x86_64-20261106-git-9028187538.7z`
- Size: 31,801,743 bytes
- Digest: `sha256:b5a3cc91b5dabf88ad5bb21d000834da4fd0bbda30f4584d39c082127bd62414`
- mpv: `902818753849d9ad8f0096fad6ca5aeaa37113b9` (v0.42.0 + 3 commits).
- FFmpeg: `9f690c6a4c124b9fc60588860ac66ae3fc808525` (master, pre-9.1).
- Recipe: `shinchiro/mpv-winbuild-cmake@bda10a2816ce9908d419b3f4b564227330457e76`.
- License: **GPLv3-or-later** (FFmpeg `--enable-gpl --enable-version3`, mpv `gpl=true`).
- Contents: `libmpv-2.dll`, `libmpv.dll.a`, `include/mpv/{client,render,render_gl,stream_cb}.h`. No `.def` or `.lib`.
- **Expires:** pruned once 30 newer releases exist, around early December 2026. Mirror it immediately.

## Obligations checklist

- [ ] Ship `LICENSE` (AGPL-3.0 text) with Granary and show the license plus a source link in About.
- [ ] `THIRD_PARTY_NOTICES.md`, bundled as a resource and viewable in About, listing:
  - libmpv (mpv tag/commit, license);
  - FFmpeg (commit, configure flags or build-image reference, license);
  - the other statically linked libraries (libplacebo, libass, dav1d, harfbuzz, freetype, …) with their licenses;
  - Tauri and the Rust/JS dependencies (generated, e.g. with cargo-about or a pnpm licenses export);
  - vendored Silo web code (AGPL-3.0-or-later, copyright Silo Media L.L.C. and contributors) and its third-party pieces (FFmpeg wasm LGPL, Mediabunny MPL-2.0, foliate-js MIT, pdf.js).
- [ ] Ship the full license texts: GPLv3, LGPLv2.1/LGPLv3, MPL-2.0, MIT/BSD notices.
- [ ] Credit FFmpeg in About. Do not rename `libmpv-2.dll`. No anti-reverse-engineering terms [S15].
- [ ] Corresponding Source for **each** binary release, attached to the same release:
  - the Granary source archive at the release tag;
  - libmpv source: the mpv tag tarball, the FFmpeg commit tarball, the build recipe, and the dependency sources or exact pointers.
  - Do not rely on shinchiro's release page, which is pruned [S3, S12].
- [ ] Mark modifications to vendored Silo files (AGPL §5(a)) and keep their headers.
- [ ] Remove or replace the Silo brand assets from vendored `web/public`. No Silo logo or wordmark as our icon or splash. Use the logo only unmodified and only to refer to Silo.
- [ ] Naming: "Granary". The tagline may say "compatible with Silo" or "a Silo client". Never "Silo Desktop", "SiloX" and the like, and never "official" [S20].
- [ ] Decide whether Granary grants the same Platform Library exception for its own code. It is optional; we may keep the exception on copied Silo code or remove it [S21].
- [ ] If the repo or binaries become public: publish the source at the same time (AGPL §6 / §13), and use a separate Tauri updater key from any Authenticode key.
- [ ] Verify the SHA-256 of the libmpv archive in CI against the value pinned in this document, and store the mirror in our own release assets.

## Sources

- **S1:** GitHub API, shinchiro releases: https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases?per_page=5 (and `?per_page=100`, which returned 30 releases, oldest `20260926`); release page https://github.com/shinchiro/mpv-winbuild-cmake/releases/tag/20261106
- **S2:** GitHub API, shinchiro workflow run 42133553972 (head_sha `bda10a28…`): https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/actions/runs/42133553972
- **S3:** shinchiro release workflow (x86_64_v3 matrix, SourceForge/GitHub upload, "Keep latest 30 tags/releases"): https://github.com/shinchiro/mpv-winbuild-cmake/blob/master/.github/workflows/mpv_clang.yml
- **S4:** shinchiro build config and README: https://github.com/shinchiro/mpv-winbuild-cmake/blob/master/packages/ffmpeg.cmake , https://github.com/shinchiro/mpv-winbuild-cmake/blob/master/packages/mpv.cmake , https://github.com/shinchiro/mpv-winbuild-cmake/blob/master/packages/openssl.cmake , https://github.com/shinchiro/mpv-winbuild-cmake/blob/master/README.md
- **S5:** commit lookups: https://api.github.com/repos/mpv-player/mpv/commits/9028187538 , https://api.github.com/repos/FFmpeg/FFmpeg/commits/9f690c6a4 , https://api.github.com/repos/mpv-player/mpv/compare/v0.42.0...902818753849d9ad8f0096fad6ca5aeaa37113b9 , https://api.github.com/repos/FFmpeg/FFmpeg/compare/n9.1-dev...9f690c6a4c124b9fc60588860ac66ae3fc808525 , https://api.github.com/repos/FFmpeg/FFmpeg/tags
- **S6:** mpv v0.42.0 release (notes, assets, digests): https://api.github.com/repos/mpv-player/mpv/releases/tags/v0.42.0 , https://github.com/mpv-player/mpv/releases/tag/v0.42.0
- **S7:** mpv releases list: https://api.github.com/repos/mpv-player/mpv/releases?per_page=3
- **S7b:** mpv licensing: https://github.com/mpv-player/mpv/blob/v0.42.0/Copyright , https://github.com/mpv-player/mpv/blob/v0.42.0/meson.options
- **S8:** mpv CI: https://github.com/mpv-player/mpv/blob/v0.42.0/.github/workflows/build.yml , https://github.com/mpv-player/mpv/blob/v0.42.0/ci/build-mingw64-full.sh , https://github.com/mpv-player/mpv/blob/v0.42.0/ci/publish-release.sh , run https://api.github.com/repos/mpv-player/mpv/actions/runs/41936479105
- **S9:** x86-64 psABI (microarchitecture levels): https://gitlab.com/x86-psABIs/x86-64-ABI ; secondary summary https://gist.github.com/scivision/38b907a684d21b66ce4af5fac5615a1c
- **S10:** Windows 11 CPU requirements: https://learn.microsoft.com/en-us/windows-hardware/design/minimum/supported/windows-11-24h2-supported-intel-processors ; 24H2 SSE4.2/POPCNT explainer https://www.prestomypc.com/guides/windows-11-24h2-compatibility-check-sse42-and-popcnt-explained/
- **S11:** GNU AGPLv3 (§5, §6, §13): https://www.gnu.org/licenses/agpl-3.0.html
- **S12:** GNU GPLv3 (§6 conveying non-source forms, §13): https://www.gnu.org/licenses/gpl-3.0.html
- **S13:** GNU GPL FAQ (#AGPLGPL, #UnchangedJustBinary, #UnreleasedModsAGPL): https://www.gnu.org/licenses/gpl-faq.html
- **S14:** GNU LGPLv3: https://www.gnu.org/licenses/lgpl-3.0.html ; LGPLv2.1: https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html
- **S15:** FFmpeg license and legal checklist: https://ffmpeg.org/legal.html
- **S16:** Tauri 2, Embedding additional files (resources): https://v2.tauri.app/develop/resources/
- **S17:** tauri-bundler NSIS template at `tauri-bundler-v2.10.4`: https://github.com/tauri-apps/tauri/blob/tauri-bundler-v2.10.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
- **S18:** Tauri 2, Windows Installer: https://v2.tauri.app/distribute/windows-installer/
- **S19:** Tauri 2 configuration reference (NsisConfig, NSISInstallerMode, WindowsConfig defaults, BundleResources): https://v2.tauri.app/reference/config/
- **S20:** Silo trademark and license: `silo-server/TRADEMARK.md@6791aac`, `silo-android/TRADEMARK.md@ee178d8`, `silo-apple/TRADEMARK.md@77def09`, `silo-server/LICENSE@6791aac`, `silo-android/LICENSE@ee178d8`, `silo-apple/LICENSE@77def09`, `silo-server/web/package.json@6791aac`; canonical policy https://github.com/Silo-Server/siloserver.org/blob/main/TRADEMARK.md
- **S21:** `silo-server/PLATFORM-EXCEPTION.md@6791aac` (identical in `silo-android@ee178d8` and `silo-apple@77def09`)
- **S22:** `silo-server/CONTRIBUTING.md@6791aac` (section "Licensing of contributions")
- **S23:** `silo-apple/APPSTORE-EXCEPTION.md@77def09`
- **S24:** `silo-server/THIRD_PARTY_NOTICES.md@6791aac`, `silo-server/web/public/NOTICE@6791aac`, `silo-server/web/vendor/foliate-js/LICENSE@6791aac`, `silo-server/internal/apiv2/docsui/NOTICE@6791aac`; also `silo-android/THIRD_PARTY_NOTICES.md@ee178d8` and `silo-apple/THIRD_PARTY_NOTICES.md@77def09` (reviewed for practice, not vendored)
- **S25:** versions: https://registry.npmjs.org/@tauri-apps/cli , https://registry.npmjs.org/@tauri-apps/api , https://registry.npmjs.org/@tauri-apps/plugin-updater , https://crates.io/api/v1/crates/tauri , https://crates.io/api/v1/crates/tauri-bundler , https://crates.io/api/v1/crates/tauri-cli , https://crates.io/api/v1/crates/tauri-plugin-updater , https://crates.io/api/v1/crates/libmpv2 , https://crates.io/api/v1/crates/libmpv2-sys
- **S26:** Tauri 2, Windows code signing: https://v2.tauri.app/distribute/sign/windows/
- **S27:** Microsoft Learn, What is Artifact Signing?: https://learn.microsoft.com/en-us/azure/artifact-signing/overview
- **S28:** Microsoft Learn, Artifact Signing FAQ: https://learn.microsoft.com/en-us/azure/artifact-signing/faq ; pricing https://azure.microsoft.com/pricing/details/artifact-signing/
- **S29:** Microsoft Learn, SmartScreen reputation for Windows app developers (EV note, $9.99 Artifact Signing, Smart App Control): https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
- **S30:** Tauri 2, Updater plugin: https://v2.tauri.app/plugin/updater/
- **S31:** GitHub REST, Release assets (authenticated download for private repos): https://docs.github.com/en/rest/releases/assets
- **S32:** GitHub Docs, GitHub Actions billing (included compute $, storage, rates): https://docs.github.com/en/billing/concepts/product-billing/github-actions
- **S33:** GitHub Docs, Actions runner pricing: https://docs.github.com/en/billing/reference/actions-runner-pricing
- **S34:** GitHub Docs, Dependency caching reference: https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching
- **S35:** GitHub Docs, Actions limits: https://docs.github.com/en/actions/reference/limits
- **S36:** tauri-action README and releases: https://github.com/tauri-apps/tauri-action , https://api.github.com/repos/tauri-apps/tauri-action/releases/latest
- **S37:** pnpm/action-setup README and releases: https://github.com/pnpm/action-setup , https://api.github.com/repos/pnpm/action-setup/releases/latest
- **S38:** actions/setup-node README and releases: https://github.com/actions/setup-node , https://api.github.com/repos/actions/setup-node/releases/latest ; actions/checkout: https://api.github.com/repos/actions/checkout/releases/latest ; actions/cache: https://api.github.com/repos/actions/cache/releases/latest ; actions/upload-artifact: https://github.com/actions/upload-artifact
- **S39:** Swatinem/rust-cache README and releases: https://github.com/Swatinem/rust-cache , https://api.github.com/repos/Swatinem/rust-cache/releases/latest ; dtolnay/rust-toolchain: https://github.com/dtolnay/rust-toolchain (branches via https://api.github.com/repos/dtolnay/rust-toolchain/branches)
- **S40:** softprops/action-gh-release: https://github.com/softprops/action-gh-release , https://api.github.com/repos/softprops/action-gh-release/releases/latest ; GitHub CLI `gh release create`: https://cli.github.com/manual/gh_release_create
