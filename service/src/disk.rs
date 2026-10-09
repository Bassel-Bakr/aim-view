//! The file system and the clock, for the whole service: every file the library reads or writes goes through here.
//! In: the library's paths and bytes. Out: the files' bytes, listings and metadata, the time and the time zone.
//!
//! Natively (the `native` feature) each call is std's, the same call as before (`std::fs`, `SystemTime`, the process
//! id, the time zone). In the browser build each is a call to the page (the host's imports, module "host"): the files
//! are in the page's mounted folders (/data, /kovaak, /vods, /models), their paths absolute with forward slashes. The
//! modules that only build natively (the native review, ffmpeg, yt-dlp, links) keep std's calls.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// A file's or a folder's size, time of change (seconds since 1970) and kind.
#[derive(Clone, Copy, Debug)]
pub struct Metadata {
    /// The size in bytes.
    len: u64,
    /// The time of change in seconds since 1970, when the file system gives one.
    modified: Option<f64>,
    /// Whether it is a folder.
    dir: bool,
}

// a file's length, as std's Metadata has it
#[allow(clippy::len_without_is_empty)]
impl Metadata {
    /// The size in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// The time of change in seconds since 1970, when the file system gives one.
    pub fn modified(&self) -> Option<f64> {
        self.modified
    }

    /// Whether it is a folder.
    pub fn is_dir(&self) -> bool {
        self.dir
    }

    /// Whether it is a file: anything that is not a folder.
    pub fn is_file(&self) -> bool {
        !self.dir
    }
}

pub use imp::*;

/// The calls for the native build: std's own.
#[cfg(feature = "native")]
mod imp {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub use std::fs::File;
    pub use std::time::Instant;

