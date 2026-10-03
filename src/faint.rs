//! The faint-target cut-off (python/review.py: `faint_scores`, `without_faint`; python/model/hand_crops.py:
//! `cutoff_crops`): each track's score, the recording's level, the tracks the user's cut-off leaves out, and the
//! detector labels a submitted cut-off gives.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W, overlay_shares, to_px};
use crate::py_random::{PyRandom, hex, md5};
use crate::python::{hypot, round};
use crate::track::{TrackFrame, TrackPoint};

/// The offset a cut-off takes when none is given (python/server.py: `faint`).
pub const DEFAULT_OFFSET: f64 = 0.3;

fn default_offset() -> f64 {
    DEFAULT_OFFSET
}

/// The user's faint-target cut-off for a recording (faint.json): whether it is on, and how far below the recording's
/// level a track may score before the cut leaves it out.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct FaintSetting {
    pub on: bool,
    #[serde(default = "default_offset")]
    pub offset: f64,
}

/// A track's score: the 90th percentile of the detector's scores for it away from the crosshair, and how many frames
/// gave one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TrackScore {
    pub id: u32,
    pub score: f64,
    pub frames: usize,
}

/// Every track's score, in the order the tracks first scored, and the recording's level: the 90th percentile of the
/// scores, weighted by frames (None without scores).
#[derive(Clone, Debug, Default, Serialize)]
pub struct FaintScores {
    pub scores: Vec<TrackScore>,
    pub level: Option<f64>,
}

/// Each track's score from the frames where it lies `near` degrees or more from the crosshair (a target under the
/// crosshair scores low), for tracks with 3 or more such frames, and the recording's level.
pub fn faint_scores(frames: &[TrackFrame], near: f64) -> FaintScores {
    let mut order: Vec<u32> = Vec::new();
    let mut seen: HashMap<u32, Vec<f64>> = HashMap::new();
    for f in frames {
        let Some(s) = &f.s else { continue };
        for (&(id, x, y), &v) in f.t.iter().zip(s) {
            if hypot(x, y) >= near {
                seen.entry(id)
                    .or_insert_with(|| {
                        order.push(id);
                        Vec::new()
                    })
                    .push(v);
            }
        }
    }
    let mut scores = Vec::new();
    for id in order {
        let mut v = seen.remove(&id).unwrap_or_default();
        if v.len() >= 3 {
            v.sort_by(f64::total_cmp);
            let k = ((0.9 * (v.len() - 1) as f64 + 0.5) as usize).min(v.len() - 1);
            scores.push(TrackScore { id, score: v[k], frames: v.len() });
        }
    }
    let total = scores.iter().map(|t| t.frames).sum::<usize>() as f64;
    let mut by_score = scores.clone();
    by_score.sort_by(|a, b| a.score.total_cmp(&b.score));
    let mut acc = 0;
    let level = by_score.iter().find_map(|t| {
        acc += t.frames;
        (acc as f64 >= 0.9 * total).then_some(t.score)
    });
    FaintScores { scores, level }
}

/// What a cut-off left: the frames without the tracks it cuts, the score it cut at (rounded to 3 decimals; None
/// without scores, when nothing is cut) and how many tracks went.
pub struct FaintCutFrames {
    pub frames: Vec<TrackFrame>,
    pub cut: Option<f64>,
    pub gone: usize,
}

/// The frames without the tracks scoring under the recording's level less `offset`. A tracking run uses near 0: its
/// bot is under the crosshair most of the time, so the frames near the crosshair must count.
pub fn without_faint(frames: &[TrackFrame], offset: f64, near: f64) -> FaintCutFrames {
    let sc = faint_scores(frames, near);
    let Some(level) = sc.level else {
        return FaintCutFrames { frames: frames.to_vec(), cut: None, gone: 0 };
    };
    let cut = level - offset;
    let gone: HashSet<u32> = sc.scores.iter().filter(|t| t.score < cut).map(|t| t.id).collect();
    let frames = frames
        .iter()
        .map(|f| {
            let keep: Vec<usize> = (0..f.t.len()).filter(|&k| !gone.contains(&f.t[k].0)).collect();
            TrackFrame {
                i: f.i,
                shift: f.shift,
                t: picked(&f.t, &keep),
                a: picked(&f.a, &keep),
                wh: f.wh.as_ref().map(|v| picked(v, &keep)),
                s: f.s.as_ref().map(|v| picked(v, &keep)),
            }
        })
        .collect();
    FaintCutFrames { frames, cut: Some(round(cut, 3)), gone: gone.len() }
}

/// The values at the indexes kept.
fn picked<T: Copy>(v: &[T], keep: &[usize]) -> Vec<T> {
    keep.iter().filter_map(|&k| v.get(k).copied()).collect()
}

/// A crop's side, in pixels at 1280 x 720.
pub const CROP: usize = 256;
/// The most crops round left-out tracks, and round kept ones.
const PER: usize = 20;

