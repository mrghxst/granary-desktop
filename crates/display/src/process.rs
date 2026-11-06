//! Identity of the process that owns a restore marker (phase-3 review H1): a second launch must not "repair" display
//! changes that a still-running instance made. A pid alone can be reused, so the process creation time is compared too.
use serde::{Deserialize, Serialize};

/// Process that wrote a restore marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerOwner {
    pub pid: u32,
    /// Process creation time (Windows FILETIME, 100 ns since 1601).
    pub started: u64,
}

/// The current process, if its creation time can be read.
pub fn current() -> Option<MarkerOwner> {
    imp::creation_time(std::process::id()).map(|started| MarkerOwner {
        pid: std::process::id(),
        started,
    })
}

/// `true` if the owning process is still running (same pid *and* same creation time).
pub fn is_alive(owner: &MarkerOwner) -> bool {
    imp::creation_time(owner.pid).is_some_and(|t| t == owner.started)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// Creation time of a *running* process; `None` if it does not exist, has exited or cannot be queried.
    pub fn creation_time(pid: u32) -> Option<u64> {
        // SAFETY: plain Win32 calls on a handle we own and close; out-params are valid stack locations.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut code = 0u32;
            let running = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
            let (mut c, mut e, mut k, mut u) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            let times = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
            let _ = CloseHandle(h);
            (running && times).then(|| (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn creation_time(_pid: u32) -> Option<u64> {
        None
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive_and_stale_owner_is_not() {
        let me = current();
        assert!(me.is_some_and(|o| is_alive(&o)));
        if let Some(o) = me {
            assert!(
                !is_alive(&MarkerOwner {
                    started: o.started.wrapping_add(1),
                    ..o
                }),
                "pid reuse must not match"
            );
        }
        assert!(!is_alive(&MarkerOwner {
            pid: u32::MAX - 3,
            started: 1
        }));
    }
}
