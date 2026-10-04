//! `popup::AreaWatch` against Python's `AreaWatch` on real recordings with a pop-up area: every frame decoded by ffmpeg
//! as review.rgb_frames does (scale=1280:720:flags=area, rgb24), and the per-frame decisions compared with Python's
//! (meta.json's `showing`). Fixtures from python/retired/tests/fixtures.py (with --areas). Decodes whole recordings, so
//! it runs on request: cargo test --release --test popup_parity -- --ignored

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use aimview::popup::AreaWatch;
use serde_json::Value;

#[test]
#[ignore = "decodes whole recordings with ffmpeg (about a minute each)"]
fn area_watch_matches_python() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    let dirs: Vec<PathBuf> = fs::read_dir(root)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    let mut checked = 0;
    for dir in dirs {
        let Ok(text) = fs::read_to_string(dir.join("meta.json")) else { continue };
        let meta: Value = serde_json::from_str(&text).unwrap();
        let Some(shows) = meta["showing"].as_array() else { continue };
        if shows.iter().all(Value::is_null) {
            continue;
        }
        let areas: Vec<[f64; 4]> = meta["areas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let v: Vec<f64> = a.as_array().unwrap()[..4].iter().map(|n| n.as_f64().unwrap()).collect();
                [v[0], v[1], v[2], v[3]]
            })
            .collect();
        let mut ffmpeg = Command::new("ffmpeg")
            .args(["-v", "error", "-i", meta["video"].as_str().unwrap()])
            .args(["-vf", "scale=1280:720:flags=area,format=rgb24", "-f", "rawvideo", "-"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("ffmpeg on the PATH");
        let mut out = ffmpeg.stdout.take().unwrap();
        let mut watch = AreaWatch::new(&areas);
        let mut frame = vec![0u8; 1280 * 720 * 3];
        while out.read_exact(&mut frame).is_ok() {
            watch.add(&frame);
        }
        ffmpeg.wait().unwrap();
        let got = watch.showing();
        for (k, (g, w)) in got.iter().zip(shows).enumerate() {
            let want: Option<Vec<bool>> = w.as_array().map(|f| f.iter().map(|v| v.as_i64() == Some(1)).collect());
            assert_eq!(g.is_some(), want.is_some(), "{} area {k}: pop-up or not", dir.display());
            if let (Some(g), Some(w)) = (g, &want) {
                let wrong = g.iter().zip(w).filter(|(a, b)| a != b).count();
                assert_eq!(g.len(), w.len(), "{} area {k}: frames", dir.display());
                assert_eq!(wrong, 0, "{} area {k}: {wrong} frames differ", dir.display());
                eprintln!("{} area {k}: excluded in {} of {} frames, as in Python", dir.display(), g.iter().filter(|&&v| v).count(), g.len());
            }
        }
        checked += 1;
    }
    assert!(checked > 0, "no fixture with a pop-up area: they are frozen (python/retired/tests/fixtures.py)");
}
