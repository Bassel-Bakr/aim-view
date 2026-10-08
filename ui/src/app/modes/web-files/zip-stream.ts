/**
 * Writes a zip as it goes, to a sink (a file the user chose, or parts kept for a Blob), so a large
 * file such as a recording's video never sits in memory whole. Small files go in deflated with
 * their sizes known; a stream goes in stored, its checksum and sizes in a data descriptor after it.
 * Zip64 throughout (sizes and offsets past 4 GB, as several videos make). In: files and streams.
 * Out: the zip's bytes, written to the sink in order.
 */
import { crc32, deflateRaw } from './zip-file';

/** Where the zip's bytes go. */
export interface ZipSink {
  /** Writes the next bytes. */
  write(chunk: Uint8Array): Promise<void>;
  /** Ends the zip. */
  close(): Promise<void>;
}

/** An entry as written, for the central directory. */
interface CentralEntry {
  /** The path as UTF-8. */
  name: Uint8Array;
  /** STORED or DEFLATED. */
  method: number;
  /** The general purpose flags. */
  flags: number;
  /** The CRC-32 of its bytes. */
  crc: number;
  /** Its size as written. */
  packed: number;
  /** Its size before compression. */
  size: number;
  /** Where its local header starts. */
  offset: number;
  /** Its time, as MS-DOS time and date. */
  dos: DosTime;
}

/** A time as MS-DOS keeps it in a zip: two 16-bit fields. */
interface DosTime {
  /** Hours, minutes, seconds / 2. */
  time: number;
  /** Year since 1980, month, day. */
  date: number;
}

/** Stored as it is. */
const STORED = 0;
/** Deflated. */
const DEFLATED = 8;
/** The version that reads zip64 (4.5). */
const ZIP64_VERSION = 45;
/** Names are UTF-8 (bit 11). */
const UTF8_NAMES = 0x0800;
/** The sizes and checksum follow the data, in a data descriptor (bit 3). */
const SIZES_AFTER = 0x0008;
/** A 32-bit field that says "see the zip64 extra field". */
const IN_ZIP64 = 0xffffffff;
/** A 16-bit count that says "see the zip64 end record". */
const COUNT_IN_ZIP64 = 0xffff;
/** The zip64 extra field's id. */
const ZIP64_EXTRA = 0x0001;
/** The local file header's signature. */
const LOCAL_HEADER = 0x04034b50;
/** The data descriptor's signature. */
const DATA_DESCRIPTOR = 0x08074b50;
/** The central directory header's signature. */
const CENTRAL_HEADER = 0x02014b50;
/** The zip64 end of central directory record's signature. */
const ZIP64_END = 0x06064b50;
/** The zip64 end locator's signature. */
const ZIP64_LOCATOR = 0x07064b50;
/** The end of central directory record's signature. */
const DIRECTORY_END = 0x06054b50;
/** The zip64 end record's size after its first 12 bytes. */
const ZIP64_END_REST = 44;
/** The year MS-DOS times start at. */
const DOS_EPOCH_YEAR = 1980;

/** Little-endian fields written one after another into a growing buffer. */
class Fields {
  /** The bytes. */
  private readonly bytes: number[] = [];

  /** A 16-bit field. */
  u16(value: number): this {
    this.bytes.push(value & 0xff, (value >>> 8) & 0xff);
    return this;
  }

  /** A 32-bit field. */
  u32(value: number): this {
    return this.u16(value & 0xffff).u16((value >>> 16) & 0xffff);
  }

  /** A 64-bit field (values up to 2^53). */
  u64(value: number): this {
    return this.u32(value % 2 ** 32).u32(Math.floor(value / 2 ** 32));
  }

  /** Bytes as they are. */
  raw(data: Uint8Array): this {
    for (const byte of data) this.bytes.push(byte);
    return this;
  }

  /** The bytes written. */
  done(): Uint8Array {
    return new Uint8Array(this.bytes);
  }
}

/** A time (ms since 1970) in MS-DOS form; times before 1980 as 1980-01-01. */
function dosTime(ms: number): DosTime {
  const at = new Date(Math.max(ms, new Date(DOS_EPOCH_YEAR, 0, 1).getTime()));
  return {
    time: (at.getHours() << 11) | (at.getMinutes() << 5) | Math.floor(at.getSeconds() / 2),
    date: ((at.getFullYear() - DOS_EPOCH_YEAR) << 9) | ((at.getMonth() + 1) << 5) | at.getDate(),
  };
}

/** Writes a zip to a sink, file after file (see the module's comment). */
export class ZipWriter {
  /** The bytes written so far: the next local header's offset. */
  private at = 0;
  /** Every entry written. */
  private readonly entries: CentralEntry[] = [];

  /** Writes to `sink`. */
  constructor(private readonly sink: ZipSink) {}

