//! Each kill matched to the target it killed, and the flick to it (python/retired/review.py: `match_times`,
//! `_attach_kills`, `appearances`, `crosshair_spots`, `ghosts`).
//!
//! In: a run's tracks (src/track.rs) and, from its stats file or HUD, the kill times (src/review.rs). Out: one `Flick`
//! per kill, with the target's path to the crosshair that src/measure.rs measures, and how the kills matched
//! (`MatchInfo`, in the report's summary). With kill times, `match_times`; without, `match_video` finds the kills in
//! the tracks. Both first drop what the detector boxed on the crosshair (`without_crosshair_boxes`,
//! `without_crosshair_ends`; the video alone also `ghosts`), and join the tracks of a target the tracker lost and found
//! again (`appearances`, `TrackIndex::continued`).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::capped::Capped;
use crate::geometry::{H, W, blob_radius_deg, to_deg};
use crate::measure::DEFAULT_TARGET_RADIUS_DEG;
use crate::python::hypot;
use crate::statistics::median;
use crate::track::{TrackFrame, TrackPoint, Tracks, spikes, view_shift_between};

/// A point of a target's path: frame, x and y (degrees from the crosshair).
pub type PathPoint = (i64, f64, f64);

/// A track's places by frame (degrees from the crosshair).
type Places = BTreeMap<i64, (f64, f64)>;

/// A track ends at the crosshair, as a killed target's does, when its last place is within this (degrees).
const AT_CROSSHAIR_DEG: f64 = 0.6;
/// A kill's track has at least this many places.
const MIN_KILL_TRACK_POINTS: usize = 3;
/// A track that ends within this many frames of the run's last frame was cut by the recording's end, not killed.
const END_MARGIN_FRAMES: i64 = 2;
/// `appearances` joins a track to one that ended up to this long before it starts (seconds), within JOIN_RADIUS_DEG of
/// where that one would be, as `match_times` and the report (src/review.rs) join them.
pub const JOIN_GAP_S: f64 = 0.5;
/// How near a track must start to where the track it continues would be (degrees), for `appearances`.
pub const JOIN_RADIUS_DEG: f64 = 1.0;
/// A track may start up to this many frames before the one it continues ends: a hit target flashes and is picked up
/// again while its old track still has a frame or two.
const OVERLAP_FRAMES: i64 = 2;
/// A target's own speed is taken over its last this many frames; the second join by it reaches this many frames back.
const SPEED_FRAMES: i64 = 3;
/// `appearances` leaves out tracks within this of a crosshair spot (degrees), where the detector marks the crosshair
/// every frame.
const SPOT_CLEARANCE_DEG: f64 = 0.2;
/// The killed target's track is seen from the window before the kill to this many frames after it.
const AFTER_KILL_FRAMES: i64 = 2;
/// A track's distance from a kill: its degrees from the crosshair plus this many for each second from the kill's time.
const DEG_PER_SECOND_OFF: f64 = 4.0;
/// The farthest a killed target's center may be from the crosshair (degrees), unless its blob's radius plus RIM_DEG
/// is farther: a big target can be hit at its rim. A target with a box may also be killed with the crosshair within
/// RIM_DEG of its box: a tall one (a robot) is hit at its head, far from its center.
const MAX_KILL_DISTANCE_DEG: f64 = 1.5;
/// How far past a target's blob radius, or past its box's edge, the crosshair may be and still hit it (degrees).
const RIM_DEG: f64 = 0.25;
/// A track last seen more than EARLY_FRAMES before the kill costs EARLY_COST_DEG more; one that never leaves a
/// crosshair spot costs SPOT_COST_DEG more (it is the killed target only when no other track is near).
const EARLY_FRAMES: i64 = 2;
/// What a track last seen early costs a kill's choice of target (degrees of distance).
const EARLY_COST_DEG: f64 = 0.2;
/// What a track that never leaves a crosshair spot costs a kill's choice of target (degrees of distance).
const SPOT_COST_DEG: f64 = 1.0;
/// A target hidden under the crosshair longer than the window is looked for up to this many windows back.
const HIDDEN_WINDOWS: i64 = 4;
/// A target first seen more than this many frames after its flick's start spawned meanwhile.
const SPAWN_FRAMES: i64 = 2;
/// A kill is confirmed when its target was last seen at the crosshair within this many frames of its kill time.
const CONFIRM_FRAMES: i64 = 2;
/// The kill times' clock lines a kill up with a track's end within this many frames; the offset is voted from the
/// first CLOCK_VOTES of each.
const CLOCK_TOLERANCE_FRAMES: f64 = 2.5;
/// The kill times and the video's kills the clock's offset is voted from: the first this many of each.
const CLOCK_VOTES: usize = 40;
/// `without_ghosts`: a track within GHOST_MAX_DEG of the crosshair that leaves more than GHOST_UNEXPLAINED_SHARE of
/// the camera's turn unexplained, over a turn of more than GHOST_MIN_TURN_DEG, is the crosshair.
const GHOST_MAX_DEG: f64 = 0.5;
/// The share of the camera's turn a track at the crosshair leaves unexplained beyond which it is the crosshair.
const GHOST_UNEXPLAINED_SHARE: f64 = 0.5;
/// The camera's turn (degrees) over a track at the crosshair beyond which its staying put says it is the crosshair.
const GHOST_MIN_TURN_DEG: f64 = 0.3;
/// A track this short on a crosshair spot is the crosshair: right after a kill it made the dead target look picked
/// up again.
const SHORT_SPOT_TRACK_POINTS: usize = 3;
/// The detector sees a target whole within this of the crosshair (degrees): only blob areas there count.
const WHOLE_TARGET_DEG: f64 = 3.0;
/// The run's typical target needs this many tracks' areas.
const MIN_TYPICAL_TRACKS: usize = 5;
/// A track of STEADY_TRACK_POINTS or more is steady; RUN_END_TRACKS steady tracks ending within a frame end the run.
const STEADY_TRACK_POINTS: usize = 5;
/// The steady tracks that, ending within a frame of each other, say the run ended or restarted: none of them is a kill.
const RUN_END_TRACKS: usize = 3;
/// A blob less than this share of the run's typical target is no target (a hit marker, a spark).
const MIN_TARGET_SHARE: f64 = 0.1;
/// Kills found within this many frames of each other are one kill.
const SAME_KILL_FRAMES: i64 = 3;
/// A jump of more than this many times `near` is a false camera turn without a spike (`TrackIndex::repair`).
const LONG_JUMP_SHARE: f64 = 2.0;
/// A target found again LONG_GAP_FRAMES or more frames after it was last seen comes back within LONG_GAP_RADII of its
/// radius from where it was, and about as big; one found again SHORT_GAP_FRAMES after, within SHORT_GAP_RADII.
const LONG_GAP_FRAMES: i64 = 3;
/// How far from where it was, in its radii, a target found again LONG_GAP_FRAMES or more after may come back.
const LONG_GAP_RADII: f64 = 0.5;
/// A target found again this many frames after it was last seen comes back within SHORT_GAP_RADII of where it was.
const SHORT_GAP_FRAMES: i64 = 2;
/// How far from where it was, in its radii, a target found again SHORT_GAP_FRAMES after may come back.
const SHORT_GAP_RADII: f64 = 1.5;
/// A missing target whose place came out from under the crosshair (beyond OUT_FROM_UNDER_SHARE of `near`) on
/// SEEN_FRAMES or more frames would have been seen: it died.
const OUT_FROM_UNDER_SHARE: f64 = 1.5;
/// The frames a missing target's place must be out from under the crosshair for it to count as dead.
const SEEN_FRAMES: usize = 2;
/// A target comes back about as big when its median blob area over SIZE_FRAMES frames is within SAME_SIZE_SHARES of
/// its old one.
const SIZE_FRAMES: usize = 3;
/// The least and most a target's new blob area may be, as shares of its old one, to count as the same target.
const SAME_SIZE_SHARES: (f64, f64) = (0.5, 2.0);

/// A kill and the flick to it: its number, the frame its target was last seen on and the frame the kill times give,
/// where the flick starts, the shots it took, the target's path, whether the target appeared after the kill before it,
/// and its median blob area in pixels.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Flick {
    /// The kill's number in the kill times' order, from 1 (a kill with no target found leaves a gap).
    pub kill_number: usize,
    /// The frame the target was last seen on, near the kill.
    pub kill_frame: i64,
    /// The frame the kill times give, on the video's clock; None from the video alone.
    pub stats_frame: Option<i64>,
    /// Where the flick starts: the kill before's frame, or the target's first appearance when it came later.
    pub start_frame: i64,
    /// The shots the kill took; None from the video alone.
    pub shots: Option<i64>,
    /// The target's places from the flick's start to its kill frame.
    pub path: Vec<PathPoint>,
    /// Whether the target appeared after the flick's start (more than SPAWN_FRAMES after it).
    pub spawned: bool,
    /// The target's median blob area (pixels); None when its tracks have no areas.
    pub area_px: Option<f64>,
}

/// Where the kill times came from.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum KillSource {
    /// The run's stats file.
    Stats,
    /// KovaaK's HUD, read in the video.
    Hud,
    /// Aim Lab's HUD, read in the video.
    Aimlab,
    /// The video alone (`match_video`).
    Video,
}

