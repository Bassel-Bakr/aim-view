//! A tracking run's summary (the old review, python/retired/review.py: `track_summary`, `track_motion`, `what_if`,
//! `stats_length`, `countdown_end`, `tracking_crosshair`, `without_crosshair`): how the crosshair stayed on the target,
//! from the tracks and the camera's turn.
//!
//! In: a tracking run's tracks (src/track.rs), its stats file's header, the camera's readings (src/camera.rs), and the
//! frames where its bots die and where it starts (src/review.rs). Out: the report's `TrackSummary`: the time on target,
//! the losses and the switches between bots, the motion diagnostics (`Motion`) and the what-if estimates.

use std::collections::HashMap;
use std::ops::Range;

use serde::Serialize;

use crate::capped::Capped;
use crate::faint::picked;
use crate::geometry::{K, degrees};
use crate::matching::KillSource;
use crate::optional_fields::OptionalFields;
use crate::python::{hypot, round};
use crate::stats_file::StatsFile;
use crate::statistics::median;
use crate::summary::DIRECTIONS;
use crate::track::{TrackFrame, Tracks};

/// The target's own motion and the camera's turn are smoothed over this long (seconds), and at least this many frames.
const SMOOTH_S: f64 = 0.05;
const MIN_SMOOTH_FRAMES: usize = 3;
/// The crosshair is engaged with the nearest target within this many of its radii, or at least ENGAGED_MIN_DEG.
const ENGAGED_RADII: f64 = 5.0;
const ENGAGED_MIN_DEG: f64 = 2.0;
/// A target moves, and its motion is measured, above this speed.
const MOVING_DEG_S: f64 = 5.0;
/// The aim error counts frames whose nearest target is within this many radii (each at least MIN_RADIUS_DEG).
const NEAR_RADII: f64 = 3.0;
const MIN_RADIUS_DEG: f64 = 0.2;
/// The motion is read when the target is measured moving for this share of the run, and at least MIN_MOVING_S.
const MIN_MOVING_SHARE: f64 = 0.2;
const MIN_MOVING_S: f64 = 10.0;
/// Offsets within this of an edge or of the target's middle are jitter.
const JITTER_DEG: f64 = 0.05;
/// An overshoot lasts at least this many frames (after a 1-frame gap is closed).
const MIN_OVERSHOOT_FRAMES: usize = 2;
/// A direction change of the target: its motion on one axis changes sign between REVERSAL_HALF_S before and after,
/// at REVERSAL_MIN_DEG_S or faster on both sides.
const REVERSAL_HALF_S: f64 = 0.1;
const REVERSAL_MIN_DEG_S: f64 = 8.0;
/// After a direction change: the mouse reacts when it turns the new way faster than REACTION_DEG_S within
/// REACTION_WINDOW_S; the overshoot and the lost time are counted over TURN_WINDOW_S; the motion within STEADY_MARGIN_S
/// of it is not steady.
const REACTION_WINDOW_S: f64 = 0.5;
const REACTION_DEG_S: f64 = 3.0;
const TURN_WINDOW_S: f64 = 0.4;
const STEADY_MARGIN_S: f64 = 0.25;
/// A swing crosses the target's middle to the other side by this share of its radius (or JITTER_DEG).
const SWING_SHARE: f64 = 0.5;
/// Steady motion counts afresh after a gap of more than this many frames.
const MAX_STEADY_GAP_FRAMES: usize = 2;
/// A direction the target moved in for this share of the moving time can be the best one.
const MIN_DIRECTION_SHARE: f64 = 0.05;
/// The crosshair is on a target within this of its box (or of its disc without one).
const INSIDE_MARGIN_DEG: f64 = 0.05;
/// A stretch off the target longer than this (seconds) is a loss; a shorter one is a slip.
const LOSS_S: f64 = 0.1;
/// "Your best 10 seconds" needs a run of this many seconds, and BEST_TRACKED_S of tracking in the window.
const MIN_RUN_SECONDS_FOR_BEST: usize = 20;
const BEST_WINDOW_SECONDS: usize = 10;
const BEST_TRACKED_S: usize = 5;
/// The what-if of switching faster: this much less per switch (seconds).
const FASTER_SWITCH_S: f64 = 0.1;

/// The camera's reading for a frame: the room's move on screen since the frame before (degrees; the camera turned by
/// minus that) and how many tiles agreed on it, or None.
pub type CameraReading = Option<(f64, f64, usize)>;

/// The radius in degrees of a target of `area_px` pixels taken as a disc.
fn disc_radius_deg(area_px: i64) -> f64 {
    degrees(((area_px as f64 / std::f64::consts::PI).sqrt() / K).atan())
}

/// A target's width and height (degrees): the model's box, else its area as a disc.
fn target_size(frame: &TrackFrame, index: usize) -> (f64, f64) {
    match &frame.wh {
        Some(sizes) => sizes[index],
        None => {
            let size = 2.0 * disc_radius_deg(frame.a[index]);
            (size, size)
        }
    }
}

/// The target nearest the crosshair in a frame: its track and center (degrees), the crosshair's offset from its center
/// line (a sphere's center, a capsule's long axis) and that offset's length, and the target's half-width (degrees).
#[derive(Clone, Copy)]
struct Nearest {
    distance: f64,
    track: u32,
    x: f64,
    y: f64,
    line_x: f64,
    line_y: f64,
    radius: f64,
}

fn nearest(frame: &TrackFrame) -> Option<Nearest> {
    let mut best: Option<(f64, usize)> = None;
    for (index, &(_, x, y)) in frame.t.iter().enumerate() {
        let distance = hypot(x, y);
        if best.is_none_or(|(best_distance, _)| distance < best_distance) {
            best = Some((distance, index));
        }
    }
    let (_, index) = best?;
    let (track, x, y) = frame.t[index];
    let (width, height) = target_size(frame, index);
    let line_x = (x.abs() - (width - height).max(0.0) / 2.0).max(0.0).copysign(x);
    let line_y = (y.abs() - (height - width).max(0.0) / 2.0).max(0.0).copysign(y);
    Some(Nearest { distance: hypot(line_x, line_y), track, x, y, line_x, line_y, radius: width.min(height) / 2.0 })
}

/// Whether the crosshair is engaged with a target: within ENGAGED_RADII of its radii, or ENGAGED_MIN_DEG.
fn engaged(target: &Nearest) -> bool {
    target.distance <= (ENGAGED_RADII * target.radius).max(ENGAGED_MIN_DEG)
}

/// The moving mean over `frames` frames (odd, centered) of values with gaps (NaN); NaN where none is in reach.
fn moving_mean(values: &[(f64, f64)], frames: usize) -> Vec<(f64, f64)> {
    let half = frames / 2;
    (0..values.len())
        .map(|i| {
            let window = &values[i.saturating_sub(half)..(i + half + 1).min(values.len())];
            let known: Vec<&(f64, f64)> = window.iter().filter(|value| !value.0.is_nan()).collect();
            if known.is_empty() {
                (f64::NAN, f64::NAN)
            } else {
                let count = known.len() as f64;
                let sum = |part: fn(&(f64, f64)) -> f64| known.iter().map(|&value| part(value)).sum::<f64>();
                (sum(|value| value.0) / count, sum(|value| value.1) / count)
            }
        })
        .collect()
}

