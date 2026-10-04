//! Each flick measured (review.py: `target_radius`, `measure`, `choices`).

use serde::Serialize;

use crate::capped::Capped;
use crate::geometry::{degrees, K};
use crate::matching::Flick;
use crate::python::{hypot, numpy_percentile, round};
use crate::statistics::{mean, median};
use crate::track::Tracks;

/// One flick's measures (seconds, degrees and degrees a second): the keys measure.py has always written, plus
/// settle, still and the time parts.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Measure {
    pub n: usize,
    pub shots: Option<i64>,
    /// The distance to the target when the flick started.
    #[serde(rename = "D0")]
    pub d0: f64,
    /// The direction to the target (0 = right, 90 = up).
    pub dir: f64,
    pub total: f64,
    pub react: Option<f64>,
    pub flick: Option<f64>,
    pub peak: f64,
    /// How far along the way to the target was left when the main flick ended (below 0: past it).
    pub end_left: f64,
    pub end_off: f64,
    pub arrive: Option<f64>,
    pub dwell: Option<f64>,
    pub past: f64,
    /// Corrections: bursts of movement after the main flick.
    pub corr: usize,
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
/// into the frame before, that frame and the next, averaged), and on past its end for a quarter of its length (at least 2 frames) to show the braking. `end`
/// is the index of the flick's last frame.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SpeedCurve {
    pub v: Vec<f64>,
    pub end: usize,
}

/// For a kill after the first: whether the next target was the nearest on screen (rank 0), and how much farther.
#[derive(Clone, Debug, Serialize)]
pub struct Choice {
    pub n: usize,
    pub rank: usize,
    pub extra: f64,
}

/// The targets' radius in degrees, from their median area near the crosshair (0.43 for 1w4ts Voltaic, with fewer
/// than 5 areas).
pub fn target_radius(flicks: &[Flick]) -> f64 {
    let a: Vec<f64> = flicks.iter().filter_map(|f| f.area).filter(|&a| a != 0.0).collect();
    if a.len() < 5 {
        return 0.43;
    }
    degrees((crate::statistics::median(&a) / std::f64::consts::PI).sqrt() / K)
}

pub fn measure(flicks: &[Flick], tracks: &Tracks, r: f64) -> Vec<Measure> {
    let camera = camera_speeds(tracks);
    flicks.iter().filter_map(|f| measure_one(f, tracks.fps, r, &camera)).collect()
}

/// The camera's speed in each frame (degrees a second): the view's shift since the frame before.
fn camera_speeds(tracks: &Tracks) -> Vec<f64> {
    let mut v = vec![0.0; tracks.frames.iter().map(|f| f.i + 1).max().unwrap_or(0)];
    for f in &tracks.frames {
        v[f.i] = hypot(f.shift.0, f.shift.1) * tracks.fps;
    }
    v
}

/// The camera's speed from frame `start` to `end` and the frames after it (see `SpeedCurve`), over 3 frames, since the
/// capture moves in uneven steps. Where the view's shift was not found (0: no target seen in both frames, or a turn of
/// over 6 degrees a frame), the target's own move relative to the crosshair stands in: `fr` and `sp` are the path's
/// frames and speeds.
fn speed_curve(start: i64, end: i64, camera: &[f64], fr: &[i64], sp: &[f64]) -> SpeedCurve {
    let len = end - start;
    let last = (end + (len + 3) / 4).max(end + 2).min(camera.len() as i64 - 1).max(end);
    let raw = |f: i64| match camera.get(f as usize) {
        Some(&c) if c > 0.0 => Some(c),
        Some(_) => Some(fr.binary_search(&f).map_or(0.0, |i| sp[i])),
        None => None,
    };
    let v = (start..=last)
        .map(|f| {
            let near: Capped<f64, 3> = (f - 1..=f + 1).filter(|&g| g >= 0).filter_map(raw).collect();
            round(mean(&near), 1)
        })
        .collect();
    SpeedCurve { v, end: len as usize }
}

