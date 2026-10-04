//! Each flick measured (review.py: `target_radius`, `measure`, `choices`).
//!
//! In: the kills matched to their flicks (src/matching.rs), each with the target's path relative to the crosshair, and
//! the tracks (src/track.rs) for the camera's turn and the targets on screen. Out: one `Measure` per kill, the flick
//! speed profile and each next target's `Choice`, which the report (src/review.rs), its summary (src/summary.rs) and
//! the what-if estimates (src/what_if.rs) read.

use serde::Serialize;

use crate::capped::Capped;
use crate::geometry::{blob_radius_deg, degrees};
use crate::matching::Flick;
use crate::python::{hypot, numpy_percentile, round};
use crate::statistics::{mean, median};
use crate::track::{TrackPoint, Tracks};

/// The targets' radius when fewer than `MIN_AREAS_FOR_RADIUS` areas were seen (degrees; 1w4ts Voltaic's).
pub(crate) const DEFAULT_TARGET_RADIUS_DEG: f64 = 0.43;
const MIN_AREAS_FOR_RADIUS: usize = 5;
/// A flick is measured when its path has this many points and starts within `MAX_PATH_START_LAG_FRAMES` of the flick.
const MIN_PATH_POINTS: usize = 4;
const MAX_PATH_START_LAG_FRAMES: i64 = 2;
/// The reaction ends at the first point from which the crosshair closes on the target faster than this for 2 points.
const REACTION_SPEED_DEG_S: f64 = 30.0;
/// The main flick ends at the first point after its peak below this share of the peak speed.
const FLICK_END_SHARE: f64 = 0.15;
/// A correction starts above the first speed and ends below the second.
const CORRECTION_START_DEG_S: f64 = 8.0;
const CORRECTION_STOP_DEG_S: f64 = 4.0;
/// The crosshair slips off the target beyond this share of its radius, and is back on inside the radius: the gap keeps
/// a crosshair on the edge from slipping off every frame.
const SLIP_OFF_SHARE: f64 = 1.15;
/// The crosshair has settled once its speed stays below this until the kill.
const SETTLED_DEG_S: f64 = 10.0;
/// The path between two points at most this many frames apart is taken as straight. Across a longer gap the target was
/// not seen.
const MAX_STRAIGHT_GAP_FRAMES: i64 = 2;
/// The speed curve goes on past the flick's end for a quarter of its length, and at least this many frames.
const MIN_BRAKING_FRAMES: i64 = 2;
/// The braking runs from the last frame at this share of the peak speed.
const BRAKING_START_SHARE: f64 = 0.9;
/// A profile needs this many flicks, each at least `MIN_PROFILE_FLICK_FRAMES` long.
const MIN_PROFILE_FLICKS: usize = 3;
const MIN_PROFILE_FLICK_FRAMES: usize = 3;
/// The next target is the one nearest the crosshair this many frames after the kill.
const CHOICE_DELAY_FRAMES: i64 = 3;
/// A target counts as nearer than the chosen one when it is nearer by more than this.
const NEARER_MARGIN_DEG: f64 = 0.3;

/// One flick's measures (seconds, degrees and degrees a second): the keys measure.py has always written, plus
/// settle, still and the time parts.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Measure {
    pub kill_number: usize,
    pub shots: Option<i64>,
    /// The distance to the target when the flick started.
    #[serde(rename = "D0")]
    pub start_distance_deg: f64,
    /// The direction to the target (0 = right, 90 = up).
    pub direction_deg: f64,
    pub total: f64,
    pub react: Option<f64>,
    pub flick: Option<f64>,
    pub peak: f64,
    /// How far along the way to the target was left when the main flick ended (below 0: past it).
    pub end_left: f64,
    pub end_off: f64,
    /// When the crosshair reached the target, from the flick's start: the first point of its path inside the target's
    /// circle, the path straight between two frames up to 2 frames apart, so it can fall between two frames (a fast
    /// flick can pass through the target between them). Dwell, settle and hold run from it.
    pub arrive: Option<f64>,
    pub dwell: Option<f64>,
    pub past: f64,
    /// Bursts of movement after the main flick.
    pub corrections: usize,
    pub click_speed: f64,
    pub click_off: f64,
    #[cfg_attr(feature = "ts", ts(as = "crate::typescript::TargetOffset"))]
    pub click_off_xy: (f64, f64),
    pub settle: Option<f64>,
    pub still: Option<f64>,
    pub start_frame: i64,
    pub kill_frame: i64,
    pub spawned: bool,
    pub hold: Option<f64>,
    /// How often the crosshair slipped off the target after reaching it, and for how long in all.
    pub breaks: usize,
    pub off: f64,
    /// React, main flick, onto the target, settle, still.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(as = "Option<crate::typescript::KillParts>", optional))]
    pub parts: Option<[f64; 5]>,
    /// The camera's speed through the main flick (none without a main flick).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub speed: Option<SpeedCurve>,
    /// The reloads an empty magazine forced in this kill, and their time in seconds (src/reload.rs; none without the
    /// scenario's ammo rules or the kills' shots).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reloads: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reload_time: Option<f64>,
}

