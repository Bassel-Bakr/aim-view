//! What the review uses from SciPy's `ndimage`, done the way SciPy does it, so results match bit for bit.
//!
//! In: float32 lines and images, and lines of booleans. Out: SciPy's results. The fixed map (fixed.rs) blurs its walls
//! with `uniform_filter`, the pop-up watch (popup.rs) finds the pixels that stand out with it and closes and counts a
//! pop-up's episodes with the line functions, and the tracking summary (tracking.rs) uses the line functions too.

/// How a filter sees past the ends of a line (SciPy's `mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The end value repeats: a a a | a b c d | d d d.
    Nearest,
    /// The line mirrored, the end value included: b a | a b c d | d c.
    Reflect,
}

/// The line's value at `i`, which can be past either end: the edge says what lies there.
fn extended(line: &[f32], i: isize, edge: Edge) -> f64 {
    let len = line.len() as isize;
    let inside = match edge {
        Edge::Nearest => i.clamp(0, len - 1),
        Edge::Reflect => {
            // the line and its mirror image repeat every two lengths
            let period = 2 * len;
            let folded = i.rem_euclid(period);
            if folded < len { folded } else { period - folded - 1 }
        }
    };
    line[inside as usize] as f64
}

/// `uniform_filter1d` along one line: the line extended at its ends, a running sum in float64, each mean stored as
/// float32 (scipy/ndimage/src/ni_filters.c: `NI_UniformFilter1D`). Each value's mean over the `size` values around it
/// goes into `out` at the same index; `out` is as long as the line.
pub fn uniform_line(line: &[f32], size: usize, edge: Edge, out: &mut [f32]) {
    let len = line.len();
    let before = (size / 2) as isize;
    let at = |i: isize| extended(line, i - before, edge);
    // SciPy's `tmp`
    let mut running_sum = 0.0f64;
    for offset in 0..size {
        running_sum += at(offset as isize);
    }
    out[0] = (running_sum / size as f64) as f32;
    for (i, mean) in out.iter_mut().enumerate().take(len).skip(1) {
        running_sum += at((i + size - 1) as isize) - at(i as isize - 1);
        *mean = (running_sum / size as f64) as f32;
    }
}

/// `uniform_filter(image, size, mode)` on a float32 image (rows x cols): along the rows' axis (each column), then
/// along each row; the output is float32 after each.
pub fn uniform_filter(image: &[f32], rows: usize, cols: usize, size: usize, edge: Edge) -> Vec<f32> {
    let mut by_cols = vec![0f32; image.len()];
    let mut column = vec![0f32; rows];
    let mut column_out = vec![0f32; rows];
    for col in 0..cols {
        for row in 0..rows {
            column[row] = image[row * cols + col];
        }
        uniform_line(&column, size, edge, &mut column_out);
        for row in 0..rows {
            by_cols[row * cols + col] = column_out[row];
        }
    }
    let mut out = vec![0f32; image.len()];
    for row in 0..rows {
        let span = row * cols..(row + 1) * cols;
        uniform_line(&by_cols[span.clone()], size, edge, &mut out[span]);
    }
    out
}

/// `binary_dilation(line, iterations=iterations)` of a line with the default structure [1, 1, 1]: true within
/// `iterations` of a true.
pub fn dilate_line(line: &[bool], iterations: usize) -> Vec<bool> {
    let len = line.len();
    (0..len).map(|i| line[i.saturating_sub(iterations)..(i + iterations + 1).min(len)].iter().any(|&on| on)).collect()
}

/// `binary_erosion(line, iterations=iterations)` of a line with the default structure and border 0: true where every
/// value within `iterations` is true, the outside counting as false.
pub fn erode_line(line: &[bool], iterations: usize) -> Vec<bool> {
    let len = line.len();
    (0..len)
        .map(|i| i >= iterations && i + iterations < len && line[i - iterations..=i + iterations].iter().all(|&on| on))
        .collect()
}

/// `binary_closing(line, iterations=iterations)` of a line: dilated, then eroded.
pub fn close_line(line: &[bool], iterations: usize) -> Vec<bool> {
    erode_line(&dilate_line(line, iterations), iterations)
}

/// `label(line)[1]` for a line: how many runs of true it holds.
pub fn count_runs(line: &[bool]) -> usize {
    line.iter().enumerate().filter(|&(i, &on)| on && (i == 0 || !line[i - 1])).count()
}

/// `find_objects(label(line)[0])` for a line: each run of true as (start, stop), in order.
pub fn runs(line: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &on) in line.iter().chain([&false]).enumerate() {
        match (on, start) {
            (true, None) => start = Some(i),
            (false, Some(run_start)) => {
                out.push((run_start, i));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Checks the filters on short lines worked out by hand.
#[cfg(test)]
mod tests {
    use super::*;

    /// A mean over 3 with `Reflect` repeats each end value once past the end.
    #[test]
    fn reflect_mirrors_the_end_value() {
        let line = [1.0, 2.0, 3.0];
        let mut out = [0.0f32; 3];
        uniform_line(&line, 3, Edge::Reflect, &mut out);
        // (1 + 1 + 2) / 3, (1 + 2 + 3) / 3, (2 + 3 + 3) / 3
        assert_eq!(out, [(4.0f64 / 3.0) as f32, 2.0, (8.0f64 / 3.0) as f32]);
    }

    /// `count_runs` counts the runs of true, dilation widens a true by one each side, and erosion counts the outside
    /// as false.
    #[test]
    fn closing_fills_short_gaps_and_counts_runs() {
        let a = [true, false, false, true, false, false, false, false, false, true];
        assert_eq!(count_runs(&a), 3);
        assert_eq!(dilate_line(&[false, true, false, false], 1), vec![true, true, true, false]);
        assert_eq!(erode_line(&[true, true, true, true], 1), vec![false, true, true, false]);
    }
}
