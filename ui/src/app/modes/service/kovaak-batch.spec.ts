import { encodeBatch } from '../web-files/file-batch';
import { freshFiles, KovaakKept, sendBatches } from './kovaak-batch';
import { ChosenFile } from './service-messages';

/** A chosen file at `path` with `text` and a time in ms. */
const chosen = (path: string, text: string, ms: number): ChosenFile => ({
  path,
  file: new File([text], path.split('/').pop() ?? path, { lastModified: ms }),
});

describe("KovaaK's files in batches", () => {
  it('writes each file as its path, time in seconds, length and bytes', async () => {
    const batch = await encodeBatch([chosen('stats/a.csv', 'abc', 1500)]);
    const view = new DataView(batch.buffer);
    const pathLen = view.getUint32(0, true);
    expect(new TextDecoder().decode(batch.subarray(4, 4 + pathLen))).toBe('stats/a.csv');
    expect(view.getFloat64(4 + pathLen, true)).toBe(1.5);
    expect(view.getUint32(12 + pathLen, true)).toBe(3);
    expect(new TextDecoder().decode(batch.subarray(16 + pathLen))).toBe('abc');
  });

  it('sends only the files the service does not keep as they are', () => {
    const files = [
      chosen('stats/same.csv', 'x', 2000),
      chosen('stats/bigger.csv', 'xx', 2000),
      chosen('stats/new.csv', 'x', 2000),
      chosen('scenarios/s.sce', 'x', 3000),
    ];
    const kept: KovaakKept = {
      stats: [
        ['same.csv', 1, 2],
        ['bigger.csv', 1, 2],
      ],
      scenarios: [['scenarios/s.sce', 1, 3]],
    };
    expect(freshFiles(files, kept).map(({ path }) => path)).toEqual([
      'stats/bigger.csv',
      'stats/new.csv',
    ]);
  });

  it('sends every file once, in batches, saying how far it got', async () => {
    const files = Array.from({ length: 2500 }, (_unused, i) => chosen(`stats/${i}.csv`, 'x', 1000));
    const sizes: number[] = [];
    const progress: number[] = [];
    const sent = await sendBatches(
      files,
      async (body) => sizes.push(body.length),
      (done) => progress.push(done),
    );
    expect(sent).toBe(2500);
    expect(sizes.length).toBe(3);
    expect(progress).toEqual([0, 1000, 2000, 2500]);
  });
});
