//! Subtitle switching during playback against the real libmpv: a newly selected embedded track must show the line
//! that is on screen now, without the user seeking. Skipped when libmpv or the test media are missing.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code

use granary_player::backend::PlayerBackend;
use granary_player::mpv::Mpv;
use granary_player::probe::probe_libmpv;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn libmpv_available() -> bool {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor-bin/libmpv/libmpv-2.dll");
    let exe = std::env::current_exe().unwrap();
    let dst = exe.parent().unwrap().join("libmpv-2.dll");
    if !src.is_file() {
        eprintln!("skipped: run scripts/fetch-libmpv.ps1");
        return false;
    }
    if std::fs::metadata(&dst).map(|d| d.len()).ok() != std::fs::metadata(&src).map(|s| s.len()).ok() {
        let _ = std::fs::copy(&src, &dst);
    }
    probe_libmpv().is_ok()
}

fn wait_for(mpv: &Mpv, what: &str, mut ok: impl FnMut(&Mpv) -> bool, timeout: Duration) {
    let end = Instant::now() + timeout;
    while !ok(mpv) {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn pos(mpv: &Mpv) -> f64 {
    mpv.get_property("time-pos").and_then(|p| p.parse().ok()).unwrap_or(0.0)
}

#[test]
fn selecting_an_embedded_subtitle_shows_the_current_line() {
    if !libmpv_available() {
        return;
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sdr_h264_1080p.mkv");
    if !file.is_file() {
        eprintln!("skipped: run scripts/gen-test-media.ps1");
        return;
    }
    let opts: Vec<(String, String)> = [
        ("vo", "null"),
        ("ao", "null"),
        ("idle", "yes"),
        ("keep-open", "yes"),
        ("config", "no"),
        ("terminal", "no"),
        ("cache", "yes"),
        ("demuxer-max-bytes", "768MiB"),
        ("demuxer-max-back-bytes", "128MiB"),
        ("sid", "no"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let mpv = Mpv::new(&opts).unwrap();
    mpv.command(&["loadfile", file.to_str().unwrap()]).unwrap();
    // Inside "Italic line two" (3.5 s to 6.0 s), with no subtitle selected so far.
    wait_for(
        &mpv,
        "playback to reach 3.8 s",
        |m| pos(m) >= 3.8,
        Duration::from_secs(20),
    );
    assert_eq!(mpv.get_property("sid").as_deref(), Some("no"));
    let started = Instant::now();
    mpv.set_property("sid", "1").unwrap();
    wait_for(
        &mpv,
        "the current subtitle line",
        |m| {
            m.get_property("sub-text")
                .is_some_and(|t| t.contains("Italic line two"))
        },
        Duration::from_millis(1500),
    );
    eprintln!("subtitle shown {} ms after selecting it", started.elapsed().as_millis());
    mpv.quit();
}

/// Serves `file` over HTTP with byte ranges at about `rate` bytes per second, like the playback relay on a slow link.
fn serve(file: PathBuf, rate: usize) -> String {
    use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let file = file.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.trim().split('-').next().and_then(|s| s.parse::<u64>().ok());
                    }
                }
                let mut f = std::fs::File::open(&file).unwrap();
                let len = f.metadata().unwrap().len();
                let start = range.unwrap_or(0);
                f.seek(SeekFrom::Start(start)).unwrap();
                let mut out = conn;
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Type: video/x-matroska\r\nAccept-Ranges: bytes\r\n\
                     Content-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\n\r\n",
                    len - 1,
                    len - start
                );
                if out.write_all(head.as_bytes()).is_err() {
                    return;
                }
                let mut buf = vec![0u8; rate / 20];
                loop {
                    let n = f.read(&mut buf).unwrap_or(0);
                    if n == 0 || out.write_all(&buf[..n]).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
        }
    });
    format!("http://127.0.0.1:{port}/media.mkv")
}

#[test]
fn selecting_an_embedded_subtitle_over_http_shows_the_current_line() {
    if !libmpv_available() {
        return;
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sdr_h264_1080p.mkv");
    if !file.is_file() {
        eprintln!("skipped: run scripts/gen-test-media.ps1");
        return;
    }
    let url = serve(file, 6 * 1024 * 1024);
    let opts: Vec<(String, String)> = [
        ("vo", "null"),
        ("ao", "null"),
        ("idle", "yes"),
        ("keep-open", "yes"),
        ("config", "no"),
        ("terminal", "no"),
        ("cache", "yes"),
        ("demuxer-max-bytes", "768MiB"),
        ("demuxer-max-back-bytes", "128MiB"),
        ("stream-buffer-size", "4MiB"),
        ("cache-pause-wait", "3"),
        ("sid", "no"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let mpv = Mpv::new(&opts).unwrap();
    mpv.command(&["loadfile", &url]).unwrap();
    wait_for(
        &mpv,
        "playback to reach 3.8 s",
        |m| pos(m) >= 3.8,
        Duration::from_secs(30),
    );
    let started = Instant::now();
    // What the controller does: select, then seek exactly to where playback is, so the demuxer prerolls the new
    // track from the file index instead of mpv's refresh (which drops the read-ahead and waits for a 3 s buffer).
    let at = pos(&mpv);
    mpv.set_property("sid", "1").unwrap();
    mpv.command(&["seek", &format!("{at:.3}"), "absolute+exact"]).unwrap();
    let shown = Instant::now() + Duration::from_secs(3);
    while !mpv
        .get_property("sub-text")
        .is_some_and(|t| t.contains("Italic line two"))
    {
        if Instant::now() > shown {
            mpv.quit();
            panic!("subtitle not shown within 3 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    eprintln!("subtitle shown {} ms after selecting it", started.elapsed().as_millis());
    mpv.quit();
}
