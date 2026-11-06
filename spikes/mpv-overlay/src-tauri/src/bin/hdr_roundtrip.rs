//! Phase 2 spike: toggle Windows HDR on each HDR-capable display and restore it, with a restore marker.
//! Usage: hdr-roundtrip [--repair] [--hold-secs N]
//!        hdr-roundtrip --with [hdr=on|off] [hz=N] [--timeout-secs N] -- <command> [args]
//!   default: for each HDR-capable target: write marker, flip HDR, read back, flip back, read back, delete marker.
//!   --repair: only apply an existing marker (crash recovery path) and exit.
//! Safety (review H2): the marker is only deleted after a *verified* restore of HDR and refresh; an unreadable marker
//! is kept; entries are matched by the reboot-stable monitor device path when the adapter LUID changed; Ctrl+C or
//! closing the console triggers a repair; the `--with` child command has a timeout.
#[path = "../display.rs"]
#[allow(dead_code)]
mod display;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{FALSE, TRUE};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::core::BOOL;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    luid_low: u32,
    luid_high: i32,
    target_id: u32,
    monitor_path: String,
    original_hdr: bool,
    hdr_changed: bool,
    /// Original refresh (numerator, denominator) when the refresh rate was changed.
    original_refresh: Option<(u32, u32)>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    version: u32,
    entries: Vec<Entry>,
}

/// Set while a marker exists that this process wrote; the console handler only repairs then.
static ARMED: AtomicBool = AtomicBool::new(false);

fn marker_path() -> PathBuf {
    // LOCALAPPDATA always exists for an interactive user; no fallback to the current directory (review L7).
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    PathBuf::from(base).join("Granary").join("spike-display-restore.json")
}

fn write_marker(entries: Vec<Entry>) -> std::io::Result<()> {
    let p = marker_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let body = serde_json::to_vec_pretty(&Marker { version: 2, entries }).map_err(std::io::Error::other)?;
    let tmp = p.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &p)?;
    ARMED.store(true, Ordering::SeqCst);
    Ok(())
}

fn entry_for(d: &display::DisplayInfo, hdr_changed: bool, refresh_changed: bool) -> Entry {
    Entry {
        luid_low: d.adapter_luid.0,
        luid_high: d.adapter_luid.1,
        target_id: d.target_id,
        monitor_path: d.monitor_path.clone(),
        original_hdr: d.hdr_enabled,
        hdr_changed,
        original_refresh: refresh_changed.then_some(d.refresh_rational),
    }
}

/// Current state of the display an entry refers to: LUID+target first, then the stable monitor path.
fn find(e: &Entry) -> Option<display::DisplayInfo> {
    let ds = display::displays();
    ds.iter()
        .find(|d| d.adapter_luid == (e.luid_low, e.luid_high) && d.target_id == e.target_id)
        .or_else(|| ds.iter().find(|d| !e.monitor_path.is_empty() && d.monitor_path == e.monitor_path))
        .cloned()
}

fn state(luid: (u32, i32), target: u32) -> Option<display::DisplayInfo> {
    display::displays().into_iter().find(|d| d.adapter_luid == luid && d.target_id == target)
}

