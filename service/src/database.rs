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
use crate::store::{
    DETECTOR_KEY, Files, IdList, Item, Kovaak, MARKS, MODELS, Mark, OLD_TRACKS_TAIL_BYTES, PARTS, Part, ReviewBy,
    ReviewSize, ScenarioRow, StatsRow, StatsRun, Store,
};

/// The database's file in the data folder.
pub const DATABASE_FILE: &str = "aimview.sqlite3";

/// The layout of the tables below, kept in the database's `user_version`; 0 is a new database. 1 had the first four
/// tables in the plural (marks, reviews, cutoff_crops) and no KovaaK tables.
const SCHEMA_VERSION: i64 = 2;

/// The tables of what the library keeps. `library`: the library's own items by their file names (settings, area
/// kinds and examples, the lists, the cut-off's rows). `mark`: each recording's marks by its folder name (its slug)
/// and the mark's file name. `review`: each review's parts, gzip-compressed, by slug, model ("" for the old review)
/// and the part's file name. `cutoff_crop`: the cut-off labels' crops by their file (train/<name>.npz). `changed` is
/// seconds since 1970.
const LIBRARY_TABLES: &str = "
CREATE TABLE library (name TEXT PRIMARY KEY, bytes BLOB NOT NULL, changed REAL NOT NULL);
CREATE TABLE mark (recording TEXT NOT NULL, mark TEXT NOT NULL, bytes BLOB NOT NULL, changed REAL NOT NULL,
  PRIMARY KEY (recording, mark));
CREATE TABLE review (recording TEXT NOT NULL, model TEXT NOT NULL, part TEXT NOT NULL, bytes BLOB NOT NULL,
  changed REAL NOT NULL, PRIMARY KEY (recording, model, part));
CREATE TABLE cutoff_crop (file TEXT PRIMARY KEY, bytes BLOB NOT NULL, changed REAL NOT NULL);
";

/// The tables of KovaaK's files in the browser build (store.rs: `Kovaak`; empty natively, which reads the folders).
/// `stats_file`: each stats file's size and time of change, its run (no score: none), and its whole text
/// gzip-compressed only once a recording used it. `scenario`: each scenario file's facts as JSON, by its path in
/// /kovaak (scenarios/... or workshop/...).
const KOVAAK_TABLES: &str = "
CREATE TABLE stats_file (name TEXT PRIMARY KEY, size INTEGER NOT NULL, modified REAL NOT NULL, score REAL,
  kills REAL, accuracy REAL, csv BLOB);
CREATE TABLE scenario (path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified REAL NOT NULL, facts TEXT NOT NULL);
";

/// Layout 1 to 2: the tables in the singular, and KovaaK's tables added.
const FROM_LAYOUT_1: &str = "
ALTER TABLE marks RENAME TO mark;
ALTER TABLE reviews RENAME TO review;
ALTER TABLE cutoff_crops RENAME TO cutoff_crop;
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
                table: "mark",
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
                Row { table: "review", columns: &["recording", "model", "part"], key, compressed: true }
            }
            Item::CutoffCrop(file) => {
                Row { table: "cutoff_crop", columns: &["file"], key: vec![text(file)], compressed: false }
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
    /// `folders` hold is imported, and one an older version made is brought to this layout, each in one transaction.
    /// Fails on a database a newer version made.
    pub fn open(mut sql: Box<dyn Sql>, place: String, folders: &Folders) -> io::Result<Database> {
        let version =
            match sql.query("PRAGMA user_version", &[])?.into_iter().next().and_then(|row| row.into_iter().next()) {
                Some(SqlValue::Integer(version)) => version,
                _ => 0,
            };
        let made = match version {
            0 => in_transaction(&mut *sql, |sql| {
                sql.batch(LIBRARY_TABLES)?;
                sql.batch(KOVAAK_TABLES)?;
                import(sql, folders)
            }),
            1 => in_transaction(&mut *sql, |sql| sql.batch(&format!("{FROM_LAYOUT_1}{KOVAAK_TABLES}"))),
            SCHEMA_VERSION => Ok(()),
            _ => Err(io::Error::other(format!("{place}: made by a newer Aim View (layout {version})"))),
        };
        made?;
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
        let statement = "SELECT found.recording, found.bytes, saved.bytes FROM mark AS found JOIN mark AS saved \
                         ON saved.recording = found.recording AND found.mark = ?1 AND saved.mark = ?2";
        let values = [text(Mark::FoundAreas.file_name()), text(Mark::SavedAreas.file_name())];
        self.sql().query(statement, &values)
    }
}