/// A vector's x (axis 0) or y (axis 1).
fn component(vector: (f64, f64), axis: usize) -> f64 {
    if axis == 0 { vector.0 } else { vector.1 }
}

/// The direction sector (0 = right, counterclockwise in 45-degree steps) of a motion.
fn sector(motion: (f64, f64)) -> usize {
    ((degrees(motion.1.atan2(motion.0)).rem_euclid(360.0) / 45.0).round_ties_even() as usize) % 8
}

fn median_of(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| median(values))
}

fn count_true(values: &[bool]) -> usize {
    values.iter().filter(|&&value| value).count()
}

/// The share of the values that are true (NaN for none).
fn share_true(values: &[bool]) -> f64 {
    count_true(values) as f64 / values.len() as f64
}

/// The tracking per direction of the target's motion: the share of the moving time, the share on the target, the
/// median distance from its center line and the median offset along the motion.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MotionDirection {
    #[cfg_attr(feature = "ts", ts(as = "crate::summary::Direction"))]
    pub name: &'static str,
    pub share: f64,
    pub on: f64,
    pub distance: f64,
    pub lag: f64,
}

/// What the off-target time went on, in frames (for the what-if estimates).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OffFrames {
    pub ahead: usize,
    pub behind: usize,
    pub turns: usize,
    pub directions: f64,
}

/// The counts behind the swings: swings, corrections, and swings as a share of corrections.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MotionCounts {
    #[cfg_attr(feature = "ts", ts(as = "Option<usize>", optional))]
    pub swing_count: usize,
    #[cfg_attr(feature = "ts", ts(as = "Option<usize>", optional))]
    pub corrections: usize,
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub overcorrect: Option<f64>,
    #[cfg_attr(feature = "ts", ts(as = "Option<OffFrames>", optional))]
    pub frames: OffFrames,
}

/// One of the bot's direction changes: its frame, and the seconds from it until the crosshair was on the bot again
/// (0 when it stayed on; None when it was not back before the next change or the run's end).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TurnBack {
    pub frame: usize,
    pub back: Option<f64>,
}

/// How long the crosshair took to get back on the bot after each of its direction changes (`turns`: their frames, in
/// order): from the change to the first frame on it after the crosshair left it within `window` frames of the change.
/// The search stops at the next change, at a `stop` frame (a bot just died) and at `end`.
fn turns_back(turns: &[usize], inside: &[bool], stop: &[bool], window: usize, end: usize, fps: f64) -> Vec<TurnBack> {
    turns
        .iter()
        .enumerate()
        .map(|(index, &turn)| {
            let next = turns.get(index + 1).map_or(end, |&after| after.min(end));
            let until = (turn..next).find(|&frame| stop[frame]).unwrap_or(next);
            let back = match (turn..until.min(turn + window)).find(|&frame| !inside[frame]) {
                None => Some(0.0),
                Some(left) => (left + 1..until).find(|&frame| inside[frame]).map(|frame| (frame - turn) as f64 / fps),
            };
            TurnBack { frame: turn, back }
        })
        .collect()
}

/// Tracking diagnostics from the target's own motion and the camera's (see the old review's `track_motion`).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Motion {
    pub camera: f64,
    pub target_speed: Option<f64>,
    pub mouse_speed: Option<f64>,
    pub lag: Option<f64>,
    pub lag_ms: Option<f64>,
    pub off_ahead: Option<f64>,
    pub off_behind: Option<f64>,
    pub off_side: Option<f64>,
    pub overshoots: Option<f64>,
    pub overshoot_dist: Option<f64>,
    pub swings: Option<f64>,
    pub reversals: usize,
    pub reaction: Option<f64>,
    pub reversal_overshoot: Option<f64>,
    pub reversal_overshoot_dist: Option<f64>,
    pub by_direction: Vec<MotionDirection>,
    pub error_h: Option<f64>,
    pub error_v: Option<f64>,
    pub seconds: f64,
    /// Each moving frame's offset along the target's motion (positive: ahead of it) and across it, and the target's
    /// radius, in degrees: where the crosshair sat around the target.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<crate::typescript::AroundPoint>>", optional))]
    pub around: Option<Box<[[f64; 3]]>>,
    /// Each direction change of the bot (both axes' together when they fall within 0.2 s), and how long the crosshair
    /// took to get back on it. Not in Python's review.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub turns_back: Option<Vec<TurnBack>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reason: Option<&'static str>,
    #[serde(flatten)]
    pub counts: OptionalFields<MotionCounts>,
}

impl Motion {
    /// A motion with only what is read before the measured frames are known to be enough.
    fn unread(camera: f64, error_h: Option<f64>, error_v: Option<f64>, seconds: f64) -> Motion {
        Motion {
            camera,
            target_speed: None,
            mouse_speed: None,
            lag: None,
            lag_ms: None,
            off_ahead: None,
            off_behind: None,
            off_side: None,
            overshoots: None,
            overshoot_dist: None,
            swings: None,
            reversals: 0,
            reaction: None,
            reversal_overshoot: None,
            reversal_overshoot_dist: None,
            by_direction: Vec::new(),
            error_h,
            error_v,
            seconds,
            around: None,
            turns_back: None,
            reason: None,
            counts: OptionalFields(None),
        }
    }
}

/// What `track_motion` reads per frame (None or NaN outside its span): the target nearest the crosshair, the target's
/// own motion (its move on screen less the room's) and the mouse's (the camera's turn), both smoothed (degrees a
/// second), the own motion's speed, and the frames measured: the target moving, the crosshair engaged with it, and no
/// bot just died.
struct MotionFrames {
    nearest: Vec<Option<Nearest>>,
    own: Vec<(f64, f64)>,
    mouse: Vec<(f64, f64)>,
    speed: Vec<f64>,
    moving: Vec<usize>,
}

impl MotionFrames {
    fn new(frames: &[TrackFrame], fps: f64, camera: &[CameraReading], span: Range<usize>, switching: &[bool]) -> Self {
        let count = frames.len();
        let mut nearest_target: Vec<Option<Nearest>> = vec![None; count];
        for i in span.clone() {
            nearest_target[i] = nearest(&frames[i]);
        }
        let unknown = (f64::NAN, f64::NAN);
        let (mut own, mut mouse) = (vec![unknown; count], vec![unknown; count]);
        for i in span.start + 1..span.end {
            let Some(Some(reading)) = camera.get(i) else { continue };
            mouse[i] = (-reading.0 * fps, -reading.1 * fps);
            if let (Some(before), Some(now)) = (nearest_target[i - 1], nearest_target[i])
                && before.track == now.track
            {
                own[i] = ((now.x - before.x - reading.0) * fps, (now.y - before.y - reading.1) * fps);
            }
        }
        let smooth_frames = ((SMOOTH_S * fps).round_ties_even() as usize | 1).max(MIN_SMOOTH_FRAMES);
        let (own, mouse) = (moving_mean(&own, smooth_frames), moving_mean(&mouse, smooth_frames));
        let speed: Vec<f64> = own.iter().map(|motion| motion.0.hypot(motion.1)).collect();
        let measured = |i: usize| {
            nearest_target[i].is_some_and(|target| engaged(&target))
                && !speed[i].is_nan()
                && speed[i] > MOVING_DEG_S
                && !switching[i]
        };
        let moving = span.filter(|&i| measured(i)).collect();
        MotionFrames { nearest: nearest_target, own, mouse, speed, moving }
    }
}

