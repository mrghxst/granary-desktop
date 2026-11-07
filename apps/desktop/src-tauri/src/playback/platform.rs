//! Windows integration for playback: display keep-awake, System Media Transport Controls, the D3D11 hardware decoder
//! probe behind the capability declaration, and the OS/output facts sent with every negotiation.
use granary_silo_api::caps::{HardwareDecoders, OutputFacts};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs, SystemMediaTransportControlsTimelineProperties,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::System::Power::{
    POWER_REQUEST_TYPE, PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired, PowerRequestSystemRequired,
    PowerSetRequest,
};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::{POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0};
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, Interface, PWSTR, w};

/// Keeps the display on while playing (`PowerRequestDisplayRequired`); cleared on pause, stop and drop.
pub struct KeepAwake {
    handle: HANDLE,
    active: bool,
    /// The reason string must outlive the request (PowerCreateRequest keeps a pointer to it).
    _reason: Vec<u16>,
}

// SAFETY: a power request handle is a kernel object handle, usable from any thread.
unsafe impl Send for KeepAwake {}

impl KeepAwake {
    pub fn new() -> Option<Self> {
        let mut reason: Vec<u16> = "Granary is playing video".encode_utf16().chain(Some(0)).collect();
        let ctx = REASON_CONTEXT {
            Version: 0, // POWER_REQUEST_CONTEXT_VERSION
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 {
                SimpleReasonString: PWSTR(reason.as_mut_ptr()),
            },
        };
        // SAFETY: `ctx` points to a NUL-terminated string that lives as long as the request (`_reason`).
        match unsafe { PowerCreateRequest(&ctx) } {
            Ok(handle) => Some(Self {
                handle,
                active: false,
                _reason: reason,
            }),
            Err(e) => {
                tracing::warn!(error = %e, "PowerCreateRequest failed; the display may sleep during playback");
                None
            }
        }
    }

