//! The raw mouse log and what is measured from it (a port of python/mouse_read.py): each flick of a KovaaK's run,
//! matched with the run's stats file.
//!
//! In: a log's bytes (python/mouse_log.py writes them, as desktop/src/mouse.rs does) and, for a run, its stats file's
//! name and text. Out: the log's facts and summary, or the run's measures per kill (`MouseRun`), which the run page
//! shows (the service answers with them: service/src/mouse.rs, and src/wasm.rs in the browser) and the command-line
//! readers print. Pure: bytes and text in, measures out. The arithmetic follows Python's step by step, so the results
//! are the same to the bit (tests/mouse_parity.rs).
//!
//! File format (little endian), 24-byte records after a 32-byte header:
//!   header  "FFML", version 1 (u16), record size 24 (u16), QPC frequency, start QPC, start time_ns (i64 each)
//!   event   QPC (i64), dx, dy (i32), usFlags, usButtonFlags, usButtonData, device index (u16)
//!   device  -1 (i64), device handle (u64), device index (u32), 0 (u32): before the first event of each device
//!   stop    -2 (i64), stop QPC, stop time_ns (i64): appended when the logger stops
//!
//! Times are local where Python prints local time: the caller gives the UTC offset (local minus UTC, seconds), since
//! the core has no time zone of its own.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::python::{hypot, round};
use crate::statistics::median;
use crate::stats_file::lines;

/// The file's first bytes, and the format's version.
pub const MAGIC: [u8; 4] = *b"FFML";
pub const VERSION: u16 = 1;
/// Bytes in the header, and in each record after it.
pub const HEADER_SIZE: usize = 32;
pub const RECORD_SIZE: usize = 24;
/// A record's first field when it holds no event's QPC time: a device's record, or the stop record.
pub const KIND_DEVICE: i64 = -1;
pub const KIND_STOP: i64 = -2;
/// RAWMOUSE's usFlags bit for a move given as an absolute place, not counts moved: the reader skips those events.
pub const MOUSE_MOVE_ABSOLUTE: u16 = 0x01;
/// RAWMOUSE's usButtonFlags bits for the left button.
pub const LEFT_BUTTON_DOWN: u16 = 0x0001;
pub const LEFT_BUTTON_UP: u16 = 0x0002;

/// The speed profile's grid step, seconds (mouse_read.py: `DT`).
const GRID_STEP_S: f64 = 0.00025;
/// A kill matches a press within this, seconds, once the clock offset is added (`MATCH_TOL`).
const MATCH_TOLERANCE_S: f64 = 0.010;
/// The clock offset is searched within this, seconds (`SEARCH`): a press farther from every kill matches none.
const OFFSET_SEARCH_S: f64 = 1.0;
/// An event's motion is spread over at most MIN_SPREAD_S (seconds), or SPREAD_MEDIAN_INTERVALS median intervals if
/// that is longer.
const MIN_SPREAD_S: f64 = 0.001;
const SPREAD_MEDIAN_INTERVALS: f64 = 2.0;
/// The speed window is widened to this many median intervals when the events are farther apart than that.
const WINDOW_MEDIAN_INTERVALS: f64 = 2.0;
/// With this many events or more, the median interval counts: for the throttling warning and the speed window.
const MIN_EVENTS_FOR_RATE: usize = 200;
/// A median interval over this (seconds) means Windows probably throttled the logger.
const THROTTLED_INTERVAL_S: f64 = 0.002;
/// The busiest rate is counted over this long, seconds.
const BUSIEST_WINDOW_S: f64 = 0.1;
/// The sensitivity when neither the options nor the stats file give one.
const DEFAULT_DPI: f64 = 1600.0;
const DEFAULT_CM360: f64 = 70.0;
/// mouse_read.py's default speed window (ms), start and stop speeds (deg/s) and hold (ms).
const DEFAULT_WINDOW_MS: f64 = 4.0;
const DEFAULT_START_DEG_S: f64 = 30.0;
const DEFAULT_STOP_DEG_S: f64 = 10.0;
const DEFAULT_HOLD_MS: f64 = 5.0;
/// Centimeters in an inch, and degrees in a full turn.
const CM_PER_INCH: f64 = 2.54;
const DEGREES_PER_TURN: f64 = 360.0;
/// The parts `statistics.quantiles` cuts the values into for the deciles.
const DECILE_PARTS: usize = 10;
/// Seconds in a day, and microseconds in a second.
const SECONDS_PER_DAY: i64 = 86_400;
const MICROS_PER_SECOND: i64 = 1_000_000;
/// A time of day more than this (microseconds) after the stats file was written is from the day before: the run went
/// over midnight.
const MAX_AFTER_WRITTEN_MICROS: i64 = 3_600_000_000;
/// The stats file's name gives whole seconds, so the run ends up to this long (seconds) after it.
const NAME_STAMP_RESOLUTION_S: f64 = 1.0;
/// Without a challenge start, the run starts this long (seconds) before its first kill.
const START_BEFORE_FIRST_KILL_S: f64 = 1.0;
/// The date and time in a stats file's name ("2026.09.30-04.55.23"): 0 stands for any digit.
const NAME_STAMP_PATTERN: &[u8; 19] = b"0000.00.00-00.00.00";
/// Python's datetime takes years from 1 to this.
const MAX_YEAR: i64 = 9999;
/// Howard Hinnant's civil calendar: years in an era (the Gregorian cycle), the days in one, and the days from
/// 0000-03-01 to 1970-01-01.
const YEARS_PER_ERA: i64 = 400;
const DAYS_PER_ERA: i64 = 146_097;
const DAYS_TO_UNIX_EPOCH: i64 = 719_468;
/// Python's `format(x, "g")`: 6 significant digits, in fixed point for exponents from -4 up to that.
const G_SIGNIFICANT_DIGITS: i32 = 6;
const G_MIN_FIXED_EXPONENT: i32 = -4;

// ---- the file ----

/// A record (or the header) written field by field in the format's order; the bytes after the last field stay 0.
struct FieldWriter<const SIZE: usize> {
    bytes: [u8; SIZE],
    written: usize,
}

impl<const SIZE: usize> FieldWriter<SIZE> {
    fn new() -> Self {
        FieldWriter { bytes: [0; SIZE], written: 0 }
    }

    fn field(mut self, field: &[u8]) -> Self {
        self.bytes[self.written..self.written + field.len()].copy_from_slice(field);
        self.written += field.len();
        self
    }
}

/// The file's header: the QPC frequency and a (QPC, time_ns) pair taken at the start.
pub fn header(qpc_frequency: i64, start_qpc: i64, start_ns: i64) -> [u8; HEADER_SIZE] {
    FieldWriter::new()
        .field(&MAGIC)
        .field(&VERSION.to_le_bytes())
        .field(&(RECORD_SIZE as u16).to_le_bytes())
        .field(&qpc_frequency.to_le_bytes())
        .field(&start_qpc.to_le_bytes())
        .field(&start_ns.to_le_bytes())
        .bytes
}

/// One event: the QPC time it was handled, the counts moved (x right, y down) and RAWMOUSE's flags.
pub fn event(
    qpc: i64,
    x_counts: i32,
    y_counts: i32,
    flags: u16,
    button_flags: u16,
    button_data: u16,
    device: u16,
) -> [u8; RECORD_SIZE] {
    FieldWriter::new()
        .field(&qpc.to_le_bytes())
        .field(&x_counts.to_le_bytes())
        .field(&y_counts.to_le_bytes())
        .field(&flags.to_le_bytes())
        .field(&button_flags.to_le_bytes())
        .field(&button_data.to_le_bytes())
        .field(&device.to_le_bytes())
        .bytes
}

/// A device's record, written before its first event.
pub fn device(handle: u64, index: u32) -> [u8; RECORD_SIZE] {
    FieldWriter::new().field(&KIND_DEVICE.to_le_bytes()).field(&handle.to_le_bytes()).field(&index.to_le_bytes()).bytes
}

/// The stop record: a second (QPC, time_ns) pair.
pub fn stop(qpc: i64, ns: i64) -> [u8; RECORD_SIZE] {
    FieldWriter::new().field(&KIND_STOP.to_le_bytes()).field(&qpc.to_le_bytes()).field(&ns.to_le_bytes()).bytes
}

/// A record's (or the header's) fields, read in the format's order.
struct FieldReader<'a> {
    bytes: &'a [u8],
    read: usize,
}