/// Along and across the target's motion: the crosshair's offset from its center line in each measured frame (along:
/// positive ahead of it) and the target's radius there (NaN elsewhere), and each measured frame's [along, across,
/// radius].
struct CenterLineOffsets {
    along: Vec<f64>,
    radius: Vec<f64>,
    around: Vec<[f64; 3]>,
}

fn offsets_from_center_line(motion: &MotionFrames, count: usize) -> CenterLineOffsets {
    let (mut along, mut radius) = (vec![f64::NAN; count], vec![f64::NAN; count]);
    let mut around = Vec::with_capacity(motion.moving.len());
    for &i in &motion.moving {
        let target = motion.nearest[i].unwrap();
        let (unit_x, unit_y) = (motion.own[i].0 / motion.speed[i], motion.own[i].1 / motion.speed[i]);
        let (to_x, to_y) = (-target.line_x, -target.line_y);
        along[i] = to_x * unit_x + to_y * unit_y;
        radius[i] = target.radius;
        around.push([along[i], -to_x * unit_y + to_y * unit_x, target.radius]);
    }
    CenterLineOffsets { along, radius, around }
}

/// The target's and the mouse's median speeds and the crosshair's median lag along the motion (degrees, and ms).
fn read_speeds(out: &mut Motion, motion: &MotionFrames, along: &[f64]) {
    let over_moving = |value: &dyn Fn(usize) -> f64| motion.moving.iter().map(|&i| value(i)).collect::<Vec<f64>>();
    out.target_speed = median_of(&over_moving(&|i| motion.speed[i]));
    let mouse_speed = over_moving(&|i| motion.mouse[i].0.hypot(motion.mouse[i].1));
    out.mouse_speed = median_of(&mouse_speed.into_iter().filter(|speed| !speed.is_nan()).collect::<Vec<_>>());
    out.lag = median_of(&over_moving(&|i| along[i]));
    out.lag_ms = median_of(&over_moving(&|i| along[i] / motion.speed[i])).map(|lag| lag * 1000.0);
}

/// Where the crosshair was when off the target (`off`, measured frames): ahead of it, behind it, or to its side.
fn read_off_sides(out: &mut Motion, off: &[usize], along: &[f64], radius: &[f64]) {
    if off.is_empty() {
        return;
    }
    let ahead = off.iter().filter(|&&i| along[i] > radius[i]).count() as f64;
    let behind = off.iter().filter(|&&i| along[i] < -radius[i]).count() as f64;
    let total = off.len() as f64;
    (out.off_ahead, out.off_behind, out.off_side) =
        (Some(ahead / total), Some(behind / total), Some(1.0 - (ahead + behind) / total));
}

/// Overshoots: stretches of MIN_OVERSHOOT_FRAMES or more ahead of the target's leading edge by more than JITTER_DEG,
/// a second; and the median of each one's farthest point past the edge.
fn read_overshoots(out: &mut Motion, moving: &[usize], along: &[f64], radius: &[f64]) {
    let mut past = vec![false; along.len()];
    for &i in moving {
        past[i] = along[i] > radius[i] + JITTER_DEG;
    }
    let episodes: Vec<(usize, usize)> = crate::scipy::runs(&crate::scipy::close_line(&past, 1))
        .into_iter()
        .filter(|(a, b)| b - a >= MIN_OVERSHOOT_FRAMES)
        .collect();
    out.overshoots = Some(episodes.len() as f64 / out.seconds);
    if !episodes.is_empty() {
        let farthest = |(a, b): (usize, usize)| {
            (a..b).filter(|&i| !along[i].is_nan()).map(|i| along[i] - radius[i]).fold(f64::NEG_INFINITY, f64::max)
        };
        out.overshoot_dist = median_of(&episodes.iter().map(|&episode| farthest(episode)).collect::<Vec<f64>>());
    }
}

/// A direction change of the target: the frame its motion is slowest on, the axis (0: x, 1: y), and the sign of the
/// motion after it.
#[derive(Clone, Copy)]
struct DirectionChange {
    frame: usize,
    axis: usize,
    sign: f64,
}

/// The target's direction changes, on each axis: its motion `half` frames before and after a frame has opposite signs,
/// both at REVERSAL_MIN_DEG_S or faster, at least 2 x `half` frames after the last change, with the crosshair engaged
/// and no bot just died.
fn direction_changes(
    motion: &MotionFrames,
    span: Range<usize>,
    switching: &[bool],
    half: usize,
) -> Vec<DirectionChange> {
    let mut changes = Vec::new();
    for axis in 0..2 {
        let along_axis = |i: usize| component(motion.own[i], axis);
        let mut last: i64 = -1_000_000_000;
        for i in span.start + half..span.end.saturating_sub(half) {
            let (before, after) = (along_axis(i - half), along_axis(i + half));
            let too_soon = (i as i64) - last < 2 * half as i64;
            let slow = before.abs().min(after.abs()) < REVERSAL_MIN_DEG_S;
            if before.is_nan() || after.is_nan() || before * after >= 0.0 || slow || too_soon {
                continue;
            }
            // the change: where it is slowest (NumPy's argmin: the first NaN, if any)
            let window = i - half..=i + half;
            let speed_at = |frame: usize| along_axis(frame).abs();
            let slower = |best: usize, frame: usize| if speed_at(frame) < speed_at(best) { frame } else { best };
            let first_unknown = window.clone().find(|&frame| along_axis(frame).is_nan());
            let slowest = first_unknown.unwrap_or_else(|| window.fold(i - half, slower));
            let readable = motion.nearest[slowest].is_some_and(|target| engaged(&target)) && !switching[slowest];
            if (slowest as i64) - last >= 2 * half as i64 && readable {
                changes.push(DirectionChange { frame: slowest, axis, sign: if after > 0.0 { 1.0 } else { -1.0 } });
                last = slowest as i64;
            }
        }
    }
    changes
}

