//! The raw mouse log and what is measured from it (python/mouse_log.py writes the log, as desktop/src/mouse.rs does;
//! python/mouse_read.py reads it): each flick of a KovaaK's run, matched with the run's stats file. Pure: the log's
//! bytes and the stats file's text in, the measures out. The arithmetic follows Python's step by step, so the results
//! are the same to the bit.
//!
//! File format (little endian), 24-byte records after a 32-byte header:
//!   header  "FFML", version 1 (u16), record size 24 (u16), QPC frequency, start QPC, start time_ns (i64 each)
//!   event   QPC (i64), dx, dy (i32), usFlags, usButtonFlags, usButtonData, device index (u16)
//!   device  -1 (i64), device handle (u64), device index (u32), 0 (u32): before the first event of each device
//!   stop    -2 (i64), stop QPC, stop time_ns (i64): appended when the logger stops
//!
//! Times are local where Python prints local time: the caller gives the UTC offset (local minus UTC, seconds), since
//! the core has no time zone of its own.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::python::{hypot, round};
use crate::statistics::median;

pub const MAGIC: [u8; 4] = *b"FFML";
pub const VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 32;
pub const RECORD_SIZE: usize = 24;
pub const KIND_DEVICE: i64 = -1;
pub const KIND_STOP: i64 = -2;
pub const MOUSE_MOVE_ABSOLUTE: u16 = 0x01;
pub const LEFT_BUTTON_DOWN: u16 = 0x0001;
pub const LEFT_BUTTON_UP: u16 = 0x0002;

/// The analysis grid, s.
const DT: f64 = 0.00025;
/// A kill matches a press within 10 ms (after the offset).
const MATCH_TOL: f64 = 0.010;
/// The clock offset is searched within 1 s.
const SEARCH: f64 = 1.0;

// ---- the file ----

/// The file's header: the QPC frequency and a (QPC, time_ns) pair taken at the start.
pub fn header(freq: i64, q0: i64, ns0: i64) -> [u8; HEADER_SIZE] {
    let mut b = [0u8; HEADER_SIZE];
    b[..4].copy_from_slice(&MAGIC);
    b[4..6].copy_from_slice(&VERSION.to_le_bytes());
    b[6..8].copy_from_slice(&(RECORD_SIZE as u16).to_le_bytes());
    b[8..16].copy_from_slice(&freq.to_le_bytes());
    b[16..24].copy_from_slice(&q0.to_le_bytes());
    b[24..32].copy_from_slice(&ns0.to_le_bytes());
    b
}

/// One event: the QPC time it was handled, the counts moved (x right, y down) and RAWMOUSE's flags.
pub fn event(qpc: i64, dx: i32, dy: i32, flags: u16, buttons: u16, data: u16, device: u16) -> [u8; RECORD_SIZE] {
    let mut b = [0u8; RECORD_SIZE];
    b[..8].copy_from_slice(&qpc.to_le_bytes());
    b[8..12].copy_from_slice(&dx.to_le_bytes());
    b[12..16].copy_from_slice(&dy.to_le_bytes());
    b[16..18].copy_from_slice(&flags.to_le_bytes());
    b[18..20].copy_from_slice(&buttons.to_le_bytes());
    b[20..22].copy_from_slice(&data.to_le_bytes());
    b[22..24].copy_from_slice(&device.to_le_bytes());
    b
}

/// A device's record, written before its first event.
pub fn device(handle: u64, index: u32) -> [u8; RECORD_SIZE] {
    let mut b = [0u8; RECORD_SIZE];
    b[..8].copy_from_slice(&KIND_DEVICE.to_le_bytes());
    b[8..16].copy_from_slice(&handle.to_le_bytes());
    b[16..20].copy_from_slice(&index.to_le_bytes());
    b
}

/// The stop record: a second (QPC, time_ns) pair.
pub fn stop(qpc: i64, ns: i64) -> [u8; RECORD_SIZE] {
    let mut b = [0u8; RECORD_SIZE];
    b[..8].copy_from_slice(&KIND_STOP.to_le_bytes());
    b[8..16].copy_from_slice(&qpc.to_le_bytes());
    b[16..24].copy_from_slice(&ns.to_le_bytes());
    b
}

