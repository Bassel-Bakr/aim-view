import { afterEveryRender, Component, EnvironmentInjector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ClickReport, TrackReport } from '../../api';
import { answer, serverMode } from '../../fake-api';
import { AreaCanvas } from '../areas/area-canvas/area-canvas';
import { Library } from '../../services/library';
import { AreaDraft } from '../areas/area-draft';
import { markPositions, Player } from './player';

describe('markPositions', () => {
  it('marks each kill of a clicking run, as a share of the video', () => {
    const report = {
      mode: 'click',
      fps: 100,
      flicks: [{ kill_frame: 500 }, { kill_frame: 1500 }],
    } as unknown as ClickReport;
    expect(markPositions(report, 20)).toEqual([25, 75]);
  });

  it("marks each bot's death in a tracking run", () => {
    const report = {
      mode: 'track',
      fps: 100,
      summary: { switches: [[1000, 1100]] },
    } as unknown as TrackReport;
    expect(markPositions(report, 20)).toEqual([50]);
  });

  it('marks nothing before the video has a length', () => {
    expect(
      markPositions({ mode: 'click', fps: 100, flicks: [] } as unknown as ClickReport, 0),
    ).toEqual([]);
  });
});

const ID = 'Air/Air - 1 - 2026.10.01-16.23.03.mp4';

/** The player on a page: a field in a panel above the video, and the excluded areas editor over it. */
@Component({
  imports: [Player, AreaCanvas],
  template: `
    <app-player src="blob:test/video">
      <input panel type="text" aria-label="A panel's field" />
      <app-area-canvas screen />
    </app-player>
  `,
})
class Host {}

/** The player with the areas editor open, and the browser's full screen, which jsdom lacks, standing in. */
async function render() {
  TestBed.configureTestingModule({ providers: serverMode() });
  TestBed.inject(Library).selectedId.set(ID);
  const draft = TestBed.inject(AreaDraft);
  draft.start(ID);
  const fixture = TestBed.createComponent(Host);
  // every request (the editor's areas) answered with a 404: the keys do not need them
  const settle = () => answer({});
  await settle();
  const el = fixture.nativeElement as HTMLElement;
  const player = el.querySelector('app-player') as HTMLElement;
  let shown: Element | null = null;
  const change = async (to: Element | null) => {
    shown = to;
    document.dispatchEvent(new Event('fullscreenchange'));
    await settle();
  };
  Object.defineProperty(document, 'fullscreenElement', { configurable: true, get: () => shown });
  player.requestFullscreen = () => change(player);
  document.exitFullscreen = () => change(null);
  const button = el.querySelector('.full-toggle') as HTMLButtonElement;
  const field = el.querySelector('input[panel]') as HTMLInputElement;
  const press = async (
    key: string,
    target: EventTarget = document,
    init: KeyboardEventInit = {},
  ) => {
    const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
    target.dispatchEvent(event);
    await settle();
    return event;
  };
  const full = () => document.fullscreenElement === player;
  return { draft, settle, player, button, field, press, change, full };
}

/** The player as above, where the browser refuses full screen; the page's top layer, which jsdom lacks, standing in. */
async function renderRefused() {
  const page = await render();
  const { player } = page;
  player.requestFullscreen = () => Promise.reject(new TypeError('Permissions check failed'));
  let shown = false;
  player.showPopover = () => {
    shown = true;
  };
  player.hidePopover = () => {
    shown = false;
  };
  const filling = () => shown && player.getAttribute('popover') === 'manual';
  return { ...page, filling };
}

describe('Player full screen', () => {
  afterEach(() => {
    Reflect.deleteProperty(document, 'fullscreenElement');
    Reflect.deleteProperty(document, 'exitFullscreen');
  });

  it('fills the screen with the whole player from the button, and leaves it from the button', async () => {
    const { settle, button, full } = await render();
    button.click();
    await settle();
    expect(full()).toBe(true);
    expect(button.getAttribute('aria-pressed')).toBe('true');
    button.click();
    await settle();
    expect(full()).toBe(false);
    expect(button.getAttribute('aria-pressed')).toBe('false');
  });

  it('goes full screen with F and leaves it with F', async () => {
    const { press, full } = await render();
    await press('f');
    expect(full()).toBe(true);
    await press('F', document, { shiftKey: true });
    expect(full()).toBe(false);
  });

  it('leaves full screen with Escape and keeps the areas editor open; out of it, Escape closes the editor', async () => {
    const { draft, press, full } = await render();
    await press('f');
    const escape = await press('Escape');
    expect(full()).toBe(false);
    expect(escape.defaultPrevented).toBe(true);
    expect(draft.open()).toBe(true);
    await press('Escape');
    expect(draft.open()).toBe(false);
  });

  it('leaves F typed into a field, and Ctrl F, to the field and the browser', async () => {
    const { field, press, full } = await render();
    await press('f', field);
    await press('f', document, { ctrlKey: true });
    expect(full()).toBe(false);
  });

  it('follows the browser when it ends full screen itself (its own Escape)', async () => {
    const { button, change, press, full } = await render();
    await press('f');
    expect(button.getAttribute('aria-pressed')).toBe('true');
    await change(null);
    expect(full()).toBe(false);
    expect(button.getAttribute('aria-pressed')).toBe('false');
  });
});

describe('Player filling the window', () => {
  afterEach(() => {
    Reflect.deleteProperty(document, 'fullscreenElement');
    Reflect.deleteProperty(document, 'exitFullscreen');
  });

  it('fills the window when the browser refuses full screen, and leaves it from the button', async () => {
    const { settle, player, button, filling } = await renderRefused();
    button.click();
    await settle();
    expect(filling()).toBe(true);
    expect(button.getAttribute('aria-pressed')).toBe('true');
    button.click();
    await settle();
    expect(filling()).toBe(false);
    expect(player.hasAttribute('popover')).toBe(false);
    expect(button.getAttribute('aria-pressed')).toBe('false');
  });

  it('fills it with F and leaves it with F', async () => {
    const { press, filling } = await renderRefused();
    await press('f');
    expect(filling()).toBe(true);
    await press('f');
    expect(filling()).toBe(false);
  });

  it('leaves it with Escape and keeps the areas editor open', async () => {
    const { draft, press, filling } = await renderRefused();
    await press('f');
    const escape = await press('Escape');
    expect(filling()).toBe(false);
    expect(escape.defaultPrevented).toBe(true);
    expect(draft.open()).toBe(true);
  });
});

describe('Player hover', () => {
  it('runs no change detection while the mouse moves over the video', async () => {
    const { player, settle } = await render();
    let renders = 0;
    afterEveryRender(() => (renders += 1), { injector: TestBed.inject(EnvironmentInjector) });
    await settle();
    const before = renders;
    const screen = player.querySelector('.screen') as HTMLElement;
    for (const x of [10, 20, 30]) {
      screen.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: 10, bubbles: true }));
    }
    screen.dispatchEvent(new MouseEvent('mouseleave'));
    await settle();
    expect(renders).toBe(before);
  });
});
