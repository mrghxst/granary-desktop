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
- **Revision 2 (2026-11-08, 0.4.0): keyframe index first.** The seek-based previews still felt slow on large remote
  files: every spot was a seek in a multi-GB stream, so frames were coarse (30 s+) and late. Granary now follows the
  same idea as AIOStreams with its own implementation (no code or crate taken from it) and without a second FFmpeg:
  - `granary_player::mkv` parses the Matroska SeekHead, Info (timestamp scale), the first video TrackEntry (codec
    private data, colour transfer) and the video Cues with byte-range reads (`granary_player::range`: plain
    HTTP/1.1 `Range` requests to the loopback relay, or a local file), read in 256 KiB windows.
  - Each preview takes the cue at or before the hovered time, reads only that keyframe block (CueRelativePosition,
    or a bounded cluster scan), and wraps it in a minimal single-frame Matroska file (EBML header, Info, the original
    TrackEntry, one Cluster with one SimpleBlock). The headless handle `loadfile`s that local temp file
    (`keep-open=yes`), so the decode never touches the network or the playing stream. The previous temp file is
    deleted once mpv has moved on, the last one when the handle is dropped.
  - HDR is known from the track TransferCharacteristics (16 PQ, 18 HLG) before the first decode, and confirmed once
    from `video-params/gamma` for files without colour metadata.
  - The host caches frames per keyframe (`Thumbnailer::snap`) as well as per requested time, so hovering anywhere
    between two keyframes costs one fetch.
  - The overlay snaps to about 400 spots per file (2–10 s) and waits 60 ms for the pointer to rest.
  - Decoding is kept cheap so playback keeps priority: `vd-lavc-threads=2`, `vd-lavc-skiploopfilter=all`,
    `vd-lavc-fast`.
  - Files without Cues, laced blocks, and non-Matroska containers (MP4, AVI, ...) keep the seek mode above.
- **Revision 3 (2026-11-08, after 0.4.0): never wait on the hover.** On remote sources, the first byte of a cold range
  can take a second or more, so even one keyframe per spot left the tile empty for seconds. What others do:
  - Jellyfin, Plex, Netflix and YouTube pre-generate sprite sheets on the server. YouTube keeps several storyboard
    levels, coarse to fine, so something is always there (spec format as reverse-engineered by Invidious and others).
    Granary already uses server sheets when they exist.
  - Client-side generators on network files are known to be slow and heavy. mpv's thumbfast ships with `network=no`
    by default, and mpv_thumbnail_script recommends fewer thumbnails for remote files (`remote_thumbnail_count`).
  - Granary keeps the index reads but splits the work into three paths:
    - **Stand-in:** `player_preview_near` answers from the frame cache only (a separate lock from the decoder) with
      the nearest decoded keyframe. The overlay shows it at once, dimmed, if it is within a twelfth of the file.
    - **Exact:** `player_preview` fetches the hovered spot's keyframe as before. It goes ahead of everything else:
      the background fill yields while a hover request waits.
    - **Background fill:** `player_preview_warm` starts once per file, 4 s after the controls mount. Index mode only;
      seek mode would compete with playback. It decodes 12, then 48, then 120 evenly spread keyframes (at most 180
      fetches, minus those already cached), one at a time with a 300 ms pause. It runs only while the main player is
      playing or paused and not buffering, and stops when the source changes. Each frame is one keyframe, typically
      0.1–1.5 MB on 4K remuxes, so the fill costs well under 1 % of a remux and gives every hover a frame within a
      120th of the file.
  - Precision is not traded away: the exact keyframe still replaces the stand-in, usually within one range read.
  - The tile has a fixed 240 px width (trickplay tiles are scaled to it) and is clamped inside the bar, so it never
    shrinks at the right edge or leaves the screen at the left.