fn i64_at(b: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// A log read into columns (mouse_log.py: `read_log`). Times t are seconds since the start pair, on the wall clock's
/// scale (the QPC time is stretched by the drift between the two pairs); without a stop pair (the logger was killed)
/// the scale is 1.
pub struct MouseLog {
    pub freq: i64,
    /// The start pair's wall time, seconds since 1970.
    pub wall0: f64,
    /// The stop pair (QPC, time_ns), when the logger stopped cleanly.
    pub stop: Option<(i64, i64)>,
    pub drift_ms: Option<f64>,
    pub duration: f64,
    /// The devices' handles, by index.
    pub devices: Vec<u64>,
    pub t: Vec<f64>,
    pub dx: Vec<i32>,
    pub dy: Vec<i32>,
    pub flags: Vec<u16>,
    pub bflags: Vec<u16>,
    pub bdata: Vec<u16>,
    pub dev: Vec<u16>,
}

/// The start of a log: its QPC frequency, start QPC and start time_ns; None when it is not a version 1 mouse log.
pub fn read_header(data: &[u8]) -> Option<(i64, i64, i64)> {
    let ok = data.len() >= HEADER_SIZE
        && data[..4] == MAGIC
        && u16_at(data, 4) == VERSION
        && u16_at(data, 6) as usize == RECORD_SIZE;
    ok.then(|| (i64_at(data, 8), i64_at(data, 16), i64_at(data, 24)))
}

pub fn read_log(data: &[u8]) -> Result<MouseLog, String> {
    let (freq, q0, ns0) = read_header(data).ok_or(format!("not a version {VERSION} mouse log"))?;
    if freq <= 0 {
        return Err("the log's QPC frequency is not positive".into());
    }
    let body = &data[HEADER_SIZE..HEADER_SIZE + (data.len() - HEADER_SIZE) / RECORD_SIZE * RECORD_SIZE];
    let mut log = MouseLog {
        freq,
        wall0: ns0 as f64 / 1e9,
        stop: None,
        drift_ms: None,
        duration: 0.0,
        devices: Vec::new(),
        t: Vec::new(),
        dx: Vec::new(),
        dy: Vec::new(),
        flags: Vec::new(),
        bflags: Vec::new(),
        bdata: Vec::new(),
        dev: Vec::new(),
    };
    let mut qpc = Vec::new();
    for r in body.chunks_exact(RECORD_SIZE) {
        let q = i64_at(r, 0);
        if q < 0 {
            if q == KIND_DEVICE {
                log.devices.push(u64::from_le_bytes(r[8..16].try_into().unwrap()));
            } else if q == KIND_STOP {
                log.stop = Some((i64_at(r, 8), i64_at(r, 16)));
            }
            continue;
        }
        qpc.push(q);
        log.dx.push(i32_at(r, 8));
        log.dy.push(i32_at(r, 12));
        log.flags.push(u16_at(r, 16));
        log.bflags.push(u16_at(r, 18));
        log.bdata.push(u16_at(r, 20));
        log.dev.push(u16_at(r, 22));
    }
    let mut scale = 1.0;
    if let Some((q1, ns1)) = log.stop
        && q1 > q0
    {
        let span = (q1 - q0) as f64 / freq as f64;
        let wall = (ns1 - ns0) as f64 / 1e9;
        scale = wall / span;
        log.drift_ms = Some((wall - span) * 1e3);
    }
    let k = scale / freq as f64;
    log.duration = match (log.stop, qpc.last()) {
        (Some((q1, _)), _) => (q1 - q0) as f64 * k,
        (None, Some(&q)) => (q - q0) as f64 * k,
        (None, None) => 0.0,
    };
    log.t = qpc.iter().map(|&q| (q - q0) as f64 * k).collect();
    Ok(log)
}

/// Events a second in the busiest window of span seconds.
pub fn busiest_rate(t: &[f64], span: f64) -> f64 {
    let (mut best, mut j) = (0usize, 0usize);
    for i in 0..t.len() {
        while t[i] - t[j] > span {
            j += 1;
        }
        best = best.max(i - j + 1);
    }
    best as f64 / span
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
    lo + a[lo..].partition_point(|&v| v <= x)
}

/// `bisect.bisect_left(a, x)`.
fn bisect_left(a: &[f64], x: f64) -> usize {
    a.partition_point(|&v| v < x)
}

/// Python's `sum` of floats (3.12 on): Neumaier's compensated sum.
fn py_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut hi, mut lo) = (0.0f64, 0.0f64);
    for x in values {
        let t = hi + x;
        if hi.abs() >= x.abs() {
            lo += (hi - t) + x;
        } else {
            lo += (x - t) + hi;
        }
        hi = t;
    }
    if lo != 0.0 && lo.is_finite() { hi + lo } else { hi }
}

/// `statistics.quantiles(vals, n=10, method="inclusive")[round(p * 10) - 1]`, or the value when there is one
/// (mouse_read.py: `q`).
fn decile(vals: &[f64], p: f64) -> f64 {
    if vals.len() < 2 {
        return vals[0];
    }
    let mut data = vals.to_vec();
    data.sort_by(f64::total_cmp);
    let (n, m) = (10usize, data.len() - 1);
    let i = (p * 10.0).round_ties_even() as usize;
    let (j, delta) = (i * m / n, i * m % n);
    (data[j] * (n - delta) as f64 + data[j + 1] * delta as f64) / n as f64
}

/// A time (seconds since 1970) as local "%H:%M:%S.%f" (Python's `datetime.fromtimestamp(wall)`: microseconds rounded
/// half to even).
pub fn local_clock(wall: f64, utc_offset: i64) -> String {
    let mut int = wall.trunc();
    let mut frac = ((wall - int) * 1e6).round_ties_even();
    if frac >= 1e6 {
        frac -= 1e6;
        int += 1.0;
    } else if frac < 0.0 {
        frac += 1e6;
        int -= 1.0;
    }
    let secs = (int as i64 + utc_offset).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}.{:06}", secs / 3600, secs / 60 % 60, secs % 60, frac as i64)
}

