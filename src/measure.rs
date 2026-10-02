//! Each flick measured (review.py: `target_radius`, `measure`, `choices`).

use serde::Serialize;

use crate::geometry::{degrees, K};
use crate::matching::Flick;
use crate::python::hypot;
use crate::track::Tracks;

/// One flick's measures (seconds, degrees and degrees a second): the keys measure.py has always written, plus
/// settle, still and the time parts.
#[derive(Clone, Debug, Serialize)]
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
    pub parts: Option<[f64; 5]>,
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

pub fn measure(flicks: &[Flick], fps: f64, r: f64) -> Vec<Measure> {
    flicks.iter().filter_map(|f| measure_one(f, fps, r)).collect()
}

fn measure_one(f: &Flick, fps: f64, r: f64) -> Option<Measure> {
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