fn measure_one(f: &Flick, fps: f64, r: f64, camera: &[f64]) -> Option<Measure> {
    let tr = &f.traj;
    if tr.len() < 4 || tr[0].0 > f.start_frame + 2 {
        return None;
    }
    let fr: Vec<i64> = tr.iter().map(|p| p.0).collect();
    let d: Vec<(f64, f64)> = tr.iter().map(|p| (p.1, p.2)).collect();
    let k = d.len() - 1;
    let gap = |i: usize| (fr[i] - fr[i - 1]).max(1) as f64;
    let dist = |a: (f64, f64), b: (f64, f64)| hypot(a.0 - b.0, a.1 - b.1);
    let d0 = hypot(d[0].0, d[0].1);
    let u = if d0 > 1e-6 { (d[0].0 / d0, d[0].1 / d0) } else { (1.0, 0.0) };
    let along: Vec<f64> = d.iter().map(|&(x, y)| x * u.0 + y * u.1).collect();
    let sp: Vec<f64> =
        std::iter::once(0.0).chain((1..d.len()).map(|i| dist(d[i], d[i - 1]) * fps / gap(i))).collect();
    let toward: Vec<f64> =
        std::iter::once(0.0).chain((1..d.len()).map(|i| (along[i - 1] - along[i]) * fps / gap(i))).collect();
    // reaction: the first frame from which the crosshair closes on the target at 30 deg/s or more for 2 frames
    let mv = (1..d.len() - 1).find(|&i| toward[i] > 30.0 && toward[i + 1] > 30.0);
    let peak_i = (0..sp.len()).fold(0, |b, i| if sp[i] > sp[b] { i } else { b });
    // the main flick ends at the first frame after the peak where the speed falls below 15% of the peak
    let end_i = (peak_i..sp.len()).find(|&i| sp[i] < 0.15 * sp[peak_i]).unwrap_or(sp.len() - 1);
    let arr = (0..d.len()).find(|&i| hypot(d[i].0, d[i].1) < r);
    let past = -along[mv.unwrap_or(0)..].iter().copied().fold(f64::INFINITY, f64::min);
    // corrections: separate bursts of movement after the main flick (speed above 8 deg/s again)
    let (mut bursts, mut moving) = (0, false);
    for &s in &sp[end_i + 1..] {
        if s > 8.0 && !moving {
            (bursts, moving) = (bursts + 1, true);
        } else if s < 4.0 {
            moving = false;
        }
    }
    // holding: after the first contact, how often the crosshair slipped off the target and for how long (with a
    // little hysteresis, so a crosshair on the edge does not count as slipping off every frame)
    let (mut breaks, mut off, mut inside) = (0, 0.0, true);
    for i in arr.unwrap_or(k) + 1..=k {
        let dr = hypot(d[i].0, d[i].1);
        if inside && dr > 1.15 * r {
            (breaks, inside) = (breaks + 1, false);
        } else if !inside && dr < r {
            inside = true;
        }
        if !inside {
            off += (fr[i] - fr[i - 1]) as f64 / fps;
        }
    }
    // settling and the still time: speeds over 3 frames, since the capture moves in uneven steps
    let ds: Vec<(f64, f64)> = (0..d.len())
        .map(|i| {
            if i == 0 || i == k {
                d[i]
            } else {
                ((d[i - 1].0 + d[i].0 + d[i + 1].0) / 3.0, (d[i - 1].1 + d[i].1 + d[i + 1].1) / 3.0)
            }
        })
        .collect();
    let sps: Vec<f64> = std::iter::once(0.0).chain((1..ds.len()).map(|i| dist(ds[i], ds[i - 1]) * fps)).collect();
    let settle = arr.map(|a| (a..=k).find(|&i| sps[i.min(k)..k].iter().all(|&v| v < 10.0)).unwrap_or(k));
    let secs = |a: usize, b: usize| (fr[b] - fr[a]) as f64 / fps;
    let total = secs(0, k);
    let react = mv.map(|m| secs(0, m));
    let flick = mv.filter(|&m| end_i > m).map(|m| secs(m, end_i));
    let arrive = arr.map(|a| secs(0, a));
    let parts = match (react, flick, arrive, settle) {
        (Some(re), Some(fl), Some(ar), Some(se)) => {
            let mut b = [re, (re + fl).min(ar), ar, secs(0, se)];
            b.sort_by(f64::total_cmp);
            let b = [0.0, b[0].min(total), b[1].min(total), b[2].min(total), b[3].min(total), total];
            Some([b[1] - b[0], b[2] - b[1], b[3] - b[2], b[4] - b[3], b[5] - b[4]])
        }
        _ => None,
    };
    Some(Measure {
        n: f.n,
        shots: f.shots,
        d0,
        dir: degrees(u.1.atan2(u.0)),
        total,
        react,
        flick,
        peak: sp[peak_i],
        end_left: along[end_i],
        end_off: hypot(d[end_i].0, d[end_i].1),
        arrive,
        dwell: arr.map(|a| secs(a, k)),
        past,
        corr: bursts,
        click_speed: sp[k],
        click_off: hypot(d[k].0, d[k].1),
        click_off_xy: d[k],
        settle: arr.zip(settle).map(|(a, s)| secs(a, s)),
        still: settle.map(|s| secs(s, k)),
        start_frame: f.start_frame,
        kill_frame: f.kill_frame,
        spawned: f.spawned,
        hold: arr.map(|a| secs(a, k)),
        breaks,
        off,
        parts,
        speed: mv.filter(|&m| end_i > m).map(|m| speed_curve(fr[m], fr[end_i], camera, &fr, &sp)),
        reloads: None,
        reload_time: None,
    })
}

