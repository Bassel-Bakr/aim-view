//! The statistics a report gives: means, spreads and medians, in plain floating point (python.rs has NumPy's, where
//! its order of operations reaches a result).
//!
//! In: a run's values (times, distances, speeds). Out: the summaries (summary.rs, what_if.rs), the measures
//! (measure.rs, matching.rs), the tracking summary (tracking.rs), the HUD's reading (hud.rs) and the mouse log
//! (mouse.rs).

/// The mean: the values added in order, divided by their count. NaN when there are none.
pub fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

/// The population standard deviation (Python's `statistics.pstdev`).
pub fn pstdev(values: &[f64]) -> f64 {
    let average = mean(values);
    (values.iter().map(|value| (value - average) * (value - average)).sum::<f64>() / values.len() as f64).sqrt()
}

/// The middle value, or the mean of the two middle values. Panics when there are none (`med` checks first).
pub fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    let middle = count / 2;
    if count % 2 == 1 { sorted[middle] } else { (sorted[middle - 1] + sorted[middle]) / 2.0 }
}

/// The median of the values that are there, or None when none are (review.py: `_med`).
pub fn med(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    let present: Vec<f64> = values.into_iter().flatten().collect();
    (!present.is_empty()).then(|| median(&present))
}
