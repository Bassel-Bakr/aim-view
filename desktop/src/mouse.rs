//! The raw mouse logger (python/mouse_log.py, ported) and the app's side of it: an on/off switch that logs in the
//! background while the user plays. Each recording's measures from the log that covers its run are the service's
//! (service/src/mouse.rs).
//!
//! The logger registers a message-only window for raw mouse input with RIDEV_INPUTSINK, so it keeps receiving input
//! while KovaaK's has focus. It never sends input and never moves the cursor. Each WM_INPUT becomes one record in
//! mouse_log.py's format (src/mouse.rs writes the records' bytes and reads them back), so logs from either logger read
//! the same.
//!
//! Raw input goes to one window per process, and tao (Tauri's windows) already registers the mouse in the app's
//! process. So the app runs the logger in a process of its own: its own executable with `--mouse-log <file>`
//! (`child_main`), which stops cleanly, writing the stop pair, when its standard input closes (the switch is turned
//! off, or the app ends).
//!
//! In: Windows' raw mouse input, and the page's /api/mouse/logger requests (protocol.rs). Out: the logs in the app's
//! data folder's mouse/, and the switch's state as JSON for the page.
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

use aimview_service::disk::utc_offset_at;
use aimview_service::{Answer, Failure};

/// Nanoseconds in a second.
const NANOS_PER_SECOND: f64 = 1e9;
/// Seconds in a day.
const SECONDS_PER_DAY: i64 = 86_400;
/// Seconds in an hour.
const SECONDS_PER_HOUR: i64 = 3_600;
/// Seconds in a minute.
const SECONDS_PER_MINUTE: i64 = 60;
/// Minutes in an hour.
const MINUTES_PER_HOUR: i64 = 60;
/// Howard Hinnant's civil calendar: the days from 0000-03-01 to 1970-01-01.
const DAYS_TO_UNIX_EPOCH: i64 = 719_468;
/// The days in an era (the Gregorian calendar's 400-year cycle).
const DAYS_PER_ERA: i64 = 146_097;
/// The years in an era.
const YEARS_PER_ERA: i64 = 400;
/// The busiest rate is counted over this long, seconds.
const BUSIEST_WINDOW_S: f64 = 0.1;
/// With this many events or more, a busiest rate at or under THROTTLED_BUSIEST_HZ means Windows throttled the logger
/// (to about 125 events a second).
const MIN_EVENTS_FOR_RATE: usize = 200;
/// The busiest rate, in events a second, at or under which a long enough log was throttled.
const THROTTLED_BUSIEST_HZ: f64 = 300.0;
/// The argument that makes the app's executable the logger.
const CHILD_ARG: &str = "--mouse-log";
/// The logger process's exit code when it logged.
const EXIT_LOGGED: i32 = 0;
/// The logger process's exit code when it failed.
const EXIT_FAILED: i32 = 1;
/// The logger process's exit code when it started without the file to write.
const EXIT_NO_FILE: i32 = 2;
/// How long a logger asked to stop may take to write its stop pair before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a stopping logger is checked.
const STOP_POLL: Duration = Duration::from_millis(20);
/// Digits between the commas of a large number.
const DIGITS_PER_GROUP: usize = 3;

// ---- Windows ----

