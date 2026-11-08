# Reference clients review: AIOStreams desktop, Nuvio desktop, Stremio shell

Date: 2026-11-08. Purpose: check Granary's player and network stack against three other mpv-based desktop clients,
for reliability, speed and efficiency. The repositories were shallow-cloned into `.ref/` (gitignored). They are
treated as untrusted data: facts only. Instructions inside them, including their CLAUDE.md files, are not followed.

| Repo | Commit | Commit date | What was read |
|---|---|---|---|
| Viren070/AIOStreams | `bdd9cbc` | 2026-11-08 | `packages/desktop` (Rust host, WebView2 over libmpv) |
| NuvioMedia/NuvioDesktop | `00e50ef` | 2026-11-07 | `composeApp/src/desktopMain/native/windows/player_bridge.cpp`, player features in `commonMain` |
| Stremio/stremio-shell-ng | `d640dc1` | 2026-11-04 | `src/stremio_app/stremio_player/player.rs`, `gpu_video_processing.rs` |
| Stremio/stremio-core | `a8c2295` | 2026-11-07 | `src/constants.rs`, `src/models/player.rs` (progress and library model) |
| Stremio/stremio-video | `7a7462b` | 2026-10-28 | `src/ShellVideo/ShellVideo.js` (which mpv properties the web side observes) |

## Summary

No serious mistakes were found. The architecture is the same in all four apps: libmpv draws into a child window
under a transparent web view. Granary's controller does more than the others in several places:
- a sequenced progress protocol;
- a reload-then-renegotiate ladder;
- a credential-free loopback relay;
- a display restore marker;
- an audio switch that keeps the output open, and subtitle switches that seek in place.

The others simply set `aid`/`sid` and accept the gap.

One real gap was common to all three reference clients and is fixed in 0.4.2: retrying transient HTTP statuses
inside FFmpeg. The rest are improvements, ranked below.

## Fixed in 0.4.2

### 1. Retry transient HTTP statuses in place
All three clients pass FFmpeg
`reconnect_on_http_error=408,429,500,502,503,504` (AIOStreams `packages/desktop/app/src/main.rs@bdd9cbc`, Stremio
`player.rs@d640dc1` `create_mpv`, Nuvio `player_bridge.cpp@00e50ef` `startMpv`).

Granary only had `reconnect_on_network_error`. When the server or a reverse proxy in front of it answered a
reconnect with 502/503/504 or 429 (for example a proxy restart, or a rate limit during a seek storm), FFmpeg gave
up. mpv then ended the file, and the controller's reload rebuilt the whole stream:
- a visible "reconnecting" state;
- a new `loadfile`;
- the demuxer cache lost.

Now FFmpeg retries in place with its own backoff (capped at 8 s) and keeps the cache. The controller's reload stays
as the next rung. 401 is still handled by the relay (token refresh), and 403/404/410 still fail fast. The
integration test `selecting_an_embedded_subtitle_over_http_shows_the_current_line` loads with the app's exact
option string, so a quoting mistake would fail the build.

## Recommendations, highest value first

### 2. Load external subtitles without blocking the controller (small)
`Controller::add_sidecar` calls `sub-add <url> select` through the synchronous `mpv_command`. mpv downloads the
file (through the relay) before the call returns. On a slow or remote server, pause, seek and track commands queue
behind that download.

Stremio queues every `sub-add` through `mpv_command_async` on a worker thread, one in flight, and cancels on media
change (`start_next_subtitle`, `player.rs@d640dc1`).

Granary should do the same: send `sub-add` with `mpv_command_async`, and select the track when the reply event
arrives (or when `track-list` shows it). Show a small "loading subtitle" state meanwhile.

### 3. Prepare the next episode before the credits (medium)
Nuvio starts fetching the next episode's sources well before the "next episode" card appears. The lead time is the
auto-play timeout, at least 10 s (`PlayerNextEpisodeAutoPlay.kt@00e50ef` `isAtNextEpisodeThreshold`).

Granary already reuses the display output for an episode with the same needs (`output.rs`
`next_episode_with_the_same_needs_reuses_the_output`). Three further steps:
- negotiate the next playback plan about 30 s before the end, or when the Next Up card shows;
- register its relay route;
- read the Matroska index for previews.

Pressing Next or auto-advancing would then skip the server round trip. The plan must be thrown away unused if the
user does not advance. Plans are cheap, but a negotiated session holds a server slot, so release it on cancel.