    impl From<std::fs::Metadata> for Metadata {
        /// std's metadata: a time of change before 1970, or none, is None.
        fn from(metadata: std::fs::Metadata) -> Metadata {
            let since_epoch = metadata.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok());
            let modified = since_epoch.map(|since| since.as_secs_f64());
            Metadata { len: metadata.len(), modified, dir: metadata.is_dir() }
        }
    }

    /// An entry of a folder: std's.
    pub struct Entry(std::fs::DirEntry);

    impl Entry {
        /// The entry's name, without its folder.
        pub fn file_name(&self) -> OsString {
            self.0.file_name()
        }

        /// The folder's path joined with the entry's name.
        pub fn path(&self) -> PathBuf {
            self.0.path()
        }

        /// Whether it is a folder (`Path::is_dir` on its path: a link to a folder is one).
        pub fn is_dir(&self) -> bool {
            self.0.path().is_dir()
        }

        /// Its metadata as the listing gives it (cheap on Windows).
        pub fn metadata(&self) -> io::Result<Metadata> {
            self.0.metadata().map(Metadata::from)
        }
    }

    /// A folder's entries, read as they are taken.
    pub struct ReadDir(std::fs::ReadDir);

    impl Iterator for ReadDir {
        /// An entry, or the error reading it.
        type Item = io::Result<Entry>;

        /// The next entry, read from the folder now.
        fn next(&mut self) -> Option<io::Result<Entry>> {
            self.0.next().map(|entry| entry.map(Entry))
        }
    }

    /// The whole file's bytes.
    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    /// The whole file as text; an error when it is not UTF-8.
    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    /// Whether the path is a file; false when it cannot be read.
    pub fn is_file(path: impl AsRef<Path>) -> bool {
        path.as_ref().is_file()
    }

    /// Whether the path is a folder; false when it cannot be read.
    pub fn is_dir(path: impl AsRef<Path>) -> bool {
        path.as_ref().is_dir()
    }

    /// Whether anything is at the path.
    pub fn exists(path: impl AsRef<Path>) -> bool {
        path.as_ref().exists()
    }

    /// Writes the file whole, in place of what it held.
    pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    /// Bytes added at the end of a file (made when missing).
    pub fn append(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(bytes)
    }

    /// Makes the folder and any missing folders above it.
    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    /// Deletes a file.
    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    /// Deletes an empty folder.
    pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir(path)
    }

    /// Deletes a folder and everything in it.
    pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    /// Moves a file or a folder to a new path.
    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    /// Copies a file; gives the bytes copied.
    pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        std::fs::copy(from, to)
    }

    /// The folder's entries.
    pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
        std::fs::read_dir(path).map(ReadDir)
    }

    /// The path's size, time of change and kind, a link followed.
    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        std::fs::metadata(path).map(Metadata::from)
    }

    /// The absolute path, with links and ".." resolved; an error when nothing is there.
    pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    /// Seconds since 1970.
    pub fn now() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
    }

    /// This process's id.
    pub fn process_id() -> u32 {
        std::process::id()
    }

    /// Windows' time calls, for `utc_offset_at`.
    #[cfg(windows)]
    mod win {
        /// Seconds from 1601 (FILETIME's start) to 1970 (Unix time's).
        pub const FILETIME_TO_UNIX_S: i64 = 11_644_473_600;
        /// FILETIME's 100 ns steps in a second.
        pub const FILETIME_STEPS_PER_S: i64 = 10_000_000;

        /// Windows' SYSTEMTIME: a date and time in parts.
        #[repr(C)]
        #[derive(Default)]
        pub struct SystemTime {
            /// The year, as 2026.
            pub year: u16,
            /// The month, 1 to 12.
            pub month: u16,
            /// The day of the week, 0 (Sunday) to 6.
            pub weekday: u16,
            /// The day of the month, 1 to 31.
            pub day: u16,
            /// The hour, 0 to 23.
            pub hour: u16,
            /// The minute, 0 to 59.
            pub minute: u16,
            /// The second, 0 to 59.
            pub second: u16,
            /// The millisecond, 0 to 999.
            pub ms: u16,
        }

        #[link(name = "kernel32")]
        unsafe extern "system" {
            /// A FILETIME (100 ns steps since 1601) in parts; 0 when it fails.
            pub fn FileTimeToSystemTime(file_time: *const u64, system_time: *mut SystemTime) -> i32;
            /// Parts back to a FILETIME; 0 when it fails.
            pub fn SystemTimeToFileTime(system_time: *const SystemTime, file_time: *mut u64) -> i32;
            /// A UTC time in parts as local time in a time zone (null: the computer's), daylight saving time included; 0
            /// when it fails.
            pub fn SystemTimeToTzSpecificLocalTime(
                zone: *const std::ffi::c_void,
                utc: *const SystemTime,
                local: *mut SystemTime,
            ) -> i32;
        }
    }

    /// This computer's offset from UTC (local minus UTC, seconds) at a moment (seconds since 1970), daylight saving time
    /// included, as Python's local time conversions take it: Windows' time zone, or on Linux and macOS the system's (TZ,
    /// else /etc/localtime), read by the C library's localtime_r. 0 where it cannot be read. (The browser build reads the
    /// browser's.)
    pub fn utc_offset_at(secs: f64) -> i64 {
        #[cfg(unix)]
        {
            unsafe extern "C" {
                /// Reads the time zone from TZ or the system: POSIX's (the libc crate declares it only for Windows).
                fn tzset();
            }
            let time = secs.floor() as libc::time_t;
            // SAFETY: tm is plain data that localtime_r fills; tzset reads the time zone (nothing here changes TZ)
            let mut tm: libc::tm = unsafe { std::mem::zeroed() };
            unsafe { tzset() };
            if !unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
                return tm.tm_gmtoff as i64;
            }
        }
        #[cfg(windows)]
        {
            let file_time = ((secs.floor() as i64 + win::FILETIME_TO_UNIX_S) * win::FILETIME_STEPS_PER_S) as u64;
            let (mut utc, mut local, mut back) = (win::SystemTime::default(), win::SystemTime::default(), 0u64);
            // SAFETY: each call reads and writes the structs it is given
            let ok = unsafe {
                win::FileTimeToSystemTime(&file_time, &mut utc) != 0
                    && win::SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
                    && win::SystemTimeToFileTime(&local, &mut back) != 0
            };
            if ok {
                return (back as i64 - file_time as i64).div_euclid(win::FILETIME_STEPS_PER_S);
            }
        }
        let _ = secs;
        0
    }
}

/// The calls for the browser build: each a call to the page.
#[cfg(not(feature = "native"))]
mod imp {
    use super::*;
    use std::alloc::{Layout, dealloc};
    use std::io::{Cursor, Read, Seek, SeekFrom};
    use std::time::Duration;