/// Windows' calls, constants and structs the logger uses (user32, kernel32, advapi32).
#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    /// The raw input message.
    pub const WM_INPUT: u32 = 0x00FF;
    /// PeekMessageW's flag to take a message off the queue.
    pub const PM_REMOVE: u32 = 0x0001;
    /// GetRawInputData's command: the whole RAWINPUT.
    pub const RID_INPUT: u32 = 0x1000_0003;
    /// RAWINPUTHEADER's dwType for a mouse.
    pub const RIM_TYPEMOUSE: u32 = 0;
    /// WM_INPUT's wParam for input while the window is foreground.
    pub const RIM_INPUT: usize = 0;
    /// WM_INPUT's wParam for input while it is not.
    pub const RIM_INPUTSINK: usize = 1;
    /// RegisterRawInputDevices' flag to stop the device's input.
    pub const RIDEV_REMOVE: u32 = 0x0000_0001;
    /// RegisterRawInputDevices' flag to take the device's input in the background too.
    pub const RIDEV_INPUTSINK: u32 = 0x0000_0100;
    /// GetRawInputDeviceInfoW's command: the device's name.
    pub const RIDI_DEVICENAME: u32 = 0x2000_0007;
    /// The HID usage page of a mouse: generic desktop controls.
    pub const USAGE_PAGE_GENERIC: u16 = 1;
    /// The HID usage of a mouse on that page.
    pub const USAGE_MOUSE: u16 = 2;
    /// The parent of a message-only window.
    pub const HWND_MESSAGE: isize = -3;
    /// MsgWaitForMultipleObjectsEx's wake mask: any message.
    pub const QS_ALLINPUT: u32 = 0x04FF;
    /// MsgWaitForMultipleObjectsEx's flag: input already in the queue counts.
    pub const MWMO_INPUTAVAILABLE: u32 = 0x0004;
    /// SetThreadPriority's highest priority short of time-critical.
    pub const THREAD_PRIORITY_HIGHEST: i32 = 2;
    /// What the raw input calls return when they fail: (UINT)-1.
    pub const FAIL: u32 = u32::MAX;
    /// HKEY_CURRENT_USER, sign-extended as the headers define it.
    pub const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    /// The registry's read-only access.
    pub const KEY_READ: u32 = 0x2_0019;
    /// The registry value type of a string.
    pub const REG_SZ: u32 = 1;
    /// The registry value type of a 32-bit number.
    pub const REG_DWORD: u32 = 4;
    /// CreateProcess's flag for a console program with no console window.
    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// RAWINPUTHEADER's size in the 64-bit layout, which GetRawInputData is told.
    pub const RAW_INPUT_HEADER_BYTES: u32 = 24;
    /// A mouse's RAWINPUT in the 64-bit layout (RAWINPUTHEADER, then RAWMOUSE): its size in bytes. The *_AT constants
    /// after it are where its fields start.
    pub const RAW_INPUT_BYTES: usize = 48;
    /// The header's dwType.
    pub const TYPE_AT: usize = 0;
    /// The header's hDevice.
    pub const DEVICE_AT: usize = 8;
    /// RAWMOUSE's usFlags.
    pub const FLAGS_AT: usize = 24;
    /// RAWMOUSE's usButtonFlags.
    pub const BUTTON_FLAGS_AT: usize = 28;
    /// RAWMOUSE's usButtonData.
    pub const BUTTON_DATA_AT: usize = 30;
    /// RAWMOUSE's lLastX.
    pub const X_AT: usize = 36;
    /// RAWMOUSE's lLastY.
    pub const Y_AT: usize = 40;

    /// Windows' MSG: a message from the thread's queue.
    #[repr(C)]
    pub struct Msg {
        /// The window it is for.
        pub hwnd: isize,
        /// The message's number (WM_INPUT, ...).
        pub message: u32,
        /// Its first parameter (for WM_INPUT, RIM_INPUT or RIM_INPUTSINK).
        pub wparam: usize,
        /// Its second parameter (for WM_INPUT, the RAWINPUT's handle).
        pub lparam: isize,
        /// When it was posted, in ms since the system started.
        pub time: u32,
        /// The cursor's x then, in screen pixels.
        pub pt_x: i32,
        /// The cursor's y then, in screen pixels.
        pub pt_y: i32,
        /// Windows' own.
        pub private: u32,
    }

    /// Windows' RAWINPUTDEVICE: a kind of device to take raw input from.
    #[repr(C)]
    pub struct RawInputDevice {
        /// The HID usage page.
        pub usage_page: u16,
        /// The HID usage.
        pub usage: u16,
        /// RIDEV_* flags.
        pub flags: u32,
        /// The window that gets the input.
        pub target: isize,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        /// Makes a window; 0 when it fails.
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
        /// Destroys a window.
        pub fn DestroyWindow(hwnd: isize) -> i32;
        /// Asks for (or with RIDEV_REMOVE stops) raw input from kinds of devices; 0 when it fails.
        pub fn RegisterRawInputDevices(devices: *const RawInputDevice, n: u32, size: u32) -> i32;
        /// A WM_INPUT's RAWINPUT into `data`; FAIL when it fails.
        pub fn GetRawInputData(input: isize, command: u32, data: *mut c_void, size: *mut u32, header: u32) -> u32;
        /// A raw input device's facts (its name); with no buffer, the size it needs.
        pub fn GetRawInputDeviceInfoW(device: isize, command: u32, data: *mut c_void, size: *mut u32) -> u32;
        /// Takes the next message from the queue without waiting; 0 when there is none.
        pub fn PeekMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32, remove: u32) -> i32;
        /// Posts a message to a window's queue.
        pub fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
        /// Turns key messages into character messages.
        pub fn TranslateMessage(msg: *const Msg) -> i32;
        /// Hands a message to its window's procedure.
        pub fn DispatchMessageW(msg: *const Msg) -> isize;
        /// The default handling of a message.
        pub fn DefWindowProcW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        /// Waits for a message, a handle or the time.
        pub fn MsgWaitForMultipleObjectsEx(n: u32, handles: *const isize, ms: u32, mask: u32, flags: u32) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// The performance counter now, in its ticks.
        pub fn QueryPerformanceCounter(count: *mut i64) -> i32;
        /// The performance counter's ticks a second.
        pub fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
        /// A handle for the calling thread.
        pub fn GetCurrentThread() -> isize;
        /// Sets a thread's priority.
        pub fn SetThreadPriority(thread: isize, priority: i32) -> i32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        /// Opens a registry key; 0 when it opened.
        pub fn RegOpenKeyExW(key: isize, sub: *const u16, options: u32, sam: u32, out: *mut isize) -> i32;
        /// A key's `index`-th value: its name, type and data; not 0 when there are no more.
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
        /// Closes a registry key.
        pub fn RegCloseKey(key: isize) -> i32;
    }

    /// A string as Windows takes it: UTF-16, ending in a 0.
    pub fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
}

/// The registry's subkey of HKEY_CURRENT_USER that holds the throttle.
#[cfg(windows)]
const THROTTLE_KEY: &str = r"Control Panel\Mouse";
/// The start of the throttle values' names (RawMouseThrottleEnabled, RawMouseThrottleForced,
/// RawMouseThrottleDuration, ...).
#[cfg(windows)]
const THROTTLE_VALUE_PREFIX: &str = "RawMouseThrottl";
/// The buffer a registry value's name is read into, in characters.
#[cfg(windows)]
const REGISTRY_NAME_CHARS: usize = 256;
/// The buffer a registry value's data is read into, in bytes.
#[cfg(windows)]
const REGISTRY_DATA_BYTES: usize = 1024;

