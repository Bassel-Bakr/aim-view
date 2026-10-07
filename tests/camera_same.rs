//! The camera watch's tile shifts (src/camera.rs) against the ones it gave before, to the bit, on the parity cases'
//! frame pairs: a change made for speed must not change a reading. AIMVIEW_KEEP_SHIFTS=1 stores the shifts as they are
//! now (test_out/parity/<case>/review/camera_shifts.json); without it they are compared with the stored ones.

use std::fs;
use std::ops::Range;

use aimview::camera::{CameraPart, CameraWatch};
use aimview::geometry::{H, W};

mod common;
use common::{GrayFrames, parity_root};

/// The parity cases with gray frames.
const CASES: [&str; 3] = ["spectral", "flower", "pokeball5"];
/// How many of flower's frames the join test watches, cut at every frame between.
const JOIN_FRAMES: usize = 12;

/// A tile's shift as the bits of its two floats, so equal means equal to the bit; None where it was not read.
type ShiftBits = Option<(u32, u32)>;

/// Every stored frame's tile shifts equal the stored ones to the bit (or, with AIMVIEW_KEEP_SHIFTS, are stored).
#[test]
fn camera_shifts_are_unchanged() {
    let keep = std::env::var("AIMVIEW_KEEP_SHIFTS").is_ok();
    for case in CASES {
        let dir = parity_root().join(case);
        let Some(frames) = GrayFrames::read(&dir) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let rgb = vec![0u8; W * H * 3];
        // every stored frame in order, each against the one before it
        let mut watch = CameraWatch::new(&frames.left_out);
        for i in 0..frames.numbers.len() {
            watch.add(frames.frame(i), &rgb);
        }
        let got: Vec<Vec<ShiftBits>> = watch
            .shifts
            .iter()
            .map(|tiles| tiles.iter().map(|tile| tile.map(|(x, y)| (x.to_bits(), y.to_bits()))).collect())
            .collect();
        let path = dir.join("review").join("camera_shifts.json");
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

/// The watch over a recording split into two runs (the first also reading the second's first frame), each sent as
/// JSON and joined, gives the whole recording's shifts and countdown, to the bit, wherever the cut is.
#[test]
fn camera_runs_join_to_the_whole() {
    // a tracking run, where the camera turns
    let dir = parity_root().join("flower");
    let Some(frames) = GrayFrames::read(&dir) else {
        eprintln!("no {}", dir.display());
        return;
    };
    let frame_count = frames.numbers.len().min(JOIN_FRAMES);
    // the countdown rows as the frame's luma, so the countdown test reads something that changes
    let rgb = |i: usize| frames.frame(i).iter().flat_map(|&luma| [luma, luma, luma]).collect::<Vec<u8>>();
    let watch = |range: Range<usize>| {
        let mut watch = CameraWatch::new(&frames.left_out);
        range.for_each(|i| watch.add(frames.frame(i), &rgb(i)));
        serde_json::from_str::<CameraPart>(&serde_json::to_string(&watch.part()).unwrap()).unwrap()
    };
    let whole = watch(0..frame_count);
    assert!(whole.shifts.iter().skip(1).any(|tiles| tiles.iter().any(Option::is_some)));
    for cut in 1..frame_count {
        let mut joined = CameraWatch::new(&frames.left_out);
        joined.join(watch(0..cut + 1));
        joined.join(watch(cut..frame_count));
        assert_eq!(joined.part(), whole, "cut at {cut}");
    }
}
