/// <reference lib="webworker" />
/**
 * The review service's database in browser mode (service/src/database.rs, docs/storage-design.md):
 * SQLite's own WebAssembly in the service's worker, on the browser's private file system through
 * its pool of sync access handles, so the service's `host_sql` calls (service/src/sql.rs) are
 * answered at once. One tab holds it at a time: a tab that opens takes it (a Web Lock, stolen),
 * and the tab that held it lets it go and says so. In: SQLite's WebAssembly's address and the
 * service's statements in its binary form. Out: their answers in that form.
 */
import sqlite3InitModule, {
  type Database,
  type PreparedStatement,
  type SAHPoolUtil,
  type SqlValue,
  type Sqlite3Static,
} from '@sqlite.org/sqlite-wasm';

/** The Web Lock the tab that holds the database keeps. */
const LOCK = 'aimview-database';
/** The pool's folder in the private file system (one pool, one database). */
const POOL_FOLDER = '.aimview-sqlite';
/** The pool's VFS name. */
const POOL_NAME = 'aimview-sahpool';
/** The database's file in the pool. */
const DATABASE_FILE = '/aimview.sqlite3';
/** How long a tab taking the database waits for the tab before it to let go, in tries. */
const TAKE_TRIES = 50;
/** The wait between two tries, in milliseconds. */
const TAKE_WAIT_MS = 100;
/** `host_sql`'s ops (service/src/sql.rs: HostOp). */
const BATCH = 0;
/** One statement with values; answers how many rows it changed. */
const EXECUTE = 1;
/** One query with values; answers its rows. */
const QUERY = 2;
/** A value's tag in the binary form: no value. */
const NULL_TAG = 0;
/** A whole number: 8 bytes, little-endian. */
const INTEGER_TAG = 1;
/** A floating-point number: 8 bytes, little-endian. */
const REAL_TAG = 2;
/** Text: a u32 length, then UTF-8. */
const TEXT_TAG = 3;
/** Bytes: a u32 length, then the bytes. */
const BLOB_TAG = 4;
/** A u32's bytes. */
const U32_BYTES = 4;
/** An i64's or f64's bytes. */
const EIGHT_BYTES = 8;
/** An answer's code: done. Any other code is an error, its bytes saying why. */
const DONE = 0;
/** An answer's code: failed. */
const FAILED = 1;

/** One `host_sql` answer: its code and its bytes. */
export interface SqlAnswer {
  /** 0 done, else failed. */
  code: number;
  /** The rows, the count of rows changed, or why it failed. */
  bytes: Uint8Array;
}

/** SQLite's start options this module passes (its types leave them out). */
interface InitOptions {
  /** Where SQLite's WebAssembly is, for the file it asks for. */
  locateFile: (file: string) => string;
}

/** The pool's options this module passes, one of which its types leave out. */
interface PoolOptions {
  /** The pool's VFS name. */
  name: string;
  /** The pool's folder. */
  directory: string;
  /** Try again after a failed start (another tab still held the pool). */
  forceReinitIfPreviouslyFailed: boolean;
}

/** Bytes built up front to back, growing as they go. */
class ByteWriter {
  /** The bytes so far, with room after them. */
  private buffer = new Uint8Array(1024);
  /** How many bytes are written. */
  private length = 0;

  /** Room for `more` bytes; gives where they go. */
  private room(more: number): number {
    if (this.length + more > this.buffer.length) {
      const grown = new Uint8Array(Math.max(this.buffer.length * 2, this.length + more));
      grown.set(this.buffer.subarray(0, this.length));
      this.buffer = grown;
    }
    const at = this.length;
    this.length += more;
    return at;
  }

  // Each write makes its room before it reads `buffer`: `room` can replace the buffer with a bigger
  // one, and a write that took the old one first would be lost.

  /** A byte. */
  byte(value: number): void {
    const at = this.room(1);
    this.buffer[at] = value;
  }

