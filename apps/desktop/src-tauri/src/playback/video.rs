//! The video surface (ADR-0003 Design A, validated in `spikes/mpv-overlay`): a `GranaryVideo` child window at the
//! bottom of the main window's z-order, under the transparent WRY/WebView2 container. libmpv renders into it (`wid`).
//!
//! When the overlay is idle the host hides the WebView container (`SW_HIDE`), so nothing static is composited over
//! the video, and the cursor is hidden over the video window. Mouse activity on the video window shows the overlay
//! again; keys typed while it is hidden are forwarded to the UI without showing it.
use serde::Serialize;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{
    BLACK_BRUSH, GetMonitorInfoW, GetStockObject, HBRUSH, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW,
    MonitorFromWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, GetKeyboardState, IsWindowEnabled, SetFocus, ToUnicode, VK_CONTROL, VK_SHIFT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, w};

static MAIN: AtomicIsize = AtomicIsize::new(0);
static VIDEO: AtomicIsize = AtomicIsize::new(0);
static WEBVIEW: AtomicIsize = AtomicIsize::new(0);
/// Playback is active: the cursor may be hidden and activity re-shows the overlay.
static PLAYING: AtomicBool = AtomicBool::new(false);
static ACTIVITY: OnceLock<Box<dyn Fn(Activity) + Send + Sync>> = OnceLock::new();
static MOVED: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// Last WM_MOUSEMOVE position (packed lParam). Windows re-sends WM_MOUSEMOVE for a stationary cursor when the window
/// under it changes (the WebView above is hidden), which must not count as user activity (spike review H1).
static LAST_MOVE: AtomicIsize = AtomicIsize::new(-1);

/// User input that reached the video window (the overlay was hidden).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    /// `mousemove | click | dblclick | contextmenu | wheel | key`.
    pub kind: &'static str,
    /// Virtual-key code for `key`.
    pub vk: Option<u32>,
    /// The character this key produces in the active keyboard layout (`#` on a German keyboard is VK_OEM_2), like
    /// a DOM `KeyboardEvent.key`. `None` for keys without a printable character, dead keys and Ctrl chords.
    pub ch: Option<String>,
    pub shift: bool,
    pub ctrl: bool,
    /// Wheel delta (120 per notch, positive = away from the user).
    pub delta: i32,
}

fn hwnd(v: isize) -> HWND {
    HWND(v as *mut _)
}

pub fn main_hwnd() -> HWND {
    hwnd(MAIN.load(Ordering::SeqCst))
}

pub fn video_hwnd() -> HWND {
    hwnd(VIDEO.load(Ordering::SeqCst))
}

pub fn webview_hwnd() -> HWND {
    hwnd(WEBVIEW.load(Ordering::SeqCst))
}

pub fn on_activity(f: impl Fn(Activity) + Send + Sync + 'static) {
    let _ = ACTIVITY.set(Box::new(f));
}

/// Window moved, DPI or display configuration changed (the output monitor may have changed).
pub fn on_moved(f: impl Fn() + Send + Sync + 'static) {
    let _ = MOVED.set(Box::new(f));
}

pub fn set_playing(playing: bool) {
    PLAYING.store(playing, Ordering::SeqCst);
    if !playing {
        set_overlay_visible(true);
    }
}

/// The printable character for a key press in the thread's keyboard layout, without changing dead-key state.
fn layout_char(vk: u32, lp: LPARAM) -> Option<String> {
    let mut state = [0u8; 256];
    // SAFETY: fills our own 256-byte buffer with the calling thread's key state.
    unsafe { GetKeyboardState(&mut state) }.ok()?;
    let mut buf = [0u16; 8];
    let scan = ((lp.0 >> 16) & 0xff) as u32;
    // SAFETY: valid buffers; flag 0x4 keeps the kernel keyboard state (dead keys) unchanged (Windows 10 1607+).
    let n = unsafe { ToUnicode(vk, scan, Some(&state), &mut buf, 0x4) };
    if n != 1 {
        return None; // dead key (-1), nothing (0) or a ligature
    }
    let c = char::from_u32(u32::from(buf[0]))?;
    (!c.is_control()).then(|| c.to_string())
}