/// Runs `work` in one transaction that ends by setting the layout to this version; nothing is kept when it fails.
fn in_transaction(sql: &mut dyn Sql, work: impl FnOnce(&mut dyn Sql) -> io::Result<()>) -> io::Result<()> {
    sql.batch("BEGIN IMMEDIATE")?;
    let done = work(sql).and_then(|()| sql.batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}; COMMIT")));
    if done.is_err() {
        let _ = sql.batch("ROLLBACK");
    }
    done
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
                table: "mark",
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
                let row = Row { table: "review", columns: &["recording", "model", "part"], key, compressed: true };
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
        let statement = "SELECT 1 FROM review WHERE recording = ?1 AND part = ?2 LIMIT 1";
        let values = [text(&slug(id)), text(Part::Tracks.file_name())];
        self.sql().query(statement, &values).is_ok_and(|rows| !rows.is_empty())
    }

    /// The models whose review of the recording has its tracks, in a folder listing's order.
    fn models(&self, id: &str) -> Vec<String> {
        let statement = "SELECT model FROM review WHERE recording = ?1 AND part = ?2 AND model <> ?3";
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
        let statement = "SELECT recording FROM mark UNION SELECT recording FROM review";
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

    /// The `review` table by model.
    fn review_sizes(&self) -> Vec<ReviewSize> {
        let statement = "SELECT model, COUNT(DISTINCT recording), SUM(length(bytes)) FROM review GROUP BY model";
        let rows = self.sql().query(statement, &[]).unwrap_or_default();
        rows.into_iter()
            .map(|row| ReviewSize {
                model: text_of(row[0].clone()),
                recordings: usize::try_from(count_of(&row[1])).unwrap_or(0),
                bytes: count_of(&row[2]),
            })
            .collect()
    }

    /// Deletes the model's rows.
    fn remove_reviews(&self, model: &str) -> io::Result<usize> {
        let mut sql = self.sql();
        let counted = sql.query("SELECT COUNT(DISTINCT recording) FROM review WHERE model = ?1", &[text(model)])?;
        sql.execute("DELETE FROM review WHERE model = ?1", &[text(model)])?;
        Ok(counted.first().map_or(0, |row| usize::try_from(count_of(&row[0])).unwrap_or(0)))
    }

    /// The `mark` table's bytes.
    fn marks_size(&self) -> u64 {
        self.sum("SELECT SUM(length(bytes)) FROM mark", &[])
    }

    /// The `cutoff_crop` table's bytes and the rows' item.
    fn cutoff_size(&self) -> u64 {
        let rows = [text(Item::CutoffRows.file_name())];
        self.sum("SELECT SUM(length(bytes)) FROM cutoff_crop", &[])
            + self.sum("SELECT SUM(length(bytes)) FROM library WHERE name = ?1", &rows)
    }

    /// Its pages' bytes.
    fn file_size(&self) -> Option<u64> {
        let pages = self.sum("PRAGMA page_count", &[]);
        Some(pages * self.sum("PRAGMA page_size", &[]))
    }

    /// VACUUM: the database file is written again without its free pages.
    fn compact(&self) -> io::Result<()> {
        self.sql().batch("VACUUM")
    }

    /// The browser build keeps KovaaK's files here; natively the library reads their folders.
    fn kovaak(&self) -> Option<&dyn Kovaak> {
        if cfg!(feature = "native") { None } else { Some(self) }
    }
}

/// A number or none, as a statement's value.
fn real(value: Option<f64>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::Real)
}

/// A row's column as a number; None when it is null.
fn real_of(value: &SqlValue) -> Option<f64> {
    match value {
        SqlValue::Real(number) => Some(*number),
        SqlValue::Integer(number) => Some(*number as f64),
        _ => None,
    }
}

/// A row's column as a count; 0 when it is not a whole number (a sum is a real when it overflows).
fn count_of(value: &SqlValue) -> u64 {
    match value {
        SqlValue::Integer(number) => u64::try_from(*number).unwrap_or(0),
        SqlValue::Real(number) if *number >= 0.0 => *number as u64,
        _ => 0,
    }
}

impl Database {
    /// A query's one number (a sum or a count); 0 when it gives none.
    fn sum(&self, statement: &str, values: &[SqlValue]) -> u64 {
        let rows = self.sql().query(statement, values).unwrap_or_default();
        rows.first().and_then(|row| row.first()).map_or(0, count_of)
    }

