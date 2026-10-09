//! What the library keeps, through one interface (`Store`), so where it is kept can change (docs/storage-design.md):
//! the files in the data folder (`Files`, laid out as config.rs's layout says) or one SQLite database (database.rs). The
//! library formats each thing (JSON as python/retired/server.py wrote it, .npz as NumPy does); a store keeps the bytes
//! it is given and gives the same bytes back. The videos (uploads), the mouse logs (the desktop app's logger writes
//! them) and the crop-check folders stay files outside it. In: the library's items and their bytes. Out: the same
//! bytes, and what is kept for each recording; the space it takes, behind a narrower interface (`StoreUsage`).

use std::collections::{BTreeSet, HashSet};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::config::Folders;
use crate::library::slug;

/// A list of recording ids the user marked.
#[derive(Clone, Copy, Debug)]
pub enum IdList {
    /// Recordings of another game, not of an aim trainer.
    NotAimTrainer,
    /// Recordings skipped in the area queue.
    LabelSkipped,
    /// Recordings skipped in the cut-off queue.
    FaintSkipped,
}

/// What is kept for a recording beside its reviews.
#[derive(Clone, Copy, Debug)]
pub enum Mark {
    /// The user's run window (run_window.rs).
    RunWindow,
    /// The stats file the user picked for it (library/stats.rs).
    StatsPick,
    /// The kind of run the user chose for it, in place of its scenario's (library/recordings.rs).
    KindPick,
    /// The bots' hitbox shape the user chose for it, in place of its scenario's (library/recordings.rs).
    HitboxPick,
    /// The faint-target cut-off (faint.rs).
    Cutoff,
    /// The areas the user saved for it (areas.rs).
    SavedAreas,
    /// What the area finder found in it (finder.rs).
    FoundAreas,
    /// The area finder's stand-out and change maps of it (.npz), which learning from saved areas reads (finder.rs).
    FoundMaps,
}

/// A part of a review: its tracks, the video's readings (the camera's turn, the countdown), what the HUD read, and the
/// check of the kills the video alone gives.
#[derive(Clone, Copy, Debug)]
pub enum Part {
    /// Each frame's kept boxes and ids, and the view shift (tracks.json; the core's track.rs).
    Tracks,
    /// The camera's turn per frame and KovaaK's countdown bar (readings.json; the core's camera.rs).
    Readings,
    /// What the HUD read (hud.json; the core's src/hud/).
    Hud,
    /// The check of the kills the video alone gives, made only without a stats file (kills.json; the core's
    /// kill_check.rs).
    Kills,
}

/// Which of a recording's reviews: a model's, or the one python/retired/server.py kept before reviews were kept per
/// model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewBy {
    /// The review a model made, by the model's name (its folder in the recording's models folder).
    Model(String),
    /// The review kept in the recording's own folder, from before reviews were kept per model.
    Old,
}

/// One thing the library keeps.
#[derive(Clone, Copy, Debug)]
pub enum Item<'a> {
    /// What the user set (library/settings.rs).
    Settings,
    /// The area types (areas.rs).
    AreaKinds,
    /// The area finder's examples, one JSON line each (areas.rs).
    AreaExamples,
    /// The areas last saved for an added recording (areas.rs).
    UploadAreas,
    /// The scenarios' facts that opened exports brought (library/export.rs), by lower-case name: used for a scenario
    /// this computer has no file of.
    ImportedScenarios,
    /// A list of recording ids the user marked (labels.rs, faint.rs).
    Ids(IdList),
    /// A recording's mark, by its id.
    Mark(&'a str, Mark),
    /// A part of one of a recording's reviews, by its id.
    ReviewPart(&'a str, &'a ReviewBy, Part),
    /// The detector labels of submitted cut-offs (faint.rs): their rows, one JSON line each.
    CutoffRows,
    /// One crop of those labels, by its row's file (train/<name>.npz).
    CutoffCrop(&'a str),
}

impl<'a> Item<'a> {
    /// The item's name as a file, which messages name it by.
    pub const fn file_name(self) -> &'a str {
        match self {
            Item::Settings => "settings.json",
            Item::AreaKinds => "area_kinds.json",
            Item::AreaExamples => "area_examples.jsonl",
            Item::UploadAreas => "exclude_uploads.json",
            Item::ImportedScenarios => "imported_scenarios.json",
            Item::Ids(IdList::NotAimTrainer) => "not_aim_trainer.json",
            Item::Ids(IdList::LabelSkipped) => "label_skipped.json",
            Item::Ids(IdList::FaintSkipped) => "faint_skipped.json",
            Item::Mark(_, mark) => mark.file_name(),
            Item::ReviewPart(_, _, part) => part.file_name(),
            Item::CutoffRows => "checked.jsonl",
            Item::CutoffCrop(file) => file,
        }
    }
}

