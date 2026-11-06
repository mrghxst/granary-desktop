//! Phase 2 risk spike: libmpv (gpu-next/d3d11) in a GranaryVideo child HWND under a transparent WebView2 (ADR-0003 A).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[allow(dead_code)] // shared with the hdr-roundtrip bin
mod display;
mod mpv;
mod video;

use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

static LOG: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static PLAYER: Mutex<Option<Arc<mpv::Mpv>>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();
/// Set when the overlay UI reports it has loaded (selftest waits for it instead of a fixed delay).
static UI_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn log(msg: impl AsRef<str>) {
    let t = START
        .get()
        .map(|s| s.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let line = format!("[{t:8.3}] {}\n", msg.as_ref());
    print!("{line}");
    if let Some(f) = LOG.get()
        && let Ok(mut f) = f.lock()
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn player() -> Option<Arc<mpv::Mpv>> {
    PLAYER.lock().ok().and_then(|p| p.clone())
}

fn mpv_options(wid: isize) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("wid", wid.to_string()),
        ("vo", "gpu-next".into()),
        ("gpu-api", "d3d11".into()),
        ("hwdec", "d3d11va".into()),
        ("d3d11-flip", "yes".into()),
        ("d3d11-exclusive-fs", "no".into()),
        ("target-colorspace-hint", "auto".into()),
        ("target-colorspace-hint-mode", "target".into()),
        ("hdr-reference-white", "auto".into()),
        ("tone-mapping", "auto".into()),
        ("hdr-compute-peak", "auto".into()),
        ("video-output-levels", "full".into()),
        ("dither-depth", "auto".into()),
        ("deband", "yes".into()),
        ("video-sync", "display-resample".into()),
        ("keep-open", "yes".into()),
        ("loop-file", "inf".into()),
        ("config", "no".into()),
        ("input-default-bindings", "no".into()),
        ("input-vo-keyboard", "no".into()),
        ("input-cursor", "no".into()),
        ("cursor-autohide", "no".into()),
        ("osc", "no".into()),
        ("terminal", "no".into()),
        ("media-controls", "no".into()),
        ("stop-screensaver", "no".into()),
    ];
    // SPIKE_MPV_OPTS="k=v,k=v" overrides defaults for A/B pacing runs.
    if let Ok(extra) = std::env::var("SPIKE_MPV_OPTS") {
        for kv in extra.split(',') {
            if let Some((k, val)) = kv.split_once('=') {
                let k: &'static str = Box::leak(k.trim().to_string().into_boxed_str());
                v.retain(|(n, _)| *n != k);
                v.push((k, val.trim().to_string()));
            }
        }
    }
    v
}

fn ensure_player() -> Result<Arc<mpv::Mpv>, String> {
    if let Some(p) = player() {
        return Ok(p);
    }
    let v = video::create_video_child()?;
    log(format!(
        "video child created hwnd={:#x} below_webview={}",
        v.0 as isize,
        video::video_below_webview()
    ));
    let m = mpv::Mpv::new(&mpv_options(v.0 as isize))?;
    for p in [
        "pause",
        "video-params/gamma",
        "current-vo",
        "hwdec-current",
        "fullscreen",
        "display-names",
    ] {
        m.observe(p);
    }
    m.spawn_events(|e| match e {
        mpv::Event::Log {
            prefix,
            level,
            text,
        } => {
            let t = text.trim_end();
            let interesting = [
                "d3d11",
                "swapchain",
                "colorspace",
                "color space",
                "HDR",
                "hdr",
                "target",
                "Output",
                "peak",
                "flip",
            ];
            if level != "v" || interesting.iter().any(|k| t.contains(k)) {
                log(format!("mpv[{prefix}:{level}] {t}"));
            }
        }
        mpv::Event::FileLoaded => log(format!(
            "event FILE_LOADED; disabled mpv child windows: {}",
            video::disable_mpv_children()
        )),
        mpv::Event::EndFile => log("event END_FILE"),
        mpv::Event::PlaybackRestart => log("event PLAYBACK_RESTART"),
        mpv::Event::VideoReconfig => log(format!(
            "event VIDEO_RECONFIG; disabled mpv child windows: {}",
            video::disable_mpv_children()
        )),
        mpv::Event::Property { name, value } => {
            log(format!("prop {name} = {value}"));
            if let Some(app) = APP.get() {
                let _ = app.emit("mpv-prop", json!({"name": name, "value": value}));
            }
        }
        mpv::Event::Shutdown => log("event SHUTDOWN"),
    });
    if let Ok(mut g) = PLAYER.lock() {
        *g = Some(Arc::clone(&m));
    }
    Ok(m)
}

