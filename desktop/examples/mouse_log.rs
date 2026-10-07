//! The raw mouse logger on its own (python/mouse_log.py's command line): logs the mouse to a file while KovaaK's runs.
//! Ctrl+C stops it; it then prints the events, the duration, the rates and the devices.
//! cargo run -p aimview-desktop --release --example mouse_log -- [--out FILE] [--seconds N]
//!   (default file: test_out/mouse/mouse_<date>_<time>.bin; an existing file is never overwritten)
//! cargo run -p aimview-desktop --release --example mouse_log -- --bench
//!   times the per-event code path without real input, as mouse_log.py --bench does
//! cargo run -p aimview-desktop --release --example mouse_log -- --stream SECONDS [--hz N] [--out FILE]
//!   checks what the logger records of a stream it is sent: SendInput mouse moves of zero counts (the cursor stays
//!   where it is, no button is pressed), N a second (default 8000), with the logger in the background as it is while
//!   KovaaK's has focus

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aimview::mouse as reader;
use aimview_desktop::mouse::{bench, local_stamp, log_to, logged_text, time_ns};

/// Set by Ctrl+C, Ctrl+Break or the console closing: the logger stops.
static STOP: AtomicBool = AtomicBool::new(false);
/// Set once the logger has written its stop pair, which a closing console waits for.
static DONE: AtomicBool = AtomicBool::new(false);

/// The console control event for the console closing (Windows' CTRL_CLOSE_EVENT).
const CTRL_CLOSE_EVENT: u32 = 2;
/// How long a closing console waits for the logger's stop pair.
const CLOSE_WAIT: Duration = Duration::from_secs(3);
/// `--bench`'s WM_INPUT messages posted in each of its rounds (mouse_log.py --bench's).
const BENCH_MESSAGES: usize = 5000;
/// `--bench`'s records decoded.
const BENCH_RECORDS: usize = 200_000;
/// `--stream`'s moves a second when `--hz` is not given.
const DEFAULT_STREAM_HZ: f64 = 8000.0;
/// How long the stream waits for the logger to start.
const LOGGER_START: Duration = Duration::from_millis(500);
/// How much longer than the stream the logger runs, in seconds.
const LOGGER_EXTRA_S: f64 = 1.0;
/// SendInput's INPUT_MOUSE: the input is the mouse's.
const INPUT_MOUSE: u32 = 0;
/// MOUSEEVENTF_MOVE: a relative move.
const MOUSEEVENTF_MOVE: u32 = 0x0001;
/// A move counts as sent late when it went out more than this many intervals after it was due.
const LATE_INTERVALS: f64 = 2.0;
/// Milliseconds in a second.
const MS_PER_S: f64 = 1000.0;
/// Nanoseconds in a second.
const NS_PER_S: f64 = 1e9;

/// Windows' MOUSEINPUT: one mouse event for SendInput.
#[repr(C)]
struct MouseInput {
    /// The move right, in counts (relative with MOUSEEVENTF_MOVE).
    dx: i32,
    /// The move down, in counts.
    dy: i32,
    /// The wheel's or the X buttons' data; 0 here.
    data: u32,
    /// What the event is (MOUSEEVENTF_*).
    flags: u32,
    /// Its time stamp in ms; 0: the system's.
    time: u32,
    /// Extra information; 0 here.
    extra: usize,
}

/// Windows' INPUT for a mouse event.
#[repr(C)]
struct Input {
    /// INPUT_MOUSE.
    kind: u32,
    /// The event.
    mouse: MouseInput,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    /// Adds (`add` 1) a handler for Ctrl+C, Ctrl+Break and the console closing.
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    /// Sends `count` input events; gives how many went in.
    fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
}

