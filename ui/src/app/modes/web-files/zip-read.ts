/**
 * Reads a zip from a Blob (a file the user chose) without reading it whole: the central directory
 * from its end (zip64 too), then each entry's bytes as a slice of the Blob, so a large stored video
 * stays on disk until it is sent on. In: a zip as a Blob. Out: its entries, each read when asked.
 */

/** One entry of a zip. */
export interface ZipItem {
  /** Its path inside the zip. */
  path: string;
  /** Its size before compression. */
  size: number;
  /** Its bytes, uncompressed (a slice of the zip when it is stored). */
  blob(): Promise<Blob>;
}

/** The central directory header's signature. */
const CENTRAL_HEADER = 0x02014b50;
/** The end of central directory record's signature. */
const DIRECTORY_END = 0x06054b50;
/** The zip64 end record's signature. */
const ZIP64_END = 0x06064b50;
/** The zip64 locator's signature. */
const ZIP64_LOCATOR = 0x07064b50;
/** The zip64 extra field's id. */
const ZIP64_EXTRA = 0x0001;
/** A 32-bit field that says "see the zip64 extra field". */
const IN_ZIP64 = 0xffffffff;
/** The end record's size without its comment. */
const END_BYTES = 22;
/** The zip64 locator's size, just before the end record. */
const LOCATOR_BYTES = 20;
/** The longest comment an end record can have, so how far from the end to look for it. */
const MAX_COMMENT = 0xffff;
/** A local header's size before its name and extra field. */
const LOCAL_BYTES = 30;
/** A central header's size before its name, extra field and comment. */
const CENTRAL_BYTES = 46;
/** Stored as it is. */
const STORED = 0;
/** Deflated. */
const DEFLATED = 8;

/** A little-endian 64-bit number (up to 2^53). */
function u64(view: DataView, at: number): number {
  return view.getUint32(at, true) + view.getUint32(at + 4, true) * 2 ** 32;
}

/** A part of the Blob as a DataView. */
async function viewOf(blob: Blob, start: number, end: number): Promise<DataView> {
  return new DataView(await blob.slice(start, end).arrayBuffer());
}

/** Where a zip's central directory starts, and how many entries it has. */
type DirectoryPlace = [start: number, count: number];

/** Where the central directory is, and how many entries it has; rejects when the Blob is no zip. */
async function directory(zip: Blob): Promise<DirectoryPlace> {
  const from = Math.max(0, zip.size - END_BYTES - MAX_COMMENT);
  const tail = await viewOf(zip, from, zip.size);
  let end = -1;
  for (let at = tail.byteLength - END_BYTES; at >= 0; at--) {
    if (tail.getUint32(at, true) === DIRECTORY_END) {
      end = at;
      break;
    }
  }
  if (end < 0) throw new Error('This file is not a zip');
  let count = tail.getUint16(end + 10, true);
  let start = tail.getUint32(end + 16, true);
  const locator = end - LOCATOR_BYTES;
  if (start === IN_ZIP64 && locator >= 0 && tail.getUint32(locator, true) === ZIP64_LOCATOR) {
    const recordAt = u64(tail, locator + 8);
    const record = await viewOf(zip, recordAt, recordAt + 56);
    if (record.getUint32(0, true) !== ZIP64_END) throw new Error('The zip64 end record is missing');
    count = u64(record, 32);
    start = u64(record, 48);
  }
  return [start, count];
}

/** The zip64 values of an entry's extra field, in the order the 32-bit fields that hold IN_ZIP64 appear. */
function zip64Values(view: DataView, at: number, end: number, wanted: number): number[] {
  while (at + 4 <= end) {
    const id = view.getUint16(at, true);
    const len = view.getUint16(at + 2, true);
    if (id === ZIP64_EXTRA)
      return Array.from({ length: wanted }, (_unused, i) => u64(view, at + 4 + i * 8));
    at += 4 + len;
  }
  return [];
}

/** Every entry of the zip, from its central directory. */
export async function readZip(zip: Blob): Promise<ZipItem[]> {
  const [start, count] = await directory(zip);
  const view = await viewOf(zip, start, zip.size);
  const items: ZipItem[] = [];
  let at = 0;
  for (let i = 0; i < count; i++) {
    if (view.getUint32(at, true) !== CENTRAL_HEADER) throw new Error('The zip is damaged');
    const method = view.getUint16(at + 10, true);
    const fields = [
      view.getUint32(at + 24, true),
      view.getUint32(at + 20, true),
      view.getUint32(at + 42, true),
    ];
    const nameLen = view.getUint16(at + 28, true);
    const extraLen = view.getUint16(at + 30, true);
    const commentLen = view.getUint16(at + 32, true);
    const nameAt = at + CENTRAL_BYTES;
    const path = new TextDecoder().decode(new Uint8Array(view.buffer, nameAt, nameLen));
    const big = zip64Values(
      view,
      nameAt + nameLen,
      nameAt + nameLen + extraLen,
      fields.filter((value) => value === IN_ZIP64).length,
    );
    const [size, packed, offset] = fields.map((value) =>
      value === IN_ZIP64 ? (big.shift() ?? 0) : value,
    );
    items.push({ path, size, blob: () => entryBlob(zip, offset, packed, method) });
    at = nameAt + nameLen + extraLen + commentLen;
  }
  return items;
}

/** An entry's bytes: its local header skipped, inflated when it is deflated. */
async function entryBlob(zip: Blob, offset: number, packed: number, method: number): Promise<Blob> {
  const local = await viewOf(zip, offset, offset + LOCAL_BYTES);
  const dataAt = offset + LOCAL_BYTES + local.getUint16(26, true) + local.getUint16(28, true);
  const data = zip.slice(dataAt, dataAt + packed);
  if (method === STORED) return data;
  if (method !== DEFLATED)
    throw new Error(`An entry is compressed in a way this page cannot read (${method})`);
  // a deflated entry is one of the small files (the videos are stored): read whole, then inflated
  const body = new Response(new Uint8Array(await data.arrayBuffer())).body;
  if (!body) throw new Error('An entry could not be read');
  return new Response(body.pipeThrough(new DecompressionStream('deflate-raw'))).blob();
}
