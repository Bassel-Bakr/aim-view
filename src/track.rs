//! Tracking: the detector's boxes kept or dropped per frame (python/retired/review.py: `track_model`'s `keep`), and the
//! targets of each frame given ids that follow them from frame to frame (`link`).
//!
//! In: each frame's boxes from the detector (detect.rs, through the track step: tracker.rs) and the excluded areas.
//! Out: the frames' tracks, which tracks.json keeps (`TrackFrame`, `Tracks`) and the matching, the measures, the
//! tracking summary, the camera watch and the faint cut-off read.

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W, to_deg};
use crate::python::{hypot, numpy_mean, round};

/// A box from the detector model, in frame pixels, as it gives them (float32): center, size, and score (on the
/// reference model's scale: src/model.rs).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[expect(clippy::min_ident_chars, reason = "the JSON's keys, which a run's part carries between workers")]
pub struct RawBox {
    /// The box's center x, in pixels of the 1280 x 720 frame.
    pub cx: f32,
    /// The box's center y, in pixels from the frame's top.
    pub cy: f32,
    /// The box's width, in pixels.
    pub w: f32,
    /// The box's height, in pixels.
    pub h: f32,
    /// How sure the detector is, from 0 to 1, on the reference model's scale.
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
        let mut kept: Box<[bool; W * H]> = vec![true; W * H].try_into().unwrap();
        let to_pixel = |share: f64, size: usize| ((share * size as f64).round_ties_even() as usize).min(size);
        for &[x0, y0, x1, y1] in boxes {
            for y in to_pixel(y0, H)..to_pixel(y1, H) {
                kept[y * W + to_pixel(x0, W)..y * W + to_pixel(x1, W).max(to_pixel(x0, W))].fill(false);
            }
        }
        Mask(kept)
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
/// pi / 4 in float32: a box's area in pixels is the ellipse inside it, worked out in float32 as NumPy does.
const QUARTER_PI: f32 = (std::f64::consts::PI / 4.0) as f32;

/// How far a box's center is from the crosshair, in degrees.
fn from_crosshair(raw: &RawBox) -> f64 {
    let (x, y) = to_deg(raw.cx as f64, raw.cy as f64);
    hypot(x, y)
}

/// The boxes of one frame that count, as targets: those whose center is not in an excluded area, and with the
/// scenario's target count, those within 2 degrees of the crosshair, then the most confident others up to the count,
/// plus one more if it scores 0.5 or more (a theme whose wall seams look like targets would flood the tracks).
/// The arithmetic on a box stays in float32, as NumPy keeps it: the model's boxes are float32.
pub fn keep(boxes: &[RawBox], mask: &Mask, cap: Option<usize>) -> Vec<Spot> {
    let mut counted: Vec<RawBox> = boxes.iter().filter(|raw| mask.counts(raw.cx, raw.cy)).copied().collect();
    if let Some(cap) = cap.filter(|&count| count > 0 && counted.len() > count) {
        counted = within_count(&counted, cap);
    }
    counted.iter().filter(|raw| mask.counts(raw.cx, raw.cy)).map(spot_of).collect()
}

/// More boxes than the scenario's target count (`cap`): those within `NEAR_DEG` of the crosshair, then the most
/// confident others up to the count, plus the next one if it scores `EXTRA_SCORE` or more.
fn within_count(boxes: &[RawBox], cap: usize) -> Vec<RawBox> {
    let near: Vec<RawBox> = boxes.iter().filter(|raw| from_crosshair(raw) < NEAR_DEG).copied().collect();
    let mut rest: Vec<RawBox> = boxes.iter().filter(|raw| from_crosshair(raw) >= NEAR_DEG).copied().collect();
    rest.sort_by(|a, b| b.score.total_cmp(&a.score));
    let others = cap.saturating_sub(near.len()).min(rest.len());
    let extra = rest.get(others).filter(|raw| raw.score >= EXTRA_SCORE).copied();
    near.into_iter().chain(rest[..others].iter().copied()).chain(extra).collect()
}

/// A kept box as a target: its center and its box in degrees from the crosshair, and its area in pixels.
fn spot_of(raw: &RawBox) -> Spot {
    let (x0, y0) = to_deg((raw.cx - raw.w / 2.0) as f64, (raw.cy - raw.h / 2.0) as f64);
    let (x1, y1) = to_deg((raw.cx + raw.w / 2.0) as f64, (raw.cy + raw.h / 2.0) as f64);
    let (x, y) = to_deg(raw.cx as f64, raw.cy as f64);
    Spot {
        x,
        y,
        area: (QUARTER_PI * raw.w * raw.h).round_ties_even() as i64,
        model: Some(ModelBox { w: x1 - x0, h: y0 - y1, score: raw.score as f64 }),
    }
}

/// The detector model's box around a target, in degrees, and how sure it is.
#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(clippy::min_ident_chars, reason = "`w` and `h` match RawBox's; the parity tests and the benches set them")]
pub struct ModelBox {
    /// The box's width, in degrees: the horizontal angle between its left and right edges.
    pub w: f64,
    /// The box's height, in degrees: the vertical angle between its top-left and bottom-right corners.
    pub h: f64,
    /// How sure the detector is, from 0 to 1, on the reference model's scale.
    pub score: f64,
}

/// A target in one frame: its place in degrees from the crosshair (right and up positive), its area in pixels, and
/// from the detector model its box and score. The hand-written detector gives no box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    /// Degrees right of the crosshair (left is negative).
    pub x: f64,
    /// Degrees above the crosshair (below is negative).
    pub y: f64,
    /// The target's area in pixels of the 1280 x 720 frame: the ellipse inside its box, rounded.
    pub area: i64,
    /// The detector model's box and score; None from the old Python review's hand-written detector, which gave no
    /// boxes (old tracks.json files, the parity fixtures).
    pub model: Option<ModelBox>,
}