/// The camera's speed through a main flick, in degrees a second, one value a frame from the flick's start (the moves
/// into the frame before, that frame and the next, averaged), and on past its end for a quarter of its length (at
/// least 2 frames) to show the braking. `flick_end` is the index of the flick's last frame.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SpeedCurve {
    pub speeds: Vec<f64>,
    pub flick_end: usize,
}

/// For a kill after the first: whether the next target was the nearest on screen (rank 0), and how much farther.
#[derive(Clone, Debug, Serialize)]
pub struct Choice {
    pub kill_number: usize,
    pub rank: usize,
    pub extra: f64,
}

/// The targets' radius in degrees, from their median area near the crosshair.
pub fn target_radius(flicks: &[Flick]) -> f64 {
    let areas: Vec<f64> = flicks.iter().filter_map(|flick| flick.area_px).filter(|&area| area != 0.0).collect();
    if areas.len() < MIN_AREAS_FOR_RADIUS {
        return DEFAULT_TARGET_RADIUS_DEG;
    }
    blob_radius_deg(median(&areas))
}

/// Each flick's measures, for targets of `radius_deg`; a flick with too short a path is left out.
pub fn measure(flicks: &[Flick], tracks: &Tracks, radius_deg: f64) -> Vec<Measure> {
    let camera = camera_speeds(tracks);
    flicks.iter().filter_map(|flick| measure_one(flick, tracks.fps, radius_deg, &camera)).collect()
}

/// The camera's speed in each frame (degrees a second): the view's shift since the frame before.
fn camera_speeds(tracks: &Tracks) -> Vec<f64> {
    let mut speeds = vec![0.0; tracks.frames.iter().map(|frame| frame.i + 1).max().unwrap_or(0)];
    for frame in &tracks.frames {
        speeds[frame.i] = hypot(frame.shift.0, frame.shift.1) * tracks.fps;
    }
    speeds
}

/// The camera's speed from frame `start` to `end` and the frames after it (see `SpeedCurve`), over 3 frames, since the
/// capture moves in uneven steps. Where the view's shift was not found (0: no target seen in both frames, or a turn of
/// over 6 degrees a frame), the target's own move relative to the crosshair stands in: `path_frames` and `path_speeds`
/// are its path's frames and speeds.
fn speed_curve(start: i64, end: i64, camera: &[f64], path_frames: &[i64], path_speeds: &[f64]) -> SpeedCurve {
    let length = end - start;
    // a quarter of the flick, rounded up
    let braking_frames = ((length + 3) / 4).max(MIN_BRAKING_FRAMES);
    let last = (end + braking_frames).min(camera.len() as i64 - 1).max(end);
    let speed_at = |frame: i64| match camera.get(frame as usize) {
        Some(&speed) if speed > 0.0 => Some(speed),
        Some(_) => Some(path_frames.binary_search(&frame).map_or(0.0, |index| path_speeds[index])),
        None => None,
    };
    let speeds = (start..=last)
        .map(|frame| {
            let around: Capped<f64, 3> =
                (frame - 1..=frame + 1).filter(|&neighbor| neighbor >= 0).filter_map(speed_at).collect();
            round(mean(&around), 1)
        })
        .collect();
    SpeedCurve { speeds, flick_end: length as usize }
}