/// What the labels of a submitted cut-off are made from: the recording's tracks and name, the run's first and last
/// frames (a clicking run: its first flick's start and its last kill; a tracking run: the run's own), the excluded
/// areas (shares of the frame; None: KovOBS's layout), the cut-off's offset, and how near the crosshair a score is
/// not counted (a tracking run 0, a clicking run 2 degrees).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutoffRequest {
    pub frames: Vec<TrackFrame>,
    pub video: String,
    pub start: Option<i64>,
    pub end: Option<i64>,
    #[serde(default)]
    pub exclude: Option<Vec<[f64; 4]>>,
    pub offset: f64,
    pub near: f64,
}

/// A label's row, as label_check.py writes its checks (checked.jsonl): the crop's file, the boxes it keeps (crop
/// pixels: center, size, 2 decimals), every box the model gave there, and where the label came from.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CutoffRow {
    pub file: String,
    pub boxes: Vec<[f64; 4]>,
    pub verdict: &'static str,
    pub model: Vec<[f64; 4]>,
    pub source: &'static str,
    pub video: String,
    pub offset: f64,
    pub cut: f64,
}

/// A crop to write: the frame, the crop's corner (pixels at 1280 x 720), the boxes it keeps as the file holds them
/// (float32 there), and its row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CutoffCrop {
    pub frame: usize,
    pub x0: usize,
    pub y0: usize,
    pub boxes: Vec<[f64; 4]>,
    pub row: CutoffRow,
}

/// The crops' file names start with the recording's name (40 characters) and 6 hex digits of its MD5.
pub fn crop_stem(video: &str) -> String {
    let name = video.rsplit(['/', '\\']).next().unwrap_or(video);
    let stem = match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    };
    let short: String = stem.chars().take(40).collect();
    format!("{short}_{}", &hex(&md5(name.as_bytes()))[..6]).replace(' ', "_")
}

/// The crops a submitted cut-off gives (hand_crops.py: `cutoff_crops`, which also reads each crop's pixels and the
/// fixed map: the page does that). The tracks the cut leaves out are not targets, the ones it keeps are. Only frames
/// inside the run, crops clear of every excluded area, and none holding a track too short to have a score (away from
/// the crosshair). Up to 20 crops round a left-out track and 20 round a kept one, from frames spread over the run, at
/// places Python's random numbers pick (seeded with the file names' stem), so they are the crops Python takes.
pub fn cutoff_crops(r: &CutoffRequest) -> Vec<CutoffCrop> {
    let sc = faint_scores(&r.frames, r.near);
    let (Some(level), Some(start), Some(end)) = (sc.level, r.start, r.end) else {
        return Vec::new();
    };
    let q: HashMap<u32, f64> = sc.scores.iter().map(|t| (t.id, t.score)).collect();
    let cut = level - r.offset;
    let (w, h) = (W as f64, H as f64);
    let ex: Vec<[f64; 4]> = r
        .exclude
        .clone()
        .unwrap_or_else(overlay_shares)
        .iter()
        .map(|b| [b[0] * w, b[1] * h, b[2] * w, b[3] * h])
        .collect();
    let stem = crop_stem(&r.video);
    let mut rnd = PyRandom::seeded(&stem);
    let box_px = |x: f64, y: f64, bw: f64, bh: f64| {
        let (cx, cy) = to_px(x, y);
        let (x0, y0) = to_px(x - bw / 2.0, y + bh / 2.0);
        let (x1, y1) = to_px(x + bw / 2.0, y - bh / 2.0);
        [cx, cy, (x1 - x0).abs(), (y1 - y0).abs()]
    };
    let mut out = Vec::new();
    for left_out in [true, false] {
        let focus = |id: u32, x: f64, y: f64| q.get(&id).is_some_and(|&v| (v < cut) == left_out) && hypot(x, y) >= r.near;
        let first = start.max(0) as usize;
        let last = (r.frames.len() as i64).min(end + 1).max(0) as usize;
        let cand: Vec<usize> = (first..last)
            .filter(|&i| r.frames[i].wh.is_some() && r.frames[i].t.iter().any(|&(id, x, y)| focus(id, x, y)))
            .collect();
        for k in 0..PER.min(cand.len()) {
            let i = cand[((k * cand.len()) as f64 / PER as f64) as usize];
            let f = &r.frames[i];
            let Some(wh) = &f.wh else { continue };
            let pts: Vec<[f64; 4]> = f.t.iter().zip(wh).map(|(&(_, x, y), &(bw, bh))| box_px(x, y, bw, bh)).collect();
            let near_ones: Vec<&[f64; 4]> =
                pts.iter().zip(&f.t).filter(|&(_, &(id, x, y))| focus(id, x, y)).map(|(p, _)| p).collect();
            let c = near_ones[rnd.choice(near_ones.len())];
            let corner = |at: f64, size: usize, jitter: i64| {
                (at - (CROP / 2) as f64 + jitter as f64).clamp(0.0, (size - CROP) as f64) as usize
            };
            let x0 = corner(c[0], W, rnd.randint(-48, 48));
            let y0 = corner(c[1], H, rnd.randint(-48, 48));
            let (fx0, fy0, side) = (x0 as f64, y0 as f64, CROP as f64);
            if ex.iter().any(|&[a, c_, b, d]| a < fx0 + side && fx0 < b && c_ < fy0 + side && fy0 < d) {
                continue; // touches an excluded area
            }
            let inside: Vec<(&[f64; 4], &TrackPoint)> = pts
                .iter()
                .zip(&f.t)
                .filter(|(p, _)| fx0 <= p[0] && p[0] < fx0 + side && fy0 <= p[1] && p[1] < fy0 + side)
                .collect();
            if inside.iter().any(|&(_, &(id, x, y))| !q.contains_key(&id) && hypot(x, y) >= r.near) {
                continue; // a track too short to judge
            }
            let shifted = |p: &[f64; 4]| [p[0] - fx0, p[1] - fy0, p[2], p[3]];
            let keep: Vec<[f64; 4]> = inside
                .iter()
                .filter(|&&(_, &(id, _, _))| q.get(&id).is_none_or(|&v| v >= cut))
                .map(|(p, _)| shifted(p))
                .collect();
            let every: Vec<[f64; 4]> = inside.iter().map(|(p, _)| shifted(p)).collect();
            let rounded = |v: &[[f64; 4]]| v.iter().map(|b| b.map(|x| round(x, 2))).collect();
            out.push(CutoffCrop {
                frame: i,
                x0,
                y0,
                row: CutoffRow {
                    file: format!("train/{stem}_{i:06}.npz"),
                    boxes: rounded(&keep),
                    verdict: "correct",
                    model: rounded(&every),
                    source: "cutoff",
                    video: r.video.clone(),
                    offset: r.offset,
                    cut: round(cut, 3),
                },
                boxes: keep,
            });
        }
    }
    out
}

