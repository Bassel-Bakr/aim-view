//! The raw mouse logger (python/mouse_log.py, ported) and the app's side of it: an on/off switch that logs in the
//! background while the user plays, and each recording's measures from the log that covers its run (src/mouse.rs reads
//! it, as python/mouse_read.py does).
//!
//! The logger registers a message-only window for raw mouse input with RIDEV_INPUTSINK, so it keeps receiving input
//! while KovaaK's has focus. It never sends input and never moves the cursor. Each WM_INPUT becomes one record in
//! mouse_log.py's format (src/mouse.rs), so logs from either logger read the same.
//!
//! Raw input goes to one window per process, and tao (Tauri's windows) already registers the mouse in the app's
//! process. So the app runs the logger in a process of its own: its own executable with `--mouse-log <file>`
//! (`child_main`), which stops cleanly, writing the stop pair, when its standard input closes (the switch is turned off,
//! or the app ends).
//!
//! Windows 11 throttles raw input to background programs to about 125 Hz, unless the user turns that off
//! (HKCU\Control Panel\Mouse, RawMouseThrottleEnabled = 0, then sign out and in): python/README.md, "Raw mouse log".

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aimview::mouse::{self as reader, MouseLog};
use serde_json::{Value, json};

use crate::library::{Answer, Failure, Library};

// ---- Windows ----

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub const WM_INPUT: u32 = 0x00FF;
    pub const PM_REMOVE: u32 = 0x0001;
    pub const RID_INPUT: u32 = 0x1000_0003;
    pub const RIM_TYPEMOUSE: u32 = 0;
    pub const RIM_INPUT: usize = 0;
    pub const RIDEV_REMOVE: u32 = 0x0000_0001;
    pub const RIDEV_INPUTSINK: u32 = 0x0000_0100;
    pub const RIDI_DEVICENAME: u32 = 0x2000_0007;
    pub const HWND_MESSAGE: isize = -3;
    pub const QS_ALLINPUT: u32 = 0x04FF;
    pub const MWMO_INPUTAVAILABLE: u32 = 0x0004;
    pub const THREAD_PRIORITY_HIGHEST: i32 = 2;
    pub const FAIL: u32 = u32::MAX;
    /// HKEY_CURRENT_USER, sign-extended as the headers define it.
    pub const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    pub const KEY_READ: u32 = 0x2_0019;
    pub const REG_SZ: u32 = 1;
    pub const REG_DWORD: u32 = 4;

    #[repr(C)]
    pub struct Msg {
        pub hwnd: isize,
        pub message: u32,
        pub wparam: usize,
        pub lparam: isize,
        pub time: u32,
        pub pt_x: i32,
        pub pt_y: i32,
        pub private: u32,
    }

    #[repr(C)]
    pub struct RawInputDevice {
        pub usage_page: u16,
        pub usage: u16,
        pub flags: u32,
        pub target: isize,
    }

    /// SYSTEMTIME.
    #[repr(C)]
    #[derive(Default)]
    pub struct SystemTime {
        pub year: u16,
        pub month: u16,
        pub weekday: u16,
        pub day: u16,
        pub hour: u16,
        pub minute: u16,
        pub second: u16,
        pub ms: u16,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        #[allow(clippy::too_many_arguments)]
        pub fn CreateWindowExW(
            ex: u32,
            class: *const u16,
            name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: isize,
            menu: isize,
            instance: isize,
            param: *mut c_void,
        ) -> isize;
        pub fn DestroyWindow(hwnd: isize) -> i32;
        pub fn RegisterRawInputDevices(devices: *const RawInputDevice, n: u32, size: u32) -> i32;
        pub fn GetRawInputData(input: isize, command: u32, data: *mut c_void, size: *mut u32, header: u32) -> u32;
        pub fn GetRawInputDeviceInfoW(device: isize, command: u32, data: *mut c_void, size: *mut u32) -> u32;
        pub fn PeekMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32, remove: u32) -> i32;
        pub fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
        pub fn TranslateMessage(msg: *const Msg) -> i32;
        pub fn DispatchMessageW(msg: *const Msg) -> isize;
        pub fn DefWindowProcW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        pub fn MsgWaitForMultipleObjectsEx(n: u32, handles: *const isize, ms: u32, mask: u32, flags: u32) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn QueryPerformanceCounter(count: *mut i64) -> i32;
        pub fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
        pub fn GetCurrentThread() -> isize;
        pub fn SetThreadPriority(thread: isize, priority: i32) -> i32;
        pub fn FileTimeToSystemTime(file_time: *const u64, system_time: *mut SystemTime) -> i32;
        pub fn SystemTimeToFileTime(system_time: *const SystemTime, file_time: *mut u64) -> i32;
        pub fn SystemTimeToTzSpecificLocalTime(zone: *const c_void, utc: *const SystemTime, local: *mut SystemTime) -> i32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        pub fn RegOpenKeyExW(key: isize, sub: *const u16, options: u32, sam: u32, out: *mut isize) -> i32;
        #[allow(clippy::too_many_arguments)]
        pub fn RegEnumValueW(
            key: isize,
            index: u32,
            name: *mut u16,
            name_len: *mut u32,
            reserved: *mut u32,
            kind: *mut u32,
            data: *mut u8,
            data_len: *mut u32,
        ) -> i32;
        pub fn RegCloseKey(key: isize) -> i32;
    }

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
}

