//! In-process libmpv backend (ADR-0002): one handle, one dedicated event thread, typed property observation.
//! libmpv is delay-loaded; call [`crate::probe::probe_libmpv`] before [`Mpv::new`].
use crate::PlayerError;
use crate::backend::PlayerBackend;
use crate::events::{EndReason, MpvEvent, PropValue};
use crate::state::ObserveFormat;
use libmpv2_sys::*;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::Arc;

pub struct Mpv {
    h: *mut mpv_handle,
}

// SAFETY: the libmpv client API is thread-safe for all functions on one handle, except `mpv_wait_event`, which only
// the event thread started by `spawn_events` calls (documented in client.h, "Thread safety").
unsafe impl Send for Mpv {}
// SAFETY: see `Send` above.
unsafe impl Sync for Mpv {}

fn cs(s: &str) -> CString {
    // Interior NULs cannot be represented; strip them rather than fail (mpv option values never need them).
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// # Safety
/// `p` must be null or a valid NUL-terminated string that stays alive for the duration of the call.
unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: guaranteed by the caller.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

fn error_string(code: c_int) -> String {
    // SAFETY: mpv_error_string returns a static string for every code.
    unsafe { cstr(mpv_error_string(code)) }
}

fn check(code: c_int, context: &str) -> Result<(), PlayerError> {
    if code >= 0 {
        Ok(())
    } else {
        Err(PlayerError::Mpv {
            code,
            message: error_string(code),
            context: context.to_string(),
        })
    }
}

impl Mpv {
    /// Create, configure (options in order) and initialise a handle.
    pub fn new(options: &[(String, String)]) -> Result<Arc<Self>, PlayerError> {
        // SAFETY: returns null on failure, checked below.
        let h = unsafe { mpv_create() };
        if h.is_null() {
            return Err(PlayerError::Unavailable("mpv_create returned null".into()));
        }
        // From here on Drop destroys the handle on every error path.
        let m = Self { h };
        for (k, v) in options {
            // SAFETY: live handle before initialisation; both C strings outlive the call.
            let r = unsafe { mpv_set_option_string(m.h, cs(k).as_ptr(), cs(v).as_ptr()) };
            check(r, &format!("option {k}"))?;
        }
        // SAFETY: live, not yet initialised handle.
        check(unsafe { mpv_initialize(m.h) }, "mpv_initialize")?;
        // SAFETY: initialised handle; the level string outlives the call.
        check(
            unsafe { mpv_request_log_messages(m.h, cs("info").as_ptr()) },
            "request_log_messages",
        )?;
        Ok(Arc::new(m))
    }

    /// Observe a property; changes arrive as [`MpvEvent::Property`].
    pub fn observe(&self, name: &str, format: ObserveFormat) -> Result<(), PlayerError> {
        let f = match format {
            ObserveFormat::Flag => mpv_format_MPV_FORMAT_FLAG,
            ObserveFormat::Int => mpv_format_MPV_FORMAT_INT64,
            ObserveFormat::Double => mpv_format_MPV_FORMAT_DOUBLE,
            ObserveFormat::Str => mpv_format_MPV_FORMAT_STRING,
        };
        // SAFETY: live handle; mpv copies the name.
        check(
            unsafe { mpv_observe_property(self.h, 0, cs(name).as_ptr(), f) },
            "observe",
        )
    }

    /// Run the event loop on a dedicated thread until SHUTDOWN; `sink` receives every translated event.
    pub fn spawn_events(
        self: &Arc<Self>,
        mut sink: impl FnMut(MpvEvent) + Send + 'static,
    ) -> std::io::Result<std::thread::JoinHandle<()>> {
        let me = Arc::clone(self);
        std::thread::Builder::new().name("mpv-events".into()).spawn(move || {
            loop {
                // SAFETY: only this thread calls mpv_wait_event; the returned event stays valid until the next call.
                let ev = unsafe { &*mpv_wait_event(me.h, -1.0) };
                // SAFETY: `ev` is the event just returned by mpv_wait_event for this handle.
                let Some(e) = (unsafe { translate(ev) }) else { continue };
                let shutdown = matches!(e, MpvEvent::Shutdown);
                sink(e);
                if shutdown {
                    break;
                }
            }
        })
    }

    /// Ask the core to quit; the event thread then receives SHUTDOWN and exits.
    pub fn quit(&self) {
        let _ = self.command(&["quit"]);
    }
}

/// # Safety
/// `ev` must be an event returned by `mpv_wait_event` that has not been invalidated by a later call.
unsafe fn translate(ev: &mpv_event) -> Option<MpvEvent> {
    let id = ev.event_id;
    Some(match id {
        x if x == mpv_event_id_MPV_EVENT_SHUTDOWN => MpvEvent::Shutdown,
        x if x == mpv_event_id_MPV_EVENT_START_FILE => MpvEvent::StartFile,
        x if x == mpv_event_id_MPV_EVENT_FILE_LOADED => MpvEvent::FileLoaded,
        x if x == mpv_event_id_MPV_EVENT_PLAYBACK_RESTART => MpvEvent::PlaybackRestart,
        x if x == mpv_event_id_MPV_EVENT_VIDEO_RECONFIG => MpvEvent::VideoReconfig,
        x if x == mpv_event_id_MPV_EVENT_AUDIO_RECONFIG => MpvEvent::AudioReconfig,
        x if x == mpv_event_id_MPV_EVENT_SEEK => MpvEvent::Seek,
        x if x == mpv_event_id_MPV_EVENT_END_FILE => {
            // SAFETY: for END_FILE, data points to an mpv_event_end_file.
            let d = unsafe { &*(ev.data as *const mpv_event_end_file) };
            let reason = match d.reason {
                r if r == mpv_end_file_reason_MPV_END_FILE_REASON_EOF => EndReason::Eof,
                r if r == mpv_end_file_reason_MPV_END_FILE_REASON_STOP => EndReason::Stop,
                r if r == mpv_end_file_reason_MPV_END_FILE_REASON_QUIT => EndReason::Quit,
                r if r == mpv_end_file_reason_MPV_END_FILE_REASON_ERROR => EndReason::Error(error_string(d.error)),
                r if r == mpv_end_file_reason_MPV_END_FILE_REASON_REDIRECT => EndReason::Redirect,
                r => EndReason::Unknown(r),
            };
            MpvEvent::EndFile { reason }
        }
        x if x == mpv_event_id_MPV_EVENT_LOG_MESSAGE => {
            // SAFETY: for LOG_MESSAGE, data points to an mpv_event_log_message whose strings live until the next wait.
            let m = unsafe { &*(ev.data as *const mpv_event_log_message) };
            // SAFETY: as above.
            unsafe {
                MpvEvent::Log {
                    prefix: cstr(m.prefix),
                    level: cstr(m.level),
                    text: cstr(m.text),
                }
            }
        }
        x if x == mpv_event_id_MPV_EVENT_PROPERTY_CHANGE => {
            // SAFETY: for PROPERTY_CHANGE, data points to an mpv_event_property.
            let p = unsafe { &*(ev.data as *const mpv_event_property) };
            // SAFETY: `p.data` has the layout given by `p.format` (or is null when the property is unavailable).
            let value = unsafe { prop_value(p.format, p.data) };
            // SAFETY: the name is a valid C string owned by the event.
            MpvEvent::Property {
                name: unsafe { cstr(p.name) },
                value,
            }
        }
        _ => return None,
    })
}

/// # Safety
/// `data` must be null or point to a value of the layout `format` describes.
unsafe fn prop_value(format: mpv_format, data: *mut c_void) -> PropValue {
    if data.is_null() {
        return PropValue::None;
    }
    // SAFETY: layout guaranteed by the caller.
    unsafe {
        match format {
            f if f == mpv_format_MPV_FORMAT_FLAG => PropValue::Flag(*(data as *const c_int) != 0),
            f if f == mpv_format_MPV_FORMAT_INT64 => PropValue::Int(*(data as *const i64)),
            f if f == mpv_format_MPV_FORMAT_DOUBLE => PropValue::Double(*(data as *const f64)),
            f if f == mpv_format_MPV_FORMAT_STRING => PropValue::Str(cstr(*(data as *const *const c_char))),
            _ => PropValue::None,
        }
    }
}

impl PlayerBackend for Mpv {
    fn command(&self, args: &[&str]) -> Result<(), PlayerError> {
        let owned: Vec<CString> = args.iter().map(|a| cs(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: NULL-terminated argv of C strings that outlive the call.
        check(
            unsafe { mpv_command(self.h, ptrs.as_mut_ptr()) },
            args.first().copied().unwrap_or("command"),
        )
    }

    fn set_property(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        // SAFETY: live handle; C strings outlive the call.
        check(
            unsafe { mpv_set_property_string(self.h, cs(name).as_ptr(), cs(value).as_ptr()) },
            name,
        )
    }

    fn get_property(&self, name: &str) -> Option<String> {
        // SAFETY: the returned string is owned by us and released with mpv_free.
        unsafe {
            let p = mpv_get_property_string(self.h, cs(name).as_ptr());
            if p.is_null() {
                return None;
            }
            let s = cstr(p);
            mpv_free(p as *mut c_void);
            Some(s)
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // SAFETY: last reference (Arc dropped); terminates the core, joins its threads and frees the handle.
        unsafe { mpv_terminate_destroy(self.h) };
    }
}
