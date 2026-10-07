/**
 * Writes zip files in the browser, with the same bytes each time for the same input. In: files'
 * paths and bytes (an .npz's arrays in npz-file.ts, the cut-off's labels in cutoff-labels.ts).
 * Out: the zip's bytes, compressed with the browser's CompressionStream where asked.
 */

/**
 * A file to put in a zip: its path inside it, its bytes, and whether to compress it (deflate) or
 * store it as it is.
 */
export interface ZipEntry {
  /** The file's path inside the zip, with forward slashes ("train/a.npz"). */
  path: string;
  /** The file's bytes before compression. */
  data: Uint8Array<ArrayBuffer>;
  /** Compress it with deflate (true) or store it as it is (false). */
  deflate: boolean;
}

/**
 * An entry as written: its name, compression method, checksum, sizes, and where its local header
 * starts.
 */
interface Written {
  /** The path as UTF-8 bytes, as both headers hold it. */
  name: Uint8Array<ArrayBuffer>;
  /** The compression method: `STORED` or `DEFLATED`. */
  method: number;
  /** The CRC-32 of the bytes before compression. */
  crc: number;
  /** The size in bytes before compression. */
  size: number;
  /** The size in bytes as written (compressed, or the same as size when stored). */
  packed: number;
  /** The byte offset of the entry's local header from the zip's start. */
  offset: number;
}

/** The compression method of a file stored as it is. */
const STORED = 0;
/** The compression method of a deflated file. */
const DEFLATED = 8;
/** The zip version needed to extract and made by: 2.0, the first with deflate. */
const VERSION = 20;
/** The names are UTF-8 (general purpose flag, bit 11). */
const UTF8_NAMES = 0x0800;
/**
 * 1980-01-01 00:00 in MS-DOS form, the zip format's earliest time: what NumPy's savez writes, so
 * files repeat. The time part: hour, minute and second all 0.
 */
const DOS_TIME = 0;
/** The date part: year 0 from 1980 (bits 9 up), month 1 (bits 5 to 8), day 1. */
const DOS_DATE = (0 << 9) | (1 << 5) | 1;

/** The signature of a local file header, before each file's bytes. */
const LOCAL_HEADER = 0x04034b50;
/** The signature of a central directory header, one for each file after all the files. */
const CENTRAL_HEADER = 0x02014b50;
/** The signature of the end of the central directory, the zip's last record. */
const DIRECTORY_END = 0x06054b50;
/** CRC-32's polynomial, reversed (the zip format shifts right). */
const CRC_POLYNOMIAL = 0xedb88320;
/** How many values a byte can take: the CRC table's length. */
const BYTE_VALUES = 256;
/** The bits in a byte. */
const BITS_PER_BYTE = 8;
/** Keeps a number's lowest byte. */
const BYTE_MASK = 0xff;
/** Keeps a number's lowest 16 bits. */
const U16_MASK = 0xffff;
/** The bits in a 16-bit field. */
const U16_BITS = 16;
/** All 32 bits set: CRC-32's start value and its final flip. */
const U32_ALL = 0xffffffff;

/** CRC-32's remainder for each byte value, so the checksum takes a byte at a time. */
const CRC_TABLE = (() => {
  const table = new Uint32Array(BYTE_VALUES);
  for (let byte = 0; byte < BYTE_VALUES; byte++) {
    let remainder = byte;
    for (let bit = 0; bit < BITS_PER_BYTE; bit++)
      remainder = remainder & 1 ? CRC_POLYNOMIAL ^ (remainder >>> 1) : remainder >>> 1;
    table[byte] = remainder >>> 0;
  }
  return table;
})();

/** The CRC-32 a zip keeps for each file. */
export function crc32(data: Uint8Array): number {
  let crc = U32_ALL;
  for (const byte of data) crc = CRC_TABLE[(crc ^ byte) & BYTE_MASK] ^ (crc >>> BITS_PER_BYTE);
  return (crc ^ U32_ALL) >>> 0;
}

