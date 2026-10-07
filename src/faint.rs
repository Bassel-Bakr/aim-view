//! The faint-target cut-off (python/retired/review.py: `faint_scores`, `without_faint`; python/model/hand_crops.py:
//! `cutoff_crops`): each track's score, the recording's level, the tracks the user's cut-off leaves out, and the
//! detector labels a submitted cut-off gives. The scores come from the tracks (tracks.json: each target's detector
//! score per frame) and the setting from faint.json. src/review.rs measures a tracking run without the tracks the cut
//! leaves out; a submitted cut-off's crops go to the service (service/src/faint.rs), or in the browser to the page
//! (`cutoff_json`), which write the crops and their labels for training the detector.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::geometry::{H, W, overlay_shares, to_px};
use crate::py_random::{PyRandom, hex, md5};
use crate::python::{hypot, round};
use crate::track::{TrackFrame, TrackPoint};

/// The offset a cut-off takes when none is given (python/server.py: `faint`).
pub const DEFAULT_OFFSET: f64 = 0.3;
/// A track needs this many frames with a score away from the crosshair to have a score of its own.
const MIN_SCORED_FRAMES: usize = 3;
/// A track's score is the one this share of the way up its sorted scores (the nearest), its 90th percentile.
const TRACK_PERCENTILE: f64 = 0.9;
/// The recording's level is the score of the track that brings the frames counted, from the lowest score up, to this
/// share of all scored frames: the 90th percentile of the scores, weighted by frames.
const LEVEL_SHARE: f64 = 0.9;
/// The decimals the cut is rounded to, and a label's boxes.
const CUT_DECIMALS: usize = 3;
const BOX_DECIMALS: usize = 2;
/// A crop's side, in pixels at 1280 x 720.
pub const CROP: usize = 256;
/// The most crops round left-out tracks, and round kept ones.
const CROPS_PER_SIDE: usize = 20;
/// A crop's corner moves from its target's center by up to this many pixels each way (Python's `randint(-48, 48)`).
const CROP_JITTER_PX: i64 = 48;
/// A crop's file name starts with this many characters of the recording's name, and this many hex digits of its MD5.
const STEM_CHARACTERS: usize = 40;
const STEM_HASH_DIGITS: usize = 6;

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

/// Each track's detector scores from the frames where it lies `near` degrees or more from the crosshair, the tracks in
/// the order they first scored.
fn scores_by_track(frames: &[TrackFrame], near: f64) -> Vec<(u32, Vec<f64>)> {
    let mut tracks: Vec<(u32, Vec<f64>)> = Vec::new();
    let mut index_of: HashMap<u32, usize> = HashMap::new();
    for frame in frames {
        let Some(frame_scores) = &frame.s else { continue };
        for (&(id, x, y), &score) in frame.t.iter().zip(frame_scores) {
            if hypot(x, y) >= near {
                let index = *index_of.entry(id).or_insert_with(|| {
                    tracks.push((id, Vec::new()));
                    tracks.len() - 1
                });
                tracks[index].1.push(score);
            }
        }
    }
    tracks
}

/// The recording's level (`LEVEL_SHARE`), None without scores.
fn recording_level(scores: &[TrackScore]) -> Option<f64> {
    let total = scores.iter().map(|track| track.frames).sum::<usize>() as f64;
    let mut by_score = scores.to_vec();
    by_score.sort_by(|a, b| a.score.total_cmp(&b.score));
    let mut counted = 0;
    by_score.iter().find_map(|track| {
        counted += track.frames;
        (counted as f64 >= LEVEL_SHARE * total).then_some(track.score)
    })
}

/// Each track's score from the frames where it lies `near` degrees or more from the crosshair (a target under the
/// crosshair scores low), for tracks with 3 or more such frames, and the recording's level.
pub fn faint_scores(frames: &[TrackFrame], near: f64) -> FaintScores {
    let mut scores = Vec::new();
    for (id, mut values) in scores_by_track(frames, near) {
        if values.len() >= MIN_SCORED_FRAMES {
            values.sort_by(f64::total_cmp);
            let last = values.len() - 1;
            let index = ((TRACK_PERCENTILE * last as f64 + 0.5) as usize).min(last);
            scores.push(TrackScore { id, score: values[index], frames: values.len() });
        }
    }
    let level = recording_level(&scores);
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
    let scores = faint_scores(frames, near);
    let Some(level) = scores.level else {
        return FaintCutFrames { frames: frames.to_vec(), cut: None, gone: 0 };
    };
    let cut = level - offset;
    let gone: HashSet<u32> = scores.scores.iter().filter(|track| track.score < cut).map(|track| track.id).collect();
    let frames = frames.iter().map(|frame| without_tracks(frame, &gone)).collect();
    FaintCutFrames { frames, cut: Some(round(cut, CUT_DECIMALS)), gone: gone.len() }
}