impl Mark {
    /// The mark's file in a recording's folder.
    pub const fn file_name(self) -> &'static str {
        match self {
            Mark::RunWindow => "run.json",
            Mark::StatsPick => "stats.json",
            Mark::KindPick => "kind.json",
            Mark::HitboxPick => "hitbox.json",
            Mark::Cutoff => "faint.json",
            Mark::SavedAreas => "exclude.json",
            Mark::FoundAreas => "areas.json",
            Mark::FoundMaps => "areas_maps.npz",
        }
    }
}

impl Part {
    /// The part's file in a review's folder.
    pub const fn file_name(self) -> &'static str {
        match self {
            Part::Tracks => "tracks.json",
            Part::Readings => "readings.json",
            Part::Hud => "hud.json",
            Part::Kills => "kills.json",
        }
    }
}

/// The space a store's parts take and their removal, for the data panel (library/usage.rs): all a caller needs that
/// only counts or frees space.
pub trait StoreUsage: Send + Sync {
    /// Each model's reviews (the old review's model is ""), with how many recordings and bytes they keep.
    fn review_sizes(&self) -> Vec<ReviewSize>;
    /// Forgets every review `model` made ("" the old reviews); answers how many recordings had one.
    fn remove_reviews(&self, model: &str) -> io::Result<usize>;
    /// The bytes the recordings' marks and areas keep.
    fn marks_size(&self) -> u64;
    /// The bytes the cut-off labels keep (their rows and crops).
    fn cutoff_size(&self) -> u64;
    /// The whole store's size when it is one file (the database), else None.
    fn file_size(&self) -> Option<u64> {
        None
    }
    /// Gives back the space that removals freed (the database's VACUUM); files need nothing.
    fn compact(&self) -> io::Result<()> {
        Ok(())
    }
}

