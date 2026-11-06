# libmpv smoke test (pre-spike, 2026-11-06)

Purpose: prove that the pinned libmpv build (`mpv-dev-x86_64-v3-20261106-git-9028187538.7z`, SHA-256
`a778b344…83bb`, fetched by `scripts/fetch-libmpv.ps1`) loads, links through the generated `mpv.lib` and decodes the
HDR10 fixture with gpu-next/d3d11/d3d11va on this machine. It does **not** test embedding (`wid`) or the overlay;
that is the Phase 2 spike.

Program: a throwaway Rust binary with hand-written FFI declarations (`mpv_create`, `mpv_set_option_string`,
`mpv_initialize`, `mpv_command`, `mpv_get_property_string`, `mpv_wait_event`, `mpv_terminate_destroy`), linked with
`cargo:rustc-link-lib=dylib=mpv` against `vendor-bin/libmpv`. Options: `vo=gpu-next gpu-api=d3d11 hwdec=d3d11va
terminal=no osc=no geometry=960x540`; properties read 3 s after `loadfile`.

Machine: Intel Arc 140V (driver 32.0.101.8826), internal 2880×1800 HDR-capable panel, Windows HDR **off** at the time.

Command: `mpvsmoke.exe test-media/hdr10_hevc_main10_2160p.mkv` → exit 0

```
client api version 0x20006
init 0
loadfile 0
mpv-version = mpv v0.42.0-3-g902818753
ffmpeg-version = N-128504-g9f690c6a4
current-vo = gpu-next
current-gpu-context = d3d11
hwdec-current = d3d11va
video-params = {"pixelformat":"d3d11","hw-pixelformat":"p010","average-bpp":24,"w":3840,"h":2160,"dw":3840,"dh":2160,"crop-x":0,"crop-y":0,"crop-w":3840,"crop-h":2160,"aspect":1.777778,"aspect-name":"16:9","par":1.000000,"sar":1.777778,"sar-name":"16:9","colormatrix":"bt.2020-ncl","colorlevels":"limited","primaries":"bt.2020","gamma":"pq","sig-peak":4.926108,"light":"display","chroma-location":"mpeg2/4/h264","stereo-in":"mono","rotate":0,"alpha":"none","min-luma":0.005000,"max-luma":1000.000000,"max-cll":1000.000000,"max-fall":400.000000,"prim-red-x":0.680000,"prim-red-y":0.320000,"prim-green-x":0.265000,"prim-green-y":0.690000,"prim-blue-x":0.150000,"prim-blue-y":0.060000,"prim-white-x":0.312700,"prim-white-y":0.329000}
video-target-params = {"pixelformat":"rgb10a2","w":920,"h":518,"dw":920,"dh":518,"aspect":1.776062,"aspect-name":"16:9","par":"-nan(ind)","sar":1.776062,"sar-name":"16:9","colormatrix":"rgb","colorlevels":"full","primaries":"bt.709","gamma":"gamma2.2","sig-peak":0.394089,"light":"auto","chroma-location":"unknown","stereo-in":"mono","rotate":0,"alpha":"premul","min-luma":0.000400,"max-luma":80.000000,"max-cll":0.000000,"max-fall":0.000000}
frame-drop-count = 0
vo-delayed-frame-count = 0
display-fps = 60.000085
display-names = \\.\DISPLAY1
```

Observations:
- With Windows HDR off, mpv targets the SDR desktop (bt.709 / gamma 2.2, full range, 10-bit `rgb10a2` swapchain) and
  tone-maps the PQ source; `min-luma=0.0004` matches DXGI `MinLuminance` of the panel (OLED-like black level).
- The FFmpeg version string `N-128504-g9f690c6a4` is recorded for THIRD_PARTY_NOTICES.

Display baseline (read-only DXGI/DisplayConfig probe before any changes): `\\.\DISPLAY1` internal panel, DXGI
ColorSpace 0 (RGB full G22 P709), BitsPerColor 10, MaxLuminance 500, MaxFullFrameLuminance 500, MinLuminance 0.0004;
refresh 60.000 Hz (max 120 Hz), advanced color supported=1 enabled=0, SDR white raw 1000 (80 nits); single monitor.
