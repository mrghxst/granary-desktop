# ADR-0014: Streaming quality, transcoding and lowering quality when buffering

- Status: Accepted
- Date: 2026-11-18
- Inputs:
  - Owner request (2026-11-18): play lower quality on slow connections with the server transcoding, and make it
    "super clear" whether a title transcodes or plays directly.
  - `docs/research/silo-api.md` §4.1l and §6: settings values, connection endpoints, plan quality fields @749db96a.
  - The server's design `docs/design/2026-10-17-buffering-quality.md@749db96a` and its conformance vectors, read as
    data.

## Context

- Every playback is a plan from the server: Direct Play (`original_http`), Direct Stream (a remux) or Transcode
  (`server_transcode_hls`). Until now Granary only asked for `auto` and showed a short toast when it was not direct.
- The server already offers a quality ladder per file (`available_qualities`), a profile setting for the default
  quality and a bitrate cap, and (with `buffering_quality_v1`) a protocol for lowering quality after repeated
  buffering, remembering a learned limit per connection.
- The web app implements the buffering controller in the browser; the design defines its exact timers and outputs
  and ships 66 conformance vectors.

## Decision

- **The play method is always visible.** The player's top bar shows a coloured chip whenever the controls are up:
  green Direct Play, blue Direct Stream, amber Transcoding (or "Transcoding audio" when only the sound is converted),
  with the resolution, HDR format and bitrate the player receives. The toolbar has a quality button in the same
  colour. A new plan announces itself once: conversions always with their reason, Direct Play when playback starts or
  returns to it.
- **The quality menu explains the plan.** It shows the original and what plays (resolution, codec, dynamic range,
  bitrate, audio), why (the server's `decision_reason` in plain language), what was changed (`transformations`,
  resolution and bitrate drops) and the server's warnings. Below that: Auto and the server's rungs for the file. A pick
  replans this playback with `quality_change`; it does not change the default.
- **Defaults live on the server, per computer.** Settings → Streaming quality writes `playback.preferred_quality`,
  `playback.max_bitrate_kbps` and `playback.buffering_quality_mode` at `profile_device` scope, so a slow laptop link
  does not cap the living room. "Use profile setting" removes the device value. The values are cached locally and a
  start never waits for them. The item page offers a quality choice for one play.
- **The buffering controller runs in the UI** (`src/lib/bufferingQuality.ts`), a pure state machine that passes all
  66 vectors, driven by `useBufferingQuality`:
  - mpv's buffering, seeks, pauses and reloads become its media events;
  - `player_network` (mpv's cache speed, polled each second) tells a slow link from an outage: no data for 4 s on a
    progressive stream or 15 s on HLS while waiting is an outage, a full idle cache is not; the average non-zero cache
    speed is the observed throughput a lowering reports, and the only evidence that counts before the first frame;
  - its replans go to the host as `player_quality`; the prompt ("ask") and undo ("always") pills count down 15 s and
    wait while a menu or another prompt covers them;
  - the host keeps the step-down estimate and repeats it on its own Auto replans (seek re-anchors, track changes,
    recovery), as the design requires.
- **Feature gating.** `buffering_quality_v1` is advertised only when the server lists it; without it the estimate
  source, observed throughput and learned-limit fields are dropped before sending. The controller is off in Watch
  Party rooms (everyone plays the same source) and for audiobooks.
- **The mock server transcodes for real tests.** Rungs and caps produce `server_transcode_hls` plans that stream the
  SDR 1080p test clip; `GRANARY_MOCK_LINK_KBPS` throttles mock media so buffering and lowering can be exercised.

## Consequences

- Viewers can see at a glance whether the server converts, and why, and can pick a lower quality on a slow link.
- Not done: the bandwidth probe before a start, `plan_summary` telemetry, profile-scope settings (the web app edits
  those). A learned limit is shown in Settings with Reset.
- The controller depends on mpv's `cache-speed`; a demuxer that reads ahead in bursts makes the throughput estimate
  conservative, which errs towards lowering slightly more.
