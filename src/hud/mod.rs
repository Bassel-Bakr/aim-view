//! A recording's on-screen HUD, read frame by frame for a run without a stats file (python/hud.py's algorithm):
//! KovaaK's session box (Kill Count, and Accuracy as hits/shots) or, when there is none, Aim Lab's POINTS and TIME
//! boxes. It gives the kill frames, the shots and hits, and the run's totals. The digits are learned from the recording
//! itself (no font): KovaaK's Kill Count counts up one kill at a time, Aim Lab's timer down one second at a time.
//!
//! In: every key frame's Y plane, then every frame's, from the review (src/session.rs: `Keys` reads the key frames,
//! each run part's `RunWatching` its frames, beside the camera watch). Out: each run part's `HudPart`, joined into one
//! watch (`Joining`), whose `HudReading` gives src/review.rs the kill times of a run without a stats file; and
//! KovaaK's box (`SessionRows`) for the area finder (src/areas.rs).
//!
//! It reads each frame's Y plane (brightness) as the decoder gives it, at the recording's size, never ffmpeg's grey
//! conversion: only the digits read must agree, not the pixels. A limited-range ("tv") recording's Y is stretched to
//! 0..255 first, as ffmpeg's grey conversion does. Each frame keeps only its value rows' glyphs (grey images of
//! GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX), and a row's glyphs are kept once while they stay the same from frame to frame,
//! so a long recording stays small.
//!
//! KovaaK's stats files are the reference, not python/hud.py: where it misreads and the stats files show it, the
//! reading here departs from it, and each place says so.
//!
//! The parts: scale.rs resamples a region as ffmpeg's `scale` does; layout.rs finds the boxes and cuts their glyphs;
//! glyphs.rs keeps each frame's glyphs (`GlyphStore`); watch.rs is the per-frame watch and its parts; reading.rs learns
//! the digits and reads the counts. This file holds what they share: the boxes' places, the glyphs' size, the rows,
//! and the reading's types.

mod glyphs;
mod layout;
mod reading;
mod scale;
mod watch;

use serde::{Deserialize, Serialize};

pub use layout::SessionRows;
pub(crate) use scale::{bilinear, taps};
pub use watch::{HudKeys, HudPart, HudWatch};

/// Where KovaaK's box can be, as shares of the frame (x0, y0, x1, y1): it grows to fit its widest number. The region
/// is scaled to BW x BH pixels (a 2560 x 1440 frame's).
pub(crate) const BOX: [f64; 4] = [0.0, 0.0, 900.0 / 2560.0, 330.0 / 1440.0];
/// The scaled region's width in pixels (the box region's width in a 2560 x 1440 frame).
pub(crate) const BW: usize = 900;
/// The scaled region's height in pixels.
pub(crate) const BH: usize = 330;
/// Glyphs are compared at GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX.
const GLYPH_WIDTH_PX: usize = 16;
/// The height glyphs are scaled to for comparing, pixels.
const GLYPH_HEIGHT_PX: usize = 24;
/// The pixels in a scaled glyph image.
const GLYPH_PIXELS: usize = GLYPH_WIDTH_PX * GLYPH_HEIGHT_PX;
/// Aim Lab's POINTS and TIME value line, as shares of a 16:9 frame, scaled to AW x AH pixels (twice 720p), and the two
/// values' columns in it.
pub(crate) const AIM_BAND: [f64; 4] = [0.30, 40.0 / 720.0, 0.565, 63.0 / 720.0];
/// The scaled value line's width in pixels.
pub(crate) const AW: usize = 678;
/// The scaled value line's height in pixels.
const AH: usize = 46;
/// The POINTS value's columns in the scaled line (from, to).
pub(crate) const AIM_POINTS: (usize, usize) = (26, 356);
/// The TIME value's columns in the scaled line (from, to).
pub(crate) const AIM_TIME: (usize, usize) = (368, 656);
/// The glyphs a row keeps for new lines to reuse.
const RECENT_GLYPHS: usize = 32;
/// The rows each frame keeps: KovaaK's Kill Count and Accuracy, Aim Lab's POINTS and TIME. This one: Kill Count.
const KILL_COUNT_ROW: usize = 0;
/// KovaaK's Accuracy row: hits/shots (percent).
const ACCURACY_ROW: usize = 1;
/// Aim Lab's POINTS row.
const POINTS_ROW: usize = 2;
/// Aim Lab's TIME row.
const TIME_ROW: usize = 3;
/// How many rows each frame keeps.
const ROWS: usize = 4;
/// Glyphs this alike (cosine of their grey images) are the same shape.
const SAME_SHAPE: f64 = 0.97;