/// The time steps of the flick speed profile: 0 to 125% of the flick, 5% apart (past 100%: after its end).
pub const PROFILE_STEP: f64 = 0.05;
const PROFILE_POINTS: usize = 26;

/// The flick speed profile: each main flick's camera speed, as a share of its own peak, against the time as a share
/// of the flick (the points are `step` apart from 0), averaged over `n` flicks, with the 25th and 75th percentiles.
/// `peak_at`: when the peak comes, as a share of the flick; `braking`: how much of the flick the braking takes, from
/// the last frame at 90% of the peak speed to the first under 15% (medians over the flicks).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FlickProfile {
    pub n: usize,
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

/// A flick's shape; None for a flick of under 3 frames, or with no camera movement.
fn shape(c: &SpeedCurve) -> Option<Shape> {
    let v = &c.v;
    if c.end < 3 || v.len() <= c.end {
        return None;
    }
    let p = (0..=c.end).fold(0, |b, i| if v[i] > v[b] { i } else { b });
    let peak = v[p];
    if peak <= 0.0 {
        return None;
    }
    let stop = (p..v.len()).find(|&i| v[i] < 0.15 * peak).unwrap_or(v.len() - 1);
    let high = (p..=stop).rev().find(|&i| v[i] >= 0.9 * peak).unwrap_or(p);
    let at = std::array::from_fn(|k| {
        let x = k as f64 * PROFILE_STEP * c.end as f64;
        let i = x.floor() as usize;
        let s = if i + 1 < v.len() { v[i] + (v[i + 1] - v[i]) * (x - i as f64) } else { v[v.len() - 1] };
        s / peak
    });
    Some(Shape { at, peak_at: p as f64 / c.end as f64, braking: (stop - high) as f64 / c.end as f64 })
}