/// For each direction change: the mouse's reaction time (ms), when it turns the new way faster than REACTION_DEG_S
/// within `reaction_frames`; and how far past the target's edge the crosshair went within `turn_frames` (degrees).
fn reactions_and_overshoots(
    changes: &[DirectionChange],
    motion: &MotionFrames,
    end: usize,
    reaction_frames: usize,
    turn_frames: usize,
    fps: f64,
) -> (Vec<f64>, Vec<f64>) {
    let (mut reactions, mut overshoots) = (Vec::new(), Vec::new());
    for &DirectionChange { frame, axis, sign } in changes {
        let turned = |later: &usize| {
            let mouse = component(motion.mouse[*later], axis);
            !mouse.is_nan() && mouse * sign > REACTION_DEG_S
        };
        if let Some(reacted) = (frame..end.min(frame + reaction_frames)).find(turned) {
            reactions.push((reacted - frame) as f64 / fps * 1000.0);
        }
        let past: Vec<f64> = (frame..end.min(frame + turn_frames))
            .filter_map(|later| motion.nearest[later])
            .map(|target| component((target.line_x, target.line_y), axis) * sign - target.radius)
            .collect();
        if !past.is_empty() {
            overshoots.push(past.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        }
    }
    (reactions, overshoots)
}

/// The frames of steady motion: measured, and not within `margin` frames of a direction change.
fn steady_frames(moving: &[usize], changes: &[DirectionChange], margin: usize, count: usize) -> Vec<bool> {
    let mut steady = vec![false; count];
    for &i in moving {
        steady[i] = true;
    }
    for change in changes {
        for frame in &mut steady[change.frame.saturating_sub(margin)..(change.frame + margin).min(count)] {
            *frame = false;
        }
    }
    steady
}

/// Swings: the crosshair crossing the target's middle to the other side by SWING_SHARE of its radius (or JITTER_DEG)
/// or more; corrections: each turn of the crosshair back toward the middle along the motion (jitter under JITTER_DEG
/// ignored). Both in steady motion only, and counted afresh after a gap.
fn swings_and_corrections(moving: &[usize], steady: &[bool], along: &[f64], radius: &[f64]) -> (usize, usize) {
    let (mut swings, mut side, mut corrections) = (0usize, 0i32, 0usize);
    // the way the offset last moved (0 before it moved past the jitter) and where that move went to
    let mut drift: Option<(i32, f64)> = None;
    let mut previous: Option<usize> = None;
    for &i in moving {
        if !steady[i] || previous.is_some_and(|before| i - before > MAX_STEADY_GAP_FRAMES) {
            (side, drift) = (0, None);
        }
        previous = Some(i);
        if !steady[i] {
            continue;
        }
        let cut = (radius[i] * SWING_SHARE).max(JITTER_DEG);
        let now = if along[i] > cut { 1 } else if along[i] < -cut { -1 } else { 0 };
        if now != 0 && side != 0 && now != side {
            swings += 1;
        }
        if now != 0 {
            side = now;
        }
        let offset = along[i];
        drift = match drift {
            None => Some((0, offset)),
            Some((0, turn)) if (offset - turn).abs() > JITTER_DEG => Some((if offset > turn { 1 } else { -1 }, offset)),
            Some((way, turn)) if way != 0 && (offset - turn) * way as f64 > 0.0 => Some((way, offset)),
            Some((way, turn)) if way != 0 && (offset - turn).abs() > JITTER_DEG => {
                corrections += 1;
                Some((-way, offset))
            }
            same => same,
        };
    }
    (swings, corrections)
}

/// The frames within `turn_frames` after each direction change.
fn turn_windows(changes: &[DirectionChange], turn_frames: usize, count: usize) -> Vec<bool> {
    let mut in_turn = vec![false; count];
    for change in changes {
        for frame in &mut in_turn[change.frame..(change.frame + turn_frames).min(count)] {
            *frame = true;
        }
    }
    in_turn
}

/// Each direction sector's measured frames, and those on the target.
fn direction_sectors(motion: &MotionFrames, inside: &[bool]) -> [(usize, usize); 8] {
    let mut sectors = [(0usize, 0usize); 8];
    for &i in &motion.moving {
        let index = sector(motion.own[i]);
        sectors[index].0 += 1;
        sectors[index].1 += inside[i] as usize;
    }
    sectors
}

/// The frames off the target that tracking every direction like the best one (among those the target moved in for
/// MIN_DIRECTION_SHARE of the time) would win back.
fn lost_to_directions(sectors: &[(usize, usize); 8], moving: usize) -> f64 {
    let best = sectors
        .iter()
        .filter(|sector| sector.0 > 0 && sector.0 as f64 >= MIN_DIRECTION_SHARE * moving as f64)
        .map(|sector| sector.1 as f64 / sector.0 as f64)
        .fold(0.0, f64::max);
    sectors.iter().filter(|sector| sector.0 > 0).map(|sector| (best * sector.0 as f64 - sector.1 as f64).max(0.0)).sum()
}

/// The tracking in each direction of the target's motion (`MotionDirection`), in DIRECTIONS' order.
fn by_direction(motion: &MotionFrames, along: &[f64], inside: &[bool]) -> Vec<MotionDirection> {
    let moving = &motion.moving;
    DIRECTIONS
        .iter()
        .enumerate()
        .filter_map(|(sector_index, &name)| {
            let in_sector = |&&i: &&usize| sector(motion.own[i]) == sector_index;
            let frames: Vec<usize> = moving.iter().filter(in_sector).copied().collect();
            (!frames.is_empty()).then(|| MotionDirection {
                name,
                share: frames.len() as f64 / moving.len() as f64,
                on: frames.iter().filter(|&&i| inside[i]).count() as f64 / frames.len() as f64,
                distance: median(&frames.iter().map(|&i| motion.nearest[i].unwrap().distance).collect::<Vec<_>>()),
                lag: median(&frames.iter().map(|&i| along[i]).collect::<Vec<_>>()),
            })
        })
        .collect()
}

/// Tracking diagnostics, frames `start` to `end`: the target is the track nearest the crosshair; its own motion is its
/// move on screen less the room's, the mouse's is the camera's turn, both smoothed over SMOOTH_S. Measured while the
/// target moves (over MOVING_DEG_S), the crosshair is engaged with it, and no bot has just died (`switching`).
pub fn track_motion(
    frames: &[TrackFrame],
    fps: f64,
    camera: &[CameraReading],
    start: usize,
    end: usize,
    inside: &[bool],
    switching: &[bool],
) -> Motion {
    let (count, end) = (frames.len(), end.min(frames.len()));
    let span_frames = end.saturating_sub(start);
    let motion = MotionFrames::new(frames, fps, camera, start..end, switching);
    let near: Vec<Nearest> = (start..end)
        .filter_map(|i| motion.nearest[i])
        .filter(|target| target.distance <= NEAR_RADII * target.radius.max(MIN_RADIUS_DEG))
        .collect();
    let read = (start..end).filter(|&i| camera.get(i).is_some_and(Option::is_some)).count();
    let error = |part: fn(&Nearest) -> f64| median_of(&near.iter().map(part).collect::<Vec<_>>());
    let moving = &motion.moving;
    let mut out = Motion::unread(
        read as f64 / span_frames.max(1) as f64,
        error(|target| target.line_x.abs()),
        error(|target| target.line_y.abs()),
        moving.len() as f64 / fps,
    );
    if (moving.len() as f64) < fps * (MIN_MOVING_SHARE * span_frames as f64 / fps).max(MIN_MOVING_S) {
        out.reason = Some("too little tracking of a moving target to read");
        return out;
    }
    let CenterLineOffsets { along, radius, around } = offsets_from_center_line(&motion, count);
    out.around = Some(around.into_boxed_slice());
    read_speeds(&mut out, &motion, &along);
    let off: Vec<usize> = moving.iter().copied().filter(|&i| !inside[i]).collect();
    read_off_sides(&mut out, &off, &along, &radius);
    read_overshoots(&mut out, moving, &along, &radius);
    let half = (REVERSAL_HALF_S * fps).round_ties_even() as usize;
    let changes = direction_changes(&motion, start..end, switching, half);
    let frames_of = |seconds: f64| (seconds * fps) as usize;
    let (reaction_frames, turn_frames) = (frames_of(REACTION_WINDOW_S), frames_of(TURN_WINDOW_S));
    let mut change_frames: Vec<usize> = changes.iter().map(|change| change.frame).collect();
    change_frames.sort_unstable();
    change_frames.dedup_by(|a, b| *a - *b < 2 * half);
    out.turns_back = Some(turns_back(&change_frames, inside, switching, turn_frames, end, fps));
    let (reactions, overshoots) = reactions_and_overshoots(&changes, &motion, end, reaction_frames, turn_frames, fps);
    let steady = steady_frames(moving, &changes, frames_of(STEADY_MARGIN_S), count);
    let (swings, corrections) = swings_and_corrections(moving, &steady, &along, &radius);
    let far: Vec<f64> = overshoots.iter().copied().filter(|&past| past > JITTER_DEG).collect();
    out.reversals = changes.len();
    out.reaction = median_of(&reactions);
    out.reversal_overshoot = (!overshoots.is_empty()).then(|| far.len() as f64 / overshoots.len() as f64);
    out.reversal_overshoot_dist = median_of(&far);
    out.swings = Some(swings as f64 / (count_true(&steady) as f64 / fps).max(1e-9));
    let in_turn = turn_windows(&changes, turn_frames, count);
    *out.counts = Some(MotionCounts {
        swing_count: swings,
        corrections,
        overcorrect: (corrections > 0).then(|| swings as f64 / corrections as f64),
        frames: OffFrames {
            ahead: off.iter().filter(|&&i| along[i] > radius[i]).count(),
            behind: off.iter().filter(|&&i| along[i] < -radius[i]).count(),
            turns: (start..end).filter(|&i| in_turn[i] && !inside[i] && !switching[i]).count(),
            directions: lost_to_directions(&direction_sectors(&motion, inside), moving.len()),
        },
    });
    out.by_direction = by_direction(&motion, &along, inside);
    out
}

/// One what-if estimate: how much of the run's time on target one change would add (a share of the run).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WhatIf {
    pub what: &'static str,
    pub gain: f64,
    pub how: String,
}

