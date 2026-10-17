//! The recordings: the list (python/retired/server.py: Library.list), a recording's video from its id and its folder,
//! videos and stats files added from the user's computer, and each scenario's facts from its scenario file.
//!
//! In: the VODs folder (a folder per scenario, files named as KovOBS names them), the uploads folder and the page's
//! uploads, and KovaaK's scenario folders. Out: the recordings list (/api/vods), each recording's video and folder for
//! the rest of the library, the uploads kept in the uploads folder, and the scenarios' facts for the reports.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};

use aimview::scenario::{Facts, HitboxKind, Kind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::names::{free_name, local_stamp, parse_name, parse_titled, parse_video, slug};
use super::stats::UPLOAD_SOURCE;
use super::{Answer, Failure, Library, keep_json, modified, read_kept};
use crate::disk::Entry;
use crate::store::{Item, Mark};

/// The extensions (lower case) of the files the library takes as videos.
pub(crate) const VIDEO_TYPES: [&str; 4] = ["mp4", "mkv", "mov", "webm"];
/// What an upload's id starts with: "uploads/<its file name in the uploads folder>".
const UPLOADS_ID: &str = "uploads/";
/// Where uploaded stats files are kept, in the uploads folder.
pub(super) const STATS_UPLOADS: &str = "stats";
/// An upload's body while it arrives (`spool`): ".incoming-<process id>-<number>.part" in the uploads folder.
const SPOOL_PREFIX: &str = ".incoming-";
/// The end of a spooled body's name (see `SPOOL_PREFIX`).
const SPOOL_SUFFIX: &str = ".part";
/// A link's download folder while it downloads (links.rs): ".link-<process id>-<number>" in the uploads folder.
const LINK_FOLDER_PREFIX: &str = ".link-";
/// A scenario file's extension.
const SCENARIO_EXTENSION: &str = "sce";

/// Whether a recording's id is an upload's (added from the user's computer or from a link).
pub(crate) fn is_upload(id: &str) -> bool {
    id.starts_with(UPLOADS_ID)
}

/// The id of the upload kept in the uploads folder as `name`.
fn upload_id(name: &str) -> String {
    format!("{UPLOADS_ID}{name}")
}

/// How long an upload body or a link's download folder must have gone unchanged, in seconds, before another process
/// may remove it as left behind: longer than any upload or download stalls.
const STALE_AFTER_S: f64 = 3600.0;

/// Removes the upload bodies (`spool`'s ".incoming-<process id>-<number>.part" files) and the links' download folders
/// (links.rs: ".link-<process id>-<number>") left in `dir` by a server that stopped mid-upload or mid-download, which
/// never moved them into place. One is left behind only when all three hold: its process id is not `process_id` (this
/// process's), no process with that id is running (disk.rs `process_running`), and nothing in it changed for
/// STALE_AFTER_S before `now_s` (seconds since 1970). The running check keeps what another live server (the phone
/// server beside the app) is still writing; the age keeps it when its id was reused or cannot be checked. Every other
/// file stays.
pub(super) fn remove_stale_spools(dir: &Path, process_id: u32, now_s: f64) {
    let stale = |owner: Option<&str>, path: &Path| {
        left_by_another(owner, process_id)
            && last_change(path).is_some_and(|changed_s| now_s - changed_s > STALE_AFTER_S)
    };
    for entry in crate::disk::read_dir(dir).into_iter().flatten().flatten() {
        let (name, path) = (entry.file_name().to_string_lossy().into_owned(), entry.path());
        let spool = name.strip_prefix(SPOOL_PREFIX).and_then(|rest| rest.strip_suffix(SPOOL_SUFFIX));
        let removed = if crate::disk::is_file(&path) && stale(spool, &path) {
            crate::disk::remove_file(&path)
        } else if entry.is_dir() && stale(name.strip_prefix(LINK_FOLDER_PREFIX), &path) {
            crate::disk::remove_dir_all(&path)
        } else {
            continue;
        };
        if let Err(error) = removed {
            eprintln!("{}: {error}", path.display());
        }
    }
}

/// When a file, or a folder or anything in it, last changed (seconds since 1970); None when a time cannot be read.
fn last_change(path: &Path) -> Option<f64> {
    let own = crate::disk::metadata(path).ok()?.modified()?;
    let mut inside =
        crate::disk::read_dir(path).into_iter().flatten().flatten().map(|entry| last_change(&entry.path()));
    inside.try_fold(own, |newest, changed| Some(newest.max(changed?)))
}

/// Whether a spool's or a link folder's "<process id>-<number>" names a process that is not `process_id` and is not
/// running.
fn left_by_another(owner_and_number: Option<&str>, process_id: u32) -> bool {
    owner_and_number
        .and_then(|rest| rest.split_once('-'))
        .filter(|(_, number)| number.parse::<u64>().is_ok())
        .and_then(|(owner, _)| owner.parse::<u32>().ok())
        .is_some_and(|owner| owner != process_id && !crate::disk::process_running(owner))
}

/// Where one part of an upload sent in parts goes (/api/upload with size=).
pub(crate) struct UploadPart {
    /// The name the first part answered (its spool's "<process id>-<number>"); None for the first part.
    pub upload: Option<String>,
    /// The part's first byte in the whole file.
    pub at: u64,
    /// The whole file's size in bytes.
    pub size: u64,
}

/// A spool's name as an upload sent in parts names it: "<process id>-<number>" (see `SPOOL_PREFIX`).
fn spool_name(spool: &Path) -> String {
    let name = spool.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    name.strip_prefix(SPOOL_PREFIX).and_then(|rest| rest.strip_suffix(SPOOL_SUFFIX)).unwrap_or_default().to_string()
}

/// The scenario files in `dir`, added to `files`.
fn push_scenario_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in crate::disk::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case(SCENARIO_EXTENSION)) {
            files.push(path);
        }
    }
}

