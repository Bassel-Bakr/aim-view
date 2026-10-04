import { FOOTER_BYTES, pastRun, readFooter } from './stats-footer';

describe('readFooter', () => {
  const footer = 'Kills:,129\nHit Count:,129\nMiss Count:,13\nScore:,1171.9\nScenario:,a\n';

  it("reads a long file's key-value lines from its end", async () => {
    const table = '1,17:09:19.328,target\n'.repeat(400);
    const meta = await readFooter(new Blob([`Kill #,Timestamp\n${table}\n${footer}`]));
    expect(pastRun('s', meta)).toEqual({
      stamp: 's',
      score: 1171.9,
      kills: 129,
      accuracy: 129 / 142,
    });
  });

  it('reads the whole file when its end holds no score', async () => {
    const meta = await readFooter(new Blob([`Score:,5\n${'x\n'.repeat(FOOTER_BYTES)}`]));
    expect(pastRun('s', meta)).toEqual({ stamp: 's', score: 5, kills: null, accuracy: null });
  });

  it('gives no run for a file without a score', async () => {
    expect(pastRun('s', await readFooter(new Blob(['Kills:,1\n'])))).toBeNull();
  });
});
