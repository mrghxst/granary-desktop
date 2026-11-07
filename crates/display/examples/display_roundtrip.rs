//! Gate 7 hardware round trip (ADR-0006): for every active display, apply the Phase 7 policy for HDR content (HDR
//! switched on if it is off) plus a refresh switch to another available rate, verify, then restore through the
//! session/marker path and verify that every display is back to its original HDR state and refresh rate.
//!
//! `cargo run -p granary-display --example display_roundtrip -- <marker-dir>`; exit 0 = every display restored (or
//! a capability skip, logged), 1 = a restore did not verify, 2 = setup error.
//! Windows only. Changes are temporary; a crash leaves the marker, which the app repairs at its next start.
#[cfg(windows)]
fn main() {
    std::process::exit(run());
}

#[cfg(not(windows))]
fn main() {
    println!("SKIP: display round trip needs Windows");
}

#[cfg(windows)]
fn run() -> i32 {
    use granary_display::policy::{Content, DynamicRange, HdrMode, MonitorPrefs, RefreshStrategy};
    use granary_display::session::{DisplaySession, Policy, Timing};
    use granary_display::win::WindowsDisplayController;
    use granary_display::{DisplayController, Rational};

    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let marker = dir.join("display-roundtrip-restore.json");
    let ctrl = WindowsDisplayController;
    let before = match ctrl.displays() {
        Ok(d) => d,
        Err(e) => {
            println!("ERROR: cannot query displays: {e}");
            return 2;
        }
    };
    let mut failed = false;
    for d in &before {
        let rates = WindowsDisplayController::refresh_rates(&d.gdi_name);
        println!(
            "display {} '{}' {}: hdr_supported={} hdr_enabled={} refresh={:.3} Hz sdr_white={:.0} nits peak={:.0} nits modes=[{}]",
            d.id,
            d.friendly_name,
            d.gdi_name,
            d.hdr_supported,
            d.hdr_enabled,
            d.refresh.hz(),
            d.sdr_white_nits,
            d.max_luminance,
            rates
                .iter()
                .map(|r| format!("{:.3}", r.hz()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        // Content whose rate is exactly another available refresh, so "match content" must switch to it; prefer a
        // fractional (NTSC) rate to exercise the rational SetDisplayConfig path.
        let target: Option<Rational> = rates
            .iter()
            .copied()
            .filter(|r| (r.hz() - d.refresh.hz()).abs() > 0.5)
            .max_by_key(|r| (r.denominator != 1, r.numerator / r.denominator.max(1)));
        if target.is_none() {
            println!("SKIP refresh: no other refresh rate at the current resolution");
        }
        if !d.hdr_supported {
            println!("SKIP hdr: display does not support HDR");
        }
        if target.is_none() && (!d.hdr_supported || d.hdr_enabled) {
            println!("SKIP display: nothing to switch");
            continue;
        }
        let content = Content {
            range: DynamicRange::Hdr10,
            fps: target.map(Rational::hz),
            passthrough: false,
        };
        let policy = Policy {
            hdr: HdrMode::ContentOnly,
            refresh: RefreshStrategy::MatchContent,
            prefs: MonitorPrefs::default(),
        };
        let mut session = DisplaySession::new(marker.clone(), Timing::default());
        let started = std::time::Instant::now();
        match session.prepare(&ctrl, &d.gdi_name, &content, &policy) {
            Ok(a) => {
                println!(
                    "APPLIED in {} ms: hdr={:?} hdr_switched={} hdr_output={} refresh_switched={} now {:.3} Hz, peak={:.0} nits, notes={:?}",
                    started.elapsed().as_millis(),
                    a.hdr,
                    a.hdr_switched,
                    a.hdr_output,
                    a.refresh_switched,
                    a.display.refresh.hz(),
                    a.display.max_luminance,
                    a.notes
                );
                println!("mpv options: {:?}", a.options);
                println!("marker present while applied: {}", marker.exists());
            }
            Err(e) => println!("ERROR applying: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        let started = std::time::Instant::now();
        match session.restore(&ctrl) {
            Some(r) => println!(
                "RESTORE in {} ms: all_ok={} {:?}",
                started.elapsed().as_millis(),
                r.all_ok(),
                r.outcomes
            ),
            None => println!("RESTORE: nothing was changed"),
        }
        let now = match ctrl.display(&d.id) {
            Ok(n) => n,
            Err(e) => {
                println!("FAIL: display gone after restore: {e}");
                failed = true;
                continue;
            }
        };
        let ok = now.hdr_enabled == d.hdr_enabled && (now.refresh.hz() - d.refresh.hz()).abs() < 0.01;
        println!(
            "{} {}: hdr {} -> {}, refresh {:.3} -> {:.3} Hz, marker removed: {}",
            if ok { "RESTORED" } else { "FAIL" },
            d.gdi_name,
            d.hdr_enabled,
            now.hdr_enabled,
            d.refresh.hz(),
            now.refresh.hz(),
            !marker.exists()
        );
        failed |= !ok || marker.exists();
    }
    if failed {
        println!("DISPLAY ROUNDTRIP FAILED");
        1
    } else {
        println!("DISPLAY ROUNDTRIP OK");
        0
    }
}
