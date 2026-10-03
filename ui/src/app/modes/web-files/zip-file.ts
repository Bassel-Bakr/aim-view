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

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

/** The CRC-32 a zip keeps for each file. */
export function crc32(data: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of data) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
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

  u16(v: number): this {
    this.bytes.push(v & 0xff, (v >>> 8) & 0xff);
    return this;
  }

  u32(v: number): this {
    return this.u16(v & 0xffff).u16(v >>> 16);
  }

  done(): Uint8Array<ArrayBuffer> {
    return new Uint8Array(this.bytes);
  }
}

/**
 * The entries as a zip file (no zip64: under 4 GB and 65,535 files), each stored or deflated, dated 1980-01-01 as
 * NumPy dates the files in an .npz.
 */
export async function zipFile(entries: readonly ZipEntry[]): Promise<Uint8Array<ArrayBuffer>> {
  const parts: Uint8Array<ArrayBuffer>[] = [];
  const written: Written[] = [];
  let at = 0;
  for (const e of entries) {
    const name = new TextEncoder().encode(e.path);
    const body = e.deflate ? await deflateRaw(e.data) : e.data;
    const w: Written = {
      name,
      method: e.deflate ? DEFLATED : STORED,
      crc: crc32(e.data),
      size: e.data.length,
      packed: body.length,
      offset: at,
    };
    const head = new Fields()
      .u32(0x04034b50)
      .u16(VERSION)
      .u16(UTF8_NAMES)
      .u16(w.method)
      .u16(DOS_TIME)
      .u16(DOS_DATE)
      .u32(w.crc)
      .u32(w.packed)
      .u32(w.size)
      .u16(name.length)
      .u16(0)
      .done();
    parts.push(head, name, body);
    at += head.length + name.length + body.length;
    written.push(w);
  }
  const start = at;
  for (const w of written) {
    const head = new Fields()
      .u32(0x02014b50)
      .u16(VERSION)
      .u16(VERSION)
      .u16(UTF8_NAMES)
      .u16(w.method)
      .u16(DOS_TIME)
      .u16(DOS_DATE)
      .u32(w.crc)
      .u32(w.packed)
      .u32(w.size)
      .u16(w.name.length)
      .u16(0)
      .u16(0)
      .u16(0)
      .u16(0)
      .u32(0)
      .u32(w.offset)
      .done();
    parts.push(head, w.name);
    at += head.length + w.name.length;
  }
  parts.push(
    new Fields()
      .u32(0x06054b50)
      .u16(0)
      .u16(0)
      .u16(written.length)
      .u16(written.length)
      .u32(at - start)
      .u32(start)
      .u16(0)
      .done(),
  );
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let k = 0;
  for (const p of parts) {
    out.set(p, k);
    k += p.length;
  }
  return out;
}