/// How the kills matched: kills seen in the video and in the kill times, kills matched, kills confirmed (the target
/// last seen at the crosshair within 2 frames of its kill time), the kill times' offset on the video's clock (s).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MatchInfo {
    /// The kills seen in the video: tracks that end at the crosshair (from the video alone, the kills it found).
    pub kills_video: usize,
    /// The kills the kill times hold; None from the video alone.
    pub kills_stats: Option<usize>,
    /// The kills matched to a target.
    pub matched: usize,
    /// The kills confirmed (`confirmed`); None when not checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub confirmed: Option<usize>,
    /// What to add to a kill time to put it on the video's clock (seconds); None from the video alone, and when there
    /// were no kill times, or no video kills to place them by.
    pub offset: Option<f64>,
    /// The video's frame rate (frames a second).
    pub fps: f64,
    /// Where the kill times came from; `match_times` leaves it None for its caller to set.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub source: Option<KillSource>,
}

/// Python's `int(round(x))`.
fn round_frame(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// The camera's turn summed from the first frame, by frame (degrees).
fn turned(frames: &[TrackFrame]) -> Vec<(f64, f64)> {
    let mut sum = Vec::with_capacity(frames.len().max(1));
    sum.push((0.0, 0.0));
    for frame in frames.iter().skip(1) {
        let (x, y) = *sum.last().unwrap();
        sum.push((x + frame.shift.0, y + frame.shift.1));
    }
    sum
}

/// Where a target last seen at `place` on frame `end` would be on frame `frame` (degrees from the crosshair): moved by
/// the camera's turn since, and by `speed` over the world (degrees a frame).
fn expected_place(place: (f64, f64), turned: &[(f64, f64)], end: i64, frame: i64, speed: (f64, f64)) -> (f64, f64) {
    let (now, then, frames) = (turned[frame as usize], turned[end as usize], (frame - end) as f64);
    (place.0 + now.0 - then.0 + speed.0 * frames, place.1 + now.1 - then.1 + speed.1 * frames)
}

/// A target's own speed (degrees a frame): over the world, with the camera's turn taken out, and on screen.
#[derive(Clone, Copy)]
struct OwnSpeed {
    /// Over the world: the camera's turn taken out.
    world: (f64, f64),
    /// On screen, as the places moved.
    screen: (f64, f64),
}

/// A target's speed where its places end, at frame `end`, over its last SPEED_FRAMES frames. Zero for both with fewer
/// than 2 places in those frames.
fn own_speed(places: &Places, end: i64, turned: &[(f64, f64)]) -> OwnSpeed {
    let mut window = places.range(end - SPEED_FRAMES..=end);
    let (Some((&from, &(x0, y0))), Some(_)) = (window.next(), window.next()) else {
        return OwnSpeed { world: (0.0, 0.0), screen: (0.0, 0.0) };
    };
    let (x1, y1) = places[&end];
    let frames = (end - from) as f64;
    let (then, now) = (turned[from as usize], turned[end as usize]);
    OwnSpeed {
        world: (((x1 - now.0) - (x0 - then.0)) / frames, ((y1 - now.1) - (y0 - then.1)) / frames),
        screen: ((x1 - x0) / frames, (y1 - y0) / frames),
    }
}

/// A track's places for its speed: a track of SPEED_FRAMES places or fewer with those of the tracks it continues
/// (`before`), back to more than SPEED_FRAMES; any other track's own.
fn places_for_speed<'a>(places: &'a HashMap<u32, Places>, before: &HashMap<u32, u32>, track: u32) -> Cow<'a, Places> {
    let own = &places[&track];
    if own.len() > SPEED_FRAMES as usize || !before.contains_key(&track) {
        return Cow::Borrowed(own);
    }
    let mut all = own.clone();
    let mut current = track;
    while let Some(&earlier) = before.get(&current) {
        if all.len() > SPEED_FRAMES as usize {
            break;
        }
        current = earlier;
        for (&frame, &place) in &places[&current] {
            all.entry(frame).or_insert(place);
        }
    }
    Cow::Owned(all)
}

/// A track and the tracks it continues (`before`), latest first.
fn chain(track: u32, before: &HashMap<u32, u32>) -> Vec<u32> {
    let mut chain = vec![track];
    while let Some(&earlier) = before.get(chain.last().unwrap()) {
        chain.push(earlier);
    }
    chain
}

/// Each track's places, blob areas and box sizes (degrees, in tracks that have them) by frame, with the tracks in the
/// order they first appear (tracks split off by `repair` come last).
struct TrackIndex {
    /// The tracks in the order they first appear; those `repair` splits off come last.
    ids: Vec<u32>,
    /// Each track's places by frame (degrees from the crosshair).
    places: HashMap<u32, Places>,
    /// Each track's blob areas by frame (pixels; 0 where the frame has none).
    areas: HashMap<u32, BTreeMap<i64, i64>>,
    /// Each track's box width and height by frame (degrees), for tracks whose frames have sizes.
    sizes: HashMap<u32, BTreeMap<i64, (f64, f64)>>,
}

impl TrackIndex {
    /// The index of a run's frames.
    fn new(frames: &[TrackFrame]) -> TrackIndex {
        let mut index =
            TrackIndex { ids: Vec::new(), places: HashMap::new(), areas: HashMap::new(), sizes: HashMap::new() };
        for frame in frames {
            let areas: Vec<i64> = if frame.a.is_empty() { vec![0; frame.t.len()] } else { frame.a.clone() };
            for (&(track, x, y), area) in frame.t.iter().zip(areas) {
                if !index.places.contains_key(&track) {
                    index.ids.push(track);
                }
                index.places.entry(track).or_default().insert(frame.i as i64, (x, y));
                index.areas.entry(track).or_default().insert(frame.i as i64, area);
            }
            for (track, _, _, size) in sized_boxes(frame) {
                index.sizes.entry(track).or_default().insert(frame.i as i64, size);
            }
        }
        index
    }

    /// Whether the crosshair is within RIM_DEG of the track's box on one of `frames`.
    fn box_reaches(&self, track: u32, frames: RangeInclusive<i64>) -> bool {
        let Some(sizes) = self.sizes.get(&track) else { return false };
        let places = &self.places[&track];
        sizes.range(frames).any(|(frame, &(width, height))| {
            let (x, y) = places[frame];
            x.abs() <= width / 2.0 + RIM_DEG && y.abs() <= height / 2.0 + RIM_DEG
        })
    }

    /// The first frame a track was seen on.
    fn first(&self, track: u32) -> i64 {
        *self.places[&track].first_key_value().unwrap().0
    }

    /// The last frame a track was seen on.
    fn last(&self, track: u32) -> i64 {
        *self.places[&track].last_key_value().unwrap().0
    }

    /// The frame a track was last seen on, and its place there.
    fn last_place(&self, track: u32) -> (i64, (f64, f64)) {
        let (&frame, &place) = self.places[&track].last_key_value().unwrap();
        (frame, place)
    }

    /// A chain's places (`chain`) by frame; on a frame two of its tracks share, the earlier track's.
    fn joined(&self, chain: &[u32]) -> Places {
        let mut places = Places::new();
        for track in chain {
            places.extend(self.places[track].iter().map(|(&frame, &place)| (frame, place)));
        }
        places
    }

    /// The track's blob areas near the crosshair (within WHOLE_TARGET_DEG), where the detector sees the target whole.
    fn near_areas(&self, track: u32) -> Vec<f64> {
        let places = &self.places[&track];
        self.areas[&track]
            .iter()
            .filter(|&(frame, &area)| area != 0 && hypot(places[frame].0, places[frame].1) < WHOLE_TARGET_DEG)
            .map(|(_, &area)| area as f64)
            .collect()
    }

    /// The run's typical target's blob area: the median of the tracks' median areas near the crosshair (None with fewer
    /// than MIN_TYPICAL_TRACKS).
    fn typical_area(&self) -> Option<f64> {
        let areas: Vec<f64> = self
            .ids
            .iter()
            .map(|&track| self.near_areas(track))
            .filter(|areas| !areas.is_empty())
            .map(|areas| median(&areas))
            .collect();
        (areas.len() >= MIN_TYPICAL_TRACKS).then(|| median(&areas))
    }
}

/// Tracks that are one target picked up again: appeared (each track with the frame its target first appeared on, in
/// the order the tracks start) and follows (a track to the track that continues it).
pub struct Appearances {
    /// Each track with the frame its target first appeared on (that of the first track in its chain), in the order
    /// the tracks start.
    pub appeared: Vec<(u32, i64)>,
    /// Each track that is continued, to the track that continues it.
    pub follows: HashMap<u32, u32>,
}

/// Where each track starts and ends (frame and place), the tracks in the order they start, and the tracks that end on
/// each frame.
struct Spans {
    /// Each track's first frame and its place there (degrees).
    first: HashMap<u32, (i64, f64, f64)>,
    /// Each track's last frame and its place there (degrees).
    last: HashMap<u32, (i64, f64, f64)>,
    /// The tracks in the order they start.
    starts: Vec<u32>,
    /// The tracks that end on each frame.
    ending: HashMap<i64, Vec<u32>>,
}

impl Spans {
    /// The spans of a run's tracks.
    fn new(frames: &[TrackFrame]) -> Spans {
        let (mut first, mut last) = (HashMap::new(), HashMap::new());
        let mut order = Vec::new();
        for frame in frames {
            for &(track, x, y) in &frame.t {
                first.entry(track).or_insert_with(|| {
                    order.push(track);
                    (frame.i as i64, x, y)
                });
                last.insert(track, (frame.i as i64, x, y));
            }
        }
        let mut ending: HashMap<i64, Vec<u32>> = HashMap::new();
        for &track in &order {
            ending.entry(last[&track].0).or_default().push(track);
        }
        let mut starts = order;
        starts.sort_by_key(|track| first[track].0);
        Spans { first, last, starts, ending }
    }

