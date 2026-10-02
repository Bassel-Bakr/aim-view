import { BlobSink } from './remux';

function bytes(...values: number[]): Uint8Array<ArrayBuffer> {
  return new Uint8Array(values);
}

/** A part of a stand-in Blob: bytes, or another stand-in. */
type Part = Uint8Array | PartsBlob;

/**
 * A stand-in for Blob that keeps its parts. jsdom's own Blob takes the test's byte arrays for text (they come from
 * another realm), so it cannot show which bytes went in.
 */
class PartsBlob {
  constructor(
    readonly parts: Part[],
    readonly options: BlobPropertyBag = {},
  ) {}

  get type(): string {
    return this.options.type ?? '';
  }

  bytes(): number[] {
    return this.parts.flatMap((p) => (p instanceof PartsBlob ? p.bytes() : [...p]));
  }
}

function read(blob: Blob): number[] {
  return (blob as unknown as PartsBlob).bytes();
}

describe('BlobSink', () => {
  beforeEach(() => vi.stubGlobal('Blob', PartsBlob));
  afterEach(() => vi.unstubAllGlobals());

  it('joins writes made in order, and lets the last one fill in bytes of the first chunk', () => {
    const sink = new BlobSink();
    sink.write({ type: 'write', data: bytes(1, 2, 0, 0), position: 0 });
    sink.write({ type: 'write', data: bytes(5, 6), position: 4 });
    sink.write({ type: 'write', data: bytes(7), position: 6 });
    sink.write({ type: 'write', data: bytes(3, 4), position: 2 });
    const blob = sink.blob('video/mp4');
    expect(blob.type).toBe('video/mp4');
    expect(read(blob)).toEqual([1, 2, 3, 4, 5, 6, 7]);
  });

  it('keeps its own copy of the first chunk', () => {
    const sink = new BlobSink();
    const first = bytes(1, 2);
    sink.write({ type: 'write', data: first, position: 0 });
    first[0] = 9;
    expect(read(sink.blob('video/mp4'))).toEqual([1, 2]);
  });

  it('refuses a write it cannot place', () => {
    const sink = new BlobSink();
    sink.write({ type: 'write', data: bytes(1, 2), position: 0 });
    sink.write({ type: 'write', data: bytes(3, 4), position: 2 });
    expect(() => sink.write({ type: 'write', data: bytes(9), position: 3 })).toThrow();
    expect(() => sink.write({ type: 'write', data: bytes(9), position: 7 })).toThrow();
  });
});