/// The shares of the run that need its frames: on a target over the whole run (switching included), and what the
/// lost stretches and the shorter slips cost.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RunShares {
    #[cfg_attr(feature = "ts", ts(as = "Option<f64>", optional))]
    pub on_all: f64,
    #[cfg_attr(feature = "ts", ts(as = "Option<f64>", optional))]
    pub lost_cost: f64,
    #[cfg_attr(feature = "ts", ts(as = "Option<f64>", optional))]
    pub slip_cost: f64,
}

/// The user's faint-target cut-off, when on: its offset, the score it cuts at, and how many tracks it left out.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FaintCut {
    pub offset: f64,
    pub cut: Option<f64>,
    pub tracks: usize,
}

/// Where the tracking run's kill times came from.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackInfo {
    pub source: KillSource,
}

/// A tracking run's summary (see the old review's `track_summary`).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackSummary {
    pub scenario: Option<String>,
    pub score: Option<f64>,
    pub accuracy: Option<f64>,
    pub fps_avg: Option<f64>,
    pub mode: crate::summary::Mode,
    pub sens: Option<String>,
    pub on_target: Option<f64>,
    pub error: Option<f64>,
    pub lost: Option<f64>,
    pub back: Option<f64>,
    pub longest_off: Option<f64>,
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::SecondShares>"))]
    pub per_second: Vec<[f64; 2]>,
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub bots: usize,
    /// Per bot death: [death, back on a target, first frame a target shows].
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::Switch>"))]
    pub switches: Vec<[usize; 3]>,
    pub to_next: Option<f64>,
    pub waiting: Option<f64>,
    pub onto: Option<f64>,
    pub switching: Option<f64>,
    #[serde(flatten)]
    pub shares: OptionalFields<RunShares>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub motion: Option<Motion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub what_if: Option<Vec<WhatIf>>,
    pub faint: Option<FaintCut>,
    pub info: TrackInfo,
}

/// A run's length in seconds from its stats file: the file is named for the moment the run ended (to the second) and
/// holds its start (to the millisecond), so the length is their gap, rounded up.
pub fn stats_length(name: &str, file: &StatsFile) -> Option<f64> {
    let stem = name.strip_suffix(".csv").unwrap_or(name);
    let stamp = stem.rsplit(" - ").next()?.replace(" Stats", "");
    let (date, time) = stamp.split_once('-')?;
    let numbers = |text: &str| text.split('.').map(|part| part.parse().ok()).collect::<Option<Vec<i64>>>();
    let (date, time) = (numbers(date)?, numbers(time)?);
    let (&[_, _, _], &[hours, minutes, seconds]) = (&date[..], &time[..]) else { return None };
    let end_micros = ((hours * 60 + minutes) * 60 + seconds) * 1_000_000;
    let start_micros = file.start_micros()?;
    let length = ((end_micros - start_micros) as f64 / 1e6).rem_euclid(86400.0);
    (0.0 < length && length < 3600.0).then(|| length.ceil())
}

/// The frame a run starts on: the one after the last frame showing KovaaK's countdown (`showing`, per frame) in the
/// first `until` seconds, or None.
pub fn countdown_end(showing: &[bool], fps: f64, until: f64) -> Option<usize> {
    let until: f64 = format!("{until:.2}").parse().unwrap();
    let before_until = |&(frame, _): &(usize, &bool)| (frame as f64) / fps < until;
    showing.iter().enumerate().take_while(before_until).filter(|&(_, &shows)| shows).map(|(frame, _)| frame + 1).last()
}

/// Where a detector marks the crosshair in a tracking run: the points its box sits on (degrees from the crosshair),
/// and the box's width and height (degrees).
struct CrosshairBox {
    points: Capped<(f64, f64), 3>,
    size: (f64, f64),
}

/// A box near the crosshair: its center and its width and height (degrees).
type NearBox = (f64, f64, f64, f64);

/// `crosshair_box`'s search: boxes within NEAR_DEG of the crosshair, in a histogram of STEP_DEG bins.
const NEAR_DEG: f64 = 0.5;
const STEP_DEG: f64 = 0.01;
/// The fewest boxes near the crosshair for a search, and the fewest in a pile (with a share of all the frames: the
/// first pile's, then the others').
const MIN_NEAR_BOXES: usize = 30;
const MIN_PILE_BOXES: f64 = 25.0;
const FIRST_PILE_SHARE: f64 = 0.05;
const MORE_PILE_SHARE: f64 = 0.02;
/// More piles are looked for within this of the first (degrees); a pile's point is its boxes' mean within POINT_DEG of
/// its bin, and its bins within CLEAR_DEG of that point are emptied.
const MORE_PILES_DEG: f64 = 0.3;
const POINT_DEG: f64 = 0.02;
const CLEAR_DEG: f64 = 0.06;
/// A box is the crosshair's within this of one of its points, with a width and a height within SAME_SIZE_SHARE.
const ON_POINT_DEG: f64 = 0.1;
const SAME_SIZE_SHARE: f64 = 0.2;

