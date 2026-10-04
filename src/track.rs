//! Tracking: the detector's boxes kept or dropped per frame (python/retired/review.py: `track_model`'s `keep`), and the
//! targets of each frame given ids that follow them from frame to frame (`link`).

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W, to_deg};
use crate::python::{hypot, numpy_mean, round};

/// A box from the detector model, in frame pixels, as it gives them (float32): center, size, and score (on the reference
/// model's scale: src/model.rs).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawBox {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
}

/// Where targets count in a frame: every pixel but the excluded areas (W x H, row by row).
#[derive(Clone, Debug, PartialEq)]
pub struct Mask(Box<[bool; W * H]>);

impl Mask {
    /// Whether each pixel (row by row, 1280 x 720) is kept.
    pub fn kept(&self) -> &[bool] {
        &self.0[..]
    }

    /// Everywhere but the boxes, given as shares of the frame [x0, y0, x1, y1] (python/retired/review.py: `mask_of`).
    pub fn without(boxes: &[[f64; 4]]) -> Mask {
        let mut m: Box<[bool; W * H]> = vec![true; W * H].try_into().unwrap();
        let at = |share: f64, size: usize| ((share * size as f64).round_ties_even() as usize).min(size);
        for &[x0, y0, x1, y1] in boxes {
            for y in at(y0, H)..at(y1, H) {
                m[y * W + at(x0, W)..y * W + at(x1, W).max(at(x0, W))].fill(false);
            }
        }
        Mask(m)
    }

    /// Whether the pixel a box's center falls in counts (Python's `int()` truncates; the edges hold the rest).
    fn counts(&self, cx: f32, cy: f32) -> bool {
        let x = (cx.trunc() as i64).clamp(0, W as i64 - 1) as usize;
        let y = (cy.trunc() as i64).clamp(0, H as i64 - 1) as usize;
        self.0[y * W + x]
    }
}

/// Within this many degrees of the crosshair a box always stays: a target under the crosshair scores low.
const NEAR_DEG: f64 = 2.0;
/// The score of the one box kept beyond the scenario's target count: a new target can show while the old one dies.
const EXTRA_SCORE: f32 = 0.5;

fn from_crosshair(b: &RawBox) -> f64 {
    let (x, y) = to_deg(b.cx as f64, b.cy as f64);
    hypot(x, y)
}

/// The boxes of one frame that count, as targets: those whose center is not in an excluded area, and with the
/// scenario's target count, those within 2 degrees of the crosshair, then the most confident others up to the count,
/// plus one more if it scores 0.5 or more (a theme whose wall seams look like targets would flood the tracks).
/// The arithmetic on a box stays in float32, as NumPy keeps it: the model's boxes are float32.
pub fn keep(boxes: &[RawBox], mask: &Mask, cap: Option<usize>) -> Vec<Spot> {
    let mut d: Vec<RawBox> = boxes.iter().filter(|b| mask.counts(b.cx, b.cy)).copied().collect();
    if let Some(cap) = cap.filter(|&c| c > 0 && d.len() > c) {
        let near: Vec<RawBox> = d.iter().filter(|b| from_crosshair(b) < NEAR_DEG).copied().collect();
        let mut rest: Vec<RawBox> = d.iter().filter(|b| from_crosshair(b) >= NEAR_DEG).copied().collect();
        rest.sort_by(|a, b| b.score.total_cmp(&a.score));
        let k = cap.saturating_sub(near.len()).min(rest.len());
        let extra = rest.get(k).filter(|b| b.score >= EXTRA_SCORE).copied();
        d = near.into_iter().chain(rest[..k].iter().copied()).chain(extra).collect();
    }
    let quarter_pi = (std::f64::consts::PI / 4.0) as f32;
    d.iter()
        .filter(|b| mask.counts(b.cx, b.cy))
        .map(|b| {
            let (x0, y0) = to_deg((b.cx - b.w / 2.0) as f64, (b.cy - b.h / 2.0) as f64);
            let (x1, y1) = to_deg((b.cx + b.w / 2.0) as f64, (b.cy + b.h / 2.0) as f64);
            let (x, y) = to_deg(b.cx as f64, b.cy as f64);
            Spot {
                x,
                y,
                area: (quarter_pi * b.w * b.h).round_ties_even() as i64,
                model: Some(ModelBox { w: x1 - x0, h: y0 - y1, score: b.score as f64 }),
            }
        })
        .collect()
}

/// The detector model's box around a target, in degrees, and how sure it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBox {
    pub w: f64,
    pub h: f64,
    pub score: f64,
}

/// A target in one frame: its place in degrees from the crosshair (right and up positive), its area in pixels, and
/// from the detector model its box and score. The hand-written detector gives no box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub x: f64,
    pub y: f64,
    pub area: i64,
    pub model: Option<ModelBox>,
}