impl<'a> FieldReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        FieldReader { bytes, read: 0 }
    }

    fn take<const SIZE: usize>(&mut self) -> [u8; SIZE] {
        let field = self.bytes[self.read..self.read + SIZE].try_into().unwrap();
        self.read += SIZE;
        field
    }

    fn i64(&mut self) -> i64 {
        i64::from_le_bytes(self.take())
    }

    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take())
    }

    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }
}

/// An event record: the QPC time it was handled, the counts moved (x right, y down), RAWMOUSE's flags and the
/// device's index.
struct Event {
    qpc: i64,
    x_counts: i32,
    y_counts: i32,
    flags: u16,
    button_flags: u16,
    button_data: u16,
    device: u16,
}

/// One record of the log.
enum Record {
    Event(Event),
    /// A device's handle.
    Device(u64),
    /// The stop pair: QPC and time_ns.
    Stop(i64, i64),
    /// A kind this version does not know (a negative QPC time): skipped.
    Unknown,
}

impl Record {
    fn read(bytes: &[u8]) -> Record {
        let mut fields = FieldReader::new(bytes);
        let qpc = fields.i64();
        match qpc {
            KIND_DEVICE => Record::Device(fields.u64()),
            KIND_STOP => Record::Stop(fields.i64(), fields.i64()),
            _ if qpc < 0 => Record::Unknown,
            _ => Record::Event(Event {
                qpc,
                x_counts: fields.i32(),
                y_counts: fields.i32(),
                flags: fields.u16(),
                button_flags: fields.u16(),
                button_data: fields.u16(),
                device: fields.u16(),
            }),
        }
    }
}

/// A time_ns as seconds.
fn seconds_of_ns(ns: i64) -> f64 {
    ns as f64 / 1e9
}

/// A log read into columns, one entry per event (mouse_log.py: `read_log`). Times are seconds since the start pair,
/// on the wall clock's scale (the QPC time is stretched by the drift between the start and stop pairs); without a stop
/// pair (the logger was killed) the scale is 1.
pub struct MouseLog {
    /// QPC counts a second.
    pub qpc_frequency: i64,
    /// The start pair's wall time, seconds since 1970.
    pub wall0: f64,
    /// The stop pair (QPC, time_ns), when the logger stopped cleanly.
    pub stop: Option<(i64, i64)>,
    /// The wall clock against QPC from the start pair to the stop pair, ms.
    pub drift_ms: Option<f64>,
    /// Seconds from the start pair to the stop pair, or to the last event without one.
    pub duration: f64,
    /// The devices' handles, by index.
    pub devices: Vec<u64>,
    pub times_s: Vec<f64>,
    /// The counts each event moved: x right, y down.
    pub x_counts: Vec<i32>,
    pub y_counts: Vec<i32>,
    /// RAWMOUSE's usFlags, usButtonFlags and usButtonData.
    pub flags: Vec<u16>,
    pub button_flags: Vec<u16>,
    pub button_data: Vec<u16>,
    /// The device each event came from: its index in `devices`.
    pub device_indexes: Vec<u16>,
}

impl MouseLog {
    fn empty(qpc_frequency: i64, wall0: f64) -> MouseLog {
        MouseLog {
            qpc_frequency,
            wall0,
            stop: None,
            drift_ms: None,
            duration: 0.0,
            devices: Vec::new(),
            times_s: Vec::new(),
            x_counts: Vec::new(),
            y_counts: Vec::new(),
            flags: Vec::new(),
            button_flags: Vec::new(),
            button_data: Vec::new(),
            device_indexes: Vec::new(),
        }
    }

    fn push_event(&mut self, event: &Event) {
        self.x_counts.push(event.x_counts);
        self.y_counts.push(event.y_counts);
        self.flags.push(event.flags);
        self.button_flags.push(event.button_flags);
        self.button_data.push(event.button_data);
        self.device_indexes.push(event.device);
    }

    /// The events' times and the log's duration from their QPC times, on the wall clock's scale when there is a stop
    /// pair.
    fn set_times(&mut self, event_qpcs: &[i64], start_qpc: i64, start_ns: i64) {
        let mut scale = 1.0;
        if let Some((stop_qpc, stop_ns)) = self.stop
            && stop_qpc > start_qpc
        {
            let qpc_span_s = (stop_qpc - start_qpc) as f64 / self.qpc_frequency as f64;
            let wall_span_s = seconds_of_ns(stop_ns - start_ns);
            scale = wall_span_s / qpc_span_s;
            self.drift_ms = Some((wall_span_s - qpc_span_s) * 1e3);
        }
        let seconds_per_count = scale / self.qpc_frequency as f64;
        let since_start_s = |qpc: i64| (qpc - start_qpc) as f64 * seconds_per_count;
        self.duration = match (self.stop, event_qpcs.last()) {
            (Some((stop_qpc, _)), _) => since_start_s(stop_qpc),
            (None, Some(&last_qpc)) => since_start_s(last_qpc),
            (None, None) => 0.0,
        };
        self.times_s = event_qpcs.iter().map(|&qpc| since_start_s(qpc)).collect();
    }

    /// The events from the device at `index` in `devices`.
    pub fn device_events(&self, index: usize) -> usize {
        self.device_indexes.iter().filter(|&&device| usize::from(device) == index).count()
    }
}

/// The start of a log: its QPC frequency, start QPC and start time_ns; None when it is not a version 1 mouse log.
pub fn read_header(bytes: &[u8]) -> Option<(i64, i64, i64)> {
    if bytes.len() < HEADER_SIZE {
        return None;
    }
    let mut fields = FieldReader::new(bytes);
    let magic: [u8; 4] = fields.take();
    let known = magic == MAGIC && fields.u16() == VERSION && usize::from(fields.u16()) == RECORD_SIZE;
    known.then(|| (fields.i64(), fields.i64(), fields.i64()))
}

/// Reads a log (a partial last record is left out).
pub fn read_log(bytes: &[u8]) -> Result<MouseLog, String> {
    let (qpc_frequency, start_qpc, start_ns) =
        read_header(bytes).ok_or(format!("not a version {VERSION} mouse log"))?;
    if qpc_frequency <= 0 {
        return Err("the log's QPC frequency is not positive".into());
    }
    let whole_records = (bytes.len() - HEADER_SIZE) / RECORD_SIZE * RECORD_SIZE;
    let mut log = MouseLog::empty(qpc_frequency, seconds_of_ns(start_ns));
    let mut event_qpcs = Vec::new();
    for record in bytes[HEADER_SIZE..HEADER_SIZE + whole_records].chunks_exact(RECORD_SIZE) {
        match Record::read(record) {
            Record::Event(event) => {
                event_qpcs.push(event.qpc);
                log.push_event(&event);
            }
            Record::Device(handle) => log.devices.push(handle),
            Record::Stop(stop_qpc, stop_ns) => log.stop = Some((stop_qpc, stop_ns)),
            Record::Unknown => {}
        }
    }
    log.set_times(&event_qpcs, start_qpc, start_ns);
    Ok(log)
}

/// Events a second in the busiest window of `window_s` seconds (`times_s` in order).
pub fn busiest_rate(times_s: &[f64], window_s: f64) -> f64 {
    let (mut most_events, mut first) = (0usize, 0usize);
    for (last, &time_s) in times_s.iter().enumerate() {
        while time_s - times_s[first] > window_s {
            first += 1;
        }
        most_events = most_events.max(last - first + 1);
    }
    most_events as f64 / window_s
}

/// The time between each event and the next, seconds.
fn intervals_s(times_s: &[f64]) -> Vec<f64> {
    times_s.windows(2).map(|pair| pair[1] - pair[0]).collect()
}

// ---- Python's small helpers ----

/// Python's `max(a, b)`: b only when it is larger.
fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Python's `min(a, b)`: b only when it is smaller.
fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `bisect.bisect_right(a, x, lo)`.
fn bisect_right(a: &[f64], x: f64, lo: usize) -> usize {
    lo + a[lo..].partition_point(|&value| value <= x)
}

/// `bisect.bisect_left(a, x)`.
fn bisect_left(a: &[f64], x: f64) -> usize {
    a.partition_point(|&value| value < x)
}

/// Python's `sum` of floats (3.12 on): Neumaier's compensated sum.
fn py_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut sum, mut compensation) = (0.0f64, 0.0f64);
    for x in values {
        let total = sum + x;
        if sum.abs() >= x.abs() {
            compensation += (sum - total) + x;
        } else {
            compensation += (x - total) + sum;
        }
        sum = total;
    }
    if compensation != 0.0 && compensation.is_finite() { sum + compensation } else { sum }
}