#[tauri::command]
fn spike_play(path: String) -> Result<(), String> {
    let p = ensure_player()?;
    log(format!("cmd play {path}"));
    let r = p.command(&["loadfile", &path]);
    if r < 0 {
        Err(format!("loadfile failed {r}"))
    } else {
        Ok(())
    }
}

#[tauri::command]
fn spike_cmd(name: String, arg: Option<f64>) -> Result<(), String> {
    let p = player().ok_or("no player")?;
    log(format!("cmd {name} {arg:?} (from UI)"));
    let r = match name.as_str() {
        "pause" => p.command(&["cycle", "pause"]),
        "seek" => p.command(&["seek", &arg.unwrap_or(10.0).to_string(), "relative"]),
        "stop" => p.command(&["stop"]),
        _ => return Err("unknown".into()),
    };
    if r < 0 {
        Err(format!("{name} failed {r}"))
    } else {
        Ok(())
    }
}

#[tauri::command]
fn spike_fullscreen(app: AppHandle) -> Result<bool, String> {
    let w = app.get_webview_window("main").ok_or("no window")?;
    let fs = !w.is_fullscreen().map_err(|e| e.to_string())?;
    w.set_fullscreen(fs).map_err(|e| e.to_string())?;
    log(format!("cmd fullscreen -> {fs} (from UI)"));
    Ok(fs)
}

#[tauri::command]
fn spike_overlay(visible: bool) {
    log(format!("cmd overlay visible={visible}"));
    video::set_webview_visible(visible);
    if !visible {
        video::focus_video();
    }
}

#[tauri::command]
fn spike_log(msg: String) {
    if msg.starts_with("overlay loaded") {
        UI_READY.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    log(format!("ui: {msg}"));
}

#[tauri::command]
fn spike_selftest_enabled() -> bool {
    std::env::var("SPIKE_SELFTEST").is_ok()
}

fn props(names: &[&str]) -> Value {
    let p = player();
    let mut m = serde_json::Map::new();
    for n in names {
        let v = p
            .as_ref()
            .and_then(|p| p.get(n))
            .unwrap_or_else(|| "<unavailable>".into());
        m.insert((*n).into(), Value::String(v));
    }
    Value::Object(m)
}

fn geometry() -> Value {
    let (main, vid, web) = (
        video::main_hwnd(),
        video::video_hwnd(),
        video::webview_hwnd(),
    );
    let (mon, name) = display::monitor_of(main);
    json!({
        "client": video::client_size(main),
        "video_rect": video::rect_of(vid),
        "webview_rect": video::rect_of(web),
        "main_rect": video::rect_of(main),
        "webview_visible": video::is_visible(web),
        "video_visible": video::is_visible(vid),
        "video_below_webview": video::video_below_webview(),
        "monitor": name,
        "hmonitor": mon.0 as isize,
        "dpi": unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(main) },
    })
}

#[tauri::command]
fn spike_geometry() -> Value {
    geometry()
}

