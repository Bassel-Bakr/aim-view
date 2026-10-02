//! Each kill matched to the target it killed and the flick to it (review.py: `match_times`, `_attach_kills`,
//! `appearances`, `crosshair_spots`).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::python::hypot;
use crate::track::{TrackFrame, Tracks};

/// A point of a target's path: frame, x and y (degrees from the crosshair).
pub type PathPoint = (i64, f64, f64);

/// A kill and the flick to it: its number, the frame it was seen on and the frame the kill times give, where the flick
/// starts, the shots it took, the target's path, whether the target appeared after the kill before it, and its median
/// area in pixels.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Flick {
    pub n: usize,
    pub kill_frame: i64,
    pub stats_frame: Option<i64>,
    pub start_frame: i64,
    pub shots: Option<i64>,
    pub traj: Vec<PathPoint>,
    pub spawned: bool,
    pub area: Option<f64>,
}

/// Where the kill times came from.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KillSource {
    Stats,
    Hud,
    Aimlab,
    Video,
}

/// How the kills matched: kills seen in the video and in the kill times, kills matched, kills confirmed (the target
/// last seen at the crosshair within 2 frames of its kill time), the kill times' offset on the video's clock (s).
#[derive(Clone, Debug, Serialize)]
pub struct MatchInfo {
    pub kills_video: usize,
    pub kills_stats: Option<usize>,
    pub matched: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<usize>,
    pub offset: Option<f64>,
    pub fps: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<KillSource>,
}

/// Python's `int(round(x))`.
fn round_frame(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// Each track's points by frame and its areas, with the tracks in the order they first appear.
struct TrackIndex {
    ids: Vec<u32>,
    points: HashMap<u32, BTreeMap<i64, (f64, f64)>>,
    areas: HashMap<u32, Vec<i64>>,
}

impl TrackIndex {
    fn new(frames: &[TrackFrame]) -> TrackIndex {
        let mut index = TrackIndex { ids: Vec::new(), points: HashMap::new(), areas: HashMap::new() };
        for f in frames {
            let areas: Vec<i64> = if f.a.is_empty() { vec![0; f.t.len()] } else { f.a.clone() };
            for (&(tid, x, y), a) in f.t.iter().zip(areas) {
                if !index.points.contains_key(&tid) {
                    index.ids.push(tid);
                }
                index.points.entry(tid).or_default().insert(f.i as i64, (x, y));
                index.areas.entry(tid).or_default().push(a);
            }
        }
        index
    }

    fn last(&self, tid: u32) -> (i64, (f64, f64)) {
        let (&i, &q) = self.points[&tid].last_key_value().unwrap();
        (i, q)
    }
}

/// Tracks that are one target picked up again: appeared (each track with the frame its target first appeared on, in
/// the order the tracks start) and follows (a track to the track that continues it).
pub struct Appearances {
    pub appeared: Vec<(u32, i64)>,
    pub follows: HashMap<u32, u32>,
}

/// The camera's turn summed from the first frame, by frame.
fn turned(frames: &[TrackFrame]) -> Vec<(f64, f64)> {
    let mut cum = vec![(0.0, 0.0)];
    for f in frames.iter().skip(1) {
        let (x, y) = *cum.last().unwrap();
        cum.push((x + f.shift.0, y + f.shift.1));
    }
    cum
}

/// A track that starts within `gap` seconds of another's end, within `radius` degrees of where that one would be now
/// (its last place moved by the camera's turn since), continues it.
pub fn appearances(tracks: &Tracks, gap: f64, radius: f64) -> Appearances {
    let cum = turned(&tracks.frames);
    let mut first: HashMap<u32, (i64, f64, f64)> = HashMap::new();
    let mut last: HashMap<u32, (i64, f64, f64)> = HashMap::new();
    let mut order = Vec::new();
    for f in &tracks.frames {
        for &(tid, x, y) in &f.t {
            first.entry(tid).or_insert_with(|| {
                order.push(tid);
                (f.i as i64, x, y)
            });
            last.insert(tid, (f.i as i64, x, y));
        }
    }
    let g = round_frame(gap * tracks.fps).max(1);
    let mut ending: HashMap<i64, Vec<u32>> = HashMap::new();
    for &tid in &order {
        ending.entry(last[&tid].0).or_default().push(tid);
    }
    let mut starts = order.clone();
    starts.sort_by_key(|t| first[t].0);
    let mut appeared: HashMap<u32, i64> = HashMap::new();
    let mut out = Appearances { appeared: Vec::new(), follows: HashMap::new() };
    for tid in starts {
        let (s0, qx, qy) = first[&tid];
        let mut best: Option<(f64, u32)> = None;
        // up to 2 frames of overlap: a hit target flashes and is picked up again while its old track still has a
        // frame or two
        for e in (s0 - g).max(0)..s0 + 3 {
            for &prev in ending.get(&e).map(Vec::as_slice).unwrap_or_default() {
                if out.follows.contains_key(&prev) || prev == tid || first[&prev].0 >= s0 {
                    continue;
                }
                let (_, x, y) = last[&prev];
                let (a, b) = (cum[s0 as usize], cum[e as usize]);
                let d = hypot(qx - (x + a.0 - b.0), qy - (y + a.1 - b.1));
                if d < radius && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, prev));
                }
            }
        }
        let at = match best {
            Some((_, prev)) => {
                out.follows.insert(prev, tid);
                appeared[&prev]
            }
            None => s0,
        };
        appeared.insert(tid, at);
        out.appeared.push((tid, at));
    }
    out
}