    /// The tracks a track may continue that end from frame `from` to OVERLAP_FRAMES after it starts, with the frame
    /// each ends on: not continued yet (`follows`), not the track itself, and started before it.
    fn may_continue<'a>(
        &'a self,
        track: u32,
        from: i64,
        follows: &'a HashMap<u32, u32>,
    ) -> impl Iterator<Item = (i64, u32)> + 'a {
        let start = self.first[&track].0;
        (from..=start + OVERLAP_FRAMES)
            .flat_map(move |end| {
                let tracks = self.ending.get(&end).map(Vec::as_slice).unwrap_or_default();
                tracks.iter().map(move |&earlier| (end, earlier))
            })
            .filter(move |&(_, earlier)| {
                !follows.contains_key(&earlier) && earlier != track && self.first[&earlier].0 < start
            })
    }
}

/// The first join of `appearances`: a track continues one that ended up to `gap_frames` before it starts, within
/// `radius` degrees of where that one would be now, its last place moved by the camera's turn since.
fn follows_by_camera(spans: &Spans, turned: &[(f64, f64)], gap_frames: i64, radius: f64) -> HashMap<u32, u32> {
    let mut follows = HashMap::new();
    for &track in &spans.starts {
        let (start, x, y) = spans.first[&track];
        let mut best: Option<(f64, u32)> = None;
        for (end, earlier) in spans.may_continue(track, (start - gap_frames).max(0), &follows) {
            let (_, last_x, last_y) = spans.last[&earlier];
            let expected = expected_place((last_x, last_y), turned, end, start, (0.0, 0.0));
            let distance = hypot(x - expected.0, y - expected.1);
            if distance < radius && best.is_none_or(|(best_distance, _)| distance < best_distance) {
                best = Some((distance, earlier));
            }
        }
        if let Some((_, earlier)) = best {
            follows.insert(earlier, track);
        }
    }
    follows
}

/// The second join of `appearances`: a track still left alone continues one that ended up to SPEED_FRAMES before it
/// starts, within `radius` degrees of where that one would be by its own speed (`own_speed`, with the tracks it
/// continues when it is short), over the world or on screen. Not on a crosshair spot. Returns the track each track
/// continues.
fn follows_by_own_speed(
    tracks: &Tracks,
    spans: &Spans,
    turned: &[(f64, f64)],
    radius: f64,
    follows: &mut HashMap<u32, u32>,
) -> HashMap<u32, u32> {
    let mut before: HashMap<u32, u32> = follows.iter().map(|(&earlier, &later)| (later, earlier)).collect();
    let spots = crosshair_spots(&tracks.frames);
    let on_spot = |x: f64, y: f64| spots.iter().any(|&(a, b)| hypot(x - a, y - b) < SPOT_CLEARANCE_DEG);
    let places = TrackIndex::new(&tracks.frames).places;
    for &track in &spans.starts {
        let (start, x, y) = spans.first[&track];
        if before.contains_key(&track) || on_spot(x, y) {
            continue;
        }
        let mut best: Option<(f64, u32)> = None;
        for (end, earlier) in spans.may_continue(track, (start - SPEED_FRAMES).max(0), follows) {
            let (_, last_x, last_y) = spans.last[&earlier];
            if on_spot(last_x, last_y) {
                continue;
            }
            let speed = own_speed(&places_for_speed(&places, &before, earlier), end, turned);
            let frames = (start - end) as f64;
            let by_world = expected_place((last_x, last_y), turned, end, start, speed.world);
            let on_screen = (last_x + speed.screen.0 * frames, last_y + speed.screen.1 * frames);
            for expected in [by_world, on_screen] {
                let distance = hypot(x - expected.0, y - expected.1);
                if distance < radius && best.is_none_or(|(best_distance, _)| distance < best_distance) {
                    best = Some((distance, earlier));
                }
            }
        }
        if let Some((_, earlier)) = best {
            follows.insert(earlier, track);
            before.insert(track, earlier);
        }
    }
    before
}

/// Tracks that are one target picked up again: a track that starts within `gap` seconds of another's end (or up to
/// OVERLAP_FRAMES before it), within `radius` degrees of where that one would be now, continues it
/// (`follows_by_camera`); then by the target's own speed (`follows_by_own_speed`): a target that moves on its own
/// (Bounce 180's spheres) fools the camera's turn and gets a new track every frame or two.
pub fn appearances(tracks: &Tracks, gap: f64, radius: f64) -> Appearances {
    let turned = turned(&tracks.frames);
    let spans = Spans::new(&tracks.frames);
    let gap_frames = round_frame(gap * tracks.fps).max(1);
    let mut follows = follows_by_camera(&spans, &turned, gap_frames, radius);
    let before = follows_by_own_speed(tracks, &spans, &turned, radius, &mut follows);
    let mut appeared_at: HashMap<u32, i64> = HashMap::new();
    let mut appeared = Vec::with_capacity(spans.starts.len());
    for &track in &spans.starts {
        let at = before.get(&track).map_or(spans.first[&track].0, |earlier| appeared_at[earlier]);
        appeared_at.insert(track, at);
        appeared.push((track, at));
    }
    Appearances { appeared, follows }
}

/// The most crosshair spots `crosshair_spots` finds.
pub const SPOTS: usize = 1;

/// A box within this many degrees of the crosshair's center (`crosshair_center`) is on the crosshair spot.
pub const ON_SPOT_DEG: f64 = 0.1;
/// The camera turns on a frame whose view shift is more than this many degrees: no target stays put on screen then.
const TURNING_DEG: f64 = 0.1;
/// The ring around the crosshair's center (inner and outer radius, degrees) whose boxes the spot's are weighed against.
const RING_DEG: (f64, f64) = (0.2, 0.4);
/// How many times as densely as the ring's the boxes on the spot lie when the detector marks the crosshair.
const SPOT_DENSITY: f64 = 5.0;
/// The fewest boxes on the spot when the detector marks the crosshair: a share of the turning frames, and a number.
/// This one is the share.
const SPOT_SHARE: f64 = 0.02;
/// The fewest boxes on the spot when the detector marks the crosshair, however few frames turn.
const SPOT_BOXES: f64 = 25.0;
/// A box has the size of the crosshair's box when its width and height are each within this share of it.
const SAME_SIZE: f64 = 0.2;

/// Where the detector's box on the crosshair sits (degrees): the screen's center, since the crosshair is always drawn
/// there. A box's center is in the pixels' own numbering (pixel i's center is at i: the labels the detector learned
/// from were blob centroids), so the center of a frame W pixels wide is at W / 2 - 0.5.
pub fn crosshair_center() -> (f64, f64) {
    to_deg(W as f64 / 2.0 - 0.5, H as f64 / 2.0 - 0.5)
}

/// Whether the camera turns on a frame (`TURNING_DEG`).
fn turning(frame: &TrackFrame) -> bool {
    hypot(frame.shift.0, frame.shift.1) > TURNING_DEG
}

/// The crosshair spot (degrees), if the detector marks the crosshair: its center (`crosshair_center`), when the boxes
/// within ON_SPOT_DEG of it on the frames where the camera turns are at least SPOT_SHARE of those frames (and
/// SPOT_BOXES), and lie at least SPOT_DENSITY times as densely as the boxes in the ring around it (`RING_DEG`): a
/// target held near the crosshair while the camera turns spreads over both.
pub fn crosshair_spots(frames: &[TrackFrame]) -> Capped<(f64, f64), SPOTS> {
    let center = crosshair_center();
    let (mut turning_frames, mut on_spot, mut in_ring) = (0usize, 0usize, 0usize);
    for frame in frames.iter().filter(|frame| turning(frame)) {
        turning_frames += 1;
        for &(_, x, y) in &frame.t {
            let distance = hypot(x - center.0, y - center.1);
            if distance < ON_SPOT_DEG {
                on_spot += 1;
            } else if (RING_DEG.0..RING_DEG.1).contains(&distance) {
                in_ring += 1;
            }
        }
    }
    let enough = on_spot as f64 >= (SPOT_SHARE * turning_frames as f64).max(SPOT_BOXES);
    let spot_area = ON_SPOT_DEG * ON_SPOT_DEG;
    let ring_area = RING_DEG.1 * RING_DEG.1 - RING_DEG.0 * RING_DEG.0;
    let dense = on_spot as f64 / spot_area >= SPOT_DENSITY * in_ring as f64 / ring_area;
    let mut spots = Capped::new();
    if enough && dense {
        spots.push(center);
    }
    spots
}

/// A box in a frame: its track, its place and its width and height (degrees).
type SizedBox = (u32, f64, f64, (f64, f64));

/// A frame's boxes with their sizes (none in tracks without sizes).
fn sized_boxes(frame: &TrackFrame) -> impl Iterator<Item = SizedBox> + '_ {
    frame.wh.iter().flat_map(|sizes| frame.t.iter().zip(sizes).map(|(&(track, x, y), &size)| (track, x, y, size)))
}

/// The crosshair spot and the size of the detector's box there (degrees): the median width and height of the boxes on
/// the spot while the camera turns (the crosshair never changes). None when the detector does not mark the crosshair.
fn crosshair_box(frames: &[TrackFrame]) -> Option<((f64, f64), (f64, f64))> {
    let &center = crosshair_spots(frames).first()?;
    let on_spot = |x: f64, y: f64| hypot(x - center.0, y - center.1) < ON_SPOT_DEG;
    let (mut widths, mut heights) = (Vec::new(), Vec::new());
    for frame in frames.iter().filter(|frame| turning(frame)) {
        for (_, _, _, (width, height)) in sized_boxes(frame).filter(|&(_, x, y, _)| on_spot(x, y)) {
            widths.push(width);
            heights.push(height);
        }
    }
    (!widths.is_empty()).then(|| (center, (median(&widths), median(&heights))))
}