/// Keeps the targets again in each frame where a pop-up is off, with only the areas still on
/// (python/retired/review.py: `reopen`). `raw` is each frame's boxes; `kept` each frame's targets with every area
/// excluded, changed in place; `areas` the excluded areas as shares of the frame; `shows`, for each area, None when it
/// is excluded all the time, else whether it is excluded in each frame (popup::AreaWatch).
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
    // each set of areas on, with its mask, made once
    let mut masks: Vec<(Vec<bool>, Mask)> = Vec::new();
    for (i, boxes) in raw.iter().enumerate() {
        let on: Vec<bool> =
            shows.iter().map(|show| show.as_ref().is_none_or(|frames| i < frames.len() && frames[i])).collect();
        if on.iter().all(|&area_on| area_on) {
            continue;
        }
        if !masks.iter().any(|(areas_on, _)| *areas_on == on) {
            let still: Vec<[f64; 4]> =
                areas.iter().zip(&on).filter(|(_, area_on)| **area_on).map(|(area, _)| *area).collect();
            masks.push((on.clone(), Mask::without(&still)));
        }
        let mask = &masks.iter().find(|(areas_on, _)| *areas_on == on).unwrap().1;
        kept[i] = keep(boxes, mask, cap);
    }
}

/// A target in a linked frame: [id, x, y], degrees rounded to 4 decimals.
pub type TrackPoint = (u32, f64, f64);

/// One frame of tracks, as tracks.json keeps it: the view's shift since the frame before (degrees), each target with
/// its id, its area, and from the model its box (w, h in degrees, 3 decimals) and score (3 decimals).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[expect(clippy::min_ident_chars, reason = "tracks.json's keys, which the UI, the workers and saved tracks read")]
pub struct TrackFrame {
    /// The frame's index in the recording, from 0.
    pub i: usize,
    /// How far the view moved since the frame before, x and y in degrees ((0, 0) when no shift was found).
    #[cfg_attr(feature = "ts", ts(as = "crate::typescript::ViewShift"))]
    pub shift: (f64, f64),
    /// The frame's targets: each one's track id and place (degrees from the crosshair, 4 decimals).
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::TrackPoint>"))]
    pub t: Vec<TrackPoint>,
    /// Each target's area in pixels, in the order of `t`.
    pub a: Vec<i64>,
    /// Each target's box width and height in degrees (3 decimals), in the order of `t`; None when the detector model
    /// gave no boxes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<crate::typescript::TargetSize>>", optional))]
    pub wh: Option<Vec<(f64, f64)>>,
    /// Each target's score (3 decimals), in the order of `t`; None when the detector model gave no boxes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub s: Option<Vec<f64>>,
}