/// A cut-off's crops as JSON (an array of `CutoffCrop`), or {"error": ...} for a request that cannot be read.
pub fn cutoff_json(request: &[u8]) -> Vec<u8> {
    match serde_json::from_slice::<CutoffRequest>(request) {
        Ok(r) => serde_json::to_vec(&cutoff_crops(&r)),
        Err(e) => serde_json::to_vec(&serde_json::json!({ "error": format!("The labels' request could not be read: {e}") })),
    }
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(i: usize, t: Vec<(u32, f64, f64)>, s: Vec<f64>) -> TrackFrame {
        let n = t.len();
        TrackFrame { i, shift: (0.0, 0.0), t, a: vec![10; n], wh: Some(vec![(1.0, 1.0); n]), s: Some(s) }
    }

    /// Two tracks: one scoring 0.9 throughout, one 0.4. The level is the strong one's score; an offset of 0.3 cuts the
    /// weak one, and with near 2 its frames under the crosshair do not count.
    #[test]
    fn a_weak_track_is_cut() {
        let frames: Vec<TrackFrame> =
            (0..10).map(|i| frame(i, vec![(1, 5.0, 0.0), (2, 3.0, 1.0)], vec![0.9, 0.4])).collect();
        let sc = faint_scores(&frames, 2.0);
        assert_eq!(sc.level, Some(0.9));
        assert_eq!(sc.scores, vec![TrackScore { id: 1, score: 0.9, frames: 10 }, TrackScore { id: 2, score: 0.4, frames: 10 }]);
        let cut = without_faint(&frames, 0.3, 2.0);
        assert_eq!((cut.cut, cut.gone), (Some(0.6), 1));
        assert!(cut.frames.iter().all(|f| f.t.len() == 1 && f.t[0].0 == 1 && f.s.as_ref().unwrap().len() == 1));
        let none = without_faint(&frames, 0.6, 2.0);
        assert_eq!((none.cut, none.gone), (Some(0.3), 0));
    }

    #[test]
    fn tracks_without_scores_are_not_cut() {
        let mut f = frame(0, vec![(1, 5.0, 0.0)], vec![0.9]);
        f.s = None;
        let cut = without_faint(&[f.clone(), f.clone(), f], 0.3, 0.0);
        assert_eq!((cut.cut, cut.gone, cut.frames.len()), (None, 0, 3));
    }

    /// Python: `f"{Path(v).stem[:40]}_{hashlib.md5(Path(v).name.encode()).hexdigest()[:6]}".replace(" ", "_")`.
    #[test]
    fn the_stem_is_pythons() {
        let v = r"E:\OBS\KovOBS\1wall 6targets extra small\1wall 6targets extra small - 889.26 - 2026.10.01-16.17.48.mp4";
        assert_eq!(crop_stem(v), PYTHON_STEM);
    }

    const PYTHON_STEM: &str = "1wall_6targets_extra_small_-_889.26_-_20_6a3425";
}