/// The QueryPerformanceCounter time.
#[cfg(windows)]
pub fn qpc() -> i64 {
    let mut q = 0i64;
    // SAFETY: the call writes one i64
    unsafe { win::QueryPerformanceCounter(&mut q) };
    q
}

/// Nanoseconds since 1970 (Python's `time.time_ns()`).
pub fn time_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64)
}

/// This computer's offset from UTC (local minus UTC, seconds) at a moment (seconds since 1970), daylight saving time
/// included, as Python's local time conversions take it.
pub fn utc_offset_at(secs: f64) -> i64 {
    #[cfg(windows)]
    {
        // FILETIME: 100 ns steps since 1601
        let ft = ((secs.floor() as i64 + 11_644_473_600) * 10_000_000) as u64;
        let (mut utc, mut local, mut back) = (win::SystemTime::default(), win::SystemTime::default(), 0u64);
        // SAFETY: each call reads and writes the structs it is given
        let ok = unsafe {
            win::FileTimeToSystemTime(&ft, &mut utc) != 0
                && win::SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
                && win::SystemTimeToFileTime(&local, &mut back) != 0
        };
        if ok {
            return (back as i64 - ft as i64).div_euclid(10_000_000);
        }
    }
    let _ = secs;
    0
}

/// A moment (seconds since 1970) as a local "2026-10-01_03-17-24", the logs' file names.
pub fn local_stamp(secs: f64) -> String {
    let t = secs.floor() as i64 + utc_offset_at(secs);
    let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // the civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}_{:02}-{:02}-{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

/// The background raw input throttle values under HKCU\Control Panel\Mouse, read only.
pub fn throttle_setting() -> String {
    let mut found: Vec<(String, String)> = Vec::new();
    #[cfg(windows)]
    {
        let mut key = 0isize;
        let sub = win::wide(r"Control Panel\Mouse");
        // SAFETY: the key is opened read only, enumerated into buffers of the sizes given, and closed
        unsafe {
            if win::RegOpenKeyExW(win::HKEY_CURRENT_USER, sub.as_ptr(), 0, win::KEY_READ, &mut key) == 0 {
                for i in 0.. {
                    let (mut name, mut data) = ([0u16; 256], [0u8; 1024]);
                    let (mut name_len, mut data_len, mut kind) = (name.len() as u32, data.len() as u32, 0u32);
                    let r = win::RegEnumValueW(
                        key,
                        i,
                        name.as_mut_ptr(),
                        &mut name_len,
                        std::ptr::null_mut(),
                        &mut kind,
                        data.as_mut_ptr(),
                        &mut data_len,
                    );
                    if r != 0 {
                        break;
                    }
                    let name = String::from_utf16_lossy(&name[..name_len as usize]);
                    if name.starts_with("RawMouseThrottl") {
                        let value = match kind {
                            win::REG_DWORD if data_len >= 4 => u32::from_le_bytes(data[..4].try_into().unwrap()).to_string(),
                            win::REG_SZ => {
                                let w: Vec<u16> = data[..data_len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                                String::from_utf16_lossy(&w).trim_end_matches('\0').to_string()
                            }
                            _ => format!("{:?}", &data[..data_len as usize]),
                        };
                        found.push((name, value));
                    }
                }
                win::RegCloseKey(key);
            }
        }
    }
    if found.is_empty() {
        return "background throttle not set (Windows 11 default: about 125 Hz while KovaaK's has focus)".into();
    }
    found.sort();
    let list: Vec<String> = found.iter().map(|(k, v)| format!("{k} = {v}")).collect();
    format!("background throttle: {}", list.join(", "))
}

// ---- the logger ----

/// A RAWINPUT's bytes (64-bit layout: the header, then RAWMOUSE).
#[cfg(windows)]
#[repr(C, align(8))]
struct RawBuffer([u8; 48]);

/// A message-only window that turns WM_INPUT messages into records in `out`.
#[cfg(windows)]
pub struct Logger {
    hwnd: isize,
    pub freq: i64,
    pub out: Vec<u8>,
    devices: HashMap<u64, u16>,
    /// The devices' names, by index.
    pub names: Vec<String>,
    /// WM_INPUT messages that could not be read.
    pub bad: usize,
    raw: RawBuffer,
}

#[cfg(windows)]
impl Logger {
    pub fn new() -> io::Result<Logger> {
        let (class, name) = (win::wide("STATIC"), win::wide("aimview mouse_log"));
        // SAFETY: a message-only window of a system class, with no parameters
        let hwnd = unsafe {
            win::CreateWindowExW(0, class.as_ptr(), name.as_ptr(), 0, 0, 0, 0, 0, win::HWND_MESSAGE, 0, 0, std::ptr::null_mut())
        };
        if hwnd == 0 {
            return Err(io::Error::other(format!("CreateWindowExW failed: {}", io::Error::last_os_error())));
        }
        let mut freq = 0i64;
        // SAFETY: the call writes one i64
        unsafe { win::QueryPerformanceFrequency(&mut freq) };
        Ok(Logger { hwnd, freq, out: Vec::new(), devices: HashMap::new(), names: Vec::new(), bad: 0, raw: RawBuffer([0; 48]) })
    }

    /// Asks for the mouse's raw input (usage page 1, usage 2), in the background too.
    pub fn register(&self) -> io::Result<()> {
        let rid = win::RawInputDevice { usage_page: 1, usage: 2, flags: win::RIDEV_INPUTSINK, target: self.hwnd };
        // SAFETY: one device, of the size given
        if unsafe { win::RegisterRawInputDevices(&rid, 1, size_of::<win::RawInputDevice>() as u32) } == 0 {
            return Err(io::Error::other(format!("RegisterRawInputDevices failed: {}", io::Error::last_os_error())));
        }
        Ok(())
    }

    /// Handles every queued message; WM_INPUT is read here, not in a window procedure.
    pub fn drain(&mut self) {
        let mut msg = win::Msg { hwnd: 0, message: 0, wparam: 0, lparam: 0, time: 0, pt_x: 0, pt_y: 0, private: 0 };
        // SAFETY: the message and the raw input buffer are the sizes the calls are told
        unsafe {
            while win::PeekMessageW(&mut msg, 0, 0, 0, win::PM_REMOVE) != 0 {
                if msg.message == win::WM_INPUT {
                    let t = qpc();
                    let mut size = self.raw.0.len() as u32;
                    if win::GetRawInputData(msg.lparam, win::RID_INPUT, self.raw.0.as_mut_ptr().cast(), &mut size, 24) == win::FAIL {
                        self.bad += 1;
                        continue;
                    }
                    if msg.wparam == win::RIM_INPUT {
                        // input while this window is foreground: let the system clean up
                        win::DefWindowProcW(msg.hwnd, win::WM_INPUT, msg.wparam, msg.lparam);
                    }
                    self.record(t);
                } else {
                    win::TranslateMessage(&msg);
                    win::DispatchMessageW(&msg);
                }
            }
        }
    }

    /// Decodes the RAWINPUT in the buffer and appends one record.
    fn record(&mut self, t: i64) {
        let r = &self.raw.0;
        let le16 = |at: usize| u16::from_le_bytes([r[at], r[at + 1]]);
        let le32 = |at: usize| i32::from_le_bytes(r[at..at + 4].try_into().unwrap());
        if le32(0) as u32 != win::RIM_TYPEMOUSE {
            return;
        }
        let h = u64::from_le_bytes(r[8..16].try_into().unwrap());
        let (flags, buttons, data, x, y) = (le16(24), le16(28), le16(30), le32(36), le32(40));
        let d = match self.devices.get(&h) {
            Some(&d) => d,
            None => self.add_device(h),
        };
        self.out.extend_from_slice(&reader::event(t, x, y, flags, buttons, data, d));
    }

    fn add_device(&mut self, h: u64) -> u16 {
        let d = self.devices.len() as u16;
        self.devices.insert(h, d);
        self.out.extend_from_slice(&reader::device(h, u32::from(d)));
        let mut name = "(no device handle: injected or synthetic input)".to_string();
        let mut n = 0u32;
        // SAFETY: the first call only asks the length (in characters); the second fills a buffer that long
        unsafe {
            if h != 0 && win::GetRawInputDeviceInfoW(h as isize, win::RIDI_DEVICENAME, std::ptr::null_mut(), &mut n) == 0 && n > 0 {
                let mut buf = vec![0u16; n as usize];
                if win::GetRawInputDeviceInfoW(h as isize, win::RIDI_DEVICENAME, buf.as_mut_ptr().cast(), &mut n) != win::FAIL {
                    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    name = String::from_utf16_lossy(&buf[..end]);
                }
            }
        }
        self.names.push(name);
        d
    }

    /// A (QPC, time_ns) pair: the tightest of 20 tries, with time_ns read between two QPC reads.
    pub fn clock_pair(&self) -> (i64, i64) {
        let mut best = (i64::MAX, 0, 0);
        for _ in 0..20 {
            let a = qpc();
            let ns = time_ns();
            let b = qpc();
            if b - a < best.0 {
                best = (b - a, (a + b).div_euclid(2), ns);
            }
        }
        (best.1, best.2)
    }

    pub fn wait(&self, ms: u32) {
        // SAFETY: no handles; it only waits for input or the time
        unsafe { win::MsgWaitForMultipleObjectsEx(0, std::ptr::null(), ms, win::QS_ALLINPUT, win::MWMO_INPUTAVAILABLE) };
    }

    pub fn close(&mut self) {
        let rid = win::RawInputDevice { usage_page: 1, usage: 2, flags: win::RIDEV_REMOVE, target: 0 };
        // SAFETY: the registration is removed and the window destroyed, both by this thread, which made them
        unsafe {
            win::RegisterRawInputDevices(&rid, 1, size_of::<win::RawInputDevice>() as u32);
            win::DestroyWindow(self.hwnd);
        }
    }
}

/// What a finished log holds, as the logger reports it.
pub struct Logged {
    pub names: Vec<String>,
    pub bad: usize,
}

/// Logs the mouse into `out` (a new file: an existing one is never overwritten) until `stop` is set or, with seconds,
/// that long. `say` gets the line the logger prints at its start.
#[cfg(windows)]
pub fn log_to(out: &Path, seconds: Option<f64>, stop: &AtomicBool, say: &dyn Fn(&str)) -> io::Result<Logged> {
    let mut lg = Logger::new()?;
    lg.register()?;
    // SAFETY: raises this thread's priority only
    unsafe { win::SetThreadPriority(win::GetCurrentThread(), win::THREAD_PRIORITY_HIGHEST) };
    let mut f = File::create_new(out)?;
    let (q0, ns0) = lg.clock_pair();
    f.write_all(&reader::header(lg.freq, q0, ns0))?;
    let end = seconds.map(|s| q0 + (s * lg.freq as f64) as i64);
    let how = seconds.map_or("Ctrl+C stops".to_string(), |s| format!("for {} s", reader::fmt_g(s)));
    say(&format!("mouse_log: logging to {} ({how}; QPC {} Hz; {})", out.display(), lg.freq, throttle_setting()));
    let mut last_flush = Instant::now();
    let mut logging = || -> io::Result<()> {
        while !stop.load(Ordering::Relaxed) {
            lg.wait(100);
            lg.drain();
            if lg.out.len() >= 1 << 16 || last_flush.elapsed() > Duration::from_millis(250) {
                // every quarter second, so a killed logger loses little
                f.write_all(&lg.out)?;
                lg.out.clear();
                last_flush = Instant::now();
            }
            if end.is_some_and(|e| qpc() >= e) {
                break;
            }
        }
        Ok(())
    };
    let logged = logging();
    lg.drain();
    let (q1, ns1) = lg.clock_pair();
    lg.out.extend_from_slice(&reader::stop(q1, ns1));
    let wrote = f.write_all(&lg.out).and_then(|_| f.flush());
    lg.close();
    logged.and(wrote)?;
    Ok(Logged { names: std::mem::take(&mut lg.names), bad: lg.bad })
}

/// A number with commas between thousands.
fn thousands(n: f64) -> String {
    let s = format!("{:.0}", n);
    let (sign, digits) = s.strip_prefix('-').map_or(("", s.as_str()), |d| ("-", d));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{sign}{out}")
}

/// What mouse_log.py prints when it stops: the events, the duration, the rates and the devices, with a warning when
/// the log looks throttled.
pub fn logged_text(log: &MouseLog, path: &Path, names: &[String]) -> String {
    let (n, dur) = (log.t.len(), log.duration);
    let peak = reader::busiest_rate(&log.t, 0.1);
    let mean = if dur != 0.0 { n as f64 / dur } else { 0.0 };
    let mut s = format!(
        "mouse_log: {n} events in {dur:.2} s ({mean:.1} Hz mean, busiest 100 ms {peak:.0} Hz); wrote {}\n",
        path.display()
    );
    for (d, h) in log.devices.iter().enumerate() {
        let name = names.get(d).map_or(String::new(), |n| format!(" {n}"));
        let count = log.dev.iter().filter(|&&v| v as usize == d).count();
        s += &format!("  device {d}: handle {h:#x}, {count} events{name}\n");
    }
    if n >= 200 && peak <= 300.0 {
        s += "  warning: events came at most about 125 a second. Windows probably throttled the logger, so the times \
              are only good to about 8 ms. See python/README.md, \"Raw mouse log\".\n";
    }
    s
}

/// Times the per-event path without real input (mouse_log.py's --bench): (1) PeekMessage, the QPC read and the
/// GetRawInputData call, on WM_INPUT messages posted to the logger's own window (their handle is invalid, so the read
/// fails as fast as the system rejects it); (2) decoding a filled RAWINPUT and appending its record. No input is sent
/// to the system.
#[cfg(windows)]
pub fn bench(n_msgs: usize, n_records: usize) -> io::Result<String> {
    let mut lg = Logger::new()?;
    lg.raw.0[..4].copy_from_slice(&0u32.to_le_bytes());
    lg.raw.0[8..16].copy_from_slice(&0x1234u64.to_le_bytes());
    lg.raw.0[36..40].copy_from_slice(&3i32.to_le_bytes());
    lg.raw.0[40..44].copy_from_slice(&(-2i32).to_le_bytes());
    lg.add_device(0x1234);
    let mut per_msg = f64::INFINITY;
    for _ in 0..5 {
        for _ in 0..n_msgs {
            // SAFETY: a message to the logger's own window
            unsafe { win::PostMessageW(lg.hwnd, win::WM_INPUT, 1, 0) };
        }
        let t0 = Instant::now();
        lg.drain();
        per_msg = per_msg.min(t0.elapsed().as_secs_f64() / n_msgs as f64);
    }
    let bad = lg.bad;
    lg.out.clear();
    let t0 = Instant::now();
    for i in 0..n_records {
        lg.record(i as i64);
    }
    let per_rec = t0.elapsed().as_secs_f64() / n_records as f64;
    std::hint::black_box(&lg.out);
    lg.close();
    let total = per_msg + per_rec;
    Ok(format!(
        "message + QPC + GetRawInputData: {:.2} us ({bad} posted messages read, all rejected as expected); decode + \
         record: {:.3} us; total {:.2} us per event, about {} events a second (an 8000 Hz mouse needs 8,000)",
        per_msg * 1e6,
        per_rec * 1e6,
        total * 1e6,
        thousands(1.0 / total)
    ))
}

// ---- the app's switch ----

/// The logger the app runs: its process, its file and when it started (seconds since 1970).
struct Running {
    child: Child,
    file: PathBuf,
    since: f64,
}

/// The switch's state: the logger running, the last log it wrote, and why the last start or stop failed.
#[derive(Default)]
struct Switch {
    running: Option<Running>,
    last: Option<Value>,
    error: Option<String>,
}

static FOLDER: OnceLock<PathBuf> = OnceLock::new();
static SWITCH: Mutex<Option<Switch>> = Mutex::new(None);

/// The folder the app keeps its mouse logs in (its data folder's mouse/).
pub fn set_folder(folder: PathBuf) {
    let _ = FOLDER.set(folder);
}

fn folder() -> Answer<&'static PathBuf> {
    FOLDER.get().ok_or_else(|| Failure::from("the mouse log folder is not set".to_string()))
}

/// The argument that makes the app's executable the logger.
const CHILD_ARG: &str = "--mouse-log";

/// In the logger's process (the app's executable with `--mouse-log <file>`): logs until standard input closes or
/// says anything, then exits. None in the app itself.
pub fn child_main() -> Option<i32> {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(String::as_str) != Some(CHILD_ARG) {
        return None;
    }
    let Some(out) = a.get(2) else {
        eprintln!("{CHILD_ARG} needs the file to write");
        return Some(2);
    };
    static STOP: AtomicBool = AtomicBool::new(false);
    std::thread::spawn(|| {
        let mut byte = [0u8; 1];
        let _ = io::stdin().read(&mut byte);
        STOP.store(true, Ordering::Relaxed);
    });
    #[cfg(windows)]
    {
        match log_to(Path::new(out), None, &STOP, &|_| {}) {
            Ok(logged) => {
                if logged.bad > 0 {
                    eprintln!("{} WM_INPUT messages could not be read", logged.bad);
                }
                Some(0)
            }
            Err(e) => {
                eprintln!("{e}");
                Some(1)
            }
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("the mouse logger needs Windows ({out})");
        Some(1)
    }
}

/// A finished log's facts for the page: its file, events, span and rates.
fn log_facts(path: &Path) -> Value {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| reader::read_log(&b)) {
        Ok(log) => {
            let f = reader::log_facts(&log, utc_offset_at(log.wall0));
            json!({
                "file": name, "events": f.events, "duration": f.duration, "busiest_hz": f.busiest_hz,
                "median_interval": f.median_interval, "throttled": f.throttled || (f.events >= 200 && f.busiest_hz <= 300.0),
            })
        }
        Err(e) => json!({ "file": name, "error": e }),
    }
}

/// Stops the running logger: its standard input closes, it writes the stop pair and exits (killed after 5 s).
fn stop_running(sw: &mut Switch) {
    let Some(mut r) = sw.running.take() else { return };
    drop(r.child.stdin.take());
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(5) {
        if let Ok(Some(_)) = r.child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !matches!(r.child.try_wait(), Ok(Some(_))) {
        let _ = r.child.kill();
        let _ = r.child.wait();
        sw.error = Some("the logger did not stop in 5 s and was ended; its log holds all but its last quarter second".into());
    }
    sw.last = Some(log_facts(&r.file));
}

/// The switch as the page shows it.
fn state_of(sw: &mut Switch) -> Value {
    // a logger that ended on its own (it failed): off, with its message
    if let Some(r) = sw.running.as_mut()
        && let Ok(Some(status)) = r.child.try_wait()
    {
        let mut why = String::new();
        if let Some(mut e) = r.child.stderr.take() {
            let _ = e.read_to_string(&mut why);
        }
        sw.error = Some(format!("the logger stopped ({status}): {}", why.trim()));
        let file = r.file.clone();
        sw.running = None;
        sw.last = Some(log_facts(&file));
    }
    let running = sw.running.as_ref();
    json!({
        "available": cfg!(windows),
        "on": running.is_some(),
        "file": running.and_then(|r| r.file.file_name()).map(|n| n.to_string_lossy()),
        "since": running.map(|r| r.since),
        "folder": FOLDER.get(),
        "throttle": throttle_setting(),
        "last": sw.last,
        "error": sw.error,
    })
}

/// The logger switch: on or off, its file, the last log and the throttle setting.
pub fn logger_state() -> Value {
    let mut guard = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    state_of(guard.get_or_insert_with(Switch::default))
}

/// Turns the logger on (a new log in the app's mouse folder) or off.
pub fn set_logger(on: bool) -> Answer<Value> {
    let mut guard = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let sw = guard.get_or_insert_with(Switch::default);
    state_of(sw);
    if !on {
        stop_running(sw);
        return Ok(state_of(sw));
    }
    if sw.running.is_some() {
        return Ok(state_of(sw));
    }
    sw.error = None;
    let dir = folder()?;
    std::fs::create_dir_all(dir).map_err(|e| Failure::from(format!("{}: {e}", dir.display())))?;
    let now = time_ns() as f64 / 1e9;
    let mut file = dir.join(format!("mouse_{}.bin", local_stamp(now)));
    for n in 2.. {
        if !file.exists() {
            break;
        }
        file = dir.join(format!("mouse_{}_{n}.bin", local_stamp(now)));
    }
    let exe = std::env::current_exe().map_err(|e| Failure::from(e.to_string()))?;
    let mut cmd = Command::new(exe);
    cmd.arg(CHILD_ARG).arg(&file).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console for the logger
        cmd.creation_flags(0x0800_0000);
    }
    let child = cmd.spawn().map_err(|e| Failure::from(format!("the logger could not start: {e}")))?;
    sw.running = Some(Running { child, file, since: now });
    Ok(state_of(sw))
}

// ---- a recording's measures ----

/// The measures of a recording's run from the log in the app's mouse folder that covers it: {file, run, error}, or
/// null when the recording has no stats file or no log covers its run.
pub fn measures(lib: &Library, id: &str) -> Answer<Value> {
    let Some(stats_path) = lib.stats_path(id) else { return Ok(Value::Null) };
    let Ok(dir) = folder() else { return Ok(Value::Null) };
    measures_in(dir, &stats_path)
}

/// The measures of a stats file's run from the newest log in `dir` that covers it (see `measures`).
pub fn measures_in(dir: &Path, stats_path: &Path) -> Answer<Value> {
    let stats_name = stats_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let text = std::fs::read(stats_path).map_err(|e| Failure::from(format!("{}: {e}", stats_path.display())))?;
    let text = String::from_utf8_lossy(&text).into_owned();
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "bin")).collect())
        .unwrap_or_default();
    logs.sort();
    logs.reverse();
    let mut first_error = None;
    for path in logs {
        let Some((wall0, end)) = span(&path) else { continue };
        let offset = utc_offset_at(wall0);
        let stats = match reader::read_stats(&stats_name, &text, offset) {
            Ok(stats) => stats,
            Err(e) => return Ok(json!({ "file": null, "run": null, "error": e })),
        };
        // the reader's own test, on the log's span: some kill within a second of it
        let (Some(first), Some(last)) = (stats.kills.first(), stats.kills.last()) else { return Ok(Value::Null) };
        if last.t < wall0 - 1.0 || first.t > end + 1.0 {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| Failure::from(format!("{}: {e}", path.display())))?;
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let request = reader::ReadRequest { stats_name: Some(stats_name.clone()), stats_text: Some(text.clone()), options: Default::default(), utc_offset: offset };
        match reader::read(&bytes, &request) {
            reader::ReadOutcome::Run(run) => return Ok(json!({ "file": file, "run": run, "error": null })),
            reader::ReadOutcome::Error(e) => {
                first_error.get_or_insert(json!({ "file": file, "run": null, "error": e }));
            }
            reader::ReadOutcome::Summary(_) => {}
        }
    }
    Ok(first_error.unwrap_or(Value::Null))
}