/// Where the library keeps what it keeps (see the module's comment).
pub trait Store: StoreUsage {
    /// The bytes kept for `item`; None when nothing is.
    fn read(&self, item: Item<'_>) -> io::Result<Option<Vec<u8>>>;
    /// Whether something is kept for `item`.
    fn has(&self, item: Item<'_>) -> bool;
    /// When `item` last changed, in seconds since 1970; None when nothing is kept or no time is known.
    fn changed(&self, item: Item<'_>) -> Option<f64>;
    /// Keeps `bytes` for `item`, in place of what was kept.
    fn write(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()>;
    /// Keeps `bytes` after what is kept for `item` (as all of it when nothing is).
    fn append(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()>;
    /// Forgets what is kept for `item`.
    fn remove(&self, item: Item<'_>) -> io::Result<()>;
    /// How messages name `item` where it is kept.
    fn name(&self, item: Item<'_>) -> String;
    /// Whether the recording has a review: a model's, or the old one.
    fn reviewed(&self, id: &str) -> bool;
    /// The models the recording has a review by, in the order the store lists them.
    fn models(&self, id: &str) -> Vec<String>;
    /// The old review's model when the recording has one: "hand" when the hand-written detector made it, else None
    /// (not recorded).
    fn old_review(&self, id: &str) -> Option<Option<String>>;
    /// The recordings anything is kept for, by their slugs (library/names.rs: `slug`).
    fn kept(&self) -> HashSet<String>;
    /// The recordings with both found and saved areas, leaving out the slugs in `skip`: each one's slug and the bytes
    /// of its found and saved areas.
    fn labelled(&self, skip: &BTreeSet<String>) -> Vec<(String, Vec<u8>, Vec<u8>)>;
    /// What this store keeps of KovaaK's files in place of reading their folders: only the browser build's database
    /// does (the page cannot keep a folder under Program Files, so it reads the files once and the service keeps what
    /// it needs); None everywhere else.
    fn kovaak(&self) -> Option<&dyn Kovaak> {
        None
    }
}

/// One model's reviews: the model ("" the old reviews, from before reviews were kept per model), how many recordings
/// it reviewed, and the bytes its reviews keep (compressed in the database).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSize {
    /// The model's name; "" for the old reviews.
    pub model: String,
    /// How many recordings it reviewed.
    pub recordings: usize,
    /// The bytes its reviews keep.
    pub bytes: u64,
}

/// A folder's size: the bytes of every file in it and below, and how many files; nothing when it is not there.
pub fn folder_size(dir: &Path) -> (u64, usize) {
    let (mut bytes, mut files) = (0, 0);
    for entry in crate::disk::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match crate::disk::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                let (below, count) = folder_size(&path);
                bytes += below;
                files += count;
            }
            Ok(metadata) => {
                bytes += metadata.len();
                files += 1;
            }
            Err(_) => {}
        }
    }
    (bytes, files)
}

/// A file's size; 0 when it is not there.
fn file_len(path: &Path) -> u64 {
    crate::disk::metadata(path).map_or(0, |metadata| if metadata.is_dir() { 0 } else { metadata.len() })
}

/// A run as its stats file's footer gives it: KovaaK's score, the kills when the file gives them, and hits over shots
/// (0 to 1) when it gives both.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsRun {
    /// KovaaK's score.
    pub score: f64,
    /// The kills; None when the file does not give them.
    pub kills: Option<f64>,
    /// Hits over shots, 0 to 1; None without hits and misses.
    pub accuracy: Option<f64>,
}

/// A stats file of KovaaK's as the browser keeps it: its name, size and time of change (to tell a changed file), and
/// its run (None: the file has no score).
#[derive(Clone, Debug, PartialEq)]
pub struct StatsRow {
    /// Its file name in the stats folder.
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its time of change in seconds since 1970.
    pub modified: f64,
    /// Its run; None when it has no score.
    pub run: Option<StatsRun>,
}

/// A scenario file of KovaaK's as the browser keeps it: its path in /kovaak (scenarios/<name>.sce or
/// workshop/<item>/<name>.sce), its size and time of change, and its facts.
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioRow {
    /// Its path in /kovaak.
    pub path: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its time of change in seconds since 1970.
    pub modified: f64,
    /// What the review needs of it.
    pub facts: aimview::scenario::Facts,
}

/// KovaaK's files as the browser keeps them (see `Store::kovaak`): every stats file's run, the whole text only of
/// those a recording used, and every scenario's facts.
pub trait Kovaak {
    /// Every stats file kept.
    fn stats_files(&self) -> io::Result<Vec<StatsRow>>;
    /// Keeps the rows, in one go, in place of those of the same names; a file that changed loses its kept text.
    fn add_stats_files(&self, rows: &[StatsRow]) -> io::Result<()>;
    /// A stats file's whole text when it is kept.
    fn stats_csv(&self, name: &str) -> io::Result<Option<Vec<u8>>>;
    /// Keeps a stats file's whole text alone (one read from the folder when its report needed it), when the file is
    /// kept.
    fn keep_stats_csv(&self, name: &str, csv: &[u8]) -> io::Result<()>;
    /// Keeps the whole texts of stats files that are kept, together in one pack: (name, text) each, best of one
    /// scenario (its files compress together far better than alone).
    fn keep_stats_pack(&self, files: &[(&str, &[u8])]) -> io::Result<()>;
    /// The stats files whose whole text is kept (their runs left out): the page sends the others again.
    fn stats_texts_kept(&self) -> io::Result<Vec<StatsRow>>;
    /// What is kept of KovaaK's files: how many stats files and scenario files, and their bytes.
    fn kovaak_size(&self) -> io::Result<(usize, usize, u64)>;
    /// Forgets every stats file and scenario file kept: the user chooses the folders again.
    fn clear_kovaak(&self) -> io::Result<()>;
    /// Every scenario file kept, the user's scenarios before the workshop's, each by path.
    fn scenarios(&self) -> io::Result<Vec<ScenarioRow>>;
    /// Keeps the rows, in one go, in place of those of the same paths.
    fn add_scenarios(&self, rows: &[ScenarioRow]) -> io::Result<()>;
}

/// The bytes at the end of an old tracks.json (python/retired/server.py's, kept in the recording's own folder) read for
/// the detector's name.
pub(crate) const OLD_TRACKS_TAIL_BYTES: u64 = 200;
/// The key an old tracks.json ends with when a model made it (the hand-written detector's has none).
pub(crate) const DETECTOR_KEY: &[u8] = b"\"detector\"";
/// The folder of a recording's reviews by model, one folder each.
pub(crate) const MODELS: &str = "models";

/// A recording's folder in a layout's recordings folder: aimview-tool gives it to Python's scripts.
pub fn recording_folder(folders: &Folders, id: &str) -> PathBuf {
    folders.recordings.join(slug(id))
}

/// A review's parts as files in a folder of their own (aimview-tool's --out, examples/track.rs), as report.rs reads
/// them.
pub fn folder_parts(dir: &Path) -> impl Fn(Part) -> Option<Vec<u8>> + '_ {
    move |part| crate::disk::read(dir.join(part.file_name())).ok()
}

/// The store as the files in the data folder (disk.rs), laid out as the layout's folders say: today's files, byte for
/// byte.
pub struct Files {
    /// The layout's folders: the library's own files, each recording's folder and the cut-off labels.
    folders: Folders,
}

impl Files {
    /// The store in `folders`; nothing is read or made until an item is.
    pub fn new(folders: Folders) -> Files {
        Files { folders }
    }

