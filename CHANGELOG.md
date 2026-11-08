# Changelog

All notable changes to Granary. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Detail page redesign (banner stays inside its box): logo or title, chips for year, rating, runtime and the best
  file's quality (4K, HDR, audio), TMDB score, a Play button whose fill shows how far you got, Mark watched, favourite,
  a 1–5 star rating and a menu with watchlist and not interested.
- Audio and subtitle pickers that always lead with the language (Deutsch, English, …), with codec, channels, Atmos,
  forced, SDH and default as badges, on the detail page and in the player.
- "Directed by / Written by", a Cast & crew carousel grouped into direction, writers and cast, and person pages with
  photo, life facts, biography and a filmography (all, movies, series).
- Trailers & more: local extras play in the app; YouTube trailers open in the browser.
- More like this on movie and series pages.
- A full-screen launch screen (backdrop, logo, loading line) from Play until the first frame, instead of an empty
  window.
- Libraries have Recommended, Library and Collections tabs. The grid loads more as you scroll, and has an A–Z rail,
  genre and decade filters, sort and shuffle.
- Optional quality tags (4K, HDR, audio) on posters (Settings → Appearance, off by default).
- Seek-bar preview images for files without server preview sheets: Granary decodes a small frame from the file
  itself (HDR tone-mapped), cached as you hover. Settings → Player → Seek-bar previews (on by default).
- A website with documentation in `site/` (Next.js static export with Fumadocs), deployed to GitHub Pages by the
  `pages` workflow.

### Changed
- Monochrome app icon (black and white) instead of the yellow one.
- README rewritten: screenshots, feature overview, install, build, architecture and how the app was built.

## [0.1.1] - 2026-11-08
Bug-fix release.

### Fixed
- The movie disappeared while the player controls were shown, and the window showed the desktop behind it; it came
  back only when the controls hid. The video window is no longer clipped by the overlay (ADR-0003 Amendment 1).
  The playback evidence run now checks the composed screen pixels with the controls shown and hidden.

## [0.1.0] - 2026-11-07
First private test release (unsigned, per-user installer).

### Added
- Connect to a self-hosted Silo server (compatible with Silo, not affiliated): address check, sign-in, profiles with
  PIN, refresh tokens in Windows Credential Manager.
- Browsing in the Silo web-app look: home rows, libraries with filters, search, movie, series, season and episode pages.
- Native playback through libmpv (gpu-next, D3D11, d3d11va) in the app window, with a transparent overlay for the
  controls: seeking with trickplay previews, chapters, intro/credits skip, audio and subtitle tracks, next episode
  (also across seasons), mini player, statistics, keyboard shortcuts that follow the keyboard layout.
- Progress reporting, resume points and server-side playback plans (direct play, remux, transcode).
- Display handling for OLED HDR monitors: automatic HDR switching and refresh-rate matching, always restored (also
  after a crash, through a restore marker); tone mapping, audio device, channels and passthrough settings.
- English and German UI, onboarding, About with licences, the exact source commit and an update link, log viewer and redacted log export,
  offline banner with automatic reload.
- Bundled mpv defaults (`mpv.conf`) and an optional power-user `%APPDATA%\Granary\mpv.conf`.
- NSIS per-user installer with libmpv, full third-party licence texts and the WebView2 bootstrapper; draft releases from CI.
- Security review (`docs/security.md`), architecture overview (`docs/ARCHITECTURE.md`), `CONTRIBUTING.md`.

### Security
- Loopback relay route tokens are masked in logs, and error messages shown in the UI get the same redaction as logs.
- Artwork references on foreign hosts may no longer name `localhost` or a loopback/private/link-local address.
- mpv never auto-loads scripts or starts `yt-dlp` (`load-scripts=no`, `ytdl=no` forced).
- A test checks the IPC contract: commands, capability, permissions and argument names.
- Dev dependency `minimatch` pinned past GHSA-g3hv-75wg-m249.

[0.1.0]: https://github.com/mrghxst/granary-desktop/releases/tag/v0.1.0
