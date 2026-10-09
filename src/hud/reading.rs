//! Reading the HUD's counts (src/hud/mod.rs): the digits learned from the recording itself (the glyphs' shapes, and
//! KovaaK's Kill Count counting up one kill at a time, Aim Lab's timer down one second at a time), then the Kill
//! Count's kill frames, the Accuracy line's shots and hits, or Aim Lab's POINTS steps.
//!
//! In: the glyph lines the watch kept (glyphs.rs). Out: the `HudReading` the watch gives the review.

use std::collections::HashMap;
use std::iter::repeat_n;

use super::glyphs::{Glyph, GlyphStore};
use super::layout::Layout;
use super::{
    ACCURACY_ROW, AH, GLYPH_PIXELS, HudFinal, HudGame, HudReading, KILL_COUNT_ROW, POINTS_ROW, SAME_SHAPE, TIME_ROW,
};
use crate::capped::Capped;

/// A shape is the mean of its first SHAPE_MEAN_GLYPHS glyphs.
const SHAPE_MEAN_GLYPHS: u32 = 50;
/// The least length an image is divided by (an empty image's is 0).
const MIN_NORM: f64 = 1e-6;
/// A glyph's shape when it is like none and new shapes are not learned.
const NO_SHAPE: i32 = -1;
/// A glyph at least this share of its band high is tall: a digit, or the Accuracy line's "/" and "(".
const TALL_SHARE: f64 = 0.5;
/// A reading is stable when it stays the same for at least this many frames.
const STABLE_FRAMES: usize = 3;
/// A shape left over when the digits are learned is the digit it is at least this alike to (cosine), and a POINTS
/// glyph, or an Accuracy line's hits or shots glyph, less alike than this to every digit spoils its number.
const DIGIT_LIKENESS: f64 = 0.9;
/// The Kill Count's likenesses tried, each with the share of its steps that must be +1. A very blurred upload can split
/// one digit into two shapes at the usual likeness, and the digits are not learned; then looser likenesses are tried,
/// trusting only a Kill Count that counts up by one at almost every step.
const KILL_COUNT_TRIES: [(f64, f64); 5] = [(SAME_SHAPE, 0.8), (0.96, 0.95), (0.95, 0.95), (0.94, 0.95), (0.93, 0.95)];
/// A Kill Count is read from at least this many steps.
const MIN_KILL_COUNT_STEPS: usize = 3;
/// A step of more kills than this is a misread.
const MAX_KILL_STEP: i64 = 3;
/// A lone misread lasts at most this many frames: a real restart's 0 stays up for a second or more.
const MAX_MISREAD_FRAMES: usize = 10;
/// A step of more hits or shots than this is a misread.
const MAX_SHOT_STEP: i64 = 50;
/// The Accuracy line's marks ("/" and "(") are the mean of their first MARK_MEAN_LINES lines that read as digits, and
/// need MIN_MARK_LINES of them.
const MARK_MEAN_LINES: u32 = 50;
/// The fewest lines read as digits that the marks are learned from; with fewer, the Kill Count's likeness reads them.
const MIN_MARK_LINES: u32 = 3;
/// "--/-- ( %)" (no shot yet) has at most this many tall glyphs.
const NO_SHOT_TALL_GLYPHS: usize = 4;
/// Aim Lab's TIME likenesses tried.
const TIME_TRIES: [f64; 3] = [SAME_SHAPE, 0.96, 0.95];
/// Aim Lab's TIME line has this many glyphs, the colon left out.
const TIME_GLYPHS: usize = 4;
/// The TIME box counts down one second at a time on at least TIME_STEP_SHARE of at least MIN_TIME_STEPS steps.
const MIN_TIME_STEPS: usize = 10;
/// The least share of the TIME box's steps that must be one second down.
const TIME_STEP_SHARE: f64 = 0.95;
/// A POINTS glyph more than this many times as wide as it is tall, before any digit, is a minus sign.
const MINUS_MIN_ASPECT: f64 = 1.2;
/// One POINTS step is up to this many hits and as many misses (two hits, or a hit and a miss, in one step).
const MAX_STEP_EVENTS: i64 = 3;
/// A POINTS step is its hits and misses when it is within STEP_TOLERANCE_POINTS of them, or STEP_TOLERANCE_SHARE of a
/// hit's points when that is more.
const STEP_TOLERANCE_POINTS: f64 = 1.0;
/// A POINTS step's tolerance as a share of a hit's points, when that is more than STEP_TOLERANCE_POINTS.
const STEP_TOLERANCE_SHARE: f64 = 0.15;
/// Aim Lab's HUD is read when it gives at least MIN_AIM_HITS hits and explains at least MIN_AIM_CHECKED of the steps.
const MIN_AIM_HITS: usize = 10;
/// The least share of the POINTS steps that must be some hits and misses for Aim Lab's HUD to count.
const MIN_AIM_CHECKED: f64 = 0.85;

