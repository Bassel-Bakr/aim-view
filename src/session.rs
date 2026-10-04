//! A review session: everything a review does between the decoder and the detector, the same for the browser and the
//! desktop. The hosts (the browser's review and camera workers in ui/src/app/modes/wasm/, the native review in
//! service/src/review.rs) decode the frames, run the detector and feed this. A recording is split into runs at key
//! frames, reviewed at once (one decoder is the limit, in the browser and natively alike), then joined:
//!
//! 1. `Review::new` plans the runs from the video's frames and the setup.
//! 2. `Keys` reads the key frames: the fixed map (the detector's 4th input) and where the HUD's boxes are.
//! 3. In each run, `RunTracking` says what each decoded frame is for, watches the excluded areas of the frames it
//!    tracks and takes the detector's maps in order. `RunWatching`, which can run beside it, reads each frame's Y plane
//!    and countdown rows: the camera's turn, KovaaK's countdown bar and the HUD. A run but the last also reads the next
//!    run's first frame, for the camera's turn into it.
//! 4. `Joining` puts the runs' parts together: the tracks, the video's readings and what the HUD read.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::camera::{COUNTDOWN_ROWS, CameraPart, CameraWatch, VideoReadings, excluded};
use crate::convert::{Converter, DST_H as H, DST_W as W, Matrix};
use crate::fixed::FixedMap;
use crate::hud::{HudKeys, HudPart, HudReading, HudWatch};
use crate::model::ModelSettings;
use crate::track::{Mask, REVIEW_VERSION, TrackFrame};
use crate::tracker::{TrackPart, Tracker};

/// The fewest frames a run has (10 s at 60 frames a second): a shorter recording is one run.
pub const LEAST_RUN: usize = 600;

/// A part of a video, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TimeWindow {
    pub start: f64,
    pub end: f64,
}

/// The frames to review: from `first` up to `end` (not included), as indexes in the recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRange {
    pub first: usize,
    pub end: usize,
}

/// The frames a time window holds: from the first at or after its start to the last at or before its end; all of them
/// for no window, or one with no frames.
pub fn window_frames(times: &[f64], window: Option<TimeWindow>) -> FrameRange {
    let all = FrameRange { first: 0, end: times.len() };
    let Some(w) = window else { return all };
    let Some(first) = times.iter().position(|&t| t >= w.start) else { return all };
    let end = times.iter().position(|&t| t > w.end).unwrap_or(times.len());
    if end <= first { all } else { FrameRange { first, end } }
}

/// A run of the recording's frames: from a key frame's time (`from`; the recording's first run at 0) to the next run's
/// (`to`, None for the last), its first frame's index in the recording and how many frames it has.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "VideoRun"))]
pub struct Run {
    pub from: f64,
    pub to: Option<f64>,
    pub first: usize,
    pub frames: usize,
}

impl Run {
    /// The frames the run reads: its own, and the next run's first but in the last run.
    pub fn reads(&self) -> usize {
        self.frames + usize::from(self.to.is_some())
    }
}

/// The recording's frames in `range` split into up to `parts` runs, each from a key frame: the first from the key frame
/// at or before the range's first frame (decoding starts at a key frame), each cut at the key frame nearest its share of
/// the frames, and none that would leave a run of fewer than `least` frames.
pub fn split_runs(times: &[f64], keys: &[f64], parts: usize, least: usize, range: FrameRange) -> Vec<Run> {
    let begin = keys
        .iter()
        .filter_map(|&k| times.iter().position(|&t| t == k))
        .filter(|&i| i <= range.first)
        .max()
        .unwrap_or(0);
    let n = range.end - begin;
    let mut starts = vec![begin];
    for i in 1..parts {
        let want = times[begin + n * i / parts];
        let best = keys
            .iter()
            .copied()
            .filter(|&k| k > times[begin])
            .min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()));
        let Some(at) = best.and_then(|k| times.iter().position(|&t| t == k)) else { continue };
        if at >= starts[starts.len() - 1] + least && range.end >= at + least {
            starts.push(at);
        }
    }
    (0..starts.len())
        .map(|i| {
            let next = starts.get(i + 1).copied();
            Run {
                from: if starts[i] > 0 { times[starts[i]] } else { 0.0 },
                to: next.map(|j| times[j]),
                first: starts[i],
                frames: next.unwrap_or(range.end) - starts[i],
            }
        })
        .collect()
}

