//! The review's hot paths (HOT_PATHS.md) timed with criterion, on real recordings' inputs kept in test_out/ (ignored
//! by git; BENCH.md, "Function benchmarks"). A bench whose input is missing is skipped with a message.
//! `cargo bench -- <name>` runs the benches whose name holds <name> (`cargo bench -- track/link`).

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

mod frames;
mod inputs;
mod review;
mod tracks;

/// The samples a bench takes, unless its group sets fewer.
const SAMPLES: usize = 50;
/// The samples of a group where one call is slow.
pub const FEW_SAMPLES: usize = 20;
/// The samples of a group where one call takes milliseconds.
pub const FEWEST_SAMPLES: usize = 10;
/// Changes smaller than this share are not reported: runs of the same code differ by up to about 10% on this machine.
const NOISE_SHARE: f64 = 0.05;

/// Short benches, so the whole suite takes about a minute: a group sets fewer samples where one call is slow, and
/// flat sampling (the same iterations in each sample) where one call takes milliseconds.
fn config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .sample_size(SAMPLES)
        .noise_threshold(NOISE_SHARE)
}

criterion_group! {
    name = hot_paths;
    config = config();
    targets = frames::fixed, frames::convert, frames::camera, frames::hud, frames::popup, frames::areas,
        tracks::track, review::matching, review::measure, review::report
}
criterion_main!(hot_paths);