    pub fn set(&mut self, on: bool) {
        if on == self.active {
            return;
        }
        let kinds: [POWER_REQUEST_TYPE; 2] = [PowerRequestDisplayRequired, PowerRequestSystemRequired];
        for k in kinds {
            // SAFETY: live power request handle.
            let r = unsafe {
                if on {
                    PowerSetRequest(self.handle, k)
                } else {
                    PowerClearRequest(self.handle, k)
                }
            };
            if let Err(e) = r {
                tracing::warn!(error = %e, on, "power request not changed");
            }
        }
        self.active = on;
        tracing::debug!(on, "display keep-awake");
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        self.set(false);
        // SAFETY: we own the handle.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// System Media Transport Controls (media keys, the Windows media flyout) for the main window.
pub struct MediaControls {
    smtc: SystemMediaTransportControls,
}

impl MediaControls {
    /// `on_button` receives `play | pause | stop | next | previous`.
    pub fn new(main: isize, on_button: impl Fn(&'static str) + Send + Sync + 'static) -> windows::core::Result<Self> {
        let interop = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        // SAFETY: `main` is our live top-level window handle.
        let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(HWND(main as *mut _))? };
        smtc.SetIsPlayEnabled(true)?;
        smtc.SetIsPauseEnabled(true)?;
        smtc.SetIsStopEnabled(true)?;
        smtc.SetIsNextEnabled(false)?;
        smtc.SetIsPreviousEnabled(false)?;
        smtc.SetIsEnabled(false)?;
        smtc.ButtonPressed(&windows::Foundation::TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_, args| {
            if let Some(a) = args.as_ref() {
                let b = a.Button()?;
                let name = match b {
                    SystemMediaTransportControlsButton::Play => "play",
                    SystemMediaTransportControlsButton::Pause => "pause",
                    SystemMediaTransportControlsButton::Stop => "stop",
                    SystemMediaTransportControlsButton::Next => "next",
                    SystemMediaTransportControlsButton::Previous => "previous",
                    _ => return Ok(()),
                };
                on_button(name);
            }
            Ok(())
        }))?;
        Ok(Self { smtc })
    }

    /// Show the item in the Windows media flyout (or remove it with `None`).
    pub fn set_item(&self, item: Option<(&str, &str)>, has_previous: bool, has_next: bool) {
        let r = (|| -> windows::core::Result<()> {
            let u = self.smtc.DisplayUpdater()?;
            match item {
                Some((title, subtitle)) => {
                    u.SetType(MediaPlaybackType::Video)?;
                    let v = u.VideoProperties()?;
                    v.SetTitle(&HSTRING::from(title))?;
                    v.SetSubtitle(&HSTRING::from(subtitle))?;
                    u.Update()?;
                    self.smtc.SetIsPreviousEnabled(has_previous)?;
                    self.smtc.SetIsNextEnabled(has_next)?;
                    self.smtc.SetIsEnabled(true)?;
                }
                None => {
                    u.ClearAll()?;
                    self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
                    self.smtc.SetIsEnabled(false)?;
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            tracing::warn!(error = %e, "media controls not updated");
        }
    }

    pub fn set_status(&self, playing: bool, position: f64, duration: f64) {
        let r = (|| -> windows::core::Result<()> {
            self.smtc.SetPlaybackStatus(if playing {
                MediaPlaybackStatus::Playing
            } else {
                MediaPlaybackStatus::Paused
            })?;
            if duration > 0.0 {
                let ticks = |s: f64| windows::Foundation::TimeSpan {
                    Duration: (s.max(0.0) * 10_000_000.0) as i64,
                };
                let t = SystemMediaTransportControlsTimelineProperties::new()?;
                t.SetStartTime(ticks(0.0))?;
                t.SetMinSeekTime(ticks(0.0))?;
                t.SetEndTime(ticks(duration))?;
                t.SetMaxSeekTime(ticks(duration))?;
                t.SetPosition(ticks(position.min(duration)))?;
                self.smtc.UpdateTimelineProperties(&t)?;
            }
            Ok(())
        })();
        if let Err(e) = r {
            tracing::debug!(error = %e, "media controls status not updated");
        }
    }
}

/// Hardware decoders of the default adapter, from its D3D11 video decoder profiles (FFmpeg codec names).
pub fn probe_hardware_decoders() -> HardwareDecoders {
    let mut hw = HardwareDecoders::default();
    let mut device: Option<ID3D11Device> = None;
    // SAFETY: plain device creation; the out pointer is a local Option.
    let created = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    };
    let Some(video) = created
        .ok()
        .and(device)
        .and_then(|d| d.cast::<ID3D11VideoDevice>().ok())
    else {
        tracing::warn!("no D3D11 video device; declaring software decoding only");
        return hw;
    };
    let table: &[(windows::core::GUID, &str, bool)] = &[
        (D3D11_DECODER_PROFILE_H264_VLD_NOFGT, "h264", false),
        (D3D11_DECODER_PROFILE_HEVC_VLD_MAIN, "hevc", false),
        (D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10, "hevc", true),
        (D3D11_DECODER_PROFILE_VP9_VLD_PROFILE0, "vp9", false),
        (D3D11_DECODER_PROFILE_VP9_VLD_10BIT_PROFILE2, "vp9", true),
        (D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0, "av1", true),
        (D3D11_DECODER_PROFILE_MPEG2_VLD, "mpeg2video", false),
        (D3D11_DECODER_PROFILE_MPEG2and1_VLD, "mpeg2video", false),
        (D3D11_DECODER_PROFILE_VC1_D2010, "vc1", false),
    ];
    // SAFETY: read-only queries on a live video device.
    let count = unsafe { video.GetVideoDecoderProfileCount() };
    for i in 0..count {
        // SAFETY: `i` is below the reported count.
        let Ok(guid) = (unsafe { video.GetVideoDecoderProfile(i) }) else {
            continue;
        };
        for (g, codec, ten) in table {
            if *g == guid {
                let list = if *ten { &mut hw.ten_bit } else { &mut hw.codecs };
                if !list.iter().any(|c| c == codec) {
                    list.push((*codec).to_owned());
                }
                if *ten && !hw.codecs.iter().any(|c| c == codec) {
                    hw.codecs.push((*codec).to_owned());
                }
            }
        }
    }
    tracing::info!(codecs = ?hw.codecs, ten_bit = ?hw.ten_bit, "hardware decoders");
    hw
}

fn reg_dword(name: windows::core::PCWSTR) -> Option<u32> {
    let mut v = 0u32;
    let mut size = 4u32;
    // SAFETY: the buffer is a local u32 and `size` says so.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    Some(v)
}

fn reg_sz(name: windows::core::PCWSTR) -> Option<String> {
    let mut buf = [0u16; 64];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the buffer is local and `size` is its length in bytes.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// `10.0.26200.7171` style version (major.minor.build.ubr).
pub fn os_version() -> String {
    let major = reg_dword(w!("CurrentMajorVersionNumber")).unwrap_or(10);
    let minor = reg_dword(w!("CurrentMinorVersionNumber")).unwrap_or(0);
    let build = reg_sz(w!("CurrentBuildNumber")).unwrap_or_else(|| "0".into());
    match reg_dword(w!("UBR")) {
        Some(ubr) => format!("{major}.{minor}.{build}.{ubr}"),
        None => format!("{major}.{minor}.{build}"),
    }
}

/// The monitor showing the main window and whether Windows HDR is active on it.
pub fn output_facts() -> OutputFacts {
    use granary_display::DisplayController;
    let Some(device) = super::video::monitor_device() else {
        return OutputFacts::default();
    };
    let hdr = granary_display::win::WindowsDisplayController
        .displays()
        .ok()
        .and_then(|ds| ds.into_iter().find(|d| d.gdi_name == device))
        .map(|d| d.hdr_active());
    OutputFacts {
        display_id: Some(device),
        hdr_active: hdr,
    }
}