/// An area the review leaves out: [x0, y0, x1, y1] as shares of the frame, and its kind's id.
pub type AreaBox = (f64, f64, f64, f64, String);

/// The recording's frames as the decoder gives them: their size, their colour matrix (`Matrix::from_code`) and whether
/// they are full range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FrameFormat {
    pub width: usize,
    pub height: usize,
    pub matrix: u32,
    pub full: bool,
}

impl FrameFormat {
    fn converter(&self) -> Converter {
        Converter::new(self.width, self.height, Matrix::from_code(self.matrix), self.full)
    }
}

/// What a review is set up from: the video's frame rate, every frame's time and the key frames' (from 0 on, in order)
/// and the frames' format; the scenario's target count (0: not known), the areas the review leaves out, the part of
/// the video to track (None: all of it), the runs to split it into, and the detector model's settings (its settings
/// file; today's values without one).
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "ReviewSetup"))]
pub struct Setup {
    pub fps: f64,
    pub times: Vec<f64>,
    pub keys: Vec<f64>,
    pub format: FrameFormat,
    pub cap: usize,
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::AreaBox>"))]
    pub areas: Vec<AreaBox>,
    pub window: Option<TimeWindow>,
    pub runs: usize,
    #[serde(skip)]
    pub model: ModelSettings,
}

/// A review: its setup and the runs it is split into.
#[derive(Clone, Debug)]
pub struct Review {
    setup: Setup,
    runs: Vec<Run>,
}

impl Review {
    pub fn new(setup: Setup) -> Result<Review, String> {
        if setup.times.is_empty() {
            return Err("the video has no frames".into());
        }
        let range = window_frames(&setup.times, setup.window);
        let runs = split_runs(&setup.times, &setup.keys, setup.runs.max(1), LEAST_RUN, range);
        Ok(Review { setup, runs })
    }

    pub fn setup(&self) -> &Setup {
        &self.setup
    }

    /// The detector model's settings (its settings file), before the runs start.
    pub fn set_model(&mut self, model: ModelSettings) {
        self.setup.model = model;
    }

    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// The frames the review tracks, every run's.
    pub fn frames(&self) -> usize {
        self.runs.iter().map(|r| r.frames).sum()
    }

    /// The key frames' pass: every key frame in order, then `Keys::finish`.
    pub fn keys(&self) -> Keys {
        let f = self.setup.format;
        Keys { fixed: FixedMap::default(), hud: HudWatch::new(f.width, f.height, f.full) }
    }

    /// Run `run`'s tracking.
    pub fn tracking(&self, run: usize) -> RunTracking {
        let r = &self.runs[run];
        let mut tracker = self.tracker();
        tracker.start_at(r.first);
        RunTracking { tracker, from: r.from, frames: r.frames, reads: r.reads(), read: 0, pushed: 0 }
    }

    /// Run `run`'s watches, from what the key frames gave. In a review from part way in, the first run's watches have
    /// nothing before its first frame.
    pub fn watching(&self, run: usize, keys: &KeysRead) -> RunWatching {
        let r = &self.runs[run];
        let skip = if run == 0 { r.first } else { 0 };
        let f = self.setup.format;
        let mut camera = self.camera(&keys.fixed);
        let mut hud = HudWatch::from_keys(f.width, f.height, f.full, &keys.hud);
        camera.skip(skip);
        hud.skip(skip);
        RunWatching {
            convert: f.converter(),
            camera,
            hud,
            luma: vec![0; W * H].try_into().unwrap(),
            rgb: vec![0; W * H * 3].try_into().unwrap(),
            y_bytes: f.width * f.height,
            from: r.from,
            reads: r.reads(),
            read: 0,
        }
    }

