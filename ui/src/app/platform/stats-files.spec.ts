import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { StatsChoice, StatsPairing } from '../api';
import { ApiRoutes } from '../fake-api';
import { MODE_CASES, setUp } from './contract-case';
import { RecordingSource } from './recording-source';
import { StatsFiles } from './stats-files';

const NAME = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,1\nScore:,1\nScenario:,Air\n';

/** A review server that pairs what it is sent, the way python/server.py does. */
function fakeServer(): ApiRoutes {
  let file: string | null = null;
  const pairing = (): StatsPairing => ({
    file,
    how: file ? 'upload' : 'none',
    scenario: 'Air',
    candidates: [],
  });
  return {
    '/api/vods': [],
    '/api/upload': (req: HttpRequest<unknown>) => {
      const name = req.params.get('name') ?? '';
      if (!req.params.get('id')) return { id: `uploads/${name}`, saved: name };
      file = name;
      return { id: req.params.get('id'), saved: name, job: { stage: 'none' }, stats: true };
    },
    '/api/stats': (req: HttpRequest<unknown>) => {
      if (req.method !== 'POST') return pairing();
      const choice = req.body as StatsChoice;
      file = 'file' in choice ? choice.file : null;
      return { job: { stage: 'none' }, stats: file !== null };
    },
  };
}

for (const mode of MODE_CASES) {
  describe(`StatsFiles (${mode.name} mode)`, () => {
    async function open(): Promise<string> {
      const source = setUp(mode, RecordingSource);
      return (await mode.finish(source.add([new File(['v'], NAME)]), routes)).ids[0];
    }
    let routes: ApiRoutes;
    beforeEach(() => (routes = fakeServer()));

    it('pairs a recording with a stats file from this computer, then with none', async () => {
      const id = await open();
      const stats = TestBed.inject(StatsFiles);
      const paired = await mode.finish(stats.pairFile(id, new File([STATS], 'mine.csv')), routes);
      expect(paired.stats).toBe(true);
      const none = await mode.finish(stats.choose(id, { file: null, source: 'kovaak' }), routes);
      expect(none.stats).toBe(false);
    });

    it('turns down a file that is not one of KovaaK’s stats files', async () => {
      const id = await open();
      const stats = TestBed.inject(StatsFiles);
      await expect(
        mode.finish(stats.pairFile(id, new File(['a,b\n'], 'notes.csv')), routes),
      ).rejects.toThrow(/not one of KovaaK's stats files/);
    });
  });
}
