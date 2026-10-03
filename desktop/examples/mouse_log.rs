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

static STOP: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);

#[repr(C)]
struct MouseInput {
    dx: i32,
    dy: i32,
    data: u32,
    flags: u32,
    time: u32,
    extra: usize,
}

#[repr(C)]
struct Input {
    kind: u32,
    mouse: MouseInput,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SendInput(n: u32, inputs: *const Input, size: i32) -> u32;
}

/// Ctrl+C, Ctrl+Break or the console closing: the logger stops and writes its stop pair (a closing console waits for
/// it, up to 3 s).
unsafe extern "system" fn on_ctrl(kind: u32) -> i32 {
    STOP.store(true, Ordering::Relaxed);
    if kind == 2 {
        let t0 = Instant::now();
        while !DONE.load(Ordering::Relaxed) && t0.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    1
}

fn default_out(prefix: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../test_out/mouse")
        .join(format!("{prefix}_{}.bin", local_stamp(time_ns() as f64 / 1e9)))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let value = |name: &str| a.iter().position(|v| v == name).and_then(|i| a.get(i + 1)).cloned();
    if a.iter().any(|v| v == "--bench") {
        match bench(5000, 200_000) {
            Ok(text) => println!("{text}"),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    if let Some(seconds) = value("--stream").and_then(|v| v.parse::<f64>().ok()) {
        stream(seconds, value("--hz").and_then(|v| v.parse().ok()).unwrap_or(8000.0), value("--out"));
        return;
    }
    let out = value("--out").map_or_else(|| default_out("mouse"), PathBuf::from);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).expect("the log's folder");
    }
    // SAFETY: the handler only sets flags and waits
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
    let seconds = value("--seconds").and_then(|v| v.parse().ok());
    let logged = log_to(&out, seconds, &STOP, &|line| println!("{line}"));
    DONE.store(true, Ordering::Relaxed);
    let logged = logged.unwrap_or_else(|e| panic!("{}: {e}", out.display()));
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
    let logger = std::thread::spawn(move || log_to(&path, Some(seconds + 1.0), &STOP, &|line| println!("{line}")));
    std::thread::sleep(Duration::from_millis(500));
    let input = Input { kind: 0, mouse: MouseInput { dx: 0, dy: 0, data: 0, flags: 0x0001, time: 0, extra: 0 } };
    let (t0, mut sent, mut late) = (Instant::now(), 0u64, 0u64);
    while t0.elapsed().as_secs_f64() < seconds {
        let due = sent as f64 / hz;
        let now = t0.elapsed().as_secs_f64();
        if now < due {
            std::hint::spin_loop();
            continue;
        }
        if now - due > 2.0 / hz {
            late += 1;
        }
        // SAFETY: one INPUT of the size given: a relative move of zero counts
        sent += u64::from(unsafe { SendInput(1, &input, size_of::<Input>() as i32) });
    }
    let logged = logger.join().unwrap().unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    let log = reader::read_log(&std::fs::read(&out).unwrap()).unwrap();
    print!("{}", logged_text(&log, &out, &logged.names));
    let f = reader::log_facts(&log, 0);
    let median = f.median_interval.map_or("-".into(), |m| format!("{:.3} ms", m * 1000.0));
    println!(
        "stream: sent {sent} moves in {seconds} s ({:.0} a second, {late} sent late); logged {} events, median interval \
         {median}, busiest 100 ms {:.0} Hz",
        sent as f64 / seconds,
        f.events,
        f.busiest_hz
    );
}
