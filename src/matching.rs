//! Each kill matched to the target it killed and the flick to it (review.py: `match_times`, `_attach_kills`,
//! `appearances`, `crosshair_spots`).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::capped::Capped;
use crate::python::hypot;
use crate::track::{TrackFrame, Tracks, spikes};

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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MatchInfo {
    pub kills_video: usize,
    pub kills_stats: Option<usize>,
    pub matched: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub confirmed: Option<usize>,
    pub offset: Option<f64>,
    pub fps: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
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
    let mut cum = Vec::with_capacity(frames.len().max(1));
    cum.push((0.0, 0.0));
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

/// The most crosshair spots `crosshair_spots` finds.
pub const SPOTS: usize = 3;

/// Fixed screen spots where the detector marks the crosshair: detections near the crosshair while the camera turns (no
/// target stays put on screen then) that pile up on one point, at least 2% of the turning frames within 0.015 degrees
/// and most of what lies within 0.1 degrees of it. At most SPOTS.
pub fn crosshair_spots(frames: &[TrackFrame]) -> Capped<(f64, f64), SPOTS> {
    const NEAR: f64 = 0.5;
    const STEP: f64 = 0.01;
    let turning: Vec<&TrackFrame> = frames.iter().filter(|f| hypot(f.shift.0, f.shift.1) > 0.1).collect();
    let p: Vec<(f64, f64)> = turning
        .iter()
        .flat_map(|f| f.t.iter().map(|&(_, x, y)| (x, y)))
        .filter(|&(x, y)| hypot(x, y) < NEAR)
        .collect();
    let mut out = Capped::new();
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
    for _ in 0..SPOTS {
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
        let mut flicks = Vec::with_capacity(kt.len().min(shots.len()));
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

/// Tracks that are the crosshair, not a target: they never leave `maxd` degrees of the crosshair, the camera turned
/// more than `moved` degrees meanwhile, and they did not move with it (a static target shifts by the camera's turn;
/// more than `share` of the turn left unexplained). A detector can take some crosshairs (Aim Lab's) for a target. Also
/// tracks of 3 frames or fewer on a crosshair spot (`crosshair_spots`): right after a kill they made the dead target
/// look picked up again, so its kill was lost or came late.
pub fn ghosts(frames: &[TrackFrame], maxd: f64, share: f64, moved: f64) -> HashSet<u32> {
    let cum = turned(frames);
    let index = TrackIndex::new(frames);
    let mut out = HashSet::new();
    for &tid in &index.ids {
        let p = &index.points[&tid];
        if p.len() < 2 || p.values().any(|&(x, y)| hypot(x, y) >= maxd) {
            continue;
        }
        let (mut turn, mut left) = (0.0, 0.0);
        for ((&i, &(xi, yi)), (&j, &(xj, yj))) in p.iter().zip(p.iter().skip(1)) {
            let (c, d) = (cum[j as usize], cum[i as usize]);
            let dc = (c.0 - d.0, c.1 - d.1);
            turn += hypot(dc.0, dc.1);
            left += hypot(xj - xi - dc.0, yj - yi - dc.1);
        }
        if turn > moved && left > share * turn {
            out.insert(tid);
        }
    }
    let spots = crosshair_spots(frames);
    if !spots.is_empty() {
        for &tid in &index.ids {
            let p = &index.points[&tid];
            let on_spot = |&(x, y): &(f64, f64)| spots.iter().any(|&(a, b)| hypot(x - a, y - b) < 0.1);
            if p.len() <= 3 && p.values().all(on_spot) {
                out.insert(tid);
            }
        }
    }
    out
}

/// The tracks without those that are the crosshair (`ghosts`).
pub fn without_ghosts(tracks: &Tracks) -> Tracks {
    let gone = ghosts(&tracks.frames, 0.5, 0.5, 0.3);
    let mut out = tracks.clone();
    if gone.is_empty() {
        return out;
    }
    for f in &mut out.frames {
        let keep: Vec<bool> = f.t.iter().map(|q| !gone.contains(&q.0)).collect();
        kept(&mut f.t, &keep);
        kept(&mut f.a, &keep);
        if let Some(wh) = f.wh.as_mut() {
            kept(wh, &keep);
        }
        if let Some(s) = f.s.as_mut() {
            kept(s, &keep);
        }
    }
    out
}

/// A frame's values for its targets, with only those `keep` keeps (a list of another length is left as it is).
fn kept<T>(v: &mut Vec<T>, keep: &[bool]) {
    if v.len() == keep.len() {
        let mut k = keep.iter();
        v.retain(|_| *k.next().unwrap());
    }
}

/// The tracks as the video alone reads them: each track's points and blob areas by frame, and the tracks in the order
/// they first appear (tracks split off by `repair` come last).
struct Paths {
    ids: Vec<u32>,
    points: HashMap<u32, BTreeMap<i64, (f64, f64)>>,
    areas: HashMap<u32, BTreeMap<i64, i64>>,
}

impl Paths {
    fn new(frames: &[TrackFrame]) -> Paths {
        let mut paths = Paths { ids: Vec::new(), points: HashMap::new(), areas: HashMap::new() };
        for f in frames {
            let areas: Vec<i64> = if f.a.is_empty() { vec![0; f.t.len()] } else { f.a.clone() };
            for (&(tid, x, y), a) in f.t.iter().zip(areas) {
                if !paths.points.contains_key(&tid) {
                    paths.ids.push(tid);
                }
                paths.points.entry(tid).or_default().insert(f.i as i64, (x, y));
                paths.areas.entry(tid).or_default().insert(f.i as i64, a);
            }
        }
        paths
    }

    fn first(&self, tid: u32) -> i64 {
        *self.points[&tid].first_key_value().unwrap().0
    }

    fn last(&self, tid: u32) -> i64 {
        *self.points[&tid].last_key_value().unwrap().0
    }

    /// The track's blob areas near the crosshair (within 3 degrees), where the detector sees the target whole.
    fn near_areas(&self, tid: u32) -> Vec<f64> {
        let p = &self.points[&tid];
        self.areas[&tid]
            .iter()
            .filter(|&(i, &a)| a != 0 && hypot(p[i].0, p[i].1) < 3.0)
            .map(|(_, &a)| a as f64)
            .collect()
    }

    /// A kill can fool the camera's turn on a plain wall: with the target at the crosshair gone, the frame's shift
    /// lines up another target with the dead one's place, and the tracker hands the dead target's track on to that
    /// target (its place on screen jumps with the shift) and starts new tracks for the targets that stayed put. A
    /// track at the crosshair whose place jumps more than `near` with the frame's shift is split there when the shift
    /// is a spike (`track::spikes`: since review version 3 the tracker repairs most, `link`), or when the jump is more
    /// than twice `near` and lands within `near` of a track that ended the frame before. Its head ends where the
    /// target died; its rest continues that track, else becomes a track of its own. Returns the camera's turn summed
    /// from the first frame, by frame, with each spike replaced by the mean of the frames either side.
    fn repair(&mut self, frames: &[TrackFrame], near: f64) -> Vec<(f64, f64)> {
        let cum = turned(frames);
        let n = frames.len();
        let spike = spikes(&frames.iter().map(|f| f.shift).collect::<Vec<_>>());
        // the tracks seen on each frame, and those that end on it
        let mut at: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut ends: HashMap<i64, HashSet<u32>> = HashMap::new();
        for &tid in &self.ids {
            for &i in self.points[&tid].keys() {
                at[i as usize].push(tid);
            }
            ends.entry(self.last(tid)).or_default().insert(tid);
        }
        let mut next = self.ids.iter().max().map_or(0, |m| m + 1);
        for j in 1..n {
            let (i, fi, fj) = (j - 1, j as i64 - 1, j as i64);
            let s = (cum[j].0 - cum[i].0, cum[j].1 - cum[i].1);
            let mut both: Vec<u32> = at[i].iter().copied().filter(|t| at[j].contains(t)).collect();
            both.sort_unstable();
            for t in both {
                let p = &self.points[&t];
                let (Some(&(x0, y0)), Some(&(x1, y1))) = (p.get(&fi), p.get(&fj)) else { continue };
                let jump = hypot(x1 - x0, y1 - y0);
                if hypot(x0, y0) >= near || jump <= near || hypot(x1 - x0 - s.0, y1 - y0 - s.1) >= near {
                    continue;
                }
                let mut ended: Vec<(f64, u32)> = ends
                    .get(&fi)
                    .into_iter()
                    .flatten()
                    .filter(|&&b| b != t && self.last(b) == fi)
                    .map(|&b| {
                        let (bx, by) = self.points[&b][&fi];
                        (hypot(x1 - bx, y1 - by), b)
                    })
                    .filter(|&(d, _)| d < near)
                    .collect();
                ended.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                if !spike[j] && (ended.is_empty() || jump <= 2.0 * near) {
                    continue;
                }
                let rest = self.points.get_mut(&t).unwrap().split_off(&fj);
                let rest_areas = self.areas.get_mut(&t).unwrap().split_off(&fj);
                let old_end = *rest.last_key_value().unwrap().0;
                let b = match ended.first() {
                    Some(&(_, b)) => {
                        ends.get_mut(&fi).unwrap().remove(&b);
                        b
                    }
                    None => {
                        next += 1;
                        self.ids.push(next - 1);
                        next - 1
                    }
                };
                for &k in rest.keys() {
                    let here = &mut at[k as usize];
                    here.retain(|&u| u != t);
                    here.push(b);
                }
                self.points.entry(b).or_default().extend(rest);
                self.areas.entry(b).or_default().extend(rest_areas);
                ends.entry(self.last(b)).or_default().insert(b);
                ends.get_mut(&old_end).unwrap().remove(&t);
                ends.entry(fi).or_default().insert(t);
            }
        }
        if !spike.contains(&true) {
            return cum;
        }
        let mut step: Vec<(f64, f64)> = (0..n)
            .map(|k| if k == 0 { (0.0, 0.0) } else { (cum[k].0 - cum[k - 1].0, cum[k].1 - cum[k - 1].1) })
            .collect();
        for j in (0..n).filter(|&j| spike[j]) {
            step[j] = ((step[j - 1].0 + step[j + 1].0) / 2.0, (step[j - 1].1 + step[j + 1].1) / 2.0);
        }
        let mut sum = step[0];
        let mut out = Vec::with_capacity(n);
        out.push(sum);
        for v in &step[1..] {
            sum = (sum.0 + v.0, sum.1 + v.1);
            out.push(sum);
        }
        out
    }

    /// Tracks that are one target picked up again (`appearances`, for the video alone): a track that starts within
    /// 0.5 s of another's end (or up to 2 frames before it), within 1 degree of where that one would be now (its last
    /// place moved by the camera's turn since, and by its own speed over its last 3 frames or not: either will do),
    /// continues it, unless the target was gone meanwhile. A target hidden under the crosshair stays there: one whose
    /// place came out from under it (farther than 1.5 x `near`) on 2 or more of the frames it was missing would have
    /// been seen, so it died. And a target that comes back is where it was and as big: one missing for 3 frames or
    /// more that comes back more than half its radius `r` from that place (2 frames: 1.5 radii), or less than half or
    /// more than twice as big (`same_size`), is a new target, such as one that spawned near the dead one. Returns the
    /// track that continues each track.
    fn continued(&self, cum: &[(f64, f64)], fps: f64, r: f64, near: f64) -> HashMap<u32, u32> {
        let g = round_frame(0.5 * fps).max(1);
        let mut ending: HashMap<i64, Vec<u32>> = HashMap::new();
        for &tid in &self.ids {
            ending.entry(self.last(tid)).or_default().push(tid);
        }
        let mut starts = self.ids.clone();
        starts.sort_by_key(|&t| self.first(t));
        let mut follows: HashMap<u32, u32> = HashMap::new();
        let mut before: HashMap<u32, u32> = HashMap::new();
        for tid in starts {
            let s0 = self.first(tid);
            let (qx, qy) = self.points[&tid][&s0];
            let mut best: Option<(f64, u32)> = None;
            for e in (s0 - g).max(0)..s0 + 3 {
                for &prev in ending.get(&e).map(Vec::as_slice).unwrap_or_default() {
                    if follows.contains_key(&prev) || prev == tid || self.first(prev) >= s0 {
                        continue;
                    }
                    let (x, y) = self.points[&prev][&e];
                    let (a, b) = (cum[s0 as usize], cum[e as usize]);
                    let n = (s0 - e) as f64;
                    // where it would be if it moved on as it did, and if it stood still: either will do
                    for v in [self.speed(prev, &before, cum, e), (0.0, 0.0)] {
                        let d = hypot(qx - (x + a.0 - b.0 + v.0 * n), qy - (y + a.1 - b.1 + v.1 * n));
                        if d >= 1.0 || best.is_some_and(|(bd, _)| d >= bd) {
                            continue;
                        }
                        let seen = (e + 1..s0)
                            .filter(|&i| {
                                let (c, k) = (cum[i as usize], (i - e) as f64);
                                hypot(x + c.0 - b.0 + v.0 * k, y + c.1 - b.1 + v.1 * k) > 1.5 * near
                            })
                            .count();
                        let back = match s0 - e {
                            3.. => d <= 0.5 * r && self.same_size(prev, tid),
                            2 => d <= 1.5 * r,
                            _ => true,
                        };
                        if seen < 2 && back {
                            best = Some((d, prev));
                        }
                    }
                }
            }
            if let Some((_, prev)) = best {
                follows.insert(prev, tid);
                before.insert(tid, prev);
            }
        }
        follows
    }

    /// Whether a target came back about as big as it was: the new track's median blob area over its first 3 frames is
    /// within half and twice the old track's over its last 3 (true when either has no area).
    fn same_size(&self, old: u32, new: u32) -> bool {
        let a: Capped<f64, 3> = self.areas[&old].values().rev().take(3).map(|&v| v as f64).collect();
        let b: Capped<f64, 3> = self.areas[&new].values().take(3).map(|&v| v as f64).collect();
        let (a, b) = (crate::statistics::median(&a), crate::statistics::median(&b));
        a <= 0.0 || b <= 0.0 || (0.5..=2.0).contains(&(b / a))
    }

    /// A target's speed over the world (degrees a frame) where its track ends, at frame `e`: from its points over its
    /// last 3 frames, with those of the tracks it continues (`before`) when it has 3 points or fewer.
    fn speed(&self, tid: u32, before: &HashMap<u32, u32>, cum: &[(f64, f64)], e: i64) -> (f64, f64) {
        const FRAMES: i64 = 3;
        let own = &self.points[&tid];
        let joined: BTreeMap<i64, (f64, f64)>;
        let mut pts = own;
        if own.len() <= FRAMES as usize && before.contains_key(&tid) {
            let mut all = own.clone();
            let mut c = tid;
            while let Some(&b) = before.get(&c) {
                if all.len() > FRAMES as usize {
                    break;
                }
                c = b;
                for (&k, &q) in &self.points[&c] {
                    all.entry(k).or_insert(q);
                }
            }
            joined = all;
            pts = &joined;
        }
        let mut window = pts.range(e - FRAMES..=e);
        let (Some((&i0, &(x0, y0))), Some(_)) = (window.next(), window.next()) else { return (0.0, 0.0) };
        let (x1, y1) = pts[&e];
        let k = (e - i0) as f64;
        let (c0, c1) = (cum[i0 as usize], cum[e as usize]);
        (((x1 - c1.0) - (x0 - c0.0)) / k, ((y1 - c1.1) - (y0 - c0.1)) / k)
    }
}

/// The flicks from the video alone, for a run without a stats file or a readable HUD. A kill is a track that ends near
/// the crosshair, unless:
/// - another track continues it (`Paths::continued`: tracking lost the target; it did not die);
/// - it is the crosshair (`ghosts`);
/// - three or more steady tracks (5 frames or more, not continued) end within a frame of it (the run ended or
///   restarted); flickering false detections, such as a game's HUD text, do not count;
/// - its blob is less than a tenth of the run's typical target (a hit marker or a spark, not a target);
/// - another kill was found within 3 frames (the same kill twice).
///
/// Tracks the camera's turn fooled at a kill are split first (`Paths::repair`). A flick's path joins the pieces of
/// its target's track. "Near" is the target's radius (from the tracks' median blob area) plus 0.25 degrees, and at
/// least 0.6. Flicks are built as `match_times` builds them, with no shot counts.
pub fn match_video(tracks: &Tracks) -> (Vec<Flick>, MatchInfo) {
    let tracks = without_ghosts(tracks);
    let (fps, frames) = (tracks.fps, &tracks.frames);
    let mut paths = Paths::new(frames);
    let areas: Vec<f64> = paths
        .ids
        .iter()
        .map(|&tid| paths.near_areas(tid))
        .filter(|v| !v.is_empty())
        .map(|v| crate::statistics::median(&v))
        .collect();
    // the run's typical target: its median blob area (None with fewer than 5 tracks) and radius
    let typical = (areas.len() >= 5).then(|| crate::statistics::median(&areas));
    let r = typical.map_or(0.43, |a| crate::geometry::degrees((a / std::f64::consts::PI).sqrt() / crate::geometry::K));
    let near = (r + 0.25).max(0.6);
    let cum = paths.repair(frames, near);
    let follows = paths.continued(&cum, fps, r, near);
    let before: HashMap<u32, u32> = follows.iter().map(|(&k, &v)| (v, k)).collect();
    let mut ends_at: HashMap<i64, usize> = HashMap::new();
    for &tid in &paths.ids {
        if paths.points[&tid].len() >= 5 && !follows.contains_key(&tid) {
            *ends_at.entry(paths.last(tid)).or_default() += 1;
        }
    }
    let ending = |e: i64| (e - 1..=e + 1).map(|i| ends_at.get(&i).copied().unwrap_or(0)).sum::<usize>();
    let n = frames.len() as i64;
    // candidates: (end frame, distance from the crosshair, track, the track and the tracks it continues)
    let mut cands: Vec<(i64, f64, u32, Vec<u32>)> = Vec::new();
    for &tid in &paths.ids {
        let e = paths.last(tid);
        let (x, y) = paths.points[&tid][&e];
        if follows.contains_key(&tid) || e >= n - 2 || hypot(x, y) >= near || ending(e) >= 3 {
            continue;
        }
        let mut chain = vec![tid];
        while let Some(&b) = before.get(chain.last().unwrap()) {
            chain.push(b);
        }
        if chain.iter().map(|c| paths.points[c].len()).sum::<usize>() < 3 {
            continue;
        }
        // a blob less than a tenth of the typical target is no target (a hit marker, a spark)
        let size: Vec<f64> = chain.iter().flat_map(|&c| paths.near_areas(c)).collect();
        if typical.is_some_and(|a| !size.is_empty() && crate::statistics::median(&size) < 0.1 * a) {
            continue;
        }
        cands.push((e, hypot(x, y), tid, chain));
    }
    cands.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut kills: Vec<(i64, f64, u32, Vec<u32>)> = Vec::new();
    for c in cands {
        if let Some(k) = kills.last_mut()
            && c.0 - k.0 <= 3
        {
            if c.1 < k.1 {
                *k = c;
            }
            continue;
        }
        kills.push(c);
    }
    let mut flicks = Vec::with_capacity(kills.len());
    let mut prev: Option<i64> = None;
    for (k, (end, _, tid, chain)) in kills.iter().enumerate() {
        let mut p: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
        for c in chain {
            p.extend(paths.points[c].iter().map(|(&i, &q)| (i, q)));
        }
        let first = *p.first_key_value().unwrap().0;
        let mut start = prev.unwrap_or(first);
        let spawned = first > start + 2;
        if spawned {
            start = first;
        }
        flicks.push(Flick {
            n: k + 1,
            kill_frame: *end,
            stats_frame: None,
            start_frame: start,
            shots: None,
            traj: p.range(start..=*end).map(|(&i, &(x, y))| (i, x, y)).collect(),
            spawned,
            area: Some(paths.near_areas(*tid)).filter(|v| !v.is_empty()).map(|v| crate::statistics::median(&v)),
        });
        prev = Some(*end);
    }
    let info = MatchInfo {
        kills_video: kills.len(),
        kills_stats: None,
        matched: flicks.len(),
        confirmed: None,
        offset: None,
        fps,
        source: Some(KillSource::Video),
    };
    (flicks, info)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame's camera turn and targets.
    type Frame = ((f64, f64), Vec<crate::track::TrackPoint>);

    /// Tracks at 60 frames a second from each frame's camera turn and targets (id, x, y), every target 40 pixels big.
    fn tracks(frames: Vec<Frame>) -> Tracks {
        let frames = frames
            .into_iter()
            .enumerate()
            .map(|(i, (shift, t))| TrackFrame { i, shift, a: vec![40; t.len()], t, wh: None, s: None })
            .collect();
        Tracks { fps: 60.0, frames, version: 0 }
    }

    fn kill_frames(t: &Tracks) -> Vec<i64> {
        match_video(t).0.iter().map(|f| f.kill_frame).collect()
    }

    #[test]
    fn a_false_turn_at_a_kill_keeps_the_kill() {
        // the target at the crosshair dies on frame 21, and the tracker takes the other target for it, turned there by
        // a one-frame spike; the camera then turns to that target, which dies at the crosshair on frame 31
        let mut f = Vec::new();
        for _ in 0..=20 {
            f.push(((0.0, 0.0), vec![(1, 0.05, 0.0), (2, -3.0, -0.5)]));
        }
        f.push(((-3.05, -0.5), vec![(1, -3.0, -0.5)]));
        for k in 1..=10 {
            f.push(((0.3, 0.05), vec![(1, -3.0 + 0.3 * k as f64, -0.5 + 0.05 * k as f64)]));
        }
        for _ in 0..10 {
            f.push(((0.0, 0.0), vec![]));
        }
        assert_eq!(kill_frames(&tracks(f)), vec![20, 31]);
    }

    #[test]
    fn a_target_back_from_under_the_crosshair_is_no_kill_but_one_spawned_beside_it_is() {
        // hidden under the crosshair for 3 frames and found again where it was: one target, killed on frame 40
        let mut f = Vec::new();
        for i in 0..=45 {
            let t = if (21..24).contains(&i) { vec![] } else { vec![(if i < 21 { 1 } else { 2 }, 0.05, 0.0)] };
            f.push(((0.0, 0.0), if i <= 40 { t } else { vec![] }));
        }
        assert_eq!(kill_frames(&tracks(f)), vec![40]);
        // killed on frame 20, and the next target spawns 0.4 degrees away on frame 25; the camera turns to it, and it
        // dies at the crosshair on frame 45
        let mut f = Vec::new();
        for i in 0..=50 {
            let shift = if (31..=38).contains(&i) { (-0.05, 0.0) } else { (0.0, 0.0) };
            let x = 0.5 - 0.05 * (i.clamp(30, 38) - 30) as f64;
            let t = match i {
                ..=20 => vec![(1, 0.1, 0.0)],
                25..=45 => vec![(2, x, 0.0)],
                _ => vec![],
            };
            f.push((shift, t));
        }
        assert_eq!(kill_frames(&tracks(f)), vec![20, 45]);
    }
}