/// The decimals tracks.json keeps of a target's place (degrees).
const PLACE_DECIMALS: usize = 4;
/// The decimals tracks.json keeps of a target's box size (degrees) and score.
const BOX_DECIMALS: usize = 3;

/// The review's version: one more each time what a review keeps changes (the tracks, the camera's readings, the HUD's),
/// so a review kept by an older one is known (its report says `outdated`). 2: the HUD is read. 3: a spike in the
/// view's shift is repaired before the frames are linked (`link`).
pub const REVIEW_VERSION: u32 = 3;

/// A recording's tracks, as tracks.json keeps them: the frame rate, each frame's targets, and the review's version
/// (0 where it is not given: Python's, and the browser's and the desktop app's before version 2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tracks {
    /// The recording's frame rate, in frames a second.
    pub fps: f64,
    /// Every frame's targets, in the recording's order.
    pub frames: Vec<TrackFrame>,
    /// The `REVIEW_VERSION` that made the tracks; 0, and left out of the JSON, where it is not known.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub version: u32,
}

/// Whether the version is 0, which tracks.json leaves out.
fn is_zero(version: &u32) -> bool {
    *version == 0
}

/// A target matched to a track, as the next frame sees it.
#[derive(Clone, Copy)]
struct Tracked {
    /// The track's id: the order tracks started in, from 0.
    id: u32,
    /// Where the target is in this frame.
    spot: Spot,
}

/// The view's shift when none is found: no move.
const NO_SHIFT: (f64, f64) = (0.0, 0.0);
/// A spot before and a spot now farther apart than this (degrees) are no candidate for the view's shift.
const MAX_PAIRING_DEG: f64 = 6.0;
/// More pairings than this are too many to compare: no shift is found.
const MAX_PAIRINGS: usize = 2500;
/// Two pairings agree when their moves are within this (degrees).
const AGREE_DEG: f64 = 0.35;
/// The pairings looked at for agreeing with one: those within this in x (degrees), a margin over `AGREE_DEG` so
/// rounding never leaves one out.
const AGREE_SEARCH_DEG: f64 = 0.36;

/// How far the view moved since the frame before: every pairing of a spot before with a spot now (within 6 degrees)
/// is a candidate; the one most pairings agree with (within 0.35 degrees) wins, and the shift is their mean. None when
/// no pairing is close enough, or there are too many to compare (over 2500).
fn view_shift(prev: &[Tracked], now: &[Spot]) -> Option<(f64, f64)> {
    let before: Vec<(f64, f64)> = prev.iter().map(|tracked| (tracked.spot.x, tracked.spot.y)).collect();
    let after: Vec<(f64, f64)> = now.iter().map(|spot| (spot.x, spot.y)).collect();
    view_shift_between(&before, &after)
}