/// A frame without the tracks `gone`.
fn without_tracks(frame: &TrackFrame, gone: &HashSet<u32>) -> TrackFrame {
    let keep: Vec<usize> = (0..frame.t.len()).filter(|&index| !gone.contains(&frame.t[index].0)).collect();
    TrackFrame {
        i: frame.i,
        shift: frame.shift,
        t: picked(&frame.t, &keep),
        a: picked(&frame.a, &keep),
        wh: frame.wh.as_ref().map(|sizes| picked(sizes, &keep)),
        s: frame.s.as_ref().map(|frame_scores| picked(frame_scores, &keep)),
    }
}

/// The values at the indexes kept.
pub(crate) fn picked<Value: Copy>(values: &[Value], keep: &[usize]) -> Vec<Value> {
    keep.iter().filter_map(|&index| values.get(index).copied()).collect()
}

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
    let short: String = stem.chars().take(STEM_CHARACTERS).collect();
    format!("{short}_{}", &hex(&md5(name.as_bytes()))[..STEM_HASH_DIGITS]).replace(' ', "_")
}

/// A target's box in pixels (center x, y, width, height) from its place and size in degrees.
fn box_px(x: f64, y: f64, width_deg: f64, height_deg: f64) -> [f64; 4] {
    let (center_x, center_y) = to_px(x, y);
    let (left, top) = to_px(x - width_deg / 2.0, y + height_deg / 2.0);
    let (right, bottom) = to_px(x + width_deg / 2.0, y - height_deg / 2.0);
    [center_x, center_y, (right - left).abs(), (bottom - top).abs()]
}

/// A crop's corner along one axis (pixels): `center` less half a crop, moved by `jitter`, inside the frame's `size`.
fn crop_corner(center: f64, size: usize, jitter: i64) -> usize {
    (center - (CROP / 2) as f64 + jitter as f64).clamp(0.0, (size - CROP) as f64) as usize
}

/// The boxes rounded as a label keeps them.
fn rounded_boxes(boxes: &[[f64; 4]]) -> Vec<[f64; 4]> {
    boxes.iter().map(|target_box| target_box.map(|value| round(value, BOX_DECIMALS))).collect()
}

/// The excluded areas in pixels (left, top, right, bottom) from their shares of the frame (None: KovOBS's layout).
fn excluded_areas_px(shares: Option<&[[f64; 4]]>) -> Vec<[f64; 4]> {
    let (width, height) = (W as f64, H as f64);
    let overlay = overlay_shares();
    let shares = shares.unwrap_or(&overlay);
    shares.iter().map(|area| [area[0] * width, area[1] * height, area[2] * width, area[3] * height]).collect()
}

/// A crop's square on the frame: its left and top (pixels at 1280 x 720).
#[derive(Clone, Copy)]
struct CropSquare {
    left: f64,
    top: f64,
}

impl CropSquare {
    /// Whether it overlaps an area (pixels: left, top, right, bottom).
    fn overlaps(self, [left, top, right, bottom]: [f64; 4]) -> bool {
        let side = CROP as f64;
        left < self.left + side && self.left < right && top < self.top + side && self.top < bottom
    }

    /// Whether a box's center (pixels) is inside it.
    fn holds(self, target_box: &[f64; 4]) -> bool {
        let side = CROP as f64;
        self.left <= target_box[0]
            && target_box[0] < self.left + side
            && self.top <= target_box[1]
            && target_box[1] < self.top + side
    }

    /// A box in the crop's pixels.
    fn shifted(self, target_box: &[f64; 4]) -> [f64; 4] {
        [target_box[0] - self.left, target_box[1] - self.top, target_box[2], target_box[3]]
    }
}