/// The excluded areas a frame's targets are kept from, as shares of the frame, with the pop-ups among them: for each
/// area, None when it is excluded all the time, else whether it is excluded in each frame (popup::AreaWatch).
/// Frames where a pop-up is off are kept again with only the areas still on (python/retired/review.py: `reopen`).
pub fn reopen(
    raw: &[Vec<RawBox>],
    kept: &mut [Vec<Spot>],
    areas: &[[f64; 4]],
    shows: &[Option<Vec<bool>>],
    cap: Option<usize>,
) {
    if shows.iter().all(Option::is_none) {
        return;
    }
    let mut masks: Vec<(Vec<bool>, Mask)> = Vec::new();
    for (i, boxes) in raw.iter().enumerate() {
        let on: Vec<bool> =
            shows.iter().map(|s| s.as_ref().is_none_or(|s| i < s.len() && s[i])).collect();
        if on.iter().all(|&o| o) {
            continue;
        }
        if !masks.iter().any(|(k, _)| *k == on) {
            let still: Vec<[f64; 4]> = areas.iter().zip(&on).filter(|(_, o)| **o).map(|(a, _)| *a).collect();
            masks.push((on.clone(), Mask::without(&still)));
        }
        let mask = &masks.iter().find(|(k, _)| *k == on).unwrap().1;
        kept[i] = keep(boxes, mask, cap);
    }
}

/// A target in a linked frame: [id, x, y], degrees rounded to 4 decimals.
pub type TrackPoint = (u32, f64, f64);

/// One frame of tracks, as tracks.json keeps it: the view's shift since the frame before (degrees), each target with
/// its id, its area, and from the model its box (w, h in degrees, 3 decimals) and score (3 decimals).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackFrame {
    pub i: usize,
    #[cfg_attr(feature = "ts", ts(as = "crate::typescript::ViewShift"))]
    pub shift: (f64, f64),
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::TrackPoint>"))]
    pub t: Vec<TrackPoint>,
    pub a: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<crate::typescript::TargetSize>>", optional))]
    pub wh: Option<Vec<(f64, f64)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub s: Option<Vec<f64>>,
}

/// The review's version: one more each time what a review keeps changes (the tracks, the camera's readings, the HUD's),
/// so a review kept by an older one is known (its report says `outdated`). 2: the HUD is read. 3: a spike in the
/// view's shift is repaired before the frames are linked (`link`).
pub const REVIEW_VERSION: u32 = 3;

/// A recording's tracks, as tracks.json keeps them: the frame rate, each frame's targets, and the review's version
/// (0 where it is not given: Python's, and the browser's and the desktop app's before version 2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tracks {
    pub fps: f64,
    pub frames: Vec<TrackFrame>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub version: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// A target matched to a track, as the next frame sees it.
#[derive(Clone, Copy)]
struct Tracked {
    id: u32,
    spot: Spot,
}

/// How far the view moved since the frame before: every pairing of a spot before with a spot now (within 6 degrees)
/// is a candidate; the one most pairings agree with (within 0.35 degrees) wins, and the shift is their mean. None when
/// no pairing is close enough, or there are too many to compare (over 2500).
fn view_shift(prev: &[Tracked], now: &[Spot]) -> Option<(f64, f64)> {
    let d: Vec<(f64, f64)> = prev
        .iter()
        .flat_map(|a| now.iter().map(move |b| (b.x - a.spot.x, b.y - a.spot.y)))
        .collect();
    // np.hypot: the C library's, as f64::hypot is
    let ok: Vec<bool> = d.iter().map(|(dx, dy)| dx.hypot(*dy) <= 6.0).collect();
    if !ok.iter().any(|&o| o) || d.len() > 2500 {
        return None;
    }
    let near = |k: usize, l: usize| (d[k].0 - d[l].0).hypot(d[k].1 - d[l].1) < 0.35;
    // only pairings within 0.35 in x can be near (hypot(x, y) >= |x|): each count looks at those, found by a binary
    // search in the pairings sorted by x, with a margin so rounding never leaves one out; `near` decides as before
    let mut order: Vec<usize> = (0..d.len()).collect();
    order.sort_unstable_by(|&a, &b| d[a].0.total_cmp(&d[b].0));
    let xs: Vec<f64> = order.iter().map(|&i| d[i].0).collect();
    let mut best = (0, i64::MIN);
    for (k, &ok) in ok.iter().enumerate() {
        let count = if ok {
            let (lo, hi) = (xs.partition_point(|&x| x < d[k].0 - 0.36), xs.partition_point(|&x| x <= d[k].0 + 0.36));
            order[lo..hi].iter().filter(|&&l| near(k, l)).count() as i64
        } else {
            -1
        };
        if count > best.1 {
            best = (k, count);
        }
    }
    let inliers: Vec<usize> = (0..d.len()).filter(|&l| near(best.0, l)).collect();
    let xs: Vec<f64> = inliers.iter().map(|&l| d[l].0).collect();
    let ys: Vec<f64> = inliers.iter().map(|&l| d[l].1).collect();
    Some((numpy_mean(&xs), numpy_mean(&ys)))
}