/// The QueryPerformanceCounter time, in its ticks.
#[cfg(windows)]
pub fn qpc_now() -> i64 {
    let mut ticks = 0i64;
    // SAFETY: the call writes one i64
    unsafe { win::QueryPerformanceCounter(&mut ticks) };
    ticks
}

/// Nanoseconds since 1970 (Python's `time.time_ns()`).
pub fn time_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since_epoch| since_epoch.as_nanos() as i64)
}

/// A moment (seconds since 1970) as a local "2026-10-01_03-17-24", the logs' file names.
pub fn local_stamp(epoch_s: f64) -> String {
    let local_s = epoch_s.floor() as i64 + utc_offset_at(epoch_s);
    let (days, second_of_day) = (local_s.div_euclid(SECONDS_PER_DAY), local_s.rem_euclid(SECONDS_PER_DAY));
    let (year, month, day) = civil_date(days);
    let hours = second_of_day / SECONDS_PER_HOUR;
    let minutes = second_of_day / SECONDS_PER_MINUTE % MINUTES_PER_HOUR;
    let seconds = second_of_day % SECONDS_PER_MINUTE;
    format!("{year:04}-{month:02}-{day:02}_{hours:02}-{minutes:02}-{seconds:02}")
}

/// The civil date (year, month, day) of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`, counting
/// years from March so that a leap day ends the year).
fn civil_date(days: i64) -> (i64, i64, i64) {
    let days_since_march_0000 = days + DAYS_TO_UNIX_EPOCH;
    let era = days_since_march_0000.div_euclid(DAYS_PER_ERA);
    let day_of_era = days_since_march_0000 - era * DAYS_PER_ERA;
    // the era's leap days taken out (one every 4 years, but not every 100, but every 400), its 365-day years
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // the months counted from March: their lengths follow (153 * month + 2) / 5
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    let year = year_of_era + era * YEARS_PER_ERA + i64::from(month <= 2);
    (year, month, day)
}

/// The background raw input throttle values under HKCU\Control Panel\Mouse, read only.
pub fn throttle_setting() -> String {
    let mut found = throttle_values();
    if found.is_empty() {
        return "background throttle not set (Windows 11 default: about 125 Hz while KovaaK's has focus)".into();
    }
    found.sort();
    let list: Vec<String> = found.iter().map(|(name, value)| format!("{name} = {value}")).collect();
    format!("background throttle: {}", list.join(", "))
}

/// The throttle's values in the registry, as (name, value as text).
#[cfg(windows)]
fn throttle_values() -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut key = 0isize;
    let subkey = win::wide(THROTTLE_KEY);
    // SAFETY: the key is opened read only, enumerated into buffers of the sizes given, and closed
    unsafe {
        if win::RegOpenKeyExW(win::HKEY_CURRENT_USER, subkey.as_ptr(), 0, win::KEY_READ, &mut key) == 0 {
            for i in 0.. {
                let (mut name, mut data) = ([0u16; REGISTRY_NAME_CHARS], [0u8; REGISTRY_DATA_BYTES]);
                let (mut name_len, mut data_len, mut kind) = (name.len() as u32, data.len() as u32, 0u32);
                let result = win::RegEnumValueW(
                    key,
                    i,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    &mut kind,
                    data.as_mut_ptr(),
                    &mut data_len,
                );
                if result != 0 {
                    break;
                }
                let name = String::from_utf16_lossy(&name[..name_len as usize]);
                if name.starts_with(THROTTLE_VALUE_PREFIX) {
                    found.push((name, registry_value_text(kind, &data[..data_len as usize])));
                }
            }
            win::RegCloseKey(key);
        }
    }
    found
}

/// Without Windows there is no registry, and no throttle.
#[cfg(not(windows))]
fn throttle_values() -> Vec<(String, String)> {
    Vec::new()
}

/// A registry value's data as text: a DWORD as its number, a string as itself, anything else as its bytes.
#[cfg(windows)]
fn registry_value_text(kind: u32, data: &[u8]) -> String {
    match kind {
        win::REG_DWORD if data.len() >= size_of::<u32>() => {
            u32::from_le_bytes(data[..size_of::<u32>()].try_into().unwrap()).to_string()
        }
        win::REG_SZ => {
            let units: Vec<u16> =
                data.chunks_exact(size_of::<u16>()).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
        }
        _ => format!("{data:?}"),
    }
}

// ---- the logger ----

/// The (QPC, time_ns) pair is the tightest of this many tries.
#[cfg(windows)]
const CLOCK_PAIR_TRIES: usize = 20;
/// The logger waits at most this long for input before it checks whether to stop, milliseconds.
#[cfg(windows)]
const WAIT_MS: u32 = 100;
/// The records go to the file once this many bytes wait, or FLUSH_INTERVAL after the last write, so a killed logger
/// loses little.
#[cfg(windows)]
const FLUSH_BYTES: usize = 1 << 16;
/// The longest the records wait before they go to the file.
#[cfg(windows)]
const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
/// The bench's rounds of posted messages (the fastest counts).
#[cfg(windows)]
const BENCH_ROUNDS: usize = 5;
/// The bench's RAWINPUT: from this made-up device.
#[cfg(windows)]
const BENCH_DEVICE_HANDLE: u64 = 0x1234;
/// The bench's RAWINPUT: a move of 3 counts right.
#[cfg(windows)]
const BENCH_X_COUNTS: i32 = 3;
/// The bench's RAWINPUT: a move of 2 counts up.
#[cfg(windows)]
const BENCH_Y_COUNTS: i32 = -2;
/// Microseconds in a second.
#[cfg(windows)]
const MICROS_PER_SECOND: f64 = 1e6;