/// The 2-D histogram's bin of a value: NumPy's `histogram2d` edges (`linspace`), a value on an edge in the bin above.
fn bin(edges: &[f64], v: f64) -> usize {
    edges.partition_point(|&e| e <= v) - 1
}

/// Fixed screen spots where the detector marks the crosshair: detections near the crosshair while the camera turns (no
/// target stays put on screen then) that pile up on one point, at least 2% of the turning frames within 0.015 degrees
/// and most of what lies within 0.1 degrees of it. At most 3.
pub fn crosshair_spots(frames: &[TrackFrame]) -> Vec<(f64, f64)> {
    const NEAR: f64 = 0.5;
    const STEP: f64 = 0.01;
    let turning: Vec<&TrackFrame> = frames.iter().filter(|f| hypot(f.shift.0, f.shift.1) > 0.1).collect();
    let p: Vec<(f64, f64)> = turning
        .iter()
        .flat_map(|f| f.t.iter().map(|&(_, x, y)| (x, y)))
        .filter(|&(x, y)| hypot(x, y) < NEAR)
        .collect();
    let mut out = Vec::new();
    if p.len() < 30 {
        return out;
    }
    let n = round_frame(2.0 * NEAR / STEP) as usize;
    let step = 2.0 * NEAR / n as f64;
    let mut edges: Vec<f64> = (0..=n).map(|k| k as f64 * step + -NEAR).collect();
    edges[n] = NEAR;
    let mut h = vec![0.0; n * n];
    for &(x, y) in &p {
        h[bin(&edges, x) * n + bin(&edges, y)] += 1.0;
    }
    let within = |c: (f64, f64), r: f64| p.iter().filter(move |&&(x, y)| (x - c.0).hypot(y - c.1) < r);
    let centers: Vec<f64> = (0..n).map(|k| -NEAR + (k as f64 + 0.5) * STEP).collect();
    for _ in 0..3 {
        // each bin with its 8 neighbours (wrapping round, as np.roll does), the first largest
        let mut best = (0, 0, f64::NEG_INFINITY);
        for i in 0..n {
            for j in 0..n {
                let mut b = 0.0;
                for a in [n - 1, 0, 1] {
                    for c in [n - 1, 0, 1] {
                        b += h[((i + a) % n) * n + (j + c) % n];
                    }
                }
                if b > best.2 {
                    best = (i, j, b);
                }
            }
        }
        let (i, j, b) = best;
        if b < (0.02 * turning.len() as f64).max(25.0) {
            break;
        }
        let c = (centers[i], centers[j]);
        let near: Vec<&(f64, f64)> = within(c, 0.02).collect();
        if near.is_empty() {
            break;
        }
        let c = (
            near.iter().map(|q| q.0).sum::<f64>() / near.len() as f64,
            near.iter().map(|q| q.1).sum::<f64>() / near.len() as f64,
        );
        if (within(c, 0.02).count() as f64) < 0.6 * within(c, 0.1).count() as f64 {
            break; // spread out: targets held near the crosshair, not a fixed spot
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
    out
}

/// The flicks for known kill times (seconds) with the shots each kill took: from the stats file, or from the session
/// HUD (already on the video's clock: `off` Some(0)).
///
/// 1. The video's clock against the kill times' clock: one constant offset, voted from tracks that end at the
///    crosshair (unless `off` is given).
/// 2. For each kill, the killed target is the track nearest the crosshair in the last `window` seconds before it.
/// 3. The flick runs from the previous kill, or from the target's first appearance when it appeared after that kill,
///    to this kill.
pub fn match_times(
    tracks: &Tracks,
    kt: &[f64],
    shots: &[i64],
    window: f64,
    off: Option<f64>,
) -> (Vec<Flick>, MatchInfo) {
    let fps = tracks.fps;
    let index = TrackIndex::new(&tracks.frames);
    let n_frames = tracks.frames.len() as i64;
    let mut ends: Vec<(i64, u32)> = index
        .ids
        .iter()
        .filter_map(|&tid| {
            let (i, (x, y)) = index.last(tid);
            (i < n_frames - 2 && hypot(x, y) < 0.6 && index.points[&tid].len() >= 3).then_some((i, tid))
        })
        .collect();
    ends.sort();
    let vt: Vec<f64> = ends.iter().map(|e| e.0 as f64 / fps).collect();
    if kt.is_empty() || (off.is_none() && vt.is_empty()) {
        let info = MatchInfo {
            kills_video: ends.len(),
            kills_stats: Some(kt.len()),
            matched: 0,
            confirmed: None,
            offset: None,
            fps,
            source: None,
        };
        return (Vec::new(), info);
    }
    let before: HashMap<u32, u32> =
        appearances(tracks, 0.5, 1.0).follows.into_iter().map(|(k, v)| (v, k)).collect();
    let spots = crosshair_spots(&tracks.frames);
    let on_spot: HashSet<u32> = index
        .ids
        .iter()
        .copied()
        .filter(|tid| {
            !spots.is_empty()
                && index.points[tid].values().all(|&(x, y)| {
                    spots.iter().map(|&(a, b)| hypot(x - a, y - b)).fold(f64::INFINITY, f64::min) < 0.1
                })
        })
        .collect();
    let off = off.unwrap_or_else(|| clock_offset(kt, &vt, fps));
    let kills = Kills { index: &index, before: &before, on_spot: &on_spot, fps, off, window };
    let flicks = kills.attach(kt, shots);
    // confirmed: the target was last seen within 0.6 degrees of the crosshair, within 2 frames of its kill time
    let confirmed = flicks
        .iter()
        .filter(|f| {
            f.traj.last().is_some_and(|&(i, x, y)| (i - f.stats_frame.unwrap_or(i)).abs() <= 2 && hypot(x, y) < 0.6)
        })
        .count();
    let info = MatchInfo {
        kills_video: ends.len(),
        kills_stats: Some(kt.len()),
        matched: flicks.len(),
        confirmed: Some(confirmed),
        offset: Some(off),
        fps,
        source: None,
    };
    (flicks, info)
}

/// The kill times' offset on the video's clock: the one most kills line up with (tracks ending at the crosshair within
/// 2.5 frames), refined by the median of how far each that lines up is off.
fn clock_offset(kt: &[f64], vt: &[f64], fps: f64) -> f64 {
    let nearest = |t: f64| {
        vt.iter().copied().fold((f64::INFINITY, 0.0), |best, v| if (t - v).abs() < best.0 { ((t - v).abs(), v) } else { best })
    };
    let mut best: Option<(usize, f64)> = None;
    for &a in vt.iter().take(40) {
        for &b in kt.iter().take(40) {
            let off = a - b;
            let m = kt.iter().filter(|&&t| nearest(t + off).0 < 2.5 / fps).count();
            if best.is_none_or(|(bm, _)| m > bm) {
                best = Some((m, off));
            }
        }
    }
    let off = best.unwrap().1;
    let res: Vec<f64> =
        kt.iter().map(|&t| nearest(t + off).1 - (t + off)).filter(|r| r.abs() < 2.5 / fps).collect();
    if res.is_empty() { off } else { off + crate::python::numpy_median(&res) }
}

/// What step 2 and 3 of match_times work from.
struct Kills<'a> {
    index: &'a TrackIndex,
    before: &'a HashMap<u32, u32>,
    on_spot: &'a HashSet<u32>,
    fps: f64,
    off: f64,
    window: f64,
}

impl Kills<'_> {
    /// Each kill's target and flick. A target lost for a few frames and picked up again as a new track keeps its
    /// earlier tracks (`before`). A track that never leaves a crosshair spot (`on_spot`) is the killed target only
    /// when no other track is near.
    fn attach(&self, kt: &[f64], shots: &[i64]) -> Vec<Flick> {
        let (index, fps) = (self.index, self.fps);
        let w = round_frame(self.window * fps).max(1);
        let mut flicks = Vec::new();
        let mut prev: Option<i64> = None;
        let mut used: HashSet<u32> = HashSet::new();
        // review.py reads `last` after its loop over the tracks: the last track looked at, not the one chosen
        let mut last = 0;
        for (k, (&t, &n_shots)) in kt.iter().zip(shots).enumerate() {
            let kf = round_frame((t + self.off) * fps);
            let mut best: Option<(u32, f64)> = None;
            for &tid in &index.ids {
                let p = &index.points[&tid];
                let mut seen = p.range(kf - w..kf + 3).map(|(&i, _)| i).peekable();
                if seen.peek().is_none() {
                    continue;
                }
                // its frame nearest the crosshair and the kill: a track can go on past the kill and move off
                let score = |i: i64| hypot(p[&i].0, p[&i].1) + 4.0 * (i - kf).abs() as f64 / fps;
                last = seen.fold(None, |b: Option<(i64, f64)>, i| {
                    let s = score(i);
                    if b.is_none_or(|(_, bs)| s < bs) { Some((i, s)) } else { b }
                })
                .unwrap()
                .0;
                let d = hypot(p[&last].0, p[&last].1);
                if d > 1.5 {
                    continue;
                }
                let cost = d
                    + 4.0 * (last.min(kf) - kf).abs() as f64 / fps
                    + if last >= kf - 2 { 0.0 } else { 0.2 }
                    + if self.on_spot.contains(&tid) { 1.0 } else { 0.0 };
                if best.is_none_or(|(_, bc)| cost < bc) {
                    best = Some((tid, cost));
                }
            }
            let mut best_tid = best.map(|b| b.0);
            if best_tid.is_none() {
                // held hidden under the crosshair longer than the window: the latest track that ended at the
                // crosshair since the previous kill, up to 1 s back, not taken by a kill
                let floor = prev.unwrap_or(-1).max(kf - 4 * w);
                let back = index
                    .ids
                    .iter()
                    .filter(|tid| !used.contains(tid))
                    .map(|&tid| (index.last(tid), tid))
                    .filter(|&((e, (x, y)), _)| floor < e && e < kf - w && hypot(x, y) < 0.6)
                    .map(|((e, _), tid)| (e, tid))
                    .max();
                if let Some((e, tid)) = back {
                    (last, best_tid) = (e, Some(tid));
                }
            }
            let Some(tid) = best_tid else {
                prev = Some(kf);
                continue;
            };
            used.insert(tid);
            let mut chain = vec![tid];
            while let Some(&b) = self.before.get(chain.last().unwrap()) {
                chain.push(b);
            }
            let mut p: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
            for c in &chain {
                p.extend(index.points[c].iter().map(|(&i, &q)| (i, q)));
            }
            let first = *p.first_key_value().unwrap().0;
            let mut start = prev.unwrap_or_else(|| round_frame(self.off * fps));
            let spawned = first > start + 2;
            if spawned {
                start = first;
            }
            let end = *p
                .iter()
                .rev()
                .find(|&(&i, &(x, y))| i <= kf + 2 && (i <= last || hypot(x, y) <= 1.5))
                .unwrap()
                .0;
            let traj: Vec<PathPoint> = p
                .iter()
                .filter(|&(&i, _)| start <= i && i <= end)
                .map(|(&i, &(x, y))| (i, x, y))
                .collect();
            // the whole chain's areas: the last track may be half under the crosshair
            let a: Vec<f64> = chain.iter().flat_map(|c| index.areas[c].iter().map(|&v| v as f64)).collect();
            flicks.push(Flick {
                n: k + 1,
                kill_frame: end,
                stats_frame: Some(kf),
                start_frame: start,
                shots: Some(n_shots),
                traj,
                spawned,
                area: a.iter().any(|&v| v != 0.0).then(|| crate::statistics::median(&a)),
            });
            prev = Some(kf);
        }
        flicks
    }
}