fn key_down(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    // SAFETY: read-only query of the calling thread's key state.
    unsafe { GetKeyState(i32::from(vk.0)) < 0 }
}

unsafe extern "system" fn video_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_MOUSEMOVE && LAST_MOVE.swap(lp.0 & 0xffff_ffff, Ordering::SeqCst) == lp.0 & 0xffff_ffff {
        return LRESULT(0);
    }
    if msg == WM_SETCURSOR && PLAYING.load(Ordering::SeqCst) && !is_visible(webview_hwnd()) {
        // Idle overlay during playback: no cursor over the picture.
        // SAFETY: plain cursor change on the UI thread.
        unsafe { SetCursor(None) };
        return LRESULT(1);
    }
    let kind = match msg {
        WM_MOUSEMOVE => Some("mousemove"),
        WM_LBUTTONDOWN => Some("click"),
        WM_LBUTTONDBLCLK => Some("dblclick"),
        WM_RBUTTONUP => Some("contextmenu"),
        WM_MOUSEWHEEL => Some("wheel"),
        WM_KEYDOWN | WM_SYSKEYDOWN => Some("key"),
        _ => None,
    };
    let Some(kind) = kind else {
        // SAFETY: default processing for everything else.
        return unsafe { DefWindowProcW(h, msg, wp, lp) };
    };
    let is_key = kind == "key";
    if !is_key {
        set_overlay_visible(true);
    }
    if let Some(f) = ACTIVITY.get() {
        f(Activity {
            kind,
            vk: is_key.then_some(wp.0 as u32),
            ch: if is_key { layout_char(wp.0 as u32, lp) } else { None },
            shift: key_down(VK_SHIFT),
            ctrl: key_down(VK_CONTROL),
            delta: if msg == WM_MOUSEWHEEL {
                i32::from(((wp.0 >> 16) & 0xffff) as u16 as i16)
            } else {
                0
            },
        });
    }
    if msg == WM_SYSKEYDOWN {
        // Keep Alt+F4 and the system menu working.
        // SAFETY: default processing.
        return unsafe { DefWindowProcW(h, msg, wp, lp) };
    }
    LRESULT(0)
}

unsafe extern "system" fn main_subclass(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _d: usize) -> LRESULT {
    match msg {
        WM_SIZE => {
            let width = (lp.0 & 0xffff) as i32;
            let height = ((lp.0 >> 16) & 0xffff) as i32;
            let v = video_hwnd();
            if !v.is_invalid() {
                // SAFETY: v is our own child window.
                unsafe {
                    let _ = SetWindowPos(v, Some(HWND_BOTTOM), 0, 0, width, height, SWP_NOACTIVATE | SWP_NOMOVE);
                }
            }
        }
        WM_MOVE | WM_DISPLAYCHANGE | WM_DPICHANGED => {
            if let Some(f) = MOVED.get() {
                f();
            }
        }
        _ => {}
    }
    // SAFETY: forward to the next handler in the chain.
    unsafe { DefSubclassProc(h, msg, wp, lp) }
}

unsafe extern "system" fn find_wry(h: HWND, lp: LPARAM) -> BOOL {
    if class_of(h) == "WRY_WEBVIEW" {
        // SAFETY: lp points to an isize owned by the caller for the duration of the enumeration.
        unsafe { *(lp.0 as *mut isize) = h.0 as isize };
        return BOOL(0);
    }
    BOOL(1)
}

