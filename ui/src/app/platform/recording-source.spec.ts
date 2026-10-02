import { HttpRequest } from '@angular/common/http';
import { Recording } from '../api';
import { ApiRoutes, recording } from '../fake-api';
import { MODE_CASES, setUp } from './contract-case';
import { RecordingSource } from './recording-source';

const NAME = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const STATS_NAME = 'Air - Challenge - 2026.10.01-16.23.05 Stats.csv';
const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,1\nScore:,1\nScenario:,Air\n';

/** A review server that keeps what is sent to it, the way python/server.py does. */
function fakeServer(): ApiRoutes {
  let list: Recording[] = [];
  return {
    '/api/vods': () => list,
    '/api/upload': (req: HttpRequest<unknown>) => {
      const name = req.params.get('name') ?? '';
      const id = req.params.get('id');
      if (id) {
        list = list.map((r) => (r.id === id ? { ...r, stats: true } : r));
        return { id, saved: name, job: { stage: 'none' }, stats: true };
      }
      const added = `uploads/${name}`;
      list = [recording({ id: added, scenario: 'Air', stats: false, analysed: false }), ...list];
      return { id: added, saved: name };
    },
  };
}

for (const mode of MODE_CASES) {
  describe(`RecordingSource (${mode.name} mode)`, () => {
    it('lists an added video with its stats file, ready to play', async () => {
      const source = setUp(mode, RecordingSource);
      const files = [new File(['v'], NAME), new File([STATS], STATS_NAME)];
      const added = await mode.finish(source.add(files), fakeServer());
      expect(added.notStats).toEqual([]);
      const [id] = added.ids;
      expect(source.recordings().find((r) => r.id === id)).toMatchObject({
        scenario: 'Air',
        stats: true,
      });
      expect(source.video(id)?.state).toBe('ready');
      expect(source.problem()).toBeNull();
    });

    it('names a .csv that is not a stats file, and adds the video all the same', async () => {
      const source = setUp(mode, RecordingSource);
      const files = [new File(['v'], NAME), new File(['a,b\n1,2\n'], 'notes.csv')];
      const added = await mode.finish(source.add(files), fakeServer());
      expect(added.notStats).toEqual(['notes.csv']);
      expect(source.recordings().find((r) => r.id === added.ids[0])?.stats).toBe(false);
    });

    it('changes a row after a change made elsewhere', async () => {
      const source = setUp(mode, RecordingSource);
      const [id] = (await mode.finish(source.add([new File(['v'], NAME)]), fakeServer())).ids;
      source.patch(id, { analysed: true });
      expect(source.recordings().find((r) => r.id === id)?.analysed).toBe(true);
    });
  });
}