/// Glyph shapes seen so far; a glyph joins the most alike shape, or starts a new one (python/hud.py: _Shapes). A
/// shape is the mean of its first SHAPE_MEAN_GLYPHS glyphs.
struct Shapes<'a> {
    /// The glyph images the shapes are learned from.
    store: &'a GlyphStore,
    /// The least likeness (cosine) for a glyph to join a shape.
    same: f64,
    /// Each shape's mean image (0 to 1 a pixel).
    shapes: Vec<[f32; GLYPH_PIXELS]>,
    /// Each shape's image's length, for the cosine.
    norms: Vec<f64>,
    /// How many glyphs each shape has taken.
    glyph_counts: Vec<u32>,
    /// Bumped whenever a shape changes; each image's most alike shape is kept with the version it was found at.
    version: u32,
    /// Per store image: the version and the most alike shape found then (None: like none).
    alike_cache: Vec<Option<(u32, Option<usize>)>>,
}

/// A vector's length (the square root of its squares' sum).
fn norm(values: impl Iterator<Item = f32>) -> f64 {
    (values.map(|x| x * x).sum::<f32>() as f64).sqrt()
}

/// An image divided by its length (MIN_NORM at least), so its length is 1.
fn to_unit(image: [f32; GLYPH_PIXELS]) -> [f32; GLYPH_PIXELS] {
    let length = norm(image.iter().copied()).max(MIN_NORM) as f32;
    image.map(|x| x / length)
}

/// The dot product of two images; of two unit images, their likeness (cosine).
fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() as f64
}

impl<'a> Shapes<'a> {
    /// No shapes yet, for the glyphs of `store`, joining at likeness `same`.
    fn new(store: &'a GlyphStore, same: f64) -> Shapes<'a> {
        Shapes {
            store,
            same,
            shapes: Vec::new(),
            norms: Vec::new(),
            glyph_counts: Vec::new(),
            version: 0,
            alike_cache: vec![None; store.images.len() / GLYPH_PIXELS],
        }
    }

    /// A store image as strengths from 0 to 1.
    fn glyph(&self, image: u32) -> [f32; GLYPH_PIXELS] {
        let image = self.store.image(image);
        std::array::from_fn(|i| image[i] as f32 / 255.0)
    }

    /// A store image as a unit image (length 1).
    fn unit_glyph(&self, image: u32) -> [f32; GLYPH_PIXELS] {
        to_unit(self.glyph(image))
    }

    /// The shape most alike to an image, when one is at least `same` alike.
    fn most_alike(&self, image: u32) -> Option<usize> {
        let glyph = self.glyph(image);
        let glyph_norm = norm(glyph.iter().copied()).max(MIN_NORM);
        let mut best: Option<(f64, usize)> = None;
        for (shape, (pixels, &shape_norm)) in self.shapes.iter().zip(&self.norms).enumerate() {
            let likeness = dot(&glyph, pixels) / glyph_norm / shape_norm.max(MIN_NORM);
            if likeness >= self.same && best.is_none_or(|(best_likeness, _)| likeness > best_likeness) {
                best = Some((likeness, shape));
            }
        }
        best.map(|(_, shape)| shape)
    }

    /// The glyph's shape; NO_SHAPE when it is like none and `learn` is off.
    fn id(&mut self, image: u32, learn: bool) -> i32 {
        let best = match self.alike_cache[image as usize] {
            Some((version, best)) if version == self.version => best,
            _ => {
                let best = self.most_alike(image);
                self.alike_cache[image as usize] = Some((self.version, best));
                best
            }
        };
        let shape = match best {
            Some(shape) => shape,
            None if !learn => return NO_SHAPE,
            None => self.add_shape(image),
        };
        self.glyph_counts[shape] += 1;
        // the shape is the mean of its first Kill Count glyphs (the Accuracy row's slashes and brackets, cut into
        // pieces, would blur it)
        let glyph_count = self.glyph_counts[shape];
        if learn && glyph_count <= SHAPE_MEAN_GLYPHS && glyph_count > 1 {
            let (glyph, count) = (self.glyph(image), glyph_count as f32);
            self.shapes[shape].iter_mut().zip(&glyph).for_each(|(mean, &level)| *mean += (level - *mean) / count);
            self.norms[shape] = norm(self.shapes[shape].iter().copied());
            self.version += 1;
        }
        shape as i32
    }

    /// A new shape: the image's glyph.
    fn add_shape(&mut self, image: u32) -> usize {
        let glyph = self.glyph(image);
        self.norms.push(norm(glyph.iter().copied()));
        self.shapes.push(glyph);
        self.glyph_counts.push(0);
        self.version += 1;
        self.shapes.len() - 1
    }

    /// A line's tall glyphs (in a band `band` rows high) as shapes, learning new ones; None without any.
    fn learn_line(&mut self, line: u32, band: usize) -> Option<Vec<i32>> {
        let store = self.store;
        let ids: Vec<i32> = store.tall_glyphs(line, band).map(|glyph| self.id(glyph.image, true)).collect();
        (!ids.is_empty()).then_some(ids)
    }

    /// A shape's mean image as a unit image (length 1).
    fn unit(&self, shape: usize) -> [f32; GLYPH_PIXELS] {
        let length = self.norms[shape].max(MIN_NORM) as f32;
        self.shapes[shape].map(|x| x / length)
    }
}

/// A reading that stays the same over the frames first..=last.
struct Stretch<T> {
    /// What the frames read.
    reading: T,
    /// The stretch's first frame.
    first: usize,
    /// Its last frame.
    last: usize,
}

/// The stable readings: each stretch of STABLE_FRAMES frames or more with the same reading (python/hud.py: _runs).
fn stable_stretches<T: Clone + PartialEq>(readings: &[Option<T>]) -> Vec<Stretch<T>> {
    let mut stretches: Vec<Stretch<Option<T>>> = Vec::new();
    for (frame, reading) in readings.iter().enumerate() {
        match stretches.last_mut() {
            Some(last) if last.reading == *reading => last.last = frame,
            _ => stretches.push(Stretch { reading: reading.clone(), first: frame, last: frame }),
        }
    }
    stretches
        .into_iter()
        .filter(|stretch| stretch.last - stretch.first + 1 >= STABLE_FRAMES)
        .filter_map(|stretch| Some(Stretch { reading: stretch.reading?, first: stretch.first, last: stretch.last }))
        .collect()
}

/// Counts in the order first seen (Python's Counter: most_common keeps that order among equal counts).
struct Counter<K>(Vec<(K, usize)>);

impl<K: PartialEq + Copy> Counter<K> {
    /// An empty counter.
    fn new() -> Counter<K> {
        Counter(Vec::new())
    }

