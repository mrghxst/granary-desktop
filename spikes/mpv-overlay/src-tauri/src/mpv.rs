//! Spike: minimal safe-ish wrapper over libmpv2-sys with an event thread.
use libmpv2_sys::*;
use std::ffi::{CStr, CString, c_char};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Mpv {
    h: *mut mpv_handle,
}

// SAFETY: the libmpv client API is documented as thread-safe for all functions on one handle
// (except mpv_wait_event, which only the event thread calls).
unsafe impl Send for Mpv {}
// SAFETY: see above.
unsafe impl Sync for Mpv {}

fn cs(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: caller guarantees p is a valid NUL-terminated string from mpv.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

pub enum Event {
    Log {
        prefix: String,
        level: String,
        text: String,
    },
    FileLoaded,
    EndFile,
    PlaybackRestart,
    VideoReconfig,
    Property {
        name: String,
        value: String,
    },
    Shutdown,
}

impl Mpv {
    pub fn new(opts: &[(&str, String)]) -> Result<Arc<Self>, String> {
        // SAFETY: mpv_create returns null on failure, checked below.
        let h = unsafe { mpv_create() };
        if h.is_null() {
            return Err("mpv_create failed".into());
        }
        let m = Self { h };
        for (k, v) in opts {
            // SAFETY: valid handle before initialisation; both C strings outlive the call.
            let r = unsafe { mpv_set_option_string(m.h, cs(k).as_ptr(), cs(v).as_ptr()) };
            if r < 0 {
                return Err(format!("option {k}={v} failed: {r}"));
            }
        }
        // SAFETY: valid, not yet initialised handle.
        let r = unsafe { mpv_initialize(m.h) };
        if r < 0 {
            return Err(format!("mpv_initialize failed: {r}"));
        }
        // SAFETY: initialised handle; the level string outlives the call.
        unsafe { mpv_request_log_messages(m.h, cs("v").as_ptr()) };
        Ok(Arc::new(m))
    }

    pub fn command(&self, args: &[&str]) -> i32 {
        let owned: Vec<CString> = args.iter().map(|a| cs(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: NUL-terminated argv of valid C strings that outlive the call.
        unsafe { mpv_command(self.h, ptrs.as_mut_ptr()) }
    }

    pub fn set(&self, name: &str, value: &str) -> i32 {
        // SAFETY: thread-safe client API call on a live handle; C strings outlive the call.
        unsafe { mpv_set_property_string(self.h, cs(name).as_ptr(), cs(value).as_ptr()) }
    }

    pub fn get(&self, name: &str) -> Option<String> {
        // SAFETY: returned string is freed with mpv_free.
        unsafe {
            let p = mpv_get_property_string(self.h, cs(name).as_ptr());
            if p.is_null() {
                return None;
            }
            let s = cstr(p);
            mpv_free(p as *mut _);
            Some(s)
        }
    }

    pub fn observe(&self, name: &str) {
        // SAFETY: live handle; mpv copies the property name.
        unsafe { mpv_observe_property(self.h, 0, cs(name).as_ptr(), mpv_format_MPV_FORMAT_STRING) };
    }

    /// Run the event loop on a new thread until shutdown.
    pub fn spawn_events(
        self: &Arc<Self>,
        mut f: impl FnMut(Event) + Send + 'static,
    ) -> Arc<AtomicBool> {
        let me = Arc::clone(self);
        let alive = Arc::new(AtomicBool::new(true));
        let alive2 = Arc::clone(&alive);
        std::thread::spawn(move || {
            loop {
                // SAFETY: only this thread calls mpv_wait_event; the event is valid until the next call.
                let ev = unsafe { &*mpv_wait_event(me.h, 1.0) };
                let e = match ev.event_id {
                    x if x == mpv_event_id_MPV_EVENT_SHUTDOWN => {
                        f(Event::Shutdown);
                        break;
                    }
                    // SAFETY: for LOG_MESSAGE, data points to an mpv_event_log_message valid until the next wait.
                    x if x == mpv_event_id_MPV_EVENT_LOG_MESSAGE => unsafe {
                        let m = &*(ev.data as *const mpv_event_log_message);
                        Event::Log {
                            prefix: cstr(m.prefix),
                            level: cstr(m.level),
                            text: cstr(m.text),
                        }
                    },
                    x if x == mpv_event_id_MPV_EVENT_FILE_LOADED => Event::FileLoaded,
                    x if x == mpv_event_id_MPV_EVENT_END_FILE => Event::EndFile,
                    x if x == mpv_event_id_MPV_EVENT_PLAYBACK_RESTART => Event::PlaybackRestart,
                    x if x == mpv_event_id_MPV_EVENT_VIDEO_RECONFIG => Event::VideoReconfig,
                    // SAFETY: for PROPERTY_CHANGE, data points to an mpv_event_property; with FORMAT_STRING its data is a char**.
                    x if x == mpv_event_id_MPV_EVENT_PROPERTY_CHANGE => unsafe {
                        let p = &*(ev.data as *const mpv_event_property);
                        let value = if p.format == mpv_format_MPV_FORMAT_STRING && !p.data.is_null()
                        {
                            cstr(*(p.data as *const *const c_char))
                        } else {
                            String::new()
                        };
                        Event::Property {
                            name: cstr(p.name),
                            value,
                        }
                    },
                    _ => continue,
                };
                f(e);
            }
            alive2.store(false, Ordering::SeqCst);
        });
        alive
    }

    pub fn quit(&self) {
        self.command(&["quit"]);
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // SAFETY: last reference; terminates the core and frees the handle.
        unsafe { mpv_terminate_destroy(self.h) };
    }
}