### 4. Size the demuxer cache from installed memory (small)
Granary always reserves 768 MiB ahead plus 128 MiB back. AIOStreams uses a sixteenth of RAM, clamped to
256 MiB–1 GiB, split 3:1 forward/back (`core/src/playback.rs@bdd9cbc` `auto_cache`, `cache`). Nuvio uses
150 MiB + 50 MiB.

On a 32 GB desktop Granary's fixed size is fine. On an 8 GB HTPC it is 11 % of memory for one stream. The proposal
is RAM/16 clamped to 384 MiB–1 GiB, which keeps about 40 s of an 80 Mbit/s 4K remux at the low end. An advanced
override can follow later.

### 5. Opt-in NVIDIA RTX Video (Super Resolution and RTX HDR) (medium)
Both Stremio and Nuvio offer this through mpv's `d3d11vpp` filter:
- `d3d11vpp=scaling-mode=nvidia:scale=<s>`, with `:format=x2bgr10:nvidia-true-hdr` added when the display is in
  HDR;
- support is probed first with `ID3D11VideoContext::VideoProcessorGetStreamExtension` for the NVIDIA PPE and
  TrueHDR GUIDs (`gpu_video_processing.rs@d640dc1`).

This fits Granary's main target. On an OLED with Windows HDR always on, SDR films and episodes could be shown as HDR
by the GPU, and 1080p would be upscaled to 4K. It needs `hwdec=d3d11va` (not `-copy`), which is already Granary's
default. It should be:
- off by default (it costs GPU power and changes the picture);
- off for HDR sources;
- shown only when the probe succeeds.

Intel's VSR uses the same filter (`scaling-mode=intel`).

### 6. Lighter rendering on integrated GPUs (small)
Stremio and Nuvio detect a unified-memory GPU (iGPU or APU) and use `profile=fast`. Otherwise they use
`deband=yes`, `scale=spline36` and `cscale=spline36` (`unified_memory_architecture`, `gpu_video_processing.rs@d640dc1`).

Granary always applies `deband=yes` and gpu-next's default scalers. On a laptop or HTPC iGPU at 4K this can drop
frames. The proposal:
- detect UMA with `D3D11_FEATURE_D3D11_OPTIONS2.UnifiedMemoryArchitecture`;
- apply `profile=fast` there unless the user picked a quality setting;
- report it in the stats overlay.

### 7. A Dolby Vision switch (small)
AIOStreams lets the user turn Dolby Vision off. For profiles 7 and 8 it then adds a labelled
`vf=@…:format=dolbyvision=no`, so the HDR10 base layer plays. Profile 5 has no base layer and keeps DV
(`strip_dolby_vision`, `core/src/player.rs@bdd9cbc`).

This is useful when a file's DV metadata makes gpu-next's tone mapping look wrong on a given display. The same
profile 5 limit explains Granary's known preview gap: profile 5 has no HDR10 base, so local previews of profile 5
files have wrong colours (ADR-0009). Better options for those files:
- prefer the server or Seekr previews;
- or skip local previews when `dolby-vision-profile` is 5.

### 8. Ideas worth a look later
- **Subtitle auto-sync** (Nuvio, `features/autosync`): retimes SRT, WebVTT and ASS timestamps to a computed
  timeline, then hands the file to libmpv. It helps with downloaded subtitles that drift. Larger effort.
- **A user `mpv.conf`** (AIOStreams writes a commented template into its config folder and loads it; the app keeps
  the options it depends on fixed). It is a power-user escape hatch for shaders and scalers. Granary runs with
  `config=no` on purpose (reproducible support). If it is ever added, it should be an advanced, clearly marked
  option, and the forced layer must still win.

## Looked at and deliberately not adopted
- **`hr-seek=no`** (Nuvio). Keyframe-only seeks are faster but land up to several seconds off. Granary's exact seeks
  and the new in-place subtitle preroll depend on precise seeking.
- **Headers in `http-header-fields`** (AIOStreams, Nuvio). With them, tokens can appear in mpv's logs and
  properties. Granary's relay keeps every credential out of mpv (ADR-0008).
- **Library model with periodic pushes** (stremio-core: `PUSH_TO_LIBRARY_EVERY` = 180 s, watched at 70 %, credits
  at 90 %). Silo owns watched state on the server, and Granary's 10-second sequenced progress samples are what its
  API expects (`silo-api.md`).
- **`audio-fallback-to-null`, SMTC, shader cache**: Granary already has all three (`output.rs`, `platform.rs`,
  `commands.rs`).