/// Degrees per count: 360 / (cm360 / 2.54 * dpi).
pub fn deg_per_count(dpi: f64, cm360: f64) -> f64 {
    360.0 / (cm360 / 2.54 * dpi)
}

// ---- the motion ----

/// The log's relative motion as cumulative degrees after each event (absolute events are skipped). Between events
/// the position is interpolated: each event's motion is spread evenly over the time since the event before it, but
/// over at most cap (1 ms, or twice the median interval if that is longer), so a report never counts as all in or all
/// out of a speed window.
struct Motion {
    t: Vec<f64>,
    x: Vec<f64>,
    y: Vec<f64>,
    cap: f64,
}

impl Motion {
    fn new(log: &MouseLog, k: f64) -> Motion {
        let n = log.t.len();
        let (mut t, mut xs, mut ys) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        let (mut x, mut y) = (0.0f64, 0.0f64);
        for i in 0..log.t.len() {
            if log.flags[i] & MOUSE_MOVE_ABSOLUTE != 0 {
                continue;
            }
            x += log.dx[i] as f64 * k;
            y -= log.dy[i] as f64 * k;
            t.push(log.t[i]);
            xs.push(x);
            ys.push(y);
        }
        let gaps: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
        let cap = if gaps.is_empty() { 0.001 } else { py_max(0.001, 2.0 * median(&gaps)) };
        Motion { t, x: xs, y: ys, cap }
    }

    fn pos(&self, i: usize) -> (f64, f64) {
        if i > 0 { (self.x[i - 1], self.y[i - 1]) } else { (0.0, 0.0) }
    }

    /// Position at time t; i is the index just past the last event at or before t, if known.
    fn at(&self, t: f64, i: Option<usize>) -> (f64, f64) {
        let i = i.unwrap_or_else(|| bisect_right(&self.t, t, 0));
        let (x0, y0) = self.pos(i);
        if i >= self.t.len() {
            return (x0, y0);
        }
        let t1 = self.t[i];
        let s = if i > 0 { py_max(self.t[i - 1], t1 - self.cap) } else { t1 - self.cap };
        if t <= s {
            return (x0, y0);
        }
        let f = (t - s) / (t1 - s);
        (x0 + f * (self.x[i] - x0), y0 + f * (self.y[i] - y0))
    }

    /// Grid times from a to c (ending exactly at c) and the speed in deg/s at each, over [t - w/2, t + w/2] cut at c.
    fn speeds(&self, a: f64, c: f64, w: f64) -> (Vec<f64>, Vec<f64>) {
        let n = ((c - a) / DT) as i64;
        let points = (n + 1).max(0) as usize;
        let (mut ts, mut vs, mut i0, mut i1) = (Vec::with_capacity(points), Vec::with_capacity(points), 0, 0);
        for g in 0..=n {
            let t = c - (n - g) as f64 * DT;
            let (lo, hi) = (t - w / 2.0, py_min(t + w / 2.0, c));
            i0 = bisect_right(&self.t, lo, i0);
            i1 = bisect_right(&self.t, hi, i1);
            let ((x0, y0), (x1, y1)) = (self.at(lo, Some(i0)), self.at(hi, Some(i1)));
            ts.push(t);
            vs.push(hypot(x1 - x0, y1 - y0) / (hi - lo));
        }
        (ts, vs)
    }
}

/// When the speed crosses thr between grid points i - 1 and i (linear), so times are not rounded to the grid.
fn cross(ts: &[f64], vs: &[f64], i: usize, thr: f64) -> f64 {
    if i == 0 || vs[i] == vs[i - 1] {
        return ts[i];
    }
    let f = py_min(1.0, py_max(0.0, (thr - vs[i - 1]) / (vs[i] - vs[i - 1])));
    ts[i - 1] + f * (ts[i] - ts[i - 1])
}

/// The reader's settings: the sensitivity (None: the stats file's, else 1600 dpi and 70 cm/360), the speed window
/// (ms) and the thresholds (deg/s; the hold in ms). mouse_read.py's options, with its defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub dpi: Option<f64>,
    pub cm360: Option<f64>,
    pub window: f64,
    pub start: f64,
    pub stop: f64,
    pub hold: f64,
}

impl Default for Options {
    fn default() -> Options {
        Options { dpi: None, cm360: None, window: 4.0, start: 30.0, stop: 10.0, hold: 5.0 }
    }
}

/// What `find` gives: the start time, the peak's index, the stop time and the settle time (the start of the final
/// still stretch); None where there is none.
type Found = (Option<f64>, usize, Option<f64>, Option<f64>);

