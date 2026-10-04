//! `popup::AreaWatch` against Python's `AreaWatch` on real recordings with a pop-up area: every frame decoded by ffmpeg
//! as review.rgb_frames does (scale=1280:720:flags=area, rgb24), and the per-frame decisions compared with Python's
//! (meta.json's `showing`). Fixtures from python/retired/tests/fixtures.py (with --areas). Decodes whole recordings, so
//! it runs on request: cargo test --release --test popup_parity -- --ignored

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use aimview::geometry::{H, W};
use aimview::popup::AreaWatch;
use serde_json::Value;

mod common;
use common::{excluded_areas, fixture_dirs, read, showing_frames};

/// The bytes of one 1280 x 720 rgb24 frame.
const RGB_FRAME_BYTES: usize = W * H * 3;

/// The watch after every frame of the video, decoded as Python's review decodes them.
fn watch_video(video: &str, areas: &[[f64; 4]]) -> AreaWatch {
    let mut ffmpeg = Command::new("ffmpeg")
        .args(["-v", "error", "-i", video])
        .args(["-vf", "scale=1280:720:flags=area,format=rgb24", "-f", "rawvideo", "-"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("ffmpeg on the PATH");
    let mut out = ffmpeg.stdout.take().unwrap();
    let mut watch = AreaWatch::new(areas);
    let mut frame = vec![0u8; RGB_FRAME_BYTES];
    while out.read_exact(&mut frame).is_ok() {
        watch.add(&frame);
    }
    ffmpeg.wait().unwrap();
    watch
}

/// Compares the watch's decisions on the fixture's video with Python's, area by area.
fn check_fixture(dir: &Path, meta: &Value, showing: &[Value]) {
    let watch = watch_video(meta["video"].as_str().unwrap(), &excluded_areas(meta));
    let got = watch.showing();
    for (area, (got_frames, python)) in got.iter().zip(showing).enumerate() {
        let want = showing_frames(python);
        assert_eq!(got_frames.is_some(), want.is_some(), "{} area {area}: pop-up or not", dir.display());
        if let (Some(got_frames), Some(want_frames)) = (got_frames, &want) {
            let wrong = got_frames.iter().zip(want_frames).filter(|(a, b)| a != b).count();
            assert_eq!(got_frames.len(), want_frames.len(), "{} area {area}: frames", dir.display());
            assert_eq!(wrong, 0, "{} area {area}: {wrong} frames differ", dir.display());
            let excluded = got_frames.iter().filter(|&&showing| showing).count();
            let frames = got_frames.len();
            eprintln!("{} area {area}: excluded in {excluded} of {frames} frames, as in Python", dir.display());
        }
    }
}

#[test]
#[ignore = "decodes whole recordings with ffmpeg (about a minute each)"]
fn area_watch_matches_python() {
    let mut checked = 0;
    for dir in fixture_dirs(&["meta.json"]) {
        let meta = read(&dir.join("meta.json"));
        let Some(showing) = meta["showing"].as_array() else { continue };
        if showing.iter().all(Value::is_null) {
            continue;
        }
        check_fixture(&dir, &meta, showing);
        checked += 1;
    }
    assert!(checked > 0, "no fixture with a pop-up area: they are frozen (python/retired/tests/fixtures.py)");
}
