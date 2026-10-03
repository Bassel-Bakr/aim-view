//! The track step for one recording, or one run of it (a recording split into runs, reviewed at once): each frame's
//! boxes kept or dropped (raw boxes kept too), its excluded areas watched for pop-ups, then the frames where a pop-up
//! is off kept again, and all linked when the frames are in. The browser drives it through src/wasm.rs, the desktop
//! app directly.

use serde::{Deserialize, Serialize};

use crate::detect;
use crate::popup::AreaWatch;
use crate::track::{Mask, RawBox, Spot, TrackFrame, keep, link, reopen};

pub struct Tracker {
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
    /// count, 0 for none.
    pub fn new(areas: Vec<[f64; 4]>, cap: usize) -> Tracker {
        Tracker {
            mask: Mask::without(&areas),
            watch: AreaWatch::new(&areas),
            areas,
            cap: (cap > 0).then_some(cap),
            raw: Vec::new(),
            frames: Vec::new(),
        }
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
        self.push_boxes(&detect::decode(score, reg, gw, gh, detect::THRESHOLD))
    }

    /// One frame's boxes, already decoded. Returns the boxes kept.
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
    /// does, and its looks after the ones before. Returns the frames added.
    pub fn add_part(&mut self, part: TrackPart) -> usize {
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