    /// The runs' parts joined, in order; `fixed`: the key frames' fixed map.
    pub fn joining(&self, fixed: &[u8]) -> Joining {
        let f = self.setup.format;
        Joining {
            tracker: self.tracker(),
            camera: self.camera(fixed),
            hud: HudWatch::new(f.width, f.height, f.full),
            fixed: fixed.iter().map(|&v| v as f64).sum::<f64>() / fixed.len().max(1) as f64,
            added: 0,
            joined: true,
            review: self.clone(),
        }
    }

    fn rects(&self) -> Vec<[f64; 4]> {
        self.setup.areas.iter().map(|a| [a.0, a.1, a.2, a.3]).collect()
    }

    /// The tracker for the areas and the model's settings: the challenge's end screen among the areas is left out only
    /// while it shows.
    fn tracker(&self) -> Tracker {
        let ends: Vec<bool> = self.setup.areas.iter().map(|a| a.4 == crate::popup::END_SCREEN).collect();
        let mut tracker = Tracker::new(self.rects(), self.setup.cap).end_screens(&ends);
        tracker.set_model(self.setup.model.clone());
        tracker
    }

    /// The camera watch, its tiles kept clear of the areas (KovOBS's layout when there are none, as python/review.py
    /// does) and of the fixed map.
    fn camera(&self, fixed: &[u8]) -> CameraWatch {
        let rects = if self.setup.areas.is_empty() { crate::geometry::overlay_shares().to_vec() } else { self.rects() };
        CameraWatch::new(&excluded(Mask::without(&rects).kept(), fixed))
    }
}

/// The key frames' pass (`Review::keys`).
pub struct Keys {
    fixed: FixedMap,
    hud: HudWatch,
}

/// What the key frames give every run: the fixed map (1280 x 720, 1 fixed) and where the HUD's boxes are.
pub struct KeysRead {
    pub fixed: Vec<u8>,
    pub hud: HudKeys,
}

impl Keys {
    /// One key frame, in order: at 1280 x 720 as YUV 4:2:0 (for the fixed map), and its Y plane as decoded (for the
    /// HUD's boxes).
    pub fn add(&mut self, small: &[u8], y: &[u8]) {
        self.fixed.add(small);
        self.hud.add_key(y);
    }

    pub fn finish(mut self) -> KeysRead {
        KeysRead { fixed: self.fixed.map(), hud: self.hud.keys() }
    }
}

/// What a decoded frame is for (`RunTracking::next_frame`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextFrame {
    /// One of the run's frames: to the tracker (`RunTracking::watch`), the detector and the watches.
    Track,
    /// The next run's first frame: to the watches only.
    Watch,
    /// Past the run: not read, and the decoding stops.
    Stop,
}

/// A run's tracking (`Review::tracking`): which frames it reads, each tracked frame's excluded areas watched for
/// pop-ups, and the detector's maps in order.
pub struct RunTracking {
    tracker: Tracker,
    from: f64,
    frames: usize,
    reads: usize,
    read: usize,
    pushed: usize,
}

impl RunTracking {
    /// What the next decoded frame is for. Each call but one that says Stop counts a frame read.
    pub fn next_frame(&mut self) -> NextFrame {
        if self.read == self.reads {
            return NextFrame::Stop;
        }
        self.read += 1;
        if self.read > self.frames { NextFrame::Watch } else { NextFrame::Track }
    }

    /// A tracked frame as RGB24 at 1280 x 720: its excluded areas are watched for pop-ups.
    pub fn watch(&mut self, rgb: &[u8]) {
        self.tracker.watch(rgb);
    }

