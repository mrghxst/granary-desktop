//! Diagnostic: switch HDR on the first HDR-capable display with `set_hdr`, print the read-back state for a few
//! seconds, then restore the original state. `cargo run -p granary-display --example hdr_probe`.
#[cfg(windows)]
fn main() {
    use granary_display::DisplayController;
    use granary_display::win::WindowsDisplayController;
    let c = WindowsDisplayController;
    let Some(d) = c.displays().unwrap_or_default().into_iter().find(|d| d.hdr_supported) else {
        println!("SKIP: no HDR-capable display");
        return;
    };
    let want = !d.hdr_enabled;
    println!(
        "{}: hdr_enabled={} cs={} -> set {want}",
        d.gdi_name, d.hdr_enabled, d.dxgi_color_space
    );
    let t = std::time::Instant::now();
    println!("set_hdr: {:?}", c.set_hdr(&d.id, want));
    for _ in 0..20 {
        if let Ok(n) = c.display(&d.id) {
            println!(
                "{:>5} ms hdr_enabled={} cs={} wce={} peak={}",
                t.elapsed().as_millis(),
                n.hdr_enabled,
                n.dxgi_color_space,
                n.wide_color_enforced,
                n.max_luminance
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    println!("restore: {:?}", c.set_hdr(&d.id, d.hdr_enabled));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    if let Ok(n) = c.display(&d.id) {
        println!("after restore: hdr_enabled={} cs={}", n.hdr_enabled, n.dxgi_color_space);
    }
}

#[cfg(not(windows))]
fn main() {}
