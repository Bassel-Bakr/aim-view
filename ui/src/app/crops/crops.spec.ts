import { HttpRequest } from '@angular/common/http';
import { HttpTestingController } from '@angular/common/http/testing';
import { afterEveryRender, EnvironmentInjector } from '@angular/core';
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
  /** A pointer event of the mouse, with a button (0 the left, 2 the right). */
  const pointer = (type: string, [x, y]: Pixel, button: number, shiftKey: boolean) => {
    const event = new MouseEvent(type, { clientX: x, clientY: y, button, shiftKey });
    Object.defineProperty(event, 'pointerType', { value: 'mouse' });
    canvas.dispatchEvent(event);
  };
  const drag = (from: Pixel, to: Pixel, button = 0, shift = false) => {
    pointer('pointerdown', from, button, shift);
    pointer('pointermove', to, button, shift);
    pointer('pointerup', to, button, shift);
    TestBed.tick();
  };
  const tap = (at: Pixel) => drag(at, at);
  const wheel = ([x, y]: Pixel, deltaY: number) => {
    canvas.dispatchEvent(new WheelEvent('wheel', { clientX: x, clientY: y, deltaY }));
    TestBed.tick();
  };
  const key = (name: string, ctrlKey: boolean) => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: name, ctrlKey }));
    document.dispatchEvent(new KeyboardEvent('keyup', { key: name, ctrlKey }));
    TestBed.tick();
  };
  return { el, draft: TestBed.inject(CropDraft), click, drag, tap, wheel, key, settle };
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
});