/// What a submitted cut-off's crops are made with: the request, each scored track's score, the score it cuts at, the
/// excluded areas (pixels), the file names' stem, and Python's random numbers, seeded with the stem.
struct CropMaker<'a> {
    request: &'a CutoffRequest,
    scores: HashMap<u32, f64>,
    cut: f64,
    excluded_px: Vec<[f64; 4]>,
    stem: String,
    random: PyRandom,
}

impl CropMaker<'_> {
    /// Whether a crop may be taken round a track: scored, on the side of the cut asked for (`left_out` or kept), and
    /// `near` degrees or more from the crosshair.
    fn in_focus(&self, left_out: bool, &(id, x, y): &TrackPoint) -> bool {
        self.scores.get(&id).is_some_and(|&score| (score < self.cut) == left_out) && hypot(x, y) >= self.request.near
    }

    /// Up to 20 crops round the tracks left out (`left_out`) or kept, from the frames in `frames` with boxes and such
    /// a track, spread over them.
    fn add_crops(&mut self, left_out: bool, frames: Range<usize>, out: &mut Vec<CutoffCrop>) {
        let request = self.request;
        let candidates: Vec<usize> = frames
            .filter(|&i| {
                let frame = &request.frames[i];
                frame.wh.is_some() && frame.t.iter().any(|point| self.in_focus(left_out, point))
            })
            .collect();
        for step in 0..CROPS_PER_SIDE.min(candidates.len()) {
            let i = candidates[((step * candidates.len()) as f64 / CROPS_PER_SIDE as f64) as usize];
            out.extend(self.crop(left_out, i));
        }
    }

    /// A crop of frame `i` round one of its tracks in focus, the track and the corner's jitter drawn from Python's
    /// random numbers; None where it touches an excluded area or holds a track too short to have a score.
    fn crop(&mut self, left_out: bool, i: usize) -> Option<CutoffCrop> {
        let request = self.request;
        let frame = &request.frames[i];
        let sizes = frame.wh.as_ref()?;
        let boxes: Vec<[f64; 4]> =
            frame.t.iter().zip(sizes).map(|(&(_, x, y), &(width, height))| box_px(x, y, width, height)).collect();
        let focus_boxes: Vec<&[f64; 4]> = boxes
            .iter()
            .zip(&frame.t)
            .filter(|&(_, point)| self.in_focus(left_out, point))
            .map(|(target_box, _)| target_box)
            .collect();
        let target = focus_boxes[self.random.choice(focus_boxes.len())];
        let left = crop_corner(target[0], W, self.random.randint(-CROP_JITTER_PX, CROP_JITTER_PX));
        let top = crop_corner(target[1], H, self.random.randint(-CROP_JITTER_PX, CROP_JITTER_PX));
        let square = CropSquare { left: left as f64, top: top as f64 };
        if self.excluded_px.iter().any(|&area| square.overlaps(area)) {
            return None;
        }
        let inside: Vec<(&[f64; 4], &TrackPoint)> =
            boxes.iter().zip(&frame.t).filter(|(target_box, _)| square.holds(target_box)).collect();
        if inside.iter().any(|&(_, &(id, x, y))| !self.scores.contains_key(&id) && hypot(x, y) >= request.near) {
            return None;
        }
        let kept: Vec<[f64; 4]> = inside
            .iter()
            .filter(|&&(_, &(id, _, _))| self.scores.get(&id).is_none_or(|&score| score >= self.cut))
            .map(|(target_box, _)| square.shifted(target_box))
            .collect();
        let every: Vec<[f64; 4]> = inside.iter().map(|(target_box, _)| square.shifted(target_box)).collect();
        Some(CutoffCrop { frame: i, x0: left, y0: top, row: self.row(i, &kept, &every), boxes: kept })
    }

    /// The label's row for a crop of frame `i`: the boxes it keeps and every box the model gave there (crop pixels).
    fn row(&self, i: usize, kept: &[[f64; 4]], every: &[[f64; 4]]) -> CutoffRow {
        CutoffRow {
            file: format!("train/{}_{i:06}.npz", self.stem),
            boxes: rounded_boxes(kept),
            verdict: "correct",
            model: rounded_boxes(every),
            source: "cutoff",
            video: self.request.video.clone(),
            offset: self.request.offset,
            cut: round(self.cut, CUT_DECIMALS),
        }
    }
}