    /// The folder that holds a review's parts: the model's folder in the recording's models folder, or the
    /// recording's own folder for the old review.
    fn review_folder(&self, id: &str, by: &ReviewBy) -> PathBuf {
        match by {
            ReviewBy::Model(model) => recording_folder(&self.folders, id).join(MODELS).join(model),
            ReviewBy::Old => recording_folder(&self.folders, id),
        }
    }

    /// The item's file.
    pub fn path(&self, item: Item<'_>) -> PathBuf {
        let name = item.file_name();
        match item {
            Item::Settings
            | Item::AreaKinds
            | Item::AreaExamples
            | Item::UploadAreas
            | Item::ImportedScenarios
            | Item::Ids(_) => self.folders.files.join(name),
            Item::Mark(id, _) => recording_folder(&self.folders, id).join(name),
            Item::ReviewPart(id, by, _) => self.review_folder(id, by).join(name),
            Item::CutoffRows | Item::CutoffCrop(_) => self.folders.cutoff.join(name),
        }
    }

    /// The folders in the recording's models folder that hold a review.
    fn model_folders(&self, id: &str) -> impl Iterator<Item = PathBuf> {
        let models = recording_folder(&self.folders, id).join(MODELS);
        let folders = crate::disk::read_dir(models).into_iter().flatten().flatten().map(|entry| entry.path());
        folders.filter(|folder| crate::disk::is_file(folder.join(Part::Tracks.file_name())))
    }
}

/// Whether Python keeps the item as a text file (`pyjson::dump`, `write_text`): on Windows each "\n" is "\r\n".
pub(crate) fn python_text(item: Item<'_>) -> bool {
    match item {
        Item::AreaKinds | Item::AreaExamples | Item::UploadAreas | Item::Ids(_) | Item::CutoffRows => true,
        Item::Mark(_, mark) => matches!(mark, Mark::Cutoff | Mark::SavedAreas | Mark::FoundAreas),
        Item::Settings | Item::ImportedScenarios | Item::ReviewPart(..) | Item::CutoffCrop(_) => false,
    }
}

/// Every part a review can have.
pub(crate) const PARTS: [Part; 4] = [Part::Tracks, Part::Readings, Part::Hud, Part::Kills];
/// Every mark a recording can have.
pub(crate) const MARKS: [Mark; 8] = [
    Mark::RunWindow,
    Mark::StatsPick,
    Mark::KindPick,
    Mark::HitboxPick,
    Mark::Cutoff,
    Mark::SavedAreas,
    Mark::FoundAreas,
    Mark::FoundMaps,
];

/// The folder a file goes in, made when missing.
fn make_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(dir) => crate::disk::create_dir_all(dir),
        None => Ok(()),
    }
}

