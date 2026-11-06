//! Spike: Design A (ADR-0003) — GranaryVideo child HWND under the WRY_WEBVIEW container.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled, SetFocus};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, w};

static MAIN: AtomicIsize = AtomicIsize::new(0);
static VIDEO: AtomicIsize = AtomicIsize::new(0);
static WEBVIEW: AtomicIsize = AtomicIsize::new(0);
static ACTIVITY: OnceLock<Box<dyn Fn(&'static str) + Send + Sync>> = OnceLock::new();
static RESIZE: OnceLock<Box<dyn Fn(u32, u32) + Send + Sync>> = OnceLock::new();
static MOVED: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn hwnd(v: isize) -> HWND {
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

pub fn on_activity(f: impl Fn(&'static str) + Send + Sync + 'static) {
    let _ = ACTIVITY.set(Box::new(f));
}
pub fn on_resize(f: impl Fn(u32, u32) + Send + Sync + 'static) {
    let _ = RESIZE.set(Box::new(f));
}
pub fn on_moved(f: impl Fn() + Send + Sync + 'static) {
    let _ = MOVED.set(Box::new(f));
}

unsafe extern "system" fn video_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let kind = match msg {
        WM_MOUSEMOVE => Some("mousemove"),
        WM_LBUTTONDOWN => Some("click"),
        WM_LBUTTONDBLCLK => Some("dblclick"),
        WM_KEYDOWN => Some("key"),
        WM_MOUSEWHEEL => Some("wheel"),
        _ => None,
    };
    if let Some(k) = kind {
        set_webview_visible(true);
        if let Some(f) = ACTIVITY.get() {
            f(k);
        }
        return LRESULT(0);
    }
    // SAFETY: default processing for everything else.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

unsafe extern "system" fn main_subclass(
    h: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _d: usize,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            let w = (lp.0 & 0xffff) as i32;
            let hh = ((lp.0 >> 16) & 0xffff) as i32;
            let v = video_hwnd();
            if !v.is_invalid() {
                // SAFETY: v is our own child window.
                unsafe {
                    let _ = SetWindowPos(
                        v,
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        w,
                        hh,
                        SWP_NOACTIVATE | SWP_NOMOVE,
                    );
                }
            }
            if let Some(f) = RESIZE.get() {
                f(w as u32, hh as u32);
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
    let mut buf = [0u16; 64];
    // SAFETY: buffer is large enough for class names we care about.
    let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
    let name = String::from_utf16_lossy(&buf[..n]);
    if name == "WRY_WEBVIEW" {
        // SAFETY: lp points to an isize owned by the caller for the duration of the enumeration.
        unsafe { *(lp.0 as *mut isize) = h.0 as isize };
        return BOOL(0);
    }
    BOOL(1)
}

/// Attach to the Tauri main window: subclass it and locate the WRY_WEBVIEW container.
pub fn attach(main: HWND) -> Result<(), String> {
    MAIN.store(main.0 as isize, Ordering::SeqCst);
    let mut wry: isize = 0;
    // SAFETY: callback only writes to `wry`, which outlives the call.
    unsafe {
        let _ = EnumChildWindows(
            Some(main),
            Some(find_wry),
            LPARAM(&mut wry as *mut isize as isize),
        );
        if !SetWindowSubclass(main, Some(main_subclass), 1, 0).as_bool() {
            return Err("SetWindowSubclass failed".into());
        }
    }
    WEBVIEW.store(wry, Ordering::SeqCst);
    if wry == 0 {
        Err("WRY_WEBVIEW not found".into())
    } else {
        Ok(())
    }
}

/// Create the GranaryVideo child at the bottom of the z-order, sized to the client rect.
pub fn create_video_child() -> Result<HWND, String> {
    let existing = video_hwnd();
    if !existing.is_invalid() {
        return Ok(existing);
    }
    let main = main_hwnd();
    // SAFETY: standard Win32 class registration and window creation on the UI thread.
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

pub fn set_webview_visible(visible: bool) {
    let w = webview_hwnd();
    if w.is_invalid() {
        return;
    }
    // SAFETY: w is the WRY container child of our main window.
    unsafe {
        let _ = ShowWindow(w, if visible { SW_SHOWNA } else { SW_HIDE });
        if visible {
            let _ = SetFocus(Some(w));
        }
    }
}

pub fn is_visible(h: HWND) -> bool {
    // SAFETY: read-only query.
    !h.is_invalid() && unsafe { IsWindowVisible(h) }.as_bool()
}

pub fn rect_of(h: HWND) -> (i32, i32, i32, i32) {
    let mut r = RECT::default();
    // SAFETY: read-only query.
    unsafe {
        let _ = GetWindowRect(h, &mut r);
    }
    (r.left, r.top, r.right - r.left, r.bottom - r.top)
}

pub fn client_size(h: HWND) -> (i32, i32) {
    let mut r = RECT::default();
    // SAFETY: read-only query.
    unsafe {
        let _ = GetClientRect(h, &mut r);
    }
    (r.right - r.left, r.bottom - r.top)
}

/// Is the video child below the webview container in sibling z-order?
pub fn video_below_webview() -> bool {
    let (v, w) = (video_hwnd(), webview_hwnd());
    // Walk siblings from the webview downwards; if we meet the video, it's below.
    let mut cur = w;
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

/// Disable mpv's own window(s) inside GranaryVideo so mouse input is routed to GranaryVideo (our proc).
/// Returns how many windows were newly disabled.
pub fn disable_mpv_children() -> u32 {
    let v = video_hwnd();
    let mut n = 0u32;
    if !v.is_invalid() {
        // SAFETY: callback only writes to `n`, which outlives the call.
        unsafe {
            let _ = EnumChildWindows(
                Some(v),
                Some(disable_child),
                LPARAM(&mut n as *mut u32 as isize),
            );
        }
    }
    n
}

/// Give keyboard focus to the video window (used while the overlay is hidden).
pub fn focus_video() {
    let v = video_hwnd();
    if !v.is_invalid() {
        // SAFETY: v is our own child window; only effective on the UI thread.
        unsafe {
            let _ = SetFocus(Some(v));
        }
    }
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: read-only query into a local buffer.
    let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: lp points to a Vec owned by the caller for the duration of the enumeration.
    let v = unsafe { &mut *(lp.0 as *mut Vec<String>) };
    // SAFETY: read-only queries.
    let (parent, en, tid) = unsafe {
        let mut pid = 0u32;
        (
            GetParent(h).unwrap_or_default(),
            IsWindowEnabled(h).as_bool(),
            GetWindowThreadProcessId(h, Some(&mut pid)),
        )
    };
    v.push(format!(
        "{:#x} parent={:#x} class={} visible={} enabled={} tid={} rect={:?}",
        h.0 as isize,
        parent.0 as isize,
        class_of(h),
        is_visible(h),
        en,
        tid,
        rect_of(h)
    ));
    BOOL(1)
}

/// Debug: every descendant of the main window, in z-order per level.
pub fn dump_tree() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    // SAFETY: callback only pushes to `v`, which outlives the call.
    unsafe {
        let _ = EnumChildWindows(
            Some(main_hwnd()),
            Some(collect),
            LPARAM(&mut v as *mut Vec<String> as isize),
        );
    }
    v
}

/// Debug: class + handle of the window under a screen point.
pub fn window_at(x: i32, y: i32) -> String {
    // SAFETY: read-only query.
    let h = unsafe { WindowFromPoint(POINT { x, y }) };
    format!("{:#x} {}", h.0 as isize, class_of(h))
}

/// Is our main window the foreground window (so synthetic input cannot reach another app)?
pub fn is_foreground() -> bool {
    // SAFETY: read-only query.
    let f = unsafe { GetForegroundWindow() };
    !f.is_invalid() && f == main_hwnd()
}

/// Is the window under the point ours (main window or a descendant)?
pub fn point_is_ours(x: i32, y: i32) -> bool {
    // SAFETY: read-only queries.
    unsafe {
        let h = WindowFromPoint(POINT { x, y });
        !h.is_invalid() && (h == main_hwnd() || IsChild(main_hwnd(), h).as_bool())
    }
}