/// A RAWINPUT's bytes (64-bit layout: the header, then RAWMOUSE).
#[cfg(windows)]
#[repr(C, align(8))]
struct RawBuffer([u8; win::RAW_INPUT_BYTES]);

/// A message-only window that turns WM_INPUT messages into records in `records`.
#[cfg(windows)]
pub struct Logger {
    /// The message-only window the raw input goes to.
    window: isize,
    /// QueryPerformanceCounter's ticks a second.
    pub qpc_frequency: i64,
    /// The records not yet written to the file.
    pub records: Vec<u8>,
    /// Each device's index in the log, by its handle.
    device_indexes: HashMap<u64, u16>,
    /// The devices' names, by index.
    pub device_names: Vec<String>,
    /// WM_INPUT messages that could not be read.
    pub unread_messages: usize,
    /// The buffer each WM_INPUT's RAWINPUT is read into.
    raw_input: RawBuffer,
}

#[cfg(windows)]
impl Logger {
    /// A logger with its message-only window made; it takes no input until `register`.
    pub fn new() -> io::Result<Logger> {
        let (class, name) = (win::wide("STATIC"), win::wide("aimview mouse_log"));
        // SAFETY: a message-only window of a system class, with no parameters
        let window = unsafe {
            win::CreateWindowExW(
                0,
                class.as_ptr(),
                name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                win::HWND_MESSAGE,
                0,
                0,
                std::ptr::null_mut(),
            )
        };
        if window == 0 {
            return Err(io::Error::other(format!("CreateWindowExW failed: {}", io::Error::last_os_error())));
        }
        let mut qpc_frequency = 0i64;
        // SAFETY: the call writes one i64
        unsafe { win::QueryPerformanceFrequency(&mut qpc_frequency) };
        Ok(Logger {
            window,
            qpc_frequency,
            records: Vec::new(),
            device_indexes: HashMap::new(),
            device_names: Vec::new(),
            unread_messages: 0,
            raw_input: RawBuffer([0; win::RAW_INPUT_BYTES]),
        })
    }

    /// Asks for the mouse's raw input, in the background too.
    pub fn register(&self) -> io::Result<()> {
        let device = win::RawInputDevice {
            usage_page: win::USAGE_PAGE_GENERIC,
            usage: win::USAGE_MOUSE,
            flags: win::RIDEV_INPUTSINK,
            target: self.window,
        };
        // SAFETY: one device, of the size given
        if unsafe { win::RegisterRawInputDevices(&device, 1, size_of::<win::RawInputDevice>() as u32) } == 0 {
            return Err(io::Error::other(format!("RegisterRawInputDevices failed: {}", io::Error::last_os_error())));
        }
        Ok(())
    }

    /// Handles every queued message; WM_INPUT is read here, not in a window procedure.
    pub fn drain(&mut self) {
        let mut queued = win::Msg { hwnd: 0, message: 0, wparam: 0, lparam: 0, time: 0, pt_x: 0, pt_y: 0, private: 0 };
        // SAFETY: the message and the raw input buffer are the sizes the calls are told
        unsafe {
            while win::PeekMessageW(&mut queued, 0, 0, 0, win::PM_REMOVE) != 0 {
                if queued.message == win::WM_INPUT {
                    let handled_qpc = qpc_now();
                    let mut size = self.raw_input.0.len() as u32;
                    let read = win::GetRawInputData(
                        queued.lparam,
                        win::RID_INPUT,
                        self.raw_input.0.as_mut_ptr().cast(),
                        &mut size,
                        win::RAW_INPUT_HEADER_BYTES,
                    );
                    if read == win::FAIL {
                        self.unread_messages += 1;
                        continue;
                    }
                    if queued.wparam == win::RIM_INPUT {
                        // input while this window is foreground: let the system clean up
                        win::DefWindowProcW(queued.hwnd, win::WM_INPUT, queued.wparam, queued.lparam);
                    }
                    self.record(handled_qpc);
                } else {
                    win::TranslateMessage(&queued);
                    win::DispatchMessageW(&queued);
                }
            }
        }
    }

    /// Decodes the RAWINPUT in the buffer and appends one record, timed at `handled_qpc`.
    fn record(&mut self, handled_qpc: i64) {
        let raw = &self.raw_input.0;
        let read_u16 = |at: usize| u16::from_le_bytes([raw[at], raw[at + 1]]);
        let read_i32 = |at: usize| i32::from_le_bytes(raw[at..at + size_of::<i32>()].try_into().unwrap());
        if read_i32(win::TYPE_AT) as u32 != win::RIM_TYPEMOUSE {
            return;
        }
        let handle = u64::from_le_bytes(raw[win::DEVICE_AT..win::DEVICE_AT + size_of::<u64>()].try_into().unwrap());
        let (flags, button_flags) = (read_u16(win::FLAGS_AT), read_u16(win::BUTTON_FLAGS_AT));
        let button_data = read_u16(win::BUTTON_DATA_AT);
        let (x_counts, y_counts) = (read_i32(win::X_AT), read_i32(win::Y_AT));
        let device = match self.device_indexes.get(&handle) {
            Some(&device) => device,
            None => self.add_device(handle),
        };
        let event = reader::event(handled_qpc, x_counts, y_counts, flags, button_flags, button_data, device);
        self.records.extend_from_slice(&event);
    }

