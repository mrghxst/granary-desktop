//! About screen, update check and the log viewer (Phase 8, ADR-0005).
//!
//! - Links open in the default browser through a fixed allow-list ([`Link`]); the UI never passes a URL.
//! - The update check is the ADR-0005 decision: no updater, a "Check for updates" link to the releases page.
//! - Log lines are already redacted when written ([`crate::redact`]); they are redacted again on the way out, so a
//!   file written by an older build cannot leak through the viewer or the export.
use crate::paths;
use crate::redact::redact;
use crate::silo::{CmdResult, CommandError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The repository (AGPL §13 source offer) and its releases page.
pub const SOURCE_URL: &str = "https://github.com/mrghxst/granary-desktop";
pub const RELEASES_URL: &str = "https://github.com/mrghxst/granary-desktop/releases";
pub const LICENSE_URL: &str = "https://www.gnu.org/licenses/agpl-3.0.html";

/// Third-party notices shipped with the build (also installed next to the exe by the installer).
const NOTICES: &str = include_str!("../../../../THIRD_PARTY_NOTICES.md");

/// At most this many lines go to the viewer.
const MAX_VIEW_LINES: usize = 2000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub version: String,
    pub license: &'static str,
    pub source_url: &'static str,
    pub releases_url: &'static str,
    pub notices: &'static str,
    pub log_dir: String,
}

#[tauri::command]
pub fn about_info() -> AboutInfo {
    AboutInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        license: "AGPL-3.0-or-later",
        source_url: SOURCE_URL,
        releases_url: RELEASES_URL,
        notices: NOTICES,
        log_dir: paths::log_dir().display().to_string(),
    }
}

/// The only external places the UI may open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Source,
    Releases,
    License,
    LogFolder,
}

impl Link {
    fn target(self) -> String {
        match self {
            Self::Source => SOURCE_URL.into(),
            Self::Releases => RELEASES_URL.into(),
            Self::License => LICENSE_URL.into(),
            Self::LogFolder => paths::log_dir().display().to_string(),
        }
    }
}

#[tauri::command]
pub fn open_link(link: Link) -> CmdResult<()> {
    let target = link.target();
    if link == Link::LogFolder {
        let _ = std::fs::create_dir_all(&target);
    }
    tracing::info!(?link, "opening external link");
    shell_open(&target).map_err(CommandError::internal)
}

#[cfg(windows)]
fn shell_open(target: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    let target = HSTRING::from(target);
    // SAFETY: all pointers are valid NUL-terminated wide strings for the duration of the call; no window handle.
    let r = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW reports success with a value greater than 32.
    if r.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW failed ({})", r.0 as usize))
    }
}

#[cfg(not(windows))]
fn shell_open(_target: &str) -> Result<(), String> {
    Err("not supported on this platform".into())
}

/// Log files, oldest first (daily rotation: `granary.YYYY-MM-DD.log`).
fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("granary") && n.ends_with(".log"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// The last `max` lines over all log files, redacted.
pub fn tail(dir: &Path, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in log_files(dir).iter().rev() {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let mut lines: Vec<String> = text
            .lines()
            .rev()
            .take(max - out.len())
            .map(|l| redact(l).into_owned())
            .collect();
        lines.reverse();
        lines.extend(out);
        out = lines;
        if out.len() >= max {
            break;
        }
    }
    out
}

#[tauri::command]
pub async fn logs_tail(lines: Option<u32>) -> CmdResult<Vec<String>> {
    let max = lines.map_or(500, |n| n as usize).clamp(1, MAX_VIEW_LINES);
    tauri::async_runtime::spawn_blocking(move || tail(&paths::log_dir(), max))
        .await
        .map_err(|e| CommandError::internal(e.to_string()))
}

/// Write every log file, redacted, into one text file under `dest_dir` and return its path.
pub fn export(dir: &Path, dest_dir: &Path, stamp: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let dest = dest_dir.join(format!("granary-logs-{stamp}.txt"));
    let mut text = format!("Granary {} log export ({stamp})\n", env!("CARGO_PKG_VERSION"));
    for f in log_files(dir) {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_owned();
        text.push_str(&format!("\n===== {name} =====\n"));
        if let Ok(body) = std::fs::read_to_string(&f) {
            for line in body.lines() {
                text.push_str(&redact(line));
                text.push('\n');
            }
        }
    }
    std::fs::write(&dest, text)?;
    Ok(dest)
}

fn downloads_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Downloads"))
        .unwrap_or_else(paths::local_dir)
}

/// Export the redacted logs to the Downloads folder and return the file path.
#[tauri::command]
pub async fn logs_export() -> CmdResult<String> {
    let stamp = chrono_stamp();
    let path = tauri::async_runtime::spawn_blocking(move || export(&paths::log_dir(), &downloads_dir(), &stamp))
        .await
        .map_err(|e| CommandError::internal(e.to_string()))?
        .map_err(|e| CommandError::internal(e.to_string()))?;
    tracing::info!(file = %path.display(), "logs exported");
    Ok(path.display().to_string())
}

/// `YYYYMMDD-HHMMSS` (UTC) without a date crate.
fn chrono_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("granary-about-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tail_spans_files_in_order_and_redacts() {
        let d = temp("tail");
        std::fs::write(d.join("granary.2026-11-06.log"), "a1\na2\n").unwrap();
        std::fs::write(d.join("granary.2026-11-07.log"), "b1\naccess_token=abc123secret\n").unwrap();
        std::fs::write(d.join("other.txt"), "ignored\n").unwrap();
        let t = tail(&d, 3);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0], "a2");
        assert_eq!(t[1], "b1");
        assert!(!t[2].contains("abc123secret"), "{}", t[2]);
        assert_eq!(tail(&d, 10).len(), 4);
    }

    #[test]
    fn export_writes_one_redacted_file() {
        let d = temp("export");
        let out = d.join("out");
        std::fs::write(
            d.join("granary.2026-11-07.log"),
            "x\nAuthorization: Bearer abc.def.ghi\n",
        )
        .unwrap();
        let p = export(&d, &out, "20261107-120000").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(p.ends_with("granary-logs-20261107-120000.txt"));
        assert!(text.contains("===== granary.2026-11-07.log ====="));
        assert!(!text.contains("abc.def.ghi"), "{text}");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_399), (2025, 11, 7));
        assert_eq!(chrono_stamp().len(), 15);
    }

    #[test]
    fn links_are_a_fixed_allow_list() {
        assert!(
            Link::Releases
                .target()
                .starts_with("https://github.com/mrghxst/granary-desktop/")
        );
        assert!(serde_json::from_str::<Link>("\"https://evil.example\"").is_err());
        assert!(NOTICES.contains("libmpv"));
    }
}