/// A file's extension in lower case; empty when it has none.
fn lower_extension(path: &Path) -> String {
    path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Two files are compared this many bytes at a time.
const COMPARE_CHUNK_BYTES: usize = 1 << 20;

/// An upload's body: its bytes, or the file it was spooled to as it arrived.
enum Body<'a> {
    /// The body in memory.
    Bytes(&'a [u8]),
    /// The body in a file the HTTP server wrote as it arrived (`Library::spool`).
    Spooled(&'a Path),
}

impl Body<'_> {
    /// Its size in bytes (0 when a spooled file cannot be read).
    fn len(&self) -> u64 {
        match self {
            Body::Bytes(bytes) => bytes.len() as u64,
            Body::Spooled(file) => crate::disk::metadata(file).map_or(0, |metadata| metadata.len()),
        }
    }

    /// Whether the file at `path` holds the same bytes.
    fn same_as(&self, path: &Path) -> bool {
        match self {
            Body::Bytes(bytes) => crate::disk::read(path).is_ok_and(|kept| kept == *bytes),
            Body::Spooled(file) => same_bytes(file, path),
        }
    }

    /// Writes the body to `destination` (a spooled one is moved there).
    fn save(&self, destination: &Path) -> std::io::Result<()> {
        match self {
            Body::Bytes(bytes) => crate::disk::write(destination, bytes),
            Body::Spooled(file) => crate::disk::rename(file, destination)
                .or_else(|_| crate::disk::copy(file, destination).and_then(|_| crate::disk::remove_file(file))),
        }
    }

    /// Removes a spooled body that is not kept.
    fn discard(&self) {
        if let Body::Spooled(file) = self {
            let _ = crate::disk::remove_file(file);
        }
    }
}

/// Whether two files hold the same bytes, read a chunk at a time.
fn same_bytes(a: &Path, b: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    let (Ok(a), Ok(b)) = (crate::disk::File::open(a), crate::disk::File::open(b)) else {
        return false;
    };
    let mut a = BufReader::with_capacity(COMPARE_CHUNK_BYTES, a);
    let mut b = BufReader::with_capacity(COMPARE_CHUNK_BYTES, b);
    loop {
        let (Ok(left), Ok(right)) = (a.fill_buf(), b.fill_buf()) else {
            return false;
        };
        let length = left.len().min(right.len());
        if length == 0 {
            return left.is_empty() && right.is_empty();
        }
        if left[..length] != right[..length] {
            return false;
        }
        a.consume(length);
        b.consume(length);
    }
}

/// A row of the quick list (`Library::recordings`): what the file's name gives; the rest is not looked at yet.
fn quick_row(id: &str, scenario: &str, score: Option<f64>, stamp: &str, not_aim: bool) -> Value {
    json!({
        "id": id, "scenario": scenario, "kind": null, "score": score, "stamp": stamp, "mtime": 0, "size": 0,
        "stats": false, "analysed": false, "not_aim": not_aim, "quick": true,
    })
}

/// The list's order, newest first: by the name's time stamp when `quick`, else by the file's time of change.
fn sort_rows(rows: &mut [Value], quick: bool) {
    if quick {
        rows.sort_by(|a, b| b["stamp"].as_str().unwrap_or("").cmp(a["stamp"].as_str().unwrap_or("")));
    } else {
        rows.sort_by(|a, b| b["mtime"].as_f64().unwrap_or(0.0).total_cmp(&a["mtime"].as_f64().unwrap_or(0.0)));
    }
}

impl Library {
    /// Where videos (and in stats/, stats files) added from the user's computer are kept.
    pub(crate) fn uploads(&self) -> PathBuf {
        self.folders.uploads.clone()
    }

    /// A recording's video, from its id: a path in the VODs folder, or "uploads/<name>".
    pub fn resolve(&self, id: &str) -> Answer<PathBuf> {
        let (root, relative) = match id.strip_prefix(UPLOADS_ID) {
            Some(name) => (self.uploads(), name.to_string()),
            None => (self.vods().ok_or_else(|| Failure::missing("no VODs folder is chosen"))?, id.to_string()),
        };
        let path = root.join(&relative);
        let is_video = VIDEO_TYPES.contains(&lower_extension(&path).as_str());
        let inside = crate::disk::canonicalize(&path)
            .ok()
            .zip(crate::disk::canonicalize(&root).ok())
            .is_some_and(|(path, root)| path.starts_with(root));
        if !is_video || !inside || !crate::disk::is_file(&path) {
            return Err(Failure::missing(format!("no recording {id}")));
        }
        Ok(path)
    }

    /// A recording's folder in the data folder (python/retired/server.py: cache_dir), where `Files` keeps its reviews,
    /// areas and marks: aimview-tool gives it to Python's scripts.
    pub fn review_dir(&self, id: &str) -> PathBuf {
        crate::store::recording_folder(&self.folders, id)
    }

    /// Each scenario's facts by lower-case name, read once: from the scenario folders' files (in the browser, what it
    /// keeps of them), then for a scenario none of them has, the facts an opened export brought (library/export.rs).
    pub(crate) fn facts(&self) -> Arc<HashMap<String, Facts>> {
        let mut cached = self.facts.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(facts) = cached.as_ref() {
            return facts.clone();
        }
        let mut by_name = match self.store().kovaak() {
            Some(kovaak) => {
                let stem = |path: &str| Path::new(path).file_stem().map(|stem| stem.to_string_lossy().to_lowercase());
                let rows = kovaak.scenarios().unwrap_or_default();
                rows.into_iter().filter_map(|row| Some((stem(&row.path)?, row.facts))).collect()
            }
            None => self.scenario_files_facts(),
        };
        let imported: HashMap<String, Facts> = read_kept(self.store(), Item::ImportedScenarios).unwrap_or_default();
        for (name, facts) in imported {
            by_name.entry(name).or_insert(facts);
        }
        let by_name = Arc::new(by_name);
        *cached = Some(by_name.clone());
        by_name
    }

    /// Each scenario's facts by lower-case name, from the scenario folders' files (a later folder's file wins).
    fn scenario_files_facts(&self) -> HashMap<String, Facts> {
        let mut files: Vec<PathBuf> = Vec::new();
        for folder in &self.config.scenarios {
            push_scenario_files(folder, &mut files);
            for entry in crate::disk::read_dir(folder).into_iter().flatten().flatten() {
                if entry.is_dir() {
                    push_scenario_files(&entry.path(), &mut files);
                }
            }
        }
        let mut by_name = HashMap::new();
        for path in files {
            let Ok(bytes) = crate::disk::read(&path) else { continue };
            let name = path.file_stem().map(|stem| stem.to_string_lossy().to_lowercase()).unwrap_or_default();
            by_name.insert(name, aimview::scenario::facts(&aimview::scenario::text_of(&bytes)));
        }
        by_name
    }

    /// Forgets the scenarios' facts read: they are read again when next needed.
    pub(super) fn forget_facts(&self) {
        *self.facts.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Every scenario's facts by lower-case name, as JSON (aimview-tool scenarios).
    pub fn scenarios(&self) -> Value {
        json!(*self.facts())
    }

    /// The facts of a video's scenario (from its name), if its scenario file was found.
    pub(crate) fn facts_of(&self, video: &Path) -> Option<Facts> {
        let scenario = parse_video(video)?.0.to_lowercase();
        self.facts().get(&scenario).cloned()
    }

    /// A scenario's kind as JSON ("static", "tracking", ...); null when its scenario file was not found.
    fn kind(&self, scenario: &str) -> Value {
        self.facts().get(&scenario.to_lowercase()).map_or(Value::Null, |facts| json!(facts.kind))
    }

    /// The kind of run the user chose for a recording (kind.json); None when it follows its scenario.
    pub(crate) fn kind_pick(&self, id: &str) -> Option<Kind> {
        read_kept::<KindPick>(self.store(), Item::Mark(id, Mark::KindPick)).map(|pick| pick.kind)
    }

    /// The bots' hitbox shape the user chose for a recording (hitbox.json); None when it follows its scenario.
    pub(crate) fn hitbox_pick(&self, id: &str) -> Option<HitboxKind> {
        read_kept::<HitboxPick>(self.store(), Item::Mark(id, Mark::HitboxPick)).map(|pick| pick.hitbox)
    }

    /// POST /api/hitbox: the bots' hitbox shape the user chose for the recording ({hitbox: "spheroid"}, "cylindrical"
    /// or "cuboid"), or {hitbox: null} to follow its scenario again. The report is worked out when it is shown, so
    /// nothing is reviewed again. Answers {hitbox_pick} as `recordings` gives it.
    pub fn set_hitbox(&self, id: &str, body: &Value) -> Answer<Value> {
        self.resolve(id)?;
        let item = Item::Mark(id, Mark::HitboxPick);
        match body.get("hitbox").filter(|hitbox| !hitbox.is_null()) {
            None => {
                let _ = self.store().remove(item);
            }
            Some(hitbox) => {
                let hitbox: HitboxKind =
                    serde_json::from_value(hitbox.clone()).map_err(|error| Failure::bad(format!("hitbox: {error}")))?;
                keep_json(self.store(), item, &HitboxPick { hitbox })?;
            }
        }
        Ok(json!({ "hitbox_pick": self.hitbox_pick(id) }))
    }

    /// A recording's row fields for its kind: `kind`, the user's choice else its scenario's (null when neither is
    /// known), and `kind_pick`, the user's choice (null when it follows the scenario).
    fn kind_fields(&self, id: &str, scenario: &str, read_pick: bool) -> (Value, Value) {
        let pick = if read_pick { self.kind_pick(id) } else { None };
        (pick.map_or_else(|| self.kind(scenario), |kind| json!(kind)), json!(pick))
    }

    /// The facts the review and the report take for a recording: its scenario's, with the kind the user chose in place
    /// of the file's; that kind alone when the scenario file is not found (a link's title names no scenario); None when
    /// neither is known.
    pub(crate) fn facts_for(&self, id: &str, video: &Path) -> Option<Facts> {
        let facts = self.facts_of(video);
        let Some(kind) = self.kind_pick(id) else { return facts };
        Some(match facts {
            Some(facts) => Facts { kind, ..facts },
            None => Facts { kind, limit: None, targets: None, reload: None, hitbox: None },
        })
    }

    /// POST /api/kind: the kind of run the user chose for the recording ({kind: "tracking"}), or {kind: null} to follow
    /// its scenario again. The report is worked out when it is shown, so nothing is reviewed again. Answers the row's
    /// {kind, kind_pick} as `recordings` gives them.
    pub fn set_kind(&self, id: &str, body: &Value) -> Answer<Value> {
        let video = self.resolve(id)?;
        let item = Item::Mark(id, Mark::KindPick);
        match body.get("kind").filter(|kind| !kind.is_null()) {
            None => {
                let _ = self.store().remove(item);
            }
            Some(kind) => {
                let kind: Kind =
                    serde_json::from_value(kind.clone()).map_err(|error| Failure::bad(format!("kind: {error}")))?;
                keep_json(self.store(), item, &KindPick { kind })?;
            }
        }
        let kind = self.facts_for(id, &video).map(|facts| facts.kind);
        Ok(json!({ "kind": kind, "kind_pick": self.kind_pick(id) }))
    }

    /// The recordings, newest first (python/retired/server.py: Library.list). `quick`: only what each file's name gives
    /// (the scenario, score and time stamp) and the user's marks, newest first by the stamp, each row marked `quick`:
    /// no file is read, no stats file paired, no review looked for, so the page lists them at once and asks for the
    /// whole list after (a VODs folder holds thousands, and in the browser each file read waits on the page).
    pub fn recordings(&self, quick: bool) -> Answer<Value> {
        let not_aim = self.not_aim();
        // the recordings with a folder in the data folder: only they can have a review or a chosen stats file, so the
        // others need no look at the disk (in the browser each look waits on the page)
        let kept: HashSet<String> = if quick { HashSet::new() } else { self.store().kept() };
        let mut rows: Vec<Value> = Vec::new();
        if let Some(vods) = self.vods() {
            for folder in crate::disk::read_dir(&vods).into_iter().flatten().flatten() {
                if !folder.is_dir() {
                    continue;
                }
                let folder_name = folder.file_name().to_string_lossy().into_owned();
                for entry in crate::disk::read_dir(folder.path()).into_iter().flatten().flatten() {
                    rows.extend(self.vod_row(&entry, &folder_name, &not_aim, &kept, quick));
                }
            }
        }
        for entry in crate::disk::read_dir(self.uploads()).into_iter().flatten().flatten() {
            let path = entry.path();
            let is_video = VIDEO_TYPES.contains(&lower_extension(&path).as_str());
            if (quick && entry.is_dir()) || !is_video || (!quick && !crate::disk::is_file(&path)) {
                continue;
            }
            rows.push(self.upload_row(&path, &not_aim, quick));
        }
        sort_rows(&mut rows, quick);
        Ok(Value::Array(rows))
    }

    /// The row of a file in the VODs folder's `folder` (None when it is not named as KovOBS names a recording): from
    /// its name alone when `quick`, else with its scenario's kind, its size and time, and whether it has a stats file
    /// and a review (looked for only when the recording has a folder in `kept`).
    fn vod_row(
        &self,
        entry: &Entry,
        folder: &str,
        not_aim: &BTreeSet<String>,
        kept: &HashSet<String>,
        quick: bool,
    ) -> Option<Value> {
        let name = entry.file_name().to_string_lossy().into_owned();
        let (scenario, score, stamp) = parse_name(&name)?;
        let id = format!("{folder}/{name}");
        if quick {
            return Some(quick_row(&id, &scenario, Some(score), &stamp, not_aim.contains(&id)));
        }
        // the listing's metadata: no call a file
        let metadata = entry.metadata().ok();
        let has_folder = kept.contains(&slug(&id));
        let pick = if has_folder { self.pairing(&id) } else { None };
        let (kind, kind_pick) = self.kind_fields(&id, &scenario, has_folder);
        let hitbox_pick = if has_folder { self.hitbox_pick(&id) } else { None };
        Some(json!({
            "id": id, "scenario": scenario, "kind": kind, "kind_pick": kind_pick, "hitbox_pick": hitbox_pick,
            "score": score, "stamp": stamp,
            "mtime": metadata.and_then(|data| data.modified()).unwrap_or(0.0),
            "size": metadata.map_or(0, |data| data.len()),
            "stats": self.has_stats(pick, &id, &entry.path()),
            "analysed": has_folder && self.reviewed(&id), "not_aim": not_aim.contains(&id),
        }))
    }

    /// An upload's row of the list: named as KovOBS names a recording, or "<title> - <stamp>" (a link's), or anyhow;
    /// `quick`: from its name alone (`recordings`).
    pub(super) fn upload_row(&self, path: &Path, not_aim: &BTreeSet<String>, quick: bool) -> Value {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let id = upload_id(&name);
        let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        let (scenario, score, stamp) = match (parse_video(path), parse_titled(path)) {
            (Some((scenario, score, stamp)), _) => (scenario, Some(score), stamp),
            (None, Some((title, stamp))) => (title, None, stamp),
            (None, None) => (stem, None, local_stamp(modified(path))),
        };
        if quick {
            let mut row = quick_row(&id, &scenario, score, &stamp, not_aim.contains(&id));
            row["uploaded"] = json!(true);
            return row;
        }
        let (kind, kind_pick) = self.kind_fields(&id, &scenario, true);
        json!({
            "id": id, "scenario": scenario, "kind": kind, "kind_pick": kind_pick, "hitbox_pick": self.hitbox_pick(&id),
            "score": score, "stamp": stamp,
            "mtime": modified(path), "size": crate::disk::metadata(path).map_or(0, |data| data.len()),
            "stats": self.has_stats(self.pairing(&id), &id, path), "uploaded": true, "analysed": self.reviewed(&id),
            "not_aim": not_aim.contains(&id),
        })
    }

    /// A video added from this computer (kept in the uploads), or a stats file for a recording (`id`), which it is then
    /// paired with. Nothing is overwritten, and a video sent again is the one already kept.
    pub fn upload(&self, name: &str, id: Option<&str>, body: &[u8]) -> Answer<Value> {
        self.add_upload(name, id, &Body::Bytes(body))
    }

    /// The same for an upload already on disk (`file`, from `spool`, written as it arrived): it is moved into place,
    /// never read into memory (or removed, when the video is kept already).
    pub fn upload_file(&self, name: &str, id: Option<&str>, file: &Path) -> Answer<Value> {
        self.add_upload(name, id, &Body::Spooled(file))
    }

    /// The video in the uploads with the same bytes as `body`, when there is one (only files of its size are read).
    fn kept_copy(&self, body: &Body) -> Option<String> {
        let size = body.len();
        crate::disk::read_dir(self.uploads())
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| VIDEO_TYPES.contains(&lower_extension(path).as_str()))
            .filter(|path| {
                crate::disk::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() == size)
            })
            .find(|path| body.same_as(path))
            .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
    }

    /// A new file in the uploads folder for an upload's body, written as it arrives (the HTTP server does so), which
    /// `upload_file` then moves into place. Its name is not a video's or a stats file's: the list never shows it.
    pub fn spool(&self) -> Answer<PathBuf> {
        /// The next spool's number in this process.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        crate::disk::create_dir_all(self.uploads())?;
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        Ok(self.uploads().join(format!("{SPOOL_PREFIX}{}-{number}{SPOOL_SUFFIX}", crate::disk::process_id())))
    }

    /// One part of an upload sent in parts (the desktop app's window hands its protocol each request whole, so a
    /// video goes in parts that fit in memory). The part is added to the upload's spool; the last one makes the spool
    /// an upload, as `upload_file` does, and gives its answer. Each earlier part answers the name the next parts send
    /// as upload= and the bytes received. A part out of order or past the size is a 400.
    pub(crate) fn upload_part(&self, name: &str, id: Option<&str>, part: &UploadPart, body: &[u8]) -> Answer<Value> {
        let extension = lower_extension(Path::new(name));
        if extension != "csv" && !VIDEO_TYPES.contains(&extension.as_str()) {
            return Err(Failure::bad(format!("not a video or a stats .csv: {name}")));
        }
        let spool = match (&part.upload, part.at) {
            (Some(upload), _) => self.named_spool(upload)?,
            (None, 0) => self.spool()?,
            (None, _) => return Err(Failure::bad("a part after the first needs upload=<the name the first got>")),
        };
        let received = crate::disk::metadata(&spool).map_or(0, |metadata| metadata.len());
        let end = part.at + body.len() as u64;
        if body.is_empty() || received != part.at || end > part.size {
            return Err(Failure::bad(format!(
                "a part of {} bytes at {} does not follow the {received} bytes received of {}",
                body.len(),
                part.at,
                part.size
            )));
        }
        crate::disk::append(&spool, body)?;
        if end < part.size {
            let upload = spool_name(&spool);
            return Ok(json!({ "upload": upload, "received": end }));
        }
        let answer = self.upload_file(name, id, &spool);
        if answer.is_err() {
            let _ = crate::disk::remove_file(&spool);
        }
        answer
    }

    /// The spool an upload sent in parts writes to, from the name its first part answered ("<process id>-<number>");
    /// a 400 for any other name, so a part never writes outside the uploads folder.
    fn named_spool(&self, upload: &str) -> Answer<PathBuf> {
        let numbers = upload
            .split_once('-')
            .filter(|(owner, number)| owner.parse::<u32>().is_ok() && number.parse::<u64>().is_ok());
        match numbers {
            Some(_) => Ok(self.uploads().join(format!("{SPOOL_PREFIX}{upload}{SPOOL_SUFFIX}"))),
            None => Err(Failure::bad(format!("no upload named {upload}"))),
        }
    }

    /// An upload's checks and its place in the uploads, where `body` is saved. An empty body is a 400: a body that
    /// never arrived (the desktop window drops some) must not become an empty video.
    fn add_upload(&self, name: &str, id: Option<&str>, body: &Body) -> Answer<Value> {
        let name = Path::new(name).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if body.len() == 0 {
            body.discard();
            return Err(Failure::bad(format!("{name} arrived empty")));
        }
        let extension = lower_extension(Path::new(&name));
        let (destination, id) = if extension == "csv" {
            let id = id.ok_or_else(|| Failure::bad("a stats file needs id=<the recording's id>"))?;
            self.resolve(id)?;
            (free_name(self.uploads().join(STATS_UPLOADS).join(&name)), Some(id))
        } else if VIDEO_TYPES.contains(&extension.as_str()) {
            if let Some(kept) = self.kept_copy(body) {
                body.discard();
                return Ok(json!({ "id": upload_id(&kept), "saved": kept }));
            }
            (free_name(self.uploads().join(&name)), None)
        } else {
            return Err(Failure::bad(format!("not a video or a stats .csv: {name}")));
        };
        crate::disk::create_dir_all(destination.parent().unwrap_or(&self.uploads()))?;
        body.save(&destination)?;
        let saved = destination.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        match id {
            None => Ok(json!({ "id": upload_id(&saved), "saved": saved })),
            Some(id) => {
                let change = self.set_stats(id, &json!({ "file": saved, "source": UPLOAD_SOURCE }))?;
                Ok(json!({ "id": id, "saved": saved, "job": change["job"], "stats": change["stats"] }))
            }
        }
    }
}