  /** A little-endian u32. */
  u32(value: number): void {
    const at = this.room(U32_BYTES);
    new DataView(this.buffer.buffer).setUint32(at, value, true);
  }

  /** A little-endian i64. */
  i64(value: bigint): void {
    const at = this.room(EIGHT_BYTES);
    new DataView(this.buffer.buffer).setBigInt64(at, value, true);
  }

  /** A little-endian f64. */
  f64(value: number): void {
    const at = this.room(EIGHT_BYTES);
    new DataView(this.buffer.buffer).setFloat64(at, value, true);
  }

  /** Bytes after their u32 length. */
  sized(bytes: Uint8Array): void {
    this.u32(bytes.length);
    const at = this.room(bytes.length);
    this.buffer.set(bytes, at);
  }

  /** The bytes written. */
  done(): Uint8Array {
    return this.buffer.slice(0, this.length);
  }
}

/** The values a statement takes, from the binary form (service/src/sql.rs: encode_values). */
export function decodeValues(bytes: Uint8Array): SqlValue[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const out: SqlValue[] = [];
  let at = 0;
  const sized = (): Uint8Array => {
    const len = view.getUint32(at, true);
    const start = at + U32_BYTES;
    at = start + len;
    return bytes.subarray(start, at);
  };
  while (at < bytes.length) {
    const tag = bytes[at++];
    if (tag === NULL_TAG) out.push(null);
    else if (tag === INTEGER_TAG) {
      out.push(view.getBigInt64(at, true));
      at += EIGHT_BYTES;
    } else if (tag === REAL_TAG) {
      out.push(view.getFloat64(at, true));
      at += EIGHT_BYTES;
    } else if (tag === TEXT_TAG) out.push(new TextDecoder().decode(sized()));
    else if (tag === BLOB_TAG) out.push(sized().slice());
    else throw new Error(`An unknown value tag ${tag}`);
  }
  return out;
}

/** A value in the binary form; whole numbers as integers, other numbers as reals. */
function writeValue(out: ByteWriter, value: SqlValue): void {
  if (value === null) out.byte(NULL_TAG);
  else if (typeof value === 'bigint') {
    out.byte(INTEGER_TAG);
    out.i64(value);
  } else if (typeof value === 'number') {
    out.byte(REAL_TAG);
    out.f64(value);
  } else if (typeof value === 'string') {
    out.byte(TEXT_TAG);
    out.sized(new TextEncoder().encode(value));
  } else {
    out.byte(BLOB_TAG);
    out.sized(value instanceof Uint8Array ? value : new Uint8Array(value as ArrayBuffer));
  }
}

/** Rows in the binary form: [u32 columns][u32 rows], then each row's values. */
export function encodeRows(columns: number, rows: SqlValue[][]): Uint8Array {
  const out = new ByteWriter();
  out.u32(columns);
  out.u32(rows.length);
  for (const row of rows) for (const value of row) writeValue(out, value);
  return out.done();
}

/** A u32 count in the binary form. */
function count(value: number): Uint8Array {
  const out = new ByteWriter();
  out.u32(value);
  return out.done();
}

