//! The SQL the data folder's database runs (database.rs), behind one interface (`Sql`) so its statements are written
//! once (docs/storage-design.md): natively SQLite built into the exe through rusqlite (`Sqlite`), in the browser build
//! SQLite's own WebAssembly in the page (`HostSql`, the `host_sql` import, in a binary form both sides read: tagged
//! values, rows as a column and a row count before them). In: a statement and its values. Out: the rows it gives.

use std::io;

/// A value a statement takes or a row gives (SQLite's five kinds).
#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    /// No value.
    Null,
    /// A whole number.
    Integer(i64),
    /// A floating-point number.
    Real(f64),
    /// UTF-8 text.
    Text(String),
    /// Bytes.
    Blob(Vec<u8>),
}

/// One connection to a database: one caller at a time (database.rs holds it behind a mutex).
pub trait Sql: Send {
    /// Runs statements that take no values and give no rows (the schema, BEGIN, COMMIT).
    fn batch(&mut self, statements: &str) -> io::Result<()>;
    /// Runs one statement with its values; returns how many rows it changed.
    fn execute(&mut self, statement: &str, values: &[SqlValue]) -> io::Result<usize>;
    /// Runs one query with its values; returns its rows, each a value per column.
    fn query(&mut self, statement: &str, values: &[SqlValue]) -> io::Result<Vec<Vec<SqlValue>>>;
}

/// A database file through rusqlite, in WAL mode: readers don't wait on a writer, and a crash mid-write loses nothing
/// committed.
#[cfg(feature = "native")]
pub struct Sqlite {
    /// The open connection.
    connection: rusqlite::Connection,
}

/// How long a statement waits for another process's write to finish before it fails, in milliseconds.
#[cfg(feature = "native")]
const BUSY_WAIT_MS: u64 = 5_000;

/// A rusqlite error as an I/O error, its message kept.
#[cfg(feature = "native")]
fn io_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(error)
}

#[cfg(feature = "native")]
impl Sqlite {
    /// Opens the database file at `path`, made when missing.
    pub fn open(path: &std::path::Path) -> io::Result<Sqlite> {
        let connection = rusqlite::Connection::open(path).map_err(io_error)?;
        connection.busy_timeout(std::time::Duration::from_millis(BUSY_WAIT_MS)).map_err(io_error)?;
        connection.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(())).map_err(io_error)?;
        connection.pragma_update(None, "synchronous", "NORMAL").map_err(io_error)?;
        Ok(Sqlite { connection })
    }
}

/// A value as rusqlite takes it.
#[cfg(feature = "native")]
fn to_sqlite(value: &SqlValue) -> rusqlite::types::Value {
    use rusqlite::types::Value;
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(number) => Value::Integer(*number),
        SqlValue::Real(number) => Value::Real(*number),
        SqlValue::Text(text) => Value::Text(text.clone()),
        SqlValue::Blob(bytes) => Value::Blob(bytes.clone()),
    }
}

/// A column of a row rusqlite gives, as a value.
#[cfg(feature = "native")]
fn from_sqlite(value: rusqlite::types::ValueRef<'_>) -> SqlValue {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Null => SqlValue::Null,
        ValueRef::Integer(number) => SqlValue::Integer(number),
        ValueRef::Real(number) => SqlValue::Real(number),
        ValueRef::Text(text) => SqlValue::Text(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Blob(bytes) => SqlValue::Blob(bytes.to_vec()),
    }
}

#[cfg(feature = "native")]
impl Sql for Sqlite {
    /// rusqlite's execute_batch.
    fn batch(&mut self, statements: &str) -> io::Result<()> {
        self.connection.execute_batch(statements).map_err(io_error)
    }

    /// The statement prepared once and kept (rusqlite's statement cache).
    fn execute(&mut self, statement: &str, values: &[SqlValue]) -> io::Result<usize> {
        let mut prepared = self.connection.prepare_cached(statement).map_err(io_error)?;
        prepared.execute(rusqlite::params_from_iter(values.iter().map(to_sqlite))).map_err(io_error)
    }