    #[link(wasm_import_module = "host")]
    unsafe extern "C" {
        /// A file system call (see `Op`) on `path` (UTF-8), with `arg` (an op's bytes): a result block the host
        /// reserved with the module's `alloc`: [u32 code][u32 len][len bytes], little-endian. Asynchronous on the
        /// page's side (Asyncify): the call waits for it.
        fn host_fs(op: u32, path_ptr: *const u8, path_len: usize, arg_ptr: *const u8, arg_len: usize) -> *mut u8;
        /// Date.now() / 1000.
        fn host_now() -> f64;
        /// Seconds east of UTC at that moment, from the browser's time zone.
        fn host_utc_offset(secs: f64) -> i32;
    }

    /// The host's file system calls.
    #[derive(Clone, Copy)]
    enum Op {
        /// The file's bytes.
        Read = 0,
        /// Writes the file whole from `arg`.
        Write = 1,
        /// Makes the folder and any missing folders above it.
        CreateDirAll = 2,
        /// Deletes a file.
        RemoveFile = 3,
        /// Deletes an empty folder.
        RemoveDir = 4,
        /// Deletes a folder and everything in it.
        RemoveDirAll = 5,
        /// Moves the path to `arg`'s path.
        Rename = 6,
        /// The folder's entries as JSON (`Listed`).
        ReadDir = 7,
        /// The path's size, time of change and kind as JSON.
        Metadata = 8,
    }

    /// The bytes before a result block's own: its code and its length, each a little-endian u32.
    const BLOCK_HEAD_BYTES: usize = 8;
    /// The alignment of the blocks the host reserves with the module's `alloc` (browser-service/src/lib.rs).
    const BLOCK_ALIGN: usize = 8;
    /// A result block's code: the call worked. Any code but this one, `NOT_FOUND` and `THERE_ALREADY` is another
    /// error, with its message as the block's bytes.
    const DONE: u32 = 0;
    /// A result block's code: the path is not there.
    const NOT_FOUND: u32 = 1;
    /// A result block's code: the path is there already, or a folder to remove is not empty.
    const THERE_ALREADY: u32 = 2;

    /// A result block's code and bytes, the block freed: [u32 code][u32 len][len bytes], little-endian, as the host
    /// answers its calls (`host_fs` here, `host_sql` in sql.rs). A null block is the page not answering.
    ///
    /// # Safety
    ///
    /// `block` is null or a block the host reserved with the module's `alloc`, whose length is its own.
    pub(crate) unsafe fn take_block(block: *mut u8) -> io::Result<(u32, Vec<u8>)> {
        if block.is_null() {
            return Err(io::Error::other("the page did not answer"));
        }
        // SAFETY: the block starts with its code and its length, then that many bytes
        unsafe {
            let head = std::slice::from_raw_parts(block, BLOCK_HEAD_BYTES);
            let code = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
            let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
            let bytes = std::slice::from_raw_parts(block.add(BLOCK_HEAD_BYTES), len).to_vec();
            let layout = Layout::from_size_align((BLOCK_HEAD_BYTES + len).max(1), BLOCK_ALIGN);
            dealloc(block, layout.expect("a block's layout"));
            Ok((code, bytes))
        }
    }

    /// A call to the host: its bytes, or its error (see `DONE` and the codes after it).
    fn call(op: Op, path: &Path, arg: &[u8]) -> io::Result<Vec<u8>> {
        let path = path.to_string_lossy();
        // SAFETY: the host reads the two byte ranges it is given and answers with a block it reserved with `alloc`
        let block = unsafe { host_fs(op as u32, path.as_ptr(), path.len(), arg.as_ptr(), arg.len()) };
        // SAFETY: the host answers with a result block it reserved with `alloc`, or none
        let (code, bytes) = unsafe { take_block(block) }?;
        let message = || String::from_utf8_lossy(&bytes).into_owned();
        match code {
            DONE => Ok(bytes),
            NOT_FOUND => Err(io::Error::new(io::ErrorKind::NotFound, format!("{}: not found", path))),
            THERE_ALREADY => {
                Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{}: there already, or not empty", path)))
            }
            _ => Err(io::Error::other(message())),
        }
    }

