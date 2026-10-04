//! Excluded areas that only sometimes show (a "Last kill" pop-up) are excluded only while they show
//! (python/retired/review.py: `AreaWatch`). Every other frame, each area's stand-out pattern is kept, small: the pixels
//! that differ from their neighbours, as text and boxes do and a plain wall does not. After the run, an area is a
//! pop-up when it is off for 30% of the run or more, comes and goes 3 times or more (the results screen covering it
//! once at the end is not), and looks the same whenever it is on; it is then excluded in its on frames and 4 frames
//! either side. Any other area (the session box, a webcam) is excluded all the time. An area the user named the
//! challenge's end screen (`END_SCREEN`) shows once or twice, at the end or between runs, and covers most of the frame:
//! it is excluded only while it shows, however few its episodes (excluded all the time, it hid the whole run: VT FlyTS,
//! 0 of 5 kills).

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W};
use crate::python::{numpy_median, numpy_percentile};
use crate::scipy::{Edge, close_line, count_runs, dilate_line, uniform_filter};

/// Frames between looks.
pub const STEP: usize = 2;

/// The area kind of the challenge's end screen (test_out/vod_app/area_kinds.json: "Challenge results").
pub const END_SCREEN: &str = "challenge_results";

/// One look at an area: which of its sampled pixels stand out (row by row). Sent between workers as bits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "LookBits", try_from = "LookBits")]
struct Look(Box<[bool]>);

/// A look as text: its pixel count, and its pixels as bits in hex (8 to a byte, the first pixel in the lowest bit).
#[derive(Serialize, Deserialize)]
struct LookBits {
    n: usize,
    hex: String,
}

impl From<Look> for LookBits {
    fn from(look: Look) -> LookBits {
        let hex = look
            .0
            .chunks(8)
            .map(|byte| format!("{:02x}", byte.iter().rev().fold(0u8, |acc, &b| (acc << 1) | b as u8)))
            .collect();
        LookBits { n: look.0.len(), hex }
    }
}

impl TryFrom<LookBits> for Look {
    type Error = String;

    fn try_from(bits: LookBits) -> Result<Look, String> {
        let bytes = (0..bits.hex.len())
            .step_by(2)
            .map(|i| bits.hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
            .collect::<Option<Vec<u8>>>()
            .ok_or("a look's bits are not hex")?;
        if bytes.len() != bits.n.div_ceil(8) {
            return Err("a look's bits do not match its pixel count".into());
        }
        Ok(Look((0..bits.n).map(|i| bytes[i / 8] >> (i % 8) & 1 == 1).collect()))
    }
}

/// Watches a recording's excluded areas frame by frame. A recording split into runs (reviewed in workers at once) has
/// a watch for each run, each started at its run's first frame and joined in order after. A recording reviewed only
/// from part way in (the user's run window) starts its first watch there: the frames before it have no looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaWatch {
    boxes: Box<[[usize; 4]]>,
    steps: Box<[usize]>,
    looks: Box<[Vec<Look>]>,
    frames: usize,
    /// The first frame watched: frames before it were not reviewed.
    #[serde(default)]
    from: usize,
    /// Per area: it is the challenge's end screen (`end_screens`); none set: no area is.
    #[serde(default)]
    ends: Box<[bool]>,
}

impl AreaWatch {
    /// For the excluded areas, as shares of the frame [x0, y0, x1, y1].
    pub fn new(areas: &[[f64; 4]]) -> AreaWatch {
        let boxes: Box<[[usize; 4]]> = areas
            .iter()
            .map(|&[x0, y0, x1, y1]| {
                let px = |v: f64, s: usize| (v * s as f64).round_ties_even().max(0.0) as usize;
                [px(x0, W), px(y0, H), px(x1, W), px(y1, H)]
            })
            .collect();
        let steps = boxes.iter().map(|&[x0, y0, x1, y1]| ((x1 - x0).max(y1 - y0) / 64).max(1)).collect();
        let looks = vec![Vec::new(); boxes.len()].into_boxed_slice();
        AreaWatch { looks, boxes, steps, frames: 0, from: 0, ends: Box::default() }
    }

    /// Which areas are the challenge's end screen, in the areas' order.
    pub fn end_screens(&mut self, which: &[bool]) {
        assert_eq!(which.len(), self.boxes.len(), "one flag per area");
        self.ends = which.into();
    }

    /// For a run of the recording that starts at frame `first`, before its first frame: it looks at the frames the
    /// whole recording's watch would (every `STEP`th from the start), so its looks follow on from the run before's.
    pub fn start_at(&mut self, first: usize) {
        self.frames = first;
        if self.unwatched() {
            self.from = first;
        }
    }

    /// The first frame watched: for a run of the recording, the frame it started at.
    pub fn from(&self) -> usize {
        self.from
    }

    fn unwatched(&self) -> bool {
        self.looks.iter().all(Vec::is_empty)
    }

    /// The looks of the run after this one (its watch started where this one stopped).
    pub fn join(&mut self, next: AreaWatch) {
        assert_eq!(self.boxes, next.boxes, "another recording's areas");
        if self.unwatched() {
            self.from = next.from;
        }
        if self.ends.is_empty() {
            self.ends = next.ends;
        }
        for (looks, more) in self.looks.iter_mut().zip(next.looks) {
            looks.extend(more);
        }
        self.frames = next.frames;
    }

