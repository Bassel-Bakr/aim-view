import { ComponentFixture, TestBed } from '@angular/core/testing';
import { Library } from '../services/library';
import { fakeFetch, recording } from '../fake-api';
import { Recordings } from './recordings';

const RECORDINGS = [
  recording({ id: 's1', scenario: '1wall 6targets', kind: 'static' }),
  recording({ id: 't1', scenario: 'Controlsphere', kind: 'tracking', analysed: false }),
  recording({ id: 't2', scenario: 'Air Tracking 180', kind: 'tracking', stats: false }),
];

async function render(): Promise<ComponentFixture<Recordings>> {
  vi.stubGlobal('fetch', fakeFetch({ '/api/vods': RECORDINGS }));
  const fixture = TestBed.createComponent(Recordings);
  await fixture.whenStable();
  return fixture;
}

const el = (f: ComponentFixture<Recordings>) => f.nativeElement as HTMLElement;
const texts = (f: ComponentFixture<Recordings>, selector: string) =>
  [...el(f).querySelectorAll(selector)].map((n) => n.textContent?.replace(/\s+/g, ' ').trim());

describe('Recordings', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    history.replaceState(null, '', '/');
  });

  it('offers a chip per kind of run, with its count', async () => {
    const f = await render();
    expect(texts(f, '.chip')).toEqual(['All 3', 'Static 1', 'Tracking 2']);
  });

  it('filters by kind and by name', async () => {
    const f = await render();
    (el(f).querySelectorAll('.chip')[2] as HTMLButtonElement).click();
    await f.whenStable();
    expect(texts(f, '.name')).toEqual(['Controlsphere', 'Air Tracking 180']);
    const input = el(f).querySelector('input') as HTMLInputElement;
    input.value = 'air';
    input.dispatchEvent(new Event('input'));
    await f.whenStable();
    expect(texts(f, '.name')).toEqual(['Air Tracking 180']);
  });

  it('moves the selection with the arrow keys and marks it for screen readers', async () => {
    const f = await render();
    const list = el(f).querySelector('[role=listbox]') as HTMLElement;
    list.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
    list.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
    await f.whenStable();
    expect(TestBed.inject(Library).selectedId()).toBe('t1');
    expect(list.getAttribute('aria-activedescendant')).toBe('rec-1');
    expect(el(f).querySelector('#rec-1')?.getAttribute('aria-selected')).toBe('true');
  });

  it('says what each recording is, in words', async () => {
    const f = await render();
    const meta = texts(f, '.meta');
    expect(meta[0]).toContain('reviewed');
    expect(meta[1]).not.toContain('reviewed');
    expect(meta[2]).toContain('no stats');
  });
});
