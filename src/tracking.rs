//! A tracking run's summary (review.py: `track_summary`, `track_motion`, `what_if`, `stats_length`,
//! `countdown_end`, `tracking_crosshair`, `without_crosshair`): how the crosshair stayed on the target, from the tracks
//! and the camera's turn.

use std::collections::HashMap;

use serde::Serialize;

use crate::faint::picked;
use crate::geometry::{K, degrees};
use crate::optional_fields::OptionalFields;
use crate::matching::KillSource;
use crate::python::{hypot, round};
use crate::stats_file::StatsFile;
use crate::statistics::median;
use crate::summary::DIRECTIONS;
use crate::track::{TrackFrame, Tracks};

/// The camera's reading for a frame: the room's move on screen since the frame before (degrees; the camera turned by
/// minus that) and how many tiles agreed on it, or None.
pub type CameraReading = Option<(f64, f64, usize)>;

/// The radius in degrees of a target of `a` pixels taken as a disc.
fn disc(a: i64) -> f64 {
    degrees(((a as f64 / std::f64::consts::PI).sqrt() / K).atan())
}

/// A target's box (w, h, degrees): the model's, else its area as a disc.
fn box_of(f: &TrackFrame, k: usize) -> (f64, f64) {
    match &f.wh {
        Some(wh) => wh[k],
        None => {
            let s = 2.0 * disc(f.a[k]);
            (s, s)
        }
    }
}

/// The target nearest the crosshair in a frame, with the crosshair's offset from its center line (a sphere's
/// center, a capsule's long axis): its distance d and parts lx, ly, and the target's half-width r.
#[derive(Clone, Copy)]
struct Nearest {
    d: f64,
    tid: u32,
    x: f64,
    y: f64,
    lx: f64,
    ly: f64,
    r: f64,
}

fn nearest(f: &TrackFrame) -> Option<Nearest> {
    let mut best: Option<(f64, usize)> = None;
    for (k, &(_, x, y)) in f.t.iter().enumerate() {
        let d = hypot(x, y);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, k));
        }
    }
    let (_, k) = best?;
    let (tid, x, y) = f.t[k];
    let (w, h) = box_of(f, k);
    let lx = (x.abs() - (w - h).max(0.0) / 2.0).max(0.0).copysign(x);
    let ly = (y.abs() - (h - w).max(0.0) / 2.0).max(0.0).copysign(y);
    Some(Nearest { d: hypot(lx, ly), tid, x, y, lx, ly, r: w.min(h) / 2.0 })
}

/// The moving mean over k frames (k odd, centered) of values with gaps (NaN); NaN where none is in reach.
fn smooth(v: &[(f64, f64)], k: usize) -> Vec<(f64, f64)> {
    let half = k / 2;
    (0..v.len())
        .map(|i| {
            let w = &v[i.saturating_sub(half)..(i + half + 1).min(v.len())];
            let ok: Vec<&(f64, f64)> = w.iter().filter(|p| !p.0.is_nan()).collect();
            if ok.is_empty() {
                (f64::NAN, f64::NAN)
            } else {
                let n = ok.len() as f64;
                (ok.iter().map(|p| p.0).sum::<f64>() / n, ok.iter().map(|p| p.1).sum::<f64>() / n)
            }
        })
        .collect()
}

fn axis(p: (f64, f64), ax: usize) -> f64 {
    if ax == 0 { p.0 } else { p.1 }
}

/// The direction sector (0 = right, counterclockwise in 45-degree steps) of a motion.
fn sector(v: (f64, f64)) -> usize {
    ((degrees(v.1.atan2(v.0)).rem_euclid(360.0) / 45.0).round_ties_even() as usize) % 8
}

fn med(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| median(v))
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
        .map(|(k, &j)| {
            let next = turns.get(k + 1).map_or(end, |&t| t.min(end));
            let until = (j..next).find(|&t| stop[t]).unwrap_or(next);
            let back = match (j..until.min(j + window)).find(|&t| !inside[t]) {
                None => Some(0.0),
                Some(left) => (left + 1..until).find(|&t| inside[t]).map(|t| (t - j) as f64 / fps),
            };
            TurnBack { frame: j, back }
        })
        .collect()
}

/// Tracking diagnostics from the target's own motion and the camera's (see review.py: `track_motion`).
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

