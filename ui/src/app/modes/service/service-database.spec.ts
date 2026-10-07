import { decodeValues, encodeRows } from './service-database';

describe('the service database binary form', () => {
  it('reads back every kind of value, large ones included', () => {
    const big = Uint8Array.from({ length: 100_000 }, (_unused, i) => i % 251);
    const values = [
      null,
      -7_000_000_000n,
      1_791_337_350.9418318,
      'Valorant ｜ #2',
      '',
      big,
      'after',
    ];
    const rows = encodeRows(
      1,
      values.map((value) => [value]),
    );
    const head = new DataView(rows.buffer, rows.byteOffset, 8);
    expect([head.getUint32(0, true), head.getUint32(4, true)]).toEqual([1, values.length]);
    expect(decodeValues(rows.subarray(8))).toEqual(values);
  });
});