    /// One frame, RGB24 at 1280 x 720: every `STEP`th frame, each area's green channel, every k-th pixel (k keeps
    /// it to about 64 a side), and which of those pixels differ from their 3 x 3 mean by more than 8.
    pub fn add(&mut self, rgb: &[u8]) {
        if self.frames.is_multiple_of(STEP) {
            for ((&[x0, y0, x1, y1], &k), looks) in self.boxes.iter().zip(&self.steps).zip(&mut self.looks) {
                let ys: Vec<usize> = (y0..y1.min(H)).step_by(k).collect();
                let xs: Vec<usize> = (x0..x1.min(W)).step_by(k).collect();
                if ys.len().min(xs.len()) < 3 {
                    looks.push(Look(Box::new([false])));
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
        self.looks.iter().enumerate().map(|(i, looks)| self.popup(looks, self.ends.get(i) == Some(&true))).collect()
    }

    /// `end`: the area is the challenge's end screen, which may show only once.
    fn popup(&self, looks: &[Look], end: bool) -> Option<Vec<bool>> {
        let n = looks.len();
        if n < 20 {
            return None;
        }
        let size = looks[0].0.len();
        let share: Vec<f64> =
            looks.iter().map(|l| l.0.iter().filter(|&&v| v).count() as f64 / size as f64).collect();
        let p98 = numpy_percentile(&share, 98.0);
        if p98 < 0.04 {
            // nothing ever shows there: an end screen that never came is not excluded
            return end.then(|| vec![false; self.frames]);
        }
        let on: Vec<bool> = share.iter().map(|&s| s >= 0.5 * p98).collect();
        let episodes = count_runs(&close_line(&on, 5));
        let on_count = on.iter().filter(|&&v| v).count();
        if !end && (on_count as f64 / n as f64 > 0.7 || episodes < 3) {
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
        let consistent = numpy_median(&pick(true)) >= 0.5 && numpy_median(&pick(false)) < 0.2;
        if !consistent && !end {
            return None;
        }
        // an end screen that does not look the same each time is excluded where it stands out (its on looks)
        let shown: Vec<bool> =
            if consistent { matched.iter().map(|&m| m >= 0.35).collect() } else { on };
        // the frames before the first look (none unless the review started part way in) are not excluded
        let skipped = self.from.div_ceil(STEP) * STEP;
        let seen: Vec<bool> = std::iter::repeat_n(false, skipped)
            .chain(shown.into_iter().flat_map(|s| std::iter::repeat_n(s, STEP)))
            .take(self.frames)
            .collect();
        Some(dilate_line(&seen, 4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An area that shows once, at the end (a challenge's end screen): an ordinary area is excluded all the time, the
    /// end screen only while it shows, and an end screen that never comes not at all.
    #[test]
    fn the_end_screen_is_left_out_only_while_it_shows() {
        let looks = |on_from: usize| -> Vec<Look> {
            (0..300).map(|i| Look((0..400).map(|p| if i >= on_from { p % 3 == 0 } else { p % 97 == i % 97 }).collect())).collect()
        };
        let watch = |end: bool, on_from: usize| {
            let mut w = AreaWatch::new(&[[0.1, 0.1, 0.9, 0.9]]);
            w.end_screens(&[end]);
            w.looks = vec![looks(on_from)].into_boxed_slice();
            w.frames = 300 * STEP;
            w.showing().remove(0)
        };
        assert_eq!(watch(false, 250), None);
        let shown = watch(true, 250).expect("the end screen is a pop-up");
        assert!(shown[..250 * STEP - 4].iter().all(|&s| !s), "left in while the run is on");
        assert!(shown[250 * STEP..].iter().all(|&s| s), "left out while it shows");
        assert_eq!(watch(true, 300), Some(vec![false; 300 * STEP]));
    }

    /// A recording's watch split into two runs at any frame, each sent as JSON and joined, is the whole recording's.
    #[test]
    fn runs_join_to_the_whole() {
        let areas = crate::geometry::overlay_shares();
        let mut seed = 7u32;
        let frames: Vec<Vec<u8>> = (0..9)
            .map(|_| {
                (0..W * H * 3)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        (seed >> 24) as u8
                    })
                    .collect()
            })
            .collect();
        let mut whole = AreaWatch::new(&areas);
        frames.iter().for_each(|f| whole.add(f));
        assert!(whole.looks.iter().all(|l| !l.is_empty()));
        let sent = |w: &AreaWatch| serde_json::from_str::<AreaWatch>(&serde_json::to_string(w).unwrap()).unwrap();
        for cut in 0..=frames.len() {
            let (mut a, mut b) = (AreaWatch::new(&areas), AreaWatch::new(&areas));
            frames[..cut].iter().for_each(|f| a.add(f));
            b.start_at(cut);
            frames[cut..].iter().for_each(|f| b.add(f));
            let mut joined = AreaWatch::new(&areas);
            joined.join(sent(&a));
            joined.join(sent(&b));
            assert_eq!(joined, whole, "cut at {cut}");
        }
    }
}
