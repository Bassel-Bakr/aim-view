import { ComponentFixture, TestBed } from '@angular/core/testing';
import { answer, recording, serverMode } from '../fake-api';
import { Library } from '../services/library';
import { Recordings } from './recordings';

const RECORDINGS = [
  recording({ id: 's1', scenario: '1wall 6targets', kind: 'static' }),
  recording({ id: 't1', scenario: 'Controlsphere', kind: 'tracking', analysed: false }),
  recording({ id: 't2', scenario: 'Air Tracking 180', kind: 'tracking', stats: false }),
];

async function render(): Promise<ComponentFixture<Recordings>> {
  TestBed.configureTestingModule({ providers: serverMode() });
  const fixture = TestBed.createComponent(Recordings);
  await answer({ '/api/vods': RECORDINGS });
  await fixture.whenStable();
  return fixture;
}

const el = (fixture: ComponentFixture<Recordings>) => fixture.nativeElement as HTMLElement;
const texts = (fixture: ComponentFixture<Recordings>, selector: string) =>
  [...el(fixture).querySelectorAll(selector)].map((node) =>
    node.textContent?.replace(/\s+/g, ' ').trim(),
  );

describe('Recordings', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('offers a chip per kind of run, with its count', async () => {
    const fixture = await render();
    expect(texts(fixture, '.chip')).toEqual(['All 3', 'Static 1', 'Tracking 2']);
  });

  it('filters by kind and by name', async () => {
    const fixture = await render();
    (el(fixture).querySelectorAll('.chip')[2] as HTMLButtonElement).click();
    await fixture.whenStable();
    expect(texts(fixture, '.name')).toEqual(['Controlsphere', 'Air Tracking 180']);
    const input = el(fixture).querySelector('input') as HTMLInputElement;
    input.value = 'air';
    input.dispatchEvent(new Event('input'));
    await fixture.whenStable();
    expect(texts(fixture, '.name')).toEqual(['Air Tracking 180']);
  });

  it('moves the selection with the arrow keys and marks it for screen readers', async () => {
    const fixture = await render();
    const list = el(fixture).querySelector('[role=listbox]') as HTMLElement;
    list.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
    list.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
    await fixture.whenStable();
    expect(TestBed.inject(Library).selectedId()).toBe('t1');
    expect(list.getAttribute('aria-activedescendant')).toBe('rec-1');
    expect(el(fixture).querySelector('#rec-1')?.getAttribute('aria-selected')).toBe('true');
  });

  it('says what each recording is, in words', async () => {
    const fixture = await render();
    const meta = texts(fixture, '.meta');
    expect(meta[0]).toContain('reviewed');
    expect(meta[1]).not.toContain('reviewed');
    expect(meta[2]).toContain('no stats');
  });
});