    /// The query prepared once and kept, its rows read whole.
    fn query(&mut self, statement: &str, values: &[SqlValue]) -> io::Result<Vec<Vec<SqlValue>>> {
        let mut prepared = self.connection.prepare_cached(statement).map_err(io_error)?;
        let columns = prepared.column_count();
        let mut rows = prepared.query(rusqlite::params_from_iter(values.iter().map(to_sqlite))).map_err(io_error)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(io_error)? {
            let mut values = Vec::with_capacity(columns);
            for column in 0..columns {
                values.push(from_sqlite(row.get_ref(column).map_err(io_error)?));
            }
            out.push(values);
        }
        Ok(out)
    }
}

/// A value's tag in the host's binary form: no value.
const NULL_TAG: u8 = 0;
/// The tag of a whole number (then 8 bytes, little-endian).
const INTEGER_TAG: u8 = 1;
/// The tag of a floating-point number (then 8 bytes, little-endian).
const REAL_TAG: u8 = 2;
/// The tag of text (then its length as a little-endian u32, then its UTF-8 bytes).
const TEXT_TAG: u8 = 3;
/// The tag of bytes (then their length as a little-endian u32, then the bytes).
const BLOB_TAG: u8 = 4;

/// Values in the host's binary form (`host_sql`): each a tag, then its bytes (see the tags above).
pub fn encode_values(values: &[SqlValue]) -> Vec<u8> {
    let mut out = Vec::new();
    let with_length = |out: &mut Vec<u8>, tag: u8, bytes: &[u8]| {
        out.push(tag);
        out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(bytes);
    };
    for value in values {
        match value {
            SqlValue::Null => out.push(NULL_TAG),
            SqlValue::Integer(number) => {
                out.push(INTEGER_TAG);
                out.extend_from_slice(&number.to_le_bytes());
            }
            SqlValue::Real(number) => {
                out.push(REAL_TAG);
                out.extend_from_slice(&number.to_le_bytes());
            }
            SqlValue::Text(text) => with_length(&mut out, TEXT_TAG, text.as_bytes()),
            SqlValue::Blob(bytes) => with_length(&mut out, BLOB_TAG, bytes),
        }
    }
    out
}

/// Reads the host's binary form (see `encode_values`), from `at` on.
struct Reader<'a> {
    /// The bytes.
    bytes: &'a [u8],
    /// Where the next read starts.
    at: usize,
}

impl Reader<'_> {
    /// The next `len` bytes; an error when fewer are left.
    fn take(&mut self, len: usize) -> io::Result<&[u8]> {
        let end = self.at.checked_add(len).filter(|end| *end <= self.bytes.len());
        let end = end.ok_or_else(|| io::Error::other("the page's rows end early"))?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    /// The next 8 bytes.
    fn eight(&mut self) -> io::Result<[u8; 8]> {
        let mut out = [0; 8];
        out.copy_from_slice(self.take(8)?);
        Ok(out)
    }

    /// The next little-endian u32.
    fn length(&mut self) -> io::Result<usize> {
        let mut out = [0; 4];
        out.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(out) as usize)
    }

    /// The next value.
    fn value(&mut self) -> io::Result<SqlValue> {
        Ok(match self.take(1)?[0] {
            NULL_TAG => SqlValue::Null,
            INTEGER_TAG => SqlValue::Integer(i64::from_le_bytes(self.eight()?)),
            REAL_TAG => SqlValue::Real(f64::from_le_bytes(self.eight()?)),
            TEXT_TAG => {
                let len = self.length()?;
                SqlValue::Text(String::from_utf8_lossy(self.take(len)?).into_owned())
            }
            BLOB_TAG => {
                let len = self.length()?;
                SqlValue::Blob(self.take(len)?.to_vec())
            }
            tag => return Err(io::Error::other(format!("the page's rows hold an unknown value tag {tag}"))),
        })
    }
}

/// A query's rows in the host's binary form: [u32 columns][u32 rows], then each row's values in turn.
pub fn decode_rows(bytes: &[u8]) -> io::Result<Vec<Vec<SqlValue>>> {
    let mut reader = Reader { bytes, at: 0 };
    let (columns, rows) = (reader.length()?, reader.length()?);
    let mut out = Vec::with_capacity(rows.min(bytes.len()));
    for _ in 0..rows {
        out.push((0..columns).map(|_| reader.value()).collect::<io::Result<Vec<_>>>()?);
    }
    Ok(out)
}

