import { TestBed } from '@angular/core/testing';
import { BrowserStore } from './browser-store';
import { LocalFiles } from './local-files';
import { chosenVideos, folderVideos } from './recordings-folder';

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
    expect(local.lasting(id)).toBe(false);
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

  it("keeps a folder video's stats file for the next visit, with no stats folder open then", async () => {
    const kept = new Map<string, unknown>();
    const store = {
      get: async (k: string) => kept.get(k),
      set: async (k: string, v: unknown) => void kept.set(k, v),
      remove: async (k: string) => void kept.delete(k),
    };
    const visit = () => {
      TestBed.resetTestingModule();
      TestBed.configureTestingModule({ providers: [{ provide: BrowserStore, useValue: store }] });
      return TestBed.inject(LocalFiles);
    };
    const video = new File(['x'], 'Probe - 30.42 - 2026.09.27-19.31.47.mp4');
    const first = visit();
    await first.addFolder([{ path: 'Probe/a.mp4', file: video }]);
    expect(await first.pair('folder:Probe/a.mp4', file('mine.csv', STATS), 'picked')).toBe(true);
    const next = visit();
    await new Promise((r) => setTimeout(r));
    await next.addFolder([{ path: 'Probe/a.mp4', file: video }]);
    expect(next.find('folder:Probe/a.mp4')).toMatchObject({
      statsHow: 'picked',
      stats: { name: 'mine.csv' },
    });
  });

  it("lists a folder's videos newest first, in place of the folder listed before, and a link to one lasts", async () => {
    const local = TestBed.inject(LocalFiles);
    const [upload] = (await local.add([file('mine.mp4')])).ids;
    const old = new File(['x'], 'Air - 1 - 2026.10.01-10.00.00.mp4', { lastModified: 1 });
    const recent = new File(['x'], 'Air - 2 - 2026.10.01-11.00.00.mkv', { lastModified: 2 });
    await local.addFolder([{ path: 'Air/old.mp4', file: old }]);
    await local.addFolder([
      { path: 'Air/old.mp4', file: old },
      { path: 'Air/recent.mkv', file: recent },
    ]);
    expect(local.recordings().map((r) => r.id)).toEqual([
      upload,
      'folder:Air/recent.mkv',
      'folder:Air/old.mp4',
    ]);
    expect(local.lasting('folder:Air/old.mp4')).toBe(true);
    // a video that is not an MP4 waits for its remux until it is opened
    expect(local.find('folder:Air/recent.mkv')?.video()).toEqual({
      state: 'remuxing',
      progress: 0,
    });
  });
});

describe('the recordings folder', () => {
  /** A stand-in for a folder's handle: folders hold folders (objects) and files (strings). */
  interface Tree {
    [name: string]: Tree | string;
  }
  const dir = (name: string, tree: Tree): FileSystemDirectoryHandle =>
    ({
      kind: 'directory',
      name,
      async *entries() {
        for (const [n, t] of Object.entries(tree))
          yield [
            n,
            typeof t === 'object'
              ? dir(n, t)
              : { kind: 'file', name: n, getFile: async () => new File([t], n) },
          ];
      },
    }) as unknown as FileSystemDirectoryHandle;

  it("finds the videos in each scenario's folder, by their paths below the folder", async () => {
    const found = await folderVideos(
      dir('KovOBS', {
        Air: { 'a.mp4': 'x', 'notes.txt': 'x' },
        Bot: { deep: { 'b.mkv': 'x' } },
        'c.mp4': 'x',
      }),
    );
    expect(found.map((e) => e.path).sort()).toEqual(['Air/a.mp4', 'Bot/deep/b.mkv', 'c.mp4']);
  });

  it('takes the videos of a folder chosen as files, by their paths below it', () => {
    const f = new File(['x'], 'a.mp4');
    Object.defineProperty(f, 'webkitRelativePath', { value: 'KovOBS/Air/a.mp4' });
    const g = new File(['x'], 'a.txt');
    Object.defineProperty(g, 'webkitRelativePath', { value: 'KovOBS/Air/a.txt' });
    expect(chosenVideos([f, g]).map((e) => e.path)).toEqual(['Air/a.mp4']);
  });
});