/// The start, peak, stop and settle in a speed profile that ends at the click (mouse_read.py: `find`).
fn find(ts: &[f64], vs: &[f64], dt: f64, o: &Options) -> Found {
    let n = vs.len();
    let s = if vs[0] >= o.start {
        // still moving from the last flick: a new start needs a stop first
        match (0..n).find(|&i| vs[i] < o.stop) {
            None => Some(0),
            Some(j) => (j..n).find(|&i| vs[i] >= o.start),
        }
    } else {
        (0..n).find(|&i| vs[i] >= o.start)
    };
    let mut p = s.unwrap_or(0);
    for i in p + 1..n {
        if vs[i] > vs[p] {
            p = i;
        }
    }
    let hold = ((o.hold / 1000.0 / dt).round_ties_even() as i64).max(1) as usize;
    let below: Vec<bool> = vs.iter().map(|&v| v < o.stop).collect();
    // left[i]: grid points in a row under the stop speed from i on
    let mut left = vec![0usize; n + 1];
    for i in (0..n).rev() {
        left[i] = if below[i] { left[i + 1] + 1 } else { 0 };
    }
    let stop = (p..n).find(|&i| below[i] && left[i] >= (hold + 1).min(n - i));
    let mut settle = None;
    if below[n - 1] {
        let mut s = n - 1;
        while s > 0 && below[s - 1] {
            s -= 1;
        }
        settle = Some(s);
    }
    let t = |i: Option<usize>, thr: f64| i.map(|i| cross(ts, vs, i, thr));
    (t(s, o.start), p, t(stop, o.stop), t(settle, o.stop))
}

/// One kill's measures, from the previous kill's press to its own (mouse_read.py: the rows of `<log>.kills.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
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

/// Python's `round(x * 1000, 3)`: seconds as milliseconds.
fn ms(x: f64) -> f64 {
    round(x * 1000.0, 3)
}

fn measure(m: &Motion, a: f64, c: f64, o: &Options, base: KillMeasure) -> Result<KillMeasure, String> {
    let (ts, vs) = m.speeds(a, c, o.window / 1000.0);
    if vs.is_empty() {
        return Err(format!("kill {}: its press comes before the press before it", base.n));
    }
    let (s, p, stop, settle) = find(&ts, &vs, DT, o);
    let ((x0, y0), (x1, y1)) = (m.at(a, None), m.at(c, None));
    Ok(KillMeasure {
        start_s: s,
        stop_s: stop,
        settle_s: settle,
        reaction_ms: s.map(|s| ms(s - a)),
        flick_ms: s.zip(stop).map(|(s, stop)| ms(stop - s)),
        peak_dps: round(vs[p], 1),
        peak_ms: ms(ts[p] - a),
        stop_to_click_ms: stop.map(|stop| ms(c - stop)),
        still_ms: settle.map_or(0.0, |settle| ms(c - settle)),
        click_dps: round(vs[vs.len() - 1], 1),
        corrections: stop.map(|stop| (1..vs.len()).filter(|&i| ts[i] > stop && vs[i] >= o.stop && o.stop > vs[i - 1]).count()),
        dist_deg: round(hypot(x1 - x0, y1 - y0), 3),
        ..base
    })
}

