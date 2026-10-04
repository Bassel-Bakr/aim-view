//! The track step for one recording, or one run of it (a recording split into runs, reviewed at once): each frame's
//! boxes kept or dropped (raw boxes kept too), its excluded areas watched for pop-ups, then the frames where a pop-up
//! is off kept again, and all linked when the frames are in. The review session (src/session.rs) drives it for the
//! browser and the desktop app.

use serde::{Deserialize, Serialize};

use crate::detect;
use crate::model::ModelSettings;
use crate::popup::AreaWatch;
use crate::track::{Mask, RawBox, Spot, TrackFrame, keep, link, reopen};

pub struct Tracker {
    model: ModelSettings,
    areas: Vec<[f64; 4]>,
    mask: Mask,
    cap: Option<usize>,
    watch: AreaWatch,
    raw: Vec<Vec<RawBox>>,
    frames: Vec<Vec<Spot>>,
}

/// A run's part of the track step (`Tracker::part`): its frames' raw boxes and its area watch, joined in order with
/// the other runs' (`Tracker::add_part`).
#[derive(Serialize, Deserialize)]
pub struct TrackPart {
    raw: Vec<Vec<RawBox>>,
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
            areas,
            cap: (cap > 0).then_some(cap),
            raw: Vec::new(),
            frames: Vec::new(),
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
        Tracker::new(crate::geometry::overlay_shares(), cap)
    }

    /// The run starts at frame `first` of the recording: call before its first frame.
    pub fn start_at(&mut self, first: usize) {
        self.watch.start_at(first);
    }

    /// The frame the next boxes are from, as RGB24 at 1280 x 720: its excluded areas are watched for pop-ups.
    pub fn watch(&mut self, rgb: &[u8]) {
        self.watch.add(rgb);
    }

    /// One frame's detector output: the score map (gh x gw) and reg maps (4 x gh x gw). Returns the boxes kept.
    pub fn push_maps(&mut self, score: &[f32], reg: &[f32], gw: usize, gh: usize) -> usize {
        self.push_boxes(&detect::decode(score, reg, gw, gh, &self.model))
    }

    /// One frame's boxes, already decoded (their scores on the reference model's scale). Returns the boxes kept.
    pub fn push_boxes(&mut self, raw: &[RawBox]) -> usize {
        let kept = keep(raw, &self.mask, self.cap);
        let n = kept.len();
        self.frames.push(kept);
        self.raw.push(raw.to_vec());
        n
    }

    /// The run's part, for joining with the other runs'.
    pub fn part(self) -> TrackPart {
        TrackPart { raw: self.raw, watch: self.watch }
    }

    /// The next run's part, after the frames the tracker has: each frame's boxes kept or dropped as `push_boxes`
    /// does, and its looks after the ones before. A part that starts later (a review from part way in) has empty frames
    /// before it. Returns the frames the part added.
    pub fn add_part(&mut self, part: TrackPart) -> usize {
        while self.raw.len() < part.watch.from() {
            self.push_boxes(&[]);
        }
        for raw in &part.raw {
            self.push_boxes(raw);
        }
        self.watch.join(part.watch);
        part.raw.len()
    }

    /// The frames linked: tracks.json's `frames`.
    pub fn finish(mut self) -> Vec<TrackFrame> {
        let shows = self.watch.showing();
        reopen(&self.raw, &mut self.frames, &self.areas, &shows, self.cap);
        link(&self.frames)
    }
}

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
            run.push_boxes(&[target]);
        }
        let mut joined = Tracker::kovobs(0);
        assert_eq!(joined.add_part(run.part()), 3);
        let frames = joined.finish();
        assert_eq!(frames.len(), 8);
        assert!(frames[..5].iter().all(|f| f.t.is_empty()));
        assert!(frames[5..].iter().all(|f| f.t.len() == 1));
    }

    /// A model's settings file decides which cells are boxes and their scores: a cell is one when its score, mapped onto
    /// the reference model's scale, is over the threshold, and the tracks keep the mapped score.
    #[test]
    fn the_settings_file_decides_the_boxes_kept() {
        // one row of maps with four cells far apart, scoring 0.2, 0.35, 0.6 and 0.9
        let (gw, gh) = (64, 1);
        let mut score = vec![0f32; gw * gh];
        for (x, s) in [(4, 0.2), (20, 0.35), (36, 0.6), (52, 0.9)] {
            score[x] = s;
        }
        let reg = vec![0f32; 4 * gw * gh];
        let scores = |file: Option<&str>| {
            let mut t = Tracker::new(vec![], 0);
            if let Some(file) = file {
                t.set_model(ModelSettings::from_json(file).unwrap());
            }
            t.push_maps(&score, &reg, gw, gh);
            t.finish()[0].s.clone().unwrap_or_default()
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
