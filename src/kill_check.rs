//! The kills the video alone gives (matching.rs `match_video`), checked in the frames round them: a target that dies
//! leaves wall where it was, while a target the tracking only lost, or a crosshair the detector boxed, still shows
//! there. Before a kill (frames kill-4 to kill-2) its target's patch is measured at its tracked place against the wall
//! round it; after it (kill+3 to kill+6) the same at the place it died, carried along by the camera's turn (a frame's
//! shift moves a still spot on screen by as much). A kill whose target still shows after, by half as much as before or
//! more, is no kill (`ruled_out`): on the gate's static runs that left out 34% of the video's false kills and 0.65% of
//! its true ones, on its dynamic runs 37% and 0.31% (measured against the stats files, 2026-10-06). The place it died
//! is measured in each of the TRAIL frames after it (`KillEvidence::trail`): a target hidden under the crosshair before
//! the click stays a while at part of its level, and goes when it dies. Such a kill is moved to when it died
//! (`with_hidden_kills`): the frame the place reached the wall, less the death's fade, the target held at the
//! crosshair until then. On the video-alone runs' dev set that took 1wall 6targets extra small from 45 to 57 of its 98
//! kills, the held-out runs unchanged (2026-10-06).
//!
//! In: the review's tracks and fixed map, then the frames it asks for (`frames`) at 1280 x 720 RGB, from a host (the
//! service reads the video again: service/src/review.rs). Out: each kill's evidence (`KillEvidence`), which the review
//! keeps (kills.json) and a review request carries (review.rs), so the report leaves out the kills it rules out.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W, to_px};
use crate::matching::{Flick, PathPoint, match_video};
use crate::python::hypot;
use crate::statistics::median;
use crate::track::Tracks;

/// The frames before a kill its target is measured in, at its tracked places (offsets from the kill's frame).
const BEFORE: [i64; 3] = [-4, -3, -2];
/// The frames after a kill whose measures, where it died, give `KillEvidence::after` (offsets from the kill's frame).
const AFTER: [i64; 4] = [3, 4, 5, 6];
/// The trail goes on to this many frames after the kill.
const TRAIL: i64 = 40;
/// The target's patch: a disc this share of its box's longer side across (at least MIN_RADIUS_PX), against a ring from
/// RING_NEAR to RING_FAR times its radius: the wall round it.
const CORE_SHARE: f64 = 0.6;
/// The smallest radius of the disc, in pixels.
const MIN_RADIUS_PX: f64 = 2.5;
/// The ring starts this many times the disc's radius out, so the target's soft edge stays out of it.
const RING_NEAR: f64 = 1.2;
/// The ring ends this many times the disc's radius out: 1.6 times the box's longer side from the center (while the
/// radius is not raised to MIN_RADIUS_PX).
const RING_FAR: f64 = 1.6 / CORE_SHARE;
/// A patch needs this many pixels in the disc and in the ring (the fixed map's pixels, the crosshair's, left out), and
/// MIN_FREE_SHARE of its disc off the fixed map: a target as small as the crosshair, at it, is mostly crosshair, whose
/// soft edges (not in the fixed map) would stand out after the kill as if the target were still there. On the gate's
/// static runs that vetoed 29 true kills where 3 are now (and caught 10 false kills where it caught 28). This is the
/// disc's count.
const MIN_CORE_PIXELS: usize = 2;
/// The fewest pixels off the fixed map the ring needs.
const MIN_RING_PIXELS: usize = 6;
/// The smallest share of the disc's pixels off the fixed map.
const MIN_FREE_SHARE: f64 = 0.3;
/// A target that still shows after the kill by this share of how it showed before (or more) did not die.
const STILL_THERE_SHARE: f64 = 0.5;
/// A target that stands out by this share of how it did before (at least GONE_FLOOR) or less, in two frames measured
/// one after the other, has gone.
const GONE_SHARE: f64 = 0.12;
/// The least that counts as gone, in the color distance `standing_out` gives (8-bit levels).
const GONE_FLOOR: f64 = 5.0;
/// A kill whose target goes more than HIDDEN_FRAMES after its track's end, its place within AT_CROSSHAIR_DEG of the
/// crosshair until then, was hidden under the crosshair: it died FADE_FRAMES before it went (the death's fade).
const HIDDEN_FRAMES: i64 = 6;
/// The frames a dying target takes to fade: a hidden target died this many frames before it went.
const FADE_FRAMES: i64 = 6;
/// How near the crosshair, in degrees, a hidden target's place must stay until it goes.
const AT_CROSSHAIR_DEG: f64 = 0.4;
/// The bytes of an RGB24 pixel.
const RGB: usize = 3;

