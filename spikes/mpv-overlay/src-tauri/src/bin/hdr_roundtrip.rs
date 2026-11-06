//! Phase 2 spike: toggle Windows HDR on each HDR-capable display and restore it, with a restore marker.
//! Usage: hdr-roundtrip [--repair] [--hold-secs N]
//!   default: for each HDR-capable target: write marker, flip HDR, read back, flip back, read back, delete marker.
//!   --repair: only apply an existing marker (crash recovery path) and exit.
#[path = "../display.rs"]
#[allow(dead_code)]
mod display;

use std::path::PathBuf;
use std::time::{Duration, Instant};

fn marker_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base)
        .join("Granary")
        .join("spike-display-restore.json")
}

fn write_marker(entries: &[(u32, i32, u32, bool)]) -> std::io::Result<()> {
    write_marker_ex(entries, None)
}

fn write_marker_ex(
    entries: &[(u32, i32, u32, bool)],
    refresh_gdi: Option<&str>,
) -> std::io::Result<()> {
    let p = marker_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let body = serde_json::to_string_pretty(
        &entries
            .iter()
            .map(|(lo, hi, t, hdr)| serde_json::json!({"luid_low": lo, "luid_high": hi, "target_id": t, "hdr_enabled": hdr, "restore_refresh_gdi": refresh_gdi}))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &p)
}

fn state(luid: (u32, i32), target: u32) -> Option<display::DisplayInfo> {
    display::displays()
        .into_iter()
        .find(|d| d.adapter_luid == luid && d.target_id == target)
}