/// The page's database, through the `host_sql` import: SQLite's own WebAssembly in the service's worker, on the
/// browser's private file system through its pool of sync access handles, so each call returns at once (no Asyncify
/// wait; ui/src/app/modes/service/service-database.ts answers it).
#[cfg(not(feature = "native"))]
pub struct HostSql;

/// What `host_sql` is asked to do.
#[cfg(not(feature = "native"))]
#[derive(Clone, Copy)]
enum HostOp {
    /// Statements that take no values (`Sql::batch`); answers no bytes.
    Batch = 0,
    /// One statement with its values; answers how many rows it changed, a little-endian u32.
    Execute = 1,
    /// One query with its values; answers its rows (`decode_rows`).
    Query = 2,
}

#[cfg(not(feature = "native"))]
#[link(wasm_import_module = "host")]
unsafe extern "C" {
    /// A statement (UTF-8) and its values (`encode_values`) for the page's database: a result block the host
    /// reserved with the module's `alloc`, [u32 code][u32 len][len bytes] (code 0 done, else the bytes say why).
    /// Synchronous: not one of Asyncify's imports.
    fn host_sql(op: u32, sql_ptr: *const u8, sql_len: usize, values_ptr: *const u8, values_len: usize) -> *mut u8;
}

#[cfg(not(feature = "native"))]
impl HostSql {
    /// One call to the page's database: its answer's bytes, or why it failed.
    fn call(op: HostOp, statement: &str, values: &[SqlValue]) -> io::Result<Vec<u8>> {
        let values = encode_values(values);
        // SAFETY: the host reads the two byte ranges it is given and answers with a block it reserved with `alloc`
        let block = unsafe { host_sql(op as u32, statement.as_ptr(), statement.len(), values.as_ptr(), values.len()) };
        // SAFETY: the block is the host's answer, as `take_block` reads it
        match unsafe { crate::disk::take_block(block) }? {
            (0, bytes) => Ok(bytes),
            (_, why) => Err(io::Error::other(String::from_utf8_lossy(&why).into_owned())),
        }
    }
}

#[cfg(not(feature = "native"))]
impl Sql for HostSql {
    /// The page runs the statements.
    fn batch(&mut self, statements: &str) -> io::Result<()> {
        HostSql::call(HostOp::Batch, statements, &[]).map(|_| ())
    }

    /// The page runs the statement and says how many rows it changed.
    fn execute(&mut self, statement: &str, values: &[SqlValue]) -> io::Result<usize> {
        let bytes = HostSql::call(HostOp::Execute, statement, values)?;
        let mut changed = [0; 4];
        changed.copy_from_slice(bytes.get(..4).ok_or_else(|| io::Error::other("the page gave no count"))?);
        Ok(u32::from_le_bytes(changed) as usize)
    }

    /// The page runs the query and gives its rows.
    fn query(&mut self, statement: &str, values: &[SqlValue]) -> io::Result<Vec<Vec<SqlValue>>> {
        decode_rows(&HostSql::call(HostOp::Query, statement, values)?)
    }
}

/// The host's binary form, both ways.
#[cfg(test)]
mod tests {
    use super::*;

    /// Values encoded as rows of one column read back the same, every kind and an empty text and blob included.
    #[test]
    fn rows_read_back_the_same() {
        let values = vec![
            SqlValue::Null,
            SqlValue::Integer(-7_000_000_000),
            SqlValue::Real(1_791_337_350.941_831_8),
            SqlValue::Text("Valorant \u{ff5c} #2".into()),
            SqlValue::Text(String::new()),
            SqlValue::Blob(vec![0, 255, 1]),
            SqlValue::Blob(Vec::new()),
        ];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(values.len()).unwrap().to_le_bytes());
        bytes.extend(encode_values(&values));
        let rows = decode_rows(&bytes).unwrap();
        assert_eq!(rows, values.into_iter().map(|value| vec![value]).collect::<Vec<_>>());
        assert!(decode_rows(&bytes[..bytes.len() - 1]).is_err(), "a short answer is an error");
    }
}