    /// The detector's maps for the next tracked frame, in order: the score map (gh x gw) and the reg maps (4 x gh x gw).
    pub fn maps(&mut self, score: &[f32], reg: &[f32], gw: usize, gh: usize) {
        self.tracker.push_maps(score, reg, gw, gh);
        self.pushed += 1;
    }

    /// The run's part, for `Joining::add`: an error when the run did not get every frame and every frame's maps.
    pub fn part(self) -> Result<TrackPart, String> {
        let (from, read, reads, pushed, frames) = (self.from, self.read, self.reads, self.pushed, self.frames);
        if read != reads {
            return Err(format!("the run from {from:.3} s gave {read} frames where it has {reads}"));
        }
        if pushed != frames {
            return Err(format!("the detector gave the run from {from:.3} s {pushed} frames where it has {frames}"));
        }
        Ok(self.tracker.part())
    }
}

/// The bytes of a frame's 720p RGB24 that the watches read: the rows of KovaaK's countdown bar.
pub fn countdown_bytes() -> Range<usize> {
    COUNTDOWN_ROWS.0 * W * 3..COUNTDOWN_ROWS.1 * W * 3
}

/// A run's watches (`Review::watching`): the camera watch and the HUD watch, fed each frame the run reads.
pub struct RunWatching {
    convert: Converter,
    camera: CameraWatch,
    hud: HudWatch,
    luma: Box<[u8; W * H]>,
    rgb: Box<[u8; W * H * 3]>,
    y_bytes: usize,
    from: f64,
    reads: usize,
    read: usize,
}

/// A run's part of the watches (`RunWatching::part`).
#[derive(Serialize, Deserialize)]
pub struct WatchPart {
    pub camera: CameraPart,
    pub hud: HudPart,
}

impl RunWatching {
    /// One frame the run reads (`RunTracking::next_frame` said Track or Watch), in order: its Y plane as decoded (the
    /// format's size), and its countdown rows (`countdown_bytes` of its 720p RGB24). The camera watch reads its 720p
    /// luma, the same bytes as ffmpeg's.
    pub fn frame(&mut self, y: &[u8], rows: &[u8]) {
        self.convert.luma(&y[..self.y_bytes], &mut self.luma[..]);
        self.rgb[countdown_bytes()].copy_from_slice(rows);
        self.camera.add(&self.luma[..], &self.rgb[..]);
        self.hud.add(y);
        self.read += 1;
    }

    /// The bytes of a decoded frame's Y plane.
    pub fn y_bytes(&self) -> usize {
        self.y_bytes
    }

    /// The run's part, for `Joining::add`: an error when the run did not read every frame.
    pub fn part(self) -> Result<WatchPart, String> {
        if self.read != self.reads {
            let (from, got, has) = (self.from, self.read, self.reads);
            return Err(format!("the run from {from:.3} s gave the watches {got} frames where it has {has}"));
        }
        Ok(WatchPart { camera: self.camera.part(), hud: self.hud.part() })
    }
}

/// The tracks as tracks.json keeps them: the frame rate, each frame's targets, the share of the frame the fixed map
/// covers, and the detector that found them.
#[derive(Serialize)]
pub struct Tracks {
    pub fps: f64,
    pub frames: Vec<TrackFrame>,
    pub fixed: f64,
    pub detector: String,
    /// The part of the video tracked, when only part of it was; the frames outside are empty.
    pub window: Option<TimeWindow>,
    /// The review's version (src/track.rs: `REVIEW_VERSION`).
    pub version: u32,
    /// The areas it left out (a review is made again when the recording's change).
    pub areas: Vec<AreaBox>,
}

/// A review's runs joined: the tracks, the video's readings (each frame's camera turn and countdown bar), and what the
/// HUD read (None: no HUD was read).
#[derive(Serialize)]
pub struct Joined {
    pub tracks: Tracks,
    pub readings: VideoReadings,
    pub hud: Option<HudReading>,
}

