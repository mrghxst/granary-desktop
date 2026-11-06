// With Windows HDR on: render a paused SDR frame with different hdr-reference-white values and screenshot each.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
#[repr(C)] struct MpvEvent { event_id: c_int, error: c_int, reply_userdata: u64, data: *mut c_void }
unsafe extern "C" {
    fn mpv_create() -> *mut c_void;
    fn mpv_initialize(h: *mut c_void) -> c_int;
    fn mpv_set_option_string(h: *mut c_void, n: *const c_char, v: *const c_char) -> c_int;
    fn mpv_set_property_string(h: *mut c_void, n: *const c_char, v: *const c_char) -> c_int;
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
unsafe fn cmd(h: *mut c_void, a: &[&str]) -> c_int { unsafe {
    let o: Vec<CString> = a.iter().map(|x| s(x)).collect();
    let mut p: Vec<*const c_char> = o.iter().map(|c| c.as_ptr()).collect(); p.push(std::ptr::null());
    mpv_command(h, p.as_ptr())
}}
unsafe fn pump(h: *mut c_void, ms: u128) { unsafe { let t = std::time::Instant::now(); while t.elapsed().as_millis() < ms { mpv_wait_event(h, 0.05); } } }
fn main() { unsafe {
    let file = std::env::args().nth(1).expect("file");
    let out = std::env::args().nth(2).expect("outdir");
    let h = mpv_create();
    let lf = format!("{out}/mpv-debug.log");
    for (k, v) in [("vo","gpu-next"),("gpu-api","d3d11"),("hwdec","d3d11va"),("terminal","no"),("osc","no"),("geometry","960x540+100+100"),("log-file",lf.as_str()),
        ("pause","yes"),("keep-open","yes"),("config","no"),("target-colorspace-hint","auto"),("screenshot-format","png"),("screenshot-high-bit-depth","yes"),("screenshot-tag-colorspace","yes")] {
        mpv_set_option_string(h, s(k).as_ptr(), s(v).as_ptr());
    }
    mpv_initialize(h);
    cmd(h, &["loadfile", &file]);
    pump(h, 2500);
    cmd(h, &["seek", "2", "absolute", "exact"]);
    pump(h, 1000);
    println!("target-params(auto) = {}", prop(h, "video-target-params"));
    for w in ["auto", "80", "200", "203"] {
        mpv_set_property_string(h, s("hdr-reference-white").as_ptr(), s(w).as_ptr());
        pump(h, 800);
        let f = format!("{out}/white-{w}.png");
        let r = cmd(h, &["screenshot-to-file", &f, "window"]);
        pump(h, 500);
        println!("hdr-reference-white={w}: screenshot rc={r}; target={}", prop(h, "video-target-params").split(",").filter(|x| x.contains("luma") || x.contains("sig-peak") || x.contains("max-cll")).collect::<Vec<_>>().join(","));
    }
    mpv_terminate_destroy(h);
}}
