//! Excluded areas that only sometimes show (a "Last kill" pop-up) are excluded only while they show
//! (python/retired/review.py: `AreaWatch`). Every other frame, each area's stand-out pattern is kept, small: the pixels
//! that differ from their neighbors, as text and boxes do and a plain wall does not. After the run, an area is a
//! pop-up when it is off for 30% of the run or more, comes and goes 3 times or more (the results screen covering it
//! once at the end is not), and looks the same whenever it is on; it is then excluded in its on frames and 4 frames
//! either side. Any other area (the session box, a webcam) is excluded all the time. An area the user named the
//! challenge's end screen (`END_SCREEN`) shows once or twice, at the end or between runs, and covers most of the frame:
//! it is excluded only while it shows, however few its episodes (excluded all the time, it hid the whole run: VT FlyTS,
//! 0 of 5 kills).
//!
//! In: each frame's 720p RGB (the browser's review worker or the native review, through the track step: tracker.rs) and
//! the excluded areas with their kinds. Out: per area, the frames it is excluded in, by which the track step keeps the
//! frames' boxes again (track.rs: `reopen`). A run's watch goes between workers as JSON, and the runs' are joined in
//! order.

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W};
use crate::python::{numpy_median, numpy_percentile};
use crate::scipy::{Edge, close_line, count_runs, dilate_line, uniform_filter};

/// Frames between looks: the watch looks at every frame whose index in the recording is a multiple of this.
pub const STEP: usize = 2;

/// The area kind of the challenge's end screen (test_out/vod_app/area_kinds.json: "Challenge results").
pub const END_SCREEN: &str = "challenge_results";

/// A look samples about this many pixels along an area's longer side.
const LOOK_SIDE_PX: usize = 64;
/// A look with fewer sampled pixels than this on a side sees nothing: one pixel that never stands out.
const MIN_LOOK_SIDE: usize = 3;
/// An RGB24 pixel's bytes.
const RGB_BYTES: usize = 3;
/// The green channel's place among a pixel's bytes: the looks read green alone.
const GREEN: usize = 1;
/// A sampled pixel stands out when its green differs from the mean of the 3 x 3 sampled pixels around it by more
/// than this, in 8-bit levels.
const STAND_OUT_LEVEL: f32 = 8.0;
/// The side of the square of sampled pixels a pixel's mean is taken over.
const NEIGHBOURHOOD: usize = 3;
/// Fewer looks than this are too few to tell a pop-up by: the area is excluded all the time.
const MIN_LOOKS: usize = 20;
/// The area's level: this percentile of its looks' shares of standing-out pixels.
const LEVEL_PERCENTILE: f64 = 98.0;
/// Under this level nothing ever shows in the area.
const MIN_LEVEL: f64 = 0.04;
/// A look is on when its share of standing-out pixels is at least this much of the level.
const ON_SHARE_OF_LEVEL: f64 = 0.5;
/// The on looks are closed over this many looks before their episodes are counted, so a short flicker is no new one.
const CLOSE_LOOKS: usize = 5;
/// An area on in more of its looks than this share is no pop-up: a pop-up is off for 30% of the run or more.
const MAX_ON_SHARE: f64 = 0.7;
/// A pop-up comes and goes at least this many times (the results screen covering an area once at the end does not).
const MIN_EPISODES: usize = 3;
/// A pixel is in the area's pattern when it stands out in more than this share of the on looks.
const PATTERN_SHARE: f64 = 0.5;
/// The area looks the same whenever it is on when its on looks match the pattern by ON_MATCH or more (median) and
/// its off looks by less than OFF_MATCH (median).
const ON_MATCH: f64 = 0.5;
/// The median match its off looks must stay under (see `ON_MATCH`).
const OFF_MATCH: f64 = 0.2;
/// A look that matches the pattern by this much or more shows the pop-up.
const SHOWN_MATCH: f64 = 0.35;
/// A pop-up is excluded this many frames either side of the frames it shows in.
const MARGIN_FRAMES: usize = 4;
/// A look's bits as text: 8 to a byte.
const BITS_PER_BYTE: usize = 8;
/// Each byte of a look's bits is written as 2 hex digits.
const HEX_DIGITS_PER_BYTE: usize = 2;

/// One look at an area: which of its sampled pixels stand out (row by row). Sent between workers as bits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "LookBits", try_from = "LookBits")]
struct Look(Box<[bool]>);

/// A look as text: its pixel count, and its pixels as bits in hex (8 to a byte, the first pixel in the lowest bit).
#[derive(Serialize, Deserialize)]
#[expect(clippy::min_ident_chars, reason = "`n` is the JSON's key, which the browser's workers send")]
struct LookBits {
    /// How many sampled pixels the look has.
    n: usize,
    /// The pixels' bits, 2 hex digits a byte, 8 pixels to a byte from the lowest bit.
    hex: String,
}

impl From<Look> for LookBits {
    /// Packs the look's pixels into bytes, 8 to a byte from the lowest bit, written as hex.
    fn from(look: Look) -> LookBits {
        let hex = look
            .0
            .chunks(BITS_PER_BYTE)
            .map(|byte| format!("{:02x}", byte.iter().rev().fold(0u8, |acc, &bit| (acc << 1) | bit as u8)))
            .collect();
        LookBits { n: look.0.len(), hex }
    }
}