/// The wall times a log covers (seconds since 1970), from its first and last bytes.
fn span(path: &Path) -> Option<(f64, f64)> {
    use std::io::{Seek, SeekFrom};
    let mut f = File::open(path).ok()?;
    let mut head = [0u8; reader::HEADER_SIZE];
    f.read_exact(&mut head).ok()?;
    let len = f.metadata().ok()?.len();
    let body = (len - reader::HEADER_SIZE as u64) / reader::RECORD_SIZE as u64 * reader::RECORD_SIZE as u64;
    let mut last = [0u8; reader::RECORD_SIZE];
    if body >= reader::RECORD_SIZE as u64 {
        f.seek(SeekFrom::Start(reader::HEADER_SIZE as u64 + body - reader::RECORD_SIZE as u64)).ok()?;
        f.read_exact(&mut last).ok()?;
    }
    reader::log_span(&head, if body > 0 { &last } else { &[] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_and_numbers() {
        let now = time_ns() as f64 / 1e9;
        assert_eq!(local_stamp(now).len(), "2026-10-01_03-17-24".len());
        assert_eq!(utc_offset_at(now) % 900, 0);
        assert_eq!(thousands(330_000.4), "330,000");
        assert_eq!(thousands(8000.0), "8,000");
    }

    #[test]
    fn finds_the_log_that_covers_the_run() {
        // the self-test's run (tests/mouse_fixtures.py), beside a log of another time
        let case = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_out/parity/mouse/selftest");
        let stats = case.join("Selftest - Challenge - 2026.09.30-04.54.28 Stats.csv");
        let Ok(log) = std::fs::read(case.join("log.bin")) else {
            eprintln!("no {} (python tests/mouse_fixtures.py makes it)", case.display());
            return;
        };
        let dir = std::env::temp_dir().join(format!("aimview-mouse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mouse_2026-09-30_04-54-20.bin"), &log).unwrap();
        let mut other = log[..reader::HEADER_SIZE].to_vec();
        other[24..32].copy_from_slice(&(i64::from_le_bytes(log[24..32].try_into().unwrap()) - 7_200_000_000_000).to_le_bytes());
        std::fs::write(dir.join("mouse_2026-09-30_04-54-21.bin"), &other).unwrap();
        let found = measures_in(&dir, &stats);
        for f in ["mouse_2026-09-30_04-54-20.bin", "mouse_2026-09-30_04-54-21.bin"] {
            std::fs::remove_file(dir.join(f)).unwrap();
        }
        std::fs::remove_dir(&dir).unwrap();
        let found = found.unwrap_or_else(|f| panic!("{}", f.message));
        assert_eq!(found["file"], "mouse_2026-09-30_04-54-20.bin");
        assert_eq!(found["run"]["matched"], 20);
        assert_eq!(found["run"]["kills"].as_array().unwrap().len(), 20);
    }

    #[cfg(windows)]
    #[test]
    fn the_logger_keeps_up() {
        // the per-event cost, as mouse_log.py --bench measures it: far under the 125 us an 8000 Hz mouse allows
        let report = bench(2000, 50_000).unwrap();
        let total: f64 = report.split("total ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(total < 60.0, "{report}");
    }
}