    /// Runs `statement` once per row's values, all in one transaction, while no one else uses the connection.
    fn insert_all(&self, statement: &str, rows: impl Iterator<Item = Vec<SqlValue>>) -> io::Result<()> {
        let mut sql = self.sql();
        sql.batch("BEGIN IMMEDIATE")?;
        let mut done = Ok(());
        for values in rows {
            done = sql.execute(statement, &values).map(|_| ());
            if done.is_err() {
                break;
            }
        }
        match done {
            Ok(()) => sql.batch("COMMIT"),
            Err(error) => {
                let _ = sql.batch("ROLLBACK");
                Err(error)
            }
        }
    }
}

impl Kovaak for Database {
    /// The `stats_file` table, its text left out.
    fn stats_files(&self) -> io::Result<Vec<StatsRow>> {
        let rows = self.sql().query("SELECT name, size, modified, score, kills, accuracy FROM stats_file", &[])?;
        Ok(rows
            .into_iter()
            .map(|row| StatsRow {
                name: text_of(row[0].clone()),
                size: count_of(&row[1]),
                modified: real_of(&row[2]).unwrap_or(0.0),
                run: real_of(&row[3]).map(|score| StatsRun {
                    score,
                    kills: real_of(&row[4]),
                    accuracy: real_of(&row[5]),
                }),
            })
            .collect())
    }

    /// An upsert per row in one transaction; the kept text stays only when the size and time are the same.
    fn add_stats_files(&self, rows: &[StatsRow]) -> io::Result<()> {
        let statement = "INSERT INTO stats_file (name, size, modified, score, kills, accuracy) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (name) DO UPDATE SET \
                         csv = CASE WHEN size = excluded.size AND modified = excluded.modified THEN csv END, \
                         size = excluded.size, modified = excluded.modified, score = excluded.score, \
                         kills = excluded.kills, accuracy = excluded.accuracy";
        self.insert_all(
            statement,
            rows.iter().map(|row| {
                let run = row.run.as_ref();
                vec![
                    text(&row.name),
                    SqlValue::Integer(i64::try_from(row.size).unwrap_or(i64::MAX)),
                    SqlValue::Real(row.modified),
                    real(run.map(|run| run.score)),
                    real(run.and_then(|run| run.kills)),
                    real(run.and_then(|run| run.accuracy)),
                ]
            }),
        )
    }

    /// The kept text, decompressed.
    fn stats_csv(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let rows = self.sql().query("SELECT csv FROM stats_file WHERE name = ?1", &[text(name)])?;
        match rows.into_iter().next().and_then(|row| row.into_iter().next()) {
            Some(SqlValue::Blob(bytes)) => decompress(&bytes).map(Some),
            _ => Ok(None),
        }
    }

    /// The text compressed into the file's row.
    fn keep_stats_csv(&self, name: &str, csv: &[u8]) -> io::Result<()> {
        let statement = "UPDATE stats_file SET csv = ?1 WHERE name = ?2";
        self.sql().execute(statement, &[SqlValue::Blob(compress(csv)?), text(name)]).map(|_| ())
    }

    /// The two tables' rows and their bytes.
    fn kovaak_size(&self) -> io::Result<(usize, usize, u64)> {
        let count = |table: &str| usize::try_from(self.sum(&format!("SELECT COUNT(*) FROM {table}"), &[])).unwrap_or(0);
        let bytes = self.sum("SELECT SUM(length(name) + IFNULL(length(csv), 0) + 48) FROM stats_file", &[])
            + self.sum("SELECT SUM(length(path) + length(facts) + 16) FROM scenario", &[]);
        Ok((count("stats_file"), count("scenario"), bytes))
    }

    /// Deletes both tables' rows.
    fn clear_kovaak(&self) -> io::Result<()> {
        self.sql().batch("DELETE FROM stats_file; DELETE FROM scenario")
    }

