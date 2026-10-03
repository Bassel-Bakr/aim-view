import { HttpRequest } from '@angular/common/http';
import { Provider, ResourceRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  AreaBox,
  AreaKind,
  errorMessage,
  FoundAreas,
  KindEdit,
  Recording,
  RecordingAreas,
} from '../api';
import { ApiRoutes, recording, Refused } from '../fake-api';
import { FinderResult } from '../modes/wasm/area-finder-messages';
import { BrowserAreaFinder } from '../modes/wasm/browser-area-finder';
import { AreasFindRequest, CoreModule } from '../modes/wasm/core-module';
import {
  builtInKinds,
  editKinds,
  kovobsLayout,
  validAreas,
  withKindIds,
} from '../modes/web-files/area-kinds';
import { AreaLabels } from './area-labels';
import { MODE_CASES, ModeCase, setUp } from './contract-case';
import { RecordingSource } from './recording-source';

const NAME = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const OTHER_NAME = 'Air - 1 - 2026.10.02-09.00.00.mp4';
const WEBCAM: AreaBox = [0.75, 0.7, 1, 1, 'webcam'];
const DRAWN: AreaBox = [0.1, 0.1, 0.2, 0.2, 'other'];
/** The finder's proposal: the webcam. */
const PROPOSAL: FoundAreas = {
  boxes: [WEBCAM],
  examples: 3,
  recordings: 1,
  copied: null,
  by: { learned: 1, rule: 0 },
};
/** What the browser's area finder read in a recording: the webcam. */
const FOUND: FinderResult = {
  frames: 90,
  areas: [{ box: [0.75, 0.7, 1, 1], feat: [], rule: 'webcam' }],
  maps: null,
};

/**
 * The browser's area finder and core, standing in for its worker and the WebAssembly as the fake server does for the
 * review server: the finder reads FOUND in any recording, and the core proposes the areas it is given.
 */
function browserFinder(): Provider[] {
  const propose = (r: AreasFindRequest): Promise<FoundAreas> =>
    Promise.resolve({
      ...PROPOSAL,
      boxes: r.found.map(({ box: [x0, y0, x1, y1], rule }): AreaBox => [x0, y0, x1, y1, rule]),
    });
  return [
    { provide: BrowserAreaFinder, useValue: { result: () => Promise.resolve(FOUND) } },
    { provide: CoreModule, useValue: { areasFind: propose } },
  ];
}

/** KovOBS's layout as python/server.py gives it for ?layout=kovobs: its kinds by name (review.OVERLAY_SHARES). */
function layoutByName(kinds: AreaKind[]): AreaBox[] {
  return kovobsLayout().map(([x0, y0, x1, y1, id]) => {
    const name = kinds.find((k) => k.id === id)?.name ?? id;
    return [x0, y0, x1, y1, name];
  });
}

/** A review server that keeps areas and kinds the way python/server.py does, with a finder that proposes one area. */
function fakeServer(): ApiRoutes {
  let kinds = builtInKinds();
  let list: Recording[] = [];
  const saved = new Map<string, AreaBox[]>();
  let lastUpload: AreaBox[] | null = null;
  const fail = (error: string) => new Refused(error);
  return {
    '/api/vods': () => list,
    '/api/upload': (req: HttpRequest<unknown>) => {
      const id = `uploads/${req.params.get('name') ?? ''}`;
      list = [recording({ id, stats: false, analysed: false }), ...list];
      return { id, saved: req.params.get('name') };
    },
    '/api/exclude': (req: HttpRequest<unknown>) => {
      const id = req.params.get('id') ?? '';
      if (req.method === 'POST') {
        if (!validAreas(req.body)) return fail('boxes: a list of [x0, y0, x1, y1, type id]');
        const boxes = withKindIds(req.body, kinds);
        saved.set(id, boxes);
        if (id.startsWith('uploads/')) lastUpload = boxes;
        return { boxes, source: 'saved' };
      }
      if (req.params.get('layout') === 'kovobs') {
        return { kinds, boxes: layoutByName(kinds), source: 'kovobs' };
      }
      const own = saved.get(id);
      if (own) return { kinds, boxes: own, source: 'saved' };
      if (id.startsWith('uploads/') && lastUpload) {
        return { kinds, boxes: lastUpload, source: 'last upload' };
      }
      return { kinds, boxes: kovobsLayout(), source: 'kovobs' };
    },
    '/api/area_kinds': (req: HttpRequest<unknown>) => {
      try {
        kinds = editKinds(kinds, req.body as KindEdit);
        return kinds;
      } catch (e) {
        return fail(e instanceof Error ? e.message : String(e));
      }
    },
    '/api/find_areas': () => PROPOSAL,
  };
}