/** Waits `ms` milliseconds. */
function pause(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * The service's database: open while this tab holds it. Its statements are prepared once and
 * kept.
 */
export class ServiceDatabase {
  /** The statements prepared so far, by their SQL. */
  private readonly prepared = new Map<string, PreparedStatement>();

  /** Keeps the open database, its pool and SQLite. */
  private constructor(
    private readonly sqlite3: Sqlite3Static,
    private readonly pool: SAHPoolUtil,
    private readonly db: Database,
  ) {}

  /**
   * Opens the database for this tab, taking it from a tab that holds it (which then lets it go).
   * `lost` is called when another tab takes it later. Rejects when SQLite cannot start, or the
   * tab before does not let go in time.
   */
  static open(wasmUrl: string, lost: () => void): Promise<ServiceDatabase> {
    return new Promise((resolve, reject) => {
      let opened = false;
      const take = async (): Promise<void> => {
        const database = await ServiceDatabase.take(wasmUrl);
        opened = true;
        resolve(database);
        // held until another tab steals it
        await new Promise<never>(() => undefined);
      };
      if (!navigator.locks) {
        take().catch(reject);
        return;
      }
      navigator.locks.request(LOCK, { steal: true }, take).catch((error: unknown) => {
        if (opened) lost();
        else reject(error instanceof Error ? error : new Error(String(error)));
      });
    });
  }

  /** Starts SQLite and opens the database, trying again while another tab still holds the pool. */
  private static async take(wasmUrl: string): Promise<ServiceDatabase> {
    const init = sqlite3InitModule as unknown as (options: InitOptions) => Promise<Sqlite3Static>;
    const sqlite3 = await init({ locateFile: () => wasmUrl });
    const options: PoolOptions = {
      name: POOL_NAME,
      directory: POOL_FOLDER,
      forceReinitIfPreviouslyFailed: true,
    };
    for (let tries = 1; ; tries++) {
      try {
        const pool = await sqlite3.installOpfsSAHPoolVfs(options);
        return new ServiceDatabase(sqlite3, pool, new pool.OpfsSAHPoolDb(DATABASE_FILE));
      } catch (error) {
        if (tries >= TAKE_TRIES) throw error;
        await pause(TAKE_WAIT_MS);
      }
    }
  }

  /** One `host_sql` call: the op, the statement and its values in the binary form. */
  run(op: number, sql: string, values: Uint8Array): SqlAnswer {
    try {
      if (op === BATCH) {
        this.db.exec(sql);
        return { code: DONE, bytes: new Uint8Array() };
      }
      const statement = this.statement(sql);
      try {
        const bound = decodeValues(values);
        if (bound.length) statement.bind(bound);
        if (op === EXECUTE) {
          while (statement.step()) continue;
          return { code: DONE, bytes: count(this.db.changes()) };
        }
        if (op !== QUERY) throw new Error(`An unknown op ${op}`);
        return { code: DONE, bytes: encodeRows(statement.columnCount, this.rows(statement)) };
      } finally {
        statement.reset(true);
      }
    } catch (error) {
      const why = error instanceof Error ? error.message : String(error);
      return { code: FAILED, bytes: new TextEncoder().encode(why) };
    }
  }

  /** The statement for `sql`, prepared the first time. */
  private statement(sql: string): PreparedStatement {
    let statement = this.prepared.get(sql);
    if (!statement) {
      statement = this.db.prepare(sql);
      this.prepared.set(sql, statement);
    }
    return statement;
  }

  /** Every row the statement gives, each column as SQLite keeps it (whole numbers as BigInt). */
  private rows(statement: PreparedStatement): SqlValue[][] {
    const { capi } = this.sqlite3;
    const rows: SqlValue[][] = [];
    while (statement.step()) {
      const row: SqlValue[] = [];
      for (let column = 0; column < statement.columnCount; column++) {
        const type = capi.sqlite3_column_type(statement, column);
        if (type === capi.SQLITE_INTEGER)
          row.push(BigInt(statement.get(column) as number | bigint));
        else if (type === capi.SQLITE_FLOAT) row.push(statement.getFloat(column));
        else if (type === capi.SQLITE_TEXT) row.push(statement.getString(column));
        else if (type === capi.SQLITE_BLOB) row.push(statement.getBlob(column) ?? new Uint8Array());
        else row.push(null);
      }
      rows.push(row);
    }
    return rows;
  }

  /** Closes the database and lets the pool's files go, for another tab to take them. */
  close(): void {
    for (const statement of this.prepared.values()) statement.finalize();
    this.prepared.clear();
    this.db.close();
    this.pool.pauseVfs();
  }
}