/** The bytes compressed as raw deflate, as a zip holds them. */
async function deflateRaw(data: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
  const body = new Response(data).body;
  if (!body) throw new Error('The bytes could not be read for compression');
  const packed = body.pipeThrough(new CompressionStream('deflate-raw'));
  return new Uint8Array(await new Response(packed).arrayBuffer());
}

/** Little-endian fields written one after another. */
class Fields {
  /** The bytes written so far, each 0 to 255. */
  private readonly bytes: number[] = [];

  /** Adds the value's lowest 16 bits as 2 bytes, low byte first. */
  u16(value: number): this {
    this.bytes.push(value & BYTE_MASK, (value >>> BITS_PER_BYTE) & BYTE_MASK);
    return this;
  }

  /** Adds the value as 4 bytes, low byte first. */
  u32(value: number): this {
    return this.u16(value & U16_MASK).u16(value >>> U16_BITS);
  }

  /** The fields' bytes. */
  done(): Uint8Array<ArrayBuffer> {
    return new Uint8Array(this.bytes);
  }
}

/** An entry's local file header: no extra field. */
function localHeader(entry: Written): Uint8Array<ArrayBuffer> {
  return new Fields()
    .u32(LOCAL_HEADER)
    .u16(VERSION)
    .u16(UTF8_NAMES)
    .u16(entry.method)
    .u16(DOS_TIME)
    .u16(DOS_DATE)
    .u32(entry.crc)
    .u32(entry.packed)
    .u32(entry.size)
    .u16(entry.name.length)
    .u16(0)
    .done();
}

/** An entry's central directory header: no extra field, comment, disk number or attributes. */
function centralHeader(entry: Written): Uint8Array<ArrayBuffer> {
  return new Fields()
    .u32(CENTRAL_HEADER)
    .u16(VERSION)
    .u16(VERSION)
    .u16(UTF8_NAMES)
    .u16(entry.method)
    .u16(DOS_TIME)
    .u16(DOS_DATE)
    .u32(entry.crc)
    .u32(entry.packed)
    .u32(entry.size)
    .u16(entry.name.length)
    .u16(0)
    .u16(0)
    .u16(0)
    .u16(0)
    .u32(0)
    .u32(entry.offset)
    .done();
}

/** The end of the central directory: its entry count, its size and where it starts. */
function directoryEnd(count: number, size: number, start: number): Uint8Array<ArrayBuffer> {
  return new Fields()
    .u32(DIRECTORY_END)
    .u16(0)
    .u16(0)
    .u16(count)
    .u16(count)
    .u32(size)
    .u32(start)
    .u16(0)
    .done();
}

/** The parts one after another. */
function joined(parts: Uint8Array<ArrayBuffer>[]): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

/**
 * The entries as a zip file (no zip64: under 4 GB and 65,535 files), each stored or deflated,
 * dated 1980-01-01 as NumPy dates the files in an .npz.
 */
export async function zipFile(entries: readonly ZipEntry[]): Promise<Uint8Array<ArrayBuffer>> {
  const parts: Uint8Array<ArrayBuffer>[] = [];
  const written: Written[] = [];
  let at = 0;
  for (const entry of entries) {
    const name = new TextEncoder().encode(entry.path);
    const body = entry.deflate ? await deflateRaw(entry.data) : entry.data;
    const kept: Written = {
      name,
      method: entry.deflate ? DEFLATED : STORED,
      crc: crc32(entry.data),
      size: entry.data.length,
      packed: body.length,
      offset: at,
    };
    const head = localHeader(kept);
    parts.push(head, name, body);
    at += head.length + name.length + body.length;
    written.push(kept);
  }
  const start = at;
  for (const kept of written) {
    const head = centralHeader(kept);
    parts.push(head, kept.name);
    at += head.length + kept.name.length;
  }
  parts.push(directoryEnd(written.length, at - start, start));
  return joined(parts);
}