    /// Counts `key` once more.
    fn add(&mut self, key: K) {
        match self.0.iter_mut().find(|entry| entry.0 == key) {
            Some(entry) => entry.1 += 1,
            None => self.0.push((key, 1)),
        }
    }

    /// The keys and their counts, most first; equal counts in the order first seen (the sort is stable).
    fn most_common(&self) -> Vec<(K, usize)> {
        let mut sorted = self.0.clone();
        sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        sorted
    }

    /// The most counted key, the first seen among equals; None when nothing was counted.
    fn top(&self) -> Option<K> {
        self.0
            .iter()
            .fold(
                None,
                |best: Option<(K, usize)>, &entry| {
                    if best.is_none_or(|best| entry.1 > best.1) { Some(entry) } else { best }
                },
            )
            .map(|entry| entry.0)
    }
}

/// Which shape is which digit, from stable readings counting up (python/hud.py: _learn_digits): by shape, its digit.
pub(super) fn learn_digits(readings: &[&[i32]], shape_count: usize) -> Option<Vec<Option<u8>>> {
    let (ends_in_zero, next_shape) = step_votes(readings);
    let zero = ends_in_zero.top()?;
    let mut next: Vec<(i32, i32)> = Vec::new();
    for ((shape, after), _) in next_shape.most_common() {
        if shape != after && !next.iter().any(|pair| pair.0 == shape) && !next.iter().any(|pair| pair.1 == after) {
            next.push((shape, after));
        }
    }
    let after = |shape: i32| next.iter().find(|pair| pair.0 == shape).map(|pair| pair.1);
    let mut digits = vec![None; shape_count];
    let (mut shape, mut seen) = (zero, Capped::<i32, 10>::from_iter([zero]));
    digits[zero as usize] = Some(0);
    for digit in 1..10 {
        shape = after(shape)?;
        if seen.contains(&shape) {
            return None;
        }
        seen.push(shape);
        digits[shape as usize] = Some(digit);
    }
    (after(shape) == Some(zero)).then_some(digits)
}

/// What each step between stable readings says: the last shape of a number that ends in 0 (the tens place changed, or
/// a digit was added), and the last digit's next shape (usually one more kill).
fn step_votes(readings: &[&[i32]]) -> (Counter<i32>, Counter<(i32, i32)>) {
    let (mut ends_in_zero, mut next_shape) = (Counter::new(), Counter::new());
    for pair in readings.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (a_last, b_last) = (a[a.len() - 1], b[b.len() - 1]);
        // a digit added after the same digits is no count's step (python/hud.py took it as one): it is a glyph that
        // is not part of the number, such as the edge of KovaaK's results panel fading out at the start of a run
        let grew = b.len() == a.len() + 1 && b[..a.len()] != *a;
        if (b.len() == a.len() && a[..a.len() - 1] != b[..b.len() - 1]) || grew {
            ends_in_zero.add(b_last);
        }
        if b.len() == a.len() || grew {
            next_shape.add((a_last, b_last));
        }
    }
    (ends_in_zero, next_shape)
}