/// The crops a submitted cut-off gives (hand_crops.py: `cutoff_crops`, which also reads each crop's pixels and the
/// fixed map: the page does that). The tracks the cut leaves out are not targets, the ones it keeps are. Only frames
/// inside the run, crops clear of every excluded area, and none holding a track too short to have a score (away from
/// the crosshair). Up to 20 crops round a left-out track and 20 round a kept one, from frames spread over the run, at
/// places Python's random numbers pick (seeded with the file names' stem), so they are the crops Python takes.
pub fn cutoff_crops(request: &CutoffRequest) -> Vec<CutoffCrop> {
    let scores = faint_scores(&request.frames, request.near);
    let (Some(level), Some(start), Some(end)) = (scores.level, request.start, request.end) else {
        return Vec::new();
    };
    let stem = crop_stem(&request.video);
    let mut maker = CropMaker {
        request,
        scores: scores.scores.iter().map(|track| (track.id, track.score)).collect(),
        cut: level - request.offset,
        excluded_px: excluded_areas_px(request.exclude.as_deref()),
        random: PyRandom::seeded(&stem),
        stem,
    };
    let first = start.max(0) as usize;
    let last = (request.frames.len() as i64).min(end + 1).max(0) as usize;
    let mut out = Vec::new();
    for left_out in [true, false] {
        maker.add_crops(left_out, first..last, &mut out);
    }
    out
}

/// A cut-off's crops as JSON (an array of `CutoffCrop`), or {"error": ...} for a request that cannot be read.
pub fn cutoff_json(request: &[u8]) -> Vec<u8> {
    match serde_json::from_slice::<CutoffRequest>(request) {
        Ok(parsed) => serde_json::to_vec(&cutoff_crops(&parsed)),
        Err(error) => {
            let message = format!("The labels' request could not be read: {error}");
            serde_json::to_vec(&serde_json::json!({ "error": message }))
        }
    }
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(i: usize, points: Vec<(u32, f64, f64)>, scores: Vec<f64>) -> TrackFrame {
        let count = points.len();
        let sizes = Some(vec![(1.0, 1.0); count]);
        TrackFrame { i, shift: (0.0, 0.0), t: points, a: vec![10; count], wh: sizes, s: Some(scores) }
    }

    /// Two tracks: one scoring 0.9 throughout, one 0.4. The level is the strong one's score; an offset of 0.3 cuts the
    /// weak one, and with near 2 its frames under the crosshair do not count.
    #[test]
    fn a_weak_track_is_cut() {
        let frames: Vec<TrackFrame> =
            (0..10).map(|i| frame(i, vec![(1, 5.0, 0.0), (2, 3.0, 1.0)], vec![0.9, 0.4])).collect();
        let scores = faint_scores(&frames, 2.0);
        assert_eq!(scores.level, Some(0.9));
        let strong = TrackScore { id: 1, score: 0.9, frames: 10 };
        assert_eq!(scores.scores, vec![strong, TrackScore { id: 2, score: 0.4, frames: 10 }]);
        let cut = without_faint(&frames, 0.3, 2.0);
        assert_eq!((cut.cut, cut.gone), (Some(0.6), 1));
        let only_the_strong =
            |kept: &TrackFrame| kept.t.len() == 1 && kept.t[0].0 == 1 && kept.s.as_ref().unwrap().len() == 1;
        assert!(cut.frames.iter().all(only_the_strong));
        let none = without_faint(&frames, 0.6, 2.0);
        assert_eq!((none.cut, none.gone), (Some(0.3), 0));
    }

    #[test]
    fn tracks_without_scores_are_not_cut() {
        let mut unscored = frame(0, vec![(1, 5.0, 0.0)], vec![0.9]);
        unscored.s = None;
        let cut = without_faint(&[unscored.clone(), unscored.clone(), unscored], 0.3, 0.0);
        assert_eq!((cut.cut, cut.gone, cut.frames.len()), (None, 0, 3));
    }

    /// Python: `f"{Path(v).stem[:40]}_{hashlib.md5(Path(v).name.encode()).hexdigest()[:6]}".replace(" ", "_")`.
    #[test]
    fn the_stem_is_pythons() {
        let video = "1wall 6targets extra small/1wall 6targets extra small - 889.26 - 2026.10.01-16.17.48.mp4";
        assert_eq!(crop_stem(video), PYTHON_STEM);
    }

    const PYTHON_STEM: &str = "1wall_6targets_extra_small_-_889.26_-_20_6a3425";
}
