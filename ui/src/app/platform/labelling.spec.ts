import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { Recording } from '../api';
import { ApiRoutes, recording } from '../fake-api';
import { MODE_CASES, setUp } from './contract-case';
import { Labelling } from './labelling';
import { RecordingSource } from './recording-source';

const AIR = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const BOUNCE = 'Bounce - 2 - 2026.10.01-17.00.00.mp4';

/** Two lines of the review server's area_examples.jsonl, as python/areas.py writes them. */
const EXAMPLES =
  '{"rec": "uploads/1902 1wall \\uff5c #2.mp4", "feat": [0.1328, 0.0406, 0.037, 0.0216, 0.4258, 0.2098, 0.0833], "kind": "clock"}\n' +
  '{"rec": "uploads/1902 1wall \\uff5c #2.mp4", "feat": [0.8839, 0.8189, 0.2321, 0.3621, 0.0, 1.0, 0.25], "kind": "none"}\n';
/** The review server's area_kinds.json, as python/server.py writes it. */
const KINDS =
  '[\n {\n  "id": "timer",\n  "name": "Timer",\n  "about": "the run\'s time left"\n },\n' +
  ' {\n  "id": "kill_feed",\n  "name": "Kill feed \\u00b7 top",\n  "about": ""\n }\n]';

/** A review server that keeps skips and other games, the way python/server.py does (its queue simplified). */
function fakeServer(): ApiRoutes {
  let list: Recording[] = [];
  const skipped = new Set<string>();
  const other = new Set<string>();
  return {
    '/api/vods': () => list.map((r) => ({ ...r, not_aim: other.has(r.id) })),
    '/api/upload': (req: HttpRequest<unknown>) => {
      const name = req.params.get('name') ?? '';
      const id = `uploads/${name}`;
      list = [recording({ id, scenario: name.split(' - ')[0], uploaded: true }), ...list];
      return { id, saved: name };
    },
    '/api/label_queue': () =>
      list.filter((r) => !skipped.has(r.id) && !other.has(r.id)).map((r) => r.id),
    '/api/label_skip': (req: HttpRequest<unknown>) => {
      const id = req.params.get('id') ?? '';
      skipped.add(id);
      return { id, skipped: true };
    },
    '/api/not_aim': (req: HttpRequest<unknown>) => {
      const id = req.params.get('id') ?? '';
      const on = req.params.get('on') === '1';
      if (on) other.add(id);
      else other.delete(id);
      return { id, not_aim: on };
    },
  };
}

for (const mode of MODE_CASES) {
  describe(`Labelling (${mode.name} mode)`, () => {
    let routes: ApiRoutes;
    beforeEach(() => (routes = fakeServer()));

    async function added(): Promise<string[]> {
      const source = setUp(mode, RecordingSource);
      const files = [new File(['v'], AIR), new File(['v'], BOUNCE)];
      return (await mode.finish(source.add(files), routes)).ids;
    }

    it('queues the recordings, and leaves a skipped one out from then on', async () => {
      const ids = await added();
      const labelling = TestBed.inject(Labelling);
      expect([...(await mode.finish(labelling.queue(), routes))].sort()).toEqual([...ids].sort());
      await mode.finish(labelling.skip(ids[0]), routes);
      expect(await mode.finish(labelling.queue(), routes)).toEqual([ids[1]]);
    });

    it('marks a recording as another game, which leaves the queue, and as an aim trainer again', async () => {
      const ids = await added();
      const labelling = TestBed.inject(Labelling);
      const row = () =>
        TestBed.inject(RecordingSource)
          .recordings()
          .find((r) => r.id === ids[1]);
      await mode.finish(labelling.setNotAim(ids[1], true), routes);
      expect(row()?.not_aim).toBe(true);
      expect(await mode.finish(labelling.queue(), routes)).toEqual([ids[0]]);
      await mode.finish(labelling.setNotAim(ids[1], false), routes);
      expect(row()?.not_aim).toBe(false);
    });

    it('keeps the area examples where the browser keeps them, and not where the server does', () => {
      const labelling = setUp(mode, Labelling);
      expect(labelling.examples === null).toBe(mode.name === 'server');
    });
  });
}

/**
 * The review service's two area finder files as the page reaches them: area_examples.jsonl through its route, and
 * area_kinds.json in its data folder (kept with no types at first).
 */
function fakeFinderFiles(): ApiRoutes {
  let examples = '';
  let kinds = '[]';
  return {
    '/api/area_examples': (req: HttpRequest<unknown>) => {
      if (req.method === 'POST') examples = req.body as string;
      return req.method === 'POST'
        ? { examples: examples.split('\n').filter(Boolean).length }
        : examples;
    },
    '/files/data/area_kinds.json': (req: HttpRequest<unknown>) => {
      if (req.method === 'PUT') kinds = req.body as string;
      return req.method === 'PUT' ? null : kinds;
    },
  };
}

describe('Labelling: the area examples the browser keeps', () => {
  const [browser] = MODE_CASES;
  let routes: ApiRoutes;
  beforeEach(() => (routes = fakeFinderFiles()));
  const served = <T>(call: Promise<T>) => browser.finish(call, routes);

  it('loads the review server’s files, and downloads them as it wrote them', async () => {
    const store = setUp(browser, Labelling).examples;
    if (!store) throw new Error('the browser keeps the examples');
    const files = [
      new File([EXAMPLES], 'area_examples.jsonl'),
      new File([KINDS], 'area_kinds.json'),
    ];
    expect(await served(store.load(files))).toEqual({ examples: 2, kinds: 2, refused: [] });
    expect(store.count()).toEqual({ examples: 2, recordings: 1, kinds: 2 });
    expect(store.fileNames).toEqual(['area_examples.jsonl', 'area_kinds.json']);
    expect(await (await served(store.file('area_examples.jsonl'))).text()).toBe(EXAMPLES);
    expect(await (await served(store.file('area_kinds.json'))).text()).toBe(KINDS);
  });

  it('replaces the examples of the recordings in a file, and keeps the others', async () => {
    const store = setUp(browser, Labelling).examples;
    if (!store) throw new Error('the browser keeps the examples');
    const other = '{"rec": "Air/a.mp4", "feat": [0.5], "kind": "timer"}\n';
    await served(store.load([new File([EXAMPLES + other], 'area_examples.jsonl')]));
    await served(store.load([new File([EXAMPLES.split('\n')[0]], 'area_examples.jsonl')]));
    expect(store.count()).toMatchObject({ examples: 2, recordings: 2 });
  });

  it('turns down a file that is not one of the review server’s', async () => {
    const store = setUp(browser, Labelling).examples;
    if (!store) throw new Error('the browser keeps the examples');
    const done = await served(
      store.load([
        new File(['{"rec": 1}\n'], 'area_examples.jsonl'),
        new File(['{}'], 'area_kinds.json'),
        new File(['x'], 'notes.txt'),
      ]),
    );
    expect(done.examples + done.kinds).toBe(0);
    expect(done.refused).toEqual([
      'area_examples.jsonl (line 1 is not an area example)',
      'area_kinds.json (it is not a list of area types with ids)',
      'notes.txt (not area_examples.jsonl or area_kinds.json)',
    ]);
  });
});