/// A box at the crosshair is far smaller than the run's targets when its width and height are each at most their
/// median's divided by this: too small to be one of them hidden under the crosshair.
const TARGET_OVER_CROSSHAIR: f64 = 2.0;
/// The fewest such boxes at the crosshair that say the detector marks it.
const CROSSHAIR_BOXES: usize = 10;
/// The detector's box on the crosshair sits within this of the crosshair's center (degrees): its center wanders by a
/// pixel or two (0.15 degrees on ww3t Voltaic 203).
const CROSSHAIR_REACH_DEG: f64 = 0.25;

/// The size of the detector's box on the crosshair (degrees), from the boxes at the crosshair's center (ON_SPOT_DEG)
/// far smaller than the run's targets (TARGET_OVER_CROSSHAIR; the targets' size is the median of the boxes elsewhere):
/// their median width and height. None when there are fewer than CROSSHAIR_BOXES of them.
fn small_crosshair_box(frames: &[TrackFrame], center: (f64, f64)) -> Option<(f64, f64)> {
    let on_spot = |x: f64, y: f64| hypot(x - center.0, y - center.1) < ON_SPOT_DEG;
    let (mut widths, mut heights) = (Vec::new(), Vec::new());
    for (_, _, _, (width, height)) in frames.iter().flat_map(sized_boxes).filter(|&(_, x, y, _)| !on_spot(x, y)) {
        widths.push(width);
        heights.push(height);
    }
    if widths.is_empty() {
        return None;
    }
    let most = (median(&widths) / TARGET_OVER_CROSSHAIR, median(&heights) / TARGET_OVER_CROSSHAIR);
    let (mut small_widths, mut small_heights) = (Vec::new(), Vec::new());
    for (_, _, _, (width, height)) in frames.iter().flat_map(sized_boxes).filter(|&(_, x, y, _)| on_spot(x, y)) {
        if width <= most.0 && height <= most.1 {
            small_widths.push(width);
            small_heights.push(height);
        }
    }
    (small_widths.len() >= CROSSHAIR_BOXES).then(|| (median(&small_widths), median(&small_heights)))
}

/// The tracks without the crosshair's boxes, in a run whose targets are far bigger than it (`small_crosshair_box`):
/// there a box of the crosshair's size (SAME_SIZE) at the crosshair's center cannot be a target hidden under the
/// crosshair, so it is the crosshair, with the camera turning or still (the user's rule, 2026-10-05: the size gap is
/// enough when it is big). A track that runs through such boxes is split there: a killed target's track that the
/// tracker handed to the crosshair's box ends at its last box, and what comes after the boxes is a track of its own. A
/// run whose targets are near the crosshair's size keeps every box, for `without_crosshair_ends` to judge.
pub fn without_crosshair_boxes(tracks: &Tracks) -> Cow<'_, Tracks> {
    let frames = &tracks.frames;
    let center = crosshair_center();
    let Some(crosshair) = small_crosshair_box(frames, center) else { return Cow::Borrowed(tracks) };
    let on_spot = |x: f64, y: f64| hypot(x - center.0, y - center.1) < CROSSHAIR_REACH_DEG;
    let near_size = |size: f64, crosshair_size: f64| (size - crosshair_size).abs() <= SAME_SIZE * crosshair_size;
    let is_crosshair = |&(_, x, y): &TrackPoint, &(width, height): &(f64, f64)| {
        on_spot(x, y) && near_size(width, crosshair.0) && near_size(height, crosshair.1)
    };
    let mut out = tracks.clone();
    let mut next_id = frames.iter().flat_map(|frame| frame.t.iter().map(|point| point.0)).max().map_or(0, |id| id + 1);
    // each split track's id from its last split on, and the tracks whose latest point was a crosshair box
    let (mut renamed, mut broken): (HashMap<u32, u32>, HashSet<u32>) = (HashMap::new(), HashSet::new());
    for frame in &mut out.frames {
        let Some(sizes) = frame.wh.as_mut() else { continue };
        let keeps: Vec<bool> =
            frame.t.iter().zip(sizes.iter()).map(|(point, size)| !is_crosshair(point, size)).collect();
        if keeps.contains(&false) {
            keep_marked(sizes, &keeps);
        }
        for (point, &keep) in frame.t.iter_mut().zip(&keeps) {
            if !keep {
                broken.insert(point.0);
            } else if broken.remove(&point.0) {
                renamed.insert(point.0, next_id);
                next_id += 1;
            }
            if let Some(&id) = renamed.get(&point.0) {
                point.0 = id;
            }
        }
        if keeps.contains(&false) {
            keep_marked(&mut frame.t, &keeps);
            keep_marked(&mut frame.a, &keeps);
            if let Some(scores) = frame.s.as_mut() {
                keep_marked(scores, &keeps);
            }
        }
    }
    if broken.is_empty() && renamed.is_empty() {
        return Cow::Borrowed(tracks);
    }
    reshift(&mut out.frames, frames);
    Cow::Owned(out)
}

/// The view's shift worked out again (`track::view_shift_between`) on each frame that had a crosshair box left out, or
/// whose frame before had one: the tracker paired the crosshair's box, which stays put on screen, with the boxes around
/// it, and could find a turn that was not there (ww3t Voltaic 203: 2.8 degrees one way at a flick, from the crosshair's
/// box and a target that had just appeared). No pairing left: no turn.
fn reshift(kept: &mut [TrackFrame], original: &[TrackFrame]) {
    let places = |frame: &TrackFrame| -> Vec<(f64, f64)> { frame.t.iter().map(|&(_, x, y)| (x, y)).collect() };
    let lost: Vec<bool> = kept.iter().zip(original).map(|(frame, before)| frame.t.len() != before.t.len()).collect();
    for i in 1..kept.len() {
        if lost[i] || lost[i - 1] {
            kept[i].shift = view_shift_between(&places(&kept[i - 1]), &places(&kept[i])).unwrap_or((0.0, 0.0));
        }
    }
}

/// The tracks without the end of each one that turns into the crosshair's box: where the detector marks the crosshair,
/// the tracker hands it the killed target's track, which runs on until the camera turns (the box stays put on screen).
/// A box is the crosshair's on the spot (`crosshair_spots`, within ON_SPOT_DEG) with the size of the crosshair's box
/// (SAME_SIZE of the median width and height of the boxes on the spot while the camera turns: the crosshair never
/// changes). A track whose last boxes are the crosshair's, after a box of another size, and that ends as the camera
/// turns, ends at that box. One that ends with the camera still keeps its end: the crosshair's box went with the
/// target (a detector can mark the crosshair only over a target). So does a target as big as the crosshair's box,
/// which cannot be told from it.
pub fn without_crosshair_ends(tracks: &Tracks) -> Cow<'_, Tracks> {
    let frames = &tracks.frames;
    let Some((center, crosshair)) = crosshair_box(frames) else { return Cow::Borrowed(tracks) };
    let on_spot = |x: f64, y: f64| hypot(x - center.0, y - center.1) < ON_SPOT_DEG;
    let near_size = |size: f64, crosshair_size: f64| (size - crosshair_size).abs() <= SAME_SIZE * crosshair_size;
    let crosshair_sized = |(width, height): (f64, f64)| near_size(width, crosshair.0) && near_size(height, crosshair.1);
    // each track's last box that is not the crosshair's (its frame, and whether it has the crosshair's size), and end
    let mut last_other: HashMap<u32, (usize, bool)> = HashMap::new();
    let mut end_frame: HashMap<u32, usize> = HashMap::new();
    for frame in frames {
        for (track, x, y, size) in sized_boxes(frame) {
            if !on_spot(x, y) || !crosshair_sized(size) {
                last_other.insert(track, (frame.i, crosshair_sized(size)));
            }
            end_frame.insert(track, frame.i);
        }
    }
    let cut_after: HashMap<u32, usize> = last_other
        .into_iter()
        .filter(|&(track, (_, sized))| !sized && frames.get(end_frame[&track] + 1).is_some_and(turning))
        .map(|(track, (frame, _))| (track, frame))
        .collect();
    if cut_after.is_empty() {
        return Cow::Borrowed(tracks);
    }
    Cow::Owned(keep_points(tracks, |frame, track| cut_after.get(&track).is_none_or(|&last| frame <= last)))
}

/// The tracks that end at the crosshair (kills seen in the video), by their last frame: with MIN_KILL_TRACK_POINTS
/// places or more, and ending more than END_MARGIN_FRAMES before the run's last frame.
fn crosshair_ends(index: &TrackIndex, frame_count: i64) -> Vec<(i64, u32)> {
    let mut ends: Vec<(i64, u32)> = index
        .ids
        .iter()
        .filter_map(|&track| {
            let (frame, (x, y)) = index.last_place(track);
            let long = index.places[&track].len() >= MIN_KILL_TRACK_POINTS;
            let at_crosshair = hypot(x, y) < AT_CROSSHAIR_DEG;
            (frame < frame_count - END_MARGIN_FRAMES && at_crosshair && long).then_some((frame, track))
        })
        .collect();
    ends.sort();
    ends
}

/// The tracks that never leave a crosshair spot.
fn spot_tracks(index: &TrackIndex, spots: &[(f64, f64)]) -> HashSet<u32> {
    if spots.is_empty() {
        return HashSet::new();
    }
    let nearest_spot = |x: f64, y: f64| spots.iter().map(|&(a, b)| hypot(x - a, y - b)).fold(f64::INFINITY, f64::min);
    index
        .ids
        .iter()
        .copied()
        .filter(|track| index.places[track].values().all(|&(x, y)| nearest_spot(x, y) < ON_SPOT_DEG))
        .collect()
}

/// Whether a kill is confirmed: its target was last seen within AT_CROSSHAIR_DEG of the crosshair, within
/// CONFIRM_FRAMES of its kill time.
fn confirmed(flick: &Flick) -> bool {
    flick.path.last().is_some_and(|&(frame, x, y)| {
        (frame - flick.stats_frame.unwrap_or(frame)).abs() <= CONFIRM_FRAMES && hypot(x, y) < AT_CROSSHAIR_DEG
    })
}