/// A kill's evidence: how much its target stood out from the wall before it and after it (the median over the frames
/// measured; None where none could be: the place was off screen, or the frames were missing), and the place it died in
/// each frame after it, 1 to TRAIL (None where it could not be measured).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct KillEvidence {
    /// The kill's frame, as the video alone gave it; the key a review request matches the kill by.
    pub frame: i64,
    /// The median of how much the target stood out at its tracked places 4 to 2 frames before the kill (the color
    /// distance between the disc and the ring, 8-bit levels).
    pub before: Option<f64>,
    /// The same median where it died, 3 to 6 frames after the kill.
    pub after: Option<f64>,
    /// How much the place it died stood out in each of the TRAIL frames after the kill (frame kill + 1 first); empty in
    /// a kills.json written before the trail.
    #[serde(default)]
    pub trail: Vec<Option<f64>>,
}

/// Whether a kill's evidence rules it out: its target still showed after it, by STILL_THERE_SHARE of how it showed
/// before or more.
pub fn ruled_out(evidence: &KillEvidence) -> bool {
    matches!((evidence.before, evidence.after), (Some(before), Some(after)) if after >= STILL_THERE_SHARE * before)
}

/// The kills (in order) with those whose target was hidden under the crosshair moved to when it died (`hidden_until`),
/// each next flick starting no earlier than the kill before it (its path cut to match). `checked`: the kills' evidence.
pub fn with_hidden_kills(flicks: Vec<Flick>, checked: &[KillEvidence], tracks: &Tracks) -> Vec<Flick> {
    let mut previous: Option<i64> = None;
    let mut out = Vec::with_capacity(flicks.len());
    for mut flick in flicks {
        if let Some(previous) = previous.filter(|&previous| previous > flick.start_frame) {
            flick.start_frame = previous;
            flick.path.retain(|point| point.0 >= previous);
        }
        let evidence = checked.iter().find(|evidence| evidence.frame == flick.kill_frame);
        if let Some((kill_frame, held)) = evidence.and_then(|evidence| hidden_until(&flick, evidence, tracks)) {
            flick.kill_frame = kill_frame;
            flick.path.extend(held);
        }
        previous = Some(flick.kill_frame);
        out.push(flick);
    }
    out
}

/// When a kill's target hidden under the crosshair died, and where it was held from its track's end until then (the
/// place it ended moved by the view's shift); None when it was not hidden.
fn hidden_until(flick: &Flick, evidence: &KillEvidence, tracks: &Tracks) -> Option<(i64, Vec<PathPoint>)> {
    let (before, &(end, x, y)) = (evidence.before?, flick.path.last()?);
    let low = (GONE_SHARE * before).max(GONE_FLOOR);
    let seen: Vec<(i64, f64)> =
        evidence.trail.iter().enumerate().filter_map(|(at, value)| Some((at as i64 + 1, (*value)?))).collect();
    let gone = seen.windows(2).find(|pair| pair[0].1 < low && pair[1].1 < low)?[0].0;
    let mut places = Vec::new();
    let (mut at_x, mut at_y) = (x, y);
    for frame in end + 1..=end + gone {
        let Some(shift) = usize::try_from(frame).ok().and_then(|frame| tracks.frames.get(frame)).map(|at| at.shift)
        else {
            break;
        };
        (at_x, at_y) = (at_x + shift.0, at_y + shift.1);
        places.push((frame, at_x, at_y));
    }
    let at_crosshair = |&(x, y): &(f64, f64)| hypot(x, y) <= AT_CROSSHAIR_DEG;
    let held = at_crosshair(&(x, y)) && places.iter().all(|&(_, x, y)| at_crosshair(&(x, y)));
    if gone <= HIDDEN_FRAMES || !held {
        return None;
    }
    places.truncate((gone - FADE_FRAMES) as usize);
    Some((end + gone - FADE_FRAMES, places))
}

