import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { AreaBox, AreaKind, KeptAreas, KindEdit } from '../../api';
import { answer, ApiRoutes, recording, RouteHandler, serverMode } from '../../fake-api';
import { MODE as BROWSER } from '../../modes/mode.browser';
import { AreaExamples } from '../../modes/web-files/area-examples';
import { builtInKinds, editKinds } from '../../modes/web-files/area-kinds';
import { LocalFiles } from '../../modes/web-files/local-files';
import { LabelQueue } from '../../services/label-queue';
import { Library } from '../../services/library';
import { Review } from '../../services/review';
import { AreaBar } from './area-bar/area-bar';
import { AreaCanvas } from './area-canvas/area-canvas';
import { AreaDraft } from './area-draft';

const ID = 'Air/Air - 1 - 2026.10.01-16.23.03.mp4';
const WEBCAM: AreaBox = [0.1, 0.1, 0.5, 0.5, 'webcam'];
const FOUND: AreaBox = [0.6, 0.6, 0.9, 0.9, 'timer'];

/** A point on the video, in pixels. */
type Pixel = [x: number, y: number];

/** What the fake review server was sent: the areas saved, and the reviews started (their params). */
interface Sent {
  saved: AreaBox[] | null;
  analysed: string[];
}

function fakeServer(sent: Sent): ApiRoutes {
  let kinds: AreaKind[] = builtInKinds();
  return {
    '/api/vods': [recording({ id: ID, analysed: true })],
    '/api/job': { stage: 'none' },
    '/api/report': null,
    '/api/exclude': (req: HttpRequest<unknown>) => {
      if (req.method !== 'POST') return { kinds, boxes: [WEBCAM], source: 'saved' };
      sent.saved = req.body as AreaBox[];
      return { boxes: sent.saved, source: 'saved' };
    },
    '/api/find_areas': {
      boxes: [FOUND],
      examples: 9,
      recordings: 2,
      copied: null,
      by: { learned: 1, rule: 0 },
    },
    '/api/analyse': (req: HttpRequest<unknown>) => {
      sent.analysed.push(`${req.params.get('id')} again=${req.params.get('again')}`);
      return { stage: 'starting' };
    },
    '/api/area_kinds': (req: HttpRequest<unknown>) =>
      (kinds = editKinds(kinds, req.body as KindEdit)),
  };
}

/** The editor open on a recording: its bar, and its canvas as 1000 x 500 pixels on screen. */
async function render(routes: ApiRoutes) {
  TestBed.configureTestingModule({ providers: serverMode() });
  TestBed.inject(Library).selectedId.set(ID);
  const draft = TestBed.inject(AreaDraft);
  draft.start(ID);
  const bar = TestBed.createComponent(AreaBar);
  bar.componentRef.setInput('recording', recording({ id: ID, analysed: true }));
  const screen = TestBed.createComponent(AreaCanvas);
  const canvas = (screen.nativeElement as HTMLElement).querySelector('canvas') as HTMLCanvasElement;
  Object.defineProperty(canvas, 'clientWidth', { value: 1000 });
  Object.defineProperty(canvas, 'clientHeight', { value: 500 });
  canvas.getBoundingClientRect = () => new DOMRect(0, 0, 1000, 500);
  canvas.setPointerCapture = () => undefined;
  const settle = async () => {
    await answer(routes);
    await bar.whenStable();
  };
  await settle();
  const el = bar.nativeElement as HTMLElement;
  const button = (label: string) =>
    [...el.querySelectorAll('button')].find(
      (b) => b.textContent?.trim() === label,
    ) as HTMLButtonElement;
  const pointer = (type: string, x: number, y: number) =>
    canvas.dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, button: 0 }));
  /** A drag on the video, in pixels. */
  const drag = (from: Pixel, to: Pixel) => {
    pointer('pointerdown', ...from);
    pointer('pointermove', ...to);
    pointer('pointerup', ...to);
    TestBed.tick();
  };
  return { draft, el, button, drag, settle };
}