/// Which game's HUD was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum HudGame {
    /// KovaaK's session box: Kill Count and Accuracy.
    Kovaak,
    /// Aim Lab's POINTS and TIME boxes.
    Aimlab,
}

/// The run's totals as the HUD shows them at the end: the kills counted, and the hits and shots (None where the
/// Accuracy line was not read).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HudFinal {
    /// The kills counted.
    pub kills: i64,
    /// The hits (in KovaaK's, with the hits of the kills the Accuracy line had not shown yet); None where the Accuracy
    /// line was not read.
    pub hits: Option<i64>,
    /// The shots, those kills' shots added likewise; None where the Accuracy line was not read.
    pub shots: Option<i64>,
}

/// What the HUD read (python/hud.py: read() and read_aimlab()). Frames are the recording's frame indexes (0 is the
/// first frame from time 0 on), on the video's own clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HudReading {
    /// Which game's HUD this is.
    pub game: HudGame,
    /// One entry per kill: the frame its count went up.
    pub kills: Vec<i64>,
    /// One entry per shot: the frame its count went up.
    pub shots: Vec<i64>,
    /// One entry per hit: the frame its count went up.
    pub hits: Vec<i64>,
    /// The run's totals at the end ("final" in the JSON).
    #[serde(rename = "final")]
    pub totals: HudFinal,
    /// The share of the count's steps that were a plausible step (+1 kill, a hit or a miss).
    pub checked: f64,
    /// Aim Lab's points at the end (it shows no score otherwise); None for KovaaK's.
    pub points: Option<f64>,
}