    /// Gives a device seen for the first time its index, and writes its record; returns the index.
    fn add_device(&mut self, handle: u64) -> u16 {
        let index = self.device_indexes.len() as u16;
        self.device_indexes.insert(handle, index);
        self.records.extend_from_slice(&reader::device(handle, u32::from(index)));
        self.device_names.push(device_name(handle));
        index
    }

    /// A (QPC, time_ns) pair: the tightest of CLOCK_PAIR_TRIES, with time_ns read between two QPC reads.
    pub fn clock_pair(&self) -> (i64, i64) {
        let (mut best_spread, mut best_qpc, mut best_ns) = (i64::MAX, 0, 0);
        for _ in 0..CLOCK_PAIR_TRIES {
            let before = qpc_now();
            let wall_ns = time_ns();
            let after = qpc_now();
            if after - before < best_spread {
                (best_spread, best_qpc, best_ns) = (after - before, (before + after).div_euclid(2), wall_ns);
            }
        }
        (best_qpc, best_ns)
    }

    /// Waits up to `timeout_ms` for input.
    pub fn wait(&self, timeout_ms: u32) {
        let (mask, flags) = (win::QS_ALLINPUT, win::MWMO_INPUTAVAILABLE);
        // SAFETY: no handles; it only waits for input or the time
        unsafe { win::MsgWaitForMultipleObjectsEx(0, std::ptr::null(), timeout_ms, mask, flags) };
    }

    /// Stops the mouse's raw input and destroys the window.
    pub fn close(&mut self) {
        let device = win::RawInputDevice {
            usage_page: win::USAGE_PAGE_GENERIC,
            usage: win::USAGE_MOUSE,
            flags: win::RIDEV_REMOVE,
            target: 0,
        };
        // SAFETY: the registration is removed and the window destroyed, both by this thread, which made them
        unsafe {
            win::RegisterRawInputDevices(&device, 1, size_of::<win::RawInputDevice>() as u32);
            win::DestroyWindow(self.window);
        }
    }
}

/// A raw input device's name (its path), or what it is when it has no handle.
#[cfg(windows)]
fn device_name(handle: u64) -> String {
    let mut name = "(no device handle: injected or synthetic input)".to_string();
    let mut name_chars = 0u32;
    let device = handle as isize;
    // SAFETY: the first call only asks the length (in characters); the second fills a buffer that long
    unsafe {
        if handle != 0
            && win::GetRawInputDeviceInfoW(device, win::RIDI_DEVICENAME, std::ptr::null_mut(), &mut name_chars) == 0
            && name_chars > 0
        {
            let mut buffer = vec![0u16; name_chars as usize];
            let read =
                win::GetRawInputDeviceInfoW(device, win::RIDI_DEVICENAME, buffer.as_mut_ptr().cast(), &mut name_chars);
            if read != win::FAIL {
                let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
                name = String::from_utf16_lossy(&buffer[..end]);
            }
        }
    }
    name
}

/// What a finished log holds, as the logger reports it.
pub struct Logged {
    /// The devices' names, by their index in the log.
    pub names: Vec<String>,
    /// WM_INPUT messages that could not be read.
    pub bad: usize,
}

/// Logs the mouse into `out` (a new file: an existing one is never overwritten) until `stop` is set or, with seconds,
/// that long. `say` gets the line the logger prints at its start.
#[cfg(windows)]
pub fn log_to(out: &Path, seconds: Option<f64>, stop: &AtomicBool, say: &dyn Fn(&str)) -> io::Result<Logged> {
    let mut logger = Logger::new()?;
    logger.register()?;
    // SAFETY: raises this thread's priority only
    unsafe { win::SetThreadPriority(win::GetCurrentThread(), win::THREAD_PRIORITY_HIGHEST) };
    let mut file = File::create_new(out)?;
    let (start_qpc, start_ns) = logger.clock_pair();
    file.write_all(&reader::header(logger.qpc_frequency, start_qpc, start_ns))?;
    let end_qpc = seconds.map(|seconds| start_qpc + (seconds * logger.qpc_frequency as f64) as i64);
    let how = seconds.map_or("Ctrl+C stops".to_string(), |seconds| format!("for {} s", reader::fmt_g(seconds)));
    say(&format!(
        "mouse_log: logging to {} ({how}; QPC {} Hz; {})",
        out.display(),
        logger.qpc_frequency,
        throttle_setting()
    ));
    let logged = log_until(&mut logger, &mut file, stop, end_qpc);
    logger.drain();
    let (stop_qpc, stop_ns) = logger.clock_pair();
    logger.records.extend_from_slice(&reader::stop(stop_qpc, stop_ns));
    let wrote = file.write_all(&logger.records).and_then(|()| file.flush());
    logger.close();
    logged.and(wrote)?;
    Ok(Logged { names: std::mem::take(&mut logger.device_names), bad: logger.unread_messages })
}