/// Where the crosshair reaches the target: the first point of its path `offsets` inside the target's circle (radius
/// `radius_deg`), the path straight between two points up to `MAX_STRAIGHT_GAP_FRAMES` apart. A fast flick can pass
/// through the target between them. Across a longer gap the first point inside the circle counts. The first point at
/// or after it (an index into the path) and its time in frames from the path's first frame (a fraction of a frame
/// between two); none when the path never reaches it.
fn arrival(frames: &[i64], offsets: &[(f64, f64)], radius_deg: f64) -> Option<(usize, f64)> {
    if hypot(offsets[0].0, offsets[0].1) < radius_deg {
        return Some((0, 0.0));
    }
    (1..offsets.len()).find_map(|index| {
        let (from, to) = (offsets[index - 1], offsets[index]);
        let gap_frames = frames[index] - frames[index - 1];
        let crossing = if gap_frames <= MAX_STRAIGHT_GAP_FRAMES { circle_entry(from, to, radius_deg) } else { None };
        let inside = hypot(to.0, to.1) < radius_deg;
        (inside || crossing.is_some_and(|share| (0.0..=1.0).contains(&share))).then(|| {
            let share = crossing.map_or(1.0, |share| share.clamp(0.0, 1.0));
            (index, (frames[index - 1] - frames[0]) as f64 + share * gap_frames as f64)
        })
    })
}

/// Where the straight line from `from` to `to` enters the circle of `radius_deg` around the crosshair, as a share of
/// the way (below 0 or above 1: outside the segment); none when the line misses the circle.
fn circle_entry(from: (f64, f64), to: (f64, f64), radius_deg: f64) -> Option<f64> {
    let (step_x, step_y) = (to.0 - from.0, to.1 - from.1);
    let a = step_x * step_x + step_y * step_y;
    let b = 2.0 * (from.0 * step_x + from.1 * step_y);
    let constant = from.0 * from.0 + from.1 * from.1 - radius_deg * radius_deg;
    let discriminant = b * b - 4.0 * a * constant;
    (a > 0.0 && discriminant > 0.0).then(|| (-b - discriminant.sqrt()) / (2.0 * a))
}

/// A flick's path: each point's frame and the target's offset from the crosshair (degrees, right and up positive).
struct Path {
    frames: Vec<i64>,
    offsets: Vec<(f64, f64)>,
    fps: f64,
}

impl Path {
    fn last(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Seconds from point `from` to point `to`.
    fn seconds(&self, from: usize, to: usize) -> f64 {
        (self.frames[to] - self.frames[from]) as f64 / self.fps
    }

    /// The target's distance from the crosshair at a point (degrees).
    fn distance(&self, index: usize) -> f64 {
        hypot(self.offsets[index].0, self.offsets[index].1)
    }

    /// The way from the crosshair to the target at the first point, as a unit vector (right when it starts on the
    /// crosshair).
    fn start_way(&self) -> (f64, f64) {
        let (start, distance) = (self.offsets[0], self.distance(0));
        if distance > 1e-6 { (start.0 / distance, start.1 / distance) } else { (1.0, 0.0) }
    }

    /// The frames from the point before to this one, at least 1.
    fn frames_since_previous(&self, index: usize) -> f64 {
        (self.frames[index] - self.frames[index - 1]).max(1) as f64
    }

    /// The crosshair's speed relative to the target at each point (degrees a second; 0 at the first).
    fn speeds(&self) -> Vec<f64> {
        let step = |index: usize| distance_between(self.offsets[index], self.offsets[index - 1]);
        let speed = |index: usize| step(index) * self.fps / self.frames_since_previous(index);
        std::iter::once(0.0).chain((1..self.offsets.len()).map(speed)).collect()
    }

    /// How fast the crosshair closes on the target along the way to it at the start (degrees a second; 0 at the
    /// first point). `along` is each point's distance along that way.
    fn closing_speeds(&self, along: &[f64]) -> Vec<f64> {
        let speed = |index: usize| (along[index - 1] - along[index]) * self.fps / self.frames_since_previous(index);
        std::iter::once(0.0).chain((1..along.len()).map(speed)).collect()
    }

    /// The speed at each point over 3 points, since the capture moves in uneven steps (degrees a frame step times the
    /// frame rate, as review.py has it; 0 at the first point).
    fn smoothed_speeds(&self) -> Vec<f64> {
        let offsets = &self.offsets;
        let last = self.last();
        let smoothed: Vec<(f64, f64)> = (0..offsets.len())
            .map(|index| {
                if index == 0 || index == last {
                    return offsets[index];
                }
                let (before, here, after) = (offsets[index - 1], offsets[index], offsets[index + 1]);
                ((before.0 + here.0 + after.0) / 3.0, (before.1 + here.1 + after.1) / 3.0)
            })
            .collect();
        let speed = |index: usize| distance_between(smoothed[index], smoothed[index - 1]) * self.fps;
        std::iter::once(0.0).chain((1..smoothed.len()).map(speed)).collect()
    }
}

fn distance_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    hypot(a.0 - b.0, a.1 - b.1)
}

