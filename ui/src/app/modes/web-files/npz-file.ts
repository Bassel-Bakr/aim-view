/**
 * Writes NumPy's .npy and .npz files in the browser, byte for byte as NumPy 2 does. In: arrays of
 * bytes with their names, types and shapes (the cut-off's crops). Out: .npz files for the
 * cut-off's labels zip (cutoff-labels.ts), the files detector training reads.
 */

import { zipFile } from './zip-file';

/** An array's element type, as NumPy names it: bytes, or little-endian 32-bit floats. */
export type NpyType = '|u1' | '<f4';

/** An array for an .npz: its name there, its element type, its shape and its bytes (C order). */
export interface NpyArray {
  /** The array's name in the .npz (its file there is name.npy). */
  name: string;
  /** The element type, as the header's 'descr' writes it. */
  descr: NpyType;
  /** The size of each axis, outermost first. */
  shape: readonly number[];
  /** The elements' bytes in C order (the last axis varies fastest). */
  data: Uint8Array<ArrayBuffer>;
}

/** NumPy's header alignment in bytes: magic, version, length and header end on a multiple of it. */
const ALIGN = 64;
/**
 * The spaces NumPy 2 leaves after the header for the first axis to grow without moving the data
 * (numpy/lib/format.py), less the digits the first axis already takes.
 */
const GROWTH_DIGITS = 21;
/** The bytes before the header: the magic "\x93NUMPY", the version (1, 0) and its 2-byte length. */
const PREFIX = 10;

/** A shape as Python writes a tuple: (256, 256, 3), (5,) or (). */
function shapeText(shape: readonly number[]): string {
  return shape.length === 1 ? `(${shape[0]},)` : `(${shape.join(', ')})`;
}

/** An array as an .npy file (format 1.0), its header as NumPy 2 writes it. */
export function npyFile(a: NpyArray): Uint8Array<ArrayBuffer> {
  let header = `{'descr': '${a.descr}', 'fortran_order': False, 'shape': ${shapeText(a.shape)}, }`;
  if (a.shape.length) header += ' '.repeat(GROWTH_DIGITS - String(a.shape[0]).length);
  const pad = ALIGN - ((PREFIX + header.length + 1) % ALIGN);
  header += `${' '.repeat(pad)}\n`;
  const out = new Uint8Array(PREFIX + header.length + a.data.length);
  out.set([
    0x93,
    ...new TextEncoder().encode('NUMPY'),
    1,
    0,
    header.length & 0xff,
    header.length >> 8,
  ]);
  out.set(new TextEncoder().encode(header), PREFIX);
  out.set(a.data, PREFIX + header.length);
  return out;
}

/** Arrays as an .npz file as NumPy's savez_compressed writes it: each a deflated .npy in a zip. */
export function npzFile(arrays: readonly NpyArray[]): Promise<Uint8Array<ArrayBuffer>> {
  return zipFile(arrays.map((a) => ({ path: `${a.name}.npy`, data: npyFile(a), deflate: true })));
}
