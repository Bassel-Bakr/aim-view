import { HttpRequest } from '@angular/common/http';
import { ResourceRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { CropAnswer, CropAnswers, CropEntry, CropPage } from '../api';
import { ApiRoutes } from '../fake-api';
import { MODE_CASES, ModeCase, setUp } from './contract-case';
import { CropSets } from './crop-sets';

const PAGE = 'check_bars';
const PAGES: CropPage[] = [
  {
    page: PAGE,
    sets: [{ set: 'bars', title: 'Bars', crossedOut: null, learn: true, count: 1, answered: 1 }],
  },
];
const CROP: CropEntry = {
  id: 'c1',
  set: 'bars',
  file: 'a.mp4',
  folder: 'Air',
  kind: 'static',
  why: ['bar'],
  rule: null,
  boxes: [[100, 100, 20, 20]],
  scores: [0.9],
};
const ANSWER: CropAnswer = {
  verdict: 'right',
  set: 'bars',
  file: 'a.mp4',
  at: 5,
  remove: [],
  edit: {},
  add: [],
};

/** The review server's crop routes, keeping what is posted to them. */
function fakeServer(posted: unknown[]): ApiRoutes {
  return {
    '/api/crop_pages': PAGES,
    '/api/crops': (req: HttpRequest<unknown>) => (req.params.get('set') === 'bars' ? [CROP] : []),
    '/api/crop_answers': { c1: ANSWER },
    '/api/crop_image': new Blob(['png']),
    '/api/crop_answer': (req: HttpRequest<unknown>) => {
      posted.push({ page: req.params.get('page'), id: req.params.get('id'), body: req.body });
      return { ...(req.body as CropAnswer), at: 7 };
    },
    '/api/crop_export': { page: PAGE, answers: { c1: ANSWER } },
    '/api/crop_import': (req: HttpRequest<unknown>) => {
      posted.push(req.body);
      return { written: 1, kept: 0, unknown: 0 };
    },
    [`/files/data/crops/${PAGE}`]: { copied: 2 },
  };
}

/** A resource's value once it has loaded. */
async function settled<T>(mode: ModeCase, ref: ResourceRef<T | undefined>, routes: ApiRoutes) {
  const read = (async () => {
    for (;;) {
      TestBed.tick();
      await new Promise((resolve) => setTimeout(resolve));
      if (ref.error()) throw ref.error();
      if (ref.hasValue() && !ref.isLoading()) return ref.value();
    }
  })();
  return mode.finish(read, routes);
}

/** A file of a folder the user picked, with its path in it. */
function picked(path: string): File {
  const file = new File(['{}'], path.split('/').at(-1) ?? path);
  Object.defineProperty(file, 'webkitRelativePath', { value: path });
  return file;
}

for (const mode of MODE_CASES) {
  describe(`CropSets (${mode.name} mode)`, () => {
    let posted: unknown[];
    let routes: ApiRoutes;
    beforeEach(() => {
      posted = [];
      routes = fakeServer(posted);
    });

    it("lists the check folders, a set's crops and their answers", async () => {
      const sets = setUp(mode, CropSets);
      const [pages, crops, answers] = TestBed.runInInjectionContext(() => [
        sets.pages(),
        sets.crops(
          () => PAGE,
          () => 'bars',
        ),
        sets.answers(
          () => PAGE,
          () => 'bars',
        ),
      ]);
      expect(await settled(mode, pages, routes)).toEqual(PAGES);
      expect(await settled(mode, crops, routes)).toEqual([CROP]);
      expect(await settled(mode, answers, routes)).toEqual({ c1: ANSWER });
    });

    it("keeps a crop's answer in its folder, and gives its picture", async () => {
      const sets = setUp(mode, CropSets);
      const kept = await mode.finish(sets.save(PAGE, 'c1', ANSWER), routes);
      expect(kept.at).toBe(7);
      expect(posted).toEqual([{ page: PAGE, id: 'c1', body: ANSWER }]);
      const picture = await mode.finish(sets.image(PAGE, 'c1'), routes);
      expect(await picture.text()).toBe('png');
    });

    it('moves the answers between modes as one document', async () => {
      const sets = setUp(mode, CropSets);
      const document: CropAnswers = await mode.finish(sets.exportAnswers(PAGE), routes);
      expect(document.answers).toEqual({ c1: ANSWER });
      const done = await mode.finish(sets.importAnswers(PAGE, document), routes);
      expect(done).toEqual({ written: 1, kept: 0, unknown: 0 });
      expect(posted).toEqual([document]);
    });

    it('adds a check folder in the browser, and none where the server reads them', async () => {
      const sets = setUp(mode, CropSets);
      const add = sets.addFolder;
      if (mode.name !== 'browser') {
        expect(add).toBeNull();
        return;
      }
      if (!add) throw new Error('browser mode adds folders');
      const files = [`${PAGE}/crops.json`, `${PAGE}/crops/c1.png`, `${PAGE}/answers/x.json`].map(
        picked,
      );
      expect(await mode.finish(add(files), routes)).toBe(PAGE);
      await expect(mode.finish(add([picked(`${PAGE}/crops/c1.png`)]), routes)).rejects.toThrow(
        /no crops.json/,
      );
    });
  });
}
