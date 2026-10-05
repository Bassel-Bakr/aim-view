import { alignOf, compareSortValues, DataColumn, groupText, textSortValue } from './data-column';

/** A row of words and numbers, as a report's table holds them. */
interface Line {
  name: string;
  value: string;
}

const NAME: DataColumn<Line> = { id: 'name', header: 'Name', text: (row) => row.name };
const VALUE: DataColumn<Line> = { id: 'value', header: 'Value', text: (row) => row.value };

describe('a data column', () => {
  it('sorts a cell by its leading number, signed and with thousands, else by its words; a dash by nothing', () => {
    expect(textSortValue('12.3° ↑')).toBe(12.3);
    expect(textSortValue('−4 ms')).toBe(-4);
    expect(textSortValue('+1,234 kills')).toBe(1234);
    expect(textSortValue('–')).toBeUndefined();
    expect(textSortValue('on target')).toBe('on target');
  });

  it('puts numbers before words, words as a reader orders them, and nothing last', () => {
    const values = ['b 10', undefined, 7, 'b 2', -1];
    expect([...values].sort(compareSortValues)).toEqual([-1, 7, 'b 2', 'b 10', undefined]);
  });

  it('sits to the end when every cell is a number or a dash, else to the start', () => {
    const rows = [
      { name: 'a', value: '12 ms' },
      { name: 'b', value: '–' },
    ];
    expect(alignOf(VALUE, rows)).toBe('end');
    expect(alignOf(NAME, rows)).toBe('start');
    expect(alignOf({ ...VALUE, prose: true }, rows)).toBe('start');
  });

  it('names a group with its count and summary', () => {
    const grouping = { label: 'By', key: () => '', noun: 'kill', summary: () => 'median 3' };
    expect(groupText(grouping, 'Fast', [1])).toBe('Fast · 1 kill · median 3');
    expect(groupText(grouping, 'Slow', [1, 2])).toBe('Slow · 2 kills · median 3');
    expect(groupText({ label: 'By', key: () => '' }, 'Plain', [1, 2])).toBe('Plain');
  });
});
