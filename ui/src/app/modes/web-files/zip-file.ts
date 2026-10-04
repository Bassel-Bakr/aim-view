/** A file to put in a zip: its path inside it, its bytes, and whether to compress it (deflate) or store it as it is. */
export interface ZipEntry {
  path: string;
  data: Uint8Array<ArrayBuffer>;
  deflate: boolean;
}

/** An entry as written: its name, compression method, checksum, sizes, and where its local header starts. */
interface Written {
  name: Uint8Array<ArrayBuffer>;
  method: number;
  crc: number;
  size: number;
  packed: number;
  offset: number;
}

const STORED = 0;
const DEFLATED = 8;
const VERSION = 20;
/** The names are UTF-8 (general purpose flag, bit 11). */
const UTF8_NAMES = 0x0800;
/** 1980-01-01 00:00 in MS-DOS form, the zip format's earliest time: what NumPy's savez writes, so files repeat. */
const DOS_TIME = 0;
const DOS_DATE = (0 << 9) | (1 << 5) | 1;

/** The zip format's record signatures: a local file header, a central directory header, the directory's end. */
const LOCAL_HEADER = 0x04034b50;
const CENTRAL_HEADER = 0x02014b50;
const DIRECTORY_END = 0x06054b50;
/** CRC-32's polynomial, reversed, and its table of each byte's remainder. */
const CRC_POLYNOMIAL = 0xedb88320;
const BYTE_VALUES = 256;
const BITS_PER_BYTE = 8;
const BYTE_MASK = 0xff;
const U16_MASK = 0xffff;
const U16_BITS = 16;
const U32_ALL = 0xffffffff;

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
  private readonly bytes: number[] = [];

  u16(value: number): this {
    this.bytes.push(value & BYTE_MASK, (value >>> BITS_PER_BYTE) & BYTE_MASK);
    return this;
  }

  u32(value: number): this {
    return this.u16(value & U16_MASK).u16(value >>> U16_BITS);
  }

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
 * The entries as a zip file (no zip64: under 4 GB and 65,535 files), each stored or deflated, dated 1980-01-01 as
 * NumPy dates the files in an .npz.
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
