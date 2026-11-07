//! Applying the display policy for one playback (ADR-0006): switch HDR and/or the refresh rate of the monitor the
//! player is on, record the original state in the restore marker *before* each change, and restore it afterwards.
//! The marker file is deleted only after a verified restore; otherwise startup repair finishes the job.
use crate::marker::{self, MarkerEntry, RestoreMarker, RestoreReport};
use crate::policy::{self, Content, HdrDecision, HdrMode, MonitorPrefs, RefreshPlan, RefreshStrategy};
use crate::{DisplayController, DisplayError, DisplayInfo, Rational};
use std::path::PathBuf;
use std::time::Duration;

/// Timing knobs (tests use zero).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How long to wait for HDR to read back on (DisplayConfig + DXGI colour space).
    pub hdr_timeout: Duration,
    /// Settle time before re-reading DXGI luminance after an HDR toggle (unstable for ~1.5 s, Phase 2 spike).
    pub settle: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            hdr_timeout: Duration::from_secs(10),
            settle: Duration::from_millis(1500),
        }
    }
}

/// Settings that drive the decisions.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub hdr: HdrMode,
    pub refresh: RefreshStrategy,
    pub prefs: MonitorPrefs,
}

/// What was decided and applied for one display.
#[derive(Debug, Clone)]
pub struct Applied {
    /// The display after the changes (fresh query).
    pub display: DisplayInfo,
    pub hdr: HdrDecision,
    /// HDR output is active (PQ swapchain).
    pub hdr_output: bool,
    pub plan: RefreshPlan,
    /// Granary switched HDR on for this playback.
    pub hdr_switched: bool,
    /// Granary switched the refresh rate for this playback.
    pub refresh_switched: bool,
    /// Per-monitor mpv options (tone-mapping target, SDR white, video sync, display fps).
    pub options: Vec<(String, String)>,
    /// Human-readable notes for the log (failed switches, fallbacks).
    pub notes: Vec<String>,
}

pub struct DisplaySession {
    marker_path: PathBuf,
    marker: RestoreMarker,
    timing: Timing,
}

impl DisplaySession {
    pub fn new(marker_path: PathBuf, timing: Timing) -> Self {
        Self {
            marker_path,
            marker: RestoreMarker::for_current_process(),
            timing,
        }
    }

    /// Displays this session changed and has not restored yet.
    pub fn changed(&self) -> &[MarkerEntry] {
        &self.marker.entries
    }

    fn record(&mut self, d: &DisplayInfo, hdr: bool, refresh: bool) -> Result<(), DisplayError> {
        self.marker.record(MarkerEntry {
            id: d.id.clone(),
            gdi_name: d.gdi_name.clone(),
            monitor_path: d.monitor_path.clone(),
            original_hdr: d.hdr_enabled,
            hdr_changed: hdr,
            original_refresh: d.refresh,
            refresh_changed: refresh,
        });
        // Written (and synced) before the change itself, so a crash in between is repaired at the next start.
        self.marker.save(&self.marker_path)
    }

    /// Decide and apply for the display named `gdi_name` (the monitor holding the player window).
    pub fn prepare(
        &mut self,
        ctrl: &dyn DisplayController,
        gdi_name: &str,
        content: &Content,
        policy: &Policy,
    ) -> Result<Applied, DisplayError> {
        let d = ctrl
            .displays()?
            .into_iter()
            .find(|d| d.gdi_name.eq_ignore_ascii_case(gdi_name))
            .ok_or_else(|| DisplayError::Api(format!("no active display named {gdi_name}")))?;
        let mut notes = Vec::new();
        let mut hdr = policy::hdr_decision(&d, policy.hdr, content.range);
        let mut hdr_switched = false;
        if hdr == HdrDecision::Enable {
            self.record(&d, true, false)?;
            let on = ctrl
                .set_hdr(&d.id, true)
                .and_then(|()| marker::wait_for_hdr(ctrl, &d.id, true, self.timing.hdr_timeout));
            match on {
                Ok(true) => hdr_switched = true,
                Ok(false) | Err(_) => {
                    let why = match on {
                        Err(e) => e.to_string(),
                        _ => "HDR did not read back on in time".into(),
                    };
                    notes.push(format!("HDR switch failed ({why}); playing tone-mapped SDR"));
                    // Undo a half-applied toggle now; the marker still covers a crash.
                    let _ = ctrl.set_hdr(&d.id, false);
                    hdr = HdrDecision::StaySdr(policy::SdrReason::Unsupported);
                }
            }
            if hdr_switched && !self.timing.settle.is_zero() {
                std::thread::sleep(self.timing.settle);
            }
        }
        let modes = match ctrl.modes(&d) {
            Ok(m) => m,
            Err(e) => {
                notes.push(format!("display modes unavailable: {e}"));
                Vec::new()
            }
        };
        let strategy = policy.prefs.refresh.unwrap_or(policy.refresh);
        let mut plan = policy::refresh_plan(d.refresh, &modes, content, strategy, policy.prefs.vrr);
        let mut refresh_switched = false;
        if let Some(to) = plan.switch_to {
            self.record(&d, false, true)?;
            match ctrl.set_refresh(&d.id, &d.gdi_name, to) {
                Ok(()) => refresh_switched = true,
                Err(e) => {
                    notes.push(format!("refresh switch to {:.3} Hz failed: {e}", to.hz()));
                    plan = RefreshPlan {
                        switch_to: None,
                        video_sync: if content.passthrough {
                            "audio"
                        } else {
                            "display-resample"
                        },
                        max_factor: policy::max_factor(d.refresh.hz(), content.fps.unwrap_or(0.0)),
                        reason: policy::RefreshReason::NoExactMode,
                    };
                }
            }
        }
        let after = ctrl.display(&d.id).unwrap_or(d);
        let hdr_output = hdr.hdr_output() && after.hdr_enabled;
        let mut options = policy::mpv_options(&after, &policy.prefs, hdr_output, &plan);
        // With `wid` embedding mpv may not notice a mode change; tell it the refresh in use (R4 §8.4).
        options.push(("display-fps-override".into(), format_hz(after.refresh)));
        Ok(Applied {
            display: after,
            hdr,
            hdr_output,
            plan,
            hdr_switched,
            refresh_switched,
            options,
            notes,
        })
    }

