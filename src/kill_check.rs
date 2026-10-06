//! The kills the video alone gives (matching.rs `match_video`), checked in the frames round them: a target that dies
//! leaves wall where it was, while a target the tracking only lost, or a crosshair the detector boxed, still shows
//! there. Before a kill (frames kill-4 to kill-2) its target's patch is measured at its tracked place against the wall
//! round it; after it (kill+3 to kill+6) the same at the place it died, carried along by the camera's turn (a frame's
//! shift moves a still spot on screen by as much). A kill whose target still shows after, by half as much as before or
//! more, is no kill (`ruled_out`): on the gate's static runs that left out 34% of the video's false kills and 0.65% of
//! its true ones, on its dynamic runs 37% and 0.31% (measured against the stats files, 2026-10-06).
//!
//! In: the review's tracks and fixed map, then the frames it asks for (`frames`) at 1280 x 720 RGB, from a host (the
//! service reads the video again: service/src/review.rs). Out: each kill's evidence (`KillEvidence`), which the review
//! keeps (kills.json) and a review request carries (review.rs), so the report leaves out the kills it rules out.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W, to_px};
use crate::matching::{Flick, match_video};
use crate::python::hypot;
use crate::statistics::median;
use crate::track::Tracks;

/// The frames before a kill its target is measured in (its tracked places), and after it (where it died).
const BEFORE: [i64; 3] = [-4, -3, -2];
const AFTER: [i64; 4] = [3, 4, 5, 6];
/// The target's patch: a disc this share of its box's longer side across (at least MIN_RADIUS_PX), against a ring from
/// RING_NEAR to RING_FAR times its radius: the wall round it.
const CORE_SHARE: f64 = 0.6;
const MIN_RADIUS_PX: f64 = 2.5;
const RING_NEAR: f64 = 1.2;
const RING_FAR: f64 = 1.6 / CORE_SHARE;
/// A patch needs this many pixels in the disc and in the ring (the fixed map's pixels, the crosshair's, left out).
const MIN_CORE_PIXELS: usize = 2;
const MIN_RING_PIXELS: usize = 6;
/// A target that still shows after the kill by this share of how it showed before (or more) did not die.
const STILL_THERE_SHARE: f64 = 0.5;
const RGB: usize = 3;

/// A kill's evidence: how much its target stood out from the wall before it and after it (the median over the frames
/// measured; None where none could be: the place was off screen, or the frames were missing).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct KillEvidence {
    pub frame: i64,
    pub before: Option<f64>,
    pub after: Option<f64>,
}

/// Whether a kill's evidence rules it out: its target still showed after it, by STILL_THERE_SHARE of how it showed
/// before or more.
pub fn ruled_out(evidence: &KillEvidence) -> bool {
    matches!((evidence.before, evidence.after), (Some(before), Some(after)) if after >= STILL_THERE_SHARE * before)
}

/// One measurement to make in a frame: the kill's, before or after it, a patch round (x, y) of this radius (pixels).
struct Look {
    kill: i64,
    after: bool,
    x: f64,
    y: f64,
    radius: f64,
}

/// The kills of a review being checked: the measurements each frame needs, and those made.
pub struct KillCheck {
    looks: BTreeMap<usize, Vec<Look>>,
    measured: BTreeMap<i64, [Vec<f64>; 2]>,
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
        self.measured.insert(kill, [Vec::new(), Vec::new()]);
        let radius = patch_radius(tracks, flick);
        for offset in BEFORE {
            if let Some(&(frame, at_x, at_y)) = flick.path.iter().find(|point| point.0 == kill + offset) {
                self.look(frame, Look { kill, after: false, x: at_x, y: at_y, radius });
            }
        }
        let (mut turn_x, mut turn_y) = (0.0, 0.0);
        for offset in 1..=AFTER[AFTER.len() - 1] {
            let Some(frame) = usize::try_from(kill + offset).ok().and_then(|frame| tracks.frames.get(frame)) else {
                break;
            };
            (turn_x, turn_y) = (turn_x + frame.shift.0, turn_y + frame.shift.1);
            if AFTER.contains(&offset) {
                self.look(kill + offset, Look { kill, after: true, x: x + turn_x, y: y + turn_y, radius });
            }
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
            if let Some(value) = standing_out(rgb, &self.fixed, look.x, look.y, look.radius) {
                self.measured.entry(look.kill).or_default()[usize::from(look.after)].push(value);
            }
        }
    }

    /// Each kill's evidence, in order.
    pub fn evidence(&self) -> Vec<KillEvidence> {
        let middle = |values: &Vec<f64>| (!values.is_empty()).then(|| median(values));
        self.measured
            .iter()
            .map(|(&frame, [before, after])| KillEvidence { frame, before: middle(before), after: middle(after) })
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
/// the fixed map's pixels left out; None near the frame's edge or with too few pixels.
fn standing_out(rgb: &[u8], fixed: &[bool], cx: f64, cy: f64, radius: f64) -> Option<f64> {
    let reach = (radius * RING_FAR).ceil() + 1.0;
    let (x0, y0, x1, y1) = (cx - reach, cy - reach, cx + reach, cy + reach);
    if x0 < 0.0 || y0 < 0.0 || x1 >= W as f64 || y1 >= H as f64 {
        return None;
    }
    let (mut core, mut ring) = (([0.0; RGB], 0usize), ([0.0; RGB], 0usize));
    for y in y0 as usize..=y1 as usize {
        for x in x0 as usize..=x1 as usize {
            let distance = hypot(x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let index = y * W + x;
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
    if core.1 < MIN_CORE_PIXELS || ring.1 < MIN_RING_PIXELS {
        return None;
    }
    let mean = |sums: [f64; RGB], count: usize, channel: usize| sums[channel] / count as f64;
    let gap = |channel: usize| mean(core.0, core.1, channel) - mean(ring.0, ring.1, channel);
    let squares: f64 = (0..RGB).map(|channel| gap(channel).powi(2)).sum();
    Some(squares.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey wall with a red disc of radius 6 at (100, 100).
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
    }

    #[test]
    fn a_kill_is_ruled_out_only_when_its_target_still_shows() {
        let evidence = |before, after| KillEvidence { frame: 0, before, after };
        assert!(ruled_out(&evidence(Some(100.0), Some(60.0))));
        assert!(!ruled_out(&evidence(Some(100.0), Some(10.0))));
        assert!(!ruled_out(&evidence(None, Some(60.0))));
        assert!(!ruled_out(&evidence(Some(100.0), None)));
    }
}