/// `statistics.quantiles(values, n=10, method="inclusive")[round(share * 10) - 1]`, or the value when there is one
/// (mouse_read.py: `q`).
fn decile(values: &[f64], share: f64) -> f64 {
    if values.len() < 2 {
        return values[0];
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let last_index = sorted.len() - 1;
    let cut = (share * DECILE_PARTS as f64).round_ties_even() as usize;
    let (j, delta) = (cut * last_index / DECILE_PARTS, cut * last_index % DECILE_PARTS);
    (sorted[j] * (DECILE_PARTS - delta) as f64 + sorted[j + 1] * delta as f64) / DECILE_PARTS as f64
}

/// A time (seconds since 1970) as local "%H:%M:%S.%f" (Python's `datetime.fromtimestamp(epoch_s)`: microseconds
/// rounded half to even).
pub fn local_clock(epoch_s: f64, utc_offset: i64) -> String {
    let mut whole_s = epoch_s.trunc();
    let mut micros = ((epoch_s - whole_s) * 1e6).round_ties_even();
    if micros >= 1e6 {
        micros -= 1e6;
        whole_s += 1.0;
    } else if micros < 0.0 {
        micros += 1e6;
        whole_s -= 1.0;
    }
    let second_of_day = (whole_s as i64 + utc_offset).rem_euclid(SECONDS_PER_DAY);
    let (hours, minutes, seconds) = (second_of_day / 3600, second_of_day / 60 % 60, second_of_day % 60);
    format!("{hours:02}:{minutes:02}:{seconds:02}.{:06}", micros as i64)
}

/// Degrees per count: 360 / (cm360 / 2.54 * dpi).
pub fn deg_per_count(dpi: f64, cm360: f64) -> f64 {
    DEGREES_PER_TURN / (cm360 / CM_PER_INCH * dpi)
}

// ---- the motion ----

/// The log's relative motion as cumulative degrees after each event (absolute events are skipped). Between events
/// the position is interpolated: each event's motion is spread evenly over the time since the event before it, but
/// over at most `spread_s` (MIN_SPREAD_S, or SPREAD_MEDIAN_INTERVALS median intervals if that is longer), so a report
/// never counts as all in or all out of a speed window.
struct Motion {
    times_s: Vec<f64>,
    x_deg: Vec<f64>,
    y_deg: Vec<f64>,
    spread_s: f64,
}

impl Motion {
    fn new(log: &MouseLog, degrees_per_count: f64) -> Motion {
        let events = log.times_s.len();
        let mut motion = Motion {
            times_s: Vec::with_capacity(events),
            x_deg: Vec::with_capacity(events),
            y_deg: Vec::with_capacity(events),
            spread_s: MIN_SPREAD_S,
        };
        let (mut x, mut y) = (0.0f64, 0.0f64);
        for i in 0..events {
            if log.flags[i] & MOUSE_MOVE_ABSOLUTE != 0 {
                continue;
            }
            x += log.x_counts[i] as f64 * degrees_per_count;
            y -= log.y_counts[i] as f64 * degrees_per_count;
            motion.times_s.push(log.times_s[i]);
            motion.x_deg.push(x);
            motion.y_deg.push(y);
        }
        let intervals = intervals_s(&motion.times_s);
        if !intervals.is_empty() {
            motion.spread_s = py_max(MIN_SPREAD_S, SPREAD_MEDIAN_INTERVALS * median(&intervals));
        }
        motion
    }

    /// The position after event i - 1 (the start before the first event), degrees.
    fn position_before(&self, i: usize) -> (f64, f64) {
        if i > 0 { (self.x_deg[i - 1], self.y_deg[i - 1]) } else { (0.0, 0.0) }
    }

    /// The position at `time_s`, degrees; `next_event` is the index just past the last event at or before it, if
    /// known.
    fn position_at(&self, time_s: f64, next_event: Option<usize>) -> (f64, f64) {
        let i = next_event.unwrap_or_else(|| bisect_right(&self.times_s, time_s, 0));
        let (x_before, y_before) = self.position_before(i);
        if i >= self.times_s.len() {
            return (x_before, y_before);
        }
        let event_s = self.times_s[i];
        let spread_from_s =
            if i > 0 { py_max(self.times_s[i - 1], event_s - self.spread_s) } else { event_s - self.spread_s };
        if time_s <= spread_from_s {
            return (x_before, y_before);
        }
        let share = (time_s - spread_from_s) / (event_s - spread_from_s);
        (x_before + share * (self.x_deg[i] - x_before), y_before + share * (self.y_deg[i] - y_before))
    }

    /// The speed profile from `from_s` to `click_s`, the grid ending exactly at the click: at each grid time t, the
    /// speed over [t - window_s / 2, t + window_s / 2], cut at the click.
    fn speeds(&self, from_s: f64, click_s: f64, window_s: f64) -> SpeedProfile {
        let steps = ((click_s - from_s) / GRID_STEP_S) as i64;
        let points = (steps + 1).max(0) as usize;
        let mut profile =
            SpeedProfile { times_s: Vec::with_capacity(points), speeds_deg_s: Vec::with_capacity(points) };
        let (mut after_start, mut after_end) = (0, 0);
        for step in 0..=steps {
            let time_s = click_s - (steps - step) as f64 * GRID_STEP_S;
            let (start_s, end_s) = (time_s - window_s / 2.0, py_min(time_s + window_s / 2.0, click_s));
            after_start = bisect_right(&self.times_s, start_s, after_start);
            after_end = bisect_right(&self.times_s, end_s, after_end);
            let (start_x, start_y) = self.position_at(start_s, Some(after_start));
            let (end_x, end_y) = self.position_at(end_s, Some(after_end));
            profile.times_s.push(time_s);
            profile.speeds_deg_s.push(hypot(end_x - start_x, end_y - start_y) / (end_s - start_s));
        }
        profile
    }
}

/// Speeds on the analysis grid up to a click: each grid time (seconds since the log's start) and the speed there.
struct SpeedProfile {
    times_s: Vec<f64>,
    speeds_deg_s: Vec<f64>,
}

/// A flick in a speed profile: when the mouse starts moving, the peak's grid index, when it stops and when it settles
/// (the start of the final still stretch), seconds since the log's start; None where there is none.
struct FlickTimes {
    start_s: Option<f64>,
    peak: usize,
    stop_s: Option<f64>,
    settle_s: Option<f64>,
}

impl SpeedProfile {
    /// When the speed crosses `threshold_deg_s` between grid points i - 1 and i (linear), so times are not rounded to
    /// the grid.
    fn crossing_s(&self, i: usize, threshold_deg_s: f64) -> f64 {
        let (times, speeds) = (&self.times_s, &self.speeds_deg_s);
        if i == 0 || speeds[i] == speeds[i - 1] {
            return times[i];
        }
        let share = py_min(1.0, py_max(0.0, (threshold_deg_s - speeds[i - 1]) / (speeds[i] - speeds[i - 1])));
        times[i - 1] + share * (times[i] - times[i - 1])
    }

    /// The start, peak, stop and settle of the flick that ends at the click (mouse_read.py: `find`).
    fn flick(&self, options: &Options) -> FlickTimes {
        let start = self.start_index(options);
        let peak = self.peak_index(start.unwrap_or(0));
        let stop = self.stop_index(peak, options);
        let settle = self.settle_index(options.stop);
        let crossing = |index: Option<usize>, threshold: f64| index.map(|i| self.crossing_s(i, threshold));
        FlickTimes {
            start_s: crossing(start, options.start),
            peak,
            stop_s: crossing(stop, options.stop),
            settle_s: crossing(settle, options.stop),
        }
    }

    /// The first grid point at the start speed; when the profile begins at it, the first after a point under the stop
    /// speed (0 when the speed never drops).
    fn start_index(&self, options: &Options) -> Option<usize> {
        let speeds = &self.speeds_deg_s;
        let first_fast_from = |from: usize| (from..speeds.len()).find(|&i| speeds[i] >= options.start);
        if speeds[0] >= options.start {
            // still moving from the last flick: a new start needs a stop first
            match (0..speeds.len()).find(|&i| speeds[i] < options.stop) {
                None => Some(0),
                Some(slow) => first_fast_from(slow),
            }
        } else {
            first_fast_from(0)
        }
    }

    /// The first grid point of the highest speed from `from` on.
    fn peak_index(&self, from: usize) -> usize {
        let speeds = &self.speeds_deg_s;
        let mut peak = from;
        for i in from + 1..speeds.len() {
            if speeds[i] > speeds[peak] {
                peak = i;
            }
        }
        peak
    }

    /// The first grid point from the peak on where the speed stays under the stop speed for the hold (or until the
    /// click, if that comes sooner).
    fn stop_index(&self, peak: usize, options: &Options) -> Option<usize> {
        let points = self.speeds_deg_s.len();
        let hold_points = ((options.hold / 1000.0 / GRID_STEP_S).round_ties_even() as i64).max(1) as usize;
        let still = self.still_points(options.stop);
        (peak..points).find(|&i| still[i] > 0 && still[i] >= (hold_points + 1).min(points - i))
    }

    /// For each grid point, the points in a row under `stop_deg_s` from it on (0 where the speed is not under it).
    fn still_points(&self, stop_deg_s: f64) -> Vec<usize> {
        let speeds = &self.speeds_deg_s;
        let mut still = vec![0usize; speeds.len() + 1];
        for i in (0..speeds.len()).rev() {
            still[i] = if speeds[i] < stop_deg_s { still[i + 1] + 1 } else { 0 };
        }
        still
    }

    /// The first grid point of the final stretch under `stop_deg_s`; None when the click came while moving.
    fn settle_index(&self, stop_deg_s: f64) -> Option<usize> {
        let below = |i: usize| self.speeds_deg_s[i] < stop_deg_s;
        let mut settle = self.speeds_deg_s.len() - 1;
        if !below(settle) {
            return None;
        }
        while settle > 0 && below(settle - 1) {
            settle -= 1;
        }
        Some(settle)
    }

    /// The times the speed rose to `stop_deg_s` again after `after_s`.
    fn rises_after(&self, after_s: f64, stop_deg_s: f64) -> usize {
        let (times, speeds) = (&self.times_s, &self.speeds_deg_s);
        let rises = |i: usize| times[i] > after_s && speeds[i] >= stop_deg_s && stop_deg_s > speeds[i - 1];
        (1..speeds.len()).filter(|&i| rises(i)).count()
    }
}

/// The reader's settings: mouse_read.py's options, with its defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// The sensitivity; None (or 0): the stats file's, else 1600 dpi and 70 cm/360.
    pub dpi: Option<f64>,
    pub cm360: Option<f64>,
    /// The speed window, ms.
    pub window: f64,
    /// A flick starts at this speed and stops under that one, deg/s.
    pub start: f64,
    pub stop: f64,
    /// A stop holds under the stop speed this long, ms.
    pub hold: f64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            dpi: None,
            cm360: None,
            window: DEFAULT_WINDOW_MS,
            start: DEFAULT_START_DEG_S,
            stop: DEFAULT_STOP_DEG_S,
            hold: DEFAULT_HOLD_MS,
        }
    }
}

