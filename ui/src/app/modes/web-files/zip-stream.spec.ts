import { readZip } from './zip-read';
import { ZipSink, ZipWriter } from './zip-stream';

/** A sink that keeps the zip's parts, for a Blob. */
class Parts implements ZipSink {
  /** The parts written. */
  readonly parts: Uint8Array[] = [];
  /** Keeps a copy of the part. */
  async write(chunk: Uint8Array): Promise<void> {
    this.parts.push(chunk.slice());
  }
  /** Nothing to close. */
  async close(): Promise<void> {
    return undefined;
  }
}

/** A stream of `chunks`. */
function streamOf(chunks: Uint8Array[]): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(chunk);
      controller.close();
    },
  });
}

describe('a zip written as it goes', () => {
  it('reads back a deflated file and a streamed one, byte for byte', async () => {
    const sink = new Parts();
    const zip = new ZipWriter(sink);
    const json = new TextEncoder().encode(
      JSON.stringify({ frames: Array.from({ length: 500 }, (_u, i) => i) }),
    );
    await zip.addBytes('recordings/01 Air/reviews/m1/tracks.json', json, Date.UTC(2026, 9, 8));
    const video = Uint8Array.from({ length: 300_000 }, (_u, i) => (i * 7) % 256);
    let seen = 0;
    await zip.addStream(
      'recordings/01 Air/video/Air ｜ 1.mp4',
      streamOf([video.subarray(0, 100_000), video.subarray(100_000)]),
      0,
      (bytes) => (seen += bytes),
    );
    await zip.finish();
    expect(seen).toBe(video.length);
    const items = await readZip(new Blob(sink.parts as BlobPart[]));
    expect(items.map((item) => [item.path, item.size])).toEqual([
      ['recordings/01 Air/reviews/m1/tracks.json', json.length],
      ['recordings/01 Air/video/Air ｜ 1.mp4', video.length],
    ]);
    const [tracks, mp4] = await Promise.all(
      items.map(async (item) => new Uint8Array(await (await item.blob()).arrayBuffer())),
    );
    // compared as plain arrays: the test's TextEncoder and the page's Response make typed arrays of two realms
    expect(Array.from(tracks)).toEqual(Array.from(json));
    expect(Array.from(mp4)).toEqual(Array.from(video));
  });

  it('refuses a file that is not a zip', async () => {
    await expect(readZip(new Blob(['not a zip']))).rejects.toThrow('not a zip');
  });
});
