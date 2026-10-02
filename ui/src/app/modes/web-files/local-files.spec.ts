import { TestBed } from '@angular/core/testing';
import { LocalFiles } from './local-files';

const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,1\nScore:,30.42\nScenario:,Probe\n';

function file(name: string, text = 'x'): File {
  return new File([text], name, { lastModified: Date.UTC(2026, 9, 2, 12, 0, 0) });
}

describe('LocalFiles', () => {
  it('opens an MP4 at once, as a recording of this browser paired with its stats file', async () => {
    const local = TestBed.inject(LocalFiles);
    const added = await local.add([
      file('Probe - 30.42 - 2026.09.27-19.31.47.mp4'),
      file('Probe - Challenge - 2026.09.27-19.31.50 Stats.csv', STATS),
    ]);
    expect(added.notStats).toEqual([]);
    const [id] = added.ids;
    expect(local.lasting()).toBe(false);
    expect(local.find(id)?.video()).toEqual({
      state: 'ready',
      url: expect.stringMatching(/^blob:/),
      remuxed: false,
    });
    const [r] = local.recordings();
    expect(r).toMatchObject({
      id,
      scenario: 'Probe',
      score: 30.42,
      stamp: '2026.09.27-19.31.47',
      stats: true,
      analysed: false,
      local: true,
    });
  });

  it('names a .csv that is not a stats file, and lists the newest recording first', async () => {
    const local = TestBed.inject(LocalFiles);
    const first = await local.add([file('a.mp4')]);
    const second = await local.add([file('b.mp4'), file('notes.csv', 'a,b\n1,2\n')]);
    expect(second.notStats).toEqual(['notes.csv']);
    expect(local.recordings().map((r) => r.id)).toEqual([...second.ids, ...first.ids]);
    expect(local.recordings()[0]).toMatchObject({ scenario: 'b', score: null, stats: false });
  });

  it('pairs a recording with a stats file later, and can take it away again', async () => {
    const local = TestBed.inject(LocalFiles);
    const [id] = (await local.add([file('clip.mp4')])).ids;
    expect(await local.pair(id, file('notes.csv', 'a,b\n'))).toBe(false);
    expect(local.recordings()[0].stats).toBe(false);
    expect(await local.pair(id, file('mine.csv', STATS))).toBe(true);
    // the stats file gives what the video's name does not
    expect(local.recordings()[0]).toMatchObject({ scenario: 'Probe', score: 30.42, stats: true });
    local.unpair(id);
    expect(local.recordings()[0].stats).toBe(false);
  });
});
