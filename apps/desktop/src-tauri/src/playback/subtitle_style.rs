//! The profile's subtitle appearance (`playback.subtitle_appearance`, `silo-server/contracts/settings/v1/schemas/
//! subtitle-appearance.json@749db96a`) as mpv subtitle options. The server stores a sparse JSON object; every member
//! that is missing or invalid falls back to the contract's default (`contracts/settings/v1/manifest.json@749db96a`).
//! The options style text subtitles (SRT, WebVTT, …); ASS keeps its own styles, as mpv's `sub-ass-override=scale`
//! leaves them, so typeset anime subtitles look as authored.
use serde::Deserialize;

/// One appearance; `None` members take the contract default.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubtitleAppearance {
    pub font_size: Option<String>,
    pub font_family: Option<String>,
    pub font_color: Option<String>,
    pub background_color: Option<String>,
    pub background_style: Option<String>,
    pub background_opacity: Option<i64>,
    pub text_opacity: Option<i64>,
    pub text_outline: Option<bool>,
    pub text_outline_color: Option<String>,
    pub position: Option<String>,
}

impl SubtitleAppearance {
    /// Parse a stored value; anything that is not an object of the known members reads as the default.
    pub fn parse(json: &str) -> Self {
        serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .filter(serde_json::Value::is_object)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }
}

/// `#RRGGBB` (the contract's `hexColor`) or nothing.
fn hex(c: Option<&str>) -> Option<&str> {
    c.filter(|c| c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit()))
}

/// mpv's `#AARRGGBB` from `#RRGGBB` and an opacity in percent.
fn with_alpha(rgb: &str, opacity: i64) -> String {
    let alpha = (opacity.clamp(0, 100) * 255 + 50) / 100;
    format!("#{alpha:02X}{}", rgb[1..].to_ascii_uppercase())
}

/// A family name mpv can look up through DirectWrite: the web's generic families become Windows fonts; a real name
/// passes when it is plain text (the contract's pattern without quotes, separators or control characters).
fn font(family: Option<&str>) -> String {
    match family.map(str::trim) {
        None | Some("" | "sans-serif" | "system-ui") => "Segoe UI".into(),
        Some("serif") => "Georgia".into(),
        Some("monospace") => "Consolas".into(),
        Some(f) if f.len() <= 64 && !f.chars().any(|c| c.is_control() || "\"'(){};:,\\/".contains(c)) => f.into(),
        Some(_) => "Segoe UI".into(),
    }
}

