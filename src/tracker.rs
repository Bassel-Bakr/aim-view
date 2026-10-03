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

    /// Which areas are the challenge's end screen (popup::END_SCREEN), in the areas' order: each is excluded only while
    /// it shows.
    pub fn end_screens(mut self, which: &[bool]) -> Tracker {
        self.watch.end_screens(which);
        self
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
}
