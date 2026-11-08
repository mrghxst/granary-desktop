# Changelog

All notable changes to Granary. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- Local seek previews are about six times faster to decode (about 0.1 s for a 4K HDR keyframe instead of 0.9 s) and
  never touch the disk: keyframes are decoded in memory in software and encoded to JPEG in the app. Several hovered
  spots download at once while one decodes, so moving along the bar no longer queues behind one slow request.
- Switching to an audio track that the open output can already play (for example stereo on a 5.1 output, or a track
  that Windows mixes down anyway) keeps the audio device open, so the sound comes back with the picture instead of a
  moment later.
- Seek-preview sheets from the server are kept in memory only (at most 64 MB) and never written to disk; sheets an
  earlier version stored in the artwork cache are removed once.
- Cached artwork that is due for its weekly refresh is shown at once and refreshed in the background, instead of
  waiting for the server.

### Fixed
- Turning on or switching to an embedded subtitle shows the current line right away; before, it often appeared only
  after a seek.
- No leftover files on disk: interrupted artwork-cache writes and local seek-preview scratch files from a run that
  did not end cleanly are removed at the next start, and the list of kept artwork is capped.

## [0.4.1] - 2026-11-08
Fixes and polish: a working right-click menu with more actions, Seekr seek previews, an artwork cache, the right subtitles at start and steadier audio switches.

### Added
- Right-click menu on every poster card (home rows, libraries, search, detail pages): Play, Play from beginning, Mark
  watched / unwatched and Remove from Continue Watching / Next Up, each shown only where it applies.
- Seek previews from Seekr (seekr.tv) with your own API key. Previews now come from a chain you can reorder and switch
  in Settings → Player: server previews first, then Seekr, then local generation. A title Seekr does not know, or a
  reached daily limit, falls back to the next source. The key is stored in Windows Credential Manager.
- Artwork is cached on disk (at most 300 MB), so posters and backgrounds appear at once after a restart and also
  offline. Artwork of titles you are still watching is kept longest. Settings → Servers shows the size and has a
  Clear button.
- Search has type filters: All, Movies, Shows or Episodes. The choice is remembered, so going back to search
  restores it.
- Seek previews feel instant. Once a file has played for a few seconds, a paced background fill fetches a
  coarse-to-fine grid of keyframes (12, then 48, then 120 over the file). It runs only while playback is healthy and
  never while you hover. Hovering shows the nearest decoded frame right away, slightly dimmed, and swaps in the exact
  keyframe for that spot a moment later.

### Changed
- Seek previews always have the same size (sheets of any tile size are scaled to it) and stay inside the bar at both
  ends instead of shrinking or going off-screen.
- The loading spinner stays visible while the controls are hidden, so a stalled picture always shows that it is
  loading.

### Fixed
- Right-clicking a card in a home row or a library no longer breaks the page. The menu was positioned relative to
  the hovered card and clipped by the row.
- The right subtitle starts with the video. The player sent the catalog's track number where the server expects
  its own track order, which could switch on an unrelated language (for example Arabic instead of forced English).
  The "forced only" subtitle mode now picks forced tracks, and when the server's pick cannot be matched, subtitles
  stay off instead of mpv choosing one.
- Starting the next episode (manually or by autoplay) shows the launch screen instead of a see-through window when
  the controls were hidden.
- After an audio-track switch on large remote files, the player checks that audio actually resumed in sync and
  resyncs on its own if it did not (no more seeking by hand to get sound back).

## [0.4.0] - 2026-11-08
First public beta: finer seek previews, a cleaner detail page and steadier playback.

### Changed
- Detail pages have a cleaner, more consistent banner: the artwork fills the frame (a blurred poster when there is no
  backdrop), the logo sits in a fixed-height slot, facts and quality tags share one line, credits are aligned, and the
  small poster beside the title is gone. Episodes show the show (logo or name) and season/episode above their title.
- Seek-bar previews read the keyframe index of Matroska files and fetch only one keyframe per preview, so they are
  much finer (every 2–10 s) and quicker on large remote files; other containers keep the previous method.
- Playback gets priority: a larger read-ahead buffer for big files, a short wait to refill the cache instead of
  stuttering, and seek-preview decoding limited to two threads with cheaper settings.
- Back sits in one fixed place on detail and person pages, is there before the page has loaded, and stays reachable
  while you scroll; pages from the sidebar have none. Going back restores the scroll position in one step.
- Long filter lists (studios, networks, countries, languages, genres) get a search field and show only the best
  matches instead of filling the whole panel.

### Fixed
- Switching audio tracks during playback no longer speeds up or breaks audio: the player pauses, switches and
  resyncs at the same spot, then continues.
- Picking the same external subtitle again reuses it instead of downloading another copy.

## [0.3.0] - 2026-11-08
Fixes from the 0.2 test round: filters, navigation, a calmer player and lighter seek previews.

### Added
- Audio and subtitle picks carry over to the next episodes of a series (matched by language and track type, so a
  different track order does not matter).
- Credits with an epilogue after them get Skip credits and Next episode buttons; the up-next countdown waits for the
  real end, and Next episode shows again there when autoplay is off or was cancelled.
- Library filters: watch state, 4K / HDR / Dolby Vision, minimum IMDb rating, recently added, age rating, language,
  studio, network and country, next to genre and decade; more sorts (year, runtime).
- Back button on detail and person pages; scroll position is restored when you go back.
- Rows (cast, trailers, more like this) can be dragged with the mouse.
- A new app icon (play mark centred on the grain).

### Changed
- Scrollbars and the A–Z rail follow the theme (thin rounded thumb, rail docked to the right edge); cards lift and
  zoom slightly on hover.
- Player menus are cleaner and close after you pick an audio track, subtitle or speed.
- Fullscreen is remembered separately for the player and for browsing.
- Seek-bar previews ask for a frame only once the pointer rests and step at least 30 s (more on long files), so they
  stay quick on slow remote files.
- The launch screen shows the poster and stays until the picture is actually moving, without the see-through flash.

### Fixed
- The poster is back on movie and series pages; audio and subtitle menus open above the cast row.
- Switching audio tracks no longer leaves audio muted or out of sync on direct-played files.

## [0.2.0] - 2026-11-08
Closer to the web app, still minimal: richer detail pages, library tabs, local seek previews, a website.

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

[0.3.0]: https://github.com/mrghxst/granary-desktop/releases/tag/v0.3.0
[0.2.0]: https://github.com/mrghxst/granary-desktop/releases/tag/v0.2.0
[0.1.1]: https://github.com/mrghxst/granary-desktop/releases/tag/v0.1.1
[0.1.0]: https://github.com/mrghxst/granary-desktop/releases/tag/v0.1.0
