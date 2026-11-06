//! Persisted restore marker (ADR-0006): written atomically *before* any display change, applied on stop/exit/panic
//! and at the next startup after a crash, deleted only after the restore was verified.
use crate::process::MarkerOwner;
use crate::{DisplayController, DisplayError, DisplayId, Rational};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

const MARKER_VERSION: u32 = 1;

/// Original state of one display that Granary changed (or is about to change).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerEntry {
    pub id: DisplayId,
    pub gdi_name: String,
    /// Reboot-stable identity used when the adapter LUID in `id` no longer matches (review H2).
    #[serde(default)]
    pub monitor_path: String,
    pub original_hdr: bool,
    pub hdr_changed: bool,
    pub original_refresh: Rational,
    pub refresh_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestoreMarker {
    pub version: u32,
    /// Process that made the changes; startup repair skips the marker while that process still runs (review H1).
    #[serde(default)]
    pub owner: Option<MarkerOwner>,
    pub entries: Vec<MarkerEntry>,
}

impl Default for RestoreMarker {
    fn default() -> Self {
        Self {
            version: MARKER_VERSION,
            owner: None,
            entries: Vec::new(),
        }
    }
}

/// File name of the marker inside the app's local data directory (`%LOCALAPPDATA%\Granary`).
pub const FILE_NAME: &str = "display-restore.json";

impl RestoreMarker {
    /// Empty marker owned by the current process (what the player writes before changing a display).
    pub fn for_current_process() -> Self {
        Self {
            owner: crate::process::current(),
            ..Self::default()
        }
    }

    /// Load the marker; `Ok(None)` if there is none.
    pub fn load(path: &Path) -> Result<Option<Self>, DisplayError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(Some(serde_json::from_str(&s)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Write atomically: temp file in the same directory, then rename over the target.
    pub fn save(&self, path: &Path) -> Result<(), DisplayError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(self)?)?;
            // Flush to disk before the rename so a power loss cannot leave an empty marker (review L7).
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn remove(path: &Path) -> Result<(), DisplayError> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Record an intended change. The first recorded original state of a display wins, so repeated changes
    /// (e.g. HDR now, refresh later) always restore to the state before Granary touched it.
    pub fn record(&mut self, entry: MarkerEntry) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == entry.id) {
            e.hdr_changed |= entry.hdr_changed;
            e.refresh_changed |= entry.refresh_changed;
        } else {
            self.entries.push(entry);
        }
    }
}

/// The current state of the display a marker entry refers to: by id first, then by the stable monitor path.
pub fn resolve(ctrl: &dyn DisplayController, e: &MarkerEntry) -> Result<crate::DisplayInfo, DisplayError> {
    let all = ctrl.displays()?;
    all.iter()
        .find(|d| d.id == e.id)
        .or_else(|| {
            all.iter()
                .find(|d| !e.monitor_path.is_empty() && d.monitor_path == e.monitor_path)
        })
        .cloned()
        .ok_or_else(|| DisplayError::NotFound(e.id.clone()))
}

/// Result of restoring one display.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryOutcome {
    pub id: DisplayId,
    pub hdr_restored: Option<bool>,
    pub refresh_restored: Option<bool>,
    pub error: Option<String>,
}