impl TryFrom<LookBits> for Look {
    type Error = String;

    /// Unpacks the hex into the look's pixels; an error when it is not hex or its length does not fit the pixel count.
    fn try_from(bits: LookBits) -> Result<Look, String> {
        let bytes = (0..bits.hex.len())
            .step_by(HEX_DIGITS_PER_BYTE)
            .map(|i| bits.hex.get(i..i + HEX_DIGITS_PER_BYTE).and_then(|digits| u8::from_str_radix(digits, 16).ok()))
            .collect::<Option<Vec<u8>>>()
            .ok_or("a look's bits are not hex")?;
        if bytes.len() != bits.n.div_ceil(BITS_PER_BYTE) {
            return Err("a look's bits do not match its pixel count".into());
        }
        Ok(Look((0..bits.n).map(|i| bytes[i / BITS_PER_BYTE] >> (i % BITS_PER_BYTE) & 1 == 1).collect()))
    }
}

/// Watches a recording's excluded areas frame by frame. A recording split into runs (reviewed in workers at once) has
/// a watch for each run, each started at its run's first frame and joined in order after. A recording reviewed only
/// from part way in (the user's run window) starts its first watch there: the frames before it have no looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaWatch {
    /// Per area: its box in pixels of the 1280 x 720 frame [x0, y0, x1, y1].
    boxes: Box<[[usize; 4]]>,
    /// Per area: how far apart its sampled pixels are, in pixels (about 64 samples along its longer side).
    steps: Box<[usize]>,
    /// Per area: its looks so far, one every `STEP` frames.
    looks: Box<[Vec<Look>]>,
    /// The frames seen, those before the first watched included: the next frame's index in the recording.
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
        let to_pixel = |share: f64, size: usize| (share * size as f64).round_ties_even().max(0.0) as usize;
        let boxes: Box<[[usize; 4]]> = areas
            .iter()
            .map(|&[x0, y0, x1, y1]| [to_pixel(x0, W), to_pixel(y0, H), to_pixel(x1, W), to_pixel(y1, H)])
            .collect();
        let steps = boxes.iter().map(|&[x0, y0, x1, y1]| ((x1 - x0).max(y1 - y0) / LOOK_SIDE_PX).max(1)).collect();
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

    /// Whether no area has a look yet.
    fn unwatched(&self) -> bool {
        self.looks.iter().all(Vec::is_empty)
    }

    /// Adds the looks of the run after this one (its watch started where this one stopped). Panics when its areas
    /// differ.
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

    /// One frame, RGB24 at 1280 x 720: every `STEP`th frame, a look at each area (`look_at`).
    pub fn add(&mut self, rgb: &[u8]) {
        if self.frames.is_multiple_of(STEP) {
            for ((&area, &sample_step), looks) in self.boxes.iter().zip(&self.steps).zip(&mut self.looks) {
                looks.push(look_at(rgb, area, sample_step));
            }
        }
        self.frames += 1;
    }

    /// Per area: for a pop-up, whether it is excluded in each frame; None for an area excluded all the time.
    pub fn showing(&self) -> Vec<Option<Vec<bool>>> {
        self.looks.iter().enumerate().map(|(i, looks)| self.popup(looks, self.ends.get(i) == Some(&true))).collect()
    }

    /// One area's verdict from its looks: whether it is excluded in each frame, when it is a pop-up; None when it is
    /// excluded all the time. `end`: the area is the challenge's end screen, which may show only once.
    fn popup(&self, looks: &[Look], end: bool) -> Option<Vec<bool>> {
        let look_count = looks.len();
        if look_count < MIN_LOOKS {
            return None;
        }
        let pixels = looks[0].0.len();
        let share: Vec<f64> = looks
            .iter()
            .map(|look| look.0.iter().filter(|&&stands_out| stands_out).count() as f64 / pixels as f64)
            .collect();
        let level = numpy_percentile(&share, LEVEL_PERCENTILE);
        if level < MIN_LEVEL {
            // nothing ever shows there: an end screen that never came is not excluded
            return end.then(|| vec![false; self.frames]);
        }
        let on: Vec<bool> = share.iter().map(|&look_share| look_share >= ON_SHARE_OF_LEVEL * level).collect();
        let episodes = count_runs(&close_line(&on, CLOSE_LOOKS));
        let on_count = on.iter().filter(|&&look_on| look_on).count();
        if !end && (on_count as f64 / look_count as f64 > MAX_ON_SHARE || episodes < MIN_EPISODES) {
            return None;
        }
        let matched = pattern_matches(looks, &on, on_count, pixels);
        let consistent = numpy_median(&looks_matched(&matched, &on, true)) >= ON_MATCH
            && numpy_median(&looks_matched(&matched, &on, false)) < OFF_MATCH;
        if !consistent && !end {
            return None;
        }
        // an end screen that does not look the same each time is excluded where it stands out (its on looks)
        let shown: Vec<bool> =
            if consistent { matched.iter().map(|&score| score >= SHOWN_MATCH).collect() } else { on };
        Some(dilate_line(&self.frames_shown(shown), MARGIN_FRAMES))
    }

    /// Per frame, whether the area shows: each look's verdict for the `STEP` frames it stands for, up to the frames
    /// seen. The frames before the first look (none unless the review started part way in) do not show it.
    fn frames_shown(&self, shown: Vec<bool>) -> Vec<bool> {
        let skipped = self.from.div_ceil(STEP) * STEP;
        std::iter::repeat_n(false, skipped)
            .chain(shown.into_iter().flat_map(|look_shown| std::iter::repeat_n(look_shown, STEP)))
            .take(self.frames)
            .collect()
    }
}