/// A sensitivity in cm/360: the mouse's dots per inch, and the centimeters it moves for a full turn.
#[derive(Clone, Copy, Debug)]
pub struct Sensitivity {
    pub dpi: f64,
    pub cm360: f64,
}

impl Options {
    /// The options' sensitivity, else `fallback`'s, else the defaults, a part at a time (0 counts as not given, as
    /// mouse_read.py's `o.dpi or ...` reads it).
    fn sensitivity_or(&self, fallback: Option<Sensitivity>) -> Sensitivity {
        Sensitivity {
            dpi: self.given_dpi().unwrap_or(fallback.map_or(DEFAULT_DPI, |given| given.dpi)),
            cm360: self.given_cm360().unwrap_or(fallback.map_or(DEFAULT_CM360, |given| given.cm360)),
        }
    }

    fn given_dpi(&self) -> Option<f64> {
        self.dpi.filter(|&dpi| dpi != 0.0)
    }

    fn given_cm360(&self) -> Option<f64> {
        self.cm360.filter(|&cm360| cm360 != 0.0)
    }
}

/// One kill's measures, from the previous kill's press to its own (mouse_read.py: the rows of `<log>.kills.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[expect(clippy::min_ident_chars, reason = "`n` is the JSON's key, which mouse_read.py writes and the run page reads")]
pub struct KillMeasure {
    /// The kill's number in the stats file.
    pub n: i64,
    /// The kill's time in the stats file, and the press's, local.
    pub kill_local: String,
    pub press_local: String,
    /// The press, seconds since the log's start.
    pub press_s: f64,
    /// The press minus the kill (after the offset), ms.
    pub gap_ms: f64,
    pub shots: i64,
    pub start_s: Option<f64>,
    pub stop_s: Option<f64>,
    pub settle_s: Option<f64>,
    /// From the previous press until the mouse starts moving, ms.
    pub reaction_ms: Option<f64>,
    /// From the start until the mouse stops, ms.
    pub flick_ms: Option<f64>,
    pub peak_dps: f64,
    pub peak_ms: f64,
    pub stop_to_click_ms: Option<f64>,
    /// How long the speed stayed under the stop speed right before the click, ms (0 when the click came while moving).
    pub still_ms: f64,
    pub click_dps: f64,
    /// The times the speed rose to the stop speed again after the stop.
    pub corrections: Option<usize>,
    /// How far the crosshair moved from the previous press, degrees.
    pub dist_deg: f64,
}

impl KillMeasure {
    /// A kill's own fields, before its flick is measured: which kill it is, and its press.
    fn unmeasured(kill: &StatsKill, press_local: String, press_s: f64, gap_ms: f64) -> KillMeasure {
        KillMeasure {
            n: kill.number,
            kill_local: kill.local_time.clone(),
            press_local,
            press_s,
            gap_ms,
            shots: kill.shots,
            start_s: None,
            stop_s: None,
            settle_s: None,
            reaction_ms: None,
            flick_ms: None,
            peak_dps: 0.0,
            peak_ms: 0.0,
            stop_to_click_ms: None,
            still_ms: 0.0,
            click_dps: 0.0,
            corrections: None,
            dist_deg: 0.0,
        }
    }
}

/// Python's `round(seconds * 1000, 3)`: seconds as milliseconds.
fn rounded_ms(seconds: f64) -> f64 {
    round(seconds * 1000.0, 3)
}

/// Measures a kill's flick, from the previous press (`from_s`) to its own (`press_s`), both seconds since the log's
/// start; `kill` gives the kill's own fields.
fn measure(
    motion: &Motion,
    from_s: f64,
    press_s: f64,
    options: &Options,
    kill: KillMeasure,
) -> Result<KillMeasure, String> {
    let profile = motion.speeds(from_s, press_s, options.window / 1000.0);
    let speeds = &profile.speeds_deg_s;
    if speeds.is_empty() {
        return Err(format!("kill {}: its press comes before the press before it", kill.n));
    }
    let flick = profile.flick(options);
    let ((from_x, from_y), (press_x, press_y)) = (motion.position_at(from_s, None), motion.position_at(press_s, None));
    Ok(KillMeasure {
        start_s: flick.start_s,
        stop_s: flick.stop_s,
        settle_s: flick.settle_s,
        reaction_ms: flick.start_s.map(|start_s| rounded_ms(start_s - from_s)),
        flick_ms: flick.start_s.zip(flick.stop_s).map(|(start_s, stop_s)| rounded_ms(stop_s - start_s)),
        peak_dps: round(speeds[flick.peak], 1),
        peak_ms: rounded_ms(profile.times_s[flick.peak] - from_s),
        stop_to_click_ms: flick.stop_s.map(|stop_s| rounded_ms(press_s - stop_s)),
        still_ms: flick.settle_s.map_or(0.0, |settle_s| rounded_ms(press_s - settle_s)),
        click_dps: round(speeds[speeds.len() - 1], 1),
        corrections: flick.stop_s.map(|stop_s| profile.rises_after(stop_s, options.stop)),
        dist_deg: round(hypot(press_x - from_x, press_y - from_y), 3),
        ..kill
    })
}

/// The left-button presses' times, seconds since the log's start.
pub fn presses_of(log: &MouseLog) -> Vec<f64> {
    (0..log.times_s.len()).filter(|&i| log.button_flags[i] & LEFT_BUTTON_DOWN != 0).map(|i| log.times_s[i]).collect()
}

// ---- the log on its own ----

/// A device in the log: its handle and its events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceFacts {
    pub handle: u64,
    pub events: usize,
}

/// What any log says (mouse_read.py: `head` and `rate_line`): its span, events, clock drift, devices and rate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogFacts {
    /// The start's wall time, seconds since 1970.
    pub wall0: f64,
    pub start_local: String,
    pub end_local: String,
    pub duration: f64,
    pub events: usize,
    /// The wall clock against QPC over the log, ms; None without a stop pair (the logger was killed).
    pub drift_ms: Option<f64>,
    pub devices: Vec<DeviceFacts>,
    /// Absolute events (MOUSE_MOVE_ABSOLUTE), skipped.
    pub absolute: usize,
    /// The median interval between events, s; None with fewer than two events.
    pub median_interval: Option<f64>,
    /// Events a second in the busiest 100 ms.
    pub busiest_hz: f64,
    /// The events are far apart (median over 2 ms in 200 or more): Windows probably throttled the logger.
    pub throttled: bool,
}