/// NumPy's histogram2d of the boxes' centers over `bins` x `bins` bins from -NEAR_DEG to NEAR_DEG: edges as `linspace`
/// makes them, a value on an edge in the bin above.
fn center_histogram(boxes: &[NearBox], bins: usize) -> Vec<f64> {
    let step = 2.0 * NEAR_DEG / bins as f64;
    let mut edges: Vec<f64> = (0..=bins).map(|edge| edge as f64 * step + -NEAR_DEG).collect();
    edges[bins] = NEAR_DEG;
    let bin = |value: f64| edges.partition_point(|&edge| edge <= value) - 1;
    let mut counts = vec![0.0; bins * bins];
    for &(x, y, _, _) in boxes {
        counts[bin(x) * bins + bin(y)] += 1.0;
    }
    counts
}

/// The bin whose count with its 8 neighbours' (wrapping round, as np.roll does) is the largest, the first such: after
/// the first point, only bins within MORE_PILES_DEG of it. Its row, column and count.
fn densest_bin(counts: &[f64], centers: &[f64], first: Option<&(f64, f64)>) -> (usize, usize, f64) {
    let bins = centers.len();
    let mut best = (0, 0, f64::NEG_INFINITY);
    for row in 0..bins {
        for column in 0..bins {
            let mut total = 0.0;
            if first.is_none_or(|point| (centers[row] - point.0).hypot(centers[column] - point.1) <= MORE_PILES_DEG) {
                for row_step in [bins - 1, 0, 1] {
                    for column_step in [bins - 1, 0, 1] {
                        total += counts[((row + row_step) % bins) * bins + (column + column_step) % bins];
                    }
                }
            }
            if total > best.2 {
                best = (row, column, total);
            }
        }
    }
    best
}

/// The crosshair's box in a tracking run (the old review's `tracking_crosshair`): piles of boxes within a hundredth of
/// a degree of a point, the first in FIRST_PILE_SHARE of all the frames or more, then up to two more within
/// MORE_PILES_DEG of it in MORE_PILE_SHARE or more. A bot never stays that still. Only boxes with a size count. Its
/// size: the median width and height of the boxes within POINT_DEG of the first point. None without such points.
fn crosshair_box(frames: &[TrackFrame]) -> Option<CrosshairBox> {
    let boxes: Vec<NearBox> = frames
        .iter()
        .filter_map(|frame| {
            let sizes = frame.wh.as_ref()?;
            Some(frame.t.iter().zip(sizes).map(|(&(_, x, y), &(width, height))| (x, y, width, height)))
        })
        .flatten()
        .filter(|&(x, y, _, _)| hypot(x, y) < NEAR_DEG)
        .collect();
    if boxes.len() < MIN_NEAR_BOXES {
        return None;
    }
    let bins = (2.0 * NEAR_DEG / STEP_DEG).round_ties_even() as usize;
    let mut counts = center_histogram(&boxes, bins);
    let centers: Vec<f64> = (0..bins).map(|bin| -NEAR_DEG + (bin as f64 + 0.5) * STEP_DEG).collect();
    let within = |point: (f64, f64)| {
        boxes.iter().filter(move |near| (near.0 - point.0).hypot(near.1 - point.1) < POINT_DEG).collect::<Vec<_>>()
    };
    let mean = |piled: &[&NearBox], part: fn(&NearBox) -> f64| {
        piled.iter().map(|&near| part(near)).sum::<f64>() / piled.len() as f64
    };
    let (mut points, mut size): (Capped<(f64, f64), 3>, (f64, f64)) = (Capped::new(), (0.0, 0.0));
    for _ in 0..3 {
        let (row, column, total) = densest_bin(&counts, &centers, points.first());
        let need = if points.is_empty() { FIRST_PILE_SHARE } else { MORE_PILE_SHARE };
        if total < (need * frames.len() as f64).max(MIN_PILE_BOXES) {
            break;
        }
        let piled = within((centers[row], centers[column]));
        if piled.is_empty() {
            break;
        }
        let point = (mean(&piled, |near| near.0), mean(&piled, |near| near.1));
        if points.is_empty() {
            let at_point = within(point);
            let side = |part: fn(&NearBox) -> f64| median(&at_point.iter().map(|&near| part(near)).collect::<Vec<_>>());
            size = (side(|near| near.2), side(|near| near.3));
        }
        points.push(point);
        for (row, &center_x) in centers.iter().enumerate() {
            for (column, &center_y) in centers.iter().enumerate() {
                if (center_x - point.0).hypot(center_y - point.1) < CLEAR_DEG {
                    counts[row * bins + column] = 0.0;
                }
            }
        }
    }
    (!points.is_empty()).then_some(CrosshairBox { points, size })
}

/// The frames without the boxes a detector puts on the crosshair in a tracking run (the old review's
/// `without_crosshair`): those within ON_POINT_DEG of one of its points (`crosshair_box`), with a width and a height
/// within SAME_SIZE_SHARE of its box's. None when the run has no such points.
fn without_crosshair(frames: &[TrackFrame]) -> Option<Vec<TrackFrame>> {
    let CrosshairBox { points, size: (crosshair_width, crosshair_height) } = crosshair_box(frames)?;
    let crosshair = |x: f64, y: f64, width: f64, height: f64| {
        points.iter().any(|&(a, b)| hypot(x - a, y - b) < ON_POINT_DEG)
            && (width - crosshair_width).abs() <= SAME_SIZE_SHARE * crosshair_width
            && (height - crosshair_height).abs() <= SAME_SIZE_SHARE * crosshair_height
    };
    let frames = frames
        .iter()
        .map(|frame| {
            let Some(sizes) = &frame.wh else { return frame.clone() };
            let keep: Vec<usize> = (frame.t.iter().zip(sizes).enumerate())
                .filter(|&(_, (&(_, x, y), &(width, height)))| !crosshair(x, y, width, height))
                .map(|(index, _)| index)
                .collect();
            TrackFrame {
                i: frame.i,
                shift: frame.shift,
                t: picked(&frame.t, &keep),
                a: picked(&frame.a, &keep),
                wh: Some(picked(sizes, &keep)),
                s: frame.s.as_ref().map(|scores| picked(scores, &keep)),
            }
        })
        .collect();
    Some(frames)
}

/// A frame's nearest target: its distance from the crosshair and its half-width (degrees), or None.
type NearestTarget = Option<(f64, f64)>;

/// Each frame's nearest target and whether the crosshair is on a target (within INSIDE_MARGIN_DEG of its box, or of its
/// disc without one).
fn nearest_and_inside(frames: &[TrackFrame]) -> (Box<[NearestTarget]>, Box<[bool]>) {
    let mut near: Vec<NearestTarget> = Vec::with_capacity(frames.len());
    let mut inside = Vec::with_capacity(frames.len());
    for frame in frames {
        let (mut best, mut on): (NearestTarget, bool) = (None, false);
        for (index, (&(_, x, y), &area)) in frame.t.iter().zip(&frame.a).enumerate() {
            let (distance, radius) = match &frame.wh {
                Some(sizes) => {
                    let (width, height) = sizes[index];
                    let within = |offset: f64, size: f64| offset.abs() <= size / 2.0 + INSIDE_MARGIN_DEG;
                    on = on || (within(x, width) && within(y, height));
                    let line_x = (x.abs() - (width - height).max(0.0) / 2.0).max(0.0);
                    let line_y = (y.abs() - (height - width).max(0.0) / 2.0).max(0.0);
                    (hypot(line_x, line_y), width.min(height) / 2.0)
                }
                None => {
                    let (radius, distance) = (disc_radius_deg(area), hypot(x, y));
                    on = on || distance <= radius + INSIDE_MARGIN_DEG;
                    (distance, radius)
                }
            };
            if best.is_none_or(|nearest| distance < nearest.0) {
                best = Some((distance, radius));
            }
        }
        near.push(best);
        inside.push(on);
    }
    (near.into_boxed_slice(), inside.into_boxed_slice())
}

