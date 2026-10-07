//! What the library keeps, through one interface (`Store`), so where it is kept can change (docs/storage-design.md):
//! today the files in the data folder (`Files`, laid out as config.rs's layout says), later one SQLite database. The
//! library formats each thing (JSON as python/server.py writes it, .npz as NumPy does); a store keeps the bytes it is
//! given and gives the same bytes back. The videos (uploads), the mouse logs (the desktop app's logger writes them) and
//! the crop-check folders stay files outside it. In: the library's items and their bytes. Out: the same bytes, and
//! what is kept for each recording.

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
    /// The faint-target cut-off (faint.rs).
    Cutoff,
    /// The areas the user saved for it (areas.rs).
    SavedAreas,
    /// What the area finder found in it, and the finder's maps (finder.rs).
    FoundAreas,
    FoundMaps,
}

/// A part of a review: its tracks, the video's readings (the camera's turn, the countdown), what the HUD read, and the
/// check of the kills the video alone gives.
#[derive(Clone, Copy, Debug)]
pub enum Part {
    Tracks,
    Readings,
    Hud,
    Kills,
}

/// Which of a recording's reviews: a model's, or the one python/server.py kept before reviews were kept per model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewBy {
    Model(String),
    Old,
}

/// One thing the library keeps.
#[derive(Clone, Copy, Debug)]
pub enum Item<'a> {
    /// What the user set (library/settings.rs).
    Settings,
    /// The area types, and the area finder's examples (one JSON line each; areas.rs).
    AreaKinds,
    AreaExamples,
    /// The areas last saved for an added recording (areas.rs).
    UploadAreas,
    Ids(IdList),
    /// A recording's mark, by its id.
    Mark(&'a str, Mark),
    /// A part of one of a recording's reviews, by its id.
    ReviewPart(&'a str, &'a ReviewBy, Part),
    /// The detector labels of submitted cut-offs (faint.rs): their rows (one JSON line each), and a crop by its row's
    /// file (train/<name>.npz).
    CutoffRows,
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

/// Where the library keeps what it keeps (see the module's comment).
pub trait Store: Send + Sync {
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
}

/// The bytes at the end of an old tracks.json (python/server.py's, kept in the recording's own folder) read for the
/// detector's name.
const OLD_TRACKS_TAIL_BYTES: u64 = 200;
/// The key an old tracks.json ends with when a model made it (the hand-written detector's has none).
const DETECTOR_KEY: &[u8] = b"\"detector\"";
/// The folder of a recording's reviews by model, one folder each.
const MODELS: &str = "models";

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
    folders: Folders,
}

impl Files {
    pub fn new(folders: Folders) -> Files {
        Files { folders }
    }

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
            Item::Settings | Item::AreaKinds | Item::AreaExamples | Item::UploadAreas | Item::Ids(_) => {
                self.folders.files.join(name)
            }
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
fn python_text(item: Item<'_>) -> bool {
    match item {
        Item::AreaKinds | Item::AreaExamples | Item::UploadAreas | Item::Ids(_) | Item::CutoffRows => true,
        Item::Mark(_, mark) => matches!(mark, Mark::Cutoff | Mark::SavedAreas | Mark::FoundAreas),
        Item::Settings | Item::ReviewPart(..) | Item::CutoffCrop(_) => false,
    }
}

/// The folder a file goes in, made when missing.
fn make_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(dir) => crate::disk::create_dir_all(dir),
        None => Ok(()),
    }
}

impl Store for Files {
    fn read(&self, item: Item<'_>) -> io::Result<Option<Vec<u8>>> {
        match crate::disk::read(self.path(item)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn has(&self, item: Item<'_>) -> bool {
        crate::disk::is_file(self.path(item))
    }

    fn changed(&self, item: Item<'_>) -> Option<f64> {
        crate::disk::metadata(self.path(item)).ok().and_then(|metadata| metadata.modified())
    }

    fn write(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(item);
        make_parent(&path)?;
        if python_text(item) {
            crate::disk::write(path, crate::pyjson::newlines(bytes))
        } else {
            crate::disk::write(path, bytes)
        }
    }

    fn append(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(item);
        make_parent(&path)?;
        if python_text(item) {
            crate::disk::append(path, &crate::pyjson::newlines(bytes))
        } else {
            crate::disk::append(path, bytes)
        }
    }

    fn remove(&self, item: Item<'_>) -> io::Result<()> {
        crate::disk::remove_file(self.path(item))
    }

    fn name(&self, item: Item<'_>) -> String {
        self.path(item).display().to_string()
    }

    fn reviewed(&self, id: &str) -> bool {
        self.has(Item::ReviewPart(id, &ReviewBy::Old, Part::Tracks)) || self.model_folders(id).next().is_some()
    }

    fn models(&self, id: &str) -> Vec<String> {
        let name = |folder: PathBuf| folder.file_name().map(|name| name.to_string_lossy().into_owned());
        self.model_folders(id).map(|folder| name(folder).unwrap_or_default()).collect()
    }

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

    fn kept(&self) -> HashSet<String> {
        let entries = crate::disk::read_dir(&self.folders.recordings).into_iter().flatten().flatten();
        entries.map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()
    }

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