/// The log's facts, its times local for `utc_offset` (local minus UTC, seconds).
pub fn log_facts(log: &MouseLog, utc_offset: i64) -> LogFacts {
    let intervals = intervals_s(&log.times_s);
    let median_interval = (!intervals.is_empty()).then(|| median(&intervals));
    let events = log.times_s.len();
    LogFacts {
        wall0: log.wall0,
        start_local: local_clock(log.wall0, utc_offset),
        end_local: local_clock(log.wall0 + log.duration, utc_offset),
        duration: log.duration,
        events,
        drift_ms: log.drift_ms,
        devices: log
            .devices
            .iter()
            .enumerate()
            .map(|(index, &handle)| DeviceFacts { handle, events: log.device_events(index) })
            .collect(),
        absolute: log.flags.iter().filter(|&&flags| flags & MOUSE_MOVE_ABSOLUTE != 0).count(),
        median_interval,
        busiest_hz: busiest_rate(&log.times_s, BUSIEST_WINDOW_S),
        throttled: median_interval
            .is_some_and(|interval_s| events >= MIN_EVENTS_FOR_RATE && interval_s > THROTTLED_INTERVAL_S),
    }
}

/// A log without its run (mouse_read.py's summary mode): its facts, the total travel and the left-button presses.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogSummary {
    pub log: LogFacts,
    pub dpi: f64,
    pub cm360: f64,
    pub deg_per_count: f64,
    /// The travel in 1 ms steps, degrees.
    pub travel_deg: f64,
    pub presses: usize,
}

/// Sums up a log on its own, with the options' sensitivity (else the defaults).
pub fn summary(log: &MouseLog, options: &Options, utc_offset: i64) -> LogSummary {
    let Sensitivity { dpi, cm360 } = options.sensitivity_or(None);
    let degrees_per_count = deg_per_count(dpi, cm360);
    LogSummary {
        log: log_facts(log, utc_offset),
        dpi,
        cm360,
        deg_per_count: degrees_per_count,
        travel_deg: travel_counts(log) * degrees_per_count,
        presses: presses_of(log).len(),
    }
}

/// The relative motion's length in counts, in 1 ms steps: each millisecond's counts added up, then their lengths
/// summed in the order the milliseconds first appear (as a Python dict keeps them).
fn travel_counts(log: &MouseLog) -> f64 {
    let (mut order, mut steps) = (Vec::new(), HashMap::<i64, [i64; 2]>::new());
    for i in 0..log.times_s.len() {
        if log.flags[i] & MOUSE_MOVE_ABSOLUTE != 0 {
            continue;
        }
        let millisecond = (log.times_s[i] * 1000.0) as i64;
        let step = steps.entry(millisecond).or_insert_with(|| {
            order.push(millisecond);
            [0, 0]
        });
        step[0] += i64::from(log.x_counts[i]);
        step[1] += i64::from(log.y_counts[i]);
    }
    py_sum(order.iter().map(|millisecond| hypot(steps[millisecond][0] as f64, steps[millisecond][1] as f64)))
}

// ---- the stats file ----

/// A kill in the stats file: its number, its local time as written, that time in seconds since 1970, and its shots.
#[derive(Clone, Debug)]
pub struct StatsKill {
    pub number: i64,
    pub local_time: String,
    pub epoch_s: f64,
    pub shots: i64,
}

/// What the reader takes from a stats file (mouse_read.py: `read_stats`): the kills, the run's start and end (seconds
/// since 1970), the shots, the sensitivity when it is in cm/360, and the scenario.
#[derive(Clone, Debug)]
pub struct StatsRun {
    pub kills: Vec<StatsKill>,
    pub start_epoch_s: f64,
    pub end_epoch_s: f64,
    pub shots: i64,
    pub sensitivity: Option<Sensitivity>,
    pub scenario: String,
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's `days_from_civil`, counting years from March so that a
/// leap day ends the year).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(YEARS_PER_ERA);
    let year_of_era = year - era * YEARS_PER_ERA;
    // the days before the month, counted from March: its lengths follow (153 * month + 2) / 5
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * DAYS_PER_ERA + day_of_era - DAYS_TO_UNIX_EPOCH
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The date in a stats file's name ("... - 2026.09.30-04.55.23 Stats.csv"), as local seconds since 1970; None when
/// the name's first such stamp is no valid date.
fn name_stamp(name: &str) -> Option<i64> {
    let bytes = name.as_bytes();
    let stamp_at = |from: usize| {
        let mut pattern = bytes[from..from + NAME_STAMP_PATTERN.len()].iter().zip(NAME_STAMP_PATTERN);
        pattern.all(|(&byte, &want)| if want == b'0' { byte.is_ascii_digit() } else { byte == want })
    };
    let at = (0..bytes.len().saturating_sub(NAME_STAMP_PATTERN.len() - 1)).find(|&from| stamp_at(from))?;
    let number = |from: usize, to: usize| name[at + from..at + to].parse::<i64>().unwrap();
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let (hour, minute, second) = (number(11, 13), number(14, 16), number(17, 19));
    let valid = (1..=MAX_YEAR).contains(&year)
        && (1..=12).contains(&month)
        && day >= 1
        && day <= days_in_month(year, month)
        && hour < 24
        && minute < 60
        && second < 60;
    valid.then(|| days_from_civil(year, month, day) * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second)
}

/// Whether every character of `text` is an ASCII digit (true when it has none).
fn all_digits(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii_digit())
}

/// A time of day as `strptime(text, "%H:%M:%S.%f")` reads it (the fraction 1 to 6 digits), in microseconds; None
/// where it fails.
fn clock_micros(text: &str) -> Option<i64> {
    let (hours_minutes_seconds, fraction) = text.split_once('.')?;
    let mut parts = hours_minutes_seconds.split(':');
    let mut field = |max: i64| -> Option<i64> {
        let digits = parts.next()?;
        if digits.is_empty() || digits.len() > 2 || !all_digits(digits) {
            return None;
        }
        let value: i64 = digits.parse().ok()?;
        (value <= max).then_some(value)
    };
    let (hours, minutes, seconds) = (field(23)?, field(59)?, field(59)?);
    let fraction_read = !fraction.is_empty() && fraction.len() <= 6 && all_digits(fraction);
    if parts.next().is_some() || !fraction_read {
        return None;
    }
    let micros: i64 = format!("{fraction:0<6}").parse().ok()?;
    Some(((hours * 60 + minutes) * 60 + seconds) * MICROS_PER_SECOND + micros)
}

/// Python's `int(text)` for plain decimal text.
fn py_int(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

/// Python's `float(text)`.
fn py_float(text: &str) -> Option<f64> {
    text.trim().parse().ok()
}

/// Turns a stats file's times of day into seconds since 1970: on the date in its name (the day before for a run over
/// midnight), less the UTC offset.
struct StatsClock {
    /// The date and time in the name, local seconds since 1970: when the stats were written, about the run's end.
    written_s: i64,
    /// Local minus UTC, seconds.
    utc_offset: i64,
}

impl StatsClock {
    fn epoch_s(&self, time_of_day: &str) -> Result<f64, String> {
        let time_of_day = time_of_day.trim();
        let time_of_day_micros = clock_micros(time_of_day)
            .ok_or_else(|| format!("time data {time_of_day:?} does not match format '%H:%M:%S.%f'"))?;
        let written_micros = self.written_s * MICROS_PER_SECOND;
        let day_micros = self.written_s.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY * MICROS_PER_SECOND;
        let mut local_micros = day_micros + time_of_day_micros;
        if local_micros > written_micros + MAX_AFTER_WRITTEN_MICROS {
            local_micros -= SECONDS_PER_DAY * MICROS_PER_SECOND;
        }
        let seconds = local_micros.div_euclid(MICROS_PER_SECOND);
        let micros_of_second = local_micros.rem_euclid(MICROS_PER_SECOND);
        Ok((seconds - self.utc_offset) as f64 + micros_of_second as f64 / 1e6)
    }

    /// The run's end: the end of the second the stats were written in.
    fn end_epoch_s(&self) -> f64 {
        (self.written_s - self.utc_offset) as f64 + NAME_STAMP_RESOLUTION_S
    }
}

/// The kill table's rows, after its header and up to the first blank line.
fn read_kills(lines: &[&str], clock: &StatsClock) -> Result<Vec<StatsKill>, String> {
    let mut kills = Vec::new();
    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            break;
        }
        let cells: Vec<&str> = line.split(',').collect();
        let unreadable = || format!("a kill row the reader cannot read: {line:?}");
        let number = py_int(cells[0]).ok_or_else(unreadable)?;
        let local_time = cells.get(1).ok_or_else(unreadable)?.to_string();
        let epoch_s = clock.epoch_s(&local_time)?;
        let shots = cells.get(5).and_then(|cell| py_int(cell)).ok_or_else(unreadable)?;
        kills.push(StatsKill { number, local_time, epoch_s, shots });
    }
    Ok(kills)
}

