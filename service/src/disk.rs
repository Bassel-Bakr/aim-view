//! The file system and the clock, for the whole service: every file the library reads or writes goes through here.
//! Natively (the `native` feature) each call is std's, the same call as before (`std::fs`, `SystemTime`, the process id,
//! the time zone). In the browser build each is a call to the page (the host's imports, module "host"): the files are in
//! the page's mounted folders (/data, /kovaak, /vods, /models), their paths absolute with forward slashes. The modules
//! that only build natively (the native review, ffmpeg, yt-dlp, links) keep std's calls.

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
        fn from(m: std::fs::Metadata) -> Metadata {
            let modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs_f64());
            Metadata { len: m.len(), modified, dir: m.is_dir() }
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
            self.0.next().map(|e| e.map(Entry))
        }
    }

    pub fn read(p: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        std::fs::read(p)
    }

    pub fn read_to_string(p: impl AsRef<Path>) -> io::Result<String> {
        std::fs::read_to_string(p)
    }

    pub fn is_file(p: impl AsRef<Path>) -> bool {
        p.as_ref().is_file()
    }

    pub fn is_dir(p: impl AsRef<Path>) -> bool {
        p.as_ref().is_dir()
    }

    pub fn exists(p: impl AsRef<Path>) -> bool {
        p.as_ref().exists()
    }

    pub fn write(p: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        std::fs::write(p, bytes)
    }

    /// Bytes added at the end of a file (made when missing).
    pub fn append(p: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        std::fs::OpenOptions::new().create(true).append(true).open(p)?.write_all(bytes)
    }

    pub fn create_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::create_dir_all(p)
    }

    pub fn remove_file(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_file(p)
    }

    pub fn remove_dir(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir(p)
    }

    pub fn remove_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir_all(p)
    }

    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        std::fs::copy(from, to)
    }

    pub fn read_dir(p: impl AsRef<Path>) -> io::Result<ReadDir> {
        std::fs::read_dir(p).map(ReadDir)
    }

    pub fn metadata(p: impl AsRef<Path>) -> io::Result<Metadata> {
        std::fs::metadata(p).map(Metadata::from)
    }

    pub fn canonicalize(p: impl AsRef<Path>) -> io::Result<PathBuf> {
        std::fs::canonicalize(p)
    }

    /// Seconds since 1970.
    pub fn now() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
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
        /// reserved with the module's `alloc`: [u32 code][u32 len][len bytes], little-endian. Asynchronous on the page's
        /// side (Asyncify): the call waits for it.
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

    /// A call to the host: its bytes, or its error (code 1 not found, 2 there already or not empty, 3 another error
    /// with its message).
    fn call(op: Op, path: &Path, arg: &[u8]) -> io::Result<Vec<u8>> {
        let path = path.to_string_lossy();
        // SAFETY: the host reads the two byte ranges it is given and answers with a block it reserved with `alloc`
        let block = unsafe { host_fs(op as u32, path.as_ptr(), path.len(), arg.as_ptr(), arg.len()) };
        if block.is_null() {
            return Err(io::Error::other("the page did not answer"));
        }
        // SAFETY: the block starts with its code and its length, then that many bytes
        let (code, bytes) = unsafe {
            let head = std::slice::from_raw_parts(block, 8);
            let code = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
            let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
            let bytes = std::slice::from_raw_parts(block.add(8), len).to_vec();
            dealloc(block, Layout::from_size_align((8 + len).max(1), 8).expect("a block's layout"));
            (code, bytes)
        };
        let message = || String::from_utf8_lossy(&bytes).into_owned();
        match code {
            0 => Ok(bytes),
            1 => Err(io::Error::new(io::ErrorKind::NotFound, format!("{}: not found", path))),
            2 => Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{}: there already, or not empty", path))),
            _ => Err(io::Error::other(message())),
        }
    }

    /// A file read whole when it is opened, then read and sought in memory (the stats files' and the mouse logs' ends,
    /// a review's last bytes).
    pub struct File {
        bytes: Cursor<Vec<u8>>,
    }

    impl File {
        pub fn open(p: impl AsRef<Path>) -> io::Result<File> {
            Ok(File { bytes: Cursor::new(read(p)?) })
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

    fn bad_answer(what: &str, e: impl std::fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!("the page's {what}: {e}"))
    }

    pub fn read(p: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        call(Op::Read, p.as_ref(), &[])
    }

    pub fn read_to_string(p: impl AsRef<Path>) -> io::Result<String> {
        String::from_utf8(read(p)?).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "stream did not contain valid UTF-8"))
    }

    pub fn is_file(p: impl AsRef<Path>) -> bool {
        metadata(p).is_ok_and(|m| m.is_file())
    }

    pub fn is_dir(p: impl AsRef<Path>) -> bool {
        metadata(p).is_ok_and(|m| m.is_dir())
    }

    pub fn exists(p: impl AsRef<Path>) -> bool {
        metadata(p).is_ok()
    }

    pub fn write(p: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        call(Op::Write, p.as_ref(), bytes.as_ref()).map(drop)
    }

    /// Bytes added at the end of a file (made when missing): the file read and written again.
    pub fn append(p: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        let mut all = match read(p.as_ref()) {
            Ok(old) => old,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        all.extend_from_slice(bytes);
        write(p, all)
    }

    pub fn create_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        call(Op::CreateDirAll, p.as_ref(), &[]).map(drop)
    }

    pub fn remove_file(p: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveFile, p.as_ref(), &[]).map(drop)
    }

    pub fn remove_dir(p: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveDir, p.as_ref(), &[]).map(drop)
    }

    pub fn remove_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        call(Op::RemoveDirAll, p.as_ref(), &[]).map(drop)
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

    pub fn read_dir(p: impl AsRef<Path>) -> io::Result<ReadDir> {
        let p = p.as_ref();
        // [name, dir, size, time]: the size and time null when the page has none at hand
        let listed: Vec<(String, bool, Option<f64>, Option<f64>)> =
            serde_json::from_slice(&call(Op::ReadDir, p, &[])?).map_err(|e| bad_answer("listing", e))?;
        Ok(ReadDir(
            listed
                .into_iter()
                .map(|(name, dir, len, modified)| {
                    let meta = len.zip(modified).map(|(len, modified)| Metadata { len: len.max(0.0) as u64, modified: Some(modified), dir });
                    Entry { path: p.join(&name), name, dir, meta }
                })
                .collect::<Vec<_>>()
                .into_iter(),
        ))
    }

    pub fn metadata(p: impl AsRef<Path>) -> io::Result<Metadata> {
        #[derive(serde::Deserialize)]
        struct Answer {
            dir: bool,
            len: f64,
            modified: Option<f64>,
        }
        let a: Answer = serde_json::from_slice(&call(Op::Metadata, p.as_ref(), &[])?).map_err(|e| bad_answer("metadata", e))?;
        Ok(Metadata { len: a.len.max(0.0) as u64, modified: a.modified, dir: a.dir })
    }

    /// The path as it is given: the page's folders have no links, and a path that climbs out with ".." is refused.
    pub fn canonicalize(p: impl AsRef<Path>) -> io::Result<PathBuf> {
        let p = p.as_ref();
        if p.components().any(|c| c == std::path::Component::ParentDir) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{}: no \"..\" in the page's paths", p.display())));
        }
        Ok(p.to_path_buf())
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