impl StoreUsage for Files {
    /// Each recording folder's old review (its own part files) and each of its model folders.
    fn review_sizes(&self) -> Vec<ReviewSize> {
        let mut by_model: std::collections::BTreeMap<String, ReviewSize> = std::collections::BTreeMap::new();
        let mut add = |model: &str, bytes: u64| {
            let size = by_model.entry(model.to_string()).or_insert_with(|| ReviewSize {
                model: model.to_string(),
                recordings: 0,
                bytes: 0,
            });
            size.recordings += 1;
            size.bytes += bytes;
        };
        for entry in crate::disk::read_dir(&self.folders.recordings).into_iter().flatten().flatten() {
            let dir = entry.path();
            if !entry.is_dir() {
                continue;
            }
            if crate::disk::is_file(dir.join(Part::Tracks.file_name())) {
                add("", PARTS.iter().map(|part| file_len(&dir.join(part.file_name()))).sum());
            }
            for model in crate::disk::read_dir(dir.join(MODELS)).into_iter().flatten().flatten() {
                let folder = model.path();
                if crate::disk::is_file(folder.join(Part::Tracks.file_name())) {
                    add(&model.file_name().to_string_lossy(), folder_size(&folder).0);
                }
            }
        }
        by_model.into_values().collect()
    }