/// Tests of the glyph cutting, the digit learning, the counts' reading and the joining of run parts, on synthetic
/// glyphs and bands.
#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::glyphs::{Glyph, GlyphStore};
    use super::layout::{Cut, Layout, Row, value_glyphs};
    use super::reading::{aimlab, kovaak, learn_digits};
    use super::*;
    use crate::capped::Capped;

    /// A glyph for `pattern` (0 to 11): two rows of its own lit, so no two patterns are alike.
    fn glyph(pattern: usize, height: u16) -> Cut {
        let mut image = [0u8; GLYPH_PIXELS];
        image[2 * pattern * GLYPH_WIDTH_PX..(2 * pattern + 2) * GLYPH_WIDTH_PX].fill(255);
        Cut { image, height, width: 10 }
    }

    /// The glyphs of a number's digits, each its digit's pattern, `height` pixels high.
    fn line(value: i64, height: u16) -> Vec<Cut> {
        value.to_string().bytes().map(|b| glyph((b - b'0') as usize, height)).collect()
    }

    /// Two digits joined by a bridge are cut in two at the bridge, and the label left of a wide gap is left out.
    #[test]
    fn a_wide_glyph_is_split_and_the_label_left_out() {
        let (width, height) = (200, 30);
        let mut band = vec![60u8; width * height];
        let mut fill = |x0: usize, x1: usize, y0: usize, y1: usize| {
            for y in y0..y1 {
                band[y * width + x0..y * width + x1].fill(230);
            }
        };
        fill(10, 40, 8, 22); // the label
        fill(150, 162, 5, 25); // two digits run together by a one-pixel bridge
        fill(164, 176, 5, 25);
        fill(162, 164, 15, 16);
        let glyphs = value_glyphs(&band, width, None);
        let sizes: Vec<(u16, u16)> = glyphs.iter().map(|glyph| (glyph.width, glyph.height)).collect();
        assert_eq!(sizes, vec![(12, 20), (14, 20)]);
        assert!(glyphs[0].image.iter().all(|&level| level == 255));
    }

    /// Shape ids for the digits 0 to 9, in an order of their own.
    const SHAPE_OF_DIGIT: [i32; 10] = [7, 3, 9, 0, 5, 1, 8, 2, 6, 4];

    /// The shape ids of a number's digits (SHAPE_OF_DIGIT).
    fn shapes_of(value: u32) -> Vec<i32> {
        value.to_string().bytes().map(|b| SHAPE_OF_DIGIT[(b - b'0') as usize]).collect()
    }

    /// A count from 0 to 25 teaches every digit's shape; one that never changes its tens place teaches none.
    #[test]
    fn digits_are_learned_from_a_count() {
        let readings: Vec<Vec<i32>> = (0..=25).map(shapes_of).collect();
        let stable: Vec<&[i32]> = readings.iter().map(|reading| reading.as_slice()).collect();
        let digits = learn_digits(&stable, 10).unwrap();
        for (digit, &shape) in SHAPE_OF_DIGIT.iter().enumerate() {
            assert_eq!(digits[shape as usize], Some(digit as u8));
        }
        assert_eq!(learn_digits(&stable[..8], 10), None); // no tens place changed: no 0
    }

    /// A synthetic HUD (`hud`) with a miss before every fifth kill.
    fn counting(frames: Range<usize>) -> (GlyphStore, Layout) {
        hud(frames, |frame, kills| {
            let fifth_kills = (1..=kills).filter(|kill| kill % 5 == 0).count() as i64;
            let misses = fifth_kills + i64::from(kills % 5 == 4 && frame % 10 >= 5);
            (kills, kills + misses)
        })
    }

    /// A store fed with a synthetic HUD: the Kill Count counting to 40, ten frames a value, and Accuracy as
    /// `accuracy(frame, kills)`, hits/shots (percent).
    fn hud(frames: Range<usize>, accuracy: impl Fn(usize, i64) -> (i64, i64)) -> (GlyphStore, Layout) {
        hud_drawn(frames, accuracy, |shots| line(shots, 18))
    }

    /// `hud` with the shots drawn by `draw_shots`.
    fn hud_drawn(
        frames: Range<usize>,
        accuracy: impl Fn(usize, i64) -> (i64, i64),
        draw_shots: impl Fn(i64) -> Vec<Cut>,
    ) -> (GlyphStore, Layout) {
        let layout = Layout {
            x0: 0,
            x1: 100,
            kills: Row { y0: 0, y1: 20, start: None },
            accuracy: Row { y0: 30, y1: 50, start: None },
        };
        let mut store = GlyphStore::default();
        let mut recent: [Capped<Glyph, RECENT_GLYPHS>; ROWS] = Default::default();
        for frame in frames {
            let kills = (frame / 10).min(40) as i64;
            let (hits, shots) = accuracy(frame, kills);
            let mut accuracy_line = line(hits, 18);
            accuracy_line.push(glyph(10, 18)); // "/"
            accuracy_line.extend(draw_shots(shots));
            accuracy_line.push(glyph(11, 18)); // "("
            accuracy_line.extend(line(90, 18));
            let rows = [
                (KILL_COUNT_ROW, line(kills, 18)),
                (ACCURACY_ROW, accuracy_line),
                (POINTS_ROW, Vec::new()),
                (TIME_ROW, Vec::new()),
            ];
            for (row, cuts) in rows {
                store.push(row, cuts, &mut recent[row]);
            }
        }
        (store, layout)
    }

    /// A Kill Count counting to 40 gives a kill every 10 frames, and the Accuracy line the hits, shots and misses.
    #[test]
    fn a_counting_hud_reads_its_kills_and_shots() {
        let (store, layout) = counting(0..430);
        let reading = kovaak(&store, &layout).unwrap();
        assert_eq!(reading.kills, (1..=40).map(|kill| 10 * kill).collect::<Vec<i64>>());
        assert_eq!(reading.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(48) });
        assert_eq!(reading.hits.len(), 40);
        assert_eq!(reading.shots.iter().filter(|&&frame| frame % 10 == 5).count(), 8); // the misses, between kills
        assert_eq!(reading.checked, 1.0);
        assert!(aimlab(&store).is_none());
    }

    /// The run before's reading at the start is dropped, and a last kill the Accuracy line never showed adds its hit.
    #[test]
    fn a_restart_and_an_early_end_still_count_every_hit() {
        // the Accuracy line is redrawn every 7 frames: first it shows the run before's 8/9, and the run ends before it
        // shows the last kill
        let (store, layout) = hud(0..430, |frame, _| {
            let redrawn = (frame / 7 * 7).min(399);
            let kills = (redrawn / 10) as i64;
            if frame < 15 { (8, 9) } else { (kills, kills) }
        });
        let reading = kovaak(&store, &layout).unwrap();
        assert_eq!(reading.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(40) });
        assert_eq!((reading.hits.len(), reading.shots.len()), (40, 40));
        assert_eq!((reading.hits[0], reading.hits[39]), (15, 400)); // the run's first reading, and the last kill
    }

    /// An Accuracy line with a digit less than DIGIT_LIKENESS alike to every digit is not read, so no shot is
    /// miscounted.
    #[test]
    fn a_digit_like_no_digit_spoils_its_accuracy_line() {
        // the left 4 of "44" drawn closer to its neighbor: most like an 8 (0.83), but less than DIGIT_LIKENESS
        let squeezed_four = || {
            let mut cut = glyph(8, 18);
            cut.image[8 * GLYPH_WIDTH_PX..10 * GLYPH_WIDTH_PX].fill(170);
            cut
        };
        let draw_shots = |shots: i64| {
            let mut cuts = line(shots, 18);
            if shots == 44 {
                cuts[0] = squeezed_four();
            }
            cuts
        };
        let misses = |kills: i64| (1..=kills).filter(|kill| kill % 5 == 0).count() as i64;
        let (store, layout) = hud_drawn(0..430, |_, kills| (kills, kills + misses(kills)), draw_shots);
        let reading = kovaak(&store, &layout).unwrap();
        assert_eq!(reading.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(48) });
        assert_eq!((reading.hits.len(), reading.shots.len()), (40, 48));
    }

    /// A glyph that shows for a moment after the number is no step of the count and gets no digit.
    #[test]
    fn a_glyph_after_the_number_teaches_no_digit() {
        let mut readings: Vec<Vec<i32>> = Vec::new();
        for value in 0..=25u32 {
            readings.push(shapes_of(value));
            if value % 4 == 1 {
                let mut junk = readings[readings.len() - 1].clone();
                junk.push(10); // an edge or a panel beside the number, for a moment
                readings.push(junk);
                readings.push(readings[readings.len() - 2].clone());
            }
        }
        let stable: Vec<&[i32]> = readings.iter().map(|reading| reading.as_slice()).collect();
        let digits = learn_digits(&stable, 11).unwrap();
        assert_eq!(digits[10], None);
        assert!(SHAPE_OF_DIGIT.iter().enumerate().all(|(digit, &shape)| digits[shape as usize] == Some(digit as u8)));
    }

    /// Three run parts, each through JSON and back, join into the same glyphs and reading as one watch.
    #[test]
    fn parts_join_as_one_watch() {
        let (whole, layout) = counting(0..430);
        let part = |frames: Range<usize>| HudPart {
            frames: frames.len(),
            layout: Some(layout),
            session: None,
            store: counting(frames).0,
        };
        // each run part but the last also reads the next part's first frame
        let parts = [part(0..201), part(200..301), part(300..430)];
        let mut watch = HudWatch::new(0, 0, true);
        for part in parts {
            let text = serde_json::to_string(&part).unwrap();
            let back: HudPart = serde_json::from_str(&text).unwrap();
            assert_eq!(back, part);
            watch.join(back);
        }
        assert_eq!(watch.frames(), 430);
        let glyphs = |store: &GlyphStore, row: usize| -> Vec<Vec<Vec<u8>>> {
            store
                .per_frame(row)
                .into_iter()
                .map(|line| store.lines[line as usize].iter().map(|glyph| store.image(glyph.image).to_vec()).collect())
                .collect()
        };
        for row in 0..ROWS {
            assert_eq!(glyphs(&watch.store, row), glyphs(&whole, row));
        }
        assert_eq!(watch.finish(), kovaak(&whole, &layout));
    }

    /// A part whose rows' frames do not add up to its frames is refused when read from JSON.
    #[test]
    fn a_part_that_does_not_add_up_is_refused() {
        let (store, layout) = counting(0..50);
        let mut text =
            serde_json::to_value(HudPart { frames: 50, layout: Some(layout), session: None, store }).unwrap();
        text["frames"] = 51.into();
        assert!(serde_json::from_value::<HudPart>(text).is_err());
    }

    /// Skipped frames count as frames with the empty line in every row, and give no reading.
    #[test]
    fn skipped_frames_read_nothing() {
        let mut watch = HudWatch::new(64, 36, true);
        watch.skip(5);
        watch.add(&[0u8; 64 * 36]);
        assert_eq!(watch.frames(), 6);
        assert_eq!(watch.store.per_frame(KILL_COUNT_ROW), vec![0; 6]);
        assert_eq!(watch.finish(), None);
    }
}