/// Tracking diagnostics, frames i0 to i1: the target is the track nearest the crosshair; its own motion is its move
/// on screen less the room's move, the mouse's is the camera's turn, both smoothed over 0.05 s. Measured while the
/// target moves (over 5 deg/s), the crosshair is engaged with it, and no bot has just died (`sw`).
pub fn track_motion(
    frames: &[TrackFrame],
    fps: f64,
    cam: &[CameraReading],
    i0: usize,
    i1: usize,
    inside: &[bool],
    sw: &[bool],
) -> Motion {
    let n = frames.len();
    let i1 = i1.min(n);
    let span = i1.saturating_sub(i0);
    let mut tgt: Vec<Option<Nearest>> = vec![None; n];
    for i in i0..i1 {
        tgt[i] = nearest(&frames[i]);
    }
    let nan = (f64::NAN, f64::NAN);
    let (mut own, mut mouse) = (vec![nan; n], vec![nan; n]);
    for i in i0 + 1..i1 {
        let Some(Some(c)) = cam.get(i) else { continue };
        mouse[i] = (-c.0 * fps, -c.1 * fps);
        if let (Some(a), Some(b)) = (tgt[i - 1], tgt[i])
            && a.tid == b.tid
        {
            own[i] = ((b.x - a.x - c.0) * fps, (b.y - a.y - c.1) * fps);
        }
    }
    let k = ((0.05 * fps).round_ties_even() as usize | 1).max(3);
    let (own, mouse) = (smooth(&own, k), smooth(&mouse, k));
    let speed: Vec<f64> = own.iter().map(|p| p.0.hypot(p.1)).collect();
    let engaged = |t: &Nearest| t.d <= (5.0 * t.r).max(2.0);
    let moving: Vec<usize> = (i0..i1)
        .filter(|&i| {
            tgt[i].is_some_and(|t| engaged(&t)) && !speed[i].is_nan() && speed[i] > 5.0 && !sw[i]
        })
        .collect();
    let near: Vec<Nearest> = (i0..i1).filter_map(|i| tgt[i]).filter(|t| t.d <= 3.0 * t.r.max(0.2)).collect();
    let mut out = Motion {
        camera: (i0..i1).filter(|&i| cam.get(i).is_some_and(Option::is_some)).count() as f64 / span.max(1) as f64,
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
        error_h: med(&near.iter().map(|t| t.lx.abs()).collect::<Vec<_>>()),
        error_v: med(&near.iter().map(|t| t.ly.abs()).collect::<Vec<_>>()),
        seconds: moving.len() as f64 / fps,
        around: None,
        turns_back: None,
        reason: None,
        counts: OptionalFields(None),
    };
    if (moving.len() as f64) < fps * (0.2 * span as f64 / fps).max(10.0) {
        out.reason = Some("too little tracking of a moving target to read");
        return out;
    }
    // along and across the target's motion: the crosshair's offset from its center line
    let mut al = vec![f64::NAN; n];
    let mut ra = vec![f64::NAN; n];
    let mut around = Vec::with_capacity(moving.len());
    for &i in &moving {
        let t = tgt[i].unwrap();
        let (ux, uy) = (own[i].0 / speed[i], own[i].1 / speed[i]);
        let (cx, cy) = (-t.lx, -t.ly);
        al[i] = cx * ux + cy * uy;
        ra[i] = t.r;
        around.push([al[i], -cx * uy + cy * ux, t.r]);
    }
    out.around = Some(around.into_boxed_slice());
    let of = |f: &dyn Fn(usize) -> f64| moving.iter().map(|&i| f(i)).collect::<Vec<f64>>();
    out.target_speed = med(&of(&|i| speed[i]));
    out.mouse_speed = med(&of(&|i| mouse[i].0.hypot(mouse[i].1)).into_iter().filter(|v| !v.is_nan()).collect::<Vec<_>>());
    out.lag = med(&of(&|i| al[i]));
    out.lag_ms = med(&of(&|i| al[i] / speed[i])).map(|v| v * 1000.0);
    let off: Vec<usize> = moving.iter().copied().filter(|&i| !inside[i]).collect();
    if !off.is_empty() {
        let ahead = off.iter().filter(|&&i| al[i] > ra[i]).count() as f64;
        let behind = off.iter().filter(|&&i| al[i] < -ra[i]).count() as f64;
        let m = off.len() as f64;
        (out.off_ahead, out.off_behind, out.off_side) = (Some(ahead / m), Some(behind / m), Some(1.0 - (ahead + behind) / m));
    }
    let seconds = moving.len() as f64 / fps;
    let mut past = vec![false; n];
    for &i in &moving {
        past[i] = al[i] > ra[i] + 0.05;
    }
    let eps: Vec<(usize, usize)> =
        crate::scipy::runs(&crate::scipy::close_line(&past, 1)).into_iter().filter(|(a, b)| b - a >= 2).collect();
    out.overshoots = Some(eps.len() as f64 / seconds);
    if !eps.is_empty() {
        let worst: Vec<f64> = eps
            .iter()
            .map(|&(a, b)| (a..b).filter(|&i| !al[i].is_nan()).map(|i| al[i] - ra[i]).fold(f64::NEG_INFINITY, f64::max))
            .collect();
        out.overshoot_dist = med(&worst);
    }
    // direction changes of the target, on each axis
    let d = (0.1 * fps).round_ties_even() as usize;
    let mut revs: Vec<(usize, usize, f64)> = Vec::new();
    for ax in 0..2 {
        let v = |i: usize| axis(own[i], ax);
        let mut last: i64 = -1_000_000_000;
        for i in i0 + d..i1.saturating_sub(d) {
            let (a, b) = (v(i - d), v(i + d));
            if a.is_nan() || b.is_nan() || a * b >= 0.0 || a.abs().min(b.abs()) < 8.0 || (i as i64) - last < 2 * d as i64
            {
                continue;
            }
            // the change: where it is slowest (NumPy's argmin: the first NaN, if any)
            let win = i - d..=i + d;
            let j = win.clone().find(|&t| v(t).is_nan()).unwrap_or_else(|| {
                win.fold(i - d, |b, t| if v(t).abs() < v(b).abs() { t } else { b })
            });
            if (j as i64) - last >= 2 * d as i64 && tgt[j].is_some_and(|t| engaged(&t)) && !sw[j] {
                revs.push((j, ax, if b > 0.0 { 1.0 } else { -1.0 }));
                last = j as i64;
            }
        }
    }
    let (half, turn, quarter) = ((0.5 * fps) as usize, (0.4 * fps) as usize, (0.25 * fps) as usize);
    let mut at: Vec<usize> = revs.iter().map(|r| r.0).collect();
    at.sort_unstable();
    at.dedup_by(|a, b| *a - *b < 2 * d);
    out.turns_back = Some(turns_back(&at, inside, sw, turn, i1, fps));
    let (mut react, mut ov) = (Vec::new(), Vec::new());
    for &(j, ax, sgn) in &revs {
        if let Some(r) = (j..i1.min(j + half)).find(|&t| {
            let m = axis(mouse[t], ax);
            !m.is_nan() && m * sgn > 3.0
        }) {
            react.push((r - j) as f64 / fps * 1000.0);
        }
        let o: Vec<f64> =
            (j..i1.min(j + turn)).filter_map(|t| tgt[t].map(|g| axis((g.lx, g.ly), ax) * sgn - g.r)).collect();
        if !o.is_empty() {
            ov.push(o.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        }
    }
    let mut steady = vec![false; n];
    for &i in &moving {
        steady[i] = true;
    }
    for &(j, _, _) in &revs {
        for s in &mut steady[j.saturating_sub(quarter)..(j + quarter).min(n)] {
            *s = false;
        }
    }
    // corrections: each turn of the crosshair back toward the bot's middle along its motion (jitter under 0.05 deg
    // ignored); swings: the crosshair crossing over the middle to the other side by half the bot's width or more.
    // Both in steady motion only, and counted afresh after a gap.
    let (mut swings, mut state, mut corrections) = (0usize, 0i32, 0usize);
    let mut zz: Option<(i32, f64)> = None;
    let mut prev: Option<usize> = None;
    for &i in &moving {
        if !steady[i] || prev.is_some_and(|p| i - p > 2) {
            (state, zz) = (0, None);
        }
        prev = Some(i);
        if !steady[i] {
            continue;
        }
        let hcut = (ra[i] / 2.0).max(0.05);
        let s = if al[i] > hcut { 1 } else if al[i] < -hcut { -1 } else { 0 };
        if s != 0 && state != 0 && s != state {
            swings += 1;
        }
        if s != 0 {
            state = s;
        }
        let v = al[i];
        zz = match zz {
            None => Some((0, v)),
            Some((0, z)) if (v - z).abs() > 0.05 => Some((if v > z { 1 } else { -1 }, v)),
            Some((dir, z)) if dir != 0 && (v - z) * dir as f64 > 0.0 => Some((dir, v)),
            Some((dir, z)) if dir != 0 && (v - z).abs() > 0.05 => {
                corrections += 1;
                Some((-dir, v))
            }
            same => same,
        };
    }
    let far: Vec<f64> = ov.iter().copied().filter(|&o| o > 0.05).collect();
    out.reversals = revs.len();
    out.reaction = med(&react);
    out.reversal_overshoot = (!ov.is_empty()).then(|| far.len() as f64 / ov.len() as f64);
    out.reversal_overshoot_dist = med(&far);
    out.swings = Some(swings as f64 / (steady.iter().filter(|&&s| s).count() as f64 / fps).max(1e-9));
    // what the off-target time went on, in frames (for the what-if estimates)
    let mut turn_win = vec![false; n];
    for &(j, _, _) in &revs {
        for t in &mut turn_win[j..(j + turn).min(n)] {
            *t = true;
        }
    }
    let mut dirs = [(0usize, 0usize); 8];
    for &i in &moving {
        let k = sector(own[i]);
        dirs[k].0 += 1;
        dirs[k].1 += inside[i] as usize;
    }
    let best = dirs
        .iter()
        .filter(|v| v.0 > 0 && v.0 as f64 >= 0.05 * moving.len() as f64)
        .map(|v| v.1 as f64 / v.0 as f64)
        .fold(0.0, f64::max);
    *out.counts = Some(MotionCounts {
        swing_count: swings,
        corrections,
        overcorrect: (corrections > 0).then(|| swings as f64 / corrections as f64),
        frames: OffFrames {
            ahead: off.iter().filter(|&&i| al[i] > ra[i]).count(),
            behind: off.iter().filter(|&&i| al[i] < -ra[i]).count(),
            turns: (i0..i1).filter(|&i| turn_win[i] && !inside[i] && !sw[i]).count(),
            directions: dirs.iter().filter(|v| v.0 > 0).map(|v| (best * v.0 as f64 - v.1 as f64).max(0.0)).sum(),
        },
    });
    for (k, &name) in DIRECTIONS.iter().enumerate() {
        let g: Vec<usize> = moving.iter().copied().filter(|&i| sector(own[i]) == k).collect();
        if !g.is_empty() {
            out.by_direction.push(MotionDirection {
                name,
                share: g.len() as f64 / moving.len() as f64,
                on: g.iter().filter(|&&i| inside[i]).count() as f64 / g.len() as f64,
                distance: median(&g.iter().map(|&i| tgt[i].unwrap().d).collect::<Vec<_>>()),
                lag: median(&g.iter().map(|&i| al[i]).collect::<Vec<_>>()),
            });
        }
    }
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

/// A tracking run's summary (see review.py: `track_summary`).
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
    let number = |t: &str| t.split('.').map(|p| p.parse().ok()).collect::<Option<Vec<i64>>>();
    let (date, time) = (number(date)?, number(time)?);
    let (&[_, _, _], &[h, m, s]) = (&date[..], &time[..]) else { return None };
    let end = ((h * 60 + m) * 60 + s) * 1_000_000;
    let t0 = file.start_micros()?;
    let d = ((end - t0) as f64 / 1e6).rem_euclid(86400.0);
    (0.0 < d && d < 3600.0).then(|| d.ceil())
}

/// The frame a run starts on: the one after the last frame showing KovaaK's countdown (`showing`, per frame) in the
/// first `until` seconds, or None.
pub fn countdown_end(showing: &[bool], fps: f64, until: f64) -> Option<usize> {
    let until: f64 = format!("{until:.2}").parse().unwrap();
    showing.iter().enumerate().take_while(|&(i, _)| (i as f64) / fps < until).filter(|&(_, &s)| s).map(|(i, _)| i + 1).last()
}

/// Where a detector marks the crosshair in a tracking run: the points its box sits on (degrees from the crosshair),
/// and the box's width and height (degrees).
struct CrosshairBox {
    points: Vec<(f64, f64)>,
    size: (f64, f64),
}

/// The crosshair's box in a tracking run (review.py: `tracking_crosshair`): piles of boxes within 0.015 degrees of a
/// point, the first in 5% of all the frames or more, then up to two more within 0.3 degrees of it in 2% or more. A bot
/// never stays that still. Only boxes with a size count. Its size: the median width and height of the boxes within
/// 0.02 degrees of the first point. None without such points.
fn crosshair_box(frames: &[TrackFrame]) -> Option<CrosshairBox> {
    const NEAR: f64 = 0.5;
    const STEP: f64 = 0.01;
    let p: Vec<(f64, f64, f64, f64)> = frames
        .iter()
        .filter_map(|f| f.wh.as_ref().map(|wh| f.t.iter().zip(wh).map(|(&(_, x, y), &(w, h))| (x, y, w, h))))
        .flatten()
        .filter(|&(x, y, _, _)| hypot(x, y) < NEAR)
        .collect();
    if p.len() < 30 {
        return None;
    }
    // NumPy's histogram2d: edges as `linspace` makes them, a value on an edge in the bin above
    let n = (2.0 * NEAR / STEP).round_ties_even() as usize;
    let step = 2.0 * NEAR / n as f64;
    let mut edges: Vec<f64> = (0..=n).map(|k| k as f64 * step + -NEAR).collect();
    edges[n] = NEAR;
    let bin = |v: f64| edges.partition_point(|&e| e <= v) - 1;
    let mut h = vec![0.0; n * n];
    for &(x, y, _, _) in &p {
        h[bin(x) * n + bin(y)] += 1.0;
    }
    let centers: Vec<f64> = (0..n).map(|k| -NEAR + (k as f64 + 0.5) * STEP).collect();
    let within = |c: (f64, f64)| p.iter().filter(move |q| (q.0 - c.0).hypot(q.1 - c.1) < 0.02).collect::<Vec<_>>();
    let mean = |v: &[&(f64, f64, f64, f64)], f: fn(&(f64, f64, f64, f64)) -> f64| {
        v.iter().map(|&q| f(q)).sum::<f64>() / v.len() as f64
    };
    let (mut out, mut size): (Vec<(f64, f64)>, (f64, f64)) = (Vec::new(), (0.0, 0.0));
    for _ in 0..3 {
        // each bin with its 8 neighbours (wrapping round, as np.roll does), the first largest; after the first point,
        // only bins within 0.3 degrees of it
        let mut best = (0, 0, f64::NEG_INFINITY);
        for i in 0..n {
            for j in 0..n {
                let mut b = 0.0;
                if out.first().is_none_or(|o| (centers[i] - o.0).hypot(centers[j] - o.1) <= 0.3) {
                    for a in [n - 1, 0, 1] {
                        for c in [n - 1, 0, 1] {
                            b += h[((i + a) % n) * n + (j + c) % n];
                        }
                    }
                }
                if b > best.2 {
                    best = (i, j, b);
                }
            }
        }
        let (i, j, b) = best;
        let need = if out.is_empty() { 0.05 } else { 0.02 };
        if b < (need * frames.len() as f64).max(25.0) {
            break;
        }
        let on = within((centers[i], centers[j]));
        if on.is_empty() {
            break;
        }
        let c = (mean(&on, |q| q.0), mean(&on, |q| q.1));
        if out.is_empty() {
            let on = within(c);
            let side = |f: fn(&(f64, f64, f64, f64)) -> f64| median(&on.iter().map(|&q| f(q)).collect::<Vec<_>>());
            size = (side(|q| q.2), side(|q| q.3));
        }
        out.push(c);
        for (a, &gx) in centers.iter().enumerate() {
            for (b, &gy) in centers.iter().enumerate() {
                if (gx - c.0).hypot(gy - c.1) < 0.06 {
                    h[a * n + b] = 0.0;
                }
            }
        }
    }
    (!out.is_empty()).then_some(CrosshairBox { points: out, size })
}

/// The frames without the boxes a detector puts on the crosshair in a tracking run (review.py: `without_crosshair`):
/// those within 0.1 degrees of one of its points (`crosshair_box`), with a width and a height within 20% of its
/// box's. None when the run has no such points.
fn without_crosshair(frames: &[TrackFrame]) -> Option<Vec<TrackFrame>> {
    let CrosshairBox { points, size: (w0, h0) } = crosshair_box(frames)?;
    let crosshair = |x: f64, y: f64, w: f64, h: f64| {
        points.iter().any(|&(a, b)| hypot(x - a, y - b) < 0.1)
            && (w - w0).abs() <= 0.2 * w0
            && (h - h0).abs() <= 0.2 * h0
    };
    let frames = frames
        .iter()
        .map(|f| {
            let Some(wh) = &f.wh else { return f.clone() };
            let keep: Vec<usize> = (f.t.iter().zip(wh).enumerate())
                .filter(|&(_, (&(_, x, y), &(w, h)))| !crosshair(x, y, w, h))
                .map(|(k, _)| k)
                .collect();
            TrackFrame {
                i: f.i,
                shift: f.shift,
                t: picked(&f.t, &keep),
                a: picked(&f.a, &keep),
                wh: Some(picked(wh, &keep)),
                s: f.s.as_ref().map(|v| picked(v, &keep)),
            }
        })
        .collect();
    Some(frames)
}

/// How the crosshair stayed on the target in a tracking run. `limit`: the run's length (seconds); `start`: its first
/// frame when known; `deaths`: the frames where bots die; `cam`: the camera's readings. The boxes a detector puts on
/// the crosshair are left out first (`without_crosshair`).
pub fn track_summary(
    tracks: &Tracks,
    meta: &HashMap<String, String>,
    limit: Option<f64>,
    cam: Option<&[CameraReading]>,
    deaths: &[i64],
    start: Option<i64>,
    source: KillSource,
) -> TrackSummary {
    const GAP: f64 = 0.1;
    let kept = without_crosshair(&tracks.frames);
    let (fr, fps): (&[TrackFrame], f64) = (kept.as_deref().unwrap_or(&tracks.frames), tracks.fps);
    let mut near: Vec<Option<(f64, f64)>> = Vec::with_capacity(fr.len());
    let mut inside = Vec::with_capacity(fr.len());
    for f in fr {
        let (mut best, mut ins): (Option<(f64, f64)>, bool) = (None, false);
        for (k, (&(_, x, y), &a)) in f.t.iter().zip(&f.a).enumerate() {
            let (d, r) = match &f.wh {
                Some(wh) => {
                    let (w, h) = wh[k];
                    ins = ins || (x.abs() <= w / 2.0 + 0.05 && y.abs() <= h / 2.0 + 0.05);
                    let d = hypot((x.abs() - (w - h).max(0.0) / 2.0).max(0.0), (y.abs() - (h - w).max(0.0) / 2.0).max(0.0));
                    (d, w.min(h) / 2.0)
                }
                None => {
                    let (r, d) = (disc(a), hypot(x, y));
                    ins = ins || d <= r + 0.05;
                    (d, r)
                }
            };
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, r));
            }
        }
        near.push(best);
        inside.push(ins);
    }
    let (near, inside) = (near.into_boxed_slice(), inside.into_boxed_slice());
    let num = |k: &str| meta.get(k).filter(|v| !v.is_empty()).and_then(|v| v.trim().parse::<f64>().ok());
    let (hits, miss) = (num("Hit Count"), num("Miss Count"));
    let mut s = TrackSummary {
        scenario: meta.get("Scenario").cloned(),
        score: num("Score"),
        accuracy: match (hits, miss) {
            (Some(h), Some(m)) if h + m > 0.0 => Some(h / (h + m)),
            _ => None,
        },
        fps_avg: num("Avg FPS"),
        mode: crate::summary::Mode::Track,
        sens: meta.get("Horiz Sens").filter(|v| !v.is_empty()).map(|h| {
            format!("{h} {}", meta.get("Sens Scale").map(String::as_str).unwrap_or("None"))
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
    };
    let idx: Vec<usize> = (0..near.len()).filter(|&i| near[i].is_some()).collect();
    let (Some(&first), Some(&last_seen)) = (idx.first(), idx.last()) else { return s };
    let frames_of = |seconds: f64| (seconds * fps).round_ties_even() as usize;
    let (mut i0, mut i1) = (first, last_seen + 1);
    if let Some(st) = start {
        i0 = st.max(0) as usize;
        if let Some(l) = limit.filter(|&l| l != 0.0) {
            i1 = near.len().min(i0 + frames_of(l));
        }
    } else if let Some(l) = limit.filter(|&l| l != 0.0) {
        // nothing places the start: the run ends at its last frame on a target, and starts its length before
        let last = (0..inside.len()).rev().find(|&i| inside[i]).unwrap_or(i1 - 1);
        i1 = last + 1;
        i0 = first.max(i1.saturating_sub(frames_of(l)));
    }
    if let Some(&most) = deaths.iter().max()
        && most >= i1 as i64
    {
        // the run went on past the limit given: up to its last death
        i1 = near.len().min(most as usize + fps as usize);
    }
    let mut sw = vec![false; fr.len()];
    let mut ds: Vec<usize> = deaths.iter().filter(|&&d| i0 as i64 <= d && d < i1 as i64).map(|&d| d as usize).collect();
    ds.sort();
    ds.dedup();
    for (k, &d) in ds.iter().enumerate() {
        let end = ds.get(k + 1).copied().unwrap_or(i1);
        let back_on = (d + 1..end).find(|&t| inside[t]).unwrap_or(end);
        let seen = (d + 1..=back_on).find(|&t| t < fr.len() && !fr[t].t.is_empty()).unwrap_or(back_on);
        for v in &mut sw[d..back_on] {
            *v = true;
        }
        s.switches.push([d, back_on, seen]);
    }
    let span = i0..i1.max(i0);
    let tracking: Vec<bool> = span.clone().map(|i| !sw[i]).collect();
    let on: Vec<bool> = span.clone().map(|i| inside[i] && !sw[i]).collect();
    let err: Vec<f64> =
        span.clone().filter(|&i| !sw[i]).filter_map(|i| near[i]).filter(|b| b.0 <= 3.0 * b.1).map(|b| b.0).collect();
    let lost_mask: Vec<bool> = on.iter().zip(&tracking).map(|(&o, &t)| !o && t).collect();
    let offs: Vec<usize> = crate::scipy::runs(&lost_mask)
        .into_iter()
        .map(|(a, b)| b - a)
        .filter(|&k| k as f64 / fps > GAP)
        .collect();
    let sec = (fps.round_ties_even() as usize).max(1);
    let t_all = tracking.iter().filter(|&&t| t).count();
    let on_all = on.iter().filter(|&&o| o).count();
    let t = t_all.max(1) as f64;
    let lost: usize = offs.iter().sum();
    let mean = |v: &[bool]| v.iter().filter(|&&b| b).count() as f64 / v.len() as f64;
    s.on_target = Some(on_all as f64 / t);
    s.error = med(&err);
    s.lost = Some(offs.len() as f64 / (t_all as f64 / fps).max(1e-9));
    s.back = med(&offs.iter().map(|&k| k as f64).collect::<Vec<_>>()).map(|m| m / fps);
    s.longest_off = offs.iter().max().map(|&k| k as f64 / fps);
    s.start = Some(i0);
    s.end = Some(i1);
    *s.shares = Some(RunShares {
        on_all: mean(&span.clone().map(|i| inside[i]).collect::<Vec<_>>()),
        lost_cost: lost as f64 / t,
        slip_cost: (1.0 - on_all as f64 / t - lost as f64 / t).max(0.0),
    });
    let switching: Vec<bool> = tracking.iter().map(|&t| !t).collect();
    s.per_second = (0..on.len())
        .step_by(sec)
        .map(|k| {
            let e = (k + sec).min(on.len());
            [round(mean(&on[k..e]), 3), round(mean(&switching[k..e]), 3)]
        })
        .collect();
    if !ds.is_empty() {
        let w = &s.switches;
        let gap_med = |f: &dyn Fn(&[usize; 3]) -> usize| median(&w.iter().map(|x| f(x) as f64).collect::<Vec<_>>()) / fps;
        s.bots = ds.len();
        s.to_next = Some(gap_med(&|x| x[1] - x[0]));
        s.waiting = Some(gap_med(&|x| x[2] - x[0]));
        s.onto = Some(gap_med(&|x| x[1] - x[2]));
        s.switching = Some(mean(&switching));
    }
    if let Some(cam) = cam {
        s.motion = Some(track_motion(fr, fps, cam, i0, i1, &inside, &sw));
    }
    s.what_if = Some(what_if(&s, &tracking, &on, &offs, fps));
    s
}

/// Estimates of how much the time on target (and so the accuracy) would rise if one thing changed, all else the same:
/// the off-target time that thing accounts for, as a share of the whole run. They overlap, so they do not add up;
/// each is a ceiling. Biggest first.
fn what_if(s: &TrackSummary, tracking: &[bool], on: &[bool], offs: &[usize], fps: f64) -> Vec<WhatIf> {
    let count = |v: &[bool]| v.iter().filter(|&&b| b).count();
    let (t, r) = (count(tracking).max(1) as f64, tracking.len().max(1) as f64);
    let mut out = Vec::new();
    let mut add = |what: &'static str, frames: f64, how: String| {
        if frames > 0.0 {
            out.push(WhatIf { what, gain: frames / r, how });
        }
    };
    let lost = offs.iter().sum::<usize>() as f64;
    add("Get back on twice as fast", lost / 2.0, "Half the time off the bot in drops longer than 0.1 s.".into());
    add("Don't slip", (t - count(on) as f64 - lost).max(0.0), "The time off the bot in slips shorter than 0.1 s.".into());
    if let Some(f) = s.motion.as_ref().and_then(|m| m.counts.as_ref()).map(|c| &c.frames) {
        add("Don't lead", f.ahead as f64, "The time off the bot ahead of it, past its leading edge.".into());
        add("Don't trail", f.behind as f64, "The time off the bot behind it, trailing it.".into());
        add(
            "Don't get thrown by its turns",
            f.turns as f64,
            "The time off the bot in the 0.4 s after each of its direction changes.".into(),
        );
        add(
            "Track every direction like your best one",
            f.directions,
            "Each direction of the bot's motion brought up to the time on target of your best one (among those it \
             moved in 5% of the time or more)."
                .into(),
        );
    }
    let sec = (fps.round_ties_even() as usize).max(1);
    let per: Vec<(usize, usize)> = (0..on.len())
        .step_by(sec)
        .map(|k| {
            let e = (k + sec).min(on.len());
            (count(&on[k..e]), count(&tracking[k..e]))
        })
        .collect();
    if per.len() >= 20 {
        let best10 = per
            .windows(10)
            .map(|w| (w.iter().map(|p| p.0).sum::<usize>(), w.iter().map(|p| p.1).sum::<usize>()))
            .filter(|&(_, b)| b >= 5 * sec)
            .map(|(a, b)| a as f64 / b as f64)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        if let (Some(best10), Some(on_target)) = (best10, s.on_target) {
            add(
                "Keep up your best 10 seconds all run",
                (best10 - on_target) * t,
                format!("Your best 10 seconds were {}% on target.", (100.0 * best10).round_ties_even()),
            );
        }
    }
    if !s.switches.is_empty() {
        let total: usize = s.switches.iter().map(|x| x[1] - x[0]).sum();
        add(
            "Get onto the next bot 100 ms faster",
            (s.switches.len() as f64 * 0.1 * fps).min(total as f64),
            "100 ms less per switch between bots.".into(),
        );
    }
    out.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_back_times_the_way_back_onto_the_bot() {
        // 10 fps: on, then off from frame 3 after the turn at 2, back on at 6; a turn at 10 the crosshair stays on
        // through; a turn at 15 it leaves and is not back before the turn at 18; one at 18 it is not back by the end
        let on = |f: &[usize]| (0..22).map(|i| !f.contains(&i)).collect::<Vec<bool>>();
        let inside = on(&[3, 4, 5, 16, 17, 18, 19, 20, 21]);
        let stop = vec![false; 22];
        let got = turns_back(&[2, 10, 15, 18], &inside, &stop, 4, 22, 10.0);
        let back: Vec<Option<f64>> = got.iter().map(|t| t.back).collect();
        assert_eq!(back, [Some(0.4), Some(0.0), None, None]);
        assert_eq!(got.iter().map(|t| t.frame).collect::<Vec<_>>(), [2, 10, 15, 18]);
        // a bot dying (a stop frame) before the crosshair is back ends the search
        let mut stop = stop;
        stop[5] = true;
        assert_eq!(turns_back(&[2], &inside, &stop, 4, 22, 10.0)[0].back, None);
    }
}
