//! The track step for one recording, or one run of it (a recording split into runs, reviewed at once): each frame's
//! boxes as the detector gave them, its excluded areas watched for pop-ups, then, when the frames are in, each frame's
//! boxes kept or dropped (those under a pop-up that is off kept again) and all linked. The review session (src/session.rs) drives it for the
//! browser and the desktop app.
//!
//! In: each frame's detector maps (or boxes already decoded) and its 720p RGB, and the other runs' parts. Out: the
//! frames' tracks (tracks.json's `frames`), and a run's part for joining.

use serde::{Deserialize, Serialize};

use crate::detect;
use crate::model::ModelSettings;
use crate::popup::AreaWatch;
use crate::track::{Mask, RawBox, Spot, TrackFrame, keep, link, reopen};

/// The track step's state: the settings it decodes and keeps boxes with, and what it has of the frames so far.
pub struct Tracker {
    /// The detector model's settings, for decoding its maps.
    model: ModelSettings,
    /// The excluded areas, as shares of the frame [x0, y0, x1, y1].
    areas: Box<[[f64; 4]]>,
    /// The pixels the excluded areas leave, where boxes count while every area is excluded.
    mask: Mask,
    /// The scenario's target count, when it is known.
    cap: Option<usize>,
    /// Watches each excluded area for when it shows, so the frames where a pop-up is off get their boxes back.
    watch: AreaWatch,
    /// Each frame's boxes as the detector gave them: `finish` keeps or drops them, once, when every run's part is in.
    raw: Vec<Vec<RawBox>>,
}

/// A run's part of the track step (`Tracker::part`): its frames' raw boxes and its area watch, joined in order with
/// the other runs' (`Tracker::add_part`).
#[derive(Serialize, Deserialize)]
pub struct TrackPart {
    /// Each of the run's frames' boxes as the detector gave them.
    raw: Vec<Vec<RawBox>>,
    /// The run's area watch, which knows the frame it started at.
    watch: AreaWatch,
}

impl Tracker {
    /// For a recording with these excluded areas (shares of the frame, [x0, y0, x1, y1]); cap: the scenario's target
    /// count, 0 for none. The detector model's settings are today's values until `set_model`.
    pub fn new(areas: Vec<[f64; 4]>, cap: usize) -> Tracker {
        Tracker {
            model: ModelSettings::default(),
            mask: Mask::without(&areas),
            watch: AreaWatch::new(&areas),
            areas: areas.into_boxed_slice(),
            cap: (cap > 0).then_some(cap),
            raw: Vec::new(),
        }
    }

    /// Which areas are the challenge's end screen (popup::END_SCREEN), in the areas' order: each is excluded only while
    /// it shows.
    pub fn end_screens(mut self, which: &[bool]) -> Tracker {
        self.watch.end_screens(which);
        self
    }

    /// The detector model's settings (its settings file): the threshold and score map `push_maps` decodes with.
    pub fn set_model(&mut self, model: ModelSettings) {
        self.model = model;
    }

    /// With the KovOBS overlay excluded (the default areas).
    pub fn kovobs(cap: usize) -> Tracker {
        Tracker::new(crate::geometry::overlay_shares().to_vec(), cap)
    }

    /// The run starts at frame `first` of the recording: call before its first frame.
    pub fn start_at(&mut self, first: usize) {
        self.watch.start_at(first);
    }

    /// The frame the next boxes are from, as RGB24 at 1280 x 720: its excluded areas are watched for pop-ups.
    pub fn watch(&mut self, rgb: &[u8]) {
        self.watch.add(rgb);
    }

    /// One frame's detector output: the score map (grid_height x grid_width) and the regression maps (4 x grid_height
    /// x grid_width: `detect::decode`).
    pub fn push_maps(&mut self, score: &[f32], regression: &[f32], grid_width: usize, grid_height: usize) {
        self.push_boxes(detect::decode(score, regression, grid_width, grid_height, &self.model));
    }

    /// One frame's boxes, already decoded (their scores on the reference model's scale).
    pub fn push_boxes(&mut self, raw: Vec<RawBox>) {
        self.raw.push(raw);
    }

