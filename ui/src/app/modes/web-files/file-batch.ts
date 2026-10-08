/**
 * Files in one body, both ways between the page and the review service (service/src/batch.rs):
 * each [u32 path length][path, UTF-8][f64 time of change, seconds since 1970][u32 length][bytes],
 * little-endian. In: files, or a body the service wrote. Out: a body, or its files.
 */

/** Milliseconds in a second: a File's time is in ms, the batch's in seconds. */
const MS_PER_SECOND = 1000;
/** A u32's bytes. */
const U32_BYTES = 4;
/** An f64's bytes. */
const F64_BYTES = 8;

/** A file to put in a batch: its path there and the file. */
export interface BatchFile {
  /** Its path in the batch. */
  path: string;
  /** The file. */
  file: File;
}

/**
 * A batch as the service reads it: per file [u32 path length][path, UTF-8][f64 time in seconds]
 * [u32 length][bytes], little-endian.
 */
export async function encodeBatch(files: readonly BatchFile[]): Promise<Uint8Array> {
  const parts = await Promise.all(
    files.map(async ({ path, file }) => ({
      path: new TextEncoder().encode(path),
      modified: file.lastModified / MS_PER_SECOND,
      bytes: new Uint8Array(await file.arrayBuffer()),
    })),
  );
  const size = parts.reduce(
    (sum, part) => sum + U32_BYTES * 2 + F64_BYTES + part.path.length + part.bytes.length,
    0,
  );
  const out = new Uint8Array(size);
  const view = new DataView(out.buffer);
  let at = 0;
  for (const part of parts) {
    view.setUint32(at, part.path.length, true);
    out.set(part.path, at + U32_BYTES);
    at += U32_BYTES + part.path.length;
    view.setFloat64(at, part.modified, true);
    view.setUint32(at + F64_BYTES, part.bytes.length, true);
    out.set(part.bytes, at + F64_BYTES + U32_BYTES);
    at += F64_BYTES + U32_BYTES + part.bytes.length;
  }
  return out;
}

/** One file of a batch read back: its path, time (seconds since 1970) and bytes. */
export interface BatchEntry {
  /** Its path. */
  path: string;
  /** Its time of change in seconds since 1970. */
  modified: number;
  /** Its bytes (a view into the batch). */
  bytes: Uint8Array<ArrayBuffer>;
}

/** A batch's files (service/src/batch.rs writes them as encodeBatch does). */
export function readBatch(batch: Uint8Array<ArrayBuffer>): BatchEntry[] {
  const view = new DataView(batch.buffer, batch.byteOffset, batch.byteLength);
  const out: BatchEntry[] = [];
  let at = 0;
  while (at < batch.length) {
    const pathLen = view.getUint32(at, true);
    const path = new TextDecoder().decode(batch.subarray(at + U32_BYTES, at + U32_BYTES + pathLen));
    at += U32_BYTES + pathLen;
    const modified = view.getFloat64(at, true);
    const len = view.getUint32(at + F64_BYTES, true);
    at += F64_BYTES + U32_BYTES;
    out.push({ path, modified, bytes: batch.subarray(at, at + len) });
    at += len;
  }
  return out;
}