/// The index of the first largest value.
fn first_max_index(values: &[f64]) -> usize {
    (0..values.len()).fold(0, |best, index| if values[index] > values[best] { index } else { best })
}

/// The first index from `peak` on where the speed falls below `FLICK_END_SHARE` of the peak's; the last when none
/// does.
fn flick_end_index(speeds: &[f64], peak: usize) -> usize {
    (peak..speeds.len()).find(|&index| speeds[index] < FLICK_END_SHARE * speeds[peak]).unwrap_or(speeds.len() - 1)
}

/// The point where the reaction ends: the first from which the crosshair closes on the target faster than
/// `REACTION_SPEED_DEG_S` for 2 points.
fn reaction_end(closing_speeds: &[f64]) -> Option<usize> {
    let fast = |index: usize| closing_speeds[index] > REACTION_SPEED_DEG_S;
    (1..closing_speeds.len() - 1).find(|&index| fast(index) && fast(index + 1))
}

/// Separate bursts of movement in `speeds`, the speeds after the main flick.
fn count_corrections(speeds: &[f64]) -> usize {
    let (mut bursts, mut moving) = (0, false);
    for &speed in speeds {
        if speed > CORRECTION_START_DEG_S && !moving {
            (bursts, moving) = (bursts + 1, true);
        } else if speed < CORRECTION_STOP_DEG_S {
            moving = false;
        }
    }
    bursts
}

/// How often the crosshair slipped off the target from point `arrival` on, and for how long in all (seconds). A path
/// that passed through the target between two points is off it at the second.
fn slips(path: &Path, arrival: Option<usize>, radius_deg: f64) -> (usize, f64) {
    let last = path.last();
    let (mut breaks, mut off_seconds, mut inside) = (0, 0.0, true);
    for index in arrival.map_or(last + 1, |arrival| arrival.max(1))..=last {
        let distance = path.distance(index);
        if inside && distance > SLIP_OFF_SHARE * radius_deg {
            (breaks, inside) = (breaks + 1, false);
        } else if !inside && distance < radius_deg {
            inside = true;
        }
        if !inside {
            off_seconds += path.seconds(index - 1, index);
        }
    }
    (breaks, off_seconds)
}

/// The first point from `arrival` on from which the smoothed speed stays below `SETTLED_DEG_S` until the kill.
fn settled_index(smoothed_speeds: &[f64], arrival: usize) -> usize {
    let last = smoothed_speeds.len() - 1;
    let settled_from = |index: usize| smoothed_speeds[index..last].iter().all(|&speed| speed < SETTLED_DEG_S);
    (arrival..=last).find(|&index| settled_from(index)).unwrap_or(last)
}