/// Wait until the HDR state reads back as `want` (DisplayConfig and DXGI colour space), up to `timeout`.
fn wait_for(
    luid: (u32, i32),
    target: u32,
    want: bool,
    timeout: Duration,
) -> (bool, Duration, Option<display::DisplayInfo>) {
    let t0 = Instant::now();
    loop {
        let s = state(luid, target);
        // DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 = 12, sRGB G22 = 0
        let ok = s
            .as_ref()
            .is_some_and(|d| d.hdr_enabled == want && (d.dxgi_color_space == 12) == want);
        if ok || t0.elapsed() > timeout {
            return (ok, t0.elapsed(), s);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn repair() -> i32 {
    let p = marker_path();
    let Ok(body) = std::fs::read_to_string(&p) else {
        println!("no marker at {}", p.display());
        return 0;
    };
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let mut rc = 0;
    for e in v.as_array().cloned().unwrap_or_default() {
        let luid = (
            e["luid_low"].as_u64().unwrap_or(0) as u32,
            e["luid_high"].as_i64().unwrap_or(0) as i32,
        );
        let target = e["target_id"].as_u64().unwrap_or(0) as u32;
        let want = e["hdr_enabled"].as_bool().unwrap_or(false);
        if let Some(g) = e["restore_refresh_gdi"].as_str() {
            println!(
                "repair refresh {g}: {:?}",
                display::restore_registry_mode(g)
            );
        }
        let cur = state(luid, target).map(|d| d.hdr_enabled);
        if cur != Some(want) {
            let r = display::set_hdr(luid, target, want);
            let (ok, dt, _) = wait_for(luid, target, want, Duration::from_secs(10));
            println!("repair target {target}: set hdr={want} -> {r:?}, verified={ok} after {dt:?}");
            if !ok {
                rc = 1;
            }
        } else {
            println!("repair target {target}: already hdr={want}");
        }
    }
    if rc == 0 {
        let _ = std::fs::remove_file(&p);
        println!("display state repaired; marker deleted");
    }
    rc
}

fn main() {
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
    println!(
        "BEFORE: {}",
        serde_json::to_string_pretty(&ds).unwrap_or_default()
    );
    let capable: Vec<_> = ds.iter().filter(|d| d.hdr_supported).cloned().collect();
    if capable.is_empty() {
        println!("no HDR-capable display; nothing to do");
        return;
    }
    let originals: Vec<_> = capable
        .iter()
        .map(|d| {
            (
                d.adapter_luid.0,
                d.adapter_luid.1,
                d.target_id,
                d.hdr_enabled,
            )
        })
        .collect();
    if let Err(e) = write_marker(&originals) {
        eprintln!("cannot write marker: {e}");
        std::process::exit(1);
    }
    println!("marker written: {}", marker_path().display());
    let mut rc = 0;
    for d in &capable {
        let (luid, t, orig) = (d.adapter_luid, d.target_id, d.hdr_enabled);
        println!(
            "--- {} ({}) target {t}: original hdr={orig}",
            d.friendly_name, d.gdi_name
        );
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
        after.iter().any(|a| {
            a.target_id == c.target_id
                && a.adapter_luid == c.adapter_luid
                && a.hdr_enabled == c.hdr_enabled
        })
    });
    println!(
        "AFTER: {}",
        serde_json::to_string_pretty(&after).unwrap_or_default()
    );
    if all_restored {
        let _ = std::fs::remove_file(marker_path());
        println!("ROUNDTRIP OK: all displays restored, marker deleted");
    } else {
        println!("ROUNDTRIP FAIL: state differs; marker kept for --repair");
        rc = 1;
    }
    std::process::exit(rc);
}

/// `--with [hdr=on|off] [hz=N] -- <command> [args]`: apply state to the first HDR-capable (or only) display, run the
/// command, then restore and verify. The marker is written before any change.
fn with_state(rest: &[String]) -> i32 {
    let sep = rest.iter().position(|a| a == "--").unwrap_or(rest.len());
    let (opts, cmd) = (&rest[..sep], rest.get(sep + 1..).unwrap_or(&[]));
    let want_hdr = opts
        .iter()
        .find_map(|o| o.strip_prefix("hdr="))
        .map(|v| v == "on");
    let want_hz = opts
        .iter()
        .find_map(|o| o.strip_prefix("hz="))
        .and_then(|v| v.parse::<u32>().ok());
    if marker_path().exists() && repair() != 0 {
        return 1;
    }
    let ds = display::displays();
    let Some(d) = ds.iter().find(|d| d.hdr_supported).or(ds.first()).cloned() else {
        println!("no display");
        return 1;
    };
    println!(
        "WITH target {} ({}): before hdr={} refresh={:.3} Hz",
        d.target_id, d.gdi_name, d.hdr_enabled, d.refresh_hz
    );
    let orig = vec![(
        d.adapter_luid.0,
        d.adapter_luid.1,
        d.target_id,
        d.hdr_enabled,
    )];
    if let Err(e) = write_marker_ex(&orig, want_hz.map(|_| d.gdi_name.as_str())) {
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
        && on != d.hdr_enabled
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
        println!("running: {cmd:?}");
        let st = std::process::Command::new(&cmd[0]).args(&cmd[1..]).status();
        println!("command exit: {st:?}");
        if !st.map(|s| s.success()).unwrap_or(false) {
            rc = 2;
        }
    }
    // Restore: HDR first, then refresh (ADR-0006 order), then verify.
    if want_hdr.is_some() {
        let cur = state(d.adapter_luid, d.target_id).map(|s| s.hdr_enabled);
        if cur != Some(d.hdr_enabled) {
            let r = display::set_hdr(d.adapter_luid, d.target_id, d.hdr_enabled);
            let (ok, dt, _) = wait_for(
                d.adapter_luid,
                d.target_id,
                d.hdr_enabled,
                Duration::from_secs(10),
            );
            println!(
                "restore hdr={} -> {r:?}; verified={ok} after {dt:?}",
                d.hdr_enabled
            );
        }
    }
    if want_hz.is_some() {
        println!(
            "restore refresh -> {:?}",
            display::restore_registry_mode(&d.gdi_name)
        );
        std::thread::sleep(Duration::from_millis(1500));
    }
    let after = state(d.adapter_luid, d.target_id);
    let restored = after.as_ref().is_some_and(|a| {
        a.hdr_enabled == d.hdr_enabled && (a.refresh_hz - d.refresh_hz).abs() < 0.01
    });
    println!(
        "WITH after: hdr={:?} refresh={:?} -> {}",
        after.as_ref().map(|a| a.hdr_enabled),
        after.as_ref().map(|a| a.refresh_hz),
        if restored {
            "RESTORED"
        } else {
            "NOT RESTORED (marker kept)"
        }
    );
    if restored {
        let _ = std::fs::remove_file(marker_path());
    } else {
        rc = 1;
    }
    rc
}