describe("the Crops page's stage", () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('duplicates the selection with Ctrl+D, and the wheel over it resizes it', async () => {
    const { draft, click, tap, wheel, key } = await render(fakeServer([]));
    await click('Wrong');
    tap([200, 200]);
    key('d', true);
    expect(draft.selection()).toEqual(['s1']);
    const copy = () => draft.draft()?.shapes.find((shape) => shape.id === 's1');
    expect(copy()?.box).toEqual([206, 206, 20, 20]);
    wheel([206, 206], -100);
    expect(copy()?.box).toEqual([206, 206, 22, 22]);
    wheel([40, 40], -100);
    expect(copy()?.box).toEqual([206, 206, 22, 22]);
  });

  it('pans with the right button and the Pan tool, and draws nothing then', async () => {
    const { draft, click, drag, wheel } = await render(fakeServer([]));
    await click('Wrong');
    wheel([0, 0], -100);
    drag([100, 100], [60, 60], 2);
    expect(draft.draft()?.shapes).toHaveLength(1);
    drag([100, 100], [120, 140]);
    expect(draft.draft()?.shapes.at(-1)?.box).toEqual([120, 128, 16, 32]);
    await click('Pan');
    drag([30, 30], [90, 90]);
    expect(draft.draft()?.shapes).toHaveLength(2);
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

describe('the Crops page state', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('waits for the answers before it shows the first crop not checked', async () => {
    history.replaceState(null, '', `/?page=crops&folder=${PAGE}&set=bars`);
    TestBed.configureTestingModule({ providers: serverMode() });
    const draft = TestBed.inject(CropDraft);
    const http = TestBed.inject(HttpTestingController);
    const reply = async (url: string, body: unknown) => {
      TestBed.tick();
      http.expectOne((req) => req.url === url).flush(body as object);
      await new Promise((resolve) => setTimeout(resolve));
      TestBed.tick();
    };
    await reply('/api/crops', [crop('c1'), crop('c2')]);
    await reply('/api/crop_answers', { c1: OLD });
    expect(draft.crop()?.id).toBe('c2');
  });

  it('opens the crop a link names, checked or not, and keeps the one on show in the address', async () => {
    history.replaceState(null, '', `/?page=crops&folder=${PAGE}&set=bars&crop=c1`);
    const { draft } = await render(fakeServer([]));
    expect(draft.crop()?.id).toBe('c1');
    draft.go(1);
    TestBed.tick();
    expect(new URLSearchParams(location.search).get('crop')).toBe('c2');
  });
});

describe("the Crops page's perfect circles and squares", () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('draws a perfect circle with Shift, and evens a selected shape with Equal sides', async () => {
    const { draft, click, drag } = await render(fakeServer([]));
    await click('Wrong');
    drag([10, 10], [30, 50], 0, true);
    expect(draft.draft()?.shapes.at(-1)?.box).toEqual([30, 30, 40, 40]);
    drag([120, 120], [140, 160]);
    expect(draft.draft()?.shapes.at(-1)?.box).toEqual([130, 140, 20, 40]);
    await click('Equal sides');
    expect(draft.draft()?.shapes.at(-1)?.box).toEqual([130, 140, 30, 30]);
  });

  it('draws a polygon of its sides, places it vertex by vertex and makes it uniform again', async () => {
    const { draft, click, drag } = await render(fakeServer([]));
    await click('Wrong');
    await click('Polygon');
    drag([100, 100], [140, 140]);
    const last = () => draft.draft()?.shapes.at(-1);
    expect(last()).toMatchObject({
      kind: 'polygon',
      sides: 6,
      points: null,
      box: [120, 120, 40, 40],
    });
    await click('+');
    expect(last()?.sides).toBe(7);
    await click('Each vertex');
    expect(last()?.points).toHaveLength(7);
    drag([120, 100], [120, 90]);
    expect(last()?.points?.[0]).toEqual([120, 90]);
    await click('Uniform');
    expect(last()).toMatchObject({ kind: 'polygon', points: null, sides: 7 });
  });

  it('draws an oval with the Oval tool, and makes a selected shape one', async () => {
    const { draft, click, drag } = await render(fakeServer([]));
    await click('Wrong');
    await click('Oval');
    drag([120, 120], [140, 160]);
    expect(draft.draft()?.shapes.at(-1)).toMatchObject({
      kind: 'ellipse',
      box: [130, 140, 20, 40],
    });
    await click('Box');
    expect(draft.draft()?.shapes.at(-1)?.kind).toBe('box');
    await click('Oval');
    expect(draft.draft()?.shapes.at(-1)).toMatchObject({
      kind: 'ellipse',
      solid: null,
      points: null,
    });
  });
});

describe('the Crops page order', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('skips checked crops after a first answer, and goes on in order after one given again', async () => {
    const routes: ApiRoutes = {
      ...fakeServer([]),
      '/api/crops': [crop('c1'), crop('c2'), crop('c3'), crop('c4')],
      '/api/crop_answers': { c2: OLD, c3: OLD },
    };
    const { draft, click } = await render(routes);
    expect(draft.crop()?.id).toBe('c1');
    await click('Right');
    expect(draft.crop()?.id).toBe('c4');
    draft.index.set(0);
    await click('Right');
    expect(draft.crop()?.id).toBe('c2');
  });

  it('with Skip checked off, a first answer goes on in order too', async () => {
    const routes: ApiRoutes = {
      ...fakeServer([]),
      '/api/crops': [crop('c1'), crop('c2'), crop('c3')],
      '/api/crop_answers': { c2: OLD },
    };
    const { draft, click } = await render(routes);
    await click('Skip checked');
    await click('Right');
    expect(draft.crop()?.id).toBe('c2');
    expect(location.search).toContain('order=all');
  });
});

describe("the Crops page's stage and change detection", () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('runs none while the pointer moves over the crop with no button down', async () => {
    const { el, click, settle } = await render(fakeServer([]));
    await click('Wrong');
    let renders = 0;
    afterEveryRender(() => (renders += 1), { injector: TestBed.inject(EnvironmentInjector) });
    await settle();
    const before = renders;
    const canvas = el.querySelector('canvas') as HTMLCanvasElement;
    for (const x of [10, 20, 30])
      canvas.dispatchEvent(new MouseEvent('pointermove', { clientX: x, clientY: 10 }));
    await settle();
    expect(renders).toBe(before);
  });
});