/// A kill's time in its five steps (seconds): react, main flick, onto the target, settle, still. Each step ends where
/// the next begins: the ends are sorted and capped at the kill's time, so a step that ends early gets none.
fn kill_parts(
    total: f64,
    react: Option<f64>,
    flick: Option<f64>,
    arrive: Option<f64>,
    settled: Option<f64>,
) -> Option<[f64; 5]> {
    let (react, flick, arrive, settled) = (react?, flick?, arrive?, settled?);
    let mut ends = [react, (react + flick).min(arrive), arrive, settled];
    ends.sort_by(f64::total_cmp);
    let bounds = [0.0, ends[0].min(total), ends[1].min(total), ends[2].min(total), ends[3].min(total), total];
    Some(std::array::from_fn(|step| bounds[step + 1] - bounds[step]))
}

fn measure_one(flick: &Flick, fps: f64, radius_deg: f64, camera: &[f64]) -> Option<Measure> {
    let points = &flick.path;
    if points.len() < MIN_PATH_POINTS || points[0].0 > flick.start_frame + MAX_PATH_START_LAG_FRAMES {
        return None;
    }
    let path = Path {
        frames: points.iter().map(|point| point.0).collect(),
        offsets: points.iter().map(|point| (point.1, point.2)).collect(),
        fps,
    };
    let last = path.last();
    let way = path.start_way();
    let along: Vec<f64> = path.offsets.iter().map(|&(x, y)| x * way.0 + y * way.1).collect();
    let speeds = path.speeds();
    let reacted = reaction_end(&path.closing_speeds(&along));
    let peak = first_max_index(&speeds);
    let flick_end = flick_end_index(&speeds, peak);
    // the main flick runs from the reaction's end to `flick_end`, when that comes later
    let flick_start = reacted.filter(|&reacted| flick_end > reacted);
    let arrived = arrival(&path.frames, &path.offsets, radius_deg);
    let arrival_index = arrived.map(|(index, _)| index);
    let (breaks, off) = slips(&path, arrival_index, radius_deg);
    let settled = arrival_index.map(|index| settled_index(&path.smoothed_speeds(), index));
    // the times from the arrival, which can fall between two frames
    let since_arrival = |to: usize| arrived.map(|(_, at)| ((path.frames[to] - path.frames[0]) as f64 - at) / fps);
    let total = path.seconds(0, last);
    let react = reacted.map(|reacted| path.seconds(0, reacted));
    let flick_time = flick_start.map(|flick_start| path.seconds(flick_start, flick_end));
    let arrive = arrived.map(|(_, at)| at / fps);
    Some(Measure {
        kill_number: flick.kill_number,
        shots: flick.shots,
        start_distance_deg: path.distance(0),
        direction_deg: degrees(way.1.atan2(way.0)),
        total,
        react,
        flick: flick_time,
        peak: speeds[peak],
        end_left: along[flick_end],
        end_off: path.distance(flick_end),
        arrive,
        dwell: since_arrival(last),
        past: -along[reacted.unwrap_or(0)..].iter().copied().fold(f64::INFINITY, f64::min),
        corrections: count_corrections(&speeds[flick_end + 1..]),
        click_speed: speeds[last],
        click_off: path.distance(last),
        click_off_xy: path.offsets[last],
        settle: settled.and_then(since_arrival),
        still: settled.map(|settled| path.seconds(settled, last)),
        start_frame: flick.start_frame,
        kill_frame: flick.kill_frame,
        spawned: flick.spawned,
        hold: since_arrival(last),
        breaks,
        off,
        parts: kill_parts(total, react, flick_time, arrive, settled.map(|settled| path.seconds(0, settled))),
        speed: flick_start
            .map(|start| speed_curve(path.frames[start], path.frames[flick_end], camera, &path.frames, &speeds)),
        reloads: None,
        reload_time: None,
    })
}

/// The time steps of the flick speed profile: 0 to 125% of the flick, 5% apart (past 100%: after its end).
pub const PROFILE_STEP: f64 = 0.05;
const PROFILE_POINTS: usize = 26;