describe('the excluded areas editor', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('opens with the areas saved and says where they come from', async () => {
    const { draft, el } = await render(fakeServer({ saved: null, analysed: [] }));
    expect(draft.boxes()).toEqual([WEBCAM]);
    expect(el.textContent).toContain('Saved for this recording.');
  });

  it('draws a new area, says what it is, and saves it: a reviewed run is tracked again', async () => {
    const sent: Sent = { saved: null, analysed: [] };
    const routes = fakeServer(sent);
    const { draft, el, button, drag, settle } = await render(routes);
    drag([600, 400], [800, 450]);
    expect(draft.boxes()[1]).toEqual([0.6, 0.8, 0.8, 0.9, 'other']);
    expect(draft.selected()).toBe(1);
    const select = el.querySelector('select') as HTMLSelectElement;
    select.value = 'clock';
    select.dispatchEvent(new Event('change'));
    button('Save').click();
    await settle();
    expect(sent.saved).toEqual([WEBCAM, [0.6, 0.8, 0.8, 0.9, 'clock']]);
    expect(sent.analysed).toEqual([`${ID} again=1`]);
    expect(draft.open()).toBe(false);
  });

  it('follows the review a save started (the desktop app tracks again by itself)', async () => {
    const sent: Sent = { saved: null, analysed: [] };
    const server = fakeServer(sent);
    const keep = server['/api/exclude'] as RouteHandler;
    const routes: ApiRoutes = {
      ...server,
      '/api/exclude': (req: HttpRequest<unknown>) => {
        const out = keep(req) as KeptAreas;
        return req.method === 'POST' ? { ...out, job: { stage: 'starting' } } : out;
      },
      '/api/job': () => ({ stage: sent.saved ? 'done' : 'none' }),
    };
    const { button, settle } = await render(routes);
    button('Save').click();
    await settle();
    expect(sent.saved).toEqual([WEBCAM]);
    expect(sent.analysed).toEqual([]);
    // the page followed the review the save started, to its end
    expect(TestBed.inject(Review).job().stage).toBe('done');
  });

  it('moves an area by its inside, resizes it by its edge, and removes it with Delete', async () => {
    const { draft, drag } = await render(fakeServer({ saved: null, analysed: [] }));
    drag([300, 150], [350, 175]);
    expect(draft.boxes()[0].map((v) => (typeof v === 'number' ? +v.toFixed(6) : v))).toEqual([
      0.15,
      0.15,
      0.55,
      0.55,
      'webcam',
    ]);
    // the right edge, at x = 550 px
    drag([550, 150], [700, 150]);
    expect(draft.boxes()[0][2]).toBeCloseTo(0.7);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete' }));
    expect(draft.boxes()).toEqual([]);
  });

  it('takes the finder’s proposal and says how it named the areas', async () => {
    const { draft, el, button, settle } = await render(fakeServer({ saved: null, analysed: [] }));
    button('Find areas').click();
    await settle();
    expect(draft.boxes()).toEqual([FOUND]);
    expect(el.textContent).toContain(
      'Found 1 area: 1 named from what you taught, 0 by rules. Learned from 2 recordings you saved.',
    );
  });

  it('adds a type of the user’s own, which becomes the selected area’s', async () => {
    const { draft, el, settle } = await render(fakeServer({ saved: null, analysed: [] }));
    draft.select(0);
    await settle();
    const select = el.querySelector('select') as HTMLSelectElement;
    select.value = '__add';
    select.dispatchEvent(new Event('change'));
    await settle();
    const name = el.querySelector('input[aria-label="Type name"]') as HTMLInputElement;
    name.value = 'Kill feed';
    name.dispatchEvent(new Event('input'));
    await settle();
    // the form's submit (the test browser does not submit a form from its button)
    el.querySelector('form')?.dispatchEvent(new Event('submit', { cancelable: true }));
    await settle();
    expect(draft.kinds().at(-1)?.name).toBe('Kill feed');
    expect(draft.boxes()[0][4]).toBe('kill_feed');
    expect(el.querySelector('form')).toBeNull();
  });

  it('opens on each recording of the labelling queue with the finder’s proposal; Save and next moves on', async () => {
    const next = 'Air/Air - 2 - 2026.10.02-09.00.00.mp4';
    const sent: Sent = { saved: null, analysed: [] };
    const found: string[] = [];
    const routes: ApiRoutes = {
      ...fakeServer(sent),
      '/api/vods': [recording({ id: ID }), recording({ id: next, analysed: false })],
      '/api/label_queue': [ID, next],
      '/api/find_areas': (req: HttpRequest<unknown>) => {
        found.push(req.params.get('id') ?? '');
        return { boxes: [FOUND], examples: 9, recordings: 2, copied: null, by: { learned: 1 } };
      },
    };
    const { draft, button, settle } = await render(routes);
    draft.stop();
    void TestBed.inject(LabelQueue).start();
    await settle();
    expect(draft.editing()).toBe(ID);
    expect(draft.boxes()).toEqual([FOUND]);
    // the next recording's areas are found meanwhile
    expect([...found].sort()).toEqual([ID, next]);
    button('Save and next').click();
    await settle();
    expect(sent.saved).toEqual([FOUND]);
    expect(TestBed.inject(Library).selectedId()).toBe(next);
    expect(draft.editing()).toBe(next);
  });

  it('closes when the labelling queue ends, and Cancel ends the queue', async () => {
    const routes: ApiRoutes = {
      ...fakeServer({ saved: null, analysed: [] }),
      '/api/label_queue': [ID],
    };
    const { draft, button, settle } = await render(routes);
    draft.stop();
    const queue = TestBed.inject(LabelQueue);
    void queue.start();
    await settle();
    expect(draft.editing()).toBe(ID);
    queue.stop();
    TestBed.tick();
    expect(draft.open()).toBe(false);
    void queue.start();
    await settle();
    button('Cancel').click();
    TestBed.tick();
    expect(queue.active()).toBe(false);
    expect(draft.open()).toBe(false);
  });

  it('in the browser, follows the kinds loaded from a kinds file, keeping the areas drawn', async () => {
    TestBed.configureTestingModule({ providers: BROWSER.providers });
    const local = TestBed.inject(LocalFiles);
    const [id] = (await local.add([new File(['v'], 'Air - 1 - 2026.10.01-16.23.03.mp4')])).ids;
    TestBed.inject(Library).selectedId.set(id);
    const draft = TestBed.inject(AreaDraft);
    draft.start(id);
    const ready = async () => {
      for (let i = 0; i < 20; i++) {
        TestBed.tick();
        await new Promise((r) => setTimeout(r));
      }
    };
    await ready();
    expect(draft.ready()).toBe(true);
    draft.add([0.2, 0.2, 0.3, 0.3]);
    const examples = TestBed.inject(AreaExamples);
    await examples.setKinds([...draft.kinds(), { id: 'kill_feed', name: 'Kill feed', about: '' }]);
    await ready();
    expect(draft.kinds().at(-1)?.name).toBe('Kill feed');
    expect(draft.boxes().at(-1)).toEqual([0.2, 0.2, 0.3, 0.3, 'other']);
  });

  it('closes with Escape, dropping what was not saved', async () => {
    const { draft, drag } = await render(fakeServer({ saved: null, analysed: [] }));
    drag([600, 400], [800, 450]);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(draft.open()).toBe(false);
  });
});
