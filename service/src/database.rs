//! The store as one SQLite database in the data folder (docs/storage-design.md): what store.rs's `Files` keeps as files,
//! kept as rows instead, the same bytes in and out, so every answer is the same. The reviews' parts are kept
//! gzip-compressed. On its first opening it imports what the data folder's files hold, in one transaction, and leaves
//! the files where they are. The SQL runs through sql.rs, natively SQLite through rusqlite. In: the library's items and
//! their bytes, and on first opening the data folder's files. Out: the same bytes, and what is kept for each recording.

use std::collections::{BTreeSet, HashSet};
use std::io::{self, Read, Write};
use std::sync::{Mutex, MutexGuard, PoisonError};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::config::Folders;
use crate::library::slug;
use crate::sql::{Sql, SqlValue};
use crate::store::{DETECTOR_KEY, Files, IdList, Item, MODELS, Mark, OLD_TRACKS_TAIL_BYTES, Part, ReviewBy, Store};

/// The database's file in the data folder.
pub const DATABASE_FILE: &str = "aimview.sqlite3";

/// The layout of the tables below, kept in the database's `user_version`; 0 is a new database.
const SCHEMA_VERSION: i64 = 1;

/// The tables. `library`: the library's own items by their file names (settings, area kinds and examples, the lists,
/// the cut-off's rows). `marks`: each recording's marks by its folder name (its slug) and the mark's file name.
/// `reviews`: each review's parts, gzip-compressed, by slug, model ("" for the old review) and the part's file name.
/// `cutoff_crops`: the cut-off labels' crops by their file (train/<name>.npz). `changed` is seconds since 1970.
const SCHEMA: &str = "
CREATE TABLE library (name TEXT PRIMARY KEY, bytes BLOB NOT NULL, changed REAL NOT NULL);
CREATE TABLE marks (recording TEXT NOT NULL, mark TEXT NOT NULL, bytes BLOB NOT NULL, changed REAL NOT NULL,
  PRIMARY KEY (recording, mark));
CREATE TABLE reviews (recording TEXT NOT NULL, model TEXT NOT NULL, part TEXT NOT NULL, bytes BLOB NOT NULL,
  changed REAL NOT NULL, PRIMARY KEY (recording, model, part));
CREATE TABLE cutoff_crops (file TEXT PRIMARY KEY, bytes BLOB NOT NULL, changed REAL NOT NULL);
";

/// The model column of the old review (python/retired/server.py's, kept before reviews were kept per model): no
/// model's folder can have an empty name.
const OLD_REVIEW_MODEL: &str = "";

/// The library's own items, the ones not kept per recording: what the import copies from the data folder's files.
const LIBRARY_ITEMS: [Item<'static>; 8] = [
    Item::Settings,
    Item::AreaKinds,
    Item::AreaExamples,
    Item::UploadAreas,
    Item::Ids(IdList::NotAimTrainer),
    Item::Ids(IdList::LabelSkipped),
    Item::Ids(IdList::FaintSkipped),
    Item::CutoffRows,
];

/// Every mark a recording can have.
const MARKS: [Mark; 6] =
    [Mark::RunWindow, Mark::StatsPick, Mark::Cutoff, Mark::SavedAreas, Mark::FoundAreas, Mark::FoundMaps];

/// Every part a review can have.
const PARTS: [Part; 4] = [Part::Tracks, Part::Readings, Part::Hud, Part::Kills];

/// The folder in the cut-off folder that holds the labels' crops.
const CROPS_FOLDER: &str = "train";

/// Where an item's row is: its table, its key's columns, and the key's values.
struct Row {
    /// The table.
    table: &'static str,
    /// The key's columns, in the order of `key`.
    columns: &'static [&'static str],
    /// The key's values.
    key: Vec<SqlValue>,
    /// Whether the bytes are kept gzip-compressed (a review's parts).
    compressed: bool,
}

/// Text as a statement's value.
fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_string())
}

