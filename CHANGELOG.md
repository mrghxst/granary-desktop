# Changelog

All notable changes to Granary. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow [Semantic Versioning](https://semver.org/).

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
- English and German UI, onboarding, About with licences and an update link, log viewer and redacted log export,
  offline banner with automatic reload.
- Bundled mpv defaults (`mpv.conf`) and an optional power-user `%APPDATA%\Granary\mpv.conf`.
- NSIS per-user installer with libmpv, licences and the WebView2 bootstrapper; draft releases from CI.
