// Baseline: standalone mpv window (no WebView), same options as the spike, 30 s steady-state pacing.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
#[repr(C)] struct MpvEvent { event_id: c_int, error: c_int, reply_userdata: u64, data: *mut c_void }
unsafe extern "C" {
    fn mpv_create() -> *mut c_void;
    fn mpv_initialize(h: *mut c_void) -> c_int;
    fn mpv_set_option_string(h: *mut c_void, n: *const c_char, v: *const c_char) -> c_int;
    fn mpv_command(h: *mut c_void, args: *const *const c_char) -> c_int;
    fn mpv_get_property_string(h: *mut c_void, n: *const c_char) -> *mut c_char;
    fn mpv_free(p: *mut c_void);
    fn mpv_wait_event(h: *mut c_void, t: f64) -> *mut MpvEvent;
    fn mpv_terminate_destroy(h: *mut c_void);
}
fn s(x: &str) -> CString { CString::new(x).unwrap() }
unsafe fn prop(h: *mut c_void, n: &str) -> String { unsafe {
    let p = mpv_get_property_string(h, s(n).as_ptr());
    if p.is_null() { return "<unavailable>".into(); }
    let r = CStr::from_ptr(p).to_string_lossy().into_owned(); mpv_free(p as _); r
}}
fn main() { unsafe {
    let file = std::env::args().nth(1).expect("file");
    let extra: Vec<String> = std::env::args().skip(2).collect();
    let h = mpv_create();
    let mut opts: Vec<(String, String)> = [("vo","gpu-next"),("gpu-api","d3d11"),("hwdec","d3d11va"),("d3d11-flip","yes"),("terminal","no"),("osc","no"),
        ("geometry","1100x640+120+90"),("ontop","yes"),("loop-file","inf"),("video-sync","display-resample"),("target-colorspace-hint","auto"),
        ("hdr-reference-white","auto"),("deband","yes"),("config","no")].iter().map(|(a,b)| (a.to_string(), b.to_string())).collect();
    for e in &extra { if let Some((k, v)) = e.split_once('=') { opts.push((k.into(), v.into())); } }
    for (k, v) in &opts { let r = mpv_set_option_string(h, s(k).as_ptr(), s(v).as_ptr()); if r < 0 { println!("opt {k} failed {r}"); } }
    mpv_initialize(h);
    let a = [s("loadfile"), s(&file)]; let ptrs = [a[0].as_ptr(), a[1].as_ptr(), std::ptr::null()];
    mpv_command(h, ptrs.as_ptr());
    let t0 = std::time::Instant::now();
    while t0.elapsed().as_millis() < 4500 { mpv_wait_event(h, 0.1); }
    let keys = ["frame-drop-count", "vo-delayed-frame-count", "mistimed-frame-count"];
    let read = || keys.map(|k| prop(h, k).parse::<i64>().unwrap_or(0));
    let pos = || prop(h, "time-pos").parse::<f64>().unwrap_or(0.0);
    let (mut prev, mut lp) = (read(), pos());
    let (mut sum, mut at_loop, mut loops) = ([0i64; 3], [0i64; 3], 0);
    for _ in 0..120 {
        let t = std::time::Instant::now();
        while t.elapsed().as_millis() < 250 { mpv_wait_event(h, 0.05); }
        let cur = read(); let p = pos();
        let restarted = p < lp || cur.iter().zip(prev.iter()).any(|(c, q)| c < q);
        if restarted { loops += 1; }
        for i in 0..3 { let d = if cur[i] >= prev[i] { cur[i] - prev[i] } else { cur[i] }; if restarted || p < 1.0 { at_loop[i] += d } else { sum[i] += d } }
        prev = cur; lp = p;
    }
    println!("BASELINE file={file} extra={extra:?} steady drops={} vo_delayed={} mistimed={} loops={loops} at_loop={at_loop:?} display-fps={} vf-fps={} hwdec={}",
        sum[0], sum[1], sum[2], prop(h, "display-fps"), prop(h, "estimated-vf-fps"), prop(h, "hwdec-current"));
    mpv_terminate_destroy(h);
}}
