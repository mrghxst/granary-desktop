# Manual test checklist

Checks that need human eyes or hardware not available to the automated build (this machine: one HDR-capable internal
laptop panel, 60/120 Hz, no VRR, no second monitor, no AV receiver). Fill in date/result per item.

## OLED desktop (primary target)
- [ ] Windows HDR Calibration profile: with a calibration profile applied, compare Granary's logged DXGI
      `MaxLuminance`/`MinLuminance` with the profile values; if they differ, set the per-monitor peak override and
      re-check highlight detail. (ADR-0003/0006, review phase-1 L14)
- [ ] Window straddling an SDR and an HDR monitor: drag the player window half-way; video must not flicker or show
      wrong colours; when released fully on one monitor, the logged display parameters must switch. (L14)

(Extended in Phase 7.)