/// One look at an area ([x0, y0, x1, y1] in pixels) of a frame (RGB24 at 1280 x 720): its green channel, every
/// `sample_step`th pixel (which keeps it to about 64 a side), and which of those pixels differ from their 3 x 3 mean
/// by more than 8.
fn look_at(rgb: &[u8], [x0, y0, x1, y1]: [usize; 4], sample_step: usize) -> Look {
    let ys: Vec<usize> = (y0..y1.min(H)).step_by(sample_step).collect();
    let xs: Vec<usize> = (x0..x1.min(W)).step_by(sample_step).collect();
    if ys.len().min(xs.len()) < MIN_LOOK_SIDE {
        return Look(Box::new([false]));
    }
    let small: Vec<f32> =
        ys.iter().flat_map(|&y| xs.iter().map(move |&x| rgb[(y * W + x) * RGB_BYTES + GREEN] as f32)).collect();
    let mean = uniform_filter(&small, ys.len(), xs.len(), NEIGHBOURHOOD, Edge::Reflect);
    Look(small.iter().zip(&mean).map(|(green, around)| (green - around).abs() > STAND_OUT_LEVEL).collect())
}

/// How well each look matches the pattern the area has when on (the pixels standing out in more than half the on
/// looks): the pixels standing out in both, over those standing out in either.
fn pattern_matches(looks: &[Look], on: &[bool], on_count: usize, pixels: usize) -> Vec<f64> {
    let pattern: Vec<bool> = (0..pixels)
        .map(|pixel| {
            let standing_out = looks.iter().zip(on).filter(|(look, look_on)| **look_on && look.0[pixel]).count();
            standing_out as f64 / on_count as f64 > PATTERN_SHARE
        })
        .collect();
    looks
        .iter()
        .map(|look| {
            let both = look.0.iter().zip(&pattern).filter(|(a, b)| **a && **b).count();
            let either = look.0.iter().zip(&pattern).filter(|(a, b)| **a || **b).count();
            both as f64 / either.max(1) as f64
        })
        .collect()
}

/// The matches of the looks that are on (`on_looks` true), or of those that are off.
fn looks_matched(matched: &[f64], on: &[bool], on_looks: bool) -> Vec<f64> {
    matched.iter().zip(on).filter(|(_, look_on)| **look_on == on_looks).map(|(score, _)| *score).collect()
}

/// Checks the end screen's rule and joining runs' watches.
#[cfg(test)]
mod tests {
    use super::*;

    /// An area that shows once, at the end (a challenge's end screen): an ordinary area is excluded all the time, the
    /// end screen only while it shows, and an end screen that never comes not at all.
    #[test]
    fn the_end_screen_is_left_out_only_while_it_shows() {
        let looks = |on_from: usize| -> Vec<Look> {
            let look = |i: usize| {
                Look((0..400).map(|pixel| if i >= on_from { pixel % 3 == 0 } else { pixel % 97 == i % 97 }).collect())
            };
            (0..300).map(look).collect()
        };
        let watch = |end: bool, on_from: usize| {
            let mut watch = AreaWatch::new(&[[0.1, 0.1, 0.9, 0.9]]);
            watch.end_screens(&[end]);
            watch.looks = vec![looks(on_from)].into_boxed_slice();
            watch.frames = 300 * STEP;
            watch.showing().remove(0)
        };
        assert_eq!(watch(false, 250), None);
        let shown = watch(true, 250).expect("the end screen is a pop-up");
        assert!(shown[..250 * STEP - 4].iter().all(|&shows| !shows), "left in while the run is on");
        assert!(shown[250 * STEP..].iter().all(|&shows| shows), "left out while it shows");
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
        frames.iter().for_each(|frame| whole.add(frame));
        assert!(whole.looks.iter().all(|looks| !looks.is_empty()));
        let sent =
            |watch: &AreaWatch| serde_json::from_str::<AreaWatch>(&serde_json::to_string(watch).unwrap()).unwrap();
        for cut in 0..=frames.len() {
            let (mut a, mut b) = (AreaWatch::new(&areas), AreaWatch::new(&areas));
            frames[..cut].iter().for_each(|frame| a.add(frame));
            b.start_at(cut);
            frames[cut..].iter().for_each(|frame| b.add(frame));
            let mut joined = AreaWatch::new(&areas);
            joined.join(sent(&a));
            joined.join(sent(&b));
            assert_eq!(joined, whole, "cut at {cut}");
        }
    }
}