    /// The `scenario` table, the user's scenarios first; a row whose facts do not read is left out.
    fn scenarios(&self) -> io::Result<Vec<ScenarioRow>> {
        let statement = "SELECT path, size, modified, facts FROM scenario \
                         ORDER BY path LIKE 'workshop/%', path";
        let rows = self.sql().query(statement, &[])?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(ScenarioRow {
                    path: text_of(row[0].clone()),
                    size: count_of(&row[1]),
                    modified: real_of(&row[2]).unwrap_or(0.0),
                    facts: serde_json::from_str(&text_of(row[3].clone())).ok()?,
                })
            })
            .collect())
    }

    /// An upsert per row in one transaction.
    fn add_scenarios(&self, rows: &[ScenarioRow]) -> io::Result<()> {
        let statement = "INSERT INTO scenario (path, size, modified, facts) VALUES (?1, ?2, ?3, ?4) \
                         ON CONFLICT (path) DO UPDATE SET size = excluded.size, modified = excluded.modified, \
                         facts = excluded.facts";
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            let facts = serde_json::to_string(&row.facts).map_err(io::Error::other)?;
            values.push(vec![
                text(&row.path),
                SqlValue::Integer(i64::try_from(row.size).unwrap_or(i64::MAX)),
                SqlValue::Real(row.modified),
                SqlValue::Text(facts),
            ]);
        }
        self.insert_all(statement, values.into_iter())
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

    /// A database layout 1 made (the tables in the plural) opens with what it kept, its tables renamed and KovaaK's
    /// added, without importing the files again.
    #[test]
    fn layout_1_is_brought_up_to_date() {
        let data = data_folder("layout1");
        let path = data.join(DATABASE_FILE);
        let mut old = crate::sql::Sqlite::open(&path).unwrap();
        old.batch(
            "CREATE TABLE library (name TEXT PRIMARY KEY, bytes BLOB NOT NULL, changed REAL NOT NULL);
             CREATE TABLE marks (recording TEXT NOT NULL, mark TEXT NOT NULL, bytes BLOB NOT NULL,
               changed REAL NOT NULL, PRIMARY KEY (recording, mark));
             CREATE TABLE reviews (recording TEXT NOT NULL, model TEXT NOT NULL, part TEXT NOT NULL,
               bytes BLOB NOT NULL, changed REAL NOT NULL, PRIMARY KEY (recording, model, part));
             CREATE TABLE cutoff_crops (file TEXT PRIMARY KEY, bytes BLOB NOT NULL, changed REAL NOT NULL);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        let mark = [text(SLUG), text("run.json"), SqlValue::Blob(b"{\"start\":1}".to_vec()), SqlValue::Real(1.0)];
        old.execute("INSERT INTO marks VALUES (?1, ?2, ?3, ?4)", &mark).unwrap();
        drop(old);
        let database = open(&data);
        assert_eq!(database.read(Item::Mark(ID, Mark::RunWindow)).unwrap().unwrap(), b"{\"start\":1}");
        assert!(!database.has(Item::Settings), "the files were not imported a second time");
        assert!(database.stats_files().unwrap().is_empty());
        drop(database);
        let _ = std::fs::remove_dir_all(&data);
    }

    /// KovaaK's tables: rows are replaced by name, a stats file's kept text goes when the file changes, and the
    /// user's scenarios come before the workshop's.
    #[test]
    fn keeps_kovaak_files_by_name() {
        let data = data_folder("kovaak");
        let database = open(&data);
        let run = StatsRun { score: 100.0, kills: Some(10.0), accuracy: Some(0.5) };
        let row =
            |name: &str, size: u64, run: Option<StatsRun>| StatsRow { name: name.into(), size, modified: 7.0, run };
        database.add_stats_files(&[row("a.csv", 10, Some(run.clone())), row("b.csv", 20, None)]).unwrap();
        database.keep_stats_csv("a.csv", b"Score:,100\n").unwrap();
        database.add_stats_files(&[row("a.csv", 10, Some(run.clone()))]).unwrap();
        assert_eq!(database.stats_csv("a.csv").unwrap().unwrap(), b"Score:,100\n", "the same file keeps its text");
        database.add_stats_files(&[row("a.csv", 11, Some(run.clone()))]).unwrap();
        assert!(database.stats_csv("a.csv").unwrap().is_none(), "a changed file loses its text");
        let mut rows = database.stats_files().unwrap();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(rows, [row("a.csv", 11, Some(run)), row("b.csv", 20, None)]);
        let facts = |limit: f64| aimview::scenario::Facts {
            kind: aimview::scenario::Kind::Static,
            limit: Some(limit),
            targets: None,
            reload: None,
            hitbox: None,
        };
        let scenario =
            |path: &str, limit: f64| ScenarioRow { path: path.into(), size: 1, modified: 2.0, facts: facts(limit) };
        database.add_scenarios(&[scenario("workshop/1/x.sce", 30.0), scenario("scenarios/x.sce", 60.0)]).unwrap();
        let paths: Vec<String> = database.scenarios().unwrap().into_iter().map(|row| row.path).collect();
        assert_eq!(paths, ["scenarios/x.sce", "workshop/1/x.sce"]);
        drop(database);
        let _ = std::fs::remove_dir_all(&data);
    }
}
