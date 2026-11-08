//! Manual benchmark of the preview decode path (not part of CI): `cargo test -p granary-player --test preview_bench
//! -- --ignored --nocapture`.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use granary_player::mkv::MkvIndex;
use granary_player::mpv::{MemoryFiles, Mpv};
use granary_player::range::FileRange;
use granary_player::thumbs;
use granary_player::{MpvEvent, PlayerBackend};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

fn media(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-media")
        .join(name)
}

fn run(label: &str, file: &str, extra: &[(&str, &str)], vf: &str) {
    let path = media(file);
    let mut src = FileRange::open(&path).unwrap();
    let index = MkvIndex::load(&mut src).unwrap();
    let cues: Vec<_> = index.cues().to_vec();
    let frames: Vec<Vec<u8>> = cues
        .iter()
        .take(8)
        .map(|c| index.frame_file(&mut src, c).unwrap())
        .collect();
    let mut opts = thumbs::options();
    for (k, v) in extra {
        opts.retain(|(n, _)| n != k);
        opts.push(((*k).into(), (*v).into()));
    }
    opts.retain(|(n, _)| n != "vf");
    opts.push(("vf".into(), vf.into()));
    let mpv = Mpv::new(&opts).unwrap();
    let files = Arc::new(MemoryFiles::default());
    mpv.add_memory_protocol("bench-frame", Arc::clone(&files)).unwrap();
    let (tx, rx) = channel();
    mpv.spawn_events(move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let mut times = Vec::new();
    for round in 0..3 {
        for (i, f) in frames.iter().enumerate() {
            let uri = format!("bench-frame://{round}-{i}.mkv");
            files.insert(uri.clone(), Arc::from(f.clone()));
            while rx.try_recv().is_ok() {}
            let t = Instant::now();
            mpv.command(&["loadfile", &uri]).unwrap();
            let mut started = false;
            loop {
                match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                    MpvEvent::StartFile => started = true,
                    MpvEvent::PlaybackRestart if started => break,
                    MpvEvent::EndFile { reason } if started => panic!("{reason:?}"),
                    _ => {}
                }
            }
            let raw = mpv.screenshot_raw().unwrap();
            let _ = thumbs::encode_jpeg(&raw).unwrap();
            if round > 0 {
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
    times.sort_by(f64::total_cmp);
    println!(
        "{label:40} median {:6.1} ms  min {:6.1} ms  (frames {})",
        times[times.len() / 2],
        times[0],
        frames.len()
    );
    mpv.quit();
}

const HDR: &str = "zscale=w=320:h=-2:t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p";

#[test]
#[ignore = "manual benchmark"]
fn preview_decode_variants() {
    for file in ["hdr10_hevc_main10_2160p.mkv", "sdr_h264_1080p.mkv"] {
        println!("== {file}");
        let vf = if file.starts_with("hdr") {
            HDR
        } else {
            "scale=w=320:h=-2"
        };
        run("current", file, &[], vf);
        run("no filter", file, &[], "");
        run("threads=1", file, &[("vd-lavc-threads", "1")], vf);
        run("demuxer=mkv", file, &[("demuxer", "mkv")], vf);
        run(
            "demuxer=mkv threads=1",
            file,
            &[("demuxer", "mkv"), ("vd-lavc-threads", "1")],
            vf,
        );
        run(
            "slice threads 4",
            file,
            &[("vd-lavc-threads", "4"), ("vd-lavc-o", "thread_type=slice")],
            vf,
        );
        run(
            "demuxer=mkv slice4 no-filter",
            file,
            &[
                ("demuxer", "mkv"),
                ("vd-lavc-threads", "4"),
                ("vd-lavc-o", "thread_type=slice"),
            ],
            "",
        );
    }
}