impl EntryOutcome {
    pub fn ok(&self) -> bool {
        self.error.is_none() && self.hdr_restored != Some(false) && self.refresh_restored != Some(false)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestoreReport {
    pub outcomes: Vec<EntryOutcome>,
}

impl RestoreReport {
    pub fn all_ok(&self) -> bool {
        self.outcomes.iter().all(EntryOutcome::ok)
    }
}

/// Poll until the display's HDR state reads back as `want`, or `timeout` elapses.
pub fn wait_for_hdr(
    ctrl: &dyn DisplayController,
    id: &DisplayId,
    want: bool,
    timeout: Duration,
) -> Result<bool, DisplayError> {
    let start = Instant::now();
    loop {
        let d = ctrl.display(id)?;
        if d.hdr_enabled == want && (d.dxgi_color_space == crate::types::DXGI_COLOR_SPACE_HDR10) == want {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Restore every entry: HDR first, then refresh (ADR-0006), then verify by re-query.
/// A display that has disappeared (unplugged) is reported as an error but does not stop the others.
pub fn restore(ctrl: &dyn DisplayController, marker: &RestoreMarker, timeout: Duration) -> RestoreReport {
    let mut outcomes = Vec::new();
    for e in &marker.entries {
        let mut out = EntryOutcome {
            id: e.id.clone(),
            hdr_restored: None,
            refresh_restored: None,
            error: None,
        };
        let current = match resolve(ctrl, e) {
            Ok(d) => d,
            Err(err) => {
                out.error = Some(err.to_string());
                outcomes.push(out);
                continue;
            }
        };
        if e.hdr_changed {
            let result = if current.hdr_enabled == e.original_hdr {
                Ok(true)
            } else {
                ctrl.set_hdr(&current.id, e.original_hdr)
                    .and_then(|()| wait_for_hdr(ctrl, &current.id, e.original_hdr, timeout))
            };
            match result {
                Ok(v) => out.hdr_restored = Some(v),
                Err(err) => {
                    out.hdr_restored = Some(false);
                    out.error = Some(err.to_string());
                }
            }
        }
        if e.refresh_changed {
            let result = ctrl
                .restore_mode(&current.gdi_name)
                .and_then(|()| ctrl.display(&current.id));
            match result {
                Ok(d) => out.refresh_restored = Some((d.refresh.hz() - e.original_refresh.hz()).abs() < 0.01),
                Err(err) => {
                    out.refresh_restored = Some(false);
                    out.error.get_or_insert(err.to_string());
                }
            }
        }
        tracing::info!(display = %e.id, ok = out.ok(), hdr = ?out.hdr_restored, refresh = ?out.refresh_restored, "display restore");
        outcomes.push(out);
    }
    RestoreReport { outcomes }
}

/// Startup repair: if a marker exists (crash, power loss), restore it before any UI and delete it when verified.
/// Returns the report if a marker was found and repaired; `Ok(None)` if there is no marker or its owner still runs.
pub fn repair_at_startup(
    ctrl: &dyn DisplayController,
    path: &Path,
    timeout: Duration,
) -> Result<Option<RestoreReport>, DisplayError> {
    repair_at_startup_with(ctrl, path, timeout, &crate::process::is_alive)
}

/// [`repair_at_startup`] with an injectable liveness check (tests).
pub fn repair_at_startup_with(
    ctrl: &dyn DisplayController,
    path: &Path,
    timeout: Duration,
    owner_alive: &dyn Fn(&MarkerOwner) -> bool,
) -> Result<Option<RestoreReport>, DisplayError> {
    let Some(marker) = RestoreMarker::load(path)? else {
        return Ok(None);
    };
    if let Some(owner) = marker.owner.filter(|o| owner_alive(o)) {
        tracing::info!(
            pid = owner.pid,
            "display restore marker belongs to a running instance; left alone"
        );
        return Ok(None);
    }
    let report = restore(ctrl, &marker, timeout);
    if report.all_ok() {
        RestoreMarker::remove(path)?;
        tracing::warn!(entries = marker.entries.len(), "display state repaired");
    } else {
        tracing::error!(?report, "display state repair incomplete; marker kept");
    }
    Ok(Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockDisplayController;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("granary-test-{}-{name}", std::process::id()))
            .join("display-restore.json")
    }

    #[test]
    fn save_load_remove_roundtrip() -> Result<(), DisplayError> {
        let p = tmp_path("roundtrip");
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        let mut m = RestoreMarker::default();
        m.record(mock.entry(true, false));
        m.save(&p)?;
        assert_eq!(RestoreMarker::load(&p)?, Some(m));
        RestoreMarker::remove(&p)?;
        assert_eq!(RestoreMarker::load(&p)?, None);
        RestoreMarker::remove(&p)?; // idempotent
        Ok(())
    }

    #[test]
    fn first_original_wins() {
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        let mut m = RestoreMarker::default();
        m.record(mock.entry(true, false));
        let mut later = mock.entry(false, true);
        later.original_hdr = true; // state after Granary enabled HDR must not overwrite the original
        m.record(later);
        assert_eq!(m.entries.len(), 1);
        assert!(!m.entries[0].original_hdr);
        assert!(m.entries[0].hdr_changed && m.entries[0].refresh_changed);
    }

    #[test]
    fn restore_reverts_hdr_and_refresh() -> Result<(), DisplayError> {
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        let mut m = RestoreMarker::default();
        m.record(mock.entry(true, true));
        let id = mock.id();
        mock.set_hdr(&id, true)?;
        mock.set_refresh(&id, "\\\\.\\DISPLAY1", Rational::new(120, 1))?;
        let report = restore(&mock, &m, Duration::from_millis(10));
        assert!(report.all_ok(), "{report:?}");
        let d = mock.display(&id)?;
        assert!(!d.hdr_enabled);
        assert_eq!(d.refresh, Rational::new(60, 1));
        Ok(())
    }

    #[test]
    fn repair_keeps_marker_when_restore_fails() -> Result<(), DisplayError> {
        let p = tmp_path("repair-fail");
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        let mut m = RestoreMarker::default();
        m.record(mock.entry(true, false));
        mock.set_hdr(&mock.id(), true)?;
        mock.fail_set_hdr(true);
        m.save(&p)?;
        let report = repair_at_startup(&mock, &p, Duration::from_millis(10))?;
        assert!(report.is_some_and(|r| !r.all_ok()));
        assert!(
            RestoreMarker::load(&p)?.is_some(),
            "marker must be kept for the next attempt"
        );
        mock.fail_set_hdr(false);
        let report = repair_at_startup(&mock, &p, Duration::from_millis(10))?;
        assert!(report.is_some_and(|r| r.all_ok()));
        assert!(RestoreMarker::load(&p)?.is_none());
        Ok(())
    }

    #[test]
    fn marker_of_running_owner_is_left_alone() -> Result<(), DisplayError> {
        let p = tmp_path("live-owner");
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        let mut m = RestoreMarker {
            owner: Some(MarkerOwner { pid: 4242, started: 7 }),
            ..RestoreMarker::default()
        };
        m.record(mock.entry(true, false));
        mock.set_hdr(&mock.id(), true)?;
        m.save(&p)?;
        let alive = |o: &MarkerOwner| o.pid == 4242;
        assert!(repair_at_startup_with(&mock, &p, Duration::from_millis(10), &alive)?.is_none());
        assert!(
            mock.displays()?[0].hdr_enabled,
            "a running instance's HDR change must not be reverted"
        );
        assert!(RestoreMarker::load(&p)?.is_some(), "marker stays with its owner");
        // Owner gone (crash): the next start repairs.
        let report = repair_at_startup_with(&mock, &p, Duration::from_millis(10), &|_| false)?;
        assert!(report.is_some_and(|r| r.all_ok()));
        assert!(!mock.displays()?[0].hdr_enabled);
        assert!(RestoreMarker::load(&p)?.is_none());
        Ok(())
    }

    #[test]
    fn old_marker_without_owner_still_loads() -> Result<(), DisplayError> {
        let m: RestoreMarker = serde_json::from_str(r#"{"version":1,"entries":[]}"#)?;
        assert_eq!(m.owner, None);
        Ok(())
    }

    #[test]
    fn repair_without_marker_is_noop() -> Result<(), DisplayError> {
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        assert!(repair_at_startup(&mock, &tmp_path("none"), Duration::from_millis(10))?.is_none());
        Ok(())
    }

    #[test]
    fn stale_adapter_luid_is_resolved_by_monitor_path() -> Result<(), DisplayError> {
        // After a reboot the adapter LUID changes; the monitor path still identifies the display.
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        let mut e = mock.entry(true, true);
        e.id.adapter_luid_low = 0xdead;
        let mut m = RestoreMarker::default();
        m.record(e);
        mock.set_hdr(&mock.id(), true)?;
        mock.set_refresh(&mock.id(), "x", Rational::new(120, 1))?;
        let report = restore(&mock, &m, Duration::from_millis(10));
        assert!(report.all_ok(), "{report:?}");
        let d = mock.display(&mock.id())?;
        assert!(!d.hdr_enabled);
        assert_eq!(d.refresh, Rational::new(60, 1));
        Ok(())
    }

    #[test]
    fn corrupt_marker_is_kept() -> Result<(), DisplayError> {
        let p = tmp_path("corrupt");
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, "{not json")?;
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        assert!(repair_at_startup(&mock, &p, Duration::from_millis(10)).is_err());
        assert!(p.exists(), "an unreadable marker must never be deleted");
        RestoreMarker::remove(&p)?;
        Ok(())
    }

    #[test]
    fn unplugged_display_reports_error_but_others_restore() -> Result<(), DisplayError> {
        let mock = MockDisplayController::single(false, Rational::new(60, 1));
        let mut m = RestoreMarker::default();
        let mut ghost = mock.entry(true, false);
        ghost.id.target_id = 999;
        ghost.monitor_path = r"\\?\DISPLAY#GONE#9".into();
        m.record(ghost);
        m.record(mock.entry(true, false));
        mock.set_hdr(&mock.id(), true)?;
        let report = restore(&mock, &m, Duration::from_millis(10));
        assert!(!report.all_ok());
        assert!(report.outcomes[1].ok());
        assert!(!mock.display(&mock.id())?.hdr_enabled);
        Ok(())
    }
}