/// The shots in the weapon table (the rows after its "Weapon,Shots" header, up to a blank line); 0 without one.
fn weapon_shots(lines: &[&str]) -> Result<i64, String> {
    let Some(header) = lines.iter().position(|line| line.starts_with("Weapon,Shots")) else {
        return Ok(0);
    };
    let mut shots = 0;
    for line in &lines[header + 1..] {
        if line.trim().is_empty() {
            break;
        }
        let row_shots = line.split(',').nth(1).and_then(py_float).filter(|row_shots| row_shots.is_finite());
        shots += row_shots.ok_or_else(|| format!("a weapon row the reader cannot read: {line:?}"))?.trunc() as i64;
    }
    Ok(shots)
}

/// The stats file's sensitivity, when its scale is cm/360.
fn stats_sensitivity(meta: &HashMap<&str, &str>) -> Result<Option<Sensitivity>, String> {
    if meta.get("Sens Scale").is_none_or(|scale| scale.trim() != "cm/360") {
        return Ok(None);
    }
    let number = |key: &str| {
        meta.get(key).and_then(|value| py_float(value)).ok_or(format!("the stats file's {key} cannot be read"))
    };
    Ok(Some(Sensitivity { dpi: number("DPI")?, cm360: number("Horiz Sens")? }))
}

/// Reads a stats file. name: its file name (it holds the date); utc_offset: local minus UTC, seconds.
pub fn read_stats(name: &str, text: &str, utc_offset: i64) -> Result<StatsRun, String> {
    let lines = lines(text);
    // the "Key:,value" lines (a later line wins)
    let meta: HashMap<&str, &str> = lines.iter().filter_map(|line| line.split_once(":,")).collect();
    let written_s =
        name_stamp(name).ok_or("the stats file name holds no date (expected '... - 2026.09.30-04.55.23 Stats.csv')")?;
    let clock = StatsClock { written_s, utc_offset };
    let kills = read_kills(&lines, &clock)?;
    let shots = weapon_shots(&lines)?;
    let sensitivity = stats_sensitivity(&meta)?;
    let start_epoch_s = match meta.get("Challenge Start") {
        Some(start) => clock.epoch_s(start)?,
        None => {
            let first = kills.first().ok_or("the stats file has no kills and no challenge start")?;
            first.epoch_s - START_BEFORE_FIRST_KILL_S
        }
    };
    Ok(StatsRun {
        kills,
        start_epoch_s,
        end_epoch_s: clock.end_epoch_s(),
        shots,
        sensitivity,
        scenario: meta.get("Scenario").map_or(String::new(), |scenario| scenario.trim().to_string()),
    })
}

// ---- the run ----

/// A press within OFFSET_SEARCH_S of a kill: the press's time minus the kill's (seconds), and the kill's index.
struct PressNearKill {
    gap_s: f64,
    kill: usize,
}

/// Every press within OFFSET_SEARCH_S of each kill, by gap, then by kill.
fn presses_near_kills(presses_s: &[f64], kills_s: &[f64]) -> Vec<PressNearKill> {
    let mut near = Vec::new();
    for (kill, &kill_s) in kills_s.iter().enumerate() {
        let presses =
            bisect_left(presses_s, kill_s - OFFSET_SEARCH_S)..bisect_right(presses_s, kill_s + OFFSET_SEARCH_S, 0);
        near.extend(presses.map(|press| PressNearKill { gap_s: presses_s[press] - kill_s, kill }));
    }
    near.sort_by(|a, b| a.gap_s.partial_cmp(&b.gap_s).unwrap_or(Ordering::Equal).then(a.kill.cmp(&b.kill)));
    near
}

/// The first of the stretches of `near` no wider than MATCH_TOLERANCE_S that hold the most distinct kills; None when
/// `near` is empty.
fn densest_stretch(near: &[PressNearKill], kill_count: usize) -> Option<RangeInclusive<usize>> {
    let mut presses_of_kill = vec![0usize; kill_count];
    let (mut kills_in_stretch, mut first) = (0usize, 0usize);
    let (mut most_kills, mut densest) = (0usize, 0..=0);
    for (last, press) in near.iter().enumerate() {
        presses_of_kill[press.kill] += 1;
        if presses_of_kill[press.kill] == 1 {
            kills_in_stretch += 1;
        }
        while press.gap_s - near[first].gap_s > MATCH_TOLERANCE_S {
            presses_of_kill[near[first].kill] -= 1;
            if presses_of_kill[near[first].kill] == 0 {
                kills_in_stretch -= 1;
            }
            first += 1;
        }
        if kills_in_stretch > most_kills {
            (most_kills, densest) = (kills_in_stretch, first..=last);
        }
    }
    (most_kills > 0).then_some(densest)
}

/// The press nearest the kill at `kill_s` plus `offset_s`, within MATCH_TOLERANCE_S (the first of equals).
fn nearest_press(presses_s: &[f64], kill_s: f64, offset_s: f64) -> Option<usize> {
    let near = bisect_left(presses_s, kill_s + offset_s - MATCH_TOLERANCE_S)
        ..bisect_right(presses_s, kill_s + offset_s + MATCH_TOLERANCE_S, 0);
    let distance = |press: usize| (presses_s[press] - kill_s - offset_s).abs();
    near.reduce(|a, b| if distance(b) < distance(a) { b } else { a })
}

/// The offset (log time minus stats time, seconds) that puts the most kills within MATCH_TOLERANCE_S of a press, and
/// each kill's press index (None when none is within MATCH_TOLERANCE_S).
fn match_kills(presses_s: &[f64], kills_s: &[f64]) -> (Option<f64>, Vec<Option<usize>>) {
    let near = presses_near_kills(presses_s, kills_s);
    let Some(densest) = densest_stretch(&near, kills_s.len()) else {
        return (None, vec![None; kills_s.len()]);
    };
    let offset_s = median(&near[densest].iter().map(|press| press.gap_s).collect::<Vec<_>>());
    let kill_presses = kills_s.iter().map(|&kill_s| nearest_press(presses_s, kill_s, offset_s)).collect();
    (Some(offset_s), kill_presses)
}

/// One measure over the kills: how many have it, and its p10, median and p90.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[expect(clippy::min_ident_chars, reason = "`n` is the JSON's key, which mouse_read.py writes and the run page reads")]
pub struct Spread {
    /// The measure's field in `KillMeasure`.
    pub key: String,
    pub n: usize,
    pub p10: f64,
    pub median: f64,
    pub p90: f64,
}

/// A measure the run's spreads cover: its field in `KillMeasure`, its label and decimals in the printed report, and
/// its value in a kill's measures.
struct SpreadField {
    key: &'static str,
    label: &'static str,
    decimals: usize,
    value: fn(&KillMeasure) -> Option<f64>,
}

/// The spreads a run gives, in the order mouse_read.py prints them.
const SPREAD_FIELDS: [SpreadField; 7] = [
    SpreadField { key: "reaction_ms", label: "reaction: start (ms)", decimals: 1, value: |kill| kill.reaction_ms },
    SpreadField { key: "flick_ms", label: "flick: start to stop (ms)", decimals: 1, value: |kill| kill.flick_ms },
    SpreadField { key: "peak_dps", label: "peak speed (deg/s)", decimals: 0, value: |kill| Some(kill.peak_dps) },
    SpreadField {
        key: "stop_to_click_ms",
        label: "stop to click (ms)",
        decimals: 1,
        value: |kill| kill.stop_to_click_ms,
    },
    SpreadField {
        key: "still_ms",
        label: "still before the click (ms)",
        decimals: 1,
        value: |kill| Some(kill.still_ms),
    },
    SpreadField {
        key: "click_dps",
        label: "speed at the click (deg/s)",
        decimals: 1,
        value: |kill| Some(kill.click_dps),
    },
    SpreadField { key: "dist_deg", label: "distance (deg)", decimals: 1, value: |kill| Some(kill.dist_deg) },
];