/// The mpv options for `a`; `scale` is Granary's own subtitle size (`sub-scale`, Settings).
pub fn mpv_options(a: &SubtitleAppearance, scale: f64) -> Vec<(String, String)> {
    // The web's cue sizes at its 720-line reference frame (`web/src/lib/subtitleAppearance.ts@749db96a`); mpv's
    // `sub-font-size` is measured against a 720-line window too.
    let size = match a.font_size.as_deref() {
        Some("small") => 20,
        Some("medium") => 26,
        Some("xlarge") => 40,
        Some("xxlarge") => 48,
        _ => 32,
    };
    let fg = hex(a.font_color.as_deref()).unwrap_or("#ffffff");
    let bg = hex(a.background_color.as_deref()).unwrap_or("#000000");
    let outline_color = hex(a.text_outline_color.as_deref()).unwrap_or("#000000");
    let bg_opacity = a.background_opacity.unwrap_or(75);
    let text_opacity = a.text_opacity.unwrap_or(100).max(1);
    let outline = a.text_outline.unwrap_or(false);
    let style = a.background_style.as_deref().unwrap_or("box");
    // (border style, border size, border colour, shadow offset, shadow colour, box colour)
    let (border_style, border_size, border_color, shadow, shadow_color, back) = match style {
        "shadow" => (
            "outline-and-shadow",
            if outline { 1.5 } else { 0.0 },
            with_alpha(outline_color, 100),
            2.0,
            with_alpha(bg, bg_opacity),
            with_alpha(bg, 0),
        ),
        "outline" => (
            "outline-and-shadow",
            3.0,
            with_alpha(if outline { outline_color } else { bg }, bg_opacity.max(1)),
            0.0,
            with_alpha(bg, 0),
            with_alpha(bg, 0),
        ),
        "none" => (
            "outline-and-shadow",
            if outline { 1.5 } else { 0.0 },
            with_alpha(outline_color, 100),
            0.0,
            with_alpha(bg, 0),
            with_alpha(bg, 0),
        ),
        // "box", and anything unknown: one box behind the cue, as the web draws it.
        _ => (
            "background-box",
            if outline { 1.5 } else { 0.0 },
            with_alpha(outline_color, 100),
            0.0,
            with_alpha(bg, 0),
            with_alpha(bg, bg_opacity),
        ),
    };
    let (align, pos) = match a.position.as_deref() {
        Some("top") => ("top", 0),
        Some("lower-third") => ("bottom", 88),
        _ => ("bottom", 100),
    };
    let scale = if scale.is_finite() { scale.clamp(0.3, 3.0) } else { 1.0 };
    [
        ("sub-font", font(a.font_family.as_deref())),
        ("sub-font-size", size.to_string()),
        ("sub-color", with_alpha(fg, text_opacity)),
        ("sub-border-style", border_style.to_owned()),
        ("sub-outline-size", format!("{border_size}")),
        ("sub-outline-color", border_color),
        ("sub-shadow-offset", format!("{shadow}")),
        ("sub-shadow-color", shadow_color),
        ("sub-back-color", back),
        ("sub-align-y", align.to_owned()),
        ("sub-pos", pos.to_string()),
        ("sub-scale", format!("{scale}")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn get<'a>(o: &'a [(String, String)], k: &str) -> &'a str {
        o.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()).unwrap()
    }

    #[test]
    fn the_default_is_a_75_percent_box_in_white() {
        let o = mpv_options(&SubtitleAppearance::parse(""), 1.0);
        assert_eq!(get(&o, "sub-font-size"), "32");
        assert_eq!(get(&o, "sub-color"), "#FFFFFFFF");
        assert_eq!(get(&o, "sub-border-style"), "background-box");
        assert_eq!(get(&o, "sub-back-color"), "#BF000000");
        assert_eq!((get(&o, "sub-align-y"), get(&o, "sub-pos")), ("bottom", "100"));
        assert_eq!(get(&o, "sub-font"), "Segoe UI");
    }

    #[test]
    fn stored_members_override_and_bad_ones_fall_back() {
        let a = SubtitleAppearance::parse(
            r##"{"fontSize":"xxlarge","fontColor":"#ffff00","backgroundStyle":"outline","textOpacity":50,
                "position":"top","fontFamily":"Comic\"Sans","backgroundColor":"red"}"##,
        );
        let o = mpv_options(&a, 1.2);
        assert_eq!(get(&o, "sub-font-size"), "48");
        assert_eq!(get(&o, "sub-color"), "#80FFFF00");
        assert_eq!(get(&o, "sub-border-style"), "outline-and-shadow");
        assert_eq!(get(&o, "sub-outline-size"), "3");
        assert_eq!(
            get(&o, "sub-outline-color"),
            "#BF000000",
            "an invalid background colour is black"
        );
        assert_eq!((get(&o, "sub-align-y"), get(&o, "sub-pos")), ("top", "0"));
        assert_eq!(get(&o, "sub-font"), "Segoe UI", "a family with a quote is refused");
        assert_eq!(get(&o, "sub-scale"), "1.2");
    }

    #[test]
    fn a_value_that_is_not_an_object_is_the_default() {
        assert_eq!(SubtitleAppearance::parse("[1]"), SubtitleAppearance::default());
        assert_eq!(
            SubtitleAppearance::parse(r#"{"fontSize":7}"#),
            SubtitleAppearance::default()
        );
    }
}
