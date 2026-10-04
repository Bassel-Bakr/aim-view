//! `fixed::FixedMap` against Python's `fixed_map` on a real recording's key frames (keys.yuv: YUV 4:2:0 at 1280 x
//! 720, as ffmpeg gave them to Python), to the bit (fixed.npy). Fixtures from python/retired/tests/fixtures.py.

use std::fs;
use std::path::PathBuf;

use aimview::fixed::FixedMap;
use aimview::geometry::{H, W};

#[test]
fn fixed_map_matches_python() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    let dirs: Vec<PathBuf> = fs::read_dir(root)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    let dirs: Vec<PathBuf> = dirs.into_iter().filter(|d| d.join("keys.yuv").exists()).collect();
    if dirs.is_empty() {
        eprintln!("no key frames in test_out/parity: frozen (python/retired/tests/fixtures.py made them)");
        return;
    }
    for dir in dirs {
        let keys = fs::read(dir.join("keys.yuv")).unwrap();
        let mut map = FixedMap::default();
        for frame in keys.chunks_exact(W * H * 3 / 2) {
            map.add(frame);
        }
        // fixed.npy: NumPy's .npy header, then W x H bytes
        let npy = fs::read(dir.join("fixed.npy")).unwrap();
        let want = &npy[npy.len() - W * H..];
        let got = map.map();
        let wrong = got.iter().zip(want).filter(|(a, b)| a != b).count();
        assert_eq!(wrong, 0, "{}: {wrong} of {} pixels differ", dir.display(), W * H);
        let fixed = got.iter().filter(|&&p| p == 1).count();
        eprintln!("{}: {} key frames, {fixed} fixed pixels, equal", dir.display(), keys.len() / (W * H * 3 / 2));
    }
}