/// `view_shift` on places alone (degrees): src/matching.rs works the view's shift out again with it, where it has
/// left the crosshair's boxes out.
pub fn view_shift_between(before: &[(f64, f64)], after: &[(f64, f64)]) -> Option<(f64, f64)> {
    let moves: Vec<(f64, f64)> =
        before.iter().flat_map(|&(x0, y0)| after.iter().map(move |&(x1, y1)| (x1 - x0, y1 - y0))).collect();
    // np.hypot: the C library's, as f64::hypot is
    let close: Vec<bool> = moves.iter().map(|(dx, dy)| dx.hypot(*dy) <= MAX_PAIRING_DEG).collect();
    if !close.iter().any(|&is_close| is_close) || moves.len() > MAX_PAIRINGS {
        return None;
    }
    let agree = |a: usize, b: usize| (moves[a].0 - moves[b].0).hypot(moves[a].1 - moves[b].1) < AGREE_DEG;
    // only pairings within AGREE_DEG in x can agree (hypot(x, y) >= |x|): each count looks at those, found by a binary
    // search in the pairings sorted by x, with a margin so rounding never leaves one out; `agree` decides as before
    let mut order: Vec<usize> = (0..moves.len()).collect();
    order.sort_unstable_by(|&a, &b| moves[a].0.total_cmp(&moves[b].0));
    let xs: Vec<f64> = order.iter().map(|&i| moves[i].0).collect();
    let (mut best, mut best_count) = (0, i64::MIN);
    for (i, &is_close) in close.iter().enumerate() {
        let count = if is_close {
            let from = xs.partition_point(|&x| x < moves[i].0 - AGREE_SEARCH_DEG);
            let to = xs.partition_point(|&x| x <= moves[i].0 + AGREE_SEARCH_DEG);
            order[from..to].iter().filter(|&&j| agree(i, j)).count() as i64
        } else {
            -1
        };
        if count > best_count {
            (best, best_count) = (i, count);
        }
    }
    let inliers: Vec<usize> = (0..moves.len()).filter(|&j| agree(best, j)).collect();
    let xs: Vec<f64> = inliers.iter().map(|&j| moves[j].0).collect();
    let ys: Vec<f64> = inliers.iter().map(|&j| moves[j].1).collect();
    Some((numpy_mean(&xs), numpy_mean(&ys)))
}

/// How far the view's shift must jump in one frame to be a spike (`spikes`), in degrees.
const SPIKE_DEG: f64 = 1.0;
/// How many times the larger of the shifts either side a spike must be.
const SPIKE_RATIO: f64 = 3.0;

/// Which frames' shifts are spikes: more than `SPIKE_DEG` degrees and `SPIKE_RATIO` times the shifts either side. A
/// kill can fool the view's shift on a plain wall: with the target at the crosshair gone, the shift lines up another
/// target with the dead one's place for one frame.
pub fn spikes(shifts: &[(f64, f64)]) -> Vec<bool> {
    let count = shifts.len();
    let size: Vec<f64> = shifts.iter().map(|shift| hypot(shift.0, shift.1)).collect();
    (0..count)
        .map(|j| j >= 1 && j + 1 < count && size[j] > SPIKE_DEG && size[j] > SPIKE_RATIO * size[j - 1].max(size[j + 1]))
        .collect()
}

/// A track takes the nearest target now within this many degrees of where the view's shift moved it.
const FOLLOW_DEG: f64 = 0.5;

/// The frame's targets given ids: each track from the frame before is moved by the view's shift and takes the nearest
/// target now within 0.5 degrees; a target nobody took starts a new track.
fn follow(prev: &[Tracked], now: &[Spot], shift: (f64, f64), next_id: &mut u32) -> Box<[Tracked]> {
    let mut current = Vec::with_capacity(now.len());
    let mut used = vec![false; now.len()];
    for track in prev {
        let (x, y) = (track.spot.x + shift.0, track.spot.y + shift.1);
        if let Some((distance, j)) = nearest_unused(now, &used, x, y)
            && distance < FOLLOW_DEG
        {
            used[j] = true;
            current.push(Tracked { id: track.id, spot: now[j] });
        }
    }
    for (j, spot) in now.iter().enumerate() {
        if !used[j] {
            current.push(Tracked { id: *next_id, spot: *spot });
            *next_id += 1;
        }
    }
    // every spot now was taken by a track or started one
    current.into_boxed_slice()
}

/// The target now nearest the place (x, y) that no track has taken, and its distance in degrees; the first of equals.
fn nearest_unused(now: &[Spot], used: &[bool], x: f64, y: f64) -> Option<(f64, usize)> {
    let mut nearest: Option<(f64, usize)> = None;
    for (j, spot) in now.iter().enumerate() {
        if used[j] {
            continue;
        }
        let distance = hypot(spot.x - x, spot.y - y);
        if nearest.is_none_or(|(nearest_distance, _)| distance < nearest_distance) {
            nearest = Some((distance, j));
        }
    }
    nearest
}

/// How many of the tracks before take a target now with this shift (`follow`).
fn linked(prev: &[Tracked], now: &[Spot], shift: (f64, f64)) -> usize {
    let mut new_tracks = 0;
    follow(prev, now, shift, &mut new_tracks);
    now.len() - new_tracks as usize
}