/// The number a reading's shapes spell, if every one is a digit.
fn number(reading: &[i32], digits: &[Option<u8>]) -> Option<i64> {
    let reading_digits = reading
        .iter()
        .map(|&shape| digits.get(usize::try_from(shape).ok()?).copied().flatten())
        .collect::<Option<Vec<u8>>>()?;
    value(&reading_digits)
}

/// The number digits spell (None without any).
fn value(digits: &[u8]) -> Option<i64> {
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0i64, |total, &digit| total.checked_mul(10)?.checked_add(digit as i64))
}

/// Whether a glyph is tall in a band `band` rows high.
pub(super) fn tall(glyph: &Glyph, band: usize) -> bool {
    glyph.height as f64 / band as f64 >= TALL_SHARE
}

/// The value rounded to 3 decimals (halves away from 0).
fn round_to_thousandths(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The Kill Count read: the shapes, the digits, the stable values and the share of steps that were +1.
struct KillCount<'a> {
    /// The shapes learned from the Kill Count, which the Accuracy line is read with too.
    shapes: Shapes<'a>,
    /// By shape, its digit; None for a shape that is no digit.
    digits: Vec<Option<u8>>,
    /// The stable values, in order.
    values: Vec<Stretch<i64>>,
    /// The share of the steps between them that were +1.
    checked: f64,
}

/// The Kill Count read at one likeness (python/hud.py: _count), or None when the digits are not learned or fewer than
/// `need` of the steps are +1. The shapes are learned from the Kill Count alone.
fn count<'a>(store: &'a GlyphStore, kill_lines: &[u32], band: usize, same: f64, need: f64) -> Option<KillCount<'a>> {
    let mut shapes = Shapes::new(store, same);
    let readings: Vec<Option<Vec<i32>>> = kill_lines.iter().map(|&line| shapes.learn_line(line, band)).collect();
    let stable = stable_stretches(&readings);
    let stable_readings: Vec<&[i32]> = stable.iter().map(|stretch| stretch.reading.as_slice()).collect();
    let mut digits = learn_digits(&stable_readings, shapes.shapes.len())?;
    join_leftover_shapes(&shapes, &mut digits);
    // the stable values, from each frame's number: a digit caught mid-change can leave a shape of its own for a frame
    // or two, which joined its digit above, so the new value is read from its first frame (python/hud.py reads stable
    // shapes, and starts the value up to two frames late)
    let numbers: Vec<Option<i64>> =
        readings.iter().map(|reading| reading.as_ref().and_then(|reading| number(reading, &digits))).collect();
    let values = stable_stretches(&numbers);
    let steps: Vec<i64> = values.windows(2).map(|pair| pair[1].reading - pair[0].reading).collect();
    if steps.len() < MIN_KILL_COUNT_STEPS {
        return None;
    }
    let checked = steps.iter().filter(|&&step| step == 1).count() as f64 / steps.len() as f64;
    (checked >= need).then_some(KillCount { shapes, digits, values, checked })
}

/// In a blurred recording one digit can leave more than one shape: the others join the most alike digit's shape, when
/// they are DIGIT_LIKENESS alike.
fn join_leftover_shapes(shapes: &Shapes, digits: &mut [Option<u8>]) {
    for shape in 0..shapes.shapes.len() {
        if digits[shape].is_none() {
            let unit = shapes.unit(shape);
            let best = (0..digits.len())
                .filter(|&digit_shape| digits[digit_shape].is_some())
                .map(|digit_shape| (dot(&unit, &shapes.unit(digit_shape)), digit_shape))
                .fold(
                    None,
                    |best: Option<(f64, usize)>, entry| {
                        if best.is_none_or(|best| entry >= best) { Some(entry) } else { best }
                    },
                );
            if let Some((likeness, digit_shape)) = best
                && likeness >= DIGIT_LIKENESS
            {
                digits[shape] = digits[digit_shape];
            }
        }
    }
}