/// Takes the mouse's input until `stop` is set or the QPC reaches `end_qpc`, writing the records to `file` as
/// FLUSH_BYTES and FLUSH_INTERVAL say.
#[cfg(windows)]
fn log_until(logger: &mut Logger, file: &mut File, stop: &AtomicBool, end_qpc: Option<i64>) -> io::Result<()> {
    let mut last_flush = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        logger.wait(WAIT_MS);
        logger.drain();
        if logger.records.len() >= FLUSH_BYTES || last_flush.elapsed() > FLUSH_INTERVAL {
            file.write_all(&logger.records)?;
            logger.records.clear();
            last_flush = Instant::now();
        }
        if end_qpc.is_some_and(|end_qpc| qpc_now() >= end_qpc) {
            break;
        }
    }
    Ok(())
}

/// A number rounded to a whole, with commas between thousands.
fn thousands(value: f64) -> String {
    let rounded = format!("{value:.0}");
    let (sign, digits) = rounded.strip_prefix('-').map_or(("", rounded.as_str()), |digits| ("-", digits));
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % DIGITS_PER_GROUP == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    format!("{sign}{out}")
}

/// What mouse_log.py prints when it stops: the events, the duration, the rates and the devices, with a warning when
/// the log looks throttled.
pub fn logged_text(log: &MouseLog, path: &Path, names: &[String]) -> String {
    let (events, duration_s) = (log.times_s.len(), log.duration);
    let busiest_hz = reader::busiest_rate(&log.times_s, BUSIEST_WINDOW_S);
    let mean_hz = if duration_s != 0.0 { events as f64 / duration_s } else { 0.0 };
    let mut text = format!(
        "mouse_log: {events} events in {duration_s:.2} s ({mean_hz:.1} Hz mean, busiest 100 ms {busiest_hz:.0} Hz); \
         wrote {}\n",
        path.display()
    );
    for (index, handle) in log.devices.iter().enumerate() {
        let name = names.get(index).map_or(String::new(), |name| format!(" {name}"));
        let count = log.device_events(index);
        text += &format!("  device {index}: handle {handle:#x}, {count} events{name}\n");
    }
    if events >= MIN_EVENTS_FOR_RATE && busiest_hz <= THROTTLED_BUSIEST_HZ {
        text += "  warning: events came at most about 125 a second. Windows probably throttled the logger, so the \
                 times are only good to about 8 ms. See python/README.md, \"Raw mouse log\".\n";
    }
    text
}

/// Times the per-event path without real input (mouse_log.py's --bench): (1) PeekMessage, the QPC read and the
/// GetRawInputData call, on WM_INPUT messages posted to the logger's own window (their handle is invalid, so the read
/// fails as fast as the system rejects it); (2) decoding a filled RAWINPUT and appending its record. No input is sent
/// to the system.
#[cfg(windows)]
pub fn bench(message_count: usize, record_count: usize) -> io::Result<String> {
    let mut logger = Logger::new()?;
    fill_bench_input(&mut logger.raw_input.0);
    logger.add_device(BENCH_DEVICE_HANDLE);
    let mut per_message_s = f64::INFINITY;
    for _ in 0..BENCH_ROUNDS {
        for _ in 0..message_count {
            // SAFETY: a message to the logger's own window
            unsafe { win::PostMessageW(logger.window, win::WM_INPUT, win::RIM_INPUTSINK, 0) };
        }
        let started = Instant::now();
        logger.drain();
        per_message_s = per_message_s.min(started.elapsed().as_secs_f64() / message_count as f64);
    }
    let rejected = logger.unread_messages;
    logger.records.clear();
    let started = Instant::now();
    for i in 0..record_count {
        logger.record(i as i64);
    }
    let per_record_s = started.elapsed().as_secs_f64() / record_count as f64;
    std::hint::black_box(&logger.records);
    logger.close();
    let total_s = per_message_s + per_record_s;
    Ok(format!(
        "message + QPC + GetRawInputData: {:.2} us ({rejected} posted messages read, all rejected as expected); decode \
         + record: {:.3} us; total {:.2} us per event, about {} events a second (an 8000 Hz mouse needs 8,000)",
        per_message_s * MICROS_PER_SECOND,
        per_record_s * MICROS_PER_SECOND,
        total_s * MICROS_PER_SECOND,
        thousands(1.0 / total_s)
    ))
}

/// The bench's RAWINPUT: a mouse's, from BENCH_DEVICE_HANDLE, moved BENCH_X_COUNTS and BENCH_Y_COUNTS.
#[cfg(windows)]
fn fill_bench_input(raw: &mut [u8; win::RAW_INPUT_BYTES]) {
    raw[win::TYPE_AT..win::TYPE_AT + size_of::<u32>()].copy_from_slice(&win::RIM_TYPEMOUSE.to_le_bytes());
    raw[win::DEVICE_AT..win::DEVICE_AT + size_of::<u64>()].copy_from_slice(&BENCH_DEVICE_HANDLE.to_le_bytes());
    raw[win::X_AT..win::X_AT + size_of::<i32>()].copy_from_slice(&BENCH_X_COUNTS.to_le_bytes());
    raw[win::Y_AT..win::Y_AT + size_of::<i32>()].copy_from_slice(&BENCH_Y_COUNTS.to_le_bytes());
}

// ---- the app's switch ----