/// A tracking run's summary with only its stats file's facts filled in.
fn unmeasured_summary(meta: &HashMap<String, String>, source: KillSource) -> TrackSummary {
    let number = |key: &str| {
        meta.get(key).filter(|value| !value.is_empty()).and_then(|value| value.trim().parse::<f64>().ok())
    };
    let (hits, misses) = (number("Hit Count"), number("Miss Count"));
    TrackSummary {
        scenario: meta.get("Scenario").cloned(),
        score: number("Score"),
        accuracy: match (hits, misses) {
            (Some(hits), Some(misses)) if hits + misses > 0.0 => Some(hits / (hits + misses)),
            _ => None,
        },
        fps_avg: number("Avg FPS"),
        mode: crate::summary::Mode::Track,
        sens: meta.get("Horiz Sens").filter(|value| !value.is_empty()).map(|horizontal| {
            format!("{horizontal} {}", meta.get("Sens Scale").map(String::as_str).unwrap_or("None"))
        }),
        on_target: None,
        error: None,
        lost: None,
        back: None,
        longest_off: None,
        per_second: Vec::new(),
        start: None,
        end: None,
        bots: 0,
        switches: Vec::new(),
        to_next: None,
        waiting: None,
        onto: None,
        switching: None,
        shares: OptionalFields(None),
        motion: None,
        what_if: None,
        faint: None,
        info: TrackInfo { source },
    }
}

/// The run's frames (first, end): from its start when known, else its first frame with a target, to its time limit
/// (`limit`, seconds), else its last frame with a target; with a limit and no start, it ends at its last frame on a
/// target and starts its length before. It goes on past the limit to its last bot's death. None without a target.
fn run_span(
    near: &[NearestTarget],
    inside: &[bool],
    fps: f64,
    limit: Option<f64>,
    start: Option<i64>,
    deaths: &[i64],
) -> Option<(usize, usize)> {
    let seen: Vec<usize> = (0..near.len()).filter(|&i| near[i].is_some()).collect();
    let (Some(&first), Some(&last_seen)) = (seen.first(), seen.last()) else { return None };
    let frames_of = |seconds: f64| (seconds * fps).round_ties_even() as usize;
    let limit = limit.filter(|&limit| limit != 0.0);
    let (mut from, mut to) = (first, last_seen + 1);
    if let Some(start) = start {
        from = start.max(0) as usize;
        if let Some(limit) = limit {
            to = near.len().min(from + frames_of(limit));
        }
    } else if let Some(limit) = limit {
        // nothing places the start: the run ends at its last frame on a target, and starts its length before
        let last = (0..inside.len()).rev().find(|&i| inside[i]).unwrap_or(to - 1);
        to = last + 1;
        from = first.max(to.saturating_sub(frames_of(limit)));
    }
    if let Some(&last_death) = deaths.iter().max()
        && last_death >= to as i64
    {
        // the run went on past the limit given: up to its last death
        to = near.len().min(last_death as usize + fps as usize);
    }
    Some((from, to))
}

/// The bots' deaths within the run, each with the frame the crosshair is back on a target and the first frame a target
/// shows; and the frames switching between bots (from a death to back on a target).
fn bot_switches(
    deaths: &[i64],
    span: Range<usize>,
    inside: &[bool],
    frames: &[TrackFrame],
) -> (Vec<[usize; 3]>, Vec<bool>) {
    let mut switching = vec![false; frames.len()];
    let in_span = |&&death: &&i64| span.start as i64 <= death && death < span.end as i64;
    let mut died: Vec<usize> = deaths.iter().filter(in_span).map(|&death| death as usize).collect();
    died.sort();
    died.dedup();
    let mut switches = Vec::with_capacity(died.len());
    for (index, &death) in died.iter().enumerate() {
        let next = died.get(index + 1).copied().unwrap_or(span.end);
        let back_on = (death + 1..next).find(|&frame| inside[frame]).unwrap_or(next);
        let shows = |&frame: &usize| frame < frames.len() && !frames[frame].t.is_empty();
        let seen = (death + 1..=back_on).find(shows).unwrap_or(back_on);
        for frame in &mut switching[death..back_on] {
            *frame = true;
        }
        switches.push([death, back_on, seen]);
    }
    (switches, switching)
}

/// Each second's share on a target and share switching between bots, to 3 decimals.
fn per_second(on: &[bool], switching: &[bool], fps: f64) -> Vec<[f64; 2]> {
    let second = (fps.round_ties_even() as usize).max(1);
    (0..on.len())
        .step_by(second)
        .map(|from| {
            let to = (from + second).min(on.len());
            [round(share_true(&on[from..to]), 3), round(share_true(&switching[from..to]), 3)]
        })
        .collect()
}

/// The switches' medians: to the next bot, waiting for it to show, and onto it once shown (seconds); and the share of
/// the run spent switching.
fn read_switches(summary: &mut TrackSummary, switching: &[bool], fps: f64) {
    let switches = &summary.switches;
    let median_gap = |gap: fn(&[usize; 3]) -> usize| {
        median(&switches.iter().map(|switch| gap(switch) as f64).collect::<Vec<_>>()) / fps
    };
    let (to_next, waiting, onto) = (
        median_gap(|switch| switch[1] - switch[0]),
        median_gap(|switch| switch[2] - switch[0]),
        median_gap(|switch| switch[1] - switch[2]),
    );
    summary.bots = summary.switches.len();
    (summary.to_next, summary.waiting, summary.onto) = (Some(to_next), Some(waiting), Some(onto));
    summary.switching = Some(share_true(switching));
}