/// The frames tracked once with the view's shift each one has against the frame before as it was tracked: those
/// shifts, and the tracks of the frame before each frame.
struct FoundShifts {
    /// Each frame's view shift against the frame before, x and y in degrees.
    shifts: Box<[(f64, f64)]>,
    /// Each frame's tracks of the frame before it, as tracked with the shifts found (empty for the first frame).
    before: Box<[Box<[Tracked]>]>,
}

/// Tracks the frames once, each with the view's shift found against the frame before (no shift when either frame has
/// no targets or none is found).
fn found_shifts(frames: &[Vec<Spot>]) -> FoundShifts {
    let mut shifts = Vec::with_capacity(frames.len());
    let mut before = Vec::with_capacity(frames.len());
    let mut prev: Box<[Tracked]> = Box::default();
    let mut next_id = 0;
    for now in frames {
        let shift =
            if prev.is_empty() || now.is_empty() { NO_SHIFT } else { view_shift(&prev, now).unwrap_or(NO_SHIFT) };
        shifts.push(shift);
        let current = follow(&prev, now, shift, &mut next_id);
        before.push(prev);
        prev = current;
    }
    FoundShifts { shifts: shifts.into_boxed_slice(), before: before.into_boxed_slice() }
}

/// The shifts found, each spike (`spikes`) replaced by the mean of the shifts either side when that mean links as many
/// of the frame before's targets as the spike does.
fn repaired_shifts(frames: &[Vec<Spot>], found: &FoundShifts) -> Box<[(f64, f64)]> {
    let mut shifts = found.shifts.clone();
    for (j, spike) in spikes(&found.shifts).into_iter().enumerate() {
        if !spike {
            continue;
        }
        let (before_spike, after_spike) = (found.shifts[j - 1], found.shifts[j + 1]);
        let mean = ((before_spike.0 + after_spike.0) / 2.0, (before_spike.1 + after_spike.1) / 2.0);
        let tracks_before = &found.before[j];
        if linked(tracks_before, &frames[j], mean) >= linked(tracks_before, &frames[j], found.shifts[j]) {
            shifts[j] = mean;
        }
    }
    shifts
}

/// Frame `i`'s tracks as tracks.json keeps them: each target's id and place, its area, and the model's boxes and
/// scores when it gave them.
fn track_frame(i: usize, shift: (f64, f64), tracked: &[Tracked]) -> TrackFrame {
    let boxes: Vec<ModelBox> = tracked.iter().filter_map(|target| target.spot.model).collect();
    let place =
        |target: &Tracked| (target.id, round(target.spot.x, PLACE_DECIMALS), round(target.spot.y, PLACE_DECIMALS));
    TrackFrame {
        i,
        shift,
        t: tracked.iter().map(place).collect(),
        a: tracked.iter().map(|target| target.spot.area).collect(),
        wh: (!boxes.is_empty())
            .then(|| boxes.iter().map(|model| (round(model.w, BOX_DECIMALS), round(model.h, BOX_DECIMALS))).collect()),
        s: (!boxes.is_empty()).then(|| boxes.iter().map(|model| round(model.score, BOX_DECIMALS)).collect()),
    }
}

/// Track ids for the targets of each frame (`follow`). The view's shift of each frame is found first, against the
/// frame before as it was tracked. A spike (`spikes`) is replaced by the mean of the shifts either side when that
/// mean links as many of the frame before's targets as the spike does: a spike that lines up more of them is the
/// camera's own jerk (frames captured unevenly), and stays. Then the frames are tracked with those shifts.
pub fn link(frames: &[Vec<Spot>]) -> Box<[TrackFrame]> {
    let shifts = repaired_shifts(frames, &found_shifts(frames));
    let mut out = Vec::with_capacity(frames.len());
    let mut prev: Box<[Tracked]> = Box::default();
    let mut next_id = 0;
    for (i, (now, &shift)) in frames.iter().zip(shifts.iter()).enumerate() {
        let current = follow(&prev, now, shift, &mut next_id);
        out.push(track_frame(i, shift, &current));
        prev = current;
    }
    out.into_boxed_slice()
}