/// KovaaK's session box over the recording (python/hud.py: read).
pub(super) fn kovaak(store: &GlyphStore, layout: &Layout) -> Option<HudReading> {
    let kill_lines = store.per_frame(KILL_COUNT_ROW);
    let band = layout.kills.y1 - layout.kills.y0;
    let KillCount { shapes, digits, values, checked } =
        KILL_COUNT_TRIES.into_iter().find_map(|(same, need)| count(store, &kill_lines, band, same, need))?;
    let values = without_lone_misreads(values);
    let values = counted_values(&values)?;
    let kills = kill_frames(values);
    let (since, until) = (values[0].first, values[values.len() - 1].last);
    let reader = AccuracyReader { shapes, digits, band: layout.accuracy.y1 - layout.accuracy.y0, marks: None };
    let accuracy = accuracy_stretches(reader, since, until);
    let mut frames = shot_and_hit_frames(&accuracy, values[0].reading == 0);
    // the totals: the kills counted, and the fullest Accuracy reading (the HUD resets to 0 when the run ends)
    let fullest = accuracy.iter().map(|stretch| stretch.reading).fold(None, |best: Option<(i64, i64)>, reading| {
        if best.is_none_or(|best| reading.1 > best.1) { Some(reading) } else { best }
    });
    let totals = add_late_kills(&mut frames, fullest, &kills);
    Some(HudReading {
        game: HudGame::Kovaak,
        totals: HudFinal {
            kills: kills.len() as i64,
            hits: totals.map(|(hits, _)| hits),
            shots: totals.map(|(_, shots)| shots),
        },
        kills,
        shots: frames.shots,
        hits: frames.hits,
        checked: round_to_thousandths(checked),
        points: None,
    })
}

/// The stable values without a lone misread between two values that follow on (0, 9, 1: a 1 caught mid-change): it
/// looked like a restart, which split the run and lost the kills before it. Only a short one: a real restart's 0
/// between two 1s stays up for a second or more.
fn without_lone_misreads(values: Vec<Stretch<i64>>) -> Vec<Stretch<i64>> {
    let follows = |step: i64| (0..=MAX_KILL_STEP).contains(&step);
    let keep: Vec<bool> = (0..values.len())
        .map(|i| {
            i == 0
                || i == values.len() - 1
                || values[i].last - values[i].first > MAX_MISREAD_FRAMES
                || follows(values[i].reading - values[i - 1].reading)
                || !follows(values[i + 1].reading - values[i - 1].reading)
        })
        .collect();
    values.into_iter().zip(keep).filter_map(|(value, kept)| kept.then_some(value)).collect()
}

/// The values of the run that counts: a drop is a restart (or the end screen), and the run is the stretch between
/// drops with the most kills.
fn counted_values(values: &[Stretch<i64>]) -> Option<&[Stretch<i64>]> {
    let mut drops = vec![0];
    drops.extend((1..values.len()).filter(|&i| values[i].reading < values[i - 1].reading));
    drops.push(values.len());
    let (start, end) = drops
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .max_by_key(|&(start, end)| (values[end - 1].reading - values[start].reading, start))?;
    Some(&values[start..end])
}

/// Each kill's frame: every step of up to MAX_KILL_STEP kills counts its kills at the frame its value shows (a bigger
/// jump is a misread).
fn kill_frames(values: &[Stretch<i64>]) -> Vec<i64> {
    let mut kills = Vec::new();
    for pair in values.windows(2) {
        let step = pair[1].reading - pair[0].reading;
        if 0 < step && step <= MAX_KILL_STEP {
            kills.extend(repeat_n(pair[1].first as i64, step as usize));
        }
    }
    kills
}

/// Reads the Accuracy line, hits/shots (percent), with the Kill Count's shapes and digits (`band`: the line's height).
struct AccuracyReader<'a> {
    /// The Kill Count's shapes; no new ones are learned here.
    shapes: Shapes<'a>,
    /// By shape, its digit.
    digits: Vec<Option<u8>>,
    /// The Accuracy band's height in pixels, which says which glyphs are tall.
    band: usize,
    /// The "/" and the "(" as unit images, once they are learned.
    marks: Option<[[f32; GLYPH_PIXELS]; 2]>,
}