    /// A file read whole when it is opened, then read and sought in memory (the stats files' and the mouse logs' ends,
    /// a review's last bytes).
    pub struct File {
        /// The file's bytes, and the place the next read starts from.
        bytes: Cursor<Vec<u8>>,
    }

    impl File {
        /// Reads the whole file from the page.
        pub fn open(path: impl AsRef<Path>) -> io::Result<File> {
            Ok(File { bytes: Cursor::new(read(path)?) })
        }

        /// Its size; no time of change, which the bytes in memory do not carry.
        pub fn metadata(&self) -> io::Result<Metadata> {
            Ok(Metadata { len: self.bytes.get_ref().len() as u64, modified: None, dir: false })
        }
    }

    impl Read for File {
        /// Reads from the bytes in memory.
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.bytes.read(buf)
        }
    }

    impl Seek for File {
        /// Moves within the bytes in memory.
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(pos)
        }
    }

    /// A moment, from the page's clock: seconds since 1970.
    #[derive(Clone, Copy, Debug)]
    pub struct Instant(f64);

    impl Instant {
        /// This moment.
        pub fn now() -> Instant {
            Instant(now())
        }

        /// The time since this moment; zero when the page's clock went back.
        pub fn elapsed(&self) -> Duration {
            Duration::from_secs_f64((now() - self.0).max(0.0))
        }
    }

    /// An entry of a folder, as the host lists it: with its metadata when the listing gave it.
    pub struct Entry {
        /// The entry's name, without its folder.
        name: String,
        /// The folder's path joined with the name.
        path: PathBuf,
        /// Whether it is a folder.
        dir: bool,
        /// Its metadata, when the listing gave both its size and its time.
        meta: Option<Metadata>,
    }

    impl Entry {
        /// The entry's name, without its folder.
        pub fn file_name(&self) -> OsString {
            OsString::from(&self.name)
        }

        /// The folder's path joined with the entry's name.
        pub fn path(&self) -> PathBuf {
            self.path.clone()
        }

        /// Whether it is a folder.
        pub fn is_dir(&self) -> bool {
            self.dir
        }

        /// Its metadata: the listing's, else a call to the host.
        pub fn metadata(&self) -> io::Result<Metadata> {
            self.meta.map_or_else(|| metadata(&self.path), Ok)
        }
    }

    /// A folder's entries.
    pub struct ReadDir(std::vec::IntoIter<Entry>);

    impl Iterator for ReadDir {
        /// An entry; the listing was read whole, so never an error.
        type Item = io::Result<Entry>;

        /// The next entry of the listing.
        fn next(&mut self) -> Option<io::Result<Entry>> {
            self.0.next().map(Ok)
        }
    }

    /// The error for an answer of the page's that is not the JSON it should be; `what` names the answer.
    fn bad_answer(what: &str, error: impl std::fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!("the page's {what}: {error}"))
    }

    /// The whole file's bytes.
    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        call(Op::Read, path.as_ref(), &[])
    }

    /// The whole file as text; an error when it is not UTF-8.
    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        // std's own words for a file that is not UTF-8
        let not_utf8 = || io::Error::new(io::ErrorKind::InvalidData, "stream did not contain valid UTF-8");
        String::from_utf8(read(path)?).map_err(|_| not_utf8())
    }

    /// Whether the path is a file; false when the page cannot read it.
    pub fn is_file(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok_and(|found| found.is_file())
    }

    /// Whether the path is a folder; false when the page cannot read it.
    pub fn is_dir(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok_and(|found| found.is_dir())
    }

    /// Whether anything is at the path.
    pub fn exists(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok()
    }

    /// Writes the file whole, in place of what it held.
    pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        call(Op::Write, path.as_ref(), bytes.as_ref()).map(drop)
    }

    /// Bytes added at the end of a file (made when missing): the file read and written again.
    pub fn append(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        let mut all = match read(path.as_ref()) {
            Ok(old) => old,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        all.extend_from_slice(bytes);
        write(path, all)
    }

    /// Makes the folder and any missing folders above it.
    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::CreateDirAll, path.as_ref(), &[]).map(drop)
    }

    /// Deletes a file.
    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveFile, path.as_ref(), &[]).map(drop)
    }

    /// Deletes an empty folder.
    pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveDir, path.as_ref(), &[]).map(drop)
    }

    /// Deletes a folder and everything in it.
    pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveDirAll, path.as_ref(), &[]).map(drop)
    }

    /// Moves a file or a folder to a new path.
    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        call(Op::Rename, from.as_ref(), to.as_ref().to_string_lossy().as_bytes()).map(drop)
    }

    /// A copy: the file read and written.
    pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        let bytes = read(from)?;
        write(to, &bytes)?;
        Ok(bytes.len() as u64)
    }

    /// An entry as the host lists it: [name, dir, size, time], the size and time null when the page has none at hand.
    type Listed = (String, bool, Option<f64>, Option<f64>);

    /// The folder's entries, from one listing by the page.
    pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
        let path = path.as_ref();
        let listed: Vec<Listed> =
            serde_json::from_slice(&call(Op::ReadDir, path, &[])?).map_err(|error| bad_answer("listing", error))?;
        let entry = |(name, dir, len, modified): Listed| {
            let meta = len.zip(modified).map(|(len, modified)| Metadata {
                len: len.max(0.0) as u64,
                modified: Some(modified),
                dir,
            });
            Entry { path: path.join(&name), name, dir, meta }
        };
        Ok(ReadDir(listed.into_iter().map(entry).collect::<Vec<_>>().into_iter()))
    }

    /// The path's size, time of change and kind, from the page.
    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        /// The host's answer: the length in bytes as a JavaScript number.
        #[derive(serde::Deserialize)]
        struct HostMetadata {
            /// Whether it is a folder.
            dir: bool,
            /// The size in bytes.
            len: f64,
            /// The time of change in seconds since 1970, when the page knows one.
            modified: Option<f64>,
        }
        let answer = call(Op::Metadata, path.as_ref(), &[])?;
        let host: HostMetadata = serde_json::from_slice(&answer).map_err(|error| bad_answer("metadata", error))?;
        Ok(Metadata { len: host.len.max(0.0) as u64, modified: host.modified, dir: host.dir })
    }

    /// The path as it is given: the page's folders have no links, and a path that climbs out with ".." is refused.
    pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        let path = path.as_ref();
        if path.components().any(|component| component == std::path::Component::ParentDir) {
            let message = format!("{}: no \"..\" in the page's paths", path.display());
            return Err(io::Error::new(io::ErrorKind::InvalidInput, message));
        }
        Ok(path.to_path_buf())
    }

    /// Seconds since 1970, from the page's clock.
    pub fn now() -> f64 {
        // SAFETY: a plain call to the host
        unsafe { host_now() }
    }

    /// The page has one service, so one process.
    pub fn process_id() -> u32 {
        0
    }

    /// The browser's offset from UTC in seconds at a moment (seconds since 1970).
    pub fn utc_offset_at(secs: f64) -> i64 {
        // SAFETY: a plain call to the host
        i64::from(unsafe { host_utc_offset(secs) })
    }
}