/// The flicks for known kill times (seconds) with the shots each kill took: from the stats file, or from the session
/// HUD (already on the video's clock: `offset` Some(0)).
///
/// 1. The video's clock against the kill times' clock: one constant offset, voted from tracks that end at the
///    crosshair (unless `offset` is given).
/// 2. For each kill, the killed target is the track nearest the crosshair in the last `window` seconds before it.
/// 3. The flick runs from the previous kill, or from the target's first appearance when it appeared after that kill,
///    to this kill.
pub fn match_times(
    tracks: &Tracks,
    kill_times: &[f64],
    shots: &[i64],
    window: f64,
    offset: Option<f64>,
) -> (Vec<Flick>, MatchInfo) {
    let without_boxes = without_crosshair_boxes(tracks);
    let tracks = &*without_crosshair_ends(&without_boxes);
    let fps = tracks.fps;
    let index = TrackIndex::new(&tracks.frames);
    let ends = crosshair_ends(&index, tracks.frames.len() as i64);
    let video_times: Vec<f64> = ends.iter().map(|&(frame, _)| frame as f64 / fps).collect();
    let info = |matched: usize, confirmed: Option<usize>, offset: Option<f64>| MatchInfo {
        kills_video: ends.len(),
        kills_stats: Some(kill_times.len()),
        matched,
        confirmed,
        offset,
        fps,
        source: None,
    };
    if kill_times.is_empty() || (offset.is_none() && video_times.is_empty()) {
        return (Vec::new(), info(0, None, None));
    }
    let joins = appearances(tracks, JOIN_GAP_S, JOIN_RADIUS_DEG).follows;
    let before: HashMap<u32, u32> = joins.into_iter().map(|(earlier, later)| (later, earlier)).collect();
    let on_spot = spot_tracks(&index, &crosshair_spots(&tracks.frames));
    let offset = offset.unwrap_or_else(|| clock_offset(kill_times, &video_times, fps));
    let kills = Kills { index: &index, before: &before, on_spot: &on_spot, fps, offset, window };
    let flicks = kills.attach(kill_times, shots);
    let confirmed_kills = flicks.iter().filter(|flick| confirmed(flick)).count();
    let info = info(flicks.len(), Some(confirmed_kills), Some(offset));
    (flicks, info)
}

/// The kill times' offset on the video's clock: the one most kills line up with (tracks ending at the crosshair within
/// CLOCK_TOLERANCE_FRAMES), refined by the median of how far each that lines up is off.
fn clock_offset(kill_times: &[f64], video_times: &[f64], fps: f64) -> f64 {
    let tolerance = CLOCK_TOLERANCE_FRAMES / fps;
    // video_times is sorted: the nearest is one of the two either side of the time, the earlier on a tie, as a scan
    // from the start finds it
    let nearest = |time: f64| {
        let at = video_times.partition_point(|&video| video < time);
        [at.wrapping_sub(1), at].into_iter().filter_map(|i| video_times.get(i)).fold(
            (f64::INFINITY, 0.0),
            |best, &video| if (time - video).abs() < best.0 { ((time - video).abs(), video) } else { best },
        )
    };
    let mut best: Option<(usize, f64)> = None;
    for &video in video_times.iter().take(CLOCK_VOTES) {
        for &kill in kill_times.iter().take(CLOCK_VOTES) {
            let offset = video - kill;
            let lined_up = kill_times.iter().filter(|&&time| nearest(time + offset).0 < tolerance).count();
            if best.is_none_or(|(best_lined_up, _)| lined_up > best_lined_up) {
                best = Some((lined_up, offset));
            }
        }
    }
    let offset = best.unwrap().1;
    let misses: Vec<f64> = kill_times
        .iter()
        .map(|&time| nearest(time + offset).1 - (time + offset))
        .filter(|miss| miss.abs() < tolerance)
        .collect();
    if misses.is_empty() { offset } else { offset + crate::python::numpy_median(&misses) }
}

/// What steps 2 and 3 of `match_times` work from: the tracks, the tracks each continues, the tracks on a crosshair
/// spot, the frame rate, the kill times' offset and the window before a kill (seconds).
struct Kills<'a> {
    /// The tracks, indexed.
    index: &'a TrackIndex,
    /// Each track that continues another, to the track it continues.
    before: &'a HashMap<u32, u32>,
    /// The tracks that never leave a crosshair spot.
    on_spot: &'a HashSet<u32>,
    /// The video's frame rate (frames a second).
    fps: f64,
    /// What to add to a kill time to put it on the video's clock (seconds).
    offset: f64,
    /// How long before a kill its target is looked for (seconds).
    window: f64,
}

impl Kills<'_> {
    /// Each kill's target and flick. A target lost for a few frames and picked up again as a new track keeps its
    /// earlier tracks (`before`).
    fn attach(&self, kill_times: &[f64], shots: &[i64]) -> Vec<Flick> {
        let window = round_frame(self.window * self.fps).max(1);
        let mut flicks = Vec::with_capacity(kill_times.len().min(shots.len()));
        let mut previous: Option<i64> = None;
        let mut used: HashSet<u32> = HashSet::new();
        // review.py reads `last` after its loop over the tracks: the last track looked at, not the one chosen, and it
        // carries over from kill to kill
        let mut last = 0;
        for (kill, (&time, &kill_shots)) in kill_times.iter().zip(shots).enumerate() {
            let kill_frame = round_frame((time + self.offset) * self.fps);
            let mut target = self.nearest_track(kill_frame, window, &mut last);
            if target.is_none()
                && let Some((end, track)) = self.hidden_track(kill_frame, window, previous, &used)
            {
                (last, target) = (end, Some(track));
            }
            if let Some(track) = target {
                used.insert(track);
                flicks.push(self.flick(kill + 1, track, kill_frame, kill_shots, previous, last));
            }
            previous = Some(kill_frame);
        }
        flicks
    }

    /// The track nearest the crosshair and the kill: seen from `window` frames before the kill to AFTER_KILL_FRAMES
    /// after, at its frame nearest both, within MAX_KILL_DISTANCE_DEG (or its blob's radius and RIM_DEG, or with the
    /// crosshair within RIM_DEG of its box on a frame seen), with costs for being early and for never leaving a
    /// crosshair spot. `last` is set to the frame picked for each track looked at.
    fn nearest_track(&self, kill_frame: i64, window: i64, last: &mut i64) -> Option<u32> {
        let (index, fps) = (self.index, self.fps);
        let mut best: Option<(u32, f64)> = None;
        let around_kill = kill_frame - window..=kill_frame + AFTER_KILL_FRAMES;
        for &track in &index.ids {
            let places = &index.places[&track];
            let mut seen = places.range(around_kill.clone()).map(|(&frame, _)| frame);
            let Some(first_seen) = seen.next() else { continue };
            // its frame nearest the crosshair and the kill: a track can go on past the kill and move off
            let score = |frame: i64| {
                hypot(places[&frame].0, places[&frame].1) + DEG_PER_SECOND_OFF * (frame - kill_frame).abs() as f64 / fps
            };
            let nearest = seen.fold((first_seen, score(first_seen)), |nearest, frame| {
                let frame_score = score(frame);
                if frame_score < nearest.1 { (frame, frame_score) } else { nearest }
            });
            *last = nearest.0;
            let distance = hypot(places[last].0, places[last].1);
            let radius = blob_radius_deg(index.areas[&track][last] as f64);
            let reached = distance <= MAX_KILL_DISTANCE_DEG.max(radius + RIM_DEG);
            if !reached && !index.box_reaches(track, around_kill.clone()) {
                continue;
            }
            let cost = distance
                + DEG_PER_SECOND_OFF * ((*last).min(kill_frame) - kill_frame).abs() as f64 / fps
                + if *last >= kill_frame - EARLY_FRAMES { 0.0 } else { EARLY_COST_DEG }
                + if self.on_spot.contains(&track) { SPOT_COST_DEG } else { 0.0 };
            if best.is_none_or(|(_, best_cost)| cost < best_cost) {
                best = Some((track, cost));
            }
        }
        best.map(|(track, _)| track)
    }

    /// A target held hidden under the crosshair longer than the window: the latest track that ended at the crosshair
    /// since the previous kill, up to HIDDEN_WINDOWS windows back, not taken by a kill; its last frame and the track.
    fn hidden_track(
        &self,
        kill_frame: i64,
        window: i64,
        previous: Option<i64>,
        used: &HashSet<u32>,
    ) -> Option<(i64, u32)> {
        let floor = previous.unwrap_or(-1).max(kill_frame - HIDDEN_WINDOWS * window);
        self.index
            .ids
            .iter()
            .filter(|track| !used.contains(track))
            .map(|&track| (self.index.last_place(track), track))
            .filter(|&((end, (x, y)), _)| floor < end && end < kill_frame - window && hypot(x, y) < AT_CROSSHAIR_DEG)
            .map(|((end, _), track)| (end, track))
            .max()
    }

    /// The flick of kill `kill_number` to `track`'s target: its path from the previous kill (or from the target's first
    /// appearance, when it appeared later) to the place it was last seen near the kill (up to `last`, or close).
    fn flick(
        &self,
        kill_number: usize,
        track: u32,
        kill_frame: i64,
        shots: i64,
        previous: Option<i64>,
        last: i64,
    ) -> Flick {
        let chain = chain(track, self.before);
        let places = self.index.joined(&chain);
        let first = *places.first_key_value().unwrap().0;
        let mut start = previous.unwrap_or_else(|| round_frame(self.offset * self.fps));
        let spawned = first > start + SPAWN_FRAMES;
        if spawned {
            start = first;
        }
        let near_the_kill = |&(&frame, &(x, y)): &(&i64, &(f64, f64))| {
            frame <= kill_frame + AFTER_KILL_FRAMES && (frame <= last || hypot(x, y) <= MAX_KILL_DISTANCE_DEG)
        };
        let end = *places.iter().rev().find(near_the_kill).unwrap().0;
        // the whole chain's areas: the last track may be half under the crosshair
        let areas: Vec<f64> =
            chain.iter().flat_map(|earlier| self.index.areas[earlier].values().map(|&area| area as f64)).collect();
        Flick {
            kill_number,
            kill_frame: end,
            stats_frame: Some(kill_frame),
            start_frame: start,
            shots: Some(shots),
            path: places
                .iter()
                .filter(|&(&frame, _)| start <= frame && frame <= end)
                .map(|(&frame, &(x, y))| (frame, x, y))
                .collect(),
            spawned,
            area_px: areas.iter().any(|&area| area != 0.0).then(|| median(&areas)),
        }
    }
}

