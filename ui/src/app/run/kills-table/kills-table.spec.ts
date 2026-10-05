import { TestBed } from '@angular/core/testing';
import { Flick } from '../../api';
import { FlickRow } from './kill-rows';
import { KillsTable } from './kills-table';

/** A kill for the table: its number, TTK in seconds and how it landed; the rest left plain. */
function row(killNumber: number, ttkSeconds: number, landed: string): FlickRow {
  const flick = { kill_number: killNumber, total: ttkSeconds, D0: 10, spawned: false } as Flick;
  return {
    flick,
    killNumber,
    distance: '10.0° →',
    ttk: String(Math.round(1000 * ttkSeconds)),
    landed,
    confirmation: '–',
    flickSpeed: '–',
    onTheMove: '–',
    shots: '1',
    missed: false,
    pathing: '',
    steps: '',
    microSplit: '',
    offCenter: '',
    micros: '',
    spawn: 'no',
    pickCost: null,
    groups: {
      landed,
      direction: '→',
      distance: '10–15°',
      firstShot: 'Hit with the first shot',
      spawn: 'On screen at the start',
    },
  };
}

const ROWS = [row(1, 0.5, 'On target'), row(2, 0.3, 'Underflick'), row(3, 0.7, 'On target')];

async function render() {
  const fixture = TestBed.createComponent(KillsTable);
  fixture.componentRef.setInput('rows', ROWS);
  await fixture.whenStable();
  const el = fixture.nativeElement as HTMLElement;
  const order = () => [...el.querySelectorAll('tr.row')].map((tr) => tr.getAttribute('data-n'));
  const header = (label: string) =>
    [...el.querySelectorAll('th')].find((th) =>
      th.textContent?.trim().startsWith(label),
    ) as HTMLElement;
  const click = async (target: Element | null) => {
    (target as HTMLElement).click();
    await fixture.whenStable();
  };
  return { fixture, el, order, header, click };
}

describe('the kills table', () => {
  it('sorts by a column: up, down, then back to the kills’ order', async () => {
    const { order, header, click } = await render();
    expect(order()).toEqual(['1', '2', '3']);
    await click(header('TTK ms').querySelector('button'));
    expect(order()).toEqual(['2', '1', '3']);
    expect(header('TTK ms').getAttribute('aria-sort')).toBe('ascending');
    await click(header('TTK ms').querySelector('button'));
    expect(order()).toEqual(['3', '1', '2']);
    await click(header('TTK ms').querySelector('button'));
    expect(order()).toEqual(['1', '2', '3']);
  });

  it('groups the kills, each group with its count and median TTK, and folds a group away', async () => {
    const { fixture, el, order, click } = await render();
    fixture.componentRef.setInput('groupBy', 'landed');
    await fixture.whenStable();
    const groups = () =>
      [...el.querySelectorAll('.group-toggle')].map((b) => b.textContent?.trim());
    expect(groups()).toEqual([
      expect.stringContaining('On target · 2 kills · median TTK 600 ms'),
      expect.stringContaining('Underflick · 1 kill · median TTK 300 ms'),
    ]);
    expect(order()).toEqual(['1', '3', '2']);
    await click(el.querySelector('.group-toggle'));
    expect(order()).toEqual(['2']);
  });

  it('plays the kill whose row is clicked', async () => {
    const { fixture, el, click } = await render();
    const played: Flick[] = [];
    fixture.componentInstance.playKill.subscribe((flick) => played.push(flick));
    await click(el.querySelector('tr[data-n="3"] .play'));
    expect(played).toEqual([ROWS[2].flick]);
  });
});
