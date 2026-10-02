//! What the review uses from SciPy's `ndimage`, done the way SciPy does it, so results match bit for bit.

/// How a filter sees past the ends of a line (SciPy's `mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The end value repeats: a a a | a b c d | d d d.
    Nearest,
    /// The line mirrored, the end value included: b a | a b c d | d c.
    Reflect,
}

fn extended(line: &[f32], i: isize, edge: Edge) -> f64 {
    let n = line.len() as isize;
    let j = match edge {
        Edge::Nearest => i.clamp(0, n - 1),
        Edge::Reflect => {
            let p = 2 * n;
            let k = i.rem_euclid(p);
            if k < n { k } else { p - k - 1 }
        }
    };
    line[j as usize] as f64
}

/// `uniform_filter1d` along one line: the line extended at its ends, a running sum in float64, each mean stored as
/// float32 (scipy/ndimage/src/ni_filters.c: `NI_UniformFilter1D`).
pub fn uniform_line(line: &[f32], size: usize, edge: Edge, out: &mut [f32]) {
    let n = line.len();
    let before = (size / 2) as isize;
    let at = |i: isize| extended(line, i - before, edge);
    let mut tmp = 0.0f64;
    for k in 0..size {
        tmp += at(k as isize);
    }
    out[0] = (tmp / size as f64) as f32;
    for (i, o) in out.iter_mut().enumerate().take(n).skip(1) {
        tmp += at((i + size - 1) as isize) - at(i as isize - 1);
        *o = (tmp / size as f64) as f32;
    }
}

/// `uniform_filter(a, size, mode)` on a float32 image (rows x cols): along the rows' axis (each column), then along
/// each row; the output is float32 after each.
pub fn uniform_filter(a: &[f32], rows: usize, cols: usize, size: usize, edge: Edge) -> Vec<f32> {
    let mut by_cols = vec![0f32; a.len()];
    let mut col = vec![0f32; rows];
    let mut out_col = vec![0f32; rows];
    for c in 0..cols {
        for r in 0..rows {
            col[r] = a[r * cols + c];
        }
        uniform_line(&col, size, edge, &mut out_col);
        for r in 0..rows {
            by_cols[r * cols + c] = out_col[r];
        }
    }
    let mut out = vec![0f32; a.len()];
    for r in 0..rows {
        uniform_line(&by_cols[r * cols..(r + 1) * cols], size, edge, &mut out[r * cols..(r + 1) * cols]);
    }
    out
}

/// `binary_dilation(a, iterations=k)` of a line with the default structure [1, 1, 1]: true within k of a true.
pub fn dilate_line(a: &[bool], k: usize) -> Vec<bool> {
    let n = a.len();
    (0..n).map(|i| a[i.saturating_sub(k)..(i + k + 1).min(n)].iter().any(|&v| v)).collect()
}

/// `binary_erosion(a, iterations=k)` of a line with the default structure and border 0: true where every value
/// within k is true, the outside counting as false.
pub fn erode_line(a: &[bool], k: usize) -> Vec<bool> {
    let n = a.len();
    (0..n).map(|i| i >= k && i + k < n && a[i - k..=i + k].iter().all(|&v| v)).collect()
}

/// `binary_closing(a, iterations=k)` of a line: dilated, then eroded.
pub fn close_line(a: &[bool], k: usize) -> Vec<bool> {
    erode_line(&dilate_line(a, k), k)
}

/// `label(a)[1]` for a line: how many runs of true it holds.
pub fn count_runs(a: &[bool]) -> usize {
    a.iter().enumerate().filter(|&(i, &v)| v && (i == 0 || !a[i - 1])).count()
}

/// `find_objects(label(a)[0])` for a line: each run of true as (start, stop), in order.
pub fn runs(a: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &v) in a.iter().chain([&false]).enumerate() {
        match (v, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_mirrors_the_end_value() {
        let line = [1.0, 2.0, 3.0];
        let mut out = [0.0f32; 3];
        uniform_line(&line, 3, Edge::Reflect, &mut out);
        // (1 + 1 + 2) / 3, (1 + 2 + 3) / 3, (2 + 3 + 3) / 3
        assert_eq!(out, [(4.0f64 / 3.0) as f32, 2.0, (8.0f64 / 3.0) as f32]);
    }

    #[test]
    fn closing_fills_short_gaps_and_counts_runs() {
        let a = [true, false, false, true, false, false, false, false, false, true];
        assert_eq!(count_runs(&a), 3);
        assert_eq!(dilate_line(&[false, true, false, false], 1), vec![true, true, true, false]);
        assert_eq!(erode_line(&[true, true, true, true], 1), vec![false, true, true, false]);
    }
}