/// One measurement to make in a frame: the kill's, `offset` frames from it (before it when negative), a patch round
/// (x, y) of this radius (pixels).
struct Look {
    /// The kill's frame, which names the kill the measure belongs to.
    kill: i64,
    /// The frame's offset from the kill: negative before it, 1 to TRAIL after it.
    offset: i64,
    /// The patch's center x: degrees from the crosshair until `KillCheck::look` turns it into pixels.
    x: f64,
    /// The patch's center y: degrees until `KillCheck::look` turns it into pixels.
    y: f64,
    /// The disc's radius, in pixels.
    radius: f64,
}

/// A kill's measurements: before it, and the trail after it (frame kill + 1 at 0).
#[derive(Default)]
struct Measured {
    /// The measures before the kill that could be made, in the order made.
    before: Vec<f64>,
    /// The measure in each of the TRAIL frames after the kill; None where it could not be made.
    trail: Vec<Option<f64>>,
}

/// The kills of a review being checked: the measurements each frame needs, and those made.
pub struct KillCheck {
    /// The measures to make, by the frame's index in the recording.
    looks: BTreeMap<usize, Vec<Look>>,
    /// The measures made, by the kill's frame.
    measured: BTreeMap<i64, Measured>,
    /// The review's fixed map, 1280 x 720 row by row: true where the screen does not move, which no patch counts.
    fixed: Vec<bool>,
}

impl KillCheck {
    /// The check of the kills the video alone gives from these tracks, with the review's fixed map (1280 x 720,
    /// non-zero where the screen does not move: the crosshair, the HUD).
    pub fn new(tracks: &Tracks, fixed: &[u8]) -> Self {
        let (flicks, _) = match_video(tracks);
        let mut check = KillCheck {
            looks: BTreeMap::new(),
            measured: BTreeMap::new(),
            fixed: fixed.iter().map(|&value| value != 0).collect(),
        };
        for flick in &flicks {
            check.plan(tracks, flick);
        }
        check
    }

    /// A kill's measurements: before it at its target's tracked places, after it where it died moved by the camera's
    /// turn since.
    fn plan(&mut self, tracks: &Tracks, flick: &Flick) {
        let Some(&(_, x, y)) = flick.path.last() else { return };
        let kill = flick.kill_frame;
        self.measured.insert(kill, Measured { before: Vec::new(), trail: vec![None; TRAIL as usize] });
        let radius = patch_radius(tracks, flick);
        for offset in BEFORE {
            if let Some(&(frame, at_x, at_y)) = flick.path.iter().find(|point| point.0 == kill + offset) {
                self.look(frame, Look { kill, offset, x: at_x, y: at_y, radius });
            }
        }
        let (mut turn_x, mut turn_y) = (0.0, 0.0);
        for offset in 1..=TRAIL {
            let Some(frame) = usize::try_from(kill + offset).ok().and_then(|frame| tracks.frames.get(frame)) else {
                break;
            };
            (turn_x, turn_y) = (turn_x + frame.shift.0, turn_y + frame.shift.1);
            self.look(kill + offset, Look { kill, offset, x: x + turn_x, y: y + turn_y, radius });
        }
    }

    /// A measurement in a frame, its place turned from degrees into pixels.
    fn look(&mut self, frame: i64, look: Look) {
        let (x, y) = to_px(look.x, look.y);
        if let Ok(frame) = usize::try_from(frame) {
            self.looks.entry(frame).or_default().push(Look { x, y, ..look });
        }
    }

    /// The frames to feed (`add`), in order.
    pub fn frames(&self) -> Vec<usize> {
        self.looks.keys().copied().collect()
    }

    /// A frame's measurements, from its RGB (1280 x 720).
    pub fn add(&mut self, frame: usize, rgb: &[u8]) {
        for look in self.looks.get(&frame).into_iter().flatten() {
            let value = standing_out(rgb, &self.fixed, look.x, look.y, look.radius);
            let measured = self.measured.entry(look.kill).or_default();
            match usize::try_from(look.offset - 1) {
                Ok(at) => measured.trail[at] = value,
                Err(_) => measured.before.extend(value),
            }
        }
    }