impl AccuracyReader<'_> {
    /// A line's tall glyphs by the rule of python/hud.py: a digit when it is like a digit shape at the likeness the
    /// Kill Count was read at (Some), else a mark (None).
    fn by_likeness(&mut self, line: u32) -> Vec<Option<u8>> {
        let store = self.shapes.store;
        store
            .tall_glyphs(line, self.band)
            .map(|glyph| usize::try_from(self.shapes.id(glyph.image, false)).ok().and_then(|shape| self.digits[shape]))
            .collect()
    }

    /// The "/" and the "(" (unit images): the mean of each in the `lines` that read as digits, the "/", digits and the
    /// "(". None when too few lines did.
    fn find_marks(&mut self, lines: &[u32]) -> Option<[[f32; GLYPH_PIXELS]; 2]> {
        let mut sums = [[0f32; GLYPH_PIXELS]; 2];
        let mut counts = [0; 2];
        let store = self.shapes.store;
        for &line in lines {
            let read = self.by_likeness(line);
            let mut marks_at = read.iter().enumerate().filter(|(_, digit)| digit.is_none()).map(|(at, _)| at);
            let (Some(slash), Some(paren)) = (marks_at.next(), marks_at.next()) else {
                continue;
            };
            if slash == 0 || paren == slash + 1 {
                continue;
            }
            let glyphs: Vec<&Glyph> = store.tall_glyphs(line, self.band).collect();
            for (mark, at) in [slash, paren].into_iter().enumerate() {
                if counts[mark] < MARK_MEAN_LINES {
                    let unit = self.shapes.unit_glyph(glyphs[at].image);
                    sums[mark].iter_mut().zip(&unit).for_each(|(sum, level)| *sum += level);
                    counts[mark] += 1;
                }
            }
        }
        if counts.iter().any(|&count| count < MIN_MARK_LINES) {
            return None;
        }
        Some(sums.map(to_unit))
    }

    /// One Accuracy line's hits and shots. The "/" and the "(" are the tall glyphs that are not digits: hits before the
    /// first, shots between them. Small or limited-range text can leave a digit just under the Kill Count's likeness
    /// (python/hud.py then reads it as a mark, and the line as nothing or as no shot yet), so with the marks known each
    /// glyph is the most alike of the digits, the "/" and the "(" instead.
    fn read(&mut self, line: u32) -> Option<(i64, i64)> {
        let read = match self.marks {
            None => self.by_likeness(line),
            Some(marks) => self.by_marks(line, &marks)?,
        };
        hits_and_shots(&read)
    }

    /// A line's tall glyphs as the most alike of the digits (Some) and the marks (None). None when a glyph of the hits
    /// or shots (before the second mark) is less than DIGIT_LIKENESS alike to it: a digit drawn closer to the next one
    /// (the left 4 of "44", the 7 of "74") is like no learned shape, and the most alike is another digit (an 8, a 1).
    fn by_marks(&self, line: u32, marks: &[[f32; GLYPH_PIXELS]; 2]) -> Option<Vec<Option<u8>>> {
        let prototypes: Vec<([f32; GLYPH_PIXELS], Option<u8>)> = (0..self.digits.len())
            .filter_map(|shape| self.digits[shape].map(|digit| (self.shapes.unit(shape), Some(digit))))
            .chain(marks.iter().map(|&mark| (mark, None)))
            .collect();
        let mut marks_seen = 0;
        let mut read = Vec::new();
        for glyph in self.shapes.store.tall_glyphs(line, self.band) {
            let unit = self.shapes.unit_glyph(glyph.image);
            let (likeness, digit) = prototypes
                .iter()
                .map(|(prototype, digit)| (dot(&unit, prototype), *digit))
                .fold((f64::MIN, None), |best, entry| if entry.0 > best.0 { entry } else { best });
            if digit.is_none() {
                marks_seen += 1;
            } else if marks_seen < 2 && likeness < DIGIT_LIKENESS {
                return None;
            }
            read.push(digit);
        }
        Some(read)
    }
}

/// The hits and shots an Accuracy line's tall glyphs spell (None: a mark): hits before the first mark, shots before the
/// second.
fn hits_and_shots(read: &[Option<u8>]) -> Option<(i64, i64)> {
    let (mut parts, mut current) = (Vec::new(), Vec::new());
    for glyph in read {
        match glyph {
            Some(digit) => current.push(*digit),
            None => parts.push(std::mem::take(&mut current)),
        }
    }
    if parts.len() < 2 {
        None
    } else if parts[0].is_empty() && parts[1].is_empty() {
        // "--/-- ( %)": no shot yet (its tall glyphs are the marks and "%)"; a line of unread digits has more)
        (read.len() <= NO_SHOT_TALL_GLYPHS).then_some((0, 0))
    } else {
        match (value(&parts[0]), value(&parts[1])) {
            (Some(hits), Some(shots)) if hits <= shots => Some((hits, shots)),
            _ => None,
        }
    }
}

/// The Accuracy line's stable (hits, shots) over the frames since..=until.
fn accuracy_stretches(mut reader: AccuracyReader, since: usize, until: usize) -> Vec<Stretch<(i64, i64)>> {
    let lines = reader.shapes.store.per_frame(ACCURACY_ROW);
    let mut window: Vec<u32> = lines[since..=until.min(lines.len() - 1)].to_vec();
    window.sort_unstable();
    window.dedup();
    reader.marks = reader.find_marks(&window);
    let mut read: HashMap<u32, Option<(i64, i64)>> = HashMap::new();
    let readings: Vec<Option<(i64, i64)>> = lines
        .into_iter()
        .enumerate()
        .map(|(frame, line)| {
            if frame < since || frame > until {
                return None;
            }
            *read.entry(line).or_insert_with(|| reader.read(line))
        })
        .collect();
    let mut stretches = stable_stretches(&readings);
    // the Accuracy line is redrawn three times a second, so a run started again first shows the run before's reading
    // for a moment: a first reading that drops right after is that one
    while stretches.len() >= 2 && stretches[1].reading.1 < stretches[0].reading.1 {
        stretches.remove(0);
    }
    stretches
}