    /// The run's part, for joining with the other runs'.
    pub fn part(self) -> TrackPart {
        TrackPart { raw: self.raw, watch: self.watch }
    }

    /// The next run's part, after the frames the tracker has: its frames' boxes, and its area watch joined after the
    /// one before. A part that starts later (a review from part way in) has empty frames before it. Returns the frames
    /// the part added.
    pub fn add_part(&mut self, part: TrackPart) -> usize {
        while self.raw.len() < part.watch.from() {
            self.push_boxes(Vec::new());
        }
        let added = part.raw.len();
        self.raw.extend(part.raw);
        self.watch.join(part.watch);
        added
    }

    /// The frames linked: tracks.json's `frames`. Each frame's boxes are kept or dropped with every area excluded,
    /// then each frame gets back the boxes under a pop-up that was not showing then.
    pub fn finish(self) -> Box<[TrackFrame]> {
        let mut frames: Vec<Vec<Spot>> = self.raw.iter().map(|raw| keep(raw, &self.mask, self.cap)).collect();
        let shows = self.watch.showing();
        reopen(&self.raw, &mut frames, &self.areas, &shows, self.cap);
        link(&frames)
    }
}

/// Checks joining run parts and decoding with a settings file.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{H, W};

    /// A review from part way in: its part joins with empty frames before it, so the frames keep their places.
    #[test]
    fn a_part_from_part_way_in_keeps_the_frames_in_place() {
        let rgb = vec![0u8; W * H * 3];
        let target = RawBox { cx: 640.0, cy: 300.0, w: 20.0, h: 20.0, score: 0.9 };
        let mut run = Tracker::kovobs(0);
        run.start_at(5);
        for _ in 0..3 {
            run.watch(&rgb);
            run.push_boxes(vec![target]);
        }
        let mut joined = Tracker::kovobs(0);
        assert_eq!(joined.add_part(run.part()), 3);
        let frames = joined.finish();
        assert_eq!(frames.len(), 8);
        assert!(frames[..5].iter().all(|frame| frame.t.is_empty()));
        assert!(frames[5..].iter().all(|frame| frame.t.len() == 1));
    }

    /// A model's settings file decides which cells are boxes and their scores: a cell is one when its score, mapped
    /// onto the reference model's scale, is over the threshold, and the tracks keep the mapped score.
    #[test]
    fn the_settings_file_decides_the_boxes_kept() {
        // one row of maps with four cells far apart, scoring 0.2, 0.35, 0.6 and 0.9
        let (grid_width, grid_height) = (64, 1);
        let mut score = vec![0f32; grid_width * grid_height];
        for (x, cell_score) in [(4, 0.2), (20, 0.35), (36, 0.6), (52, 0.9)] {
            score[x] = cell_score;
        }
        let regression = vec![0f32; 4 * grid_width * grid_height];
        let scores = |file: Option<&str>| {
            let mut tracker = Tracker::new(vec![], 0);
            if let Some(file) = file {
                tracker.set_model(ModelSettings::from_json(file).unwrap());
            }
            tracker.push_maps(&score, &regression, grid_width, grid_height);
            tracker.finish()[0].s.clone().unwrap_or_default()
        };
        assert_eq!(scores(None), vec![0.35, 0.6, 0.9]);
        let file = |threshold: f64, map: &str| {
            let rest = r#""name": "made_up", "reference": "full_v3""#;
            format!(r#"{{"format": 1, "threshold": {threshold}, "score_map": {map}, {rest}}}"#)
        };
        assert_eq!(scores(Some(&file(0.3, "null"))), vec![0.35, 0.6, 0.9]);
        assert_eq!(scores(Some(&file(0.5, "null"))), vec![0.6, 0.9]);
        // 0.2, 0.35, 0.6 and 0.9 map to 0.1, 0.175, 0.4 and 0.85
        assert_eq!(scores(Some(&file(0.3, "[[0, 0], [0.5, 0.25], [1, 1]]"))), vec![0.4, 0.85]);
        assert_eq!(scores(Some(&file(0.15, "[[0, 0], [0.5, 0.25], [1, 1]]"))), vec![0.175, 0.4, 0.85]);
    }
}