/// Wait until the HDR state reads back as `want` (DisplayConfig and DXGI colour space), up to `timeout`.
fn wait_for(luid: (u32, i32), target: u32, want: bool, timeout: Duration) -> (bool, Duration, Option<display::DisplayInfo>) {
    let t0 = Instant::now();
    loop {
        let s = state(luid, target);
        // DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 = 12, sRGB G22 = 0
        let ok = s.as_ref().is_some_and(|d| d.hdr_enabled == want && (d.dxgi_color_space == 12) == want);
        if ok || t0.elapsed() > timeout {
            return (ok, t0.elapsed(), s);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn hz(r: (u32, u32)) -> f64 {
    f64::from(r.0) / f64::from(r.1.max(1))
}

/// Restore every marker entry (HDR first, then refresh) and verify both. Returns 0 only if everything verified;
/// the marker is deleted only then.
fn repair() -> i32 {
    let p = marker_path();
    let body = match std::fs::read_to_string(&p) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no marker at {}", p.display());
            return 0;
        }
        Err(e) => {
            println!("marker unreadable ({e}); kept");
            return 1;
        }
    };
    let marker: Marker = match serde_json::from_str(&body) {
        Ok(m) => m,
        Err(e) => {
            println!("marker does not parse ({e}); kept for manual inspection at {}", p.display());
            return 1;
        }
    };
    let mut rc = 0;
    for e in &marker.entries {
        let Some(d) = find(e) else {
            println!("repair target {} ({}): display not present; marker kept", e.target_id, e.monitor_path);
            rc = 1;
            continue;
        };
        if e.hdr_changed {
            if d.hdr_enabled != e.original_hdr {
                let r = display::set_hdr(d.adapter_luid, d.target_id, e.original_hdr);
                let (ok, dt, _) = wait_for(d.adapter_luid, d.target_id, e.original_hdr, Duration::from_secs(10));
                println!("repair target {}: set hdr={} -> {r:?}, verified={ok} after {dt:?}", d.target_id, e.original_hdr);
                if !ok {
                    rc = 1;
                }
            } else {
                println!("repair target {}: already hdr={}", d.target_id, e.original_hdr);
            }
        }
        if let Some(orig) = e.original_refresh {
            let r = display::restore_registry_mode(&d.gdi_name);
            std::thread::sleep(Duration::from_millis(1500));
            let now = find(e).map(|s| s.refresh_hz);
            let ok = r.is_ok() && now.is_some_and(|n| (n - hz(orig)).abs() < 0.01);
            println!("repair refresh {}: {r:?}; now {now:?} Hz, want {:.3} -> verified={ok}", d.gdi_name, hz(orig));
            if !ok {
                rc = 1;
            }
        }
    }
    if rc == 0 {
        match std::fs::remove_file(&p) {
            Ok(()) => println!("display state repaired; marker deleted"),
            Err(e) => {
                println!("restored but marker could not be deleted: {e}");
                rc = 1;
            }
        }
        ARMED.store(false, Ordering::SeqCst);
    } else {
        println!("repair incomplete; marker kept");
    }
    rc
}

/// Ctrl+C, Ctrl+Break, console close, logoff, shutdown: repair before the process dies.
unsafe extern "system" fn on_console_event(_kind: u32) -> BOOL {
    if ARMED.load(Ordering::SeqCst) {
        println!("console event: repairing display state before exit");
        let rc = repair();
        std::process::exit(if rc == 0 { 130 } else { 1 });
    }
    FALSE
}

fn install_console_handler() {
    // SAFETY: registers a plain function pointer that lives for the whole process.
    if let Err(e) = unsafe { SetConsoleCtrlHandler(Some(on_console_event), TRUE.into()) } {
        println!("warning: SetConsoleCtrlHandler failed: {e}");
    }
}

fn main() {
    install_console_handler();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--repair") {
        std::process::exit(repair());
    }
    if let Some(i) = args.iter().position(|a| a == "--with") {
        std::process::exit(with_state(&args[i + 1..]));
    }
    let hold = args
        .iter()
        .position(|a| a == "--hold-secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(3);
    // A leftover marker from a crashed run is repaired first.
    if marker_path().exists() {
        println!("found leftover marker, repairing first");
        if repair() != 0 {
            std::process::exit(1);
        }
    }
    let ds = display::displays();
    println!("BEFORE: {}", serde_json::to_string_pretty(&ds).unwrap_or_default());
    let capable: Vec<_> = ds.iter().filter(|d| d.hdr_supported).cloned().collect();
    if capable.is_empty() {
        println!("no HDR-capable display; nothing to do");
        return;
    }
    if let Err(e) = write_marker(capable.iter().map(|d| entry_for(d, true, false)).collect()) {
        eprintln!("cannot write marker: {e}");
        std::process::exit(1);
    }
    println!("marker written: {}", marker_path().display());
    let mut rc = 0;
    for d in &capable {
        let (luid, t, orig) = (d.adapter_luid, d.target_id, d.hdr_enabled);
        println!("--- {} ({}) target {t}: original hdr={orig}", d.friendly_name, d.gdi_name);
        let r = display::set_hdr(luid, t, !orig);
        let (ok, dt, s) = wait_for(luid, t, !orig, Duration::from_secs(10));
        println!(
            "flip to hdr={} -> {r:?}; verified={ok} after {dt:?}; colorspace={:?} sdr_white={:?} max_lum={:?}",
            !orig,
            s.as_ref().map(|s| s.dxgi_color_space),
            s.as_ref().map(|s| s.sdr_white_nits),
            s.as_ref().map(|s| s.dxgi_max_luminance)
        );
        if !ok {
            rc = 1;
        }
        std::thread::sleep(Duration::from_secs(hold));
        let r = display::set_hdr(luid, t, orig);
        let (ok, dt, s) = wait_for(luid, t, orig, Duration::from_secs(10));
        println!(
            "restore to hdr={orig} -> {r:?}; verified={ok} after {dt:?}; colorspace={:?} sdr_white={:?}",
            s.as_ref().map(|s| s.dxgi_color_space),
            s.as_ref().map(|s| s.sdr_white_nits)
        );
        if !ok {
            rc = 1;
        }
    }
    let after = display::displays();
    let all_restored = capable.iter().all(|c| {
        after.iter().any(|a| a.target_id == c.target_id && a.adapter_luid == c.adapter_luid && a.hdr_enabled == c.hdr_enabled)
    });
    println!("AFTER: {}", serde_json::to_string_pretty(&after).unwrap_or_default());
    if all_restored {
        let _ = std::fs::remove_file(marker_path());
        ARMED.store(false, Ordering::SeqCst);
        if rc == 0 {
            println!("ROUNDTRIP OK: all displays toggled and restored, marker deleted");
        } else {
            println!("ROUNDTRIP FAIL: displays restored, but a toggle did not verify (rc=1)");
        }
    } else {
        println!("ROUNDTRIP FAIL: state differs; marker kept for --repair");
        rc = 1;
    }
    std::process::exit(rc);
}

/// Run a child command with a timeout; kills it when the timeout elapses.
fn run_child(cmd: &[String], timeout: Duration) -> i32 {
    println!("running: {cmd:?} (timeout {timeout:?})");
    let mut child = match std::process::Command::new(&cmd[0]).args(&cmd[1..]).spawn() {
        Ok(c) => c,
        Err(e) => {
            println!("command failed to start: {e}");
            return 2;
        }
    };
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                println!("command exit: {st:?}");
                return if st.success() { 0 } else { 2 };
            }
            Ok(None) if t0.elapsed() > timeout => {
                println!("command timed out after {timeout:?}; killing");
                let _ = child.kill();
                let _ = child.wait();
                return 2;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => {
                println!("command wait failed: {e}");
                return 2;
            }
        }
    }
}