/// The flick speed profile: each main flick's camera speed, as a share of its own peak, against the time as a share of
/// the flick (the points are `step` apart from 0), averaged over `flicks` flicks, with the 25th and 75th
/// percentiles. `peak_at`: when the peak comes, as a share of the flick; `braking`: how much of the flick the
/// braking takes, from the last frame at 90% of the peak speed to the first under 15% (medians over the flicks).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FlickProfile {
    pub flicks: usize,
    pub step: f64,
    #[cfg_attr(feature = "ts", ts(as = "Vec<f64>"))]
    pub mean: [f64; PROFILE_POINTS],
    #[cfg_attr(feature = "ts", ts(as = "Vec<f64>"))]
    pub p25: [f64; PROFILE_POINTS],
    #[cfg_attr(feature = "ts", ts(as = "Vec<f64>"))]
    pub p75: [f64; PROFILE_POINTS],
    pub peak_at: f64,
    pub braking: f64,
}

/// One flick's curve at the profile's points (a share of its peak), when its peak comes and how long it brakes.
struct Shape {
    at: [f64; PROFILE_POINTS],
    peak_at: f64,
    braking: f64,
}

/// A flick's shape; None for a flick shorter than `MIN_PROFILE_FLICK_FRAMES`, or with no camera movement.
fn shape(curve: &SpeedCurve) -> Option<Shape> {
    let (speeds, flick_end) = (&curve.speeds, curve.flick_end);
    if flick_end < MIN_PROFILE_FLICK_FRAMES || speeds.len() <= flick_end {
        return None;
    }
    let peak_index = first_max_index(&speeds[..=flick_end]);
    let peak = speeds[peak_index];
    if peak <= 0.0 {
        return None;
    }
    let braking_end = flick_end_index(speeds, peak_index);
    let at_braking_speed = |index: &usize| speeds[*index] >= BRAKING_START_SHARE * peak;
    let braking_start = (peak_index..=braking_end).rev().find(at_braking_speed).unwrap_or(peak_index);
    let at = std::array::from_fn(|point| {
        let frames = point as f64 * PROFILE_STEP * flick_end as f64;
        let i = frames.floor() as usize;
        let speed = if i + 1 < speeds.len() {
            speeds[i] + (speeds[i + 1] - speeds[i]) * (frames - i as f64)
        } else {
            speeds[speeds.len() - 1]
        };
        speed / peak
    });
    let share_of_flick = |frames: usize| frames as f64 / flick_end as f64;
    Some(Shape { at, peak_at: share_of_flick(peak_index), braking: share_of_flick(braking_end - braking_start) })
}

/// The flick speed profile over the measured flicks; None with fewer than `MIN_PROFILE_FLICKS` to average.
pub fn flick_profile(measures: &[Measure]) -> Option<FlickProfile> {
    let shapes: Vec<Shape> = measures.iter().filter_map(|measure| measure.speed.as_ref().and_then(shape)).collect();
    if shapes.len() < MIN_PROFILE_FLICKS {
        return None;
    }
    let column = |point: usize| shapes.iter().map(|shape| shape.at[point]).collect::<Vec<f64>>();
    let points = |statistic: &dyn Fn(&[f64]) -> f64| std::array::from_fn(|point| round(statistic(&column(point)), 3));
    let middle = |field: &dyn Fn(&Shape) -> f64| round(median(&shapes.iter().map(field).collect::<Vec<f64>>()), 3);
    Some(FlickProfile {
        flicks: shapes.len(),
        step: PROFILE_STEP,
        mean: points(&mean),
        p25: points(&|column| numpy_percentile(column, 25.0)),
        p75: points(&|column| numpy_percentile(column, 75.0)),
        peak_at: middle(&|shape| shape.peak_at),
        braking: middle(&|shape| shape.braking),
    })
}

