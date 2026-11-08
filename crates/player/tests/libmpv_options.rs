//! The real libmpv must accept every option Granary sets: `Mpv::new` fails on the first rejected option, which would
//! disable playback entirely. Runs against the pinned DLL from `scripts/fetch-libmpv.ps1`; skipped (with a message)
//! when it has not been fetched.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code

use granary_player::mpv::Mpv;
use granary_player::options::{BUNDLED_CONF, layered, parse_conf};
use granary_player::probe::probe_libmpv;
use std::path::PathBuf;
use std::sync::OnceLock;

/// The probe only searches the exe folder and system folders (not PATH), like the installed app; put the pinned DLL
/// next to the test binary. Done once per process: the tests run in parallel, and once one of them has loaded the DLL
/// it cannot be overwritten (sharing violation).
fn libmpv_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor-bin/libmpv/libmpv-2.dll");
        let exe = std::env::current_exe().unwrap();
        let dst = exe.parent().unwrap().join("libmpv-2.dll");
        if !src.is_file() {
            eprintln!("skipped: {} missing; run scripts/fetch-libmpv.ps1", src.display());
            return false;
        }
        let stale = std::fs::metadata(&dst).map(|d| d.len()).ok() != std::fs::metadata(&src).map(|s| s.len()).ok();
        if stale {
            std::fs::copy(&src, &dst).unwrap();
        }
        probe_libmpv().is_ok()
    })
}

#[test]
fn real_libmpv_accepts_every_option() {
    if !libmpv_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("granary-mpv-options-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bundled = dir.join("mpv.conf");
    std::fs::write(&bundled, BUNDLED_CONF).unwrap();
    // Settings-driven options the host can produce (display, audio, subtitles, cache).
    let settings: Vec<(String, String)> = [
        ("hwdec", "auto-safe"),
        ("target-peak", "1000"),
        ("tone-mapping", "bt.2446a"),
        ("video-sync", "display-resample"),
        ("audio-spdif", "ac3,eac3,dts"),
        ("audio-channels", "7.1,5.1,stereo"),
        ("audio-exclusive", "yes"),
        ("sub-scale", "1.2"),
        ("deband", "yes"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let opts = layered(0, settings, Some(&bundled), None);
    assert!(opts.iter().any(|(k, v)| k == "ytdl" && v == "no"));
    assert!(opts.iter().any(|(k, v)| k == "load-scripts" && v == "no"));
    assert!(parse_conf(BUNDLED_CONF).len() >= 10);
    let mpv = Mpv::new(&opts);
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = mpv {
        panic!("libmpv rejected an option: {e}");
    }
}

/// libmpv only warns about errors inside an included config file, so a broken user `mpv.conf` never stops playback
/// (phase-9 review L2); the host's retry without the user file covers only a failing `mpv_initialize`.
#[test]
fn broken_user_conf_does_not_stop_mpv() {
    if !libmpv_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("granary-mpv-user-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in [
        ("unknown", "no-such-option=1\n"),
        ("badvalue", "hwdec=banana\nvolume=abc\n"),
        ("garbage", "[[[\n=\n"),
    ] {
        let user = dir.join(format!("{name}.conf"));
        std::fs::write(&user, text).unwrap();
        let r = Mpv::new(&layered(0, Vec::new(), None, Some(&user)));
        assert!(r.is_ok(), "{name}: {:?}", r.err().map(|e| e.to_string()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Seek previews (ADR-0009): the headless handle accepts its options and returns JPEGs for SDR and HDR sources.
#[test]
fn preview_frames_from_real_files() {
    use granary_player::thumbs::Thumbnailer;
    use std::time::Duration;
    if !libmpv_available() {
        return;
    }
    let media = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-media");
    let dir = std::env::temp_dir().join(format!("granary-previews-{}", std::process::id()));
    for name in ["sdr_h264_1080p.mkv", "hdr10_hevc_main10_2160p.mkv"] {
        let file = media.join(name);
        if !file.is_file() {
            eprintln!("skipped {name}: run scripts/gen-test-media.ps1");
            continue;
        }
        let mut t = Thumbnailer::open(file.to_str().unwrap(), &dir, Duration::from_secs(15)).unwrap();
        assert!(t.indexed(), "{name}: test media has a keyframe index");
        let keys = t.keyframes().unwrap_or_default();
        assert!(
            !keys.is_empty() && keys.windows(2).all(|w| w[0] <= w[1]),
            "{name}: keyframe times in order"
        );
        assert!(t.snap(5.0).is_some_and(|k| k <= 5.0));
        for at in [1.0, 5.0] {
            let jpg = t.frame(at, Duration::from_secs(10)).unwrap();
            assert!(jpg.starts_with(&[0xFF, 0xD8]), "{name} @ {at}: not a JPEG");
            assert!(jpg.len() > 1000, "{name} @ {at}: {} bytes", jpg.len());
        }
        assert!(t.frame(-1.0, Duration::from_secs(1)).is_err());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
