# R3: Shell and player (Tauri 2 + mpv, window and overlay architecture)

- Date: 2026-11-06 (all versions observed on this date unless noted)
- Author: research subagent R3
- Scope: Tauri 2 on Windows/WebView2, mpv integration options, the video plus overlay window architecture, pitfalls,
  prior art, and recommendations for ADR-0002 (player integration) and ADR-0003 (window/overlay architecture).
- Related: R4 [windows-display-audio.md](windows-display-audio.md) (HDR/mpv.conf baseline), R5
  [packaging-and-licensing.md](packaging-and-licensing.md) (libmpv builds, licensing), ADR-0001 (own React UI).

## Summary

1. Tauri **2.12.5** is the current stable crate (2026-10-21). `@tauri-apps/cli` and `@tauri-apps/api` are **2.12.4**. 3.0.0-alpha.5 exists, so stay on 2.x.
2. Tauri exposes `WebviewWindow::hwnd()` on Windows, plus `owner`/`owner_raw`/`parent_raw`, `transparent`, `no_redirection_bitmap`, `skip_taskbar`, `focusable` and `set_ignore_cursor_events`. wry 0.57.1 hosts WebView2 in **windowed** mode inside a `WRY_WEBVIEW` child HWND. It has no composition-controller hosting.
3. **Key finding:** libmpv's render API, including the new **D3D11 render backend in mpv 0.42.0 (2026-11-05)**, still runs on the legacy `vo_gpu` renderer, not `gpu-next`/libplacebo. Upstream issue #10810 is still open. Using HDR with gpu-next therefore means embedding mpv by **window (`--wid`)**, not through the render API.
4. **Recommended crate: `libmpv2` 7.0.0** (2026-10-30, LGPL-2.1) with `libmpv2-sys` 4.0.1. Its bundled headers are client API 2.5, and mpv 0.42 is API 2.6. Stremio's production Rust/WebView2 shell uses the same crate family.
5. **Linking:** `libmpv2-sys` emits `rustc-link-lib=mpv` and no search path. shinchiro's `mpv-dev-x86_64` package ships `libmpv-2.dll`, `libmpv.dll.a` and headers, but no `.def`. Generate `mpv.lib` with `lib /def` from the DLL's exports, then add a link-search path in our `build.rs`.
6. **Prior art that matches our stack:** `stremio-shell-ng` (Rust, WebView2, libmpv2) uses **one top-level window**. mpv renders through `wid` = main HWND with `vo=gpu-next`, d3d11 and `target-colorspace-hint`. A windowed WebView2 controller with `DefaultBackgroundColor` alpha 0 sits on top in the same window. There is no second overlay window.
7. **Primary design for ADR-0003:** keep a single Tauri main window. Add a raw Win32 video child HWND **below** the WRY WebView2 container, set mpv `--wid` to that child, and keep the WebView2 transparent. Hide the WebView2 entirely when the controls are hidden, so the video swapchain can run without the webview on top.
8. **Fallback:** a borderless video top-level window owns a transparent, owned WebView2 overlay (`owner`, `skip_taskbar`, `transparent`, `no_redirection_bitmap`). Its position is synced from the owner's window-position messages. **Spike option:** mpv's new `--d3d11-output-mode=composition` + `display-swapchain` attaches mpv's swapchain to a DirectComposition visual behind the webview. It is in-process only and gives up automatic HDR target detection.
9. **Main risks:**
   - Independent flip, MPO and VRR may not engage for a swapchain in a child HWND. This must be measured with PresentMon.
   - The cost of a full-screen transparent WebView2 at 4K and high refresh.
   - A crash in libmpv or a GPU driver takes the whole app down, since libmpv runs in-process.
   - The GPL/LGPL implications of the libmpv build we ship (R5).
   - mpv 0.42 and the matching builds are one day old.

---

## 1. Tauri 2 on Windows / WebView2

### 1.1 Versions (observed 2026-11-06)

| Component | Version | Published | Notes |
|---|---|---|---|
| `tauri` (crate) | **2.12.5** | 2026-10-21 | `3.0.0-alpha.5` published 2026-10-14 (pre-release, not used) |
| `@tauri-apps/cli` (npm) | **2.12.4** | — | npm `latest` tag |
| `@tauri-apps/api` (npm) | **2.12.4** | — | npm `latest` tag |
| `wry` (transitive) | 0.57.1 | — | docs.rs latest; WebView2 host |
| `tauri-plugin-single-instance` | **2.5.5** | 2026-10-28 | |
| `tauri-plugin-window-state` | **2.5.0** | 2026-09-26 | |
| `tauri-plugin-log` | **2.10.1** | 2026-10-10 | |
| `tauri-plugin-opener` | **2.7.2** | 2026-10-14 | |
| `tauri-plugin-os` | **2.4.0** | 2026-09-26 | |
| `tauri-plugin-process` | **2.4.0** | 2026-09-26 | |
| `tauri-plugin-store` | **2.5.0** | 2026-09-26 | |
| `windows` (crate) | 0.62.2 | 2025-10-06 | Win32/DXGI/DComp bindings for our own HWND work |
| `raw-window-handle` | 0.6.2 | 2024-05-17 | Tauri implements `HasWindowHandle` |
| `webview2-com` | 0.39.1 | 2026-03-11 | Only needed if we touch `ICoreWebView2Controller` directly |
| `libloading` | 0.9.0 | 2025-11-05 | Only for the runtime-loading option |
| WebView2 SDK docs | 1.0.4314.35 | docs dated 2026-10-19 | Reference version on learn.microsoft.com |

The npm guest packages (`@tauri-apps/plugin-*`) were not checked one by one. Pin each to the same minor version as its crate, and confirm the versions with `npm view` when scaffolding.

### 1.2 Getting the raw HWND

- `WebviewWindow::hwnd(&self) -> crate::Result<HWND>`, `#[cfg(windows)]`, doc "Returns the native handle that is used by this window" (tauri 2.12.5 `webview_window.rs` L1964–1966). The same method exists on `Window`.
- Tauri also implements `raw_window_handle::HasWindowHandle` (`window_handle()`), which returns `RawWindowHandle::Win32 { hwnd, .. }`. Third-party plugins use this pattern to avoid a direct `hwnd()` dependency.
- The webview's own HWND differs from the top-level window's. wry creates a container child window of class `WRY_WEBVIEW` with `WS_CHILD | WS_CLIPCHILDREN` under the top-level HWND, and attaches the WebView2 controller to that container. `WebviewWindow::with_webview(|PlatformWebview| …)` (feature `wry`, runs on the main thread) gives access to the `ICoreWebView2Controller`, for example for `put_IsVisible`, `MoveFocus` or `put_DefaultBackgroundColor`.

### 1.3 Additional windows: builder options relevant to us

All of these are `WebviewWindowBuilder` methods in tauri 2.12.5:

| Need | API | Windows notes from docs |
|---|---|---|
| Transparent window | `transparent(bool)` | "using `no_redirection_bitmap` can help avoid a white flash when creating a transparent window" |
| No redirection bitmap | `no_redirection_bitmap(bool)` | "This sets `WS_EX_NOREDIRECTIONBITMAP`", Windows only |
| Borderless | `decorations(false)` | `shadow(true)` on undecorated windows adds a 1px border and Win11 rounded corners; `shadow(false)` has no effect on decorated windows |
| Always on top | `always_on_top(bool)` | (not needed for an owned overlay: an owned window is always above its owner) |
| Owner (Windows) | `owner(&WebviewWindow)` / `owner_raw(HWND)` | MSDN semantics quoted in source: z-order above owner, destroyed with owner, hidden when owner minimized |
| Parent | `parent(&WebviewWindow)` | **On Windows this sets the passed window as owner** (not WS_CHILD) |
| True child | `parent_raw(HWND)` | `WS_CHILD`, "confined to the client area of its parent window" |
| No taskbar button | `skip_taskbar(bool)` / `set_skip_taskbar` | unsupported on macOS only |
| Focus | `focused(bool)`, `focusable(bool)` / `set_focusable` | |
| Click-through | `set_ignore_cursor_events(bool)` | desktop only. The Win32 implementation was not re-verified in this pass (it is believed to toggle `WS_EX_TRANSPARENT`/`WS_EX_LAYERED`) |
| Background | `background_color(Color)` | "alpha channel is ignored for the window layer"; WebView2 accepts only alpha 0 or 255 |
| Browser args | `additional_browser_args(&str)` | Windows only. Webviews with different args need different data directories. wry defaults to `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` |
| Custom scheme origin | `use_https_scheme(bool)` | switches to `https://<scheme>.localhost`. **Changing it later moves localStorage/IndexedDB/cookies**, so decide once |
| Window class | `window_classname(..)` | useful for identifying our windows in hooks and Spy++ |

