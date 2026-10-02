//! Excluded areas that only sometimes show (a "Last kill" pop-up) are excluded only while they show (python/review.py:
//! `AreaWatch`). Every other frame, each area's stand-out pattern is kept, small: the pixels that differ from their
//! neighbours, as text and boxes do and a plain wall does not. After the run, an area is a pop-up when it is off for
//! 30% of the run or more, comes and goes 3 times or more (the results screen covering it once at the end is not),
//! and looks the same whenever it is on; it is then excluded in its on frames and 4 frames either side. Any other
//! area (the session box, a webcam) is excluded all the time.

use crate::geometry::{H, W};
use crate::python::{numpy_median, numpy_percentile};
use crate::scipy::{Edge, close_line, count_runs, dilate_line, uniform_filter};

/// Frames between looks.
pub const STEP: usize = 2;

/// One look at an area: which of its sampled pixels stand out (row by row).
#[derive(Clone, Debug)]
struct Look(Vec<bool>);

/// Watches a recording's excluded areas frame by frame.
#[derive(Clone, Debug)]
pub struct AreaWatch {
    boxes: Vec<[usize; 4]>,
    steps: Vec<usize>,
    looks: Vec<Vec<Look>>,
    frames: usize,
}

impl AreaWatch {
    /// For the excluded areas, as shares of the frame [x0, y0, x1, y1].
    pub fn new(areas: &[[f64; 4]]) -> AreaWatch {
        let boxes: Vec<[usize; 4]> = areas
            .iter()
            .map(|&[x0, y0, x1, y1]| {
                let px = |v: f64, s: usize| (v * s as f64).round_ties_even().max(0.0) as usize;
                [px(x0, W), px(y0, H), px(x1, W), px(y1, H)]
            })
            .collect();
        let steps = boxes.iter().map(|&[x0, y0, x1, y1]| ((x1 - x0).max(y1 - y0) / 64).max(1)).collect();
        AreaWatch { looks: vec![Vec::new(); boxes.len()], boxes, steps, frames: 0 }
    }

    /// One frame, RGB24 at 1280 x 720: every `STEP`th frame, each area's green channel, every k-th pixel (k keeps
    /// it to about 64 a side), and which of those pixels differ from their 3 x 3 mean by more than 8.
    pub fn add(&mut self, rgb: &[u8]) {
        if self.frames.is_multiple_of(STEP) {
            for ((&[x0, y0, x1, y1], &k), looks) in self.boxes.iter().zip(&self.steps).zip(&mut self.looks) {
                let ys: Vec<usize> = (y0..y1.min(H)).step_by(k).collect();
                let xs: Vec<usize> = (x0..x1.min(W)).step_by(k).collect();
                if ys.len().min(xs.len()) < 3 {
                    looks.push(Look(vec![false]));
                    continue;
                }
                let small: Vec<f32> =
                    ys.iter().flat_map(|&y| xs.iter().map(move |&x| rgb[(y * W + x) * 3 + 1] as f32)).collect();
                let mean = uniform_filter(&small, ys.len(), xs.len(), 3, Edge::Reflect);
                looks.push(Look(small.iter().zip(&mean).map(|(a, m)| (a - m).abs() > 8.0).collect()));
            }
        }
        self.frames += 1;
    }

    /// Per area: for a pop-up, whether it is excluded in each frame; None for an area excluded all the time.
    pub fn showing(&self) -> Vec<Option<Vec<bool>>> {
        self.looks.iter().map(|looks| self.popup(looks)).collect()
    }

    fn popup(&self, looks: &[Look]) -> Option<Vec<bool>> {
        let n = looks.len();
        if n < 20 {
            return None;
        }
        let size = looks[0].0.len();
        let share: Vec<f64> =
            looks.iter().map(|l| l.0.iter().filter(|&&v| v).count() as f64 / size as f64).collect();
        let p98 = numpy_percentile(&share, 98.0);
        if p98 < 0.04 {
            return None;
        }
        let on: Vec<bool> = share.iter().map(|&s| s >= 0.5 * p98).collect();
        let episodes = count_runs(&close_line(&on, 5));
        let on_count = on.iter().filter(|&&v| v).count();
        if on_count as f64 / n as f64 > 0.7 || episodes < 3 {
            return None;
        }
        // the pattern it has when on: pixels standing out in more than half the on frames
        let tpl: Vec<bool> = (0..size)
            .map(|p| {
                let c = looks.iter().zip(&on).filter(|(l, o)| **o && l.0[p]).count();
                c as f64 / on_count as f64 > 0.5
            })
            .collect();
        let matched: Vec<f64> = looks
            .iter()
            .map(|l| {
                let inter = l.0.iter().zip(&tpl).filter(|(a, b)| **a && **b).count();
                let union = l.0.iter().zip(&tpl).filter(|(a, b)| **a || **b).count();
                inter as f64 / union.max(1) as f64
            })
            .collect();
        let pick = |want: bool| -> Vec<f64> {
            matched.iter().zip(&on).filter(|(_, o)| **o == want).map(|(m, _)| *m).collect()
        };
        if numpy_median(&pick(true)) < 0.5 || numpy_median(&pick(false)) >= 0.2 {
            return None;
        }
        let seen: Vec<bool> =
            matched.iter().flat_map(|&m| std::iter::repeat_n(m >= 0.35, STEP)).take(self.frames).collect();
        Some(dilate_line(&seen, 4))
    }
}