- **Revision 4 (2026-11-08, 0.4.2): decode in memory, in software, in parallel.** Measured per frame on the test
  media (local file, so network time is excluded), the decoder itself was the slow part: about 0.86 s for an HDR10
  2160p keyframe and 0.33 s for SDR 1080p. Broken down:
  - `hwdec=auto-copy` alone cost about 0.4 s per frame: a new D3D11 decoder is set up for every single-frame file,
    then the frame is copied back. The software decoder (`hwdec=no`) decodes one HEVC Main10 4K keyframe in about
    0.1 s. Thread count, `demuxer=mkv` and the HDR filter made little difference.
  - Each frame went through two temp files (the keyframe for mpv, then mpv's JPEG screenshot), each with its own
    open, write, read and delete.
  - The new pipeline:
    - Keyframes are served to mpv from memory through a custom protocol (`mpv_stream_cb_add_ro`, `granary-frame://`).
    - The frame comes back as pixels (`screenshot-raw`) and is encoded to JPEG in process (`jpeg-encoder`, quality
      78). Previews now write nothing to disk; the startup sweep of the old `granary-previews` folder stays for files
      older versions left behind.
    - Keyframe downloads run outside the decoder lock (`KeyframeFetcher`), so several hover requests fetch at once
      while one decodes. The overlay keeps up to three exact requests in flight; the spot under the pointer always
      goes next.
  - Result: about 0.12 s per frame for HDR10 2160p and 0.06 s for SDR 1080p (was 0.86 s and 0.33 s), plus the range
    read. On remote sources the range read is now most of the wait, and parallel fetches hide it for quick series of
    rests.
- **Revision 5 (2026-11-09, 0.4.3): newest request wins, frames are pushed.** Sweeping the bar still felt slow
  compared with the AIOStreams desktop app. We read its thumbnail code (AIOStreams/media @87dee2c). Its libmpv build
  ships shared FFmpeg libraries and decodes with libavcodec directly. Ours (shinchiro) exports no `av*` symbols, so
  decoding stays in mpv. Measured with `crates/player/tests/preview_bench.rs`, mpv costs 40–70 ms per 4K keyframe
  and about 20 ms per 1080p one, against about 20–40 ms for plain libavcodec. The decoder was therefore not why it
  felt slow. The causes were in the scheduling and the reads:
  - **Dimmed stand-ins.** A frame from elsewhere was drawn dimmed and desaturated until the exact one came, which
    looked like the colours "correcting". Stand-ins are now drawn as they are.
  - **Waiting for the pointer.** Nothing was asked until the pointer had rested for 60 ms.
  - **A queue of stale spots.** Requests waited up to three deep in the overlay, all of them for spots the pointer
    had already left.
  - **Several reads per keyframe.** Each keyframe took two or three sequential range requests: a 256 KB probe, then
    the rest of the block.

  The new design follows AIOStreams' session model:
  - One preview session per file, held by the host. Each new spot under the pointer replaces the previous request
    at once (`player_preview_want`); there is no queue.
  - Three fetch threads read keyframes, taking the wanted one first. One render thread decodes the wanted keyframe
    if it has arrived, else the most recently asked one.
  - Every decoded frame is pushed to the overlay (`player://preview`) when it answers a request newer than the
    frame on screen. During a sweep the tile therefore keeps following the pointer instead of waiting for the end.
  - Each keyframe is one range request, sized from the keyframes read so far: the 75th percentile × 1.4, between
    256 KiB and 8 MiB. A short read only fetches the missing tail.
  - The background fill of the grid starts 4 s after playback, pauses while the pointer moves, and takes at most one
    keyframe per 300 ms. Answers about an earlier file are dropped by generation.
- Server trickplay sheets win when they exist. Setting **Seek-bar previews** (Player, on by default) turns local
  previews off, for example on a metered remote connection.

## Consequences
- Measured on the dev machine (test media, mock server through the relay): open 0.24 s, a new frame 35–90 ms (SDR
  1080p to HDR10 2160p), cached 4 ms. Integration test `preview_frames_from_real_files` checks SDR and HDR against the
  pinned DLL.
- Extra traffic only while hovering: the index once (Cues are usually tens to hundreds of KiB), then one keyframe per
  new spot. Integration test `preview_frames_from_real_files` asserts index mode on the generated test media: open
  plus two frames each for SDR 1080p and HDR10 2160p in about 0.5 s.
- Dolby Vision profile 5 (no HDR10 base layer) is not reshaped by libavfilter, so its previews have wrong colours;
  profiles 7/8 use the HDR10 base and look right. Known issue, low impact (previews only).
- No new dependency or licence: the bundled libmpv/FFmpeg already provide everything (zscale/zimg is part of the
  pinned build).
