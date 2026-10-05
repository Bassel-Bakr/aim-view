import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { CropAnswer, CropEntry, CropPage, SceneView } from '../api';
import { answer, ApiRoutes, serverMode } from '../fake-api';
import { CoreModule } from '../modes/wasm/core-module';
import { CropDraft } from './crop-draft';
import { Crops } from './crops';

const PAGE = 'check_bars';
const PAGES: CropPage[] = [
  {
    page: PAGE,
    sets: [{ set: 'bars', title: 'Bars', crossedOut: null, learn: true, count: 2, answered: 1 }],
  },
];

/** A crop for the page: one model box at (200, 200). */
function crop(id: string): CropEntry {
  return {
    id,
    set: 'bars',
    file: `${id}.mp4`,
    folder: 'Air',
    kind: 'static',
    why: [],
    rule: null,
    boxes: [[200, 200, 20, 20]],
    scores: [0.9],
  };
}

const OLD: CropAnswer = {
  verdict: 'right',
  set: 'bars',
  file: 'c1.mp4',
  at: 1,
  remove: [],
  edit: {},
  add: [],
};

/** A point on the stage, in CSS pixels (the stage is 256 pixels square: one per crop pixel). */
type Pixel = [x: number, y: number];

function fakeServer(posted: CropAnswer[]): ApiRoutes {
  return {
    '/api/crop_pages': PAGES,
    '/api/crops': [crop('c1'), crop('c2')],
    '/api/crop_answers': { c1: OLD },
    '/api/crop_image': new Blob(['png']),
    '/api/crop_answer': (req: HttpRequest<unknown>) => {
      posted.push(req.body as CropAnswer);
      return req.body;
    },
  };
}

/** The page on the review server, its stage 256 pixels square, the core's view of a scene left empty. */
async function render(routes: ApiRoutes) {
  const empty: SceneView = { targets: [], mask: [] };
  TestBed.configureTestingModule({
    providers: [
      ...serverMode(),
      { provide: CoreModule, useValue: { shapesVisible: () => Promise.resolve(empty) } },
    ],
  });
  const page = TestBed.createComponent(Crops);
  const settle = async () => {
    await answer(routes);
    await page.whenStable();
  };
  await settle();
  const el = page.nativeElement as HTMLElement;
  const canvas = el.querySelector('canvas') as HTMLCanvasElement;
  Object.defineProperty(canvas, 'clientWidth', { value: 256 });
  canvas.getBoundingClientRect = () => new DOMRect(0, 0, 256, 256);
  canvas.setPointerCapture = () => undefined;
  const click = async (label: string) => {
    const button = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
    button?.click();
    await settle();
  };
  const pointer = (type: string, [x, y]: Pixel) =>
    canvas.dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, button: 0 }));
  const drag = (from: Pixel, to: Pixel) => {
    pointer('pointerdown', from);
    pointer('pointermove', to);
    pointer('pointerup', to);
    TestBed.tick();
  };
  const tap = (at: Pixel) => drag(at, at);
  return { el, draft: TestBed.inject(CropDraft), click, drag, tap, settle };
}

describe('the Crops page', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('opens the first folder with crops to check, at the first crop not checked', async () => {
    const { el, draft } = await render(fakeServer([]));
    expect(draft.folder()).toBe(PAGE);
    expect(draft.crop()?.id).toBe('c2');
    expect(el.textContent).toContain('1 / 2 checked');
    expect(location.search).toContain('folder=check_bars');
  });

  it('saves a fix: a drawn pill joined to the model shape, as one target with a head', async () => {
    const posted: CropAnswer[] = [];
    const { draft, click, drag, tap } = await render(fakeServer(posted));
    await click('Wrong');
    drag([190, 150], [210, 180]);
    const drawn = draft.selection()[0];
    expect(draft.draft()?.shapes.find((shape) => shape.id === drawn)?.box).toEqual([
      200, 165, 20, 30,
    ]);
    tap([200, 200]);
    expect(draft.selection()).toEqual([drawn, 'm0']);
    await click('Join');
    tap([200, 200]);
    await click('Head');
    await click('Save');
    const [saved] = posted;
    expect(saved.verdict).toBe('wrong');
    expect(saved.scene?.targets).toEqual([[drawn, 'm0']]);
    expect(saved.scene?.shapes.find((shape) => shape.id === drawn)?.role).toBe('head');
    expect(saved.add).toEqual([[200, 165, 20, 30]]);
    expect(draft.mode()).toBe('view');
  });

  it("says Right for the model's shapes, and every crop is checked", async () => {
    const posted: CropAnswer[] = [];
    const { el, click } = await render(fakeServer(posted));
    await click('Right');
    expect(posted.map((one) => [one.verdict, one.file, one.suggested])).toEqual([
      ['right', 'c2.mp4', undefined],
    ]);
    expect(el.textContent).toContain('Every crop of this set is checked.');
  });
});