**`transparent` on Windows, caveats:**
- WebView2's `DefaultBackgroundColor` supports only fully transparent or opaque. "Semi-transparent colors are not currently supported"; any other alpha returns `E_INVALIDARG`. wry forces alpha to 255 unless it is 0.
- Setting the color through the API can still show a **white flicker** before it takes effect. The `WEBVIEW2_DEFAULT_BACKGROUND_COLOR` environment variable avoids this (documented workaround). Tauri adds `no_redirection_bitmap` for the same reason.
- Resizing WebView2 is slow in general, especially with hardware acceleration (tauri #6322, discussed with WebView2 110). A Tauri 2 developer building a transparent widget reported flicker and pixel jumps on every resize (anecdotal, Product Hunt write-up). Undecorated-window resizing moved to native handling in tauri-runtime-wry 2.0.0-beta.19.

### 1.4 Custom URI scheme protocols

- `Builder::register_uri_scheme_protocol(scheme, |ctx: UriSchemeContext<R>, req: http::Request<Vec<u8>>| -> http::Response<T>)` is synchronous. `register_asynchronous_uri_scheme_protocol(scheme, |ctx, req, responder: UriSchemeResponder| …)` lets the handler finish on another thread with `responder.respond(response)`.
- On Windows the URL is `http://<scheme>.localhost/<path>` by default, or `https://` with `use_https_scheme`. The docs warn that the Origin differs per platform.
- Under ADR-0001 the UI is our own bundle, which Tauri serves via its default `tauri` protocol. A custom **async** scheme (for example `granary://` → `http://granary.localhost/...`) is the right tool for **artwork and image proxying**: the Rust side attaches the auth token from ADR-0004, streams bytes, and caches. Use the async variant so network I/O never blocks the WebView2 thread.
- `on_web_resource_request` runs only for the `tauri` protocol.

### 1.5 Capabilities and permissions per window

- Capability files live in `src-tauri/capabilities/*.json|toml` and select windows by **label** (`"windows": ["main"]`, `"*"` allowed). An optional `platforms` array restricts targets.
- Once `tauri.conf.json` explicitly lists capabilities, only those are used. Remote URLs get IPC only through an explicit `remote.urls` block, which we will not use.
- Security boundaries depend on window labels, not titles. The docs recommend allowing only higher-privilege windows to create new windows.
- For the fallback design: the `overlay` window gets a capability containing only the player commands. The `main` window gets library and settings commands, and the plugins.

### 1.6 CSP

- CSP protection applies only when a CSP is set in the Tauri config. Tauri appends nonces and hashes for bundled scripts and styles at compile time.
- The docs example uses `connect-src ipc: http://ipc.localhost`. Custom schemes must be listed explicitly (for example `img-src 'self' asset: http://asset.localhost`). For us that means `img-src 'self' http://granary.localhost`.
- Avoid CDN or remote scripts.

### 1.7 IPC: commands, events, channels

- **Commands** take JSON arguments by default, and async commands run off the main thread. For large payloads, `tauri::ipc::Response` returns raw bytes and `tauri::ipc::Request` exposes the raw body (`InvokeBody::Raw`).
- **Events**: JSON payloads, "not designed for low latency or high throughput", and they evaluate JS under the hood. Fine for rare state changes such as `player:ended` or window state.
- **Channels** are "designed to be fast and deliver ordered data". Tauri uses them internally for download progress and child-process output. **Use a `Channel<PlayerEvent>`** passed in a `player_subscribe` command for the mpv property stream (`time-pos`, `pause`, `cache-*`, track lists). Throttle `time-pos` to about 4–10 Hz in Rust.
- `WebviewWindow::eval` exists, but it is not needed.

---

## 2. mpv integration options

### 2.1 Context: mpv itself (observed 2026-11-06)

- **mpv 0.42.0** was released 2026-11-05. Notes include:
  - "libmpv: add D3D11 render API backend"
  - "scRGB output with `--target-trc=scrgb`"
  - "d3d11: support `--d3d11-output-format` fully for `gpu-next`"
  - "w32_common: refactor window resizing"
  - "input/ipc-win: allow Windows handles for --input-ipc-client"
- **mpv 0.41.0** added "d3d11 composition mode support", "support --wid=0", and "handle runtime --wid change".
- Client API history (`DOCS/client-api-changes.rst`):
  - **2.6** (mpv 0.42.0) adds `MPV_RENDER_API_TYPE_D3D11`.
  - 2.5 (0.40.0) deprecates `MPV_RENDER_PARAM_AMBIENT_LIGHT`.
  - 2.4 (0.39.0).
- **The render API does not use gpu-next.** `video/out/d3d11/libmpv_d3d11.c` fills `struct libmpv_gpu_context_fns libmpv_gpu_context_d3d11` and includes `video/out/gpu/libmpv_gpu.h` and `ra_d3d11.h`, which is the `vo_gpu` abstraction with no libplacebo `pl_*` types.
  - Issue **#10810** ("libplacebo/gpu-next support in the render API") is **open**, under milestone "Fix vo=gpu-next limitations".
  - The only PR, #18607 (2026-10-13), is an RFC for an OpenGL-only `MPV_RENDER_API_TYPE_OPENGL_NEXT`. A maintainer (kasper93) commented that the render API "needs a proper design" for libplacebo-based backends.
  - Jellyfin maintainers concluded that gpu-next is "effectively impossible" in their render-API-based client (jellyfin-media-player #362).
- **Consequence:** mpv/libplacebo (`vo=gpu-next`) with Windows HDR signalling (`target-colorspace-hint`, which needs the D3D11 context and gpu-next) is available **only when mpv owns the swapchain**: its own window, `--wid`, or `--d3d11-output-mode=composition`.
- Relevant D3D11 options (options.rst, master):
  - `--d3d11-flip` (default yes, flip model).
  - `--d3d11-exclusive-fs` (default off).
  - `--d3d11-output-format` (auto; with gpu-next a hint, and `rgba16f` gives scRGB).
  - `--d3d11-output-csp` (auto = desktop colorspace; only srgb and pq reliable).
  - `--target-colorspace-hint` (auto/yes/no, gpu-next + d3d11) and `--target-colorspace-hint-mode` (target/source/source-dynamic).
  - `--d3d11-output-mode=<auto|window|composition>` and `--d3d11-composition-size=<WxH>`.
- `--wid` on win32 (options.rst): the value is an HWND. "mpv creates its own window and sets the `wid` window as its parent." `--wid=-1` detaches at runtime.
  - Source (`w32_common.c`): `CreateWindowExW(WS_EX_NOPARENTNOTIFY, …, WS_CHILD | WS_VISIBLE, 0, 0, r.right, r.bottom, w32->parent, …)`, followed by `EnableWindow(w32->window, 0)`.
  - A **parent hook keeps the child sized to the parent client rect** via `SetWindowPos(…, SWP_ASYNCWINDOWPOS | …)`.
  - Because mpv's child is disabled, mouse and keyboard input is meant to land in the host window, so the host owns input. This is an inference from the code.

### 2.2 (a) In-process libmpv from Rust: crate survey

| Crate | Latest | Released | Status | Client API headers | Render API | Property observation / events | Notes |
|---|---|---|---|---|---|---|---|
| **`libmpv2`** (kohsine/libmpv-rs) | **7.0.0** | 2026-10-30 | **Active** (6.0.0 May 2026, 6.0.1 and 7.0.0 Oct 2026); 115k downloads, 67 stars; LGPL-2.1 | via `libmpv2-sys` **4.0.1** (2025-12-29), bundled `client.h` = **API 2.5**; optional `use-bindgen` | Yes, `Mpv::create_render_context()` (OpenGL example; since 6.0.0 the `RenderContext` borrows `Mpv`) | `observe_property(name, Format, id)`, `unobserve_property` (fixed in 7.0.0), `wait_event(timeout)`, `set_wakeup_callback`, `create_client(name)`, `enable/disable_event`; `Mpv: Send + Sync` | Used by Stremio shell-ng (`libmpv2 = 6.0.0`, `libmpv2-sys = 4.0.1`). 5.0.0 removed `mpv_node` support (use JSON strings). 7.0.0: `MPV_FORMAT_NONE` returns `None` instead of panicking |
| `libmpv2-sys` | 4.0.1 | 2025-12-29 | Active (same repo) | API 2.5 headers (`client.h`, `render.h`, `render_gl.h`, `stream_cb.h`) | n/a (raw FFI) | n/a | `build.rs`: `cargo:rustc-link-lib=mpv`, **no** `rustc-link-search` |
| `libmpv` (ParadoxSpiral) | 2.0.1 | 2020-09-29 | **Unmaintained** | pre-2.0 libmpv API | partial | yes | Superseded by the `libmpv2` fork |
| `libmpv-sys` (ParadoxSpiral) | 3.1.0 | 2020-07-22 | **Unmaintained** | old headers | — | — | |
| `mpv-client` (TheCactusVert) | 1.1.0 | 2025-06-28 | Low activity | — | No | yes | Built for writing mpv **plugins** (cplugins) in Rust, not for hosting mpv |
| `tauri-plugin-libmpv` (nini22P) | 0.3.2 | — | Small (23 stars, 20 commits) | via its own C `libmpv-wrapper.dll` | — | JS API | Requires a transparent Tauri window; Windows "fully tested", Linux embedding broken. MPL-2.0 (0.0.x–0.2 were LGPL). Adds a second native DLL |
| `tauri-plugin-mpv` (nini22P) | 0.5.2 | 2025-11-18 | Small | n/a (JSON IPC to mpv in PATH) | — | via IPC | Process plus JSON IPC, transparent window. Many yanked versions |
| `mpvipc` | 1.3.1 | 2026-02-15 | Active | n/a | — | IPC | Socket-based. Windows named-pipe support not confirmed |

Own bindings (bindgen over the mpv headers) are a reasonable alternative. jellium-desktop does this (`bindgen = "0.73"` in its `jfn-mpv` crate). Its advantage is access to `render_d3d11.h` and API 2.6, which `libmpv2-sys` 4.0.1 lacks. We do not need the render API, so `libmpv2` is enough. Options like `d3d11-output-mode` are plain string properties and need no new headers.

**Event loop pattern (as Stremio does it):**
- One dedicated thread calls `wait_event(0.1)` on a separate client handle (`create_client`).
- It drains a channel of `observe_property` requests and forwards events.
- Commands are issued from other threads, since `Mpv` is `Sync`.
- Alternatively, use `set_wakeup_callback` to wake an async task. The callback "must not call client API functions".

### 2.3 Linking libmpv on Windows MSVC

- The **shinchiro** build (`mpv-winbuild-cmake`) has its latest release tag **20261106**, with `mpv-dev-x86_64-20261106-git-9028187538.7z` (30.3 MB) and `mpv-dev-x86_64-v3-…` (31.5 MB, x86-64-v3 CPUs only). According to `packages/mpv.cmake`, the dev package contains:
  - `libmpv-2.dll`
  - `libmpv.dll.a` (MinGW import library)
  - `include/mpv/client.h`, `stream_cb.h`, `render.h`, `render_gl.h`. The script observed does not copy `render_d3d11.h`. **No `.def` file.**
- **zhongfly** (`mpv-winbuild`) has its latest tag `2026-11-05-3138fc8eea`. It publishes LGPL FFmpeg variants. `tauri-plugin-libmpv` tells users to use zhongfly's `mpv-dev-lgpl` build, but that asset was not visible in the collapsed listing. Licensing choice: see R5.
- **Option L1: import library via `lib.exe /def` (recommended).**
  1. Produce a `.def` from the DLL exports, either with `gendef - libmpv-2.dll > mpv.def` (MSYS2) or from `dumpbin /exports`. Make sure the file starts with `LIBRARY libmpv-2` / `EXPORTS`.
  2. Run `lib /def:mpv.def /name:libmpv-2.dll /out:mpv.lib /machine:x64`. The OpenSpace and C-Play guides document this flow. Older guides use `mpv-2.dll`; current builds name the DLL `libmpv-2.dll`.
  3. Our `src-tauri/build.rs` prints `cargo:rustc-link-search=native=<dir with mpv.lib>`, because `libmpv2-sys` only prints `rustc-link-lib=mpv`.
  4. Machine type must match, or you get LNK4272 (Jellyfin forum).
  5. Commit the generated `mpv.lib` and the `.def`, or regenerate them in CI from the pinned DLL.
  - Variant (not verified here): MSVC's `/DELAYLOAD:libmpv-2.dll` plus `delayimp.lib`, passed via `cargo:rustc-link-arg`, would let the app start and show a clear error if the DLL is missing or corrupted.
- **Option L2: use `libmpv.dll.a` directly.** MSVC `link.exe` can often consume a GNU import library renamed to `mpv.lib`. This is unverified and not guaranteed, so avoid it.
- **Option L3: runtime loading via `libloading` 0.9.0.** We would write our own function table (`mpv_create`, `mpv_initialize`, `mpv_set_property`, `mpv_observe_property`, `mpv_wait_event`, …) and resolve it at runtime.
  - Pros: no import library, graceful failure, the DLL can be swapped or upgraded.
  - Cons: we cannot use `libmpv2`'s safe wrapper (it links statically against `libmpv2-sys`), so we would maintain an unsafe FFI layer ourselves.
  - Only worth it if we write our own bindings anyway.

### 2.4 (b) Bundled `mpv.exe` controlled over JSON IPC

- Start it with `--input-ipc-server=\\.\pipe\granary-<random>`. The protocol is line-delimited JSON with optional integer `request_id`s, async commands, and `observe_property` events of type `property-change`.
  - Events can interleave with replies. "The only guarantee is that replies to IPC messages are sent in sequence."
  - **Observations are lost if the connection closes.**
  - On Windows you need overlapped I/O to read and write at the same time.
- mpv 0.42 allows **inherited Windows handles for `--input-ipc-client`**. We could create an anonymous or duplex pipe and pass the handle, which avoids pipe-name squatting.
- Embed with `--wid=<our HWND>`. This works cross-process: mpv creates a `WS_CHILD` window under our HWND from its own process.
  - Not verified in this pass: cross-process parent/child windows attach the threads' input queues, so a hang in one process can stall the other. Treat this as a design risk to test.
- The approach is "fully tested" on Windows by `tauri-plugin-mpv`, which uses a transparent Tauri window with mpv beneath it.

### 2.5 Comparison: in-process libmpv vs mpv.exe + JSON IPC

| Criterion | (a) libmpv in-process (`libmpv2`) | (b) mpv.exe + named-pipe JSON IPC |
|---|---|---|
| Command/property latency | Direct C calls; no serialization | JSON encode/decode plus a pipe round trip per message; sub-millisecond expected but unmeasured |
| Property observation | `mpv_observe_property`, typed formats, `wait_event` thread | `observe_property` → `property-change` JSON events; lost on reconnect |
| Crash isolation | **None**: a decoder, driver or libplacebo crash kills Granary | **Good**: restart mpv, keep the UI alive |
| Startup | DLL load + `mpv_initialize` (fast, no process spawn) | Process spawn + pipe connect/retry; a second process per playback or a warm standby |
| wid embedding | Same process; well-trodden (Stremio, mpv.net-style hosts) | Cross-process child window; shared input queue risks (see above) |
| DComp composition mode (`display-swapchain`) | Possible (pointer valid in-process) | **Impossible**: DirectComposition targets and swapchain pointers do not cross processes |
| HDR / gpu-next | Same (mpv owns the swapchain) | Same |
| Packaging | `libmpv-2.dll` (+ `mpv.lib` at build time) | `mpv.exe` (+ any DLLs it needs); must keep users' global `mpv.conf` out (`--config-dir`/`--no-config`) |
| Licensing | Linking GPL libmpv makes a combined work; an LGPL libmpv (`-Dgpl=false`, LGPL FFmpeg) is needed if Granary is not GPL-compatible (R5) | Separate executable; arms-length use is arguably simpler, but needs a legal opinion (R5) |
| Debuggability | One process; mpv logs via `request_log_messages`/events | Separate logs; can be run by hand with the same args |
| Prior art | Stremio shell-ng, jellium-desktop, JMP (render API), Plex HTPC (libmpv-2.dll) | tauri-plugin-mpv, jellyfin-mpv-shim (external player) |

---

## 3. Window architecture (mpv-owned D3D11 swapchain + WebView2 controls)

### 3.1 Relevant platform facts

- **Owned windows** (MSDN "Window Features"):
  - An owned window "is always above its owner in the z-order", is destroyed with its owner, and "is hidden when its owner is minimized".
  - Only overlapped or pop-up windows can own, and "an application cannot transfer ownership" after creation.
  - If the specified parent is a child window, ownership goes to its top-level ancestor.
- **Layered windows:** since Windows 8, `WS_EX_LAYERED` also works on child windows. Hit testing of a layered window follows its shape and alpha. With `WS_EX_TRANSPARENT`, "the mouse events will be passed to other windows underneath".
- **DirectComposition** (`IDCompositionDevice::CreateTargetForHwnd`):
  - Each window has four layers: (1) content drawn or `Present`ed directly to the HWND, (2) an optional DComp tree, (3) child windows, (4) an optional topmost DComp tree.
  - DComp content "is always composed on top of" the window's own HDC or `Present` content.
  - The window "must belong to the calling process", otherwise the call returns `DCOMPOSITION_ERROR_ACCESS_DENIED`.
  - At most one topmost and one non-topmost target per window.
- **Flip model, DirectFlip, Independent Flip and MPO** (MS "For best performance, use DXGI flip model"):
  - DirectFlip needs the window client area to cover the screen, possibly with panel fitters. MPO allows an "alpha-blended sub-region" via a hardware plane.
  - Once DirectFlipped, frames go out in "Independent Flip".
  - "If other desktop contents come on top, the DWM can either seamlessly transition back to composed mode, efficiently 'reverse compose'… or leverage MPO."
  - Verify with PresentMon.
- **HDR composition:**
  - With Advanced Color on, DWM composes in FP16 scRGB (CCCS) and converts each window's colorspace before blending.
  - HDR swapchains must be flip model.
  - "If your app always renders SDR and HDR to separate surfaces, and relies on OS composition, then Windows will automatically… boost SDR content to the desired white level." This is exactly the video swapchain plus SDR WebView2 case, so the overlay needs no manual SDR-white handling.
  - HDR10 (`R10G10B10A2` + `SetColorSpace1(G2084_P2020)`) is allowed only if the swapchain "doesn't require blending with alpha/transparency", which holds for the video layer.

### 3.2 Candidate designs

#### Design A: single Tauri window, sibling child HWNDs (recommended primary)

```
Tauri main window (top-level, decorated or custom chrome, opaque black bg)
 ├─ [z-top]    WRY_WEBVIEW container (WS_CHILD) → WebView2 (windowed controller), DefaultBackgroundColor A=0
 └─ [z-bottom] GranaryVideo child HWND (our class, WS_CHILD|WS_CLIPSIBLINGS, black brush)
                └─ mpv child window (created by mpv for --wid, WS_CHILD, disabled) → D3D11 flip swapchain
```

- **Evidence:** Stremio shell-ng ships exactly this topology in production on Windows.
  - `wid` = main window HWND.
  - `vo=gpu-next,gpu`, `gpu-context=d3d11`, `d3d11-output-format=auto`, `d3d11-output-csp=auto`, `target-colorspace-hint=auto`, `target-colorspace-hint-mode=target`, `tone-mapping=bt.2390`, `hwdec=auto`.
  - A WebView2 windowed controller created on the same parent with `put_default_background_color({255,255,255,a:0})`, `--autoplay-policy=no-user-gesture-required`, and `move_focus(Programmatic)` on creation and on `WM_SETFOCUS`.
  - No layered styles, no second window. `tauri-plugin-libmpv` reports the equivalent working in Tauri on Windows, using a transparent window and a transparent body.
- **Creating the video HWND:** use the `windows` crate. Call `RegisterClassExW` (black `hbrBackground`, `CS_DBLCLKS`), then `CreateWindowExW(0, "GranaryVideo", …, WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS, 0, 0, w, h, tauri_hwnd, …)`, then `SetWindowPos(video, HWND_BOTTOM, …, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`.
  - Create it **before** passing it to mpv. Then mpv's child lives inside ours, and we never race mpv's own window creation for z-order. With `wid` = the Tauri top-level HWND, mpv's new child would land above the WebView2 sibling.
  - Do not use a Tauri child webview for video: there is nothing to render in it.
- **Keeping it aligned:** subclass the Tauri top-level HWND with `SetWindowSubclass`. On `WM_SIZE` and `WM_DPICHANGED`, resize the video child synchronously to the client rect. mpv's own parent hook then resizes its child. Tauri `WindowEvent::Resized` arrives via the event loop and can lag a frame, so do not rely on it. Minimize, maximize, move and snap need nothing, because child windows follow their parent.
- **Fullscreen:** use Tauri `set_fullscreen(true)` on the top-level (borderless window covering the monitor). Never use mpv's fullscreen in `wid` mode, and keep `d3d11-exclusive-fs=no`. mpv 0.42 resizes buffers when DXGI leaves exclusive fullscreen.
- **Input and focus:** while the WebView2 is visible it covers the whole client area and receives all input. The React player chrome implements double-click to fullscreen, wheel to volume, and key bindings, then calls Rust commands. No click-through toggling is needed.
- **Hiding controls:** after the idle timeout, call `controller.put_IsVisible(false)`, or `ShowWindow(SW_HIDE)` on the `WRY_WEBVIEW` container. This removes the webview from composition entirely.
  - Our `GranaryVideo` wndproc (the mpv child is disabled) then receives `WM_MOUSEMOVE` and `WM_KEYDOWN`. On those we re-show the webview, call `MoveFocus`, and forward the key to the UI.
  - Keep the hide/show transition free of layout changes, so the webview resumes without a reflow.
- **DPI:** a single top-level means a single DPI. Handle `WM_DPICHANGED` with the suggested rect, as Stremio does. mpv sizes in physical pixels.
- **Pros:**
  - Native taskbar, Alt+Tab, snap layouts, minimize/restore, multi-monitor moves.
  - No position syncing and no activation games.
  - Smallest amount of Win32 code. Matches proven prior art.
- **Cons and risks:**
  - The video swapchain is in a **child** HWND. Whether DWM can promote a child-window swapchain to Independent Flip or MPO (and hence VRR) when the webview is hidden is **unverified**. Spike S1 measures it with PresentMon.
  - While controls are visible, DWM always composes the transparent full-window WebView2 over the video (GPU cost at 4K/240 Hz; spike S2).
  - Tauri `transparent: true` may make the top-level itself see-through to the desktop before the video draws. Prefer an opaque window with webview background alpha 0 if wry honours that (spike S3).

#### Design B: owned transparent overlay window (fallback)

```
Video window (top-level, Tauri WebviewWindow-less raw HWND or a Tauri window, decorations as needed) ← taskbar/Alt+Tab
 └─ mpv child (--wid) → D3D11 swapchain
Overlay window (Tauri WebviewWindow: owner(video), decorations:false, transparent:true,
                no_redirection_bitmap:true, skip_taskbar:true, shadow:false, resizable:false)
```

- **Z-order:** owner semantics keep the overlay above the video window without `always_on_top`. Do not use topmost: it would float above other apps.
- **Alignment:** subclass the video window. In `WM_WINDOWPOSCHANGED`, read the client rect in screen coordinates and apply it to the overlay with `SetWindowPos(overlay, nullptr, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER)`. Batch with `DeferWindowPos` when several windows move.
  - Minimize hides owned windows automatically.
  - On maximize and fullscreen, the client-rect update covers it.
  - On `WM_DPICHANGED`, apply the owner's suggested rect, then resync the overlay. Both windows may briefly sit on monitors with different DPI.
  - Expect a one-frame lag of the overlay during interactive drag-move and resize. This is an inherent cost of the two-window approach.
- **Activation:** clicking the overlay activates it, so the owner's frame looks inactive. Either keep the video window undecorated (custom chrome drawn in the overlay), or handle `WM_NCACTIVATE` on the owner to keep it painted active while the overlay has focus. `WS_EX_NOACTIVATE` on the overlay would break WebView2 keyboard focus, so do not use it.
- **Hiding controls:** `ShowWindow(overlay, SW_HIDE)`. The video window is then the only thing on screen. If spike S1 finds that child-window swapchains cannot independent-flip, variant **B′** applies: let mpv create **its own top-level window** (no `--wid`; take the HWND from the `window-id` property, hook it) and make the overlay owned by it.
  - jellium-desktop does this. Its comment reads: "a top-level window here (no `--wid`)". It reads mpv's observed `window-id`, installs a `WH_CALLWNDPROCRET` hook on mpv's UI thread for `WM_SIZE`/`WM_MOVE`/`WM_DPICHANGED`/`WM_CLOSE`, and detects fullscreen from mpv's styles.
- **Pros:** the overlay can vanish completely, and the video window can be a plain top-level window with nothing composed over it.
- **Cons:**
  - Position sync, activation and inactive-frame handling.
  - Snap layouts and Aero Snap operate on the owner only (the overlay follows).
  - More pitfalls (section 4).
  - Two Tauri windows mean two capability sets.

#### Design C: single HWND, mpv composition swapchain under the webview (spike only)

- Use `--d3d11-output-mode=composition` (mpv 0.41+) with `--d3d11-composition-size=WxH`. mpv creates **no window**, and publishes the swapchain pointer in the `display-swapchain` property.
- The app binds it to an `IDCompositionVisual` in a **non-topmost** DComp target on the Tauri top-level HWND. That is layer 2, so the `WRY_WEBVIEW` child (layer 3) composes above it.
- **Pros:** no mpv HWND at all, and resize is driven by us. The video rect can be anything, for example an inline trailer or mini-player.
- **Cons:**
  - In-process only.
  - The mode is new and "intended for WinUI apps".
  - In composition mode mpv's `d3d11_target_color_space` returns `{0}` and the reference-luma query returns 0, so **automatic display/HDR target detection is off**. We must feed `target-trc`/`target-prim`/`target-peak`/`d3d11-output-csp` from `DXGI_OUTPUT_DESC1` ourselves and update them on monitor changes.
  - Not known to be used by any shipping player.
  - Treat it as a research spike (S4), not the plan.

#### Rejected: libmpv render API into our own D3D11/DComp surface

Cleanest compositing (WebView2 visual hosting or DComp over our own swapchain), but the render API is `vo_gpu`-only, even with the new D3D11 backend. We would lose gpu-next/libplacebo, Dolby Vision handling and `target-colorspace-hint`. The app would also have to manage the HDR swapchain colorspace itself, since `render_d3d11.h` documents no colorspace parameters. This is the reason JMP never got gpu-next and the reason the Jellyfin CEF rewrite moved to "the same pipeline standalone mpv uses".

#### Note on the airspace problem

Classic "airspace" problems (HWND content always drawn over or under a framework's composition) do not stop Design A. WebView2's windowed controller and mpv's flip swapchain are both DWM-composed child HWNDs with z-order between siblings, and Stremio demonstrates transparency working. True airspace would only bite if we used WebView2 **composition hosting** (`CreateCoreWebView2CompositionController`) inside a window that also contains an mpv child HWND. Tauri/wry does not support composition hosting anyway; it calls `CreateCoreWebView2Controller[WithOptions]`.

### 3.3 Design comparison

| Aspect | A: single window, sibling children | B: owned overlay window | C: DComp composition swapchain |
|---|---|---|---|
| Proven in production | **Yes** (Stremio shell-ng, Rust + WebView2 + libmpv2) | Partially (jellium's mpv-owned top-level + overlay, but with CEF OSR via DComp) | No |
| Alignment on move/resize/DPI | Automatic (children) + 1 synchronous resize | Manual sync, one-frame lag possible | Manual (`d3d11-composition-size`) |
| Taskbar, Alt+Tab, snap, minimize | Native | Owner only; needs care | Native |
| Focus / activation | Single window; `MoveFocus` to WebView2 | Overlay activation vs owner frame | Single window |
| Overlay fully removable | Yes (`put_IsVisible(false)`) | Yes (`SW_HIDE`) | Yes |
| Independent flip / MPO when hidden | **Unverified** (child-window swapchain) | B: same question; B′ (mpv top-level): classic case | Composition swapchain; unverified |
| HDR auto-detection | Yes (window mode) | Yes | **No** (manual target params) |
| Tauri compatibility | Good (wry child + our child) | Good (two windows) | Good, but custom DComp code |
| Implementation size | Small | Medium | Medium–large |

---

## 4. Pitfalls

- **Focus stealing:**
  - `SetForegroundWindow` is restricted (the caller must be the foreground process, started by it, or have received the last input…).
  - Never call it from the playback path. Use `WebviewWindow::set_focus` only in response to user input or single-instance activation.
  - In B, showing the overlay must use `SW_SHOWNOACTIVATE`/`SWP_NOACTIVATE`.
  - Calling `MoveFocus` on the WebView2 changes focus only inside our own window.
- **Alt+Tab and taskbar:** only the main/video window should appear. In B, the overlay uses `skip_taskbar(true)`. Owned windows inherit Alt+Tab grouping with the owner (to verify on Win11 26200). In A this is a non-issue.
- **Snap layouts:**
  - Native with a decorated window.
  - With `decorations(false)` plus custom chrome, the Win11 snap flyout appears only if the maximize button area answers `WM_NCHITTEST` with `HTMAXBUTTON`. tao/wry do not do this for HTML buttons (to verify), so keep native decorations in the library view, or implement the hit test.
- **Minimize/restore:**
  - In A, nothing to do.
  - In B, owned windows auto-hide. On restore, resync position before showing.
  - mpv keeps decoding while minimized. Consider pausing rendering (`vid=no`) or rely on mpv's behaviour (out of scope here).
- **DWM composition of HDR windowed swapchains:**
  - When anything overlaps the video (WebView2 visible, toast, volume flyout), DWM composes in FP16 scRGB. Correct, but costs GPU time and a frame of latency, and blocks independent flip.
  - Hide the WebView2 during steady playback.
  - A window straddling two monitors with different Advanced Color states is down-converted (only the "main" display, the one containing the window centre, is reported).
- **Transparent WebView2 performance:**
  - A full-window transparent WebView2 is composited on every video frame while visible.
  - Avoid CSS `backdrop-filter`/blur over the video (it forces readback-style effects; to measure).
  - Keep the controls layer small or static, and hide it when idle.
  - WebView2 resizing is slow under hardware acceleration (tauri #6322). Avoid animating window size.
- **White flash:** set `WEBVIEW2_DEFAULT_BACKGROUND_COLOR` (for example `00000000`) before creating the webview, and use `no_redirection_bitmap` for transparent windows.
- **Click-through:**
  - Only needed if the overlay must let clicks reach the video. Our design avoids it, because the webview owns all input while visible.
  - If ever needed: `set_ignore_cursor_events` toggles the whole window, not per-pixel. Per-pixel alpha hit testing applies to classic layered windows, not WebView2/DComp content, so hit regions would have to come from JS (for example on `pointermove`). This is complex, so avoid it.
- **mpv `wid` specifics:**
  - mpv creates and owns the child window and keeps it sized via an async `SetWindowPos`.
  - Do not destroy our video HWND while mpv still renders. Destroy mpv first (`quit`/terminate), then the HWND.
  - mpv's media-controls (SMTC) integration had exit and hang fixes in 0.42. Decide whether we want mpv's SMTC (`media-controls=yes`, as Stremio does) or our own.
- **Users' `mpv.conf` leaking in:** with libmpv, config loading is off unless `config=yes`. With mpv.exe, pass `--no-config` or `--config-dir`.
- **Version churn:** mpv 0.42.0 is one day old, and so are the shinchiro (20261106) and zhongfly (2026-11-05) builds. Pin a specific build in packaging (R5) and record its hash.

---

## 5. Prior art

| Project | Stack | Video / UI composition | Why | Status (observed) |
|---|---|---|---|---|
| **Stremio shell-ng** | Rust (edition 2018), native-windows-gui fork, `webview2` 0.1.4, `libmpv2` 6.0.0 / `-sys` 4.0.1, winapi | **One top-level window.** mpv via `wid` = main window HWND (`vo=gpu-next,gpu`, `gpu-context=d3d11`, `target-colorspace-hint=auto`, `tone-mapping=bt.2390`); WebView2 **windowed** controller on the same parent with `DefaultBackgroundColor` alpha 0, resized to the client rect, `MoveFocus` on `WM_SETFOCUS`. mpv event loop on a thread with `wait_event(0.1)` on a second client; observe requests via a channel; JSON RPC between web UI and player. Fullscreen and topmost via style toggling; `WM_DPICHANGED` handled | Replaces the Qt shell; avoids bundling Chromium (uses the system WebView2) and lets mpv use its "optimal video output, normally d3d11" | v5.0.27 in Cargo.toml; 93 stars, 371 commits |
| **Jellyfin Media Player / Jellyfin Desktop (Qt)** | Qt + QtWebEngine + libmpv **render API** (`PlayerQuickItem.cpp`); 2.0 moved to Qt 6 + MpvQt | UI drawn in the same Qt scene as the mpv-rendered video | Simple single-scene compositing | gpu-next impossible via render API (#362, open); Qt client archived as `jellyfin-desktop-qt` |
| **Jellyfin Desktop (CEF rewrite)** | CEF + mpv on "the same pipeline standalone mpv uses", `vo=gpu-next`, native HDR on Wayland/macOS/Windows | "Platform-native mechanisms that seamlessly overlay CEF on top" (no per-platform detail published) | Qt/QtWebEngine maintenance and leaks, and the render API lacking gpu-next/HDR | State of the Fin 2026-05-24: not ready for v3.0.0; Flathub declined the CEF build 2026-11-01 as "a different app" |
| **jellium-desktop** (unofficial, Rust) | CEF **off-screen rendering** + in-process mpv (own bindgen), wgpu | **Windows:** mpv creates **its own top-level window** (no `--wid`); app reads `window-id`, hooks it with `WH_CALLWNDPROCRET`, and composes CEF frames via **DirectComposition on mpv's HWND** (`CreateTargetForHwnd(hwnd, false)`, visuals with wgpu swapchains; "every visual is above" the video). Wayland: re-parents mpv's surface as a subsurface via wl-proxy | Keeps all standalone-mpv features (gpu-next, HDR) | Active; 156 forks |
| **Plex HTPC (Windows)** | Qt (QtWebEngine) + `libmpv-2.dll` | Supports `vo=gpu-next` with d3d11 or Vulkan for HDR passthrough, so it cannot be using the render API (inference) | HDR passthrough, refresh switching | Forum guidance 2022+; 1.28.1 sets the target colorspace hint automatically |
| **Emby Theater (Windows)** | Electron (2016 release note); WinForms shell with DirectShow/madVR (`jellyfin-theater-windows`) | Not mpv in the shipped shell (a "theater-mpv" test build existed in 2017) | — | Legacy |
| **tauri-plugin-libmpv / tauri-plugin-mpv** | Tauri 2 + libmpv via a C wrapper DLL / mpv.exe via JSON IPC | Transparent Tauri window with mpv rendered beneath; Windows "fully tested" | Easiest Tauri path | Small projects (23 stars) |
| **Tauri discussion #6343** | — | Users report libmpv in a child `WebviewWindow` resized to a DOM rect ("faked embedding"), and VLC drawing over a same-window webview | — | No confirmed transparent-over-wid recipe until the plugins above |
| **IINA** (macOS) | libmpv render API | — | Its developer was "waiting on" #10810 (gpu-next in the render API) per that issue | — |
| **mpv.net / Celluloid** | libmpv (.NET WinForms/WPF; GTK) | Not re-verified in this pass | — | — |

**Takeaways:**
- Every project that wants gpu-next/HDR has moved off the render API: Stremio (`wid`), Plex HTPC (inferred), the Jellyfin rewrite and jellium (mpv-owned window).
- The Rust/WebView2 one (Stremio) uses Design A.

---

## 6. Recommendation

### ADR-0002: player integration

- **Decision:** in-process **libmpv** via **`libmpv2` 7.0.0** (`libmpv2-sys` 4.0.1), embedded by **`wid`**. Base options:
  - `vo=gpu-next`, `gpu-context=d3d11`, `hwdec=auto` (or `d3d11va`).
  - `target-colorspace-hint=auto` and the rest of the R4 baseline.
  - `config=no` by default (a user `mpv.conf` is opt-in).
  - `input-default-bindings=no`, `osc=no`; the host owns input and UI.
- **Why:**
  - (1) gpu-next/libplacebo and Windows HDR signalling require mpv to own the swapchain. The render API, including 0.42's D3D11 backend, is `vo_gpu`-only (#10810 open).
  - (2) In-process gives the lowest control latency, typed property observation, and is the only way to keep Design C open.
  - (3) `libmpv2` is the only maintained high-level Rust binding (releases 2026-05 and 2026-10), and it is proven by Stremio on this exact platform.
  - (4) No second native wrapper DLL, unlike `tauri-plugin-libmpv`.
- **Linking:**
  - Generate `mpv.lib` with `lib /def:mpv.def /name:libmpv-2.dll /out:mpv.lib /machine:x64` from the pinned DLL's exports.
  - Emit `cargo:rustc-link-search` from `build.rs`.
  - Ship `libmpv-2.dll` next to the exe.
  - Consider `/DELAYLOAD` (spike) for graceful failure.
  - Use the dev package from the build chosen in R5. The licensing (GPL vs LGPL libmpv) decision is R5's, and it constrains Granary's own license if we link in-process.
- **Abstraction:** put mpv behind a `PlayerBackend` trait (load, command, set/get property, observe → `Channel<PlayerEvent>`). If crash isolation becomes necessary, an `mpv.exe` + JSON-IPC backend (inherited pipe handle, mpv ≥ 0.42) can be added without touching the UI.
- **Threading:** one event thread doing `wait_event` on a dedicated client handle. Commands come from Tauri async commands, and property changes are pushed to the UI over a Tauri **Channel**, throttled.

### ADR-0003: window/overlay architecture

- **Primary: Design A.** One Tauri main window. Our own `GranaryVideo` Win32 child HWND sits at `HWND_BOTTOM` under the `WRY_WEBVIEW` container, mpv uses `--wid=<GranaryVideo>`, and the WebView2 background is alpha 0 (env var `WEBVIEW2_DEFAULT_BACKGROUND_COLOR=00000000` to avoid the white flash).
  - Synchronous resize via `SetWindowSubclass` on the top-level.
  - Fullscreen via Tauri `set_fullscreen`.
  - When controls are idle, set the WebView2 controller `IsVisible=false`. The video HWND then catches mouse and keys to wake the UI.
  - Reasoning: it is the proven topology of the closest prior art (Rust + WebView2 + libmpv2, with HDR), it avoids every multi-window pitfall, and it needs the least Win32 code.
- **Fallback: Design B/B′.** An owned, transparent, `skip_taskbar`, `no_redirection_bitmap` overlay `WebviewWindow` above the video window, synced on `WM_WINDOWPOSCHANGED`, hidden with `SW_HIDE` when idle. Switch to B′ (mpv-owned top-level window, as jellium does) if spike S1 shows that a child-window swapchain never reaches Independent Flip.
- **Research spike: Design C** (`d3d11-output-mode=composition` + DComp). Revisit only if A and B both fail on flip and VRR, or if we want inline video in library pages.
- **Spikes before accepting ADR-0003:**
  - **S1:** PresentMon on an OLED HDR + VRR monitor. Check the 23.976/24 fps HDR10 present mode in A with the WebView2 hidden, windowed and fullscreen. If needed, compare with B′.
  - **S2:** GPU cost of a visible transparent WebView2 over 4K HDR at 120–240 Hz.
  - **S3:** opaque top-level + webview alpha 0 versus Tauri `transparent: true` (desktop show-through, flash).
  - **S4:** optional, Design C feasibility.
  - **S5:** `lib /def` import library and `/DELAYLOAD` with `libmpv2-sys`.

### Key risks

1. Independent flip, MPO and VRR may not engage for child-HWND swapchains (S1). Mitigation: B′.
2. In-process crashes. Mitigation: the `PlayerBackend` seam keeps an IPC backend possible later.
3. Licensing of the libmpv build (R5).
4. The bleeding-edge mpv 0.42 and its builds. Pin them and test.
5. `libmpv2-sys` headers are API 2.5 while mpv is 2.6. This is harmless for the client API, but blocks `render_d3d11.h`, which we do not need.

### Not verified in this pass (verify before relying on them)

- The tao/Tauri Win32 implementation of `set_ignore_cursor_events` and `skip_taskbar`.
- Whether wry honours webview alpha 0 without `transparent: true`.
- MSVC link.exe consuming `libmpv.dll.a`.
- `/DELAYLOAD` behaviour with Rust.
- Cross-process `wid` input-queue coupling.
- Win11 snap flyout with custom chrome.
- The zhongfly `mpv-dev-lgpl` asset name.
- mpv.net/Celluloid embedding details.
- Exact `@tauri-apps/plugin-*` npm versions.

---

## Sources

Observed 2026-11-06 unless stated.

**Tauri / wry / WebView2**
- https://crates.io/api/v1/crates/tauri: tauri 2.12.5 (2026-10-21), 3.0.0-alpha.5 (2026-10-14)
- https://registry.npmjs.org/@tauri-apps/cli/latest: 2.12.4
- https://registry.npmjs.org/@tauri-apps/api/latest: 2.12.4
- https://crates.io/api/v1/crates/tauri-plugin-single-instance: 2.5.5 (2026-10-28)
- https://crates.io/api/v1/crates/tauri-plugin-window-state: 2.5.0 (2026-09-26)
- https://crates.io/api/v1/crates/tauri-plugin-log: 2.10.1 (2026-10-10)
- https://crates.io/api/v1/crates/tauri-plugin-opener: 2.7.2 (2026-10-14)
- https://crates.io/api/v1/crates/tauri-plugin-os: 2.4.0 (2026-09-26)
- https://crates.io/api/v1/crates/tauri-plugin-process: 2.4.0 (2026-09-26)
- https://crates.io/api/v1/crates/tauri-plugin-store: 2.5.0 (2026-09-26)
- https://crates.io/api/v1/crates/windows: 0.62.2 (2025-10-06)
- https://crates.io/api/v1/crates/raw-window-handle: 0.6.2 (2024-05-17)
- https://crates.io/api/v1/crates/webview2-com: 0.39.1 (2026-03-11)
- https://crates.io/api/v1/crates/libloading: 0.9.0 (2025-11-05)
- https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindowBuilder.html: tauri 2.12.5 builder methods (transparent, parent, no_redirection_bitmap, skip_taskbar, background_color, use_https_scheme, additional_browser_args, on_web_resource_request…)
- https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindow.html: tauri 2.12.5 (set_ignore_cursor_events, set_focusable, with_webview, HasWindowHandle…)
- https://docs.rs/tauri/latest/src/tauri/webview/webview_window.rs.html: tauri 2.12.5 source (`hwnd` L1964, `owner` L703, `owner_raw` L718, `parent_raw` L730, `transparent` L1146)
- https://docs.rs/tauri/latest/src/tauri/window/mod.rs.html: `Window::hwnd`, `owner_raw`, `HasWindowHandle`
- https://docs.rs/tauri/latest/tauri/struct.Builder.html: register_uri_scheme_protocol / register_asynchronous_uri_scheme_protocol
- https://docs.rs/crate/wry/latest/source/src/webview2/mod.rs: wry 0.57.1 (CreateCoreWebView2Controller[WithOptions], `WRY_WEBVIEW` container, background alpha handling)
- https://v2.tauri.app/security/capabilities/
- https://v2.tauri.app/security/csp/
- https://v2.tauri.app/develop/calling-frontend/: events vs channels
- https://v2.tauri.app/develop/calling-rust/: commands, ipc::Response, raw request body
- https://github.com/tauri-apps/tauri/issues/6322: WebView2 resize slowness
- https://v2.tauri.app/release/tauri-runtime-wry/v2.0.0-beta.19/: native undecorated resize
- https://www.producthunt.com/p/tauri-2/i-built-sticky-notes-that-hide-in-your-windows-screen-edge-with-tauri-2-6-lessons-learned: anecdotal transparent-window flicker
- https://github.com/orgs/tauri-apps/discussions/6343: Tauri + mpv embedding discussion (2023–2026)
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2: DefaultBackgroundColor (alpha 0/255 only, env var, white flicker; SDK 1.0.4314.35, doc date 2026-10-19)
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2compositioncontroller: visual hosting, manual input forwarding

**mpv**
- https://github.com/mpv-player/mpv/releases: v0.42.0 (2026-11-05), v0.41.0, v0.40.0
- https://github.com/mpv-player/mpv/releases/tag/v0.42.0: release notes (D3D11 render API, scRGB, ipc-win handles, w32_common resize)
- https://raw.githubusercontent.com/mpv-player/mpv/master/DOCS/client-api-changes.rst: API 2.6 / 2.5 / 2.4
- https://raw.githubusercontent.com/mpv-player/mpv/master/include/mpv/render_d3d11.h: D3D11 render API params
- https://raw.githubusercontent.com/mpv-player/mpv/master/video/out/d3d11/libmpv_d3d11.c: vo_gpu-based (`libmpv_gpu_context_d3d11`)
- https://github.com/mpv-player/mpv/tree/master/video/out/d3d11: file listing
- https://raw.githubusercontent.com/mpv-player/mpv/master/video/out/d3d11/context.c: composition mode behaviour (no target colorspace)
- https://raw.githubusercontent.com/mpv-player/mpv/master/video/out/w32_common.c: `--wid` child creation, `EnableWindow(…,0)`, parent hook, `vo_w32_composition_size`
- https://raw.githubusercontent.com/mpv-player/mpv/master/DOCS/man/options.rst: `--wid`, `--d3d11-*`, `--target-colorspace-hint*`, `--d3d11-output-mode`, `--d3d11-composition-size`, `--input-cursor-passthrough`
- https://raw.githubusercontent.com/mpv-player/mpv/master/DOCS/man/ipc.rst: JSON IPC, named pipes, observe_property, async
- https://raw.githubusercontent.com/mpv-player/mpv/master/Copyright: GPLv2+ default, `-Dgpl=false` LGPL mode
- https://github.com/mpv-player/mpv/issues/10810: gpu-next in render API, open
- https://github.com/mpv-player/mpv/pull/18607: OPENGL_NEXT RFC, open, 2026-10-13

**Rust mpv crates**
- https://crates.io/api/v1/crates/libmpv2: 7.0.0 (2026-10-30), 6.0.1 (2026-10-29), 6.0.0 (2026-05-12)
- https://github.com/kohsine/libmpv-rs: README (libmpv ≥ 2.0 / mpv 0.35, MPV_SOURCE on Windows)
- https://raw.githubusercontent.com/kohsine/libmpv-rs/master/CHANGELOG.md: 5.x–7.0.0 changes
- https://docs.rs/libmpv2/latest/libmpv2/: 7.0.0 modules, LGPL-2.1, libmpv2-sys ^4.0.1
- https://docs.rs/libmpv2/latest/libmpv2/struct.Mpv.html: method list, Send + Sync
- https://docs.rs/crate/libmpv2-sys/latest: 4.0.1 (2025-12-29)
- https://docs.rs/crate/libmpv2-sys/latest/source/build.rs: `rustc-link-lib=mpv`, no search path
- https://docs.rs/crate/libmpv2-sys/latest/source/include/client.h: MPV_CLIENT_API_VERSION 2.5
- https://crates.io/api/v1/crates/libmpv: 2.0.1 (2020-09-29)
- https://crates.io/api/v1/crates/libmpv-sys: 3.1.0 (2020-07-22)
- https://crates.io/api/v1/crates/mpv-client: 1.1.0 (2025-06-28)
- https://crates.io/api/v1/crates/mpvipc: 1.3.1 (2026-02-15)
- https://docs.rs/crate/tauri-plugin-libmpv/latest/source/README.md: 0.3.2
- https://github.com/nini22P/tauri-plugin-libmpv: 23 stars
- https://docs.rs/crate/tauri-plugin-mpv/latest: 0.5.2 (2025-11-18)

**Windows builds and linking**
- https://github.com/shinchiro/mpv-winbuild-cmake/releases: tag 20261106, mpv-dev-x86_64-20261106-git-9028187538.7z (30.3 MB)
- https://raw.githubusercontent.com/shinchiro/mpv-winbuild-cmake/master/packages/mpv.cmake: dev package contents (libmpv-2.dll, libmpv.dll.a, headers)
- https://raw.githubusercontent.com/shinchiro/mpv-winbuild-cmake/master/packages/mpv-packaging.cmake
- https://github.com/zhongfly/mpv-winbuild/releases: tag 2026-11-05-3138fc8eea
- https://docs.openspaceproject.com/latest/contribute/development/dependencies/libmpv.html: gendef + `lib /def … /MACHINE:X64`
- https://c-toolbox.github.io/C-Play/guides/build/mpv_ffmpeg: `lib /def:mpv.def /name:mpv-2.dll /out:mpv.lib /MACHINE:X64`
- https://forum.jellyfin.org/printthread.php?tid=9566: LNK4272 machine mismatch

**Win32 / DXGI / DComp / HDR**
- https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features: owned windows, layered windows (Win8 child), WS_EX_TRANSPARENT hit testing, SetForegroundWindow rules
- https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nf-dcomp-idcompositiondevice-createtargetforhwnd: four layers, same-process requirement, topmost
- https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model: DirectFlip / Independent Flip / MPO
- https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range: CCCS FP16 composition, flip model requirement, SDR white boost for separate surfaces, HDR10 no-alpha condition

**Prior art**
- https://github.com/Stremio/stremio-shell-ng: repo overview
- https://raw.githubusercontent.com/Stremio/stremio-shell-ng/main/Cargo.toml: v5.0.27; libmpv2 6.0.0, libmpv2-sys 4.0.1, webview2 0.1.4, native-windows-gui fork
- https://raw.githubusercontent.com/Stremio/stremio-shell-ng/main/src/stremio_app/stremio_player/player.rs: wid, vo=gpu-next, d3d11, colorspace hint, event thread
- https://raw.githubusercontent.com/Stremio/stremio-shell-ng/main/src/stremio_app/stremio_wevbiew/wevbiew.rs: windowed controller, alpha-0 background, MoveFocus
- https://raw.githubusercontent.com/Stremio/stremio-shell-ng/main/src/stremio_app/app.rs: single main window, partials parented to it, fullscreen, DPI
- https://github.com/jellyfin/jellyfin-desktop: Qt WebEngine + libmpv (master)
- https://github.com/jellyfin/jellyfin-media-player/issues/362: render API, no gpu-next
- https://jellyfin.org/posts/state-of-the-fin-2026-05-24/: CEF rewrite, gpu-next, native HDR
- https://github.com/flathub/org.jellyfin.JellyfinDesktop/issues/10: CEF build closed 2026-11-01
- https://github.com/jellyfin-archive/jellyfin-desktop-qt: archived Qt client
- https://github.com/andrewrabert/jellium-desktop: unofficial Rust CEF + mpv client
- https://github.com/andrewrabert/jellium-desktop/wiki/Implementation-Details: render API lacks gpu-next/HDR, wraps standalone mpv window
- https://raw.githubusercontent.com/andrewrabert/jellium-desktop/main/src/windows/src/platform.rs: mpv top-level (no --wid), WH_CALLWNDPROCRET hook
- https://raw.githubusercontent.com/andrewrabert/jellium-desktop/main/src/windows/src/render/mod.rs: DComp target is the mpv HWND
- https://raw.githubusercontent.com/andrewrabert/jellium-desktop/main/src/windows/src/render/device.rs: `CreateTargetForHwnd(hwnd, false)`
- https://raw.githubusercontent.com/andrewrabert/jellium-desktop/main/src/windows/src/render/layer.rs: visual layers
- https://raw.githubusercontent.com/andrewrabert/jellium-desktop/main/src/mpv/Cargo.toml: bindgen-based mpv bindings
- https://forums.plex.tv/t/hdr-metadata-passthrough-plex-htpc-for-windows/794358.md: Plex HTPC uses mpv, gpu-next + d3d11/vulkan, colorspace hint since 1.28.1
- https://github.com/jellyfin/jellyfin-theater-windows: WinForms shell, DirectShow (Emby Theater lineage)
- https://emby.media/community/blogs/entry/290-introducing-the-new-emby-theater-for-windows-and-the-web: Electron-based (2016)
- https://github.com/mpvnet-player/mpv.net: libmpv-based .NET player (embedding details not verified)