impl Row {
    /// The row that keeps `item`.
    fn of(item: Item<'_>) -> Row {
        match item {
            Item::Mark(id, mark) => Row {
                table: "marks",
                columns: &["recording", "mark"],
                key: vec![text(&slug(id)), text(mark.file_name())],
                compressed: false,
            },
            Item::ReviewPart(id, by, part) => {
                let model = match by {
                    ReviewBy::Model(model) => model.as_str(),
                    ReviewBy::Old => OLD_REVIEW_MODEL,
                };
                let key = vec![text(&slug(id)), text(model), text(part.file_name())];
                Row { table: "reviews", columns: &["recording", "model", "part"], key, compressed: true }
            }
            Item::CutoffCrop(file) => {
                Row { table: "cutoff_crops", columns: &["file"], key: vec![text(file)], compressed: false }
            }
            _ => Row { table: "library", columns: &["name"], key: vec![text(item.file_name())], compressed: false },
        }
    }

    /// The condition that picks the row: each key column equal to its value, numbered from 1.
    fn condition(&self) -> String {
        let each = self.columns.iter().enumerate().map(|(i, column)| format!("{column} = ?{}", i + 1));
        each.collect::<Vec<_>>().join(" AND ")
    }
}

/// Bytes gzip-compressed.
fn compress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes)?;
    encoder.finish()
}

/// Gzip-compressed bytes as they were.
fn decompress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    GzDecoder::new(bytes).read_to_end(&mut out)?;
    Ok(out)
}


/// A row's column as bytes; empty when it is not bytes or text.
fn bytes_of(value: SqlValue) -> Vec<u8> {
    match value {
        SqlValue::Blob(bytes) => bytes,
        SqlValue::Text(text) => text.into_bytes(),
        _ => Vec::new(),
    }
}

/// A row's column as text; empty when it is not text.
fn text_of(value: SqlValue) -> String {
    match value {
        SqlValue::Text(text) => text,
        _ => String::new(),
    }
}

/// A name's place in a folder listing on Windows (NTFS sorts by each UTF-16 unit in upper case), so the database lists
/// recordings and models in the order `Files` read them from their folders.
fn listing_key(name: &str) -> Vec<u16> {
    let upper = |character: char| {
        let mut upper = character.to_uppercase();
        match (upper.next(), upper.next()) {
            (Some(single), None) => single,
            _ => character,
        }
    };
    name.chars().map(upper).collect::<String>().encode_utf16().collect()
}

/// The store as one SQLite database (see the module's comment).
pub struct Database {
    /// The connection; one caller at a time.
    sql: Mutex<Box<dyn Sql>>,
    /// How messages name the database: its file.
    place: String,
}

impl Database {
    /// The database file `DATABASE_FILE` in the data folder whose layout's folders are `folders`, made and filled
    /// from those folders' files on its first opening.
    #[cfg(feature = "native")]
    pub fn open_file(data: &std::path::Path, folders: &Folders) -> io::Result<Database> {
        let path = data.join(DATABASE_FILE);
        let sql = crate::sql::Sqlite::open(&path)?;
        Database::open(Box::new(sql), path.display().to_string(), folders)
    }

