//! The statistics a report gives: means, spreads and medians, in plain floating point.

pub fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

/// The population standard deviation (Python's `statistics.pstdev`).
pub fn pstdev(values: &[f64]) -> f64 {
    let m = mean(values);
    (values.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / values.len() as f64).sqrt()
}

/// The middle value, or the mean of the two middle values.
pub fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// The median of the values that are there, or None when none are (review.py: `_med`).
pub fn med(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    let v: Vec<f64> = values.into_iter().flatten().collect();
    (!v.is_empty()).then(|| median(&v))
}