/** Why a call was refused, in words (the server's own words for a refusal); throws when it was not. */
async function refusal(call: Promise<unknown>): Promise<string> {
  try {
    await call;
  } catch (e) {
    return errorMessage(e);
  }
  throw new Error('it was not refused');
}

/** The resource's value once it has one, while the fake server answers. */
async function settled(
  mode: ModeCase,
  ref: ResourceRef<RecordingAreas | undefined>,
  routes: ApiRoutes,
): Promise<RecordingAreas | undefined> {
  const read = (async () => {
    for (;;) {
      TestBed.tick();
      await new Promise((r) => setTimeout(r));
      if (ref.error()) throw ref.error();
      if (ref.hasValue() && !ref.isLoading()) return ref.value();
    }
  })();
  return mode.finish(read, routes);
}

for (const mode of MODE_CASES) {
  describe(`AreaLabels (${mode.name} mode)`, () => {
    let routes: ApiRoutes;
    beforeEach(() => (routes = fakeServer()));

    /** The mode's services; each test sets them up once. */
    const labels = () => setUp(mode, AreaLabels);

    async function open(name = NAME): Promise<string> {
      const source = TestBed.inject(RecordingSource);
      return (await mode.finish(source.add([new File(['v'], name)]), routes)).ids[0];
    }

    function read(id: string): Promise<RecordingAreas | undefined> {
      const service = TestBed.inject(AreaLabels);
      const ref = TestBed.runInInjectionContext(() => service.areas(() => id));
      return settled(mode, ref, routes);
    }

    it("starts a recording from KovOBS's layout, with every kind", async () => {
      const service = labels();
      const id = await open();
      const areas = await read(id);
      expect(areas?.source).toBe('kovobs');
      expect(areas?.boxes).toEqual(kovobsLayout());
      expect(areas?.kinds.map((k) => k.name)).toContain('Zoomed crosshair');
      const layout = await mode.finish(service.layout(), routes);
      expect(layout).toEqual({ boxes: kovobsLayout(), source: 'kovobs' });
    });

    it('keeps the areas saved, and the next added recording starts from them', async () => {
      const service = labels();
      const id = await open();
      const kept = await mode.finish(service.save(id, [WEBCAM, DRAWN]), routes);
      expect(kept).toEqual({ boxes: [WEBCAM, DRAWN], source: 'saved' });
      expect(await read(id)).toMatchObject({ boxes: [WEBCAM, DRAWN], source: 'saved' });
      const other = await open(OTHER_NAME);
      expect(await read(other)).toMatchObject({ boxes: [WEBCAM, DRAWN], source: 'last upload' });
    });

    it('turns down areas outside the frame', async () => {
      const service = labels();
      const id = await open();
      const why = await refusal(
        mode.finish(service.save(id, [[0.5, 0.5, 0.4, 1.2, 'other']]), routes),
      );
      expect(why).toContain('x0, y0, x1, y1');
    });

    it('adds a kind with an id from its name, renames it, and turns down a name taken', async () => {
      const service = labels();
      await open();
      const added = await mode.finish(
        service.saveKind({ id: null, name: 'Kill feed', about: '' }),
        routes,
      );
      expect(added.at(-1)).toEqual({ id: 'kill_feed', name: 'Kill feed', about: '' });
      const renamed = await mode.finish(
        service.saveKind({ id: 'kill_feed', name: 'Feed', about: 'who killed whom' }),
        routes,
      );
      expect(renamed.find((k) => k.id === 'kill_feed')?.name).toBe('Feed');
      const why = await refusal(
        mode.finish(service.saveKind({ id: null, name: 'timer', about: '' }), routes),
      );
      expect(why).toBe('there is a type called timer already');
    });

    it('proposes areas for a recording not reviewed (the browser reads its frames first)', async () => {
      const extra = mode.name === 'browser' ? browserFinder() : [];
      TestBed.configureTestingModule({ providers: [...mode.providers(), ...extra] });
      const service = TestBed.inject(AreaLabels);
      const id = await open();
      const found = await mode.finish(service.find(id, true), routes);
      expect(found.boxes).toEqual([WEBCAM]);
    });
  });
}