/// Each shot's and each hit's frame.
struct ShotFrames {
    /// One frame per shot, in order.
    shots: Vec<i64>,
    /// One frame per hit, in order.
    hits: Vec<i64>,
}

/// Each shot and hit at the frame its Accuracy reading shows it (a step of more than MAX_SHOT_STEP is a misread). When
/// the Kill Count starts at 0 (`from_zero`), the counts of the first reading are the run's first shots and hits
/// (python/hud.py counts only the steps after it).
fn shot_and_hit_frames(accuracy: &[Stretch<(i64, i64)>], from_zero: bool) -> ShotFrames {
    let mut frames = ShotFrames { shots: Vec::new(), hits: Vec::new() };
    if let Some(first) = accuracy.first()
        && from_zero
        && first.reading.1 <= MAX_SHOT_STEP
    {
        let (hits, shots) = first.reading;
        frames.shots.extend(repeat_n(first.first as i64, shots as usize));
        frames.hits.extend(repeat_n(first.first as i64, hits as usize));
    }
    for pair in accuracy.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        let (new_hits, new_shots) = (after.reading.0 - before.reading.0, after.reading.1 - before.reading.1);
        if (0..=MAX_SHOT_STEP).contains(&new_hits) && 0 < new_shots && new_shots <= MAX_SHOT_STEP {
            frames.shots.extend(repeat_n(after.first as i64, new_shots as usize));
            frames.hits.extend(repeat_n(after.first as i64, new_hits as usize));
        }
    }
    frames
}

/// The totals (hits, shots) with the late kills. Each kill takes a hit. The Accuracy line is redrawn three times a
/// second, and a run can end before it shows its last kills: those kills' hits (each a shot) are added at their kill
/// frames (python/hud.py gives the last reading).
fn add_late_kills(frames: &mut ShotFrames, totals: Option<(i64, i64)>, kills: &[i64]) -> Option<(i64, i64)> {
    if let Some((hits, shots)) = totals
        && hits < kills.len() as i64
    {
        let late = &kills[kills.len() - (kills.len() as i64 - hits) as usize..];
        frames.hits.extend(late);
        frames.shots.extend(late);
        frames.hits.sort_unstable();
        frames.shots.sort_unstable();
        return Some((hits + late.len() as i64, shots + late.len() as i64));
    }
    totals
}

/// Aim Lab's HUD (python/hud.py: read_aimlab): every hit counted as a kill (one-hit targets). A hit adds points and a
/// miss takes some off, so the POINTS number gives every hit and miss. The digits are learned from the TIME box,
/// which counts down one second at a time (read backwards it counts up, as the Kill Count does).
pub(super) fn aimlab(store: &GlyphStore) -> Option<HudReading> {
    let time_lines = store.per_frame(TIME_ROW);
    let (shapes, digits) = TIME_TRIES.into_iter().find_map(|same| time_digits(store, &time_lines, same))?;
    let points = points_stretches(store, &shapes, &digits);
    let changes: Vec<(i64, usize)> =
        points.windows(2).map(|pair| (pair[1].reading - pair[0].reading, pair[1].first)).collect();
    let (hit, miss) = step_points(&changes)?;
    let (mut hits, mut misses, mut explained) = (Vec::new(), Vec::new(), 0);
    for &(change, frame) in &changes {
        if let Some((step_hits, step_misses)) = step_events(change, hit, miss) {
            hits.extend(repeat_n(frame as i64, step_hits as usize));
            misses.extend(repeat_n(frame as i64, step_misses as usize));
            explained += 1;
        }
    }
    let checked = explained as f64 / changes.len().max(1) as f64;
    if hits.len() < MIN_AIM_HITS || checked < MIN_AIM_CHECKED {
        return None;
    }
    let mut shots: Vec<i64> = hits.iter().chain(&misses).copied().collect();
    shots.sort_unstable();
    Some(HudReading {
        game: HudGame::Aimlab,
        totals: HudFinal {
            kills: hits.len() as i64,
            hits: Some(hits.len() as i64),
            shots: Some((hits.len() + misses.len()) as i64),
        },
        kills: hits.clone(),
        shots,
        hits,
        checked: round_to_thousandths(checked),
        points: points.last().map(|stretch| stretch.reading as f64),
    })
}

