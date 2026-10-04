//! `fixed::FixedMap` against Python's `fixed_map` on a real recording's key frames (keys.yuv: YUV 4:2:0 at 1280 x
//! 720, as ffmpeg gave them to Python), to the bit (fixed.npy). Fixtures from python/retired/tests/fixtures.py.

use std::fs;

use aimview::fixed::FixedMap;

mod common;
use common::{FRAME_PIXELS, fixed_map, fixture_dirs};

/// The bytes of one yuv420p key frame: the Y plane, then U and V at a quarter of its size each.
const YUV_FRAME_BYTES: usize = FRAME_PIXELS * 3 / 2;

#[test]
fn fixed_map_matches_python() {
    let dirs = fixture_dirs(&["keys.yuv"]);
    if dirs.is_empty() {
        eprintln!("no key frames in test_out/parity: frozen (python/retired/tests/fixtures.py made them)");
        return;
    }
    for dir in dirs {
        let keys = fs::read(dir.join("keys.yuv")).unwrap();
        let mut map = FixedMap::default();
        for frame in keys.chunks_exact(YUV_FRAME_BYTES) {
            map.add(frame);
        }
        let npy = fs::read(dir.join("fixed.npy")).unwrap();
        let want = fixed_map(&npy);
        let got = map.map();
        let wrong = got.iter().zip(want).filter(|(a, b)| a != b).count();
        assert_eq!(wrong, 0, "{}: {wrong} of {} pixels differ", dir.display(), FRAME_PIXELS);
        let fixed = got.iter().filter(|&&pixel| pixel == 1).count();
        let key_frames = keys.len() / YUV_FRAME_BYTES;
        eprintln!("{}: {key_frames} key frames, {fixed} fixed pixels, equal", dir.display());
    }
}