/// The runs' parts joined in order (`Review::joining`).
pub struct Joining {
    review: Review,
    tracker: Tracker,
    camera: CameraWatch,
    hud: HudWatch,
    fixed: f64,
    added: usize,
    joined: bool,
}

impl Joining {
    /// The next run's parts: its tracking's and its watches'.
    pub fn add(&mut self, track: TrackPart, watch: WatchPart) {
        let frames = self.review.runs.get(self.added).map(|r| r.frames);
        self.joined &= Some(self.tracker.add_part(track)) == frames;
        self.camera.join(watch.camera);
        self.hud.join(watch.hud);
        self.added += 1;
    }

    /// The review, every run's parts added. `detector`: the detector that ran, as tracks.json names it.
    pub fn finish(self, detector: String) -> Result<Joined, String> {
        if !self.joined || self.added != self.review.runs.len() {
            return Err("the review's runs do not join up".into());
        }
        let frames = self.tracker.finish();
        let readings = self.camera.finish(&frames);
        if readings.countdown.len() != frames.len() {
            return Err("the camera watch's runs do not join up".into());
        }
        if self.hud.frames() != frames.len() {
            return Err("the HUD watch's runs do not join up".into());
        }
        let s = self.review.setup;
        let tracks = Tracks {
            fps: s.fps,
            frames,
            fixed: self.fixed,
            detector,
            window: s.window,
            version: REVIEW_VERSION,
            areas: s.areas,
        };
        Ok(Joined { tracks, readings, hud: self.hud.finish() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// n frames at 60 a second from 0, a key frame every `every` frames.
    fn video(n: usize, every: usize) -> (Vec<f64>, Vec<f64>) {
        let times: Vec<f64> = (0..n).map(|i| i as f64 / 60.0).collect();
        let keys = times.iter().copied().step_by(every).collect();
        (times, keys)
    }

    fn all(n: usize) -> FrameRange {
        FrameRange { first: 0, end: n }
    }

    /// Cut at the key frame nearest the middle, and the runs cover every frame once.
    #[test]
    fn runs_cover_every_frame_once() {
        let (times, keys) = video(6038, 240);
        assert_eq!(
            split_runs(&times, &keys, 2, 600, all(6038)),
            vec![
                Run { from: 0.0, to: Some(times[3120]), first: 0, frames: 3120 },
                Run { from: times[3120], to: None, first: 3120, frames: 2918 },
            ]
        );
        let (times, keys) = video(9000, 300);
        let firsts: Vec<usize> = split_runs(&times, &keys, 3, 600, all(9000)).iter().map(|r| r.first).collect();
        assert_eq!(firsts, vec![0, 3000, 6000]);
    }

    /// A short recording, one with a single key frame, and one whose only cut would leave a short run: one run each.
    #[test]
    fn no_cut_leaves_a_run_too_short() {
        let one = |n: usize| vec![Run { from: 0.0, to: None, first: 0, frames: n }];
        let (times, keys) = video(900, 240);
        assert_eq!(split_runs(&times, &keys, 2, 600, all(900)), one(900));
        let (times, _) = video(6000, 240);
        assert_eq!(split_runs(&times, &[0.0], 2, 600, all(6000)), one(6000));
        assert_eq!(split_runs(&times, &[0.0, times[5900]], 2, 600, all(6000)).len(), 1);
    }

    /// A window: from the key frame before it to its end; one with no frames is the whole video.
    #[test]
    fn runs_in_a_window() {
        let (times, keys) = video(6000, 240);
        let range = window_frames(&times, Some(TimeWindow { start: 30.0, end: 80.0 }));
        assert_eq!(range, FrameRange { first: 1800, end: 4801 });
        assert_eq!(
            split_runs(&times, &keys, 2, 600, range),
            vec![
                Run { from: times[1680], to: Some(times[3120]), first: 1680, frames: 1440 },
                Run { from: times[3120], to: None, first: 3120, frames: 1681 },
            ]
        );
        assert_eq!(window_frames(&times[..600], None), all(600));
        assert_eq!(window_frames(&times[..600], Some(TimeWindow { start: 50.0, end: 60.0 })), all(600));
    }

    /// A run reads its frames and the next run's first, which only the watches take; the last run reads its own.
    #[test]
    fn a_run_reads_the_next_runs_first_frame_for_the_watches() {
        let (times, keys) = video(1300, 600);
        let format = FrameFormat { width: 64, height: 36, matrix: 1, full: false };
        let setup = Setup {
            fps: 60.0,
            times,
            keys,
            format,
            cap: 0,
            areas: vec![],
            window: None,
            runs: 2,
            model: ModelSettings::default(),
        };
        let review = Review::new(setup).unwrap();
        assert_eq!(review.runs().iter().map(|r| r.frames).collect::<Vec<_>>(), vec![600, 700]);
        let mut first = review.tracking(0);
        let uses: Vec<NextFrame> = (0..602).map(|_| first.next_frame()).collect();
        assert!(uses[..600].iter().all(|&u| u == NextFrame::Track));
        assert_eq!(&uses[600..], &[NextFrame::Watch, NextFrame::Stop]);
        let mut last = review.tracking(1);
        assert_eq!((0..701).filter(|_| last.next_frame() == NextFrame::Track).count(), 700);
        assert_eq!(last.next_frame(), NextFrame::Stop);
        assert!(last.part().is_err(), "a run without its maps has no part");
    }

    /// A run tracked, watched and joined: every frame in the tracks and the readings, the setup's facts in the tracks;
    /// parts that do not match the runs are refused.
    #[test]
    fn a_run_joins_into_tracks_readings_and_hud() {
        let times: Vec<f64> = (0..5).map(|i| i as f64 / 60.0).collect();
        let format = FrameFormat { width: W, height: H, matrix: 1, full: false };
        let area: AreaBox = (0.75, 0.0, 1.0, 0.25, crate::popup::END_SCREEN.into());
        let setup = Setup {
            fps: 60.0,
            times,
            keys: vec![0.0],
            format,
            cap: 0,
            areas: vec![area.clone()],
            window: None,
            runs: 2,
            model: ModelSettings::default(),
        };
        let review = Review::new(setup).unwrap();
        assert_eq!(review.runs().len(), 1, "too short for two runs");
        let y = vec![16u8; W * H];
        let mut keys = review.keys();
        keys.add(&vec![16u8; W * H * 3 / 2], &y);
        let keys = keys.finish();
        let (gw, gh) = (W / 4, H / 4);
        let (score, reg, rgb) = (vec![0f32; gw * gh], vec![0f32; 4 * gw * gh], vec![0u8; W * H * 3]);
        let rows = vec![0u8; countdown_bytes().len()];
        let part = || {
            let (mut tracking, mut watching) = (review.tracking(0), review.watching(0, &keys));
            while tracking.next_frame() != NextFrame::Stop {
                tracking.watch(&rgb);
                tracking.maps(&score, &reg, gw, gh);
                watching.frame(&y, &rows);
            }
            (tracking.part().unwrap(), watching.part().unwrap())
        };
        let mut joining = review.joining(&keys.fixed);
        let (track, watch) = part();
        joining.add(track, watch);
        let joined = joining.finish("a detector".into()).unwrap();
        assert_eq!((joined.tracks.frames.len(), joined.readings.countdown.len()), (5, 5));
        assert_eq!((joined.tracks.fps, joined.tracks.version), (60.0, REVIEW_VERSION));
        assert_eq!((joined.tracks.detector.as_str(), joined.tracks.areas), ("a detector", vec![area]));
        assert!(joined.hud.is_none(), "a blank frame has no HUD");
        let mut joining = review.joining(&keys.fixed);
        for (track, watch) in [part(), part()] {
            joining.add(track, watch);
        }
        assert!(joining.finish("a detector".into()).is_err(), "one run, two parts");
    }
}