/// The logger the app runs: its process, its file and when it started (seconds since 1970).
struct Running {
    /// The logger's process; closing its standard input stops it.
    child: Child,
    /// The log it writes.
    file: PathBuf,
    /// When it started, in seconds since 1970.
    started_s: f64,
}

/// The switch's state: the logger running, the last log it wrote, and why the last start or stop failed.
#[derive(Default)]
struct Switch {
    /// The logger running; None: the switch is off.
    running: Option<Running>,
    /// The last finished log's facts (`log_facts_json`).
    last_log: Option<Value>,
    /// Why the last start or stop failed.
    error: Option<String>,
}

/// The folder the app keeps its mouse logs in, set once at the app's start.
static FOLDER: OnceLock<PathBuf> = OnceLock::new();
/// The switch, made on first use.
static SWITCH: Mutex<Option<Switch>> = Mutex::new(None);

/// The folder the app keeps its mouse logs in (its data folder's mouse/).
pub fn set_folder(folder: PathBuf) {
    let _ = FOLDER.set(folder);
}

/// The mouse logs' folder; a failure when the app has not set it.
fn log_folder() -> Answer<&'static PathBuf> {
    FOLDER.get().ok_or_else(|| Failure::from("the mouse log folder is not set".to_string()))
}

/// In the logger's process (the app's executable with `--mouse-log <file>`): logs until standard input closes or
/// says anything, then exits. None in the app itself.
pub fn child_main() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(CHILD_ARG) {
        return None;
    }
    let Some(out) = args.get(2) else {
        eprintln!("{CHILD_ARG} needs the file to write");
        return Some(EXIT_NO_FILE);
    };
    /// Set once standard input closes or says anything.
    static STOP: AtomicBool = AtomicBool::new(false);
    std::thread::spawn(|| {
        let mut byte = [0u8; 1];
        let _ = io::stdin().read(&mut byte);
        STOP.store(true, Ordering::Relaxed);
    });
    Some(log_in_child(out, &STOP))
}

/// The logger process's work: logs into `out` until `stop` is set; gives the exit code.
#[cfg(windows)]
fn log_in_child(out: &str, stop: &AtomicBool) -> i32 {
    match log_to(Path::new(out), None, stop, &|_| {}) {
        Ok(logged) => {
            if logged.bad > 0 {
                eprintln!("{} WM_INPUT messages could not be read", logged.bad);
            }
            EXIT_LOGGED
        }
        Err(error) => {
            eprintln!("{error}");
            EXIT_FAILED
        }
    }
}

/// Without Windows there is no logger: the process fails.
#[cfg(not(windows))]
fn log_in_child(out: &str, _stop: &AtomicBool) -> i32 {
    eprintln!("the mouse logger needs Windows ({out})");
    EXIT_FAILED
}

/// A finished log's facts for the page: its file, events, span and rates.
fn log_facts_json(path: &Path) -> Value {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
    match std::fs::read(path).map_err(|error| error.to_string()).and_then(|bytes| reader::read_log(&bytes)) {
        Ok(log) => {
            let facts = reader::log_facts(&log, utc_offset_at(log.wall0));
            let throttled =
                facts.throttled || (facts.events >= MIN_EVENTS_FOR_RATE && facts.busiest_hz <= THROTTLED_BUSIEST_HZ);
            json!({
                "file": name, "events": facts.events, "duration": facts.duration, "busiest_hz": facts.busiest_hz,
                "median_interval": facts.median_interval, "throttled": throttled,
            })
        }
        Err(error) => json!({ "file": name, "error": error }),
    }
}

/// Stops the running logger: its standard input closes, it writes the stop pair and exits (killed after
/// STOP_TIMEOUT).
fn stop_running(switch: &mut Switch) {
    let Some(mut running) = switch.running.take() else { return };
    drop(running.child.stdin.take());
    let asked = Instant::now();
    while asked.elapsed() < STOP_TIMEOUT {
        if let Ok(Some(_)) = running.child.try_wait() {
            break;
        }
        std::thread::sleep(STOP_POLL);
    }
    if !matches!(running.child.try_wait(), Ok(Some(_))) {
        let _ = running.child.kill();
        let _ = running.child.wait();
        switch.error =
            Some("the logger did not stop in 5 s and was ended; its log holds all but its last quarter second".into());
    }
    switch.last_log = Some(log_facts_json(&running.file));
}

/// A logger that ended on its own (it failed): the switch turns off, with its message.
fn note_ended_logger(switch: &mut Switch) {
    if let Some(running) = switch.running.as_mut()
        && let Ok(Some(status)) = running.child.try_wait()
    {
        let mut why = String::new();
        if let Some(mut stderr) = running.child.stderr.take() {
            let _ = stderr.read_to_string(&mut why);
        }
        switch.error = Some(format!("the logger stopped ({status}): {}", why.trim()));
        let file = running.file.clone();
        switch.running = None;
        switch.last_log = Some(log_facts_json(&file));
    }
}

/// The switch as the page shows it.
fn switch_state(switch: &mut Switch) -> Value {
    note_ended_logger(switch);
    let running = switch.running.as_ref();
    json!({
        "available": cfg!(windows),
        "on": running.is_some(),
        "file": running.and_then(|running| running.file.file_name()).map(|name| name.to_string_lossy()),
        "since": running.map(|running| running.started_s),
        "folder": FOLDER.get(),
        "throttle": throttle_setting(),
        "last": switch.last_log,
        "error": switch.error,
    })
}

