//! The SQL the data folder's database runs (database.rs), behind one interface (`Sql`) so its statements are written
//! once (docs/storage-design.md): natively SQLite built into the exe through rusqlite (`Sqlite`), in the browser build
//! SQLite's own WebAssembly in the page. In: a statement and its values. Out: the rows it gives, as values.

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