/// The spreads of the kills' measures (one for each of SPREAD_FIELDS that some kill has).
fn spreads(kills: &[KillMeasure]) -> Vec<Spread> {
    let spread = |field: &SpreadField| {
        let values: Vec<f64> = kills.iter().filter_map(field.value).collect();
        (!values.is_empty()).then(|| Spread {
            key: field.key.to_string(),
            n: values.len(),
            p10: decile(&values, 0.1),
            median: median(&values),
            p90: decile(&values, 0.9),
        })
    };
    SPREAD_FIELDS.iter().filter_map(spread).collect()
}

/// A run measured from its mouse log (mouse_read.py's run mode): everything it prints, and the kills it writes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MouseRun {
    pub log: LogFacts,
    pub scenario: String,
    pub dpi: f64,
    pub cm360: f64,
    /// Where the sensitivity came from: "options", "the stats file" or "defaults".
    pub sens_from: String,
    pub deg_per_count: f64,
    /// The speed window, ms, and whether it was widened to twice the median interval.
    pub window_ms: f64,
    pub window_widened: bool,
    pub start_dps: f64,
    pub stop_dps: f64,
    pub hold_ms: f64,
    /// The press minus the stats file's kill time, ms (3 decimals), and unrounded in s.
    pub offset_ms: f64,
    pub offset_s: f64,
    /// The kills in the stats file, and those matched with a press within 10 ms.
    pub kill_count: usize,
    pub matched: usize,
    /// The p90 of the matched gaps between press and kill, ms.
    pub gap_p90_ms: f64,
    pub presses_in_run: usize,
    /// The shots in the stats file.
    pub shots: i64,
    /// The presses in the run that killed nothing, s since the log's start (6 decimals).
    pub misses_s: Vec<f64>,
    pub kills: Vec<KillMeasure>,
    pub spreads: Vec<Spread>,
    /// Kills clicked while moving, kills with no stop before the click, and kills with corrections.
    pub moving_clicks: usize,
    pub no_stop: usize,
    pub corrected: usize,
}

/// Where the run's sensitivity comes from: the options, else the stats file, else the defaults.
fn sensitivity_source(options: &Options, stats: &StatsRun) -> &'static str {
    if options.given_dpi().is_some() || options.given_cm360().is_some() {
        "options"
    } else if stats.sensitivity.is_some() {
        "the stats file"
    } else {
        "defaults"
    }
}

/// The options, with the speed window widened to WINDOW_MEDIAN_INTERVALS median intervals when a log of
/// MIN_EVENTS_FOR_RATE events or more has its events farther apart than the window allows; and whether it was.
fn widened_window(options: &Options, facts: &LogFacts) -> (Options, bool) {
    let mut options = options.clone();
    if let Some(interval_s) = facts.median_interval
        && facts.events >= MIN_EVENTS_FOR_RATE
        && WINDOW_MEDIAN_INTERVALS * interval_s * 1000.0 > options.window
    {
        options.window = round(WINDOW_MEDIAN_INTERVALS * interval_s * 1000.0, 2);
        return (options, true);
    }
    (options, false)
}

/// The stats file's kill times on the log's clock (seconds since its start, before the offset); an error when the log
/// does not cover the run.
fn kills_on_log_clock(log: &MouseLog, stats: &StatsRun, utc_offset: i64) -> Result<Vec<f64>, String> {
    let kills_s: Vec<f64> = stats.kills.iter().map(|kill| kill.epoch_s - log.wall0).collect();
    if kills_s.is_empty()
        || kills_s[kills_s.len() - 1] < -OFFSET_SEARCH_S
        || kills_s[0] > log.duration + OFFSET_SEARCH_S
    {
        return Err(format!(
            "the log ({} to {}) does not cover this run ({} on)",
            local_clock(log.wall0, utc_offset),
            local_clock(log.wall0 + log.duration, utc_offset),
            stats.kills.first().map_or("?", |kill| kill.local_time.as_str())
        ));
    }
    Ok(kills_s)
}

/// A run's kills matched with its log's presses, all on the log's clock (seconds since its start).
struct MatchedRun<'a> {
    log: &'a MouseLog,
    stats: &'a StatsRun,
    presses_s: Vec<f64>,
    /// Each kill's stats time, before the offset.
    kills_s: Vec<f64>,
    /// Each kill's press index, None when no press is within MATCH_TOLERANCE_S.
    kill_presses: Vec<Option<usize>>,
    /// A press minus its kill's stats time.
    offset_s: f64,
}

impl MatchedRun<'_> {
    /// The matched gaps between press and kill, ms.
    fn gaps_ms(&self) -> Vec<f64> {
        let matched = self.kill_presses.iter().zip(&self.kills_s);
        let gap_ms = |(press, kill_s): (&Option<usize>, &f64)| {
            press.map(|press| (self.presses_s[press] - kill_s - self.offset_s).abs() * 1000.0)
        };
        matched.filter_map(gap_ms).collect()
    }

    /// The presses from the run's start to its end (with the offset), by index.
    fn presses_in_run(&self) -> Vec<usize> {
        let wall0 = self.log.wall0;
        let start_s = self.stats.start_epoch_s - wall0 + self.offset_s;
        let end_s = self.stats.end_epoch_s - wall0 + self.offset_s;
        (0..self.presses_s.len()).filter(|&i| start_s <= self.presses_s[i] && self.presses_s[i] <= end_s).collect()
    }

    /// The presses in `in_run` that killed nothing, seconds since the log's start (6 decimals).
    fn misses_s(&self, in_run: &[usize]) -> Vec<f64> {
        let used: HashSet<usize> = self.kill_presses.iter().flatten().copied().collect();
        in_run.iter().filter(|press| !used.contains(press)).map(|&press| round(self.presses_s[press], 6)).collect()
    }

    /// Each matched kill's measures, from the press before it (after an unmatched kill, that kill's time with the
    /// offset; for the first kill, the run's start) to its own.
    fn measure_kills(
        &self,
        options: &Options,
        degrees_per_count: f64,
        utc_offset: i64,
    ) -> Result<Vec<KillMeasure>, String> {
        let (wall0, offset_s) = (self.log.wall0, self.offset_s);
        let motion = Motion::new(self.log, degrees_per_count);
        let (mut measures, mut from_s) = (Vec::new(), self.stats.start_epoch_s - wall0 + offset_s);
        for ((kill, &kill_s), press) in self.stats.kills.iter().zip(&self.kills_s).zip(&self.kill_presses) {
            let Some(press) = *press else {
                from_s = kill_s + offset_s;
                continue;
            };
            let press_s = self.presses_s[press];
            let press_local = local_clock(wall0 + press_s, utc_offset);
            let gap_ms = round((press_s - kill_s - offset_s) * 1000.0, 3);
            let unmeasured = KillMeasure::unmeasured(kill, press_local, round(press_s, 6), gap_ms);
            measures.push(measure(&motion, from_s, press_s, options, unmeasured)?);
            from_s = press_s;
        }
        Ok(measures)
    }
}

/// Measures the run: the stats file's kills matched with the log's presses, then each kill's flick.
pub fn run(log: &MouseLog, stats: &StatsRun, options: &Options, utc_offset: i64) -> Result<MouseRun, String> {
    let Sensitivity { dpi, cm360 } = options.sensitivity_or(stats.sensitivity);
    let degrees_per_count = deg_per_count(dpi, cm360);
    let facts = log_facts(log, utc_offset);
    let (options, window_widened) = widened_window(options, &facts);
    let kills_s = kills_on_log_clock(log, stats, utc_offset)?;
    let presses_s = presses_of(log);
    let (offset_s, kill_presses) = match_kills(&presses_s, &kills_s);
    let offset_s = offset_s.ok_or("no left-button press lies within 1 s of any kill")?;
    let matched = MatchedRun { log, stats, presses_s, kills_s, kill_presses, offset_s };
    let gaps_ms = matched.gaps_ms();
    let in_run = matched.presses_in_run();
    let kills = matched.measure_kills(&options, degrees_per_count, utc_offset)?;
    Ok(MouseRun {
        log: facts,
        scenario: stats.scenario.clone(),
        dpi,
        cm360,
        sens_from: sensitivity_source(&options, stats).to_string(),
        deg_per_count: degrees_per_count,
        window_ms: options.window,
        window_widened,
        start_dps: options.start,
        stop_dps: options.stop,
        hold_ms: options.hold,
        offset_ms: round(offset_s * 1000.0, 3),
        offset_s,
        kill_count: matched.kills_s.len(),
        matched: gaps_ms.len(),
        gap_p90_ms: if gaps_ms.is_empty() { 0.0 } else { decile(&gaps_ms, 0.9) },
        presses_in_run: in_run.len(),
        shots: stats.shots,
        misses_s: matched.misses_s(&in_run),
        moving_clicks: kills.iter().filter(|kill| kill.still_ms == 0.0).count(),
        no_stop: kills.iter().filter(|kill| kill.stop_s.is_none()).count(),
        corrected: kills.iter().filter(|kill| kill.corrections.is_some_and(|corrections| corrections > 0)).count(),
        spreads: spreads(&kills),
        kills,
    })
}