/// The logger switch: on or off, its file, the last log and the throttle setting.
pub fn logger_state() -> Value {
    let mut guard = SWITCH.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    switch_state(guard.get_or_insert_with(Switch::default))
}

/// Turns the logger on (a new log in the app's mouse folder) or off.
pub fn set_logger(on: bool) -> Answer<Value> {
    let mut guard = SWITCH.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let switch = guard.get_or_insert_with(Switch::default);
    switch_state(switch);
    if !on {
        stop_running(switch);
        return Ok(switch_state(switch));
    }
    if switch.running.is_some() {
        return Ok(switch_state(switch));
    }
    switch.error = None;
    let folder = log_folder()?;
    std::fs::create_dir_all(folder).map_err(|error| Failure::from(format!("{}: {error}", folder.display())))?;
    let now_s = time_ns() as f64 / NANOS_PER_SECOND;
    let file = new_log_file(folder, now_s);
    let child = start_logger(&file)?;
    switch.running = Some(Running { child, file, started_s: now_s });
    Ok(switch_state(switch))
}

/// A new log's file in `folder`, named for the time (`now_s`, seconds since 1970): mouse_<stamp>.bin, or
/// mouse_<stamp>_<number>.bin when that is taken.
fn new_log_file(folder: &Path, now_s: f64) -> PathBuf {
    let mut file = folder.join(format!("mouse_{}.bin", local_stamp(now_s)));
    for number in 2.. {
        if !file.exists() {
            break;
        }
        file = folder.join(format!("mouse_{}_{number}.bin", local_stamp(now_s)));
    }
    file
}

/// Starts the logger's process (this executable with CHILD_ARG), logging into `file`.
fn start_logger(file: &Path) -> Answer<Child> {
    let exe = std::env::current_exe().map_err(|error| Failure::from(error.to_string()))?;
    let mut command = Command::new(exe);
    command.arg(CHILD_ARG).arg(file).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console for the logger
        command.creation_flags(win::CREATE_NO_WINDOW);
    }
    command.spawn().map_err(|error| Failure::from(format!("the logger could not start: {error}")))
}

/// The logger's stamps, dates, records and speed.
#[cfg(test)]
mod tests {
    use super::*;

    /// A stamp has its length, this computer's offset is whole quarter hours, and large numbers get commas.
    #[test]
    fn stamps_and_numbers() {
        let now_s = time_ns() as f64 / NANOS_PER_SECOND;
        assert_eq!(local_stamp(now_s).len(), "2026-10-01_03-17-24".len());
        assert_eq!(utc_offset_at(now_s) % 900, 0);
        assert_eq!(thousands(330_000.4), "330,000");
        assert_eq!(thousands(8000.0), "8,000");
    }

    /// Days from 1970 give their dates, across leap days and centuries and before 1970.
    #[test]
    fn civil_dates() {
        for (days, date) in [
            (0, (1970, 1, 1)),
            (-1, (1969, 12, 31)),
            (11_016, (2000, 2, 29)),
            (11_017, (2000, 3, 1)),
            (20_730, (2026, 10, 4)),
            (47_540, (2100, 2, 28)),
            (47_541, (2100, 3, 1)),
            (-135_081, (1600, 2, 29)),
        ] {
            assert_eq!(civil_date(days), date, "{days}");
        }
    }

    /// A mouse's RAWINPUT gives a device record then an event with its fields; a keyboard's gives none.
    #[cfg(windows)]
    #[test]
    fn a_record_holds_the_raw_inputs_fields() {
        // a RAWINPUT as Windows lays it out (64-bit): dwType, dwSize, hDevice, wParam, then RAWMOUSE's usFlags, a pad,
        // usButtonFlags, usButtonData, ulRawButtons, lLastX, lLastY, ulExtraInformation
        let mut logger = Logger::new().unwrap();
        let raw = &mut logger.raw_input.0;
        raw[0..4].copy_from_slice(&0u32.to_le_bytes());
        raw[8..16].copy_from_slice(&0xABCDu64.to_le_bytes());
        raw[24..26].copy_from_slice(&0x0001u16.to_le_bytes());
        raw[28..30].copy_from_slice(&0x0400u16.to_le_bytes());
        raw[30..32].copy_from_slice(&0xFF88u16.to_le_bytes());
        raw[36..40].copy_from_slice(&(-7i32).to_le_bytes());
        raw[40..44].copy_from_slice(&12i32.to_le_bytes());
        logger.record(42);
        // a keyboard's input is no record
        logger.raw_input.0[0..4].copy_from_slice(&1u32.to_le_bytes());
        logger.record(43);
        logger.close();
        let mut expected = reader::device(0xABCD, 0).to_vec();
        expected.extend_from_slice(&reader::event(42, -7, 12, 0x0001, 0x0400, 0xFF88, 0));
        assert_eq!(logger.records, expected);
        assert_eq!(logger.device_names.len(), 1);
    }

    /// The per-event cost is under 60 us, well inside an 8000 Hz mouse's 125 us.
    #[cfg(windows)]
    #[test]
    fn the_logger_keeps_up() {
        // the per-event cost, as mouse_log.py --bench measures it: far under the 125 us an 8000 Hz mouse allows
        let report = bench(2000, 50_000).unwrap();
        let total: f64 = report.split("total ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(total < 60.0, "{report}");
    }
}