/// Tracks that are the crosshair, not a target: they never leave `max_distance` degrees of the crosshair, the camera
/// turned more than `min_turn` degrees meanwhile, and they did not move with it (a static target shifts by the camera's
/// turn; more than `unexplained_share` of the turn left unexplained). A detector can take some crosshairs (Aim Lab's)
/// for a target. Also tracks of SHORT_SPOT_TRACK_POINTS or fewer on a crosshair spot (`crosshair_spots`): right after
/// a kill they made the dead target look picked up again, so its kill was lost or came late.
pub fn ghosts(frames: &[TrackFrame], max_distance: f64, unexplained_share: f64, min_turn: f64) -> HashSet<u32> {
    let turned = turned(frames);
    let index = TrackIndex::new(frames);
    let mut ghosts = HashSet::new();
    for &track in &index.ids {
        let places = &index.places[&track];
        if places.len() < 2 || places.values().any(|&(x, y)| hypot(x, y) >= max_distance) {
            continue;
        }
        let (mut turn, mut unexplained) = (0.0, 0.0);
        for ((&from, &(from_x, from_y)), (&to, &(to_x, to_y))) in places.iter().zip(places.iter().skip(1)) {
            let (now, then) = (turned[to as usize], turned[from as usize]);
            let step = (now.0 - then.0, now.1 - then.1);
            turn += hypot(step.0, step.1);
            unexplained += hypot(to_x - from_x - step.0, to_y - from_y - step.1);
        }
        if turn > min_turn && unexplained > unexplained_share * turn {
            ghosts.insert(track);
        }
    }
    let spots = crosshair_spots(frames);
    if !spots.is_empty() {
        let on_spot = |&(x, y): &(f64, f64)| spots.iter().any(|&(a, b)| hypot(x - a, y - b) < ON_SPOT_DEG);
        for &track in &index.ids {
            let places = &index.places[&track];
            if places.len() <= SHORT_SPOT_TRACK_POINTS && places.values().all(on_spot) {
                ghosts.insert(track);
            }
        }
    }
    ghosts
}

/// The tracks without those that are the crosshair (`ghosts`).
pub fn without_ghosts(tracks: &Tracks) -> Tracks {
    let gone = ghosts(&tracks.frames, GHOST_MAX_DEG, GHOST_UNEXPLAINED_SHARE, GHOST_MIN_TURN_DEG);
    if gone.is_empty() {
        return tracks.clone();
    }
    keep_points(tracks, |_, track| !gone.contains(&track))
}

/// The tracks with only the points `keep` (frame, track) keeps.
fn keep_points(tracks: &Tracks, keep: impl Fn(usize, u32) -> bool) -> Tracks {
    let mut kept = tracks.clone();
    for frame in &mut kept.frames {
        let keeps: Vec<bool> = frame.t.iter().map(|point| keep(frame.i, point.0)).collect();
        if !keeps.contains(&false) {
            continue;
        }
        keep_marked(&mut frame.t, &keeps);
        keep_marked(&mut frame.a, &keeps);
        if let Some(sizes) = frame.wh.as_mut() {
            keep_marked(sizes, &keeps);
        }
        if let Some(scores) = frame.s.as_mut() {
            keep_marked(scores, &keeps);
        }
    }
    kept
}

/// A frame's values for its targets, with only those `keeps` marks (a list of another length is left as it is).
fn keep_marked<T>(values: &mut Vec<T>, keeps: &[bool]) {
    if values.len() == keeps.len() {
        let mut marks = keeps.iter();
        values.retain(|_| *marks.next().unwrap());
    }
}

/// Where the rest of a track goes when `repair` splits it: on with a track that ended the frame before, or a new one.
enum Heir {
    /// The track (its id) that ended the frame before, near where the rest goes on.
    Ended(u32),
    /// A new track.
    New,
}

impl TrackIndex {
    /// A kill can fool the camera's turn on a plain wall: with the target at the crosshair gone, the frame's shift
    /// lines up another target with the dead one's place, and the tracker hands the dead target's track on to that
    /// target (its place on screen jumps with the shift) and starts new tracks for the targets that stayed put. A
    /// track at the crosshair whose place jumps more than `near` with the frame's shift is split there (`false_turn`).
    /// Its head ends where the target died; its rest continues that track, else becomes a track of its own. Returns
    /// the camera's turn summed from the first frame, by frame, with each spike replaced by the mean of the frames
    /// either side.
    fn repair(&mut self, frames: &[TrackFrame], near: f64) -> Vec<(f64, f64)> {
        let turned = turned(frames);
        let spike = spikes(&frames.iter().map(|frame| frame.shift).collect::<Vec<_>>());
        let (mut seen_at, mut ends) = self.seen_and_ends(frames.len());
        let mut next_id = self.ids.iter().max().map_or(0, |id| id + 1);
        for frame in 1..frames.len() {
            let shift = (turned[frame].0 - turned[frame - 1].0, turned[frame].1 - turned[frame - 1].1);
            let mut on_both: Vec<u32> =
                seen_at[frame - 1].iter().copied().filter(|track| seen_at[frame].contains(track)).collect();
            on_both.sort_unstable();
            let frame = frame as i64;
            for track in on_both {
                let Some(heir) = self.false_turn(track, frame, shift, spike[frame as usize], near, &ends) else {
                    continue;
                };
                let heir = match heir {
                    Heir::Ended(ended) => {
                        ends.get_mut(&(frame - 1)).unwrap().remove(&ended);
                        ended
                    }
                    Heir::New => {
                        self.ids.push(next_id);
                        next_id += 1;
                        next_id - 1
                    }
                };
                self.hand_on(track, frame, heir, &mut seen_at, &mut ends);
            }
        }
        if !spike.contains(&true) {
            return turned;
        }
        without_spikes(&turned, &spike)
    }

    /// The tracks seen on each frame, and those that end on each frame.
    fn seen_and_ends(&self, frame_count: usize) -> (Vec<Vec<u32>>, HashMap<i64, HashSet<u32>>) {
        let mut seen_at: Vec<Vec<u32>> = vec![Vec::new(); frame_count];
        let mut ends: HashMap<i64, HashSet<u32>> = HashMap::new();
        for &track in &self.ids {
            for &frame in self.places[&track].keys() {
                seen_at[frame as usize].push(track);
            }
            ends.entry(self.last(track)).or_default().insert(track);
        }
        (seen_at, ends)
    }

    /// Whether `track`'s target died at the crosshair on the frame before `frame`, and the tracker handed its track on
    /// to another target there: its place jumps more than `near` with the frame's `shift`, and the shift is a spike
    /// (`track::spikes`: since review version 3 the tracker repairs most, `link`), or the jump is more than
    /// LONG_JUMP_SHARE times `near` and lands within `near` of a track that ended the frame before. Where its rest
    /// goes; None when it is not split.
    fn false_turn(
        &self,
        track: u32,
        frame: i64,
        shift: (f64, f64),
        spike: bool,
        near: f64,
        ends: &HashMap<i64, HashSet<u32>>,
    ) -> Option<Heir> {
        let places = &self.places[&track];
        let (Some(&(x0, y0)), Some(&(x1, y1))) = (places.get(&(frame - 1)), places.get(&frame)) else { return None };
        let jump = hypot(x1 - x0, y1 - y0);
        if hypot(x0, y0) >= near || jump <= near || hypot(x1 - x0 - shift.0, y1 - y0 - shift.1) >= near {
            return None;
        }
        let mut ended: Vec<(f64, u32)> = ends
            .get(&(frame - 1))
            .into_iter()
            .flatten()
            .filter(|&&other| other != track && self.last(other) == frame - 1)
            .map(|&other| {
                let (other_x, other_y) = self.places[&other][&(frame - 1)];
                (hypot(x1 - other_x, y1 - other_y), other)
            })
            .filter(|&(distance, _)| distance < near)
            .collect();
        ended.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if !spike && (ended.is_empty() || jump <= LONG_JUMP_SHARE * near) {
            return None;
        }
        Some(ended.first().map_or(Heir::New, |&(_, other)| Heir::Ended(other)))
    }

    /// Moves `track`'s places from `frame` on to `heir`, keeping `seen_at` and `ends` up to date.
    fn hand_on(
        &mut self,
        track: u32,
        frame: i64,
        heir: u32,
        seen_at: &mut [Vec<u32>],
        ends: &mut HashMap<i64, HashSet<u32>>,
    ) {
        let rest = self.places.get_mut(&track).unwrap().split_off(&frame);
        let rest_areas = self.areas.get_mut(&track).unwrap().split_off(&frame);
        let old_end = *rest.last_key_value().unwrap().0;
        for &moved in rest.keys() {
            let here = &mut seen_at[moved as usize];
            here.retain(|&seen| seen != track);
            here.push(heir);
        }
        self.places.entry(heir).or_default().extend(rest);
        self.areas.entry(heir).or_default().extend(rest_areas);
        ends.entry(self.last(heir)).or_default().insert(heir);
        ends.get_mut(&old_end).unwrap().remove(&track);
        ends.entry(frame - 1).or_default().insert(track);
    }

