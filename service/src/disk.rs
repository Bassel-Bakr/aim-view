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
    len: u64,
    modified: Option<f64>,
    dir: bool,
}

// a file's length, as std's Metadata has it
#[allow(clippy::len_without_is_empty)]
impl Metadata {
    pub fn len(&self) -> u64 {
        self.len
    }

    /// The time of change in seconds since 1970, when the file system gives one.
    pub fn modified(&self) -> Option<f64> {
        self.modified
    }

    pub fn is_dir(&self) -> bool {
        self.dir
    }

    pub fn is_file(&self) -> bool {
        !self.dir
    }
}

pub use imp::*;

#[cfg(feature = "native")]
mod imp {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub use std::fs::File;
    pub use std::time::Instant;

    impl From<std::fs::Metadata> for Metadata {
        fn from(metadata: std::fs::Metadata) -> Metadata {
            let since_epoch = metadata.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok());
            let modified = since_epoch.map(|since| since.as_secs_f64());
            Metadata { len: metadata.len(), modified, dir: metadata.is_dir() }
        }
    }

    /// An entry of a folder.
    pub struct Entry(std::fs::DirEntry);

    impl Entry {
        pub fn file_name(&self) -> OsString {
            self.0.file_name()
        }

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
        type Item = io::Result<Entry>;

        fn next(&mut self) -> Option<io::Result<Entry>> {
            self.0.next().map(|entry| entry.map(Entry))
        }
    }

    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    pub fn is_file(path: impl AsRef<Path>) -> bool {
        path.as_ref().is_file()
    }

    pub fn is_dir(path: impl AsRef<Path>) -> bool {
        path.as_ref().is_dir()
    }

    pub fn exists(path: impl AsRef<Path>) -> bool {
        path.as_ref().exists()
    }

    pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    /// Bytes added at the end of a file (made when missing).
    pub fn append(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(bytes)
    }

    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir(path)
    }

    pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        std::fs::copy(from, to)
    }

    pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
        std::fs::read_dir(path).map(ReadDir)
    }

    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        std::fs::metadata(path).map(Metadata::from)
    }

    pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    /// Seconds since 1970.
    pub fn now() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
    }

    pub fn process_id() -> u32 {
        std::process::id()
    }

    /// This computer's offset from UTC in seconds at a moment (seconds since 1970): mouse.rs's.
    pub fn utc_offset_at(secs: f64) -> i64 {
        crate::mouse::utc_offset_at(secs)
    }
}

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
        Read = 0,
        Write = 1,
        CreateDirAll = 2,
        RemoveFile = 3,
        RemoveDir = 4,
        RemoveDirAll = 5,
        Rename = 6,
        ReadDir = 7,
        Metadata = 8,
    }

    /// The bytes before a result block's own: its code and its length, each a little-endian u32.
    const BLOCK_HEAD_BYTES: usize = 8;
    /// The alignment of the blocks the host reserves with the module's `alloc` (browser-service/src/lib.rs).
    const BLOCK_ALIGN: usize = 8;
    /// A result block's codes: the call worked, the path is not there, the path is there already (or a folder to remove
    /// is not empty). Any other code is another error, with its message as the block's bytes.
    const DONE: u32 = 0;
    const NOT_FOUND: u32 = 1;
    const THERE_ALREADY: u32 = 2;

    /// A call to the host: its bytes, or its error (see `DONE` and the codes after it).
    fn call(op: Op, path: &Path, arg: &[u8]) -> io::Result<Vec<u8>> {
        let path = path.to_string_lossy();
        // SAFETY: the host reads the two byte ranges it is given and answers with a block it reserved with `alloc`
        let block = unsafe { host_fs(op as u32, path.as_ptr(), path.len(), arg.as_ptr(), arg.len()) };
        if block.is_null() {
            return Err(io::Error::other("the page did not answer"));
        }
        // SAFETY: the block starts with its code and its length, then that many bytes
        let (code, bytes) = unsafe {
            let head = std::slice::from_raw_parts(block, BLOCK_HEAD_BYTES);
            let code = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
            let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
            let bytes = std::slice::from_raw_parts(block.add(BLOCK_HEAD_BYTES), len).to_vec();
            let layout = Layout::from_size_align((BLOCK_HEAD_BYTES + len).max(1), BLOCK_ALIGN);
            dealloc(block, layout.expect("a block's layout"));
            (code, bytes)
        };
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
        bytes: Cursor<Vec<u8>>,
    }

    impl File {
        pub fn open(path: impl AsRef<Path>) -> io::Result<File> {
            Ok(File { bytes: Cursor::new(read(path)?) })
        }

        pub fn metadata(&self) -> io::Result<Metadata> {
            Ok(Metadata { len: self.bytes.get_ref().len() as u64, modified: None, dir: false })
        }
    }

    impl Read for File {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.bytes.read(buf)
        }
    }

    impl Seek for File {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(pos)
        }
    }

    /// A moment, from the page's clock.
    #[derive(Clone, Copy, Debug)]
    pub struct Instant(f64);

    impl Instant {
        pub fn now() -> Instant {
            Instant(now())
        }

        pub fn elapsed(&self) -> Duration {
            Duration::from_secs_f64((now() - self.0).max(0.0))
        }
    }

    /// An entry of a folder, as the host lists it: with its metadata when the listing gave it.
    pub struct Entry {
        name: String,
        path: PathBuf,
        dir: bool,
        meta: Option<Metadata>,
    }

    impl Entry {
        pub fn file_name(&self) -> OsString {
            OsString::from(&self.name)
        }

        pub fn path(&self) -> PathBuf {
            self.path.clone()
        }

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
        type Item = io::Result<Entry>;

        fn next(&mut self) -> Option<io::Result<Entry>> {
            self.0.next().map(Ok)
        }
    }

    fn bad_answer(what: &str, error: impl std::fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!("the page's {what}: {error}"))
    }

    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        call(Op::Read, path.as_ref(), &[])
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        // std's own words for a file that is not UTF-8
        let not_utf8 = || io::Error::new(io::ErrorKind::InvalidData, "stream did not contain valid UTF-8");
        String::from_utf8(read(path)?).map_err(|_| not_utf8())
    }

    pub fn is_file(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok_and(|found| found.is_file())
    }

    pub fn is_dir(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok_and(|found| found.is_dir())
    }

    pub fn exists(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok()
    }

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

    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::CreateDirAll, path.as_ref(), &[]).map(drop)
    }

    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveFile, path.as_ref(), &[]).map(drop)
    }

    pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveDir, path.as_ref(), &[]).map(drop)
    }

    pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveDirAll, path.as_ref(), &[]).map(drop)
    }

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

    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        /// The host's answer: the length in bytes as a JavaScript number.
        #[derive(serde::Deserialize)]
        struct HostMetadata {
            dir: bool,
            len: f64,
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