/// For each kill after the first: was the next target the nearest one on screen `CHOICE_DELAY_FRAMES` after the kill.
pub fn choices(tracks: &Tracks, flicks: &[Flick]) -> Vec<Choice> {
    let targets_by_frame: std::collections::HashMap<i64, &Vec<TrackPoint>> =
        tracks.frames.iter().map(|frame| (frame.i as i64, &frame.t)).collect();
    let mut choices = Vec::with_capacity(flicks.len().saturating_sub(1));
    for pair in flicks.windows(2) {
        let (killed, next) = (&pair[0], &pair[1]);
        let chosen_frame = killed.kill_frame + CHOICE_DELAY_FRAMES;
        let Some(targets) = targets_by_frame.get(&chosen_frame).filter(|targets| !targets.is_empty()) else { continue };
        let Some(&(_, x, y)) = next.path.iter().find(|point| point.0 >= chosen_frame) else { continue };
        let mut distances: Vec<f64> = targets.iter().map(|&(_, x, y)| hypot(x, y)).collect();
        distances.sort_by(f64::total_cmp);
        let chosen = hypot(x, y);
        choices.push(Choice {
            kill_number: next.kill_number,
            rank: distances.iter().filter(|&&distance| distance < chosen - NEARER_MARGIN_DEG).count(),
            extra: chosen - distances[0],
        });
    }
    choices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::TrackFrame;

    /// A made-up flick at 100 frames a second: still for 5 frames, then 400, 800, 1200, 1200, 800, 400 and 100
    /// degrees a second onto a target 50 degrees to the right, then still. The view's shift is lost in the two
    /// fastest frames (12 degrees a frame), so the target's own move stands in there.
    #[test]
    fn speed_curve_and_profile_of_a_made_up_flick() {
        let moves = [0.0, 0.0, 0.0, 0.0, 0.0, 4.0, 8.0, 12.0, 12.0, 8.0, 4.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let mut x = 50.0;
        let path: Vec<(i64, f64, f64)> = moves
            .iter()
            .enumerate()
            .map(|(i, step)| {
                x -= step;
                (i as i64, x, 0.0)
            })
            .collect();
        let frames = moves
            .iter()
            .enumerate()
            .map(|(i, &step)| TrackFrame {
                i,
                shift: if step > 6.0 { (0.0, 0.0) } else { (-step, 0.0) },
                t: Vec::new(),
                a: Vec::new(),
                wh: None,
                s: None,
            })
            .collect();
        let tracks = Tracks { fps: 100.0, frames, version: 0 };
        let flick = Flick {
            kill_number: 1,
            kill_frame: 15,
            stats_frame: Some(15),
            start_frame: 0,
            shots: Some(1),
            path,
            spawned: false,
            area_px: None,
        };
        let measures = measure(&[flick.clone(), flick.clone(), flick], &tracks, 0.5);
        let curve = measures[0].speed.as_ref().unwrap();
        // from the flick's start (frame 5) to its end (frame 11, under 15% of the peak), and 2 frames more, each over 3
        // frames
        assert_eq!(curve.speeds, [400.0, 800.0, 1066.7, 1066.7, 800.0, 433.3, 166.7, 33.3, 0.0]);
        assert_eq!(curve.flick_end, 6);
        let profile = flick_profile(&measures).unwrap();
        assert_eq!((profile.flicks, profile.mean.len()), (3, 26));
        assert_eq!((profile.mean[0], profile.mean[10], profile.mean[20], profile.mean[25]), (0.375, 1.0, 0.156, 0.016));
        assert_eq!((profile.p25[5], profile.p75[5]), (profile.mean[5], profile.mean[5]));
        // the peak at frame 2 of 6; the braking from the last frame at 90% (3) to the first under 15% (7, after the end)
        assert_eq!((profile.peak_at, profile.braking), (0.333, 0.667));
    }

    #[test]
    fn a_flick_reaches_the_target_where_its_path_enters_the_circle() {
        let radius = 0.5;
        // 1 degree a frame onto the target: inside from frame 2, its path entering the circle at frame 1.5
        assert_eq!(arrival(&[0, 1, 2], &[(2.0, 0.0), (1.0, 0.0), (0.0, 0.0)], radius), Some((2, 1.5)));
        // through the target between frames 1 and 2, from 1 degree before it to 1 past it: there at frame 1.25
        let through = [(3.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 0.0)];
        assert_eq!(arrival(&[0, 1, 2, 3], &through, radius), Some((2, 1.25)));
        // across a gap of 3 frames the path is not taken as straight: the first frame inside
        assert_eq!(arrival(&[0, 1, 4], &[(3.0, 0.0), (1.0, 0.0), (0.0, 0.0)], radius), Some((2, 4.0)));
        assert_eq!(arrival(&[0, 1], &[(3.0, 0.0), (1.0, 0.0)], radius), None);
    }
}
