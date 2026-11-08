# ADR-0012: Rendering quality levels, GPU video processing and the Dolby Vision switch

- Status: Accepted
- Date: 2026-11-09
- Inputs:
  - `docs/research/reference-clients.md` items 5–7.
  - The owner's request for 0.4.3: make rendering "slightly" lighter on integrated GPUs, keep it possible to enable
    maximum rendering, add opt-in RTX Video, and add a Dolby Vision switch.
  - Reference: stremio-shell-ng `gpu_video_processing.rs` and `player.rs`, read as data.

## Context

- Granary renders with mpv `gpu-next` defaults plus debanding (bundled `mpv.conf`). On a 4K OLED and a discrete GPU
  that is right. On an integrated GPU (for example Intel Arc 140V), debanding and same-frame HDR peak detection are
  the costliest extras at 4K.
- NVIDIA (RTX Video Super Resolution, RTX Video HDR) and Intel (Video Super Resolution) offer driver upscaling
  through the D3D11 video processor. mpv reaches both with `vf=d3d11vpp` (`scaling-mode=nvidia|intel`,
  `nvidia-true-hdr`).
- Some Dolby Vision profile 7 and 8 files look better from their HDR10 base layer on some displays. mpv can drop the
  RPU with `vf=format=dolbyvision=no`.

## Decision

- **Where the options go.** A new module, `playback/rendering.rs`, decides the options per title.
  - `Output::prepare` adds them before each load. Settings changes and monitor moves send only the options that
    changed, because setting `vf` again re-creates the filters.
  - Every load sets every key the module owns, so one title's choice never leaks into the next.
- **Quality levels** (setting `player.renderQuality`, default `auto`):
  - `high` is mpv as configured: baseline, bundled `mpv.conf` and the user's `mpv.conf`. The values are read back
    from mpv once after start-up, so a power user's file keeps working.
  - `fast` is mpv's built-in `fast` profile, without debanding or peak detection.
  - `balanced` turns off only debanding and same-frame peak detection (`allow-delayed-peak-detect=yes`).
  - `max` is mpv's built-in `high-quality` profile plus debanding: `ewa_lanczossharp`, anti-ringing 0.6, peak
    percentile 99.995, contrast recovery 0.30.
  - `auto` is `balanced` on a unified-memory GPU (`D3D11_FEATURE_D3D11_OPTIONS2.UnifiedMemoryArchitecture`) and
    `high` elsewhere. It is always `high` when a user `mpv.conf` exists.
- **GPU video processing** (settings `superResolution` and `rtxHdr`, both off by default). Probed once on the default
  adapter:
  - RTX VSR when the driver accepts the NVIDIA extension (GUID `d43ce1b3-…`) on a test video processor.
  - RTX HDR when the driver reports the TrueHDR extension (GUID `fdd62bb4-…`).
  - Intel VSR is offered on Intel GPUs. Where the extension is missing, mpv falls back to the driver's plain scaler.
  - The filter is used only for SDR sources with hardware decoding. VSR is added only when the monitor is at least
    1.1 × the video height, with the factor capped at 4. RTX HDR is added only while the output is HDR.
  - Checked on this machine (Intel Arc 140V, mpv 0.42) with `gpu_video_filters_run`: the filters keep `d3d11va`,
    and the output size follows `scale`.
- **Dolby Vision** (setting `dolbyVision`, on by default). When off, profiles 7 and 8 play their base layer.
  Profile 5 has no base layer and always keeps Dolby Vision.
- The stats overlay shows `render-quality`, `video-processing` and, when it applies, `dolby-vision`.

## Consequences

- A non-`high` level overrides scaler keys from a user `mpv.conf`. That is the point of choosing it, and `auto`
  never does it when a user file exists.
- A live debanding toggle (the `deband` key in the player) is reset at the next title. Earlier it lasted for the
  session.
- An NVIDIA probe failure only hides the RTX options. The level and Dolby Vision settings work everywhere.