// ---- one call for the page ----

/// What the page asks of a log: its run's stats file (name and text; none for the log on its own), the options, and
/// the UTC offset (local minus UTC, seconds) for local times.
#[derive(Deserialize)]
pub struct ReadRequest {
    #[serde(default)]
    pub stats_name: Option<String>,
    #[serde(default)]
    pub stats_text: Option<String>,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub utc_offset: i64,
}

/// The answer: the run's measures, the log's summary (no stats file), or why there are none.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadOutcome {
    Run(Box<MouseRun>),
    Summary(LogSummary),
    Error(String),
}

/// Reads a log for the request.
pub fn read(log: &[u8], request: &ReadRequest) -> ReadOutcome {
    let outcome = read_log(log).and_then(|log| match (&request.stats_name, &request.stats_text) {
        (Some(name), Some(text)) => read_stats(name, text, request.utc_offset)
            .and_then(|stats| run(&log, &stats, &request.options, request.utc_offset))
            .map(|measured| ReadOutcome::Run(Box::new(measured))),
        _ => Ok(ReadOutcome::Summary(summary(&log, &request.options, request.utc_offset))),
    });
    outcome.unwrap_or_else(ReadOutcome::Error)
}

/// `read` with the request as JSON, the outcome as JSON ({run}, {summary} or {error}).
pub fn read_json(log: &[u8], request: &[u8]) -> Vec<u8> {
    let outcome = match serde_json::from_slice::<ReadRequest>(request) {
        Ok(request) => read(log, &request),
        Err(error) => ReadOutcome::Error(format!("the request cannot be read: {error}")),
    };
    serde_json::to_vec(&outcome).unwrap_or_default()
}

/// The wall times a log covers (seconds since 1970), from its header and its last record only: for finding the log
/// of a run among many without reading them whole. None when it is not a mouse log.
pub fn log_span(head: &[u8], last: &[u8]) -> Option<(f64, f64)> {
    let (qpc_frequency, start_qpc, start_ns) = read_header(head)?;
    let wall0 = seconds_of_ns(start_ns);
    if last.len() < RECORD_SIZE || qpc_frequency <= 0 {
        return Some((wall0, wall0));
    }
    let end = match Record::read(last) {
        Record::Stop(_, stop_ns) => seconds_of_ns(stop_ns),
        Record::Event(event) => wall0 + (event.qpc - start_qpc) as f64 / qpc_frequency as f64,
        Record::Device(_) | Record::Unknown => wall0,
    };
    Some((wall0, end.max(wall0)))
}

// ---- the printed report (the command-line readers) ----

/// Python's `format(x, "g")`.
pub fn fmt_g(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let scientific = format!("{x:.*e}", (G_SIGNIFICANT_DIGITS - 1) as usize);
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    if (G_MIN_FIXED_EXPONENT..G_SIGNIFICANT_DIGITS).contains(&exponent) {
        let fixed = format!("{x:.*}", (G_SIGNIFICANT_DIGITS - 1 - exponent) as usize);
        if fixed.contains('.') { fixed.trim_end_matches('0').trim_end_matches('.').to_string() } else { fixed }
    } else {
        let mantissa =
            if mantissa.contains('.') { mantissa.trim_end_matches('0').trim_end_matches('.') } else { mantissa };
        format!("{mantissa}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
    }
}

/// Python's `str(x)` for a float of ordinary size.
fn py_str(x: f64) -> String {
    let text = format!("{x}");
    if text.contains(['.', 'e', 'i', 'N']) { text } else { text + ".0" }
}

/// The rate line mouse_read.py prints (`rate_line`), with its throttling warning.
fn rate_text(facts: &LogFacts) -> String {
    let Some(interval_s) = facts.median_interval else {
        return "too few events for a rate".into();
    };
    let hz = if interval_s != 0.0 { 1.0 / interval_s } else { f64::INFINITY };
    let (interval_ms, busiest_hz) = (interval_s * 1000.0, facts.busiest_hz);
    let mut text = format!("median interval {interval_ms:.3} ms ({hz:.0} Hz), busiest 100 ms {busiest_hz:.0} Hz");
    if facts.throttled {
        text += "\n  warning: events are far apart; Windows probably throttled the logger (about 8 ms when \
                 throttled), so times are only good to about the interval";
    }
    text
}

/// What mouse_read.py prints about any log (`head`, then the rate line).
fn head_text(facts: &LogFacts, path: &str) -> String {
    let drift =
        facts.drift_ms.map_or("no stop pair (killed?)".to_string(), |drift_ms| format!("clock drift {drift_ms:.3} ms"));
    let mut text = format!(
        "log: {path}\n  {} to {}, {:.2} s, {} events, {drift}\n",
        facts.start_local, facts.end_local, facts.duration, facts.events
    );
    for (index, device) in facts.devices.iter().enumerate() {
        text += &format!("  device {index}: handle {:#x}, {} events\n", device.handle, device.events);
    }
    if facts.absolute > 0 {
        text += &format!("  {} absolute events (MOUSE_MOVE_ABSOLUTE) skipped\n", facts.absolute);
    }
    text + "  " + &rate_text(facts) + "\n"
}

/// What mouse_read.py prints for a log on its own.
pub fn summary_text(summary: &LogSummary, path: &str) -> String {
    head_text(&summary.log, path)
        + &format!(
            "  travel {:.1} deg (1 ms steps, {:.6} deg per count), left-button presses {}\n",
            summary.travel_deg, summary.deg_per_count, summary.presses
        )
}

/// What mouse_read.py prints for a run (up to the line naming the file it writes).
pub fn run_text(run: &MouseRun, path: &str, stats_name: &str) -> String {
    let mut text = head_text(&run.log, path);
    if run.window_widened {
        text += &format!("  speed window widened to {} ms (twice the median interval)\n", py_str(run.window_ms));
    }
    text += &format!(
        "stats: {stats_name}\n  {} dpi, {} cm/360 (from {}), {:.6} deg per count\n",
        fmt_g(run.dpi),
        fmt_g(run.cm360),
        run.sens_from,
        run.deg_per_count
    );
    text += &format!(
        "  clock offset (press minus stats kill time) {:+.1} ms; {} of {} kills matched within 10 ms \
         (gap p90 {:.1} ms)\n",
        run.offset_s * 1000.0,
        run.matched,
        run.kill_count,
        run.gap_p90_ms
    );
    text += &format!(
        "  presses in the run {} (stats shots {}), misses {}\n",
        run.presses_in_run,
        run.shots,
        run.misses_s.len()
    );
    text += &format!("{} kills measured (times from the previous kill's press)\n", run.kills.len());
    for field in &SPREAD_FIELDS {
        if let Some(spread) = run.spreads.iter().find(|spread| spread.key == field.key) {
            let (label, digits) = (field.label, field.decimals);
            text += &format!(
                "  {label:34} n {:3}  p10 {:7.digits$}  median {:7.digits$}  p90 {:7.digits$}\n",
                spread.n, spread.p10, spread.median, spread.p90
            );
        }
    }
    text += &format!(
        "  clicked while moving: {} of {}; no stop before the click: {}; with corrections: {}\n",
        run.moving_clicks,
        run.kills.len(),
        run.no_stop,
        run.corrected
    );
    text
}

/// The `<log>.kills.json` mouse_read.py writes beside a log.
pub fn kills_json(run: &MouseRun, log_path: &str, stats_path: &str) -> serde_json::Value {
    serde_json::json!({
        "log": log_path,
        "stats": stats_path,
        "offset_ms": run.offset_ms,
        "dpi": run.dpi,
        "cm360": run.cm360,
        "window_ms": run.window_ms,
        "start_dps": run.start_dps,
        "stop_dps": run.stop_dps,
        "hold_ms": run.hold_ms,
        "misses_s": run.misses_s,
        "kills": run.kills,
    })
}
