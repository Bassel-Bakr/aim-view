import { zipFile } from './zip-file';

/** An array's element type, as NumPy names it: bytes, or little-endian 32-bit floats. */
export type NpyType = '|u1' | '<f4';

/** An array for an .npz: its name there, its element type, its shape and its bytes (C order). */
export interface NpyArray {
  name: string;
  descr: NpyType;
  shape: readonly number[];
  data: Uint8Array<ArrayBuffer>;
}

/** NumPy's header alignment, and the room it leaves for the first axis to grow (numpy/lib/format.py). */
const ALIGN = 64;
const GROWTH_DIGITS = 21;
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