/// The flick speed profile over the measured flicks; None with fewer than 3 flicks to average.
pub fn flick_profile(ms: &[Measure]) -> Option<FlickProfile> {
    let shapes: Vec<Shape> = ms.iter().filter_map(|m| m.speed.as_ref().and_then(shape)).collect();
    if shapes.len() < 3 {
        return None;
    }
    let column = |k: usize| shapes.iter().map(|s| s.at[k]).collect::<Vec<f64>>();
    let points = |f: &dyn Fn(&[f64]) -> f64| std::array::from_fn(|k| round(f(&column(k)), 3));
    let middle = |f: &dyn Fn(&Shape) -> f64| round(median(&shapes.iter().map(f).collect::<Vec<f64>>()), 3);
    Some(FlickProfile {
        n: shapes.len(),
        step: PROFILE_STEP,
        mean: points(&mean),
        p25: points(&|c| numpy_percentile(c, 25.0)),
        p75: points(&|c| numpy_percentile(c, 75.0)),
        peak_at: middle(&|s| s.peak_at),
        braking: middle(&|s| s.braking),
    })
}

/// For each kill after the first: was the next target the nearest one on screen 3 frames after the kill.
pub fn choices(tracks: &Tracks, flicks: &[Flick]) -> Vec<Choice> {
    let by_frame: std::collections::HashMap<i64, &Vec<crate::track::TrackPoint>> =
        tracks.frames.iter().map(|f| (f.i as i64, &f.t)).collect();
    let mut out = Vec::new();
    for w in flicks.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let kf = a.kill_frame;
        let Some(ts) = by_frame.get(&(kf + 3)).filter(|t| !t.is_empty()) else { continue };
        let Some(&(_, x, y)) = b.traj.iter().find(|p| p.0 >= kf + 3) else { continue };
        let mut dists: Vec<f64> = ts.iter().map(|&(_, x, y)| hypot(x, y)).collect();
        dists.sort_by(f64::total_cmp);
        let chosen = hypot(x, y);
        out.push(Choice { n: b.n, rank: dists.iter().filter(|&&d| d < chosen - 0.3).count(), extra: chosen - dists[0] });
    }
    out
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
        let traj: Vec<(i64, f64, f64)> = moves
            .iter()
            .enumerate()
            .map(|(i, m)| {
                x -= m;
                (i as i64, x, 0.0)
            })
            .collect();
        let frames = moves
            .iter()
            .enumerate()
            .map(|(i, &m)| TrackFrame {
                i,
                shift: if m > 6.0 { (0.0, 0.0) } else { (-m, 0.0) },
                t: Vec::new(),
                a: Vec::new(),
                wh: None,
                s: None,
            })
            .collect();
        let tracks = Tracks { fps: 100.0, frames, version: 0 };
        let flick = Flick {
            n: 1,
            kill_frame: 15,
            stats_frame: Some(15),
            start_frame: 0,
            shots: Some(1),
            traj,
            spawned: false,
            area: None,
        };
        let ms = measure(&[flick.clone(), flick.clone(), flick], &tracks, 0.5);
        let c = ms[0].speed.as_ref().unwrap();
        // from the flick's start (frame 5) to its end (frame 11, under 15% of the peak), and 2 frames more, each over 3
        // frames
        assert_eq!(c.v, [400.0, 800.0, 1066.7, 1066.7, 800.0, 433.3, 166.7, 33.3, 0.0]);
        assert_eq!(c.end, 6);
        let p = flick_profile(&ms).unwrap();
        assert_eq!((p.n, p.mean.len()), (3, 26));
        assert_eq!((p.mean[0], p.mean[10], p.mean[20], p.mean[25]), (0.375, 1.0, 0.156, 0.016));
        assert_eq!((p.p25[5], p.p75[5]), (p.mean[5], p.mean[5]));
        // the peak at frame 2 of 6; the braking from the last frame at 90% (3) to the first under 15% (7, after the end)
        assert_eq!((p.peak_at, p.braking), (0.333, 0.667));
    }
}
