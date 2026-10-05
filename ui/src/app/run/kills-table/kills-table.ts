import { Component, input, linkedSignal, output, signal } from '@angular/core';
import {
  aggregationFn_median,
  columnGroupingFeature,
  createColumnHelper,
  createExpandedRowModel,
  createGroupedRowModel,
  createSortedRowModel,
  ExpandedState,
  injectTable,
  Row,
  rowAggregationFeature,
  rowExpandingFeature,
  rowSortingFeature,
  SortingState,
  tableFeatures,
} from '@tanstack/angular-table';
import { Flick } from '../../api';
import { FlickRow, KillGrouping } from './kill-rows';

/**
 * A clicking run's kills, one row each (kill-rows.ts says what each cell holds): a column header sorts by it (again:
 * the other way, a third time: back to the kills' order), and the kills can be grouped (how the flick landed, its
 * direction, its distance, the first shot, the spawn), each group's row giving its count and median TTK and folding
 * its kills away. Sorting and grouping are TanStack Table's (headless: the markup and styles are this component's).
 * A row's click plays its kill (playKill).
 */

const features = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
  columnGroupingFeature,
  groupedRowModel: createGroupedRowModel(),
  rowExpandingFeature,
  expandedRowModel: createExpandedRowModel(),
  rowAggregationFeature,
  aggregationFns: { median: aggregationFn_median },
});

/** The table's rows, kills or groups of them. */
export type KillRow = Row<typeof features, FlickRow>;

/** A column as the table shows it: its header, the tip on it, and the number it sorts by (null: it does not sort). */
export interface KillColumn {
  id: string;
  label: string;
  title: string;
  sortValue: ((row: FlickRow) => number | null) | null;
}

const ms = (seconds: number | null | undefined) => (seconds == null ? null : 1000 * seconds);

export const KILL_COLUMNS: readonly KillColumn[] = [
  { id: 'kill', label: 'Kill', title: 'The kill, in order', sortValue: (row) => row.killNumber },
  {
    id: 'distance',
    label: 'Dist',
    title: 'Distance to the target, and the way it was',
    sortValue: (row) => row.flick.D0,
  },
  {
    id: 'ttk',
    label: 'TTK ms',
    title: 'TTK: time to kill, in milliseconds',
    sortValue: (row) => ms(row.flick.total),
  },
  {
    id: 'landed',
    label: 'Landed',
    title:
      'Flick landed: on target, an underflick (the degrees still to go) or an overflick (the degrees past the far edge)',
    sortValue: (row) => row.flick.end_left,
  },
  {
    id: 'confirmation',
    label: 'Conf ms',
    title: 'Confirmation: still on the target before the click, in milliseconds',
    sortValue: (row) => ms(row.flick.still),
  },
  {
    id: 'speed',
    label: 'Speed °/s',
    title: 'Flick speed: the highest speed in the flick, in degrees a second',
    sortValue: (row) => row.flick.peak,
  },
  {
    id: 'onMove',
    label: 'On move °/s',
    title: "Click on the move: the crosshair's speed at the click, in degrees a second",
    sortValue: (row) => row.flick.click_speed,
  },
  {
    id: 'shots',
    label: 'Shots',
    title: 'Shots at this target',
    sortValue: (row) => row.flick.shots,
  },
  {
    id: 'pathing',
    label: 'Pathing',
    title: 'Pathing: what picking this target cost against the fastest pick',
    sortValue: (row) => row.pickCost,
  },
  {
    id: 'steps',
    label: 'React · Flick · Micro ms',
    title:
      "Reaction, flick (the main movement) and micro (from the flick's end onto the target, then settling; each row splits it on hover)",
    sortValue: null,
  },
  {
    id: 'offCenter',
    label: 'Off ctr',
    title: "Click off center: how far the crosshair was from the target's center at the click",
    sortValue: (row) => row.flick.click_off,
  },
  {
    id: 'micros',
    label: 'Micros',
    title: 'Micros: separate movements after the flick',
    sortValue: (row) => row.flick.corrections,
  },
  {
    id: 'spawn',
    label: 'Spawn',
    title: 'Spawn: the target appeared after the flick began',
    sortValue: (row) => (row.flick.spawned ? 1 : 0),
  },
];

const GROUPS: readonly KillGrouping[] = ['landed', 'direction', 'distance', 'firstShot', 'spawn'];
const groupColumn = (key: KillGrouping) => `group-${key}`;

const helper = createColumnHelper<typeof features, FlickRow>();
// columns() keeps each column's own value type (numbers to sort by, words to group by) in one list
const columns = helper.columns([
  ...KILL_COLUMNS.map((column) =>
    helper.accessor((row: FlickRow) => column.sortValue?.(row) ?? undefined, {
      id: column.id,
      enableSorting: column.sortValue !== null,
      sortUndefined: 'last',
      ...(column.id === 'ttk' ? { aggregationFn: 'median' as const } : {}),
    }),
  ),
  ...GROUPS.map((key) =>
    helper.accessor((row: FlickRow) => row.groups[key], {
      id: groupColumn(key),
      enableSorting: false,
    }),
  ),
]);

/** How a sorted column says so to a screen reader. */
const ARIA_SORT = { asc: 'ascending', desc: 'descending' } as const;

@Component({
  selector: 'app-kills-table',
  templateUrl: './kills-table.html',
  styleUrl: './kills-table.scss',
})
export class KillsTable {
  readonly rows = input.required<FlickRow[]>();
  readonly selected = input<Flick | null>(null);
  /** What the kills are grouped by; null: not grouped. */
  readonly groupBy = input<KillGrouping | null>(null);
  readonly playKill = output<Flick>();
  protected readonly columns = KILL_COLUMNS;
  private readonly sorting = signal<SortingState>([]);
  /** Which groups are open: every one at first, and again whenever the grouping changes. */
  private readonly expanded = linkedSignal<ExpandedState>(() => {
    this.groupBy();
    return true;
  });
  protected readonly table = injectTable(() => {
    const groupBy = this.groupBy();
    return {
      features,
      columns,
      data: this.rows(),
      state: {
        sorting: this.sorting(),
        expanded: this.expanded(),
        grouping: groupBy ? [groupColumn(groupBy)] : [],
      },
      onSortingChange: (next) =>
        typeof next === 'function' ? this.sorting.update(next) : this.sorting.set(next),
      onExpandedChange: (next) =>
        typeof next === 'function' ? this.expanded.update(next) : this.expanded.set(next),
      getRowId: (row) => String(row.killNumber),
      sortDescFirst: false,
      // the groups' open state is this component's: a new row model must not reset it
      autoResetExpanded: false,
    };
  });

  protected sortOf(id: string): string | null {
    const sorted = this.table.getColumn(id)?.getIsSorted();
    return sorted ? ARIA_SORT[sorted] : null;
  }

  protected sortBy(id: string): void {
    this.table.getColumn(id)?.toggleSorting(undefined, false);
  }

  /** A group's row: what its kills share, how many there are, and their median TTK. */
  protected groupText(row: KillRow): string {
    const count = row.getLeafRows().length;
    const median = row.getValue<number | undefined>('ttk');
    const ttk = median === undefined ? '' : ` · median TTK ${Math.round(median)} ms`;
    return `${String(row.groupingValue)} · ${count} ${count === 1 ? 'kill' : 'kills'}${ttk}`;
  }
}