    /// Tracks that are one target picked up again (`appearances`, for the video alone): a track that starts within
    /// JOIN_GAP_S of another's end (or up to OVERLAP_FRAMES before it), within JOIN_RADIUS_DEG of where that one would
    /// be now (its last place moved by the camera's turn since, and by its own speed over its last frames or not:
    /// either will do), continues it, unless the target was gone meanwhile. A target hidden under the crosshair stays
    /// there: one whose place came out from under it (`OUT_FROM_UNDER_SHARE` x `near`) on SEEN_FRAMES or more of the
    /// frames it was missing would have been seen, so it died. And a target that comes back is where it was and as
    /// big: one found again LONG_GAP_FRAMES or more after it was last seen more than LONG_GAP_RADII of its radius
    /// `radius` from that place (SHORT_GAP_FRAMES: SHORT_GAP_RADII), or not as big (`same_size`), is a new target, such
    /// as one that spawned near the dead one. Returns the track that continues each track.
    fn continued(&self, turned: &[(f64, f64)], fps: f64, radius: f64, near: f64) -> HashMap<u32, u32> {
        let gap_frames = round_frame(JOIN_GAP_S * fps).max(1);
        let mut ending: HashMap<i64, Vec<u32>> = HashMap::new();
        for &track in &self.ids {
            ending.entry(self.last(track)).or_default().push(track);
        }
        let mut starts = self.ids.clone();
        starts.sort_by_key(|&track| self.first(track));
        let mut follows: HashMap<u32, u32> = HashMap::new();
        let mut before: HashMap<u32, u32> = HashMap::new();
        for track in starts {
            let start = self.first(track);
            let place = self.places[&track][&start];
            // whether `earlier`'s target, gone after `end` and moving at `speed`, can be this one back
            let came_back = |earlier: u32, end: i64, speed: (f64, f64), distance: f64| {
                let gone_from = self.places[&earlier][&end];
                let out_from_under = (end + 1..start)
                    .filter(|&frame| {
                        let (x, y) = expected_place(gone_from, turned, end, frame, speed);
                        hypot(x, y) > OUT_FROM_UNDER_SHARE * near
                    })
                    .count();
                let same_place = match start - end {
                    LONG_GAP_FRAMES.. => distance <= LONG_GAP_RADII * radius && self.same_size(earlier, track),
                    SHORT_GAP_FRAMES => distance <= SHORT_GAP_RADII * radius,
                    _ => true,
                };
                out_from_under < SEEN_FRAMES && same_place
            };
            let mut best: Option<(f64, u32)> = None;
            for end in (start - gap_frames).max(0)..=start + OVERLAP_FRAMES {
                for &earlier in ending.get(&end).map(Vec::as_slice).unwrap_or_default() {
                    if follows.contains_key(&earlier) || earlier == track || self.first(earlier) >= start {
                        continue;
                    }
                    // where it would be if it moved on as it did, and if it stood still: either will do
                    for speed in [self.speed(earlier, &before, turned, end), (0.0, 0.0)] {
                        let expected = expected_place(self.places[&earlier][&end], turned, end, start, speed);
                        let distance = hypot(place.0 - expected.0, place.1 - expected.1);
                        let nearer = best.is_none_or(|(best_distance, _)| distance < best_distance);
                        if distance < JOIN_RADIUS_DEG && nearer && came_back(earlier, end, speed, distance) {
                            best = Some((distance, earlier));
                        }
                    }
                }
            }
            if let Some((_, earlier)) = best {
                follows.insert(earlier, track);
                before.insert(track, earlier);
            }
        }
        follows
    }

    /// Whether a target came back about as big as it was: the new track's median blob area over its first SIZE_FRAMES
    /// frames is within SAME_SIZE_SHARES of the old track's over its last (true when either has no area).
    fn same_size(&self, old: u32, new: u32) -> bool {
        let old_areas: Capped<f64, SIZE_FRAMES> =
            self.areas[&old].values().rev().take(SIZE_FRAMES).map(|&area| area as f64).collect();
        let new_areas: Capped<f64, SIZE_FRAMES> =
            self.areas[&new].values().take(SIZE_FRAMES).map(|&area| area as f64).collect();
        let (old_area, new_area) = (median(&old_areas), median(&new_areas));
        old_area <= 0.0 || new_area <= 0.0 || (SAME_SIZE_SHARES.0..=SAME_SIZE_SHARES.1).contains(&(new_area / old_area))
    }

    /// A target's speed over the world (degrees a frame) where its track ends, at frame `end` (`own_speed`, with the
    /// tracks it continues when it is short).
    fn speed(&self, track: u32, before: &HashMap<u32, u32>, turned: &[(f64, f64)], end: i64) -> (f64, f64) {
        own_speed(&places_for_speed(&self.places, before, track), end, turned).world
    }
}

/// The camera's turn summed from the first frame, with each spike's step replaced by the mean of the steps either
/// side.
fn without_spikes(turned: &[(f64, f64)], spike: &[bool]) -> Vec<(f64, f64)> {
    let count = turned.len();
    let mut steps: Vec<(f64, f64)> = (0..count)
        .map(|frame| {
            if frame == 0 {
                (0.0, 0.0)
            } else {
                (turned[frame].0 - turned[frame - 1].0, turned[frame].1 - turned[frame - 1].1)
            }
        })
        .collect();
    for frame in (0..count).filter(|&frame| spike[frame]) {
        steps[frame] =
            ((steps[frame - 1].0 + steps[frame + 1].0) / 2.0, (steps[frame - 1].1 + steps[frame + 1].1) / 2.0);
    }
    let mut sum = steps[0];
    let mut summed = Vec::with_capacity(count);
    summed.push(sum);
    for &(step_x, step_y) in &steps[1..] {
        sum = (sum.0 + step_x, sum.1 + step_y);
        summed.push(sum);
    }
    summed
}

/// A track that may be a video kill: its last frame, its distance from the crosshair there, the track, and its chain
/// (`chain`).
struct Candidate {
    /// The track's last frame.
    end: i64,
    /// Its distance from the crosshair there (degrees).
    distance: f64,
    /// The track's id.
    track: u32,
    /// The track and the tracks it continues, latest first.
    chain: Vec<u32>,
}

/// The tracks that may be kills, in order of their end, then distance, then id (see `match_video`).
fn kill_candidates(
    index: &TrackIndex,
    follows: &HashMap<u32, u32>,
    typical_area: Option<f64>,
    near: f64,
    frame_count: i64,
) -> Vec<Candidate> {
    let before: HashMap<u32, u32> = follows.iter().map(|(&earlier, &later)| (later, earlier)).collect();
    let mut steady_ends: HashMap<i64, usize> = HashMap::new();
    for &track in &index.ids {
        if index.places[&track].len() >= STEADY_TRACK_POINTS && !follows.contains_key(&track) {
            *steady_ends.entry(index.last(track)).or_default() += 1;
        }
    }
    let ending_together =
        |end: i64| (end - 1..=end + 1).map(|frame| steady_ends.get(&frame).copied().unwrap_or(0)).sum::<usize>();
    let mut candidates = Vec::new();
    for &track in &index.ids {
        let end = index.last(track);
        let (x, y) = index.places[&track][&end];
        let cut_short = end >= frame_count - END_MARGIN_FRAMES;
        if follows.contains_key(&track) || cut_short || hypot(x, y) >= near || ending_together(end) >= RUN_END_TRACKS {
            continue;
        }
        let chain = chain(track, &before);
        if chain.iter().map(|earlier| index.places[earlier].len()).sum::<usize>() < MIN_KILL_TRACK_POINTS {
            continue;
        }
        let sizes: Vec<f64> = chain.iter().flat_map(|&earlier| index.near_areas(earlier)).collect();
        if typical_area.is_some_and(|area| !sizes.is_empty() && median(&sizes) < MIN_TARGET_SHARE * area) {
            continue;
        }
        candidates.push(Candidate { end, distance: hypot(x, y), track, chain });
    }
    candidates.sort_by(|a, b| a.end.cmp(&b.end).then(a.distance.total_cmp(&b.distance)).then(a.track.cmp(&b.track)));
    candidates
}

/// One kill for candidates within SAME_KILL_FRAMES of each other: the one nearest the crosshair.
fn one_per_kill(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut kills: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        if let Some(kill) = kills.last_mut()
            && candidate.end - kill.end <= SAME_KILL_FRAMES
        {
            if candidate.distance < kill.distance {
                *kill = candidate;
            }
            continue;
        }
        kills.push(candidate);
    }
    kills
}

/// The flicks to the video's kills, built as `Kills::flick` builds them, with no shot counts.
fn video_flicks(index: &TrackIndex, kills: &[Candidate]) -> Vec<Flick> {
    let mut flicks = Vec::with_capacity(kills.len());
    let mut previous: Option<i64> = None;
    for (kill, candidate) in kills.iter().enumerate() {
        let places = index.joined(&candidate.chain);
        let first = *places.first_key_value().unwrap().0;
        let mut start = previous.unwrap_or(first);
        let spawned = first > start + SPAWN_FRAMES;
        if spawned {
            start = first;
        }
        let areas = index.near_areas(candidate.track);
        flicks.push(Flick {
            kill_number: kill + 1,
            kill_frame: candidate.end,
            stats_frame: None,
            start_frame: start,
            shots: None,
            path: places.range(start..=candidate.end).map(|(&frame, &(x, y))| (frame, x, y)).collect(),
            spawned,
            area_px: (!areas.is_empty()).then(|| median(&areas)),
        });
        previous = Some(candidate.end);
    }
    flicks
}

