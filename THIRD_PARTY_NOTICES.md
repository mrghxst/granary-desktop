# Third-party notices

Granary is licensed under the GNU Affero General Public License v3.0 or later (see `LICENSE`).
Granary is an independent project, compatible with Silo media servers; it is not made or endorsed by Silo Media.
This file lists third-party components shipped in, or copied into, Granary builds. It is updated with every libmpv
pin bump (ADR-0005) and every change of bundled components.

## libmpv (bundled as `libmpv-2.dll`)

- Build: shinchiro/mpv-winbuild-cmake release `20261106`, asset `mpv-dev-x86_64-v3-20261106-git-9028187538.7z`,
  SHA-256 `a778b3444b0b8b6d93cb00ca49f2acf62a02d0b61249229fc9944672f80683bb`
  (https://github.com/shinchiro/mpv-winbuild-cmake/releases/tag/20261106). Built with `--enable-gpl --enable-version3`:
  the DLL as a whole is **GPL-3.0-or-later**, which combines with Granary's AGPL-3.0-or-later (GPLv3/AGPLv3 §13).
- mpv `v0.42.0-3-g902818753` — source: https://github.com/mpv-player/mpv/commit/9028187538 (GPL-2.0-or-later /
  LGPL-2.1-or-later parts; GPL in this build).
- FFmpeg `N-128504-g9f690c6a4` — source: https://github.com/FFmpeg/FFmpeg/commit/9f690c6a4 (GPL in this build).
  This software uses code of FFmpeg (https://ffmpeg.org) licensed under the GPLv3 and its source can be obtained
  from the link above.
- libplacebo `v7.387.0-13-g9fabf6d-dirty` (commit 9fabf6d plus the build recipe's patches) — source:
  https://code.videolan.org/videolan/libplacebo/-/commit/9fabf6d (LGPL-2.1-or-later); patches in the recipe commit below.
- Build recipe: https://github.com/shinchiro/mpv-winbuild-cmake/commit/bda10a2816ce9908d419b3f4b564227330457e76,
  workflow run 42133553972 (`mpv_clang.yml`). Other statically linked libraries (e.g. libass, freetype, harfbuzz,
  dav1d, zlib) are listed in that run's logs; see ADR-0005 for the known gap on the complete source set.

## Design tokens and font

- The "midnight-cinema" colour, radius and font tokens in `apps/desktop/src/styles/tokens.css` are taken from
  `silo-server/web/src/app.css` (commit 6791aac), Copyright (c) Silo contributors, AGPL-3.0-or-later,
  https://github.com/Silo-Server/silo-server. No Silo brand assets (logo, wordmark, icons) are used.
- Plus Jakarta Sans (variable), via `@fontsource-variable/plus-jakarta-sans` 5.3.0 — SIL Open Font License 1.1,
  Copyright 2020 The Plus Jakarta Sans Project Authors (https://github.com/tokotype/PlusJakartaSans).

## Application frameworks and libraries

Rust crates (see `Cargo.lock` for exact versions; licences MIT and/or Apache-2.0 unless noted):
Tauri 2 (`tauri`, `tauri-build`, `tauri-plugin-single-instance`, `tauri-plugin-window-state`), `windows`
(Microsoft), `libmpv2-sys` (LGPL-2.1), `serde`, `serde_json`, `thiserror`, `tracing`, `tracing-subscriber`,
`tracing-appender`, `tokio`, `reqwest` (with `rustls`, `rustls-platform-verifier`, `aws-lc-rs`: Apache-2.0 / ISC / MIT;
`aws-lc-sys` also OpenSSL-style licences), `keyring-core` and `windows-native-keyring-store`, `url`, `uuid`,
`async-trait`, `bytes`, `futures-core`, `futures-util`, `hyper`, `hyper-util`, `http-body-util` (loopback media relay),
`tokio-tungstenite` (events socket, MIT), `rustls`.
Test-only (not shipped): `wiremock`, `rcgen`, `tokio-rustls`.

JavaScript packages (see `pnpm-lock.yaml`; MIT unless noted): React, React DOM, React Router, i18next,
react-i18next, `@tauri-apps/api` (MIT/Apache-2.0), Tailwind CSS, lucide-react (icons, ISC).

Full licence texts of all bundled Rust and JavaScript dependencies are generated into the installer at packaging
time (Phase 9).

## Microsoft Edge WebView2

The WebView2 runtime is part of Windows 11 and is not redistributed by Granary.

## Silo API contract excerpt (tests only, not shipped)

`crates/silo-api/tests/fixtures/openapi-subset.json` is an excerpt (29 operations, 125 schemas) of
`silo-server/contracts/api/v2/openapi.json` at commit 6791aac, Copyright (c) Silo contributors, AGPL-3.0-or-later,
https://github.com/Silo-Server/silo-server. The `upstream_*.json` fixtures are copied verbatim from
`silo-server/contracts/api/v2/fixtures/` at the same commit (same copyright and licence). The other fixtures next to it
are hand-written or generated, sanitized examples.