/// The time zone's offset.
#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;

    /// This computer's offset now is a whole number of quarter hours.
    #[test]
    fn offsets_are_whole_quarter_hours() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        assert_eq!(utc_offset_at(now) % 900, 0);
    }

    /// Off Windows the offset follows the system's time zone and its daylight saving time: New York's (as a POSIX TZ,
    /// which needs no time zone files), in a child process of this test, so no other test sees TZ change.
    #[cfg(unix)]
    #[test]
    fn offsets_follow_the_time_zone() {
        if std::env::var_os("AIMVIEW_TZ_CHILD").is_some() {
            assert_eq!(utc_offset_at(1_704_067_200.0), -5 * 3600, "2024-01-01 00:00 UTC: EST");
            assert_eq!(utc_offset_at(1_719_792_000.0), -4 * 3600, "2024-07-01 00:00 UTC: EDT");
            assert_eq!(crate::library::local_stamp(1_719_792_000.0), "2024.06.30-20.00.00");
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "disk::tests::offsets_follow_the_time_zone"])
            .env("AIMVIEW_TZ_CHILD", "1")
            .env("TZ", "EST5EDT,M3.2.0,M11.1.0")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && said.contains("1 passed"), "{said}{}", String::from_utf8_lossy(&out.stderr));
    }
}