fn wait(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn click_ui(app: &AppHandle, id: &str) {
    log(format!("selftest: clicking overlay button #{id}"));
    let _ = app.emit("selftest-click", id);
}

fn check(results: &mut Vec<Value>, name: &str, ok: bool, detail: Value) {
    log(format!(
        "CHECK {} {name} {detail}",
        if ok { "PASS" } else { "FAIL" }
    ));
    results.push(json!({"check": name, "ok": ok, "detail": detail}));
}

fn video_matches_client() -> bool {
    let (cw, ch) = video::client_size(video::main_hwnd());
    let (_, _, vw, vh) = video::rect_of(video::video_hwnd());
    cw == vw && ch == vh
}

/// Scripted self-test, run on a background thread when SPIKE_SELFTEST is set.
fn selftest(app: AppHandle) {
    let mut r: Vec<Value> = Vec::new();
    let media = repo_root().join("test-media");
    let hdr = media
        .join("hdr10_hevc_main10_2160p.mkv")
        .to_string_lossy()
        .replace('\\', "/");
    let sdr = media
        .join("sdr_h264_1080p.mkv")
        .to_string_lossy()
        .replace('\\', "/");
    let label = std::env::var("SPIKE_LABEL").unwrap_or_else(|_| "run".into());
    let Some(win) = app.get_webview_window("main") else {
        return;
    };

    log(format!(
        "selftest[{label}] displays: {}",
        serde_json::to_string(&display::displays()).unwrap_or_default()
    ));
    // Wait for the overlay UI (a cold Vite start can take several seconds), then let it settle.
    for _ in 0..120 {
        if UI_READY.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        wait(250);
    }
    wait(1500);

    // 1. HDR clip through the overlay's Play button.
    let _ = app.emit("selftest-set-file", &hdr);
    click_ui(&app, "play");
    wait(4500);
    let p = props(&[
        "mpv-version",
        "ffmpeg-version",
        "libplacebo-version",
        "current-vo",
        "current-gpu-context",
        "hwdec-current",
        "video-params",
        "video-target-params",
        "display-fps",
        "display-names",
        "estimated-display-fps",
    ]);
    log(format!("selftest[{label}] HDR props: {p}"));
    check(
        &mut r,
        "current-vo=gpu-next",
        p["current-vo"] == "gpu-next",
        p["current-vo"].clone(),
    );
    check(
        &mut r,
        "hwdec-current!=no",
        p["hwdec-current"] != "no" && p["hwdec-current"] != "<unavailable>",
        p["hwdec-current"].clone(),
    );
    let vp: Value =
        serde_json::from_str(p["video-params"].as_str().unwrap_or("{}")).unwrap_or_default();
    check(
        &mut r,
        "hdr gamma=pq primaries=bt.2020",
        vp["gamma"] == "pq" && vp["primaries"] == "bt.2020",
        json!({"gamma": vp["gamma"], "primaries": vp["primaries"], "max-cll": vp["max-cll"]}),
    );
    let g = geometry();
    check(
        &mut r,
        "video child below webview",
        g["video_below_webview"] == true,
        g.clone(),
    );
    check(
        &mut r,
        "video child fills client",
        video_matches_client(),
        g.clone(),
    );

    // 2. Pause / seek via overlay buttons (IPC).
    click_ui(&app, "pause");
    wait(800);
    let paused = player().and_then(|p| p.get("pause")).unwrap_or_default();
    check(
        &mut r,
        "overlay pause -> pause=yes",
        paused == "yes",
        json!(paused),
    );
    click_ui(&app, "pause");
    wait(800);
    let t0: f64 = player()
        .and_then(|p| p.get("time-pos"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    click_ui(&app, "fwd");
    wait(900);
    let t1: f64 = player()
        .and_then(|p| p.get("time-pos"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let dt = (t1 - t0 + 10.0).rem_euclid(10.0); // clip loops every 10 s
    check(
        &mut r,
        "overlay seek +3s",
        (2.0..=5.5).contains(&dt),
        json!({"t0": t0, "t1": t1, "delta_mod10": dt}),
    );

    // 3. Fullscreen via overlay, geometry follows.
    click_ui(&app, "fs");
    wait(1500);
    let g = geometry();
    let fs = win.is_fullscreen().unwrap_or(false);
    check(
        &mut r,
        "overlay fullscreen on + video fills",
        fs && video_matches_client(),
        g.clone(),
    );
    let p = props(&[
        "video-target-params",
        "display-fps",
        "frame-drop-count",
        "vo-delayed-frame-count",
    ]);
    log(format!("selftest[{label}] fullscreen HDR target: {p}"));
    click_ui(&app, "fs");
    wait(1500);
    check(
        &mut r,
        "fullscreen off",
        !win.is_fullscreen().unwrap_or(true) && video_matches_client(),
        geometry(),
    );

    // 4. Move / resize / minimize / restore.
    let _ = win.set_size(tauri::PhysicalSize::new(1100u32, 640u32));
    let _ = win.set_position(tauri::PhysicalPosition::new(120i32, 90i32));
    wait(800);
    check(
        &mut r,
        "resize 1100x640 -> video follows",
        video_matches_client(),
        geometry(),
    );
    let _ = win.minimize();
    wait(800);
    let _ = win.unminimize();
    wait(1000);
    check(
        &mut r,
        "minimize/restore -> video follows",
        video_matches_client() && video::is_visible(video::video_hwnd()),
        geometry(),
    );

    // 5. Overlay truly hidden, wake on mouse move over the video window.
    click_ui(&app, "hide");
    wait(700);
    let hidden = !video::is_visible(video::webview_hwnd());
    check(
        &mut r,
        "overlay hidden (WRY_WEBVIEW SW_HIDE)",
        hidden,
        geometry(),
    );
    // Review H1: the overlay must *stay* hidden without input (no self-triggered wake-ups) for >= 10 s.
    let mut reshown = 0;
    let mut was_hidden = hidden;
    for _ in 0..40 {
        wait(250);
        let h = !video::is_visible(video::webview_hwnd());
        if was_hidden && !h {
            reshown += 1;
        }
        was_hidden = h;
    }
    check(
        &mut r,
        "overlay stays hidden 10s without input",
        hidden && reshown == 0 && was_hidden,
        json!({"reshown": reshown, "hidden_at_end": was_hidden}),
    );
    for l in video::dump_tree() {
        log(format!("tree: {l}"));
    }
    // Synthetic input must never reach another application: only send it when our window is on top/foreground.
    let _ = win.set_always_on_top(true);
    let _ = win.set_focus();
    wait(500);
    let (x, y, w, h) = video::rect_of(video::video_hwnd());
    let (cx, cy) = (x + w / 2, y + h / 2);
    log(format!(
        "window under video centre: {} (ours={})",
        video::window_at(cx, cy),
        video::point_is_ours(cx, cy)
    ));
    if video::point_is_ours(cx, cy) {
        send_mouse_move_over_video();
        wait(700);
        check(
            &mut r,
            "mouse move wakes overlay",
            video::is_visible(video::webview_hwnd()),
            geometry(),
        );
    } else {
        check(
            &mut r,
            "mouse move wakes overlay",
            false,
            json!("skipped: another window covers the video centre"),
        );
    }

    // 6. Keyboard focus: real key press reaches the WebView (space toggles pause in the UI).
    let before = player().and_then(|p| p.get("pause")).unwrap_or_default();
    let _ = win.set_focus();
    wait(400);
    if video::is_foreground() {
        send_key_space();
        wait(900);
        let after = player().and_then(|p| p.get("pause")).unwrap_or_default();
        check(
            &mut r,
            "keyboard space via WebView toggles pause",
            before != after,
            json!({"before": before, "after": after}),
        );
    } else {
        check(
            &mut r,
            "keyboard space via WebView toggles pause",
            false,
            json!("skipped: spike window is not foreground"),
        );
    }
    let after = player().and_then(|p| p.get("pause")).unwrap_or_default();
    if after == "yes" {
        let _ = player().map(|p| p.set("pause", "no"));
    }

    // 7. Frame pacing over 30 s (HDR clip looping, overlay hidden).
    click_ui(&app, "hide");
    wait(600);
    // Counters reset when the 10 s clip loops, so sample every 250 ms and sum increments (a drop in value = reset).
    let keys = [
        "frame-drop-count",
        "vo-delayed-frame-count",
        "mistimed-frame-count",
    ];
    let read = || {
        keys.map(|k| {
            player()
                .and_then(|p| p.get(k))
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
        })
    };
    // Increments within 1 s after a loop restart are counted separately (file re-init, not steady-state pacing).
    let pos = || {
        player()
            .and_then(|p| p.get("time-pos"))
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let mut prev = read();
    let mut sum = [0i64; 3];
    let mut at_loop = [0i64; 3];
    let mut loops = 0;
    let mut last_pos = pos();
    let mut overlay_visible_samples = 0;
    for _ in 0..120 {
        wait(250);
        if video::is_visible(video::webview_hwnd()) {
            overlay_visible_samples += 1;
        }
        let cur = read();
        let p = pos();
        let restarted = p < last_pos || cur.iter().zip(prev.iter()).any(|(c, q)| c < q);
        if restarted {
            loops += 1;
        }
        let near_loop = restarted || p < 1.0;
        for i in 0..3 {
            let d = if cur[i] >= prev[i] {
                cur[i] - prev[i]
            } else {
                cur[i]
            };
            if near_loop {
                at_loop[i] += d
            } else {
                sum[i] += d
            }
        }
        prev = cur;
        last_pos = p;
    }
    let ctx = props(&[
        "display-fps",
        "estimated-vf-fps",
        "video-sync",
        "current-vo",
    ]);
    let (drops, delayed, mistimed) = (sum[0], sum[1], sum[2]);
    check(
        &mut r,
        "30s steady-state drops/delayed near 0",
        drops <= 2 && delayed <= 2 && overlay_visible_samples == 0,
        json!({"drops": drops, "vo_delayed": delayed, "mistimed": mistimed, "loops": loops, "overlay_visible_samples": overlay_visible_samples,
               "at_loop_restart": {"drops": at_loop[0], "vo_delayed": at_loop[1], "mistimed": at_loop[2]}, "ctx": ctx}),
    );
    let _ = win.set_always_on_top(false);
    video::set_webview_visible(true);

    // 8. SDR clip: target params (SDR white handling).
    let _ = app.emit("selftest-set-file", &sdr);
    click_ui(&app, "play");
    wait(3500);
    let p = props(&[
        "video-params",
        "video-target-params",
        "hdr-reference-white",
        "target-peak",
    ]);
    log(format!("selftest[{label}] SDR props: {p}"));
    let tp: Value =
        serde_json::from_str(p["video-target-params"].as_str().unwrap_or("{}")).unwrap_or_default();
    check(
        &mut r,
        "sdr clip target params available",
        tp.is_object() && tp.get("gamma").is_some(),
        tp.clone(),
    );

    let passed = r.iter().filter(|c| c["ok"] == true).count();
    log(format!(
        "SELFTEST[{label}] SUMMARY {passed}/{} passed",
        r.len()
    ));
    let out = repo_root().join(format!("spikes/mpv-overlay/selftest-{label}.json"));
    let _ = std::fs::write(
        &out,
        serde_json::to_string_pretty(&json!({"label": label, "results": r})).unwrap_or_default(),
    );
    if let Some(p) = player() {
        p.quit();
    }
    wait(800);
    app.exit(if passed == r.len() { 0 } else { 1 });
}

fn send_mouse_move_over_video() {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    let (x, y, w, h) = video::rect_of(video::video_hwnd());
    // SAFETY: synthesises a relative mouse move after positioning the cursor inside our own window.
    unsafe {
        let _ = SetCursorPos(x + w / 2, y + h / 2);
        let i = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 5,
                    dy: 5,
                    dwFlags: MOUSEEVENTF_MOVE,
                    ..Default::default()
                },
            },
        };
        SendInput(&[i], std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_key_space() {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let mk = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_SPACE,
                wScan: 0x39,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    // SAFETY: synthesises a key press/release; our window has focus.
    unsafe {
        SendInput(
            &[mk(KEYBD_EVENT_FLAGS(0)), mk(KEYEVENTF_KEYUP)],
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

fn main() {
    let _ = START.set(Instant::now());
    if std::env::args().any(|a| a == "--probe-mpv") {
        std::process::exit(probe_mpv());
    }
    let log_path = repo_root().join("spikes/mpv-overlay/spike.log");
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = LOG.set(Mutex::new(f));
    }
    log("==== spike start ====");
    // SAFETY: set before any other thread exists; tells WebView2 to use a transparent default background.
    unsafe { std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "00000000") };

    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            spike_play,
            spike_cmd,
            spike_fullscreen,
            spike_overlay,
            spike_log,
            spike_selftest_enabled,
            spike_geometry
        ])
        .setup(|app| {
            let _ = APP.set(app.handle().clone());
            let win = app.get_webview_window("main").ok_or("no main window")?;
            let hwnd = win.hwnd()?;
            let main = video::hwnd(hwnd.0 as isize);
            // The WRY container exists once the webview is created; retry briefly.
            let mut attached = Err(String::from("not tried"));
            for _ in 0..20 {
                attached = video::attach(main);
                if attached.is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            log(format!("attach main={:#x}: {attached:?}", main.0 as isize));
            // The video child must be created on the UI thread (it owns the parent's message loop).
            let vc = video::create_video_child();
            log(format!("video child: {vc:?}"));
            let h = app.handle().clone();
            video::on_activity(move |k| {
                let _ = h.emit("video-activity", k);
            });
            video::on_resize(|w, h| log(format!("WM_SIZE {w}x{h}")));
            let last = Mutex::new(String::new());
            video::on_moved(move || {
                let (_, name) = display::monitor_of(video::main_hwnd());
                if let Ok(mut l) = last.lock()
                    && *l != name
                {
                    log(format!(
                        "monitor changed: {} -> {name}; displays={}",
                        *l,
                        serde_json::to_string(&display::displays()).unwrap_or_default()
                    ));
                    *l = name;
                }
            });
            if std::env::var("SPIKE_SELFTEST").is_ok() {
                let h = app.handle().clone();
                std::thread::spawn(move || selftest(h));
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        log(format!("tauri error: {e}"));
    }
}

/// With /DELAYLOAD the exe starts without libmpv-2.dll; probe it explicitly before the first mpv call.
fn probe_mpv() -> i32 {
    use windows::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LoadLibraryExW};
    // SAFETY: loads a DLL from the app dir + System32 (no CWD/PATH search); the handle is intentionally kept for the process.
    let h = unsafe {
        LoadLibraryExW(
            windows::core::w!("libmpv-2.dll"),
            None,
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    match h {
        Err(e) => {
            println!("probe: libmpv-2.dll not loadable: {e}");
            3
        }
        Ok(_) => {
            // SAFETY: the DLL is loaded; this is the first delay-loaded call.
            let v = unsafe { libmpv2_sys::mpv_client_api_version() };
            println!("probe: libmpv client api {:#x}", v);
            0
        }
    }
}