/// How far the view's shift must jump in one frame (degrees), and how many times the shifts either side, to be a
/// spike (`spikes`).
const SPIKE: f64 = 1.0;
const SPIKE_RATIO: f64 = 3.0;

/// Which frames' shifts are spikes: more than `SPIKE` degrees and `SPIKE_RATIO` times the shifts either side. A kill
/// can fool the view's shift on a plain wall: with the target at the crosshair gone, the shift lines up another target
/// with the dead one's place for one frame.
pub fn spikes(shifts: &[(f64, f64)]) -> Vec<bool> {
    let n = shifts.len();
    let size: Vec<f64> = shifts.iter().map(|s| hypot(s.0, s.1)).collect();
    (0..n)
        .map(|j| j >= 1 && j + 1 < n && size[j] > SPIKE && size[j] > SPIKE_RATIO * size[j - 1].max(size[j + 1]))
        .collect()
}

/// The frame's targets given ids: each track from the frame before is moved by the view's shift and takes the nearest
/// target now within 0.5 degrees; a target nobody took starts a new track.
fn follow(prev: &[Tracked], now: &[Spot], shift: (f64, f64), next_id: &mut u32) -> Box<[Tracked]> {
    let mut cur = Vec::with_capacity(now.len());
    let mut used = vec![false; now.len()];
    for a in prev {
        let (px, py) = (a.spot.x + shift.0, a.spot.y + shift.1);
        let mut nearest: Option<(f64, usize)> = None;
        for (j, b) in now.iter().enumerate() {
            if used[j] {
                continue;
            }
            let d = hypot(b.x - px, b.y - py);
            if nearest.is_none_or(|(n, _)| d < n) {
                nearest = Some((d, j));
            }
        }
        if let Some((d, j)) = nearest
            && d < 0.5
        {
            used[j] = true;
            cur.push(Tracked { id: a.id, spot: now[j] });
        }
    }
    for (j, b) in now.iter().enumerate() {
        if !used[j] {
            cur.push(Tracked { id: *next_id, spot: *b });
            *next_id += 1;
        }
    }
    // every spot now was taken by a track or started one
    cur.into_boxed_slice()
}

/// How many of the tracks before take a target now with this shift (`follow`).
fn linked(prev: &[Tracked], now: &[Spot], shift: (f64, f64)) -> usize {
    let mut new = 0;
    follow(prev, now, shift, &mut new);
    now.len() - new as usize
}

/// Track ids for the targets of each frame (`follow`). The view's shift of each frame is found first, against the
/// frame before as it was tracked. A spike (`spikes`) is replaced by the mean of the shifts either side when that
/// mean links as many of the frame before's targets as the spike does: a spike that lines up more of them is the
/// camera's own jerk (frames captured unevenly), and stays. Then the frames are tracked with those shifts.
pub fn link(frames: &[Vec<Spot>]) -> Box<[TrackFrame]> {
    let mut shifts = Vec::with_capacity(frames.len());
    let mut before = Vec::with_capacity(frames.len());
    let mut prev: Box<[Tracked]> = Box::default();
    let mut next_id = 0;
    for now in frames {
        let shift = if prev.is_empty() || now.is_empty() {
            (0.0, 0.0)
        } else {
            view_shift(&prev, now).unwrap_or((0.0, 0.0))
        };
        shifts.push(shift);
        let cur = follow(&prev, now, shift, &mut next_id);
        before.push(prev);
        prev = cur;
    }
    let (mut shifts, before) = (shifts.into_boxed_slice(), before.into_boxed_slice());
    let found = shifts.clone();
    for (j, spike) in spikes(&found).into_iter().enumerate() {
        if !spike {
            continue;
        }
        let mean = ((found[j - 1].0 + found[j + 1].0) / 2.0, (found[j - 1].1 + found[j + 1].1) / 2.0);
        if linked(&before[j], &frames[j], mean) >= linked(&before[j], &frames[j], found[j]) {
            shifts[j] = mean;
        }
    }
    let mut out = Vec::with_capacity(frames.len());
    let mut prev: Box<[Tracked]> = Box::default();
    let mut next_id = 0;
    for (i, (now, &shift)) in frames.iter().zip(shifts.iter()).enumerate() {
        let cur = follow(&prev, now, shift, &mut next_id);
        let boxes: Vec<ModelBox> = cur.iter().filter_map(|c| c.spot.model).collect();
        out.push(TrackFrame {
            i,
            shift,
            t: cur.iter().map(|c| (c.id, round(c.spot.x, 4), round(c.spot.y, 4))).collect(),
            a: cur.iter().map(|c| c.spot.area).collect(),
            wh: (!boxes.is_empty())
                .then(|| boxes.iter().map(|b| (round(b.w, 3), round(b.h, 3))).collect()),
            s: (!boxes.is_empty()).then(|| boxes.iter().map(|b| round(b.score, 3)).collect()),
        });
        prev = cur;
    }
    out.into_boxed_slice()
}