/// The flicks from the video alone, for a run without a stats file or a readable HUD. A kill is a track that ends near
/// the crosshair, unless:
/// - another track continues it (`TrackIndex::continued`: tracking lost the target; it did not die);
/// - it is the crosshair (`ghosts`);
/// - RUN_END_TRACKS or more steady tracks (STEADY_TRACK_POINTS frames or more, not continued) end within a frame of it
///   (the run ended or restarted); flickering false detections, such as a game's HUD text, do not count;
/// - its blob is less than MIN_TARGET_SHARE of the run's typical target (a hit marker or a spark, not a target);
/// - another kill was found within SAME_KILL_FRAMES (the same kill twice).
///
/// Tracks the camera's turn fooled at a kill are split first (`TrackIndex::repair`). A flick's path joins the pieces of
/// its target's track. "Near" is the target's radius (from the tracks' median blob area) plus RIM_DEG, and at least
/// AT_CROSSHAIR_DEG.
pub fn match_video(tracks: &Tracks) -> (Vec<Flick>, MatchInfo) {
    let tracks = without_ghosts(&without_crosshair_ends(&without_crosshair_boxes(tracks)));
    let (fps, frames) = (tracks.fps, &tracks.frames);
    let mut index = TrackIndex::new(frames);
    let typical_area = index.typical_area();
    let radius = typical_area.map_or(DEFAULT_TARGET_RADIUS_DEG, blob_radius_deg);
    let near = (radius + RIM_DEG).max(AT_CROSSHAIR_DEG);
    let turned = index.repair(frames, near);
    let follows = index.continued(&turned, fps, radius, near);
    let kills = one_per_kill(kill_candidates(&index, &follows, typical_area, near, frames.len() as i64));
    let flicks = video_flicks(&index, &kills);
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

/// Checks the kills found from the video alone and matched to kill times, on small made-up runs.
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
            .map(|(i, (shift, targets))| {
                TrackFrame { i, shift, a: vec![40; targets.len()], t: targets, wh: None, s: None }
            })
            .collect();
        Tracks { fps: 60.0, frames, version: 0 }
    }

    /// The kill frames the video alone finds in the tracks.
    fn kill_frames(tracks: &Tracks) -> Vec<i64> {
        match_video(tracks).0.iter().map(|flick| flick.kill_frame).collect()
    }

    /// A kill whose one-frame false camera turn hands its track to another target still counts, and so does that
    /// target's own kill.
    #[test]
    fn a_false_turn_at_a_kill_keeps_the_kill() {
        // the target at the crosshair dies on frame 21, and the tracker takes the other target for it, turned there by
        // a one-frame spike; the camera then turns to that target, which dies at the crosshair on frame 31
        let mut frames = Vec::new();
        for _ in 0..=20 {
            frames.push(((0.0, 0.0), vec![(1, 0.05, 0.0), (2, -3.0, -0.5)]));
        }
        frames.push(((-3.05, -0.5), vec![(1, -3.0, -0.5)]));
        for step in 1..=10 {
            frames.push(((0.3, 0.05), vec![(1, -3.0 + 0.3 * step as f64, -0.5 + 0.05 * step as f64)]));
        }
        for _ in 0..10 {
            frames.push(((0.0, 0.0), vec![]));
        }
        assert_eq!(kill_frames(&tracks(frames)), vec![20, 31]);
    }

    /// A target found again where it hid under the crosshair is the same target; one that spawns beside a dead one is
    /// a new target, and the dead one's kill counts.
    #[test]
    fn a_target_back_from_under_the_crosshair_is_no_kill_but_one_spawned_beside_it_is() {
        // hidden under the crosshair for 3 frames and found again where it was: one target, killed on frame 40
        let mut frames = Vec::new();
        for i in 0..=45 {
            let targets = if (21..24).contains(&i) { vec![] } else { vec![(if i < 21 { 1 } else { 2 }, 0.05, 0.0)] };
            frames.push(((0.0, 0.0), if i <= 40 { targets } else { vec![] }));
        }
        assert_eq!(kill_frames(&tracks(frames)), vec![40]);
        // killed on frame 20, and the next target spawns 0.4 degrees away on frame 25; the camera turns to it, and it
        // dies at the crosshair on frame 45
        let mut frames = Vec::new();
        for i in 0..=50 {
            let shift = if (31..=38).contains(&i) { (-0.05, 0.0) } else { (0.0, 0.0) };
            let x = 0.5 - 0.05 * (i.clamp(30, 38) - 30) as f64;
            let targets = match i {
                ..=20 => vec![(1, 0.1, 0.0)],
                25..=45 => vec![(2, x, 0.0)],
                _ => vec![],
            };
            frames.push((shift, targets));
        }
        assert_eq!(kill_frames(&tracks(frames)), vec![20, 45]);
    }

    /// `appearances` joins the pieces of a target that moves faster on its own than the camera's turn explains.
    #[test]
    fn a_target_that_outruns_the_camera_turn_is_one_track() {
        // the frames' turn is 0, but the target moves 1.3 degrees a frame on its own: one track for 5 frames, then a
        // new track every frame, too far for the camera's turn alone (1 degree), each where the target's speed takes it
        let frames =
            (0..12usize).map(|i| ((0.0, 0.0), vec![(i.max(4) as u32 - 3, -13.0 + 1.3 * i as f64, 0.5)])).collect();
        let follows = appearances(&tracks(frames), JOIN_GAP_S, JOIN_RADIUS_DEG).follows;
        assert_eq!(follows, (1..=7).map(|track| (track, track + 1)).collect());
    }

    /// A kill 1.8 degrees off a target's center matches it when the target is big enough to reach the crosshair.
    #[test]
    fn a_big_target_hit_at_its_rim_is_the_killed_one() {
        // the kill on frame 10, the target 1.8 degrees off: too far for a small blob, not for one 2 degrees in radius
        for (area, matched) in [(40, 0), (1000, 1)] {
            let frames = (0..20)
                .map(|i| TrackFrame { i, shift: (0.0, 0.0), t: vec![(1, 1.8, 0.0)], a: vec![area], wh: None, s: None })
                .collect();
            let tracks = Tracks { fps: 60.0, frames, version: 0 };
            assert_eq!(match_times(&tracks, &[10.0 / 60.0], &[1], 0.25, Some(0.0)).0.len(), matched);
        }
    }

    /// A kill matches a tall target whose box reaches the crosshair, though its center is far below it.
    #[test]
    fn a_tall_target_hit_at_its_head_is_the_killed_one() {
        // the kill on frame 10, a robot's box (1.5 x 5 degrees) centered 2.6 degrees below the crosshair: its blob's
        // radius cannot reach the crosshair, its box can (the crosshair 0.1 degrees past its top edge)
        for (size, matched) in [(None, 0), (Some((1.5, 5.0)), 1)] {
            let frames = (0..20)
                .map(|i| TrackFrame {
                    i,
                    shift: (0.0, 0.0),
                    t: vec![(1, 0.3, -2.6)],
                    a: vec![600],
                    wh: size.map(|size| vec![size]),
                    s: None,
                })
                .collect();
            let tracks = Tracks { fps: 60.0, frames, version: 0 };
            assert_eq!(match_times(&tracks, &[10.0 / 60.0], &[1], 0.25, Some(0.0)).0.len(), matched);
        }
    }

    /// A target's track that runs on over the crosshair's box ends at the target's last box, unless the target is not
    /// clearly bigger than the crosshair's box and the camera stays still (then the box went with the target).
    #[test]
    fn a_target_handed_to_the_crosshairs_box_ends_where_it_was_last_seen() {
        // the detector boxes the crosshair (0.6 degrees) at the center while the camera turns; a target comes to the
        // crosshair and dies on frame 50, and its track runs on over the crosshair's box until frame 55. A target of
        // 1.4 degrees is clearly bigger than the crosshair's box, so that box is the crosshair: the kill is on frame
        // 50, the camera turning after it or not. A target of 0.9 degrees is not: when the camera turns on frame 56,
        // the box stayed after the target (the kill is on frame 50); when the camera is still and the box is gone, the
        // box went with the target (the kill is on frame 55).
        let center = crosshair_center();
        for (target, turns, kill) in [(1.4, true, 50), (1.4, false, 50), (0.9, true, 50), (0.9, false, 55)] {
            let frames = (0..70usize)
                .map(|i| {
                    let crosshair = (1000 + i as u32, center.0, center.1, 0.6);
                    let (shift, boxes) = match i {
                        ..40 => ((0.3, 0.0), vec![crosshair]),
                        40..=50 => ((0.0, 0.0), vec![(1, 0.5 - 0.04 * (i - 40) as f64, 0.0, target)]),
                        51..=55 => ((0.0, 0.0), vec![(1, center.0, center.1, 0.6)]),
                        56..60 if turns => ((0.5, 0.0), vec![crosshair]),
                        _ => ((0.0, 0.0), vec![]),
                    };
                    TrackFrame {
                        i,
                        shift,
                        t: boxes.iter().map(|&(track, x, y, _)| (track, x, y)).collect(),
                        a: vec![40; boxes.len()],
                        wh: Some(boxes.iter().map(|&(_, _, _, size)| (size, size)).collect()),
                        s: None,
                    }
                })
                .collect();
            assert_eq!(kill_frames(&Tracks { fps: 60.0, frames, version: 0 }), vec![kill]);
        }
    }
}
