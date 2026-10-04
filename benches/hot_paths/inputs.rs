//! Where the benches' inputs are: the parity fixtures (test_out/parity/, from python/retired/tests/fixtures.py) and the
//! native review's kept outputs (test_out/baselines/4b7ddc4/native/).

use std::path::PathBuf;

use serde::de::DeserializeOwned;

/// av1: 1wall 2targets xsmall (KovaaK's, 2560 x 1440 AV1, 6,038 frames, 25 key frames, 66 kills).
pub const AV1: &str = "1wall 2targets xsmall - valorant - 558.46 - 2026.10.01-16.23.04";
/// flower: Flower Easier (a tracking run).
pub const FLOWER: &str = "Flower Easier - 4801 - 2026.09.20-02.33.56";
/// The native review's outputs at the accuracy baseline's commit.
pub const NATIVE: &str = "test_out/baselines/4b7ddc4/native";

/// A file's bytes (its path from the repository's root), or None after a message naming the bench and the file.
pub fn bytes(bench: &str, path: &str) -> Option<Vec<u8>> {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    match std::fs::read(&full) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("{bench}: skipped, no {}", full.display());
            None
        }
    }
}

/// A JSON file read as `T`, or None as `bytes` gives it.
pub fn json<T: DeserializeOwned>(bench: &str, path: &str) -> Option<T> {
    bytes(bench, path).map(|b| serde_json::from_slice(&b).unwrap_or_else(|e| panic!("{path}: {e}")))
}

/// A text file, or None as `bytes` gives it.
pub fn text(bench: &str, path: &str) -> Option<String> {
    bytes(bench, path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The last `n` bytes of a NumPy .npy file: its data after the header.
pub fn npy(bench: &str, path: &str, n: usize) -> Option<Vec<u8>> {
    bytes(bench, path).map(|b| b[b.len() - n..].to_vec())
}

/// The excluded areas of a fixture's meta.json ([x0, y0, x1, y1, kind] as shares of the frame).
pub fn areas(meta: &serde_json::Value) -> Vec<[f64; 4]> {
    let areas = meta["areas"].as_array().map(Vec::as_slice).unwrap_or_default();
    areas.iter().map(|a| std::array::from_fn(|k| a[k].as_f64().unwrap())).collect()
}
