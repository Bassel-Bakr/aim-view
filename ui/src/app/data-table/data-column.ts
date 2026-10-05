/**
 * What a data table (data-table.ts) is told about its columns and groups, and how it sorts a cell: by the column's own
 * value, else by its text's leading number (a dash: not measured, last), else by its words.
 */

/** How a cell's text stands out: a value worth seeing, the best one, one that needs attention, or a quiet one. */
export type CellTone = 'value' | 'good' | 'attention' | 'muted';

/** Where a column's cells sit: numbers to the end, words to the start. */
export type CellAlign = 'start' | 'end';

/** From which width of the page's main area a table's rows become cards: 520, 720 or 960 px (data-table.scss). */
export type CardWidth = 'narrow' | 'medium' | 'wide';

/** What a column sorts by: a number, words, or nothing (sorted last). */
export type SortValue = number | string | undefined;

/** One column of a data table: its header, what each row shows in it, and how it sorts and sits. */
export interface DataColumn<Row> {
  /** Unique in the table; a template for its cells (appCell) names it. */
  id: string;
  header: string;
  /** What the column means, on the header's hover. */
  title?: string;
  /** The cell's text (a template for the column's cells replaces it on screen; it still sorts and fills a card). */
  text: (row: Row) => string;
  /** The value the column sorts by; without it, the text's leading number, else its words. */
  sortBy?: (row: Row) => SortValue;
  /** False: the column does not sort. */
  sortable?: boolean;
  /** A cell's tip, on hover. */
  tip?: (row: Row) => string;
  tone?: (row: Row) => CellTone | null;
  /** Without it: to the end when every cell is a number (or a dash), else to the start. */
  align?: CellAlign;
  /** A sentence or more: wrapped, at least a readable width. Every other cell stays on one line. */
  prose?: boolean;
  /** Words that may take two lines when the table is short of room (a change's name). */
  wrap?: boolean;
  /** The row's name (a header cell for its row, which screen readers read with each cell). */
  rowHeader?: boolean;
  /** Hidden until the Columns menu shows it. */
  hidden?: boolean;
}

/** How a table groups its rows: each row's group (empty: no group row), and what a group's row says. */
export interface TableGrouping<Row> {
  /** The name a grouping goes by in the table's Group by menu. */
  label: string;
  key: (row: Row) => string;
  /** The word for a row in a group's count: "kill" gives "1 kill", "2 kills"; none: no count. */
  noun?: string;
  /** What a group's row says after its count: "median TTK 600 ms". */
  summary?: (rows: readonly Row[]) => string;
}

/** A column that shows one of the row's fields as it is; `more` adds or overrides the rest (a title, a tone). */
export function fieldColumn<Row, Key extends keyof Row & string>(
  id: Key,
  header: string,
  more: Partial<DataColumn<Row>> = {},
): DataColumn<Row> {
  return { id, header, text: (row) => String(row[id]), ...more };
}

const DASHES = new Set(['', '–', '—', '-']);
/** A leading number, signed (a minus or a plus), with thousands commas and a fraction. */
const LEADING_NUMBER = /^([+\-−]?)(\d[\d,]*(?:\.\d+)?)/;
const words = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** What a cell's text sorts by: its leading number ("12.3° ↑": 12.3), else its words; a dash: nothing. */
export function textSortValue(text: string): SortValue {
  const trimmed = text.trim();
  if (DASHES.has(trimmed)) return undefined;
  const match = LEADING_NUMBER.exec(trimmed);
  if (!match) return trimmed;
  const value = Number(match[2].replaceAll(',', ''));
  return match[1] === '' || match[1] === '+' ? value : -value;
}

export function sortValueOf<Row>(column: DataColumn<Row>, row: Row): SortValue {
  return column.sortBy ? column.sortBy(row) : textSortValue(column.text(row));
}

/** Numbers before words, numbers by size, words as a reader orders them ("2" before "10"); nothing last. */
export function compareSortValues(a: SortValue, b: SortValue): number {
  if (a === undefined || b === undefined) return a === b ? 0 : a === undefined ? 1 : -1;
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  if (typeof a === 'number') return -1;
  if (typeof b === 'number') return 1;
  return words.compare(a, b);
}

/** Where a column's cells sit: as it says, else to the end when every row's text is a number or a dash. */
export function alignOf<Row>(column: DataColumn<Row>, rows: readonly Row[]): CellAlign {
  if (column.align) return column.align;
  if (column.prose) return 'start';
  const numeric = rows.every((row) => typeof textSortValue(column.text(row)) !== 'string');
  return numeric && rows.length > 0 ? 'end' : 'start';
}

/** A group's row: its name, its count in the grouping's noun, and its summary. */
export function groupText<Row>(
  grouping: TableGrouping<Row>,
  name: string,
  rows: readonly Row[],
): string {
  const parts = [name];
  if (grouping.noun) parts.push(`${rows.length} ${grouping.noun}${rows.length === 1 ? '' : 's'}`);
  const summary = grouping.summary?.(rows);
  if (summary) parts.push(summary);
  return parts.join(' · ');
}