    /// Each kill's evidence, in order: `after` the median of the trail's frames AFTER.
    pub fn evidence(&self) -> Vec<KillEvidence> {
        let middle = |values: &[f64]| (!values.is_empty()).then(|| median(values));
        self.measured
            .iter()
            .map(|(&frame, measured)| {
                let after: Vec<f64> = AFTER.iter().filter_map(|&offset| measured.trail[offset as usize - 1]).collect();
                let (before, after) = (middle(&measured.before), middle(&after));
                KillEvidence { frame, before, after, trail: measured.trail.clone() }
            })
            .collect()
    }
}

/// The patch's radius (pixels) for a kill's target: CORE_SHARE of its box's longer side at the kill (else of its
/// blob's width), at least MIN_RADIUS_PX.
fn patch_radius(tracks: &Tracks, flick: &Flick) -> f64 {
    let side = flick.path.last().and_then(|&(frame, x, y)| {
        let frame = tracks.frames.get(usize::try_from(frame).ok()?)?;
        let index = frame.t.iter().position(|&(_, at_x, at_y)| at_x == x && at_y == y)?;
        let (width, height) = frame.wh.as_ref()?.get(index).copied()?;
        let (left, top) = to_px(x - width / 2.0, y + height / 2.0);
        let (right, bottom) = to_px(x + width / 2.0, y - height / 2.0);
        Some((right - left).max(bottom - top))
    });
    let side = side.or_else(|| flick.area_px.map(|area| 2.0 * (area / std::f64::consts::PI).sqrt()));
    (CORE_SHARE * side.unwrap_or(0.0)).max(MIN_RADIUS_PX)
}