    /// Deletes the model's folder in each recording folder; the old reviews' part files for "".
    fn remove_reviews(&self, model: &str) -> io::Result<usize> {
        let mut removed = 0;
        for entry in crate::disk::read_dir(&self.folders.recordings).into_iter().flatten().flatten() {
            let dir = entry.path();
            if model.is_empty() {
                let parts: Vec<PathBuf> = PARTS.iter().map(|part| dir.join(part.file_name())).collect();
                if parts.iter().any(crate::disk::is_file) {
                    removed += 1;
                }
                for part in parts.iter().filter(|part| crate::disk::is_file(part)) {
                    crate::disk::remove_file(part)?;
                }
            } else if crate::disk::is_dir(dir.join(MODELS).join(model)) {
                crate::disk::remove_dir_all(dir.join(MODELS).join(model))?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// The mark files in each recording folder.
    fn marks_size(&self) -> u64 {
        let folders = crate::disk::read_dir(&self.folders.recordings).into_iter().flatten().flatten();
        folders.map(|entry| MARKS.iter().map(|mark| file_len(&entry.path().join(mark.file_name()))).sum::<u64>()).sum()
    }

    /// The cut-off folder.
    fn cutoff_size(&self) -> u64 {
        folder_size(&self.folders.cutoff).0
    }
}

impl Store for Files {
    /// The item's file; a missing file is None, any other error an error.
    fn read(&self, item: Item<'_>) -> io::Result<Option<Vec<u8>>> {
        match crate::disk::read(self.path(item)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Whether the item's file is there.
    fn has(&self, item: Item<'_>) -> bool {
        crate::disk::is_file(self.path(item))
    }

    /// The item's file's time of change.
    fn changed(&self, item: Item<'_>) -> Option<f64> {
        crate::disk::metadata(self.path(item)).ok().and_then(|metadata| metadata.modified())
    }

    /// Writes the item's file, its folder made when missing; a file Python keeps as text gets Windows' line ends.
    fn write(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(item);
        make_parent(&path)?;
        if python_text(item) {
            crate::disk::write(path, crate::pyjson::newlines(bytes))
        } else {
            crate::disk::write(path, bytes)
        }
    }

    /// Adds to the end of the item's file, as `write` writes it.
    fn append(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(item);
        make_parent(&path)?;
        if python_text(item) {
            crate::disk::append(path, &crate::pyjson::newlines(bytes))
        } else {
            crate::disk::append(path, bytes)
        }
    }

    /// Deletes the item's file; a missing file is an error.
    fn remove(&self, item: Item<'_>) -> io::Result<()> {
        crate::disk::remove_file(self.path(item))
    }

    /// The item's file's path.
    fn name(&self, item: Item<'_>) -> String {
        self.path(item).display().to_string()
    }

    /// Whether the recording's folder holds an old tracks.json, or a model folder with one.
    fn reviewed(&self, id: &str) -> bool {
        self.has(Item::ReviewPart(id, &ReviewBy::Old, Part::Tracks)) || self.model_folders(id).next().is_some()
    }

    /// The names of the recording's model folders that hold a tracks.json, in the order the folder lists them.
    fn models(&self, id: &str) -> Vec<String> {
        let name = |folder: PathBuf| folder.file_name().map(|name| name.to_string_lossy().into_owned());
        self.model_folders(id).map(|folder| name(folder).unwrap_or_default()).collect()
    }

    /// Reads the last bytes of the old tracks.json: without a "detector" key the hand-written detector made it.
    fn old_review(&self, id: &str) -> Option<Option<String>> {
        let tracks = self.path(Item::ReviewPart(id, &ReviewBy::Old, Part::Tracks));
        if !crate::disk::is_file(&tracks) {
            return None;
        }
        let mut end = Vec::new();
        if let Ok(mut file) = crate::disk::File::open(&tracks) {
            let size = file.metadata().map_or(0, |metadata| metadata.len());
            let start = SeekFrom::Start(size.saturating_sub(OLD_TRACKS_TAIL_BYTES));
            let _ = file.seek(start).and_then(|_| file.read_to_end(&mut end));
        }
        let named = end.windows(DETECTOR_KEY.len()).any(|window| window == DETECTOR_KEY);
        Some((!named).then(|| "hand".to_string()))
    }

    /// The names of the entries in the recordings folder.
    fn kept(&self) -> HashSet<String> {
        let entries = crate::disk::read_dir(&self.folders.recordings).into_iter().flatten().flatten();
        entries.map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()
    }

    /// Reads areas.json and exclude.json from each recording folder that has both.
    fn labelled(&self, skip: &BTreeSet<String>) -> Vec<(String, Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in crate::disk::read_dir(&self.folders.recordings).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.is_dir() || skip.contains(&name) {
                continue;
            }
            let dir = entry.path();
            let read = |mark: Mark| crate::disk::read(dir.join(mark.file_name())).ok();
            if let (Some(found), Some(saved)) = (read(Mark::FoundAreas), read(Mark::SavedAreas)) {
                out.push((name, found, saved));
            }
        }
        out
    }
}
