//! Option layering (ADR-0002 L2): forced host options, then the desktop baseline, then settings-driven overrides.
//! Defaults target OLED desktop HDR monitors (Windows HDR often on, high refresh/VRR); TV/receiver features are opt-in.

/// Options the host always forces, regardless of user files.
pub fn forced(wid: isize) -> Vec<(String, String)> {
    [
        ("wid", wid.to_string()),
        ("config", "no".into()),
        ("input-default-bindings", "no".into()),
        ("input-vo-keyboard", "no".into()),
        ("input-cursor", "no".into()),
        ("cursor-autohide", "no".into()),
        ("osc", "no".into()),
        ("terminal", "no".into()),
        // mpv only opens loopback relay URLs: never start yt-dlp from PATH on a failed load, never auto-load scripts
        // (docs/security.md §3). Forced, so a user mpv.conf cannot turn them back on.
        ("ytdl", "no".into()),
        // The host drives the playlist: mpv must stay alive between titles and keep the last frame at the end so the
        // host decides what happens next (up next, back to details). A user mpv.conf cannot change this (phase-9 L1).
        ("idle", "yes".into()),
        ("keep-open", "yes".into()),
        // Keep the video output (black) between titles and while one loads. The main window is transparent for the
        // overlay, so without it the desktop showed through while the next episode or a slow start loaded.
        ("force-window", "yes".into()),
        ("load-scripts", "no".into()),
        // The host owns SMTC and the screensaver policy (Phase 8).
        ("media-controls", "no".into()),
        ("stop-screensaver", "no".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Desktop baseline validated in the Phase 2 spike (docs/spikes/mpv-overlay.md).
pub fn desktop_baseline() -> Vec<(String, String)> {
    [
        ("vo", "gpu-next"),
        ("gpu-api", "d3d11"),
        ("hwdec", "d3d11va"),
        ("d3d11-flip", "yes"),
        ("d3d11-exclusive-fs", "no"),
        ("target-colorspace-hint", "auto"),
        ("target-colorspace-hint-mode", "target"),
        ("hdr-reference-white", "auto"),
        ("tone-mapping", "auto"),
        ("hdr-compute-peak", "auto"),
        ("dither-depth", "auto"),
        ("deband", "yes"),
        ("video-sync", "display-resample"),
        ("audio-channels", "auto-safe"),
        ("cache", "yes"),
        ("demuxer-max-bytes", "768MiB"),
        ("demuxer-max-back-bytes", "128MiB"),
        ("stream-buffer-size", "4MiB"),
        ("cache-pause-wait", "3"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Demuxer cache sizes `(ahead, back)` in MiB for a machine with `total_ram` bytes: a sixteenth of the memory ahead,
/// between 384 MiB (about 40 s of an 80 Mbit/s 4K remux) and 1 GiB, and a sixth of that behind for short seeks back.
/// A fixed 768 MiB took 11 % of an 8 GB HTPC for one stream (docs/research/reference-clients.md §4).
pub fn demuxer_cache_mib(total_ram: u64) -> (u64, u64) {
    let ahead = ((total_ram / 16) >> 20).clamp(384, 1024);
    (ahead, ahead / 6)
}

/// Merge layers: later layers override earlier ones; `forced` always wins.
pub fn merge(forced: Vec<(String, String)>, layers: &[Vec<(String, String)>]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for layer in layers.iter().chain(std::iter::once(&forced)) {
        for (k, v) in layer {
            out.retain(|(n, _)| n != k);
            out.push((k.clone(), v.clone()));
        }
    }
    out
}

/// The bundled defaults file (ADR-0002 L2 (a)), installed next to the executable as `mpv.conf`.
/// [`desktop_baseline`] holds the same options (a test keeps them equal), so a missing file changes nothing.
pub const BUNDLED_CONF: &str = include_str!("../mpv.conf");

/// Parse the `key=value` lines of an mpv config file; blank lines and `#` comment lines are skipped.
pub fn parse_conf(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// The full option list in the order it is applied (ADR-0002 L2):
/// 1. the code baseline;
/// 2. `include=<bundled mpv.conf>`;
/// 3. the settings-driven options;
/// 4. `include=<%APPDATA%\Granary\mpv.conf>`, the optional power-user file;
/// 5. the forced host options.
///
/// mpv parses an `include` as soon as it is set, so each later step overrides the earlier ones and forced options
/// always win.
pub fn layered(
    wid: isize,
    settings: Vec<(String, String)>,
    bundled_conf: Option<&std::path::Path>,
    user_conf: Option<&std::path::Path>,
) -> Vec<(String, String)> {
    let include = |p: &std::path::Path| ("include".to_owned(), p.to_string_lossy().into_owned());
    let mut out: Vec<(String, String)> = desktop_baseline()
        .into_iter()
        .filter(|(k, _)| !settings.iter().any(|(s, _)| s == k))
        .collect();
    out.extend(bundled_conf.map(include));
    out.extend(settings);
    out.extend(user_conf.map(include));
    out.extend(forced(wid));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    fn get<'a>(o: &'a [(String, String)], k: &str) -> Option<&'a str> {
        o.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn demuxer_cache_follows_installed_memory() {
        const GIB: u64 = 1 << 30;
        assert_eq!(demuxer_cache_mib(4 * GIB), (384, 64));
        assert_eq!(demuxer_cache_mib(8 * GIB), (512, 85));
        assert_eq!(demuxer_cache_mib(16 * GIB), (1024, 170));
        assert_eq!(demuxer_cache_mib(64 * GIB), (1024, 170));
        assert_eq!(demuxer_cache_mib(0), (384, 64));
    }

    #[test]
    fn forced_wins_and_overrides_apply() {
        let user = vec![
            ("config".to_string(), "yes".to_string()),
            ("video-sync".to_string(), "audio".to_string()),
        ];
        let o = merge(forced(42), &[desktop_baseline(), user]);
        assert_eq!(get(&o, "config"), Some("no"));
        assert_eq!(get(&o, "wid"), Some("42"));
        assert_eq!(get(&o, "ytdl"), Some("no"));
        assert_eq!(get(&o, "load-scripts"), Some("no"));
        assert_eq!(get(&o, "idle"), Some("yes"));
        assert_eq!(get(&o, "keep-open"), Some("yes"));
        assert_eq!(get(&o, "force-window"), Some("yes"));
        assert_eq!(get(&o, "video-sync"), Some("audio"));
        assert_eq!(get(&o, "vo"), Some("gpu-next"));
        assert_eq!(o.iter().filter(|(k, _)| k == "config").count(), 1);
    }

    #[test]
    fn bundled_conf_matches_the_code_baseline() {
        assert_eq!(parse_conf(BUNDLED_CONF), desktop_baseline());
    }

    #[test]
    fn layering_order_is_baseline_bundled_settings_user_forced() {
        let settings = vec![("hwdec".to_string(), "no".to_string())];
        let o = layered(
            7,
            settings,
            Some(std::path::Path::new("C:/app/mpv.conf")),
            Some(std::path::Path::new("C:/user/mpv.conf")),
        );
        let pos = |k: &str, v: &str| o.iter().position(|(a, b)| a == k && b == v);
        let bundled = pos("include", "C:/app/mpv.conf").unwrap();
        let hwdec = pos("hwdec", "no").unwrap();
        let user = pos("include", "C:/user/mpv.conf").unwrap();
        let wid = pos("wid", "7").unwrap();
        assert!(pos("vo", "gpu-next").unwrap() < bundled);
        assert!(bundled < hwdec && hwdec < user && user < wid);
        // The settings value replaces the baseline value instead of being applied twice.
        assert_eq!(o.iter().filter(|(k, _)| k == "hwdec").count(), 1);
        assert_eq!(o.last().map(|(k, _)| k.as_str()), Some("stop-screensaver"));
    }

    #[test]
    fn missing_files_are_not_included() {
        let o = layered(1, Vec::new(), None, None);
        assert!(o.iter().all(|(k, _)| k != "include"));
    }
}