/// `--with [hdr=on|off] [hz=N] [--timeout-secs N] -- <command> [args]`: apply state to the first HDR-capable (or
/// only) display, run the command, then restore and verify. The marker is written before any change.
fn with_state(rest: &[String]) -> i32 {
    let sep = rest.iter().position(|a| a == "--").unwrap_or(rest.len());
    let (opts, cmd) = (&rest[..sep], rest.get(sep + 1..).unwrap_or(&[]));
    let want_hdr = opts.iter().find_map(|o| o.strip_prefix("hdr=")).map(|v| v == "on");
    let want_hz = opts.iter().find_map(|o| o.strip_prefix("hz=")).and_then(|v| v.parse::<u32>().ok());
    let timeout = opts
        .iter()
        .position(|a| a == "--timeout-secs")
        .and_then(|i| opts.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(900);
    if marker_path().exists() && repair() != 0 {
        return 1;
    }
    let ds = display::displays();
    let Some(d) = ds.iter().find(|d| d.hdr_supported).or(ds.first()).cloned() else {
        println!("no display");
        return 1;
    };
    println!("WITH target {} ({}): before hdr={} refresh={:.3} Hz", d.target_id, d.gdi_name, d.hdr_enabled, d.refresh_hz);
    let hdr_change = want_hdr.is_some_and(|on| on != d.hdr_enabled);
    if let Err(e) = write_marker(vec![entry_for(&d, hdr_change, want_hz.is_some())]) {
        println!("cannot write marker: {e}");
        return 1;
    }
    let mut rc = 0;
    if let Some(hz) = want_hz {
        let r = display::set_refresh_hz(&d.gdi_name, hz);
        std::thread::sleep(Duration::from_millis(1500));
        let now = state(d.adapter_luid, d.target_id).map(|s| s.refresh_hz);
        println!("set refresh {hz} Hz -> {r:?}; now {now:?}");
        if r.is_err() {
            rc = 1;
        }
    }
    if let Some(on) = want_hdr
        && hdr_change
    {
        let r = display::set_hdr(d.adapter_luid, d.target_id, on);
        let (ok, dt, _) = wait_for(d.adapter_luid, d.target_id, on, Duration::from_secs(10));
        println!("set hdr={on} -> {r:?}; verified={ok} after {dt:?}");
        if !ok {
            rc = 1;
        }
        std::thread::sleep(Duration::from_millis(1500)); // let DXGI luminance settle
    }
    if rc == 0 && !cmd.is_empty() {
        rc = run_child(cmd, Duration::from_secs(timeout));
    }
    // Restore (HDR first, then refresh, ADR-0006 order) and verify through the same path as crash repair.
    let repaired = repair();
    let after = state(d.adapter_luid, d.target_id);
    let restored = repaired == 0
        && after.as_ref().is_some_and(|a| a.hdr_enabled == d.hdr_enabled && (a.refresh_hz - d.refresh_hz).abs() < 0.01);
    println!(
        "WITH after: hdr={:?} refresh={:?} -> {}",
        after.as_ref().map(|a| a.hdr_enabled),
        after.as_ref().map(|a| a.refresh_hz),
        if restored { "RESTORED" } else { "NOT RESTORED (marker kept)" }
    );
    if !restored {
        rc = 1;
    }
    rc
}