/// How much the disc round (cx, cy) differs in color from the ring round it (the distance between their mean colors),
/// the fixed map's pixels left out; None near the frame's edge, with too few pixels, or with the disc mostly fixed.
fn standing_out(rgb: &[u8], fixed: &[bool], cx: f64, cy: f64, radius: f64) -> Option<f64> {
    let reach = (radius * RING_FAR).ceil() + 1.0;
    let (x0, y0, x1, y1) = (cx - reach, cy - reach, cx + reach, cy + reach);
    if x0 < 0.0 || y0 < 0.0 || x1 >= W as f64 || y1 >= H as f64 {
        return None;
    }
    let (mut core, mut ring) = (([0.0; RGB], 0usize), ([0.0; RGB], 0usize));
    let mut disc = 0usize;
    for y in y0 as usize..=y1 as usize {
        for x in x0 as usize..=x1 as usize {
            let distance = hypot(x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let index = y * W + x;
            disc += usize::from(distance <= radius);
            let into = if fixed[index] {
                continue;
            } else if distance <= radius {
                &mut core
            } else if distance > radius * RING_NEAR && distance <= radius * RING_FAR {
                &mut ring
            } else {
                continue;
            };
            for channel in 0..RGB {
                into.0[channel] += f64::from(rgb[index * RGB + channel]);
            }
            into.1 += 1;
        }
    }
    if core.1 < MIN_CORE_PIXELS || ring.1 < MIN_RING_PIXELS || (core.1 as f64) < MIN_FREE_SHARE * disc as f64 {
        return None;
    }
    let mean = |sums: [f64; RGB], count: usize, channel: usize| sums[channel] / count as f64;
    let gap = |channel: usize| mean(core.0, core.1, channel) - mean(ring.0, ring.1, channel);
    let squares: f64 = (0..RGB).map(|channel| gap(channel).powi(2)).sum();
    Some(squares.sqrt())
}

/// Checks the patch measure, the hidden kills and the rule that leaves a kill out.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::TrackFrame;

    /// A gray wall with a red disc of radius 6 at (100, 100).
    fn wall_with_target() -> Vec<u8> {
        let mut rgb = vec![128u8; W * H * RGB];
        for y in 90..110 {
            for x in 90..110 {
                if hypot(x as f64 + 0.5 - 100.0, y as f64 + 0.5 - 100.0) <= 6.0 {
                    rgb[(y * W + x) * RGB..(y * W + x + 1) * RGB].copy_from_slice(&[220, 30, 30]);
                }
            }
        }
        rgb
    }

    /// A red disc stands out from a plain wall and the wall does not; a patch near the frame's edge, under the fixed
    /// map, or mostly under it has no measure.
    #[test]
    fn a_target_stands_out_and_the_wall_does_not() {
        let (rgb, fixed) = (wall_with_target(), vec![false; W * H]);
        let target = standing_out(&rgb, &fixed, 100.0, 100.0, 4.0).unwrap();
        let wall = standing_out(&rgb, &fixed, 300.0, 300.0, 4.0).unwrap();
        assert!(target > 100.0 && wall < 1e-9, "{target} {wall}");
        assert_eq!(standing_out(&rgb, &fixed, 2.0, 2.0, 4.0), None, "too near the edge");
        let mut covered = fixed.clone();
        (90..110).for_each(|y| covered[y * W + 90..y * W + 110].fill(true));
        assert_eq!(standing_out(&rgb, &covered, 100.0, 100.0, 4.0), None, "the fixed map's pixels left out");
        let mut mostly = fixed.clone();
        (90..103).for_each(|y| mostly[y * W + 90..y * W + 110].fill(true));
        assert_eq!(standing_out(&rgb, &mostly, 100.0, 100.0, 4.0), None, "a disc mostly under the crosshair");
        let mut edge = fixed.clone();
        edge[96 * W + 90..96 * W + 110].fill(true);
        assert!(standing_out(&rgb, &edge, 100.0, 100.0, 4.0).is_some(), "a disc a little under it");
    }

    /// 60 empty frames, each turning the view by `shift_deg` in x.
    fn still_tracks(shift_deg: f64) -> Tracks {
        let frame = |i| TrackFrame { i, shift: (shift_deg, 0.0), t: Vec::new(), a: Vec::new(), wh: None, s: None };
        Tracks { fps: 60.0, frames: (0..60).map(frame).collect(), version: 0 }
    }

    /// A flick ending at frame 10 at (0.1, 0) degrees: the target lost under the crosshair.
    fn lost_at_crosshair() -> Flick {
        let path = (0..=10).map(|frame| (frame, 0.1, 0.0)).collect();
        Flick {
            kill_number: 1,
            kill_frame: 10,
            stats_frame: None,
            start_frame: 0,
            shots: None,
            path,
            spawned: false,
            area_px: None,
        }
    }

    /// The target at a third of its level for 11 frames after the track's end, then wall.
    fn hidden_then_gone() -> KillEvidence {
        let trail = (1..=TRAIL).map(|offset| Some(if offset <= 11 { 30.0 } else { 1.0 })).collect();
        KillEvidence { frame: 10, before: Some(100.0), after: Some(30.0), trail }
    }

    /// A target held at the crosshair after its track ended dies a fade before it goes; not when the view turned it
    /// away, and the next flick starts at the moved kill.
    #[test]
    fn a_target_hidden_under_the_crosshair_dies_when_it_goes() {
        let flicks = with_hidden_kills(vec![lost_at_crosshair()], &[hidden_then_gone()], &still_tracks(0.0));
        let gone = 12;
        assert_eq!(flicks[0].kill_frame, 10 + gone - FADE_FRAMES);
        assert_eq!(flicks[0].path.last(), Some(&(10 + gone - FADE_FRAMES, 0.1, 0.0)), "held at the crosshair");
        let turned = with_hidden_kills(vec![lost_at_crosshair()], &[hidden_then_gone()], &still_tracks(0.2));
        assert_eq!(turned[0].kill_frame, 10, "the view turned away: the place left the crosshair");
        let next = Flick { kill_number: 2, kill_frame: 30, start_frame: 10, ..lost_at_crosshair() };
        let both = with_hidden_kills(vec![lost_at_crosshair(), next], &[hidden_then_gone()], &still_tracks(0.0));
        assert_eq!(both[1].start_frame, both[0].kill_frame, "the next flick starts at the moved kill");
        assert!(both[1].path.iter().all(|point| point.0 >= both[0].kill_frame));
    }

    /// A kill is left out when its target shows after by half as much as before or more, and never without both
    /// measures.
    #[test]
    fn a_kill_is_ruled_out_only_when_its_target_still_shows() {
        let evidence = |before, after| KillEvidence { frame: 0, before, after, trail: Vec::new() };
        assert!(ruled_out(&evidence(Some(100.0), Some(60.0))));
        assert!(!ruled_out(&evidence(Some(100.0), Some(10.0))));
        assert!(!ruled_out(&evidence(None, Some(60.0))));
        assert!(!ruled_out(&evidence(Some(100.0), None)));
    }
}
