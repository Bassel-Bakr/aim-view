//! The camera watch's tile shifts (src/camera.rs) against the ones it gave before, to the bit, on the parity cases'
//! frame pairs: a change made for speed must not change a reading. AIMVIEW_KEEP_SHIFTS=1 stores the shifts as they are
//! now (test_out/parity/<case>/review/camera_shifts.json); without it they are compared with the stored ones.

use std::fs;
use std::path::PathBuf;

use aimview::camera::{CameraWatch, excluded};
use aimview::geometry::{H, W, overlay_shares};
use aimview::track::Mask;

const CASES: [&str; 3] = ["spectral", "flower", "pokeball5"];

/// A tile's shift as the bits of its two floats, so equal means equal to the bit; None where it was not read.
type ShiftBits = Option<(u32, u32)>;

#[test]
fn camera_shifts_are_unchanged() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    let keep = std::env::var("AIMVIEW_KEEP_SHIFTS").is_ok();
    for case in CASES {
        let dir = root.join(case);
        let Ok(npy) = fs::read(dir.join("fixed.npy")) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let review = dir.join("review");
        let picks: serde_json::Value = serde_json::from_str(&fs::read_to_string(review.join("gray.json")).unwrap()).unwrap();
        let stored: Vec<usize> = serde_json::from_value(picks["frames"].clone()).unwrap();
        let gray = fs::read(review.join("gray.raw")).unwrap();
        let bad = excluded(Mask::without(&overlay_shares()).kept(), &npy[npy.len() - W * H..]);
        let rgb = vec![0u8; W * H * 3];
        // every stored frame in order, each against the one before it
        let mut watch = CameraWatch::new(&bad);
        for k in 0..stored.len() {
            watch.add(&gray[k * W * H..(k + 1) * W * H], &rgb);
        }
        let got: Vec<Vec<ShiftBits>> = watch
            .shifts
            .iter()
            .map(|s| s.iter().map(|t| t.map(|(x, y)| (x.to_bits(), y.to_bits()))).collect())
            .collect();
        let path = review.join("camera_shifts.json");
        if keep {
            fs::write(&path, serde_json::to_string(&got).unwrap()).unwrap();
            eprintln!("{case}: {} frames' shifts kept", got.len());
            continue;
        }
        let want: Vec<Vec<ShiftBits>> = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let differ = got.iter().zip(&want).filter(|(a, b)| a != b).count();
        eprintln!("{case}: {} frames, {differ} differ", got.len());
        assert!(got.len() == want.len() && differ == 0, "{case}: {differ} frames' shifts changed");
    }
}
