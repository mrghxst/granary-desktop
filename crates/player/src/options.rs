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
        ("keep-open", "yes"),
        ("audio-channels", "auto-safe"),
        ("cache", "yes"),
        ("demuxer-max-bytes", "256MiB"),
        ("demuxer-max-back-bytes", "64MiB"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(o: &'a [(String, String)], k: &str) -> Option<&'a str> {
        o.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
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
        assert_eq!(get(&o, "video-sync"), Some("audio"));
        assert_eq!(get(&o, "vo"), Some("gpu-next"));
        assert_eq!(o.iter().filter(|(k, _)| k == "config").count(), 1);
    }
}