/// Uploads: stale bodies removed, a video sent again kept once.
/// The bots' hitbox shape the user chose for a recording, as hitbox.json keeps it.
#[derive(Serialize, Deserialize)]
struct HitboxPick {
    /// The shape, in place of its scenario's.
    hitbox: HitboxKind,
}

/// The kind of run the user chose for a recording, as kind.json keeps it.
#[derive(Serialize, Deserialize)]
struct KindPick {
    /// The kind, in place of its scenario's.
    kind: Kind,
}

#[cfg(test)]
mod tests {
    use crate::config::{Config, Layout};
    use crate::library::Library;

    /// Two hours, in seconds: what is left that long is past STALE_AFTER_S.
    const LATER_S: f64 = 7200.0;

    /// A child process that runs for a while (`ping` waits a second between its 30 tries), for a running process's id.
    fn running_child() -> std::process::Child {
        let mut command = std::process::Command::new(if cfg!(windows) { "ping" } else { "sleep" });
        command.args(if cfg!(windows) { &["-n", "30", "127.0.0.1"][..] } else { &["30"][..] });
        command.stdout(std::process::Stdio::null()).spawn().unwrap()
    }

    /// The id of a process that has ended.
    fn ended_process_id() -> u32 {
        let mut child = running_child();
        let id = child.id();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!crate::disk::process_running(id), "the child still runs");
        id
    }

    /// The names in a folder, sorted.
    fn names_in(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The upload bodies and link folders a process that has ended left behind are removed once they are old; this
    /// process's and every other file stay.
    #[test]
    fn other_processes_upload_bodies_are_removed() {
        let dir = std::env::temp_dir().join(format!("aimview-spools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let other = ended_process_id();
        let mine = format!(".incoming-{}-0.part", std::process::id());
        let kept = [mine.as_str(), "run.mp4", ".incoming-x-0.part", ".incoming-12.part", "incoming-12-0.part"];
        for name in kept.iter().copied().chain([format!(".incoming-{other}-3.part").as_str()]) {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        // a link's download folder: the ended process's goes, this one's stays
        std::fs::create_dir_all(dir.join(format!(".link-{other}-0"))).unwrap();
        std::fs::write(dir.join(format!(".link-{other}-0")).join("video.mp4.part"), b"x").unwrap();
        let link = format!(".link-{}-0", std::process::id());
        std::fs::create_dir_all(dir.join(&link)).unwrap();
        super::remove_stale_spools(&dir, std::process::id(), crate::disk::now() + LATER_S);
        let mut want: Vec<String> = kept.iter().map(|name| name.to_string()).chain([link]).collect();
        want.sort();
        assert_eq!(names_in(&dir), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a running process is still writing stays however old it is, and what an ended process left stays while it
    /// is fresh; opening the library removes neither.
    #[test]
    fn running_and_fresh_upload_bodies_stay() {
        let dir = std::env::temp_dir().join(format!("aimview-live-spools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Config::new(dir.clone(), Layout::App, dir.join("models"));
        let uploads = config.folders().uploads;
        std::fs::create_dir_all(&uploads).unwrap();
        let mut running = running_child();
        let ended = ended_process_id();
        let names = [format!(".incoming-{}-0.part", running.id()), format!(".incoming-{ended}-0.part")];
        for name in &names {
            std::fs::write(uploads.join(name), b"x").unwrap();
        }
        let links = [format!(".link-{}-0", running.id()), format!(".link-{ended}-0")];
        for link in &links {
            std::fs::create_dir_all(uploads.join(link)).unwrap();
            std::fs::write(uploads.join(link).join("video.mp4.part"), b"x").unwrap();
        }
        let mut want: Vec<String> = names.iter().chain(&links).cloned().collect();
        want.sort();
        let library = Library::open(config).unwrap();
        assert_eq!(names_in(&library.uploads()), want, "fresh, or still written");
        // two hours on, the running process's are still kept; only the ended process's go
        super::remove_stale_spools(&uploads, std::process::id(), crate::disk::now() + LATER_S);
        let mut want = vec![names[0].clone(), links[0].clone()];
        want.sort();
        assert_eq!(names_in(&uploads), want, "still written");
        running.kill().unwrap();
        running.wait().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A video sent again is the one already kept (its id and reviews), whatever its name; a different video of the
    /// same name and size is a new upload. A spooled body that matches is removed.
    #[test]
    fn a_video_sent_again_is_the_one_kept() {
        let dir = std::env::temp_dir().join(format!("aimview-again-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let library = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        let first = library.upload("run.mp4", None, b"video one").unwrap();
        assert_eq!(library.upload("run.mp4", None, b"video one").unwrap(), first);
        assert_eq!(library.upload("renamed.mp4", None, b"video one").unwrap(), first);
        let other = library.upload("run.mp4", None, b"video two").unwrap();
        assert_eq!(other["saved"], "run (2).mp4");
        let spooled = library.spool().unwrap();
        std::fs::write(&spooled, b"video two").unwrap();
        assert_eq!(library.upload_file("run.mp4", None, &spooled).unwrap(), other);
        assert!(!spooled.exists());
        let mut kept: Vec<String> = std::fs::read_dir(library.uploads())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        kept.sort();
        assert_eq!(kept, ["run (2).mp4", "run.mp4"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The kind the user chooses for a recording is its row's kind and the facts' kind the review and the report take,
    /// even with no scenario file found (a link's title); choosing none follows the scenario again.
    #[test]
    fn a_chosen_kind_wins() {
        let dir = std::env::temp_dir().join(format!("aimview-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let library = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        let saved = library.upload("not a scenario at all - 2026.10.08-12.50.56.mp4", None, b"video").unwrap();
        let id = format!("uploads/{}", saved["saved"].as_str().unwrap());
        let video = library.resolve(&id).unwrap();
        let row = |library: &Library| {
            let rows = library.recordings(false).unwrap();
            rows.as_array().unwrap().iter().find(|row| row["id"] == id).cloned().unwrap()
        };
        assert_eq!(row(&library)["kind"], serde_json::Value::Null);
        assert!(library.facts_for(&id, &video).is_none());
        let answer = library.set_kind(&id, &serde_json::json!({ "kind": "tracking" })).unwrap();
        assert_eq!(answer, serde_json::json!({ "kind": "tracking", "kind_pick": "tracking" }));
        assert_eq!(
            (row(&library)["kind"].clone(), row(&library)["kind_pick"].clone()),
            ("tracking".into(), "tracking".into())
        );
        assert_eq!(library.facts_for(&id, &video).map(|facts| facts.kind), Some(aimview::scenario::Kind::Tracking));
        assert!(library.set_kind(&id, &serde_json::json!({ "kind": "spinning" })).is_err(), "not a kind");
        library.set_kind(&id, &serde_json::json!({ "kind": null })).unwrap();
        assert_eq!(row(&library)["kind_pick"], serde_json::Value::Null);
        assert!(library.facts_for(&id, &video).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The hitbox the user chooses is the report's: a sphere as wide as tall, a box as wide as the review's target
    /// boxes; choosing none follows the scenario again (no hitbox here).
    #[test]
    fn a_chosen_hitbox_is_the_reports() {
        use aimview::scenario::{Hitbox, HitboxKind};
        use aimview::track::Tracks;
        let dir = std::env::temp_dir().join(format!("aimview-hitbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let library = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        let saved = library.upload("not a scenario at all - 2026.10.08-12.50.56.mp4", None, b"video").unwrap();
        let id = format!("uploads/{}", saved["saved"].as_str().unwrap());
        let tracks = br#"{"fps": 60, "frames": [{"i": 0, "shift": [0, 0], "t": [], "a": [], "wh": [[1.5, 1.0]]}]}"#;
        let tracks: Tracks = serde_json::from_slice(tracks).unwrap();
        let chosen =
            |library: &Library| library.hitbox_pick(&id).map(|pick| crate::report::chosen_hitbox(pick, &tracks));
        assert_eq!(chosen(&library), None);
        let answer = library.set_hitbox(&id, &serde_json::json!({ "hitbox": "spheroid" })).unwrap();
        assert_eq!(answer, serde_json::json!({ "hitbox_pick": "spheroid" }));
        let sphere = Hitbox { kind: HitboxKind::Spheroid, width_to_height: 1.0 };
        assert_eq!(chosen(&library), Some(sphere));
        library.set_hitbox(&id, &serde_json::json!({ "hitbox": "cuboid" })).unwrap();
        let cuboid = Hitbox { kind: HitboxKind::Cuboid, width_to_height: 1.5 };
        assert_eq!(chosen(&library), Some(cuboid));
        assert!(library.set_hitbox(&id, &serde_json::json!({ "hitbox": "cone" })).is_err(), "not a shape");
        library.set_hitbox(&id, &serde_json::json!({ "hitbox": null })).unwrap();
        assert_eq!(chosen(&library), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An upload sent in parts is the whole file once its last part arrives, and only then; a part out of order, past
    /// the size or for an upload name not the service's is refused. An empty upload is refused and saves nothing.
    #[test]
    fn an_upload_in_parts_is_the_whole_file() {
        use super::UploadPart;
        let dir = std::env::temp_dir().join(format!("aimview-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let library = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        let part = |upload: Option<&str>, at: u64| UploadPart { upload: upload.map(String::from), at, size: 10 };
        let first = library.upload_part("run.mp4", None, &part(None, 0), b"0123").unwrap();
        let upload = first["upload"].as_str().unwrap().to_string();
        assert_eq!(first["received"], 4);
        assert!(library.upload_part("run.mp4", None, &part(Some(&upload), 0), b"0123").is_err(), "out of order");
        assert!(library.upload_part("run.mp4", None, &part(Some(&upload), 4), b"4567890").is_err(), "past the size");
        assert!(library.upload_part("run.mp4", None, &part(Some("../x"), 4), b"4567").is_err(), "not a spool");
        assert!(library.upload_part("run.mp4", None, &part(None, 4), b"4567").is_err(), "a later part needs its name");
        let second = library.upload_part("run.mp4", None, &part(Some(&upload), 4), b"4567").unwrap();
        assert_eq!(second, serde_json::json!({ "upload": upload, "received": 8 }));
        assert!(names_in(&library.uploads()).iter().all(|name| name.starts_with(".incoming-")), "not yet a video");
        let last = library.upload_part("run.mp4", None, &part(Some(&upload), 8), b"89").unwrap();
        assert_eq!(last, serde_json::json!({ "id": "uploads/run.mp4", "saved": "run.mp4" }));
        assert_eq!(std::fs::read(library.uploads().join("run.mp4")).unwrap(), b"0123456789");
        assert_eq!(names_in(&library.uploads()), ["run.mp4"], "the spool became the video");
        assert!(library.upload("empty.mp4", None, b"").is_err());
        assert_eq!(names_in(&library.uploads()), ["run.mp4"], "nothing saved for an empty upload");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