/// Attach to the Tauri main window (UI thread): subclass it, find the WRY container and create the video child.
pub fn attach(main: isize) -> Result<(), String> {
    let main = hwnd(main);
    MAIN.store(main.0 as isize, Ordering::SeqCst);
    let mut wry: isize = 0;
    // The WRY container appears once the webview exists; it normally does by the time `setup` returns.
    for _ in 0..20 {
        // SAFETY: the callback only writes to `wry`, which outlives the call.
        unsafe {
            let _ = EnumChildWindows(Some(main), Some(find_wry), LPARAM(&mut wry as *mut isize as isize));
        }
        if wry != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if wry == 0 {
        return Err("WRY_WEBVIEW container not found".into());
    }
    WEBVIEW.store(wry, Ordering::SeqCst);
    // SAFETY: subclassing our own top-level window on its UI thread.
    if !unsafe { SetWindowSubclass(main, Some(main_subclass), 1, 0) }.as_bool() {
        return Err("SetWindowSubclass failed".into());
    }
    create_video_child()?;
    Ok(())
}

fn create_video_child() -> Result<HWND, String> {
    let existing = video_hwnd();
    if !existing.is_invalid() {
        return Ok(existing);
    }
    let main = main_hwnd();
    // SAFETY: standard Win32 class registration and child creation on the UI thread that owns `main`.
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let cls = w!("GranaryVideo");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(video_proc),
            hInstance: hinst.into(),
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: cls,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let mut rc = RECT::default();
        let _ = GetClientRect(main, &mut rc);
        let v = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            cls,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            rc.right - rc.left,
            rc.bottom - rc.top,
            Some(main),
            None,
            Some(hinst.into()),
            None,
        )
        .map_err(|e| e.to_string())?;
        let _ = SetWindowPos(
            v,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        VIDEO.store(v.0 as isize, Ordering::SeqCst);
        Ok(v)
    }
}

/// Show or hide the WebView container (UI thread for the focus change to take effect).
pub fn set_overlay_visible(visible: bool) {
    let web = webview_hwnd();
    if web.is_invalid() || is_visible(web) == visible {
        return;
    }
    // SAFETY: `web` is the WRY container child of our main window; `video` is our own child.
    unsafe {
        let _ = ShowWindow(web, if visible { SW_SHOWNA } else { SW_HIDE });
        let _ = SetFocus(Some(if visible { web } else { video_hwnd() }));
    }
}

pub fn overlay_visible() -> bool {
    is_visible(webview_hwnd())
}

fn is_visible(h: HWND) -> bool {
    // SAFETY: read-only query.
    !h.is_invalid() && unsafe { IsWindowVisible(h) }.as_bool()
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: read-only query into a local buffer.
    let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

unsafe extern "system" fn disable_child(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: h is a descendant of our video window; lp points to a counter owned by the caller.
    unsafe {
        if IsWindowEnabled(h).as_bool() {
            let _ = EnableWindow(h, false);
            *(lp.0 as *mut u32) += 1;
        }
    }
    BOOL(1)
}

/// Disable mpv's own window inside GranaryVideo so mouse input reaches `video_proc`. Returns how many were disabled.
pub fn disable_mpv_children() -> u32 {
    let v = video_hwnd();
    let mut n = 0u32;
    if !v.is_invalid() {
        // SAFETY: the callback only writes to `n`, which outlives the call.
        unsafe {
            let _ = EnumChildWindows(Some(v), Some(disable_child), LPARAM(&mut n as *mut u32 as isize));
        }
    }
    n
}

/// Is the video child below the WebView container in sibling z-order (Design A invariant)?
pub fn video_below_webview() -> bool {
    let (v, mut cur) = (video_hwnd(), webview_hwnd());
    for _ in 0..64 {
        // SAFETY: read-only z-order walk.
        match unsafe { GetWindow(cur, GW_HWNDNEXT) } {
            Ok(n) if !n.is_invalid() => {
                if n == v {
                    return true;
                }
                cur = n;
            }
            _ => break,
        }
    }
    false
}

/// GDI device name (`\\.\DISPLAY1`) of the monitor showing most of the main window.
pub fn monitor_device() -> Option<String> {
    let main = main_hwnd();
    if main.is_invalid() {
        return None;
    }
    // SAFETY: read-only monitor queries; MONITORINFOEXW starts with MONITORINFO and cbSize says which one it is.
    unsafe {
        let mon = MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(mon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
            return None;
        }
        let n = info
            .szDevice
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(info.szDevice.len());
        Some(String::from_utf16_lossy(&info.szDevice[..n]))
    }
}
