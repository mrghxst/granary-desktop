# ADR-0009: Seek-bar previews without server trickplay

- Status: Accepted
- Date: 2026-11-08
- Inputs: user request (0.2 list, item 8: "how the AIOStreams desktop app implemented ... preview images"); ADR-0002
  (in-process libmpv), ADR-0008 (loopback relay); `docs/research/silo-api.md` §9 (trickplay sheets, already used when
  the server has them).

## Context
- The seek bar already shows server trickplay tiles (`watch_trickplay`), but many libraries have no sheets generated,
  and then hovering the bar shows only a time.
- **How AIOStreams does it** (Viren070/AIOStreams, `packages/desktop/core/src/thumbnails.rs`, which uses the AGPL
  crates `aiostreams-container` / `aiostreams-thumbs` from github.com/AIOStreams/media @87dee2cc; read as data, no
  code copied): it reads the container index (MKV Cues, MP4 `stss`) over HTTP range requests, fetches only the
  keyframe packets near the hovered time, decodes them with a dynamically loaded libavcodec, tone-maps HDR to SDR,
  scales to a thumbnail and serves the JPEG to its web UI, with a per-file cache.
- Granary's libmpv build links FFmpeg statically, so there is no separate libavcodec DLL to load. Bundling a second
  FFmpeg (about 80–100 MB of DLLs) for this alone would almost double the installer.

## Decision
- A **second, headless libmpv handle** per previewed file (`granary_player::thumbs::Thumbnailer`): `vo=null`, no
  audio or subtitles, `pause=yes`, `hr-seek=no` (keyframe seeks), `hwdec=auto-copy`, no config or scripts. It opens
  the same loopback relay URL as the main player (ADR-0008), so no token reaches mpv, and only for direct play
  (`original_http`): seeking a transcode would start server work.
- Each preview: `seek <t> absolute+keyframes`, wait for `PLAYBACK_RESTART`, `screenshot-to-file <tmp>.jpg video`,
  read and delete the file. The video filter chain scales to 320 px wide; for PQ/HLG sources (`video-params/gamma`)
  it is `zscale` (linearise, BT.709 primaries) + `tonemap=hable` + `zscale` back to BT.709, all inside the preview
  handle. The main player's GPU pipeline is never touched.
- Host command `player_preview(seconds)` returns a JPEG data URL (CSP already allows `data:` images), caches 400
  frames per source (keyed by requested time), decodes one frame at a time, and remembers a source that failed to open so it is
  not retried on every hover. The overlay hook snaps hover positions to an interval of at
  least 30 s (about 120 frames per file for long ones, at most 2 min), waits 140 ms for the pointer to rest before
  asking, keeps one request in flight and always fetches the newest hover position next; while a frame loads it shows
  a decoded neighbour up to two intervals away. Revised 2026-11-08: the first version used 2 s buckets and asked on
  every bucket crossed, which on slow, high-latency remote sources meant many seeks and frames that arrived late or
  timed out on large files. Open and frame timeouts are now 20 s and 15 s.
- Server trickplay sheets win when they exist. Setting **Seek-bar previews** (Player, on by default) turns local
  previews off, for example on a metered remote connection.

## Consequences
- Measured on the dev machine (test media, mock server through the relay): open 0.24 s, a new frame 35–90 ms (SDR
  1080p to HDR10 2160p), cached 4 ms. Integration test `preview_frames_from_real_files` checks SDR and HDR against the
  pinned DLL.
- Extra traffic only while hovering: each new preview spot reads around one keyframe (the demuxer readahead is off).
- Dolby Vision profile 5 (no HDR10 base layer) is not reshaped by libavfilter, so its previews have wrong colours;
  profiles 7/8 use the HDR10 base and look right. Known issue, low impact (previews only).
- No new dependency or licence: the bundled libmpv/FFmpeg already provide everything (zscale/zimg is part of the
  pinned build).