/// Ctrl+C, Ctrl+Break or the console closing: the logger stops and writes its stop pair (a closing console waits for
/// it, up to 3 s).
unsafe extern "system" fn on_ctrl(kind: u32) -> i32 {
    STOP.store(true, Ordering::Relaxed);
    if kind == CTRL_CLOSE_EVENT {
        let closing = Instant::now();
        while !DONE.load(Ordering::Relaxed) && closing.elapsed() < CLOSE_WAIT {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    1
}

/// The log's file when --out is not given: test_out/mouse/<prefix>_<local time stamp>.bin.
fn default_out(prefix: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../test_out/mouse")
        .join(format!("{prefix}_{}.bin", local_stamp(time_ns() as f64 / NS_PER_S)))
}

/// Runs --bench, --stream, or the logger until Ctrl+C (or --seconds), then prints what it logged.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| args.iter().position(|arg| arg == name).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|arg| arg == "--bench") {
        match bench(BENCH_MESSAGES, BENCH_RECORDS) {
            Ok(text) => println!("{text}"),
            Err(error) => eprintln!("{error}"),
        }
        return;
    }
    if let Some(seconds) = value("--stream").and_then(|arg| arg.parse::<f64>().ok()) {
        let hz = value("--hz").and_then(|arg| arg.parse().ok()).unwrap_or(DEFAULT_STREAM_HZ);
        stream(seconds, hz, value("--out"));
        return;
    }
    let out = value("--out").map_or_else(|| default_out("mouse"), PathBuf::from);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).expect("the log's folder");
    }
    // SAFETY: the handler only sets flags and waits
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
    let seconds = value("--seconds").and_then(|arg| arg.parse().ok());
    let logged = log_to(&out, seconds, &STOP, &|line| println!("{line}"));
    DONE.store(true, Ordering::Relaxed);
    let logged = logged.unwrap_or_else(|error| panic!("{}: {error}", out.display()));
    if logged.bad > 0 {
        println!("  {} WM_INPUT messages could not be read", logged.bad);
    }
    let log = reader::read_log(&std::fs::read(&out).unwrap()).unwrap();
    print!("{}", logged_text(&log, &out, &logged.names));
}

/// Sends zero moves at hz for seconds while the logger runs in a thread, then says what it recorded.
fn stream(seconds: f64, hz: f64, out: Option<String>) {
    let out = out.map_or_else(|| default_out("stream"), PathBuf::from);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).expect("the log's folder");
    }
    let path = out.clone();
    let logged_s = Some(seconds + LOGGER_EXTRA_S);
    let logger = std::thread::spawn(move || log_to(&path, logged_s, &STOP, &|line| println!("{line}")));
    std::thread::sleep(LOGGER_START);
    let mouse = MouseInput { dx: 0, dy: 0, data: 0, flags: MOUSEEVENTF_MOVE, time: 0, extra: 0 };
    let input = Input { kind: INPUT_MOUSE, mouse };
    let (started, mut sent, mut late) = (Instant::now(), 0u64, 0u64);
    while started.elapsed().as_secs_f64() < seconds {
        let due = sent as f64 / hz;
        let now = started.elapsed().as_secs_f64();
        if now < due {
            std::hint::spin_loop();
            continue;
        }
        if now - due > LATE_INTERVALS / hz {
            late += 1;
        }
        // SAFETY: one INPUT of the size given: a relative move of zero counts
        sent += u64::from(unsafe { SendInput(1, &input, size_of::<Input>() as i32) });
    }
    let logged = logger.join().unwrap().unwrap_or_else(|error| panic!("{}: {error}", out.display()));
    let log = reader::read_log(&std::fs::read(&out).unwrap()).unwrap();
    print!("{}", logged_text(&log, &out, &logged.names));
    // a UTC offset of 0: the numbers printed here do not depend on the time zone
    let facts = reader::log_facts(&log, 0);
    let median_ms = facts.median_interval.map(|interval_s| interval_s * MS_PER_S);
    let median = median_ms.map_or("-".into(), |ms| format!("{ms:.3} ms"));
    println!(
        "stream: sent {sent} moves in {seconds} s ({:.0} a second, {late} sent late); logged {} events, median \
         interval {median}, busiest 100 ms {:.0} Hz",
        sent as f64 / seconds,
        facts.events,
        facts.busiest_hz
    );
}