    /// The database `sql` reaches, named `place` in messages; on its first opening the tables are made and what
    /// `folders` hold is imported, all in one transaction. Fails on a database a newer version made.
    pub fn open(mut sql: Box<dyn Sql>, place: String, folders: &Folders) -> io::Result<Database> {
        let version =
            match sql.query("PRAGMA user_version", &[])?.into_iter().next().and_then(|row| row.into_iter().next()) {
                Some(SqlValue::Integer(version)) => version,
                _ => 0,
            };
        if version > SCHEMA_VERSION {
            return Err(io::Error::other(format!("{place}: made by a newer Aim View (layout {version})")));
        }
        if version == 0 {
            sql.batch("BEGIN IMMEDIATE")?;
            let made = sql.batch(SCHEMA).and_then(|()| import(&mut *sql, folders));
            let done = made.and_then(|()| sql.batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}; COMMIT")));
            if let Err(error) = done {
                let _ = sql.batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(Database { sql: Mutex::new(sql), place })
    }

    /// The connection, for one caller.
    fn sql(&self) -> MutexGuard<'_, Box<dyn Sql>> {
        self.sql.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The bytes kept in `row` as they were given (decompressed); None when there is no row.
    fn read_row(sql: &mut dyn Sql, row: &Row) -> io::Result<Option<Vec<u8>>> {
        let statement = format!("SELECT bytes FROM {} WHERE {}", row.table, row.condition());
        let Some(found) = sql.query(&statement, &row.key)?.into_iter().next() else {
            return Ok(None);
        };
        let kept = found.into_iter().next().map(bytes_of).unwrap_or_default();
        if row.compressed { decompress(&kept).map(Some) } else { Ok(Some(kept)) }
    }

    /// Keeps `bytes` in `row` (compressed when the row is), with `changed` as its time of change.
    fn write_row(sql: &mut dyn Sql, row: &Row, bytes: &[u8], changed: f64) -> io::Result<()> {
        let kept = if row.compressed { compress(bytes)? } else { bytes.to_vec() };
        let columns = row.columns.join(", ");
        let places: Vec<String> = (1..=row.columns.len() + 2).map(|i| format!("?{i}")).collect();
        let statement = format!(
            "INSERT INTO {table} ({columns}, bytes, changed) VALUES ({places}) ON CONFLICT ({columns}) \
             DO UPDATE SET bytes = excluded.bytes, changed = excluded.changed",
            table = row.table,
            places = places.join(", "),
        );
        let mut values = row.key.clone();
        values.extend([SqlValue::Blob(kept), SqlValue::Real(changed)]);
        sql.execute(&statement, &values).map(|_| ())
    }

    /// The text of an item Python keeps as text, as `Files` writes it (Windows' line ends), so its bytes read back the
    /// same from either store.
    fn as_kept(item: Item<'_>, bytes: &[u8]) -> Vec<u8> {
        if crate::store::python_text(item) { crate::pyjson::newlines(bytes) } else { bytes.to_vec() }
    }

    /// Each recording's slug, with the bytes of its found and saved areas when it has both.
    fn areas_pairs(&self) -> io::Result<Vec<Vec<SqlValue>>> {
        let statement = "SELECT found.recording, found.bytes, saved.bytes FROM marks AS found JOIN marks AS saved \
                         ON saved.recording = found.recording AND found.mark = ?1 AND saved.mark = ?2";
        let values = [text(Mark::FoundAreas.file_name()), text(Mark::SavedAreas.file_name())];
        self.sql().query(statement, &values)
    }
}

/// Copies what the data folder's files hold into the database's tables (`Files`' layout, read from disk): the
/// library's items, each recording folder's marks and reviews, and the cut-off labels' crops, each with its file's
/// time of change.
fn import(sql: &mut dyn Sql, folders: &Folders) -> io::Result<()> {
    let files = Files::new(folders.clone());
    let changed = |path: &std::path::Path| {
        crate::disk::metadata(path).ok().and_then(|metadata| metadata.modified()).unwrap_or(0.0)
    };
    let mut copy = |row: Row, path: std::path::PathBuf| -> io::Result<()> {
        match crate::disk::read(&path) {
            Ok(bytes) => Database::write_row(sql, &row, &bytes, changed(&path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    };
    for item in LIBRARY_ITEMS {
        copy(Row::of(item), files.path(item))?;
    }
    for folder in crate::disk::read_dir(&folders.recordings).into_iter().flatten().flatten() {
        if !folder.is_dir() {
            continue;
        }
        let recording = folder.file_name().to_string_lossy().into_owned();
        let dir = folder.path();
        for mark in MARKS {
            let row = Row {
                table: "marks",
                columns: &["recording", "mark"],
                key: vec![text(&recording), text(mark.file_name())],
                compressed: false,
            };
            copy(row, dir.join(mark.file_name()))?;
        }
        let mut reviews = vec![(OLD_REVIEW_MODEL.to_string(), dir.clone())];
        for model in crate::disk::read_dir(dir.join(MODELS)).into_iter().flatten().flatten() {
            if model.is_dir() {
                reviews.push((model.file_name().to_string_lossy().into_owned(), model.path()));
            }
        }
        for (model, review) in reviews {
            for part in PARTS {
                let key = vec![text(&recording), text(&model), text(part.file_name())];
                let row = Row { table: "reviews", columns: &["recording", "model", "part"], key, compressed: true };
                copy(row, review.join(part.file_name()))?;
            }
        }
    }
    for crop in crate::disk::read_dir(folders.cutoff.join(CROPS_FOLDER)).into_iter().flatten().flatten() {
        let file = format!("{CROPS_FOLDER}/{}", crop.file_name().to_string_lossy());
        copy(Row::of(Item::CutoffCrop(&file)), crop.path())?;
    }
    Ok(())
}

impl Store for Database {
    /// The item's row, as it was given.
    fn read(&self, item: Item<'_>) -> io::Result<Option<Vec<u8>>> {
        Database::read_row(&mut **self.sql(), &Row::of(item))
    }

    /// Whether the item has a row.
    fn has(&self, item: Item<'_>) -> bool {
        let row = Row::of(item);
        let statement = format!("SELECT 1 FROM {} WHERE {}", row.table, row.condition());
        self.sql().query(&statement, &row.key).is_ok_and(|rows| !rows.is_empty())
    }

    /// The row's time of change.
    fn changed(&self, item: Item<'_>) -> Option<f64> {
        let row = Row::of(item);
        let statement = format!("SELECT changed FROM {} WHERE {}", row.table, row.condition());
        match self.sql().query(&statement, &row.key).ok()?.into_iter().next()?.into_iter().next()? {
            SqlValue::Real(seconds) => Some(seconds),
            SqlValue::Integer(seconds) => Some(seconds as f64),
            _ => None,
        }
    }

    /// Keeps the bytes in the item's row, now its time of change; text Python keeps gets Windows' line ends.
    fn write(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()> {
        Database::write_row(&mut **self.sql(), &Row::of(item), &Database::as_kept(item, bytes), crate::disk::now())
    }

    /// Reads the row and writes it back with the bytes after it, while no one else uses the connection.
    fn append(&self, item: Item<'_>, bytes: &[u8]) -> io::Result<()> {
        let row = Row::of(item);
        let mut sql = self.sql();
        let mut kept = Database::read_row(&mut **sql, &row)?.unwrap_or_default();
        kept.extend_from_slice(&Database::as_kept(item, bytes));
        Database::write_row(&mut **sql, &row, &kept, crate::disk::now())
    }

    /// Deletes the item's row; a missing row is an error, as a missing file is.
    fn remove(&self, item: Item<'_>) -> io::Result<()> {
        let row = Row::of(item);
        let statement = format!("DELETE FROM {} WHERE {}", row.table, row.condition());
        match self.sql().execute(&statement, &row.key)? {
            0 => Err(io::Error::new(io::ErrorKind::NotFound, format!("{}: nothing kept", self.name(item)))),
            _ => Ok(()),
        }
    }

    /// The database's file and the item's key.
    fn name(&self, item: Item<'_>) -> String {
        let key: Vec<String> = Row::of(item).key.into_iter().map(text_of).filter(|part| !part.is_empty()).collect();
        format!("{} ({})", self.place, key.join("/"))
    }

    /// Whether any review of the recording has its tracks.
    fn reviewed(&self, id: &str) -> bool {
        let statement = "SELECT 1 FROM reviews WHERE recording = ?1 AND part = ?2 LIMIT 1";
        let values = [text(&slug(id)), text(Part::Tracks.file_name())];
        self.sql().query(statement, &values).is_ok_and(|rows| !rows.is_empty())
    }

    /// The models whose review of the recording has its tracks, in a folder listing's order.
    fn models(&self, id: &str) -> Vec<String> {
        let statement = "SELECT model FROM reviews WHERE recording = ?1 AND part = ?2 AND model <> ?3";
        let values = [text(&slug(id)), text(Part::Tracks.file_name()), text(OLD_REVIEW_MODEL)];
        let rows = self.sql().query(statement, &values).unwrap_or_default();
        let mut models: Vec<String> = rows.into_iter().filter_map(|row| row.into_iter().next()).map(text_of).collect();
        models.sort_by_cached_key(|model| listing_key(model));
        models
    }

    /// The old review's tracks' last bytes: without a "detector" key the hand-written detector made it.
    fn old_review(&self, id: &str) -> Option<Option<String>> {
        let tracks = self.read(Item::ReviewPart(id, &ReviewBy::Old, Part::Tracks)).ok()??;
        let tail = usize::try_from(OLD_TRACKS_TAIL_BYTES).unwrap_or(usize::MAX);
        let end = &tracks[tracks.len().saturating_sub(tail)..];
        let named = end.windows(DETECTOR_KEY.len()).any(|window| window == DETECTOR_KEY);
        Some((!named).then(|| "hand".to_string()))
    }

    /// The slugs with a mark or a review.
    fn kept(&self) -> HashSet<String> {
        let statement = "SELECT recording FROM marks UNION SELECT recording FROM reviews";
        let rows = self.sql().query(statement, &[]).unwrap_or_default();
        rows.into_iter().filter_map(|row| row.into_iter().next()).map(text_of).collect()
    }

    /// The recordings with both found and saved areas, in a folder listing's order.
    fn labelled(&self, skip: &BTreeSet<String>) -> Vec<(String, Vec<u8>, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();
        for row in self.areas_pairs().unwrap_or_default() {
            let mut columns = row.into_iter();
            let (Some(recording), Some(found), Some(saved)) = (columns.next(), columns.next(), columns.next()) else {
                continue;
            };
            let recording = text_of(recording);
            if !skip.contains(&recording) {
                out.push((recording, bytes_of(found), bytes_of(saved)));
            }
        }
        out.sort_by_cached_key(|(recording, ..)| listing_key(recording));
        out
    }
}

/// The database store against the files it imports.
#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use crate::config::Layout;
    use std::path::Path;

    /// A recording's id, and its folder name (its slug).
    const ID: &str = "Scenario/Scenario - 100 - 2026.10.01-12.00.00.mp4";
    /// The slug of `ID`.
    const SLUG: &str = "Scenario_-_100_-_2026.10.01-12.00.00";

    /// Writes `bytes` to `path`, its folder made.
    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// A data folder in the app's layout with a library item, marks, a model's review, an old review by the
    /// hand-written detector, the cut-off's rows and a crop.
    fn data_folder(name: &str) -> std::path::PathBuf {
        let data = std::env::temp_dir().join(format!("aimview-database-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let recording = data.join("reviews").join(SLUG);
        put(&data.join("settings.json"), br#"{"model":"m1"}"#);
        put(&data.join("area_examples.jsonl"), b"{\"a\":1}\r\n");
        put(&recording.join("exclude.json"), b"[]\r\n");
        put(&recording.join("areas.json"), b"{\"found\":[]}\r\n");
        put(&recording.join("tracks.json"), b"{\"frames\":[]}");
        put(&recording.join("models").join("m1").join("tracks.json"), b"{\"frames\":[1],\"detector\":\"m1\"}");
        put(&recording.join("models").join("m1").join("hud.json"), b"{}");
        put(&data.join("cutoff").join("checked.jsonl"), b"{\"file\":\"train/a.npz\"}\r\n");
        put(&data.join("cutoff").join("train").join("a.npz"), b"PK\x03\x04");
        data
    }

    /// The database in `data`, opened (and on the first opening, filled from the files).
    fn open(data: &Path) -> Database {
        Database::open_file(data, &Layout::App.folders(data)).unwrap()
    }

    /// The first opening imports every item byte for byte, and answers what `Files` answers about each recording.
    #[test]
    fn imports_what_the_files_hold() {
        let data = data_folder("import");
        let files = Files::new(Layout::App.folders(&data));
        let database = open(&data);
        let model = ReviewBy::Model("m1".into());
        let items = [
            Item::Settings,
            Item::AreaExamples,
            Item::Mark(ID, Mark::SavedAreas),
            Item::Mark(ID, Mark::FoundAreas),
            Item::ReviewPart(ID, &ReviewBy::Old, Part::Tracks),
            Item::ReviewPart(ID, &model, Part::Tracks),
            Item::ReviewPart(ID, &model, Part::Hud),
            Item::CutoffRows,
            Item::CutoffCrop("train/a.npz"),
            Item::Mark(ID, Mark::RunWindow),
        ];
        for item in items {
            assert_eq!(database.read(item).unwrap(), files.read(item).unwrap(), "{item:?}");
            assert_eq!(database.has(item), files.has(item), "{item:?}");
        }
        assert_eq!(database.models(ID), files.models(ID));
        assert_eq!(database.old_review(ID), Some(Some("hand".to_string())));
        assert!(database.reviewed(ID));
        assert_eq!(database.kept(), files.kept());
        assert_eq!(database.labelled(&BTreeSet::new()), files.labelled(&BTreeSet::new()));
        assert!(database.labelled(&BTreeSet::from([SLUG.to_string()])).is_empty());
        let _ = std::fs::remove_dir_all(&data);
    }

    /// Writes, appends and removes keep the bytes as `Files` would, and last when the database is opened again
    /// (without importing the files a second time).
    #[test]
    fn keeps_changes_across_openings() {
        let data = data_folder("changes");
        let database = open(&data);
        let run = Item::Mark(ID, Mark::RunWindow);
        database.write(run, b"{\"start\":5}").unwrap();
        database.append(Item::AreaExamples, b"{\"b\":2}\n").unwrap();
        database.write(Item::Mark(ID, Mark::Cutoff), b"{}\n").unwrap();
        database.remove(Item::Mark(ID, Mark::FoundAreas)).unwrap();
        assert!(database.remove(Item::Mark(ID, Mark::FoundAreas)).is_err());
        let model = ReviewBy::Model("m2".into());
        let big = vec![b'7'; 100_000];
        database.write(Item::ReviewPart(ID, &model, Part::Tracks), &big).unwrap();
        drop(database);
        std::fs::remove_file(data.join("settings.json")).unwrap();
        let again = open(&data);
        let line_end: &[u8] = if cfg!(windows) { b"\r\n" } else { b"\n" };
        assert_eq!(again.read(run).unwrap().unwrap(), b"{\"start\":5}");
        assert_eq!(
            again.read(Item::AreaExamples).unwrap().unwrap(),
            [b"{\"a\":1}\r\n{\"b\":2}".as_slice(), line_end].concat()
        );
        assert_eq!(again.read(Item::Mark(ID, Mark::Cutoff)).unwrap().unwrap(), [b"{}".as_slice(), line_end].concat());
        assert!(!again.has(Item::Mark(ID, Mark::FoundAreas)));
        assert!(again.labelled(&BTreeSet::new()).is_empty());
        assert_eq!(again.read(Item::ReviewPart(ID, &model, Part::Tracks)).unwrap().unwrap(), big);
        assert_eq!(again.models(ID), ["m1", "m2"]);
        assert!(again.has(Item::Settings), "the settings came from the database, not the removed file");
        assert!(again.changed(run).is_some_and(|seconds| seconds > 0.0));
        let _ = std::fs::remove_dir_all(&data);
    }

    /// Names sort as a Windows folder lists them (checked against one): letters without case, "_" after them, then
    /// letters outside ASCII.
    #[test]
    fn lists_as_a_windows_folder() {
        let mut names = vec!["b", "_a", "A", "a_b", "ab", "Zz", "\u{e9}", "\u{e4}_x"];
        names.sort_by_cached_key(|name| listing_key(name));
        assert_eq!(names, ["A", "ab", "a_b", "b", "Zz", "_a", "\u{e4}_x", "\u{e9}"]);
    }
}