    /// Restore everything this session changed. The marker file is removed only when every display verified;
    /// otherwise it stays for startup repair. Returns `None` when nothing was changed.
    pub fn restore(&mut self, ctrl: &dyn DisplayController) -> Option<RestoreReport> {
        if self.marker.entries.is_empty() {
            return None;
        }
        let report = marker::restore(ctrl, &self.marker, self.timing.hdr_timeout);
        if report.all_ok() {
            if let Err(e) = RestoreMarker::remove(&self.marker_path) {
                tracing::warn!(error = %e, "display restore marker could not be removed");
            }
            self.marker.entries.clear();
        } else {
            tracing::warn!("display restore incomplete; marker kept for startup repair");
            // Keep only the entries that failed, so a retry does not touch restored displays again.
            let failed: Vec<_> = report
                .outcomes
                .iter()
                .filter(|o| !o.ok())
                .map(|o| o.id.clone())
                .collect();
            self.marker.entries.retain(|e| failed.contains(&e.id));
            let _ = self.marker.save(&self.marker_path);
        }
        Some(report)
    }
}

/// `display-fps-override` value with enough precision for NTSC rates (119.880120).
pub fn format_hz(r: Rational) -> String {
    format!("{:.6}", r.hz())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockDisplayController;
    use crate::policy::{DynamicRange, RefreshReason};

    fn path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("granary-session-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join(marker::FILE_NAME)
    }

    fn zero() -> Timing {
        Timing {
            hdr_timeout: Duration::ZERO,
            settle: Duration::ZERO,
        }
    }

    fn hdr10(fps: f64) -> Content {
        Content {
            range: DynamicRange::Hdr10,
            fps: Some(fps),
            passthrough: false,
        }
    }

    fn opt<'a>(a: &'a Applied, k: &str) -> Option<&'a str> {
        a.options.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    const GDI: &str = r"\\.\DISPLAY1";

    #[test]
    fn hdr_already_on_is_left_alone_and_nothing_is_recorded() -> Result<(), DisplayError> {
        let m = MockDisplayController::single(true, Rational::new(240, 1));
        let p = path("on");
        let mut s = DisplaySession::new(p.clone(), zero());
        let a = s.prepare(&m, GDI, &hdr10(24.0), &Policy::default())?;
        assert_eq!(a.hdr, HdrDecision::AlreadyOn);
        assert!(a.hdr_output && !a.hdr_switched && !a.refresh_switched);
        assert_eq!(opt(&a, "target-peak"), Some("1000"));
        assert_eq!(opt(&a, "hdr-reference-white"), Some("200"));
        assert!(s.changed().is_empty());
        assert!(!p.exists());
        assert!(s.restore(&m).is_none());
        Ok(())
    }

    #[test]
    fn hdr_is_enabled_for_hdr_content_and_restored_afterwards() -> Result<(), DisplayError> {
        let m = MockDisplayController::single(false, Rational::new(120, 1));
        let p = path("enable");
        let mut s = DisplaySession::new(p.clone(), zero());
        let a = s.prepare(&m, GDI, &hdr10(24.0), &Policy::default())?;
        assert!(a.hdr_switched && a.hdr_output);
        assert!(m.display(&m.id())?.hdr_enabled);
        // The marker was written before the change and still exists while playing.
        let saved = RestoreMarker::load(&p)?.ok_or(DisplayError::Api("no marker".into()))?;
        assert!(saved.entries[0].hdr_changed && !saved.entries[0].original_hdr);
        let report = s.restore(&m).ok_or(DisplayError::Api("no report".into()))?;
        assert!(report.all_ok());
        assert!(!m.display(&m.id())?.hdr_enabled);
        assert!(!p.exists());
        Ok(())
    }

    #[test]
    fn sdr_content_with_content_only_does_not_toggle() -> Result<(), DisplayError> {
        let m = MockDisplayController::single(false, Rational::new(120, 1));
        let mut s = DisplaySession::new(path("sdr"), zero());
        let c = Content {
            range: DynamicRange::Sdr,
            ..hdr10(24.0)
        };
        let a = s.prepare(&m, GDI, &c, &Policy::default())?;
        assert!(!a.hdr_output && !a.hdr_switched);
        assert_eq!(opt(&a, "target-peak"), Some("auto"));
        assert!(!m.display(&m.id())?.hdr_enabled);
        Ok(())
    }

    #[test]
    fn failed_hdr_switch_falls_back_to_sdr_and_keeps_playing() -> Result<(), DisplayError> {
        let m = MockDisplayController::single(false, Rational::new(120, 1));
        m.fail_set_hdr(true);
        let mut s = DisplaySession::new(path("hdrfail"), zero());
        let a = s.prepare(&m, GDI, &hdr10(24.0), &Policy::default())?;
        assert!(!a.hdr_output && !a.hdr_switched);
        assert!(a.notes.iter().any(|n| n.contains("HDR switch failed")));
        Ok(())
    }

    #[test]
    fn exact_multiple_refresh_switch_is_recorded_and_restored() -> Result<(), DisplayError> {
        let m = MockDisplayController::single(true, Rational::new(165, 1));
        m.set_modes(vec![Rational::new(60, 1), Rational::new(150, 1), Rational::new(165, 1)]);
        let p = path("refresh");
        let mut s = DisplaySession::new(p.clone(), zero());
        let a = s.prepare(&m, GDI, &hdr10(25.0), &Policy::default())?;
        assert_eq!(a.plan.reason, RefreshReason::ExactMultiple);
        assert!(a.refresh_switched);
        assert_eq!(m.display(&m.id())?.refresh, Rational::new(150, 1));
        assert_eq!(opt(&a, "display-fps-override"), Some("150.000000"));
        assert_eq!(opt(&a, "video-sync"), Some("display-resample"));
        let report = s.restore(&m).ok_or(DisplayError::Api("no report".into()))?;
        assert!(report.all_ok());
        assert_eq!(m.display(&m.id())?.refresh, Rational::new(165, 1));
        assert!(!p.exists());
        Ok(())
    }

    #[test]
    fn hdr_and_refresh_changes_restore_to_the_state_before_either() -> Result<(), DisplayError> {
        let m = MockDisplayController::single(false, Rational::new(60, 1));
        m.set_modes(vec![Rational::new(24_000, 1001), Rational::new(60, 1)]);
        let mut s = DisplaySession::new(path("both"), zero());
        let policy = Policy {
            refresh: RefreshStrategy::MatchContent,
            ..Policy::default()
        };
        let a = s.prepare(&m, GDI, &hdr10(24_000.0 / 1001.0), &policy)?;
        assert!(a.hdr_switched && a.refresh_switched);
        assert_eq!(opt(&a, "video-sync"), Some("audio"));
        assert_eq!(s.changed().len(), 1);
        assert!(s.restore(&m).is_some_and(|r| r.all_ok()));
        let d = m.display(&m.id())?;
        assert!(!d.hdr_enabled);
        assert_eq!(d.refresh, Rational::new(60, 1));
        Ok(())
    }

    #[test]
    fn rejected_refresh_switch_falls_back_to_resampling() -> Result<(), DisplayError> {
        let m = MockDisplayController::single(true, Rational::new(165, 1));
        m.set_modes(vec![Rational::new(150, 1)]);
        m.fail_set_refresh(true);
        let mut s = DisplaySession::new(path("reject"), zero());
        let a = s.prepare(&m, GDI, &hdr10(25.0), &Policy::default())?;
        assert!(!a.refresh_switched);
        assert_eq!(a.plan.switch_to, None);
        assert_eq!(opt(&a, "display-fps-override"), Some("165.000000"));
        assert!(a.notes.iter().any(|n| n.contains("refresh switch")));
        Ok(())
    }

    #[test]
    fn unknown_monitor_is_an_error() {
        let m = MockDisplayController::single(true, Rational::new(60, 1));
        let mut s = DisplaySession::new(path("unknown"), zero());
        assert!(
            s.prepare(&m, r"\\.\DISPLAY9", &hdr10(24.0), &Policy::default())
                .is_err()
        );
    }
}
