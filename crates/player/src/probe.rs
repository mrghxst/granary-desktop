//! libmpv is linked with `/DELAYLOAD`, so a missing or broken `libmpv-2.dll` would only fail at the first mpv call
//! (as an SEH exception). Probe it explicitly first and turn failure into an error the UI can show (ADR-0005).
use crate::PlayerError;
use windows::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LoadLibraryExW};

/// Load `libmpv-2.dll` from the application directory (+ System32 for its own imports, never CWD/PATH) and return
/// the client API version. Safe to call repeatedly.
pub fn probe_libmpv() -> Result<u64, PlayerError> {
    // SAFETY: loads a DLL by name with a restricted search path; the module handle is intentionally kept for the
    // lifetime of the process (the delay-load helper reuses it).
    let loaded = unsafe {
        LoadLibraryExW(
            windows::core::w!("libmpv-2.dll"),
            None,
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    match loaded {
        Err(e) => Err(PlayerError::Unavailable(format!(
            "libmpv-2.dll could not be loaded: {e}"
        ))),
        Ok(_) => {
            // SAFETY: the DLL is loaded, so the delay-load stub resolves; the function has no preconditions.
            let v = unsafe { libmpv2_sys::mpv_client_api_version() };
            Ok(u64::from(v))
        }
    }
}