  /** Writes bytes and counts them. */
  private async put(chunk: Uint8Array): Promise<void> {
    await this.sink.write(chunk);
    this.at += chunk.length;
  }

  /** A local header; its sizes go in the zip64 extra field (zeros when they follow the data). */
  private localHeader(entry: CentralEntry): Uint8Array {
    const after = (entry.flags & SIZES_AFTER) !== 0;
    return new Fields()
      .u32(LOCAL_HEADER)
      .u16(ZIP64_VERSION)
      .u16(entry.flags)
      .u16(entry.method)
      .u16(entry.dos.time)
      .u16(entry.dos.date)
      .u32(after ? 0 : entry.crc)
      .u32(IN_ZIP64)
      .u32(IN_ZIP64)
      .u16(entry.name.length)
      .u16(20)
      .raw(entry.name)
      .u16(ZIP64_EXTRA)
      .u16(16)
      .u64(after ? 0 : entry.size)
      .u64(after ? 0 : entry.packed)
      .done();
  }

  /** Adds a file whose bytes are at hand, deflated when `deflate`. */
  async addBytes(
    path: string,
    data: Uint8Array<ArrayBuffer>,
    modifiedMs: number,
    deflate = true,
  ): Promise<void> {
    const packed = deflate ? await deflateRaw(data) : data;
    const entry: CentralEntry = {
      name: new TextEncoder().encode(path),
      method: deflate ? DEFLATED : STORED,
      flags: UTF8_NAMES,
      crc: crc32(data),
      packed: packed.length,
      size: data.length,
      offset: this.at,
      dos: dosTime(modifiedMs),
    };
    await this.put(this.localHeader(entry));
    await this.put(packed);
    this.entries.push(entry);
  }

  /**
   * Adds a file read from a stream, stored as it is (a video is compressed already); its checksum
   * and sizes follow it. `progress` is told each chunk's length.
   */
  async addStream(
    path: string,
    stream: ReadableStream<Uint8Array>,
    modifiedMs: number,
    progress: (bytes: number) => void = () => undefined,
  ): Promise<void> {
    const entry: CentralEntry = {
      name: new TextEncoder().encode(path),
      method: STORED,
      flags: UTF8_NAMES | SIZES_AFTER,
      crc: 0,
      packed: 0,
      size: 0,
      offset: this.at,
      dos: dosTime(modifiedMs),
    };
    await this.put(this.localHeader(entry));
    const reader = stream.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      entry.crc = crc32(value, entry.crc);
      entry.size += value.length;
      await this.put(value);
      progress(value.length);
    }
    entry.packed = entry.size;
    await this.put(
      new Fields().u32(DATA_DESCRIPTOR).u32(entry.crc).u64(entry.packed).u64(entry.size).done(),
    );
    this.entries.push(entry);
  }

  /** A central directory header, every size and the offset in its zip64 extra field. */
  private centralHeader(entry: CentralEntry): Uint8Array {
    return new Fields()
      .u32(CENTRAL_HEADER)
      .u16(ZIP64_VERSION)
      .u16(ZIP64_VERSION)
      .u16(entry.flags)
      .u16(entry.method)
      .u16(entry.dos.time)
      .u16(entry.dos.date)
      .u32(entry.crc)
      .u32(IN_ZIP64)
      .u32(IN_ZIP64)
      .u16(entry.name.length)
      .u16(28)
      .u16(0)
      .u16(0)
      .u16(0)
      .u32(0)
      .u32(IN_ZIP64)
      .raw(entry.name)
      .u16(ZIP64_EXTRA)
      .u16(24)
      .u64(entry.size)
      .u64(entry.packed)
      .u64(entry.offset)
      .done();
  }

  /** Writes the central directory and its zip64 and plain end records, and closes the sink. */
  async finish(): Promise<void> {
    const start = this.at;
    for (const entry of this.entries) await this.put(this.centralHeader(entry));
    const size = this.at - start;
    const end64 = this.at;
    const count = this.entries.length;
    const record = new Fields()
      .u32(ZIP64_END)
      .u64(ZIP64_END_REST)
      .u16(ZIP64_VERSION)
      .u16(ZIP64_VERSION)
      .u32(0)
      .u32(0)
      .u64(count)
      .u64(count)
      .u64(size)
      .u64(start)
      .u32(ZIP64_LOCATOR)
      .u32(0)
      .u64(end64)
      .u32(1)
      .u32(DIRECTORY_END)
      .u16(0)
      .u16(0)
      .u16(Math.min(count, COUNT_IN_ZIP64))
      .u16(Math.min(count, COUNT_IN_ZIP64))
      .u32(IN_ZIP64)
      .u32(IN_ZIP64)
      .u16(0)
      .done();
    await this.put(record);
    await this.sink.close();
  }
}