/// The left-button presses' times.
pub fn presses_of(log: &MouseLog) -> Vec<f64> {
    (0..log.t.len()).filter(|&i| log.bflags[i] & LEFT_BUTTON_DOWN != 0).map(|i| log.t[i]).collect()
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

pub fn log_facts(log: &MouseLog, utc_offset: i64) -> LogFacts {
    let (t0, dur) = (log.wall0, log.duration);
    let gaps: Vec<f64> = log.t.windows(2).map(|w| w[1] - w[0]).collect();
    let med = (!gaps.is_empty()).then(|| median(&gaps));
    LogFacts {
        wall0: t0,
        start_local: local_clock(t0, utc_offset),
        end_local: local_clock(t0 + dur, utc_offset),
        duration: dur,
        events: log.t.len(),
        drift_ms: log.drift_ms,
        devices: (0..log.devices.len())
            .map(|d| DeviceFacts { handle: log.devices[d], events: log.dev.iter().filter(|&&v| v as usize == d).count() })
            .collect(),
        absolute: log.flags.iter().filter(|&&f| f & MOUSE_MOVE_ABSOLUTE != 0).count(),
        median_interval: med,
        busiest_hz: busiest_rate(&log.t, 0.1),
        throttled: med.is_some_and(|m| log.t.len() >= 200 && m > 0.002),
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

pub fn summary(log: &MouseLog, o: &Options, utc_offset: i64) -> LogSummary {
    let (dpi, cm360) = (o.dpi.filter(|&v| v != 0.0).unwrap_or(1600.0), o.cm360.filter(|&v| v != 0.0).unwrap_or(70.0));
    let k = deg_per_count(dpi, cm360);
    // 1 ms bins in the order they first appear, as a Python dict keeps them
    let (mut order, mut bins) = (Vec::new(), HashMap::<i64, [i64; 2]>::new());
    for i in 0..log.t.len() {
        if log.flags[i] & MOUSE_MOVE_ABSOLUTE == 0 {
            let key = (log.t[i] * 1000.0) as i64;
            let b = bins.entry(key).or_insert_with(|| {
                order.push(key);
                [0, 0]
            });
            b[0] += i64::from(log.dx[i]);
            b[1] += i64::from(log.dy[i]);
        }
    }
    let travel = py_sum(order.iter().map(|key| hypot(bins[key][0] as f64, bins[key][1] as f64))) * k;
    LogSummary { log: log_facts(log, utc_offset), dpi, cm360, deg_per_count: k, travel_deg: travel, presses: presses_of(log).len() }
}

// ---- the stats file ----

/// A kill in the stats file: its number, its local time as written, that time in seconds since 1970, and its shots.
#[derive(Clone, Debug)]
pub struct StatsKill {
    pub n: i64,
    pub local: String,
    pub t: f64,
    pub shots: i64,
}

/// What the reader takes from a stats file (mouse_read.py: `read_stats`): the kills, the run's start and end
/// (seconds since 1970), the shots, the sensitivity when it is in cm/360, and the scenario.
#[derive(Clone, Debug)]
pub struct StatsRun {
    pub kills: Vec<StatsKill>,
    pub start: f64,
    pub end: f64,
    pub shots: i64,
    pub sens: Option<(f64, f64)>,
    pub scenario: String,
}

/// The text's lines, split where Python's `str.splitlines` splits them.
fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let end = match c {
            '\r' if chars.peek().is_some_and(|&(_, n)| n == '\n') => {
                chars.next();
                i + 2
            }
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                i + c.len_utf8()
            }
            _ => continue,
        };
        out.push(&text[start..i]);
        start = end;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The date in a stats file's name ("... - 2026.09.30-04.55.23 Stats.csv"), as local seconds since 1970.
fn name_stamp(name: &str) -> Option<i64> {
    let b = name.as_bytes();
    let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    (0..b.len().saturating_sub(18)).find(|&i| {
        let s = &b[i..i + 19];
        digits(&s[0..4]) && s[4] == b'.' && digits(&s[5..7]) && s[7] == b'.' && digits(&s[8..10])
            && s[10] == b'-' && digits(&s[11..13]) && s[13] == b'.' && digits(&s[14..16]) && s[16] == b'.'
            && digits(&s[17..19])
    })
    .and_then(|i| {
        let num = |a: usize, z: usize| name[i + a..i + z].parse::<i64>().unwrap();
        let (y, mo, d, h, mi, s) = (num(0, 4), num(5, 7), num(8, 10), num(11, 13), num(14, 16), num(17, 19));
        let valid = (1..=9999).contains(&y) && (1..=12).contains(&mo) && d >= 1 && d <= days_in_month(y, mo) && h < 24 && mi < 60 && s < 60;
        valid.then(|| days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s)
    })
}

/// A time of day as `strptime(text, "%H:%M:%S.%f")` reads it, in microseconds; None where it fails.
fn clock_micros(text: &str) -> Option<i64> {
    let (hms, frac) = text.split_once('.')?;
    let mut parts = hms.split(':');
    let mut field = |max: i64| -> Option<i64> {
        let s = parts.next()?;
        let ok = !s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit());
        let v: i64 = if ok { s.parse().ok()? } else { return None };
        (v <= max).then_some(v)
    };
    let (h, m, s) = (field(23)?, field(59)?, field(59)?);
    if parts.next().is_some() || frac.is_empty() || frac.len() > 6 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let f: i64 = format!("{frac:0<6}").parse().ok()?;
    Some(((h * 60 + m) * 60 + s) * 1_000_000 + f)
}

/// Python's `int(text)` for plain decimal text.
fn py_int(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

/// Python's `float(text)`.
fn py_float(text: &str) -> Option<f64> {
    text.trim().parse().ok()
}

/// Reads a stats file. name: its file name (it holds the date); utc_offset: local minus UTC, seconds.
pub fn read_stats(name: &str, text: &str, utc_offset: i64) -> Result<StatsRun, String> {
    let l = lines(text);
    let mut meta: HashMap<&str, &str> = HashMap::new();
    for line in &l {
        if let Some((k, v)) = line.split_once(":,") {
            meta.insert(k, v);
        }
    }
    let end = name_stamp(name).ok_or("the stats file name holds no date (expected '... - 2026.09.30-04.55.23 Stats.csv')")?;
    let end_us = end * 1_000_000;
    // a time of day on the stats file's date (the day before for a run over midnight), as seconds since 1970
    let epoch = |hms: &str| -> Result<f64, String> {
        let us = clock_micros(hms.trim()).ok_or_else(|| format!("time data {:?} does not match format '%H:%M:%S.%f'", hms.trim()))?;
        let mut d = end.div_euclid(86_400) * 86_400 * 1_000_000 + us;
        if d > end_us + 3_600_000_000 {
            d -= 86_400 * 1_000_000;
        }
        let (secs, micro) = (d.div_euclid(1_000_000), d.rem_euclid(1_000_000));
        Ok((secs - utc_offset) as f64 + micro as f64 / 1e6)
    };
    let mut kills = Vec::new();
    for line in l.iter().skip(1) {
        if line.trim().is_empty() {
            break;
        }
        let r: Vec<&str> = line.split(',').collect();
        let bad = || format!("a kill row the reader cannot read: {line:?}");
        let n = py_int(r[0]).ok_or_else(bad)?;
        let local = r.get(1).ok_or_else(bad)?.to_string();
        let t = epoch(&local)?;
        let shots = r.get(5).and_then(|s| py_int(s)).ok_or_else(bad)?;
        kills.push(StatsKill { n, local, t, shots });
    }
    let mut shots = 0;
    if let Some(w) = l.iter().position(|line| line.starts_with("Weapon,Shots")) {
        for line in &l[w + 1..] {
            if line.trim().is_empty() {
                break;
            }
            let v = line.split(',').nth(1).and_then(py_float).filter(|v| v.is_finite());
            shots += v.ok_or_else(|| format!("a weapon row the reader cannot read: {line:?}"))?.trunc() as i64;
        }
    }
    let sens = if meta.get("Sens Scale").is_some_and(|s| s.trim() == "cm/360") {
        let get = |k: &str| meta.get(k).and_then(|v| py_float(v)).ok_or(format!("the stats file's {k} cannot be read"));
        Some((get("DPI")?, get("Horiz Sens")?))
    } else {
        None
    };
    let start = match meta.get("Challenge Start") {
        Some(v) => epoch(v)?,
        None => kills.first().ok_or("the stats file has no kills and no challenge start")?.t - 1.0,
    };
    Ok(StatsRun {
        kills,
        start,
        end: (end - utc_offset) as f64 + 1.0,
        shots,
        sens,
        scenario: meta.get("Scenario").map_or(String::new(), |s| s.trim().to_string()),
    })
}

// ---- the run ----

/// The offset (log time minus stats time) that puts the most kills within MATCH_TOL of a press, and each kill's
/// press index (None when none is within MATCH_TOL).
fn match_kills(presses: &[f64], kills: &[f64]) -> (Option<f64>, Vec<Option<usize>>) {
    let mut d: Vec<(f64, usize)> = Vec::new();
    for (ki, &k) in kills.iter().enumerate() {
        let (i, j) = (bisect_left(presses, k - SEARCH), bisect_right(presses, k + SEARCH, 0));
        d.extend((i..j).map(|x| (presses[x] - k, ki)));
    }
    d.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    let mut cnt = vec![0usize; kills.len()];
    let (mut distinct, mut j, mut best) = (0usize, 0usize, (0usize, 0usize, 0usize));
    for i in 0..d.len() {
        cnt[d[i].1] += 1;
        if cnt[d[i].1] == 1 {
            distinct += 1;
        }
        while d[i].0 - d[j].0 > MATCH_TOL {
            cnt[d[j].1] -= 1;
            if cnt[d[j].1] == 0 {
                distinct -= 1;
            }
            j += 1;
        }
        if distinct > best.0 {
            best = (distinct, j, i);
        }
    }
    if best.0 == 0 {
        return (None, vec![None; kills.len()]);
    }
    let off = median(&d[best.1..=best.2].iter().map(|x| x.0).collect::<Vec<_>>());
    let idx = kills
        .iter()
        .map(|&k| {
            let (lo, hi) = (bisect_left(presses, k + off - MATCH_TOL), bisect_right(presses, k + off + MATCH_TOL, 0));
            let key = |x: usize| (presses[x] - k - off).abs();
            (lo..hi).reduce(|a, b| if key(b) < key(a) { b } else { a })
        })
        .collect();
    (Some(off), idx)
}

/// One measure over the kills: how many have it, and its p10, median and p90.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spread {
    /// The measure's field in `KillMeasure`.
    pub key: String,
    pub n: usize,
    pub p10: f64,
    pub median: f64,
    pub p90: f64,
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

/// Measures the run: the stats file's kills matched with the log's presses, then each kill's flick.
pub fn run(log: &MouseLog, stats: &StatsRun, options: &Options, utc_offset: i64) -> Result<MouseRun, String> {
    let mut o = options.clone();
    let (opt_dpi, opt_cm) = (o.dpi.filter(|&v| v != 0.0), o.cm360.filter(|&v| v != 0.0));
    let (dpi, cm360) = (
        opt_dpi.unwrap_or(stats.sens.map_or(1600.0, |s| s.0)),
        opt_cm.unwrap_or(stats.sens.map_or(70.0, |s| s.1)),
    );
    let k = deg_per_count(dpi, cm360);
    let src = if opt_dpi.is_some() || opt_cm.is_some() {
        "options"
    } else if stats.sens.is_some() {
        "the stats file"
    } else {
        "defaults"
    };
    let facts = log_facts(log, utc_offset);
    let mut widened = false;
    if let Some(med) = facts.median_interval
        && log.t.len() >= 200
        && 2.0 * med * 1000.0 > o.window
    {
        o.window = round(2.0 * med * 1000.0, 2);
        widened = true;
    }
    let w0 = log.wall0;
    let kills: Vec<f64> = stats.kills.iter().map(|r| r.t - w0).collect();
    if kills.is_empty() || kills[kills.len() - 1] < -SEARCH || kills[0] > log.duration + SEARCH {
        return Err(format!(
            "the log ({} to {}) does not cover this run ({} on)",
            local_clock(w0, utc_offset),
            local_clock(w0 + log.duration, utc_offset),
            stats.kills.first().map_or("?", |r| r.local.as_str())
        ));
    }
    let presses = presses_of(log);
    let (off, idx) = match_kills(&presses, &kills);
    let off = off.ok_or("no left-button press lies within 1 s of any kill")?;
    let gaps: Vec<f64> =
        idx.iter().zip(&kills).filter_map(|(i, k)| i.map(|i| (presses[i] - k - off).abs() * 1000.0)).collect();
    let (lo, hi) = (stats.start - w0 + off, stats.end - w0 + off);
    let used: std::collections::HashSet<usize> = idx.iter().flatten().copied().collect();
    let in_run: Vec<usize> = (0..presses.len()).filter(|&i| lo <= presses[i] && presses[i] <= hi).collect();
    let misses: Vec<f64> = in_run.iter().filter(|i| !used.contains(i)).map(|&i| presses[i]).collect();
    let m = Motion::new(log, k);
    let (mut out, mut a) = (Vec::new(), stats.start - w0 + off);
    for ((r, &kt), i) in stats.kills.iter().zip(&kills).zip(&idx) {
        let Some(i) = *i else {
            a = kt + off;
            continue;
        };
        let c = presses[i];
        let base = KillMeasure {
            n: r.n,
            kill_local: r.local.clone(),
            press_local: local_clock(w0 + c, utc_offset),
            press_s: round(c, 6),
            gap_ms: round((c - kt - off) * 1000.0, 3),
            shots: r.shots,
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
        };
        out.push(measure(&m, a, c, &o, base)?);
        a = c;
    }
    let spread = |key: &str, vals: Vec<Option<f64>>| {
        let vals: Vec<f64> = vals.into_iter().flatten().collect();
        (!vals.is_empty()).then(|| Spread {
            key: key.to_string(),
            n: vals.len(),
            p10: decile(&vals, 0.1),
            median: median(&vals),
            p90: decile(&vals, 0.9),
        })
    };
    let spreads = [
        spread("reaction_ms", out.iter().map(|r| r.reaction_ms).collect()),
        spread("flick_ms", out.iter().map(|r| r.flick_ms).collect()),
        spread("peak_dps", out.iter().map(|r| Some(r.peak_dps)).collect()),
        spread("stop_to_click_ms", out.iter().map(|r| r.stop_to_click_ms).collect()),
        spread("still_ms", out.iter().map(|r| Some(r.still_ms)).collect()),
        spread("click_dps", out.iter().map(|r| Some(r.click_dps)).collect()),
        spread("dist_deg", out.iter().map(|r| Some(r.dist_deg)).collect()),
    ]
    .into_iter()
    .flatten()
    .collect();
    Ok(MouseRun {
        log: facts,
        scenario: stats.scenario.clone(),
        dpi,
        cm360,
        sens_from: src.to_string(),
        deg_per_count: k,
        window_ms: o.window,
        window_widened: widened,
        start_dps: o.start,
        stop_dps: o.stop,
        hold_ms: o.hold,
        offset_ms: round(off * 1000.0, 3),
        offset_s: off,
        kill_count: kills.len(),
        matched: gaps.len(),
        gap_p90_ms: if gaps.is_empty() { 0.0 } else { decile(&gaps, 0.9) },
        presses_in_run: in_run.len(),
        shots: stats.shots,
        misses_s: misses.iter().map(|&t| round(t, 6)).collect(),
        moving_clicks: out.iter().filter(|r| r.still_ms == 0.0).count(),
        no_stop: out.iter().filter(|r| r.stop_s.is_none()).count(),
        corrected: out.iter().filter(|r| r.corrections.is_some_and(|c| c > 0)).count(),
        kills: out,
        spreads,
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
            .map(|r| ReadOutcome::Run(Box::new(r))),
        _ => Ok(ReadOutcome::Summary(summary(&log, &request.options, request.utc_offset))),
    });
    outcome.unwrap_or_else(ReadOutcome::Error)
}

/// `read` with the request as JSON, the outcome as JSON ({run}, {summary} or {error}).
pub fn read_json(log: &[u8], request: &[u8]) -> Vec<u8> {
    let outcome = match serde_json::from_slice::<ReadRequest>(request) {
        Ok(req) => read(log, &req),
        Err(e) => ReadOutcome::Error(format!("the request cannot be read: {e}")),
    };
    serde_json::to_vec(&outcome).unwrap_or_default()
}

/// The wall times a log covers (seconds since 1970), from its header and its last record only: for finding the log
/// of a run among many without reading them whole. None when it is not a mouse log.
pub fn log_span(head: &[u8], last: &[u8]) -> Option<(f64, f64)> {
    let (freq, q0, ns0) = read_header(head)?;
    let wall0 = ns0 as f64 / 1e9;
    if last.len() < RECORD_SIZE || freq <= 0 {
        return Some((wall0, wall0));
    }
    let q = i64_at(last, 0);
    let end = if q == KIND_STOP {
        i64_at(last, 16) as f64 / 1e9
    } else if q >= 0 {
        wall0 + (q - q0) as f64 / freq as f64
    } else {
        wall0
    };
    Some((wall0, end.max(wall0)))
}

// ---- the printed report (the command-line readers) ----

/// Python's `format(x, "g")`.
pub fn fmt_g(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return if x.is_nan() { "nan".into() } else if x.is_infinite() { if x > 0.0 { "inf" } else { "-inf" }.into() } else { "0".into() };
    }
    let sci = format!("{x:.5e}");
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if (-4..6).contains(&exp) {
        let s = format!("{x:.*}", (5 - exp) as usize);
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    } else {
        let mant = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
        format!("{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

/// Python's `str(x)` for a float of ordinary size.
fn py_str(x: f64) -> String {
    let s = format!("{x}");
    if s.contains(['.', 'e', 'i', 'N']) { s } else { s + ".0" }
}

fn head_text(f: &LogFacts, path: &str) -> String {
    let drift = f.drift_ms.map_or("no stop pair (killed?)".to_string(), |d| format!("clock drift {d:.3} ms"));
    let mut s = format!("log: {path}\n  {} to {}, {:.2} s, {} events, {drift}\n", f.start_local, f.end_local, f.duration, f.events);
    for (d, dev) in f.devices.iter().enumerate() {
        s += &format!("  device {d}: handle {:#x}, {} events\n", dev.handle, dev.events);
    }
    if f.absolute > 0 {
        s += &format!("  {} absolute events (MOUSE_MOVE_ABSOLUTE) skipped\n", f.absolute);
    }
    let rate = match f.median_interval {
        Some(med) => {
            let hz = if med != 0.0 { 1.0 / med } else { f64::INFINITY };
            let mut t = format!("median interval {:.3} ms ({hz:.0} Hz), busiest 100 ms {:.0} Hz", med * 1000.0, f.busiest_hz);
            if f.throttled {
                t += "\n  warning: events are far apart; Windows probably throttled the logger (about 8 ms when \
                      throttled), so times are only good to about the interval";
            }
            t
        }
        None => "too few events for a rate".into(),
    };
    s + "  " + &rate + "\n"
}

/// What mouse_read.py prints for a log on its own.
pub fn summary_text(s: &LogSummary, path: &str) -> String {
    head_text(&s.log, path)
        + &format!(
            "  travel {:.1} deg (1 ms steps, {:.6} deg per count), left-button presses {}\n",
            s.travel_deg, s.deg_per_count, s.presses
        )
}

/// What mouse_read.py prints for a run (up to the line naming the file it writes).
pub fn run_text(r: &MouseRun, path: &str, stats_name: &str) -> String {
    let mut s = head_text(&r.log, path);
    if r.window_widened {
        s += &format!("  speed window widened to {} ms (twice the median interval)\n", py_str(r.window_ms));
    }
    s += &format!(
        "stats: {stats_name}\n  {} dpi, {} cm/360 (from {}), {:.6} deg per count\n",
        fmt_g(r.dpi),
        fmt_g(r.cm360),
        r.sens_from,
        r.deg_per_count
    );
    s += &format!(
        "  clock offset (press minus stats kill time) {:+.1} ms; {} of {} kills matched within 10 ms (gap p90 {:.1} ms)\n",
        r.offset_s * 1000.0,
        r.matched,
        r.kill_count,
        r.gap_p90_ms
    );
    s += &format!("  presses in the run {} (stats shots {}), misses {}\n", r.presses_in_run, r.shots, r.misses_s.len());
    s += &format!("{} kills measured (times from the previous kill's press)\n", r.kills.len());
    let labels = [
        ("reaction_ms", "reaction: start (ms)", 1),
        ("flick_ms", "flick: start to stop (ms)", 1),
        ("peak_dps", "peak speed (deg/s)", 0),
        ("stop_to_click_ms", "stop to click (ms)", 1),
        ("still_ms", "still before the click (ms)", 1),
        ("click_dps", "speed at the click (deg/s)", 1),
        ("dist_deg", "distance (deg)", 1),
    ];
    for (key, label, digits) in labels {
        if let Some(sp) = r.spreads.iter().find(|sp| sp.key == key) {
            s += &format!(
                "  {label:34} n {:3}  p10 {:7.digits$}  median {:7.digits$}  p90 {:7.digits$}\n",
                sp.n, sp.p10, sp.median, sp.p90
            );
        }
    }
    s += &format!(
        "  clicked while moving: {} of {}; no stop before the click: {}; with corrections: {}\n",
        r.moving_clicks,
        r.kills.len(),
        r.no_stop,
        r.corrected
    );
    s
}

/// The `<log>.kills.json` mouse_read.py writes beside a log.
pub fn kills_json(r: &MouseRun, log_path: &str, stats_path: &str) -> serde_json::Value {
    serde_json::json!({
        "log": log_path,
        "stats": stats_path,
        "offset_ms": r.offset_ms,
        "dpi": r.dpi,
        "cm360": r.cm360,
        "window_ms": r.window_ms,
        "start_dps": r.start_dps,
        "stop_dps": r.stop_dps,
        "hold_ms": r.hold_ms,
        "misses_s": r.misses_s,
        "kills": r.kills,
    })
}
