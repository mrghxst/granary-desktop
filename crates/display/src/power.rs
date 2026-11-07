//! Power source (phase-7 review M6): Windows refuses to switch HDR on while a laptop runs on battery (unless the
//! user allowed it in Settings), and the refusal only shows as a read-back timeout. The session remembers a refusal
//! per monitor and power source, so later titles do not wait for it again.

/// `true` when the system reports it runs on battery (AC line offline). Unknown counts as mains power.
#[cfg(windows)]
pub fn on_battery() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` is a valid, writable SYSTEM_POWER_STATUS for the duration of the call.
    let ok = unsafe { GetSystemPowerStatus(&mut status) }.is_ok();
    ok && status.ACLineStatus == 0
}

#[cfg(not(windows))]
pub fn on_battery() -> bool {
    false
}