/// How the crosshair stayed on the target in a tracking run. `limit`: the run's length (seconds); `start`: its first
/// frame when known; `deaths`: the frames where bots die; `camera`: the camera's readings. The boxes a detector puts on
/// the crosshair are left out first (`without_crosshair`).
pub fn track_summary(
    tracks: &Tracks,
    meta: &HashMap<String, String>,
    limit: Option<f64>,
    camera: Option<&[CameraReading]>,
    deaths: &[i64],
    start: Option<i64>,
    source: KillSource,
) -> TrackSummary {
    let kept = without_crosshair(&tracks.frames);
    let (frames, fps): (&[TrackFrame], f64) = (kept.as_deref().unwrap_or(&tracks.frames), tracks.fps);
    let (near, inside) = nearest_and_inside(frames);
    let mut summary = unmeasured_summary(meta, source);
    let Some((first, end)) = run_span(&near, &inside, fps, limit, start, deaths) else { return summary };
    let (switches, switching_frames) = bot_switches(deaths, first..end, &inside, frames);
    summary.switches = switches;
    let span = first..end.max(first);
    let tracking: Vec<bool> = span.clone().map(|i| !switching_frames[i]).collect();
    let on: Vec<bool> = span.clone().map(|i| inside[i] && !switching_frames[i]).collect();
    let errors: Vec<f64> = span
        .clone()
        .filter(|&i| !switching_frames[i])
        .filter_map(|i| near[i])
        .filter(|&(distance, radius)| distance <= NEAR_RADII * radius)
        .map(|(distance, _)| distance)
        .collect();
    let lost_frames: Vec<bool> = on.iter().zip(&tracking).map(|(&on_target, &tracked)| !on_target && tracked).collect();
    let losses: Vec<usize> = crate::scipy::runs(&lost_frames)
        .into_iter()
        .map(|(a, b)| b - a)
        .filter(|&frames| frames as f64 / fps > LOSS_S)
        .collect();
    let (tracked, on_count, lost) = (count_true(&tracking), count_true(&on), losses.iter().sum::<usize>());
    let tracked_frames = tracked.max(1) as f64;
    summary.on_target = Some(on_count as f64 / tracked_frames);
    summary.error = median_of(&errors);
    summary.lost = Some(losses.len() as f64 / (tracked as f64 / fps).max(1e-9));
    let loss_frames: Vec<f64> = losses.iter().map(|&frames| frames as f64).collect();
    summary.back = median_of(&loss_frames).map(|frames| frames / fps);
    summary.longest_off = losses.iter().max().map(|&frames| frames as f64 / fps);
    (summary.start, summary.end) = (Some(first), Some(end));
    *summary.shares = Some(RunShares {
        on_all: share_true(&span.clone().map(|i| inside[i]).collect::<Vec<_>>()),
        lost_cost: lost as f64 / tracked_frames,
        slip_cost: (1.0 - on_count as f64 / tracked_frames - lost as f64 / tracked_frames).max(0.0),
    });
    let switching: Vec<bool> = tracking.iter().map(|&tracked| !tracked).collect();
    summary.per_second = per_second(&on, &switching, fps);
    if !summary.switches.is_empty() {
        read_switches(&mut summary, &switching, fps);
    }
    if let Some(camera) = camera {
        summary.motion = Some(track_motion(frames, fps, camera, first, end, &inside, &switching_frames));
    }
    summary.what_if = Some(what_if(&summary, &tracking, &on, &losses, fps));
    summary
}

/// The best 10 seconds' share on target (among windows with BEST_TRACKED_S of tracking), in a run of
/// MIN_RUN_SECONDS_FOR_BEST seconds or more.
fn best_ten_seconds(on: &[bool], tracking: &[bool], fps: f64) -> Option<f64> {
    let second = (fps.round_ties_even() as usize).max(1);
    let per_second: Vec<(usize, usize)> = (0..on.len())
        .step_by(second)
        .map(|from| {
            let to = (from + second).min(on.len());
            (count_true(&on[from..to]), count_true(&tracking[from..to]))
        })
        .collect();
    if per_second.len() < MIN_RUN_SECONDS_FOR_BEST {
        return None;
    }
    per_second
        .windows(BEST_WINDOW_SECONDS)
        .map(|window| window.iter().fold((0, 0), |sum, second| (sum.0 + second.0, sum.1 + second.1)))
        .filter(|&(_, tracked)| tracked >= BEST_TRACKED_S * second)
        .map(|(on_frames, tracked)| on_frames as f64 / tracked as f64)
        .fold(None, |best: Option<f64>, share| Some(best.map_or(share, |best| best.max(share))))
}

/// Estimates of how much the time on target (and so the accuracy) would rise if one thing changed, all else the same:
/// the off-target time that thing accounts for, as a share of the whole run. They overlap, so they do not add up;
/// each is a ceiling. Biggest first.
fn what_if(summary: &TrackSummary, tracking: &[bool], on: &[bool], losses: &[usize], fps: f64) -> Vec<WhatIf> {
    let (tracked, run) = (count_true(tracking).max(1) as f64, tracking.len().max(1) as f64);
    let mut estimates = Vec::new();
    let mut add = |what: &'static str, frames: f64, how: String| {
        if frames > 0.0 {
            estimates.push(WhatIf { what, gain: frames / run, how });
        }
    };
    let lost = losses.iter().sum::<usize>() as f64;
    add("Get back on twice as fast", lost / 2.0, "Half the time off the bot in drops longer than 0.1 s.".into());
    let slips = (tracked - count_true(on) as f64 - lost).max(0.0);
    add("Don't slip", slips, "The time off the bot in slips shorter than 0.1 s.".into());
    if let Some(off) = summary.motion.as_ref().and_then(|motion| motion.counts.as_ref()).map(|counts| &counts.frames) {
        add("Don't lead", off.ahead as f64, "The time off the bot ahead of it, past its leading edge.".into());
        add("Don't trail", off.behind as f64, "The time off the bot behind it, trailing it.".into());
        add(
            "Don't get thrown by its turns",
            off.turns as f64,
            "The time off the bot in the 0.4 s after each of its direction changes.".into(),
        );
        add(
            "Track every direction like your best one",
            off.directions,
            "Each direction of the bot's motion brought up to the time on target of your best one (among those it \
             moved in 5% of the time or more)."
                .into(),
        );
    }
    if let (Some(best), Some(on_target)) = (best_ten_seconds(on, tracking, fps), summary.on_target) {
        add(
            "Keep up your best 10 seconds all run",
            (best - on_target) * tracked,
            format!("Your best 10 seconds were {}% on target.", (100.0 * best).round_ties_even()),
        );
    }
    if !summary.switches.is_empty() {
        let total: usize = summary.switches.iter().map(|switch| switch[1] - switch[0]).sum();
        add(
            "Get onto the next bot 100 ms faster",
            (summary.switches.len() as f64 * FASTER_SWITCH_S * fps).min(total as f64),
            "100 ms less per switch between bots.".into(),
        );
    }
    estimates.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    estimates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_back_times_the_way_back_onto_the_bot() {
        // 10 fps: on, then off from frame 3 after the turn at 2, back on at 6; a turn at 10 the crosshair stays on
        // through; a turn at 15 it leaves and is not back before the turn at 18; one at 18 it is not back by the end
        let on_except = |off: &[usize]| (0..22).map(|i| !off.contains(&i)).collect::<Vec<bool>>();
        let inside = on_except(&[3, 4, 5, 16, 17, 18, 19, 20, 21]);
        let stop = vec![false; 22];
        let got = turns_back(&[2, 10, 15, 18], &inside, &stop, 4, 22, 10.0);
        let back: Vec<Option<f64>> = got.iter().map(|turn| turn.back).collect();
        assert_eq!(back, [Some(0.4), Some(0.0), None, None]);
        assert_eq!(got.iter().map(|turn| turn.frame).collect::<Vec<_>>(), [2, 10, 15, 18]);
        // a bot dying (a stop frame) before the crosshair is back ends the search
        let mut stop = stop;
        stop[5] = true;
        assert_eq!(turns_back(&[2], &inside, &stop, 4, 22, 10.0)[0].back, None);
    }
}
