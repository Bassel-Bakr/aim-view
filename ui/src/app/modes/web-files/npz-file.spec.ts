import { npyFile, npzFile } from './npz-file';
import { crc32 } from './zip-file';

describe('npz files', () => {
  it('checks each file as zip does', () => {
    expect(crc32(new TextEncoder().encode('123456789'))).toBe(0xcbf43926);
  });

  it('writes an array with the header NumPy writes', () => {
    const npy = npyFile({
      name: 'rgb',
      descr: '|u1',
      shape: [256, 256, 3],
      data: new Uint8Array(3),
    });
    const header = new TextDecoder().decode(npy.subarray(10, 10 + 118));
    expect([...npy.subarray(0, 10)]).toEqual([0x93, 78, 85, 77, 80, 89, 1, 0, 118, 0]);
    expect(
      header.startsWith("{'descr': '|u1', 'fortran_order': False, 'shape': (256, 256, 3), }"),
    ).toBe(true);
    expect(header.endsWith('\n')).toBe(true);
    const scalar = npyFile({ name: 'hidden', descr: '|u1', shape: [], data: new Uint8Array(1) });
    expect((10 + scalar[8]) % 64).toBe(0);
    expect(new TextDecoder().decode(scalar.subarray(10, 70))).toContain("'shape': (), }");
  });

  it('puts the arrays in a zip, one .npy each', async () => {
    const npz = await npzFile([
      { name: 'boxes', descr: '<f4', shape: [0, 4], data: new Uint8Array(0) },
      { name: 'hidden', descr: '|u1', shape: [], data: new Uint8Array(1) },
    ]);
    const text = new TextDecoder('latin1').decode(npz);
    expect(text.startsWith('PK\x03\x04')).toBe(true);
    expect(text).toContain('boxes.npy');
    expect(text).toContain('hidden.npy');
    expect(text).toContain('PK\x05\x06');
  });
});