/// The shapes and digits learned from Aim Lab's TIME box at one likeness, when it then counts down one second at a
/// time.
fn time_digits<'a>(store: &'a GlyphStore, time_lines: &[u32], same: f64) -> Option<(Shapes<'a>, Vec<Option<u8>>)> {
    let mut shapes = Shapes::new(store, same);
    let readings: Vec<Option<Vec<i32>>> = time_lines
        .iter()
        .map(|&line| {
            if store.lines[line as usize].len() != TIME_GLYPHS {
                return None;
            }
            shapes.learn_line(line, AH)
        })
        .collect();
    let stable = stable_stretches(&readings);
    let backwards: Vec<&[i32]> = stable.iter().rev().map(|stretch| stretch.reading.as_slice()).collect();
    let digits = learn_digits(&backwards, shapes.shapes.len())?;
    let seconds: Vec<i64> =
        stable.iter().filter_map(|stretch| number(&stretch.reading, &digits)).map(clock_seconds).collect();
    let steps: Vec<i64> = seconds.windows(2).map(|pair| pair[0] - pair[1]).collect();
    let counts_down = steps.len() >= MIN_TIME_STEPS
        && steps.iter().filter(|&&step| step == 1).count() as f64 >= TIME_STEP_SHARE * steps.len() as f64;
    counts_down.then_some((shapes, digits))
}

/// A clock's m:ss, read as the number mss, in seconds.
fn clock_seconds(clock: i64) -> i64 {
    60 * (clock / 100) + clock % 100
}

/// The POINTS number's stable values. Each glyph is the most alike digit shape (a minus sign is short and wide).
fn points_stretches(store: &GlyphStore, shapes: &Shapes, digits: &[Option<u8>]) -> Vec<Stretch<i64>> {
    let prototypes: Vec<([f32; GLYPH_PIXELS], u8)> =
        digits.iter().enumerate().filter_map(|(shape, digit)| digit.map(|digit| (shapes.unit(shape), digit))).collect();
    let mut read: HashMap<u32, Option<i64>> = HashMap::new();
    let values: Vec<Option<i64>> = store
        .per_frame(POINTS_ROW)
        .into_iter()
        .map(|line| *read.entry(line).or_insert_with(|| points(&store.lines[line as usize], shapes, &prototypes)))
        .collect();
    stable_stretches(&values)
}

/// A POINTS line's number; None when a digit is less than DIGIT_LIKENESS alike to every digit shape.
fn points(glyphs: &[Glyph], shapes: &Shapes, prototypes: &[([f32; GLYPH_PIXELS], u8)]) -> Option<i64> {
    let (mut number_digits, mut sign) = (Vec::new(), 1);
    for glyph in glyphs {
        if !tall(glyph, AH) {
            if number_digits.is_empty() && glyph.width as f64 / glyph.height as f64 > MINUS_MIN_ASPECT {
                sign = -1;
            }
            continue;
        }
        let unit = shapes.unit_glyph(glyph.image);
        let (likeness, digit) = prototypes
            .iter()
            .map(|(prototype, digit)| (dot(&unit, prototype), *digit))
            .fold((f64::MIN, 0), |best, entry| if entry >= best { entry } else { best });
        if likeness < DIGIT_LIKENESS {
            number_digits.clear();
            break;
        }
        number_digits.push(digit);
    }
    value(&number_digits).map(|number| sign * number)
}

/// A hit's points and a miss's (None when no step went down): the most common step up and down.
fn step_points(changes: &[(i64, usize)]) -> Option<(i64, Option<i64>)> {
    let (mut ups, mut downs) = (Counter::new(), Counter::new());
    for &(change, _) in changes {
        if change > 0 {
            ups.add(change);
        } else if change < 0 {
            downs.add(change);
        }
    }
    Some((ups.top()?, downs.top()))
}

/// The hits and misses one POINTS change is (two hits, or a hit and a miss, can come in one step), when it is within
/// the tolerance of them.
fn step_events(change: i64, hit: i64, miss: Option<i64>) -> Option<(i64, i64)> {
    let most_misses = if miss.is_some() { MAX_STEP_EVENTS } else { 0 };
    let (error, hits, misses) = (0..=MAX_STEP_EVENTS)
        .flat_map(|hits| (0..=most_misses).map(move |misses| (hits, misses)))
        .filter(|&(hits, misses)| hits + misses > 0)
        .map(|(hits, misses)| ((change - hits * hit - misses * miss.unwrap_or(0)).abs(), hits, misses))
        .min()?;
    (error as f64 <= STEP_TOLERANCE_POINTS.max(STEP_TOLERANCE_SHARE * hit as f64)).then_some((hits, misses))
}
