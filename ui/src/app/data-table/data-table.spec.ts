import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DataCell } from './data-cell';
import { DataColumn, TableGrouping } from './data-column';
import { DataTable } from './data-table';

/** A row as a report's table holds it: words, a measure as text, and the group it falls in. */
interface Line {
  name: string;
  time: string;
  part: string;
}

const LINES: Line[] = [
  { name: 'First', time: '500 ms', part: 'Opening' },
  { name: 'Second', time: '–', part: '' },
  { name: 'Third', time: '90 ms', part: 'Opening' },
];

const COLUMNS: DataColumn<Line>[] = [
  { id: 'name', header: 'Name', text: (row) => row.name, rowHeader: true },
  { id: 'time', header: 'Time', text: (row) => row.time },
  { id: 'note', header: 'Note', text: () => 'a sentence', prose: true, sortable: false },
];

const BY_PART: TableGrouping<Line> = { label: 'Part', key: (row) => row.part, noun: 'line' };

async function render(inputs: Record<string, unknown> = {}) {
  const fixture = TestBed.createComponent(DataTable<Line>);
  fixture.componentRef.setInput('rows', LINES);
  fixture.componentRef.setInput('columns', COLUMNS);
  fixture.componentRef.setInput('rowId', (row: Line) => row.name);
  for (const [name, value] of Object.entries(inputs)) fixture.componentRef.setInput(name, value);
  await fixture.whenStable();
  const el = fixture.nativeElement as HTMLElement;
  const order = () =>
    [...el.querySelectorAll('tr.data-row')].map((tr) => tr.getAttribute('data-id'));
  const headers = () => [...el.querySelectorAll('thead th')].map((th) => th.textContent?.trim());
  const click = async (target: Element | null) => {
    (target as HTMLElement).click();
    await fixture.whenStable();
  };
  return { fixture, el, order, headers, click };
}

describe('the data table', () => {
  it("sorts by a column's numbers, a dash last; its prose column wraps and does not sort", async () => {
    const { el, order, click } = await render();
    const timeSort = () => el.querySelectorAll('thead th')[1].querySelector('button');
    await click(timeSort());
    expect(order()).toEqual(['Third', 'First', 'Second']);
    await click(timeSort());
    expect(order()).toEqual(['First', 'Third', 'Second']);
    await click(timeSort());
    expect(order()).toEqual(['First', 'Second', 'Third']);
    const note = el.querySelectorAll('thead th')[2];
    expect(note.querySelector('button')).toBeNull();
    expect(note.hasAttribute('data-prose')).toBe(true);
    expect(el.querySelector('td[data-label="Time"]')?.getAttribute('data-align')).toBe('end');
  });

  it('groups rows under group rows that fold them away, with none for rows of no group', async () => {
    const { el, order, click } = await render({ grouping: BY_PART });
    const toggles = el.querySelectorAll('.data-group-toggle');
    expect([...toggles].map((b) => b.textContent?.trim())).toEqual([
      expect.stringContaining('Opening · 2 lines'),
    ]);
    expect(order()).toEqual(['First', 'Third', 'Second']);
    await click(toggles[0]);
    expect(order()).toEqual(['Second']);
  });

  it('hides a column from its menu, keeps the choice in this browser, and keeps the last column', async () => {
    localStorage.removeItem('aimview-columns-spec');
    const first = await render({ columnMenu: true, storageKey: 'spec' });
    const boxes = () => [...first.el.querySelectorAll<HTMLInputElement>('.data-menu input')];
    await first.click(boxes()[1]);
    expect(first.headers()).toEqual([expect.stringContaining('Name'), 'Note']);
    await first.click(boxes()[0]);
    expect(boxes()[2].disabled).toBe(true);
    const again = await render({ columnMenu: true, storageKey: 'spec' });
    expect(again.headers()).toEqual(['Note']);
    localStorage.removeItem('aimview-columns-spec');
  });

  it('sends a picked row out, and marks the selected one', async () => {
    const pickLabel = (row: Line) => `Open ${row.name}`;
    const { fixture, el, click } = await render({ pickLabel, selectedId: 'Third' });
    const picked: Line[] = [];
    fixture.componentInstance.pick.subscribe((row) => picked.push(row));
    await click(el.querySelector('tr[data-id="Second"] td'));
    await click(el.querySelector('[aria-label="Open First"]'));
    expect(picked).toEqual([LINES[1], LINES[0]]);
    expect(el.querySelector('[aria-current="true"]')?.getAttribute('data-id')).toBe('Third');
  });
});

/** A table's user that draws the name column's cells itself. */
@Component({
  imports: [DataTable, DataCell],
  template: `
    <app-data-table [rows]="lines" [columns]="columns">
      <ng-template appCell="name" [appCellRows]="lines" let-row>
        <b>{{ row.name.toUpperCase() }}</b>
      </ng-template>
    </app-data-table>
  `,
})
class TemplatedTable {
  readonly lines = LINES;
  readonly columns = COLUMNS;
}

describe("a data table's cell template", () => {
  it("draws a column's cells", async () => {
    const fixture = TestBed.createComponent(TemplatedTable);
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    expect([...el.querySelectorAll('th[scope="row"] b')].map((b) => b.textContent)).toEqual([
      'FIRST',
      'SECOND',
      'THIRD',
    ]);
  });
});
