/**
 * A report's what-if section: what one change would add. In: the lines the reports build from the
 * review's what-ifs (report/click-stats.ts `clickWhatIf` for a clicking run's score,
 * report/track-stats.ts `whatIfTable` for a tracking run's accuracy). Out: a heading, a note and a
 * data table of the lines.
 */

import { Component, computed, input } from '@angular/core';
import { DataColumn, TableGrouping } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';

/**
 * A what-if line as shown: the change, its gains (one per gain column) and why it would help.
 */
export interface WhatIfLine {
  /** The change, in words. */
  what: string;
  /** What the change would add, as text, one per gain column in the table's order. */
  gains: string[];
  /** Why the change would help. */
  how: string;
}

/** The lines under one heading (name null: no heading). */
export interface WhatIfGroup {
  /** The heading, or null for lines under none. */
  name: string | null;
  /** The group's lines, in the order shown. */
  lines: WhatIfLine[];
}

/** A what-if table: the gain columns' headings and the groups of lines. */
export interface WhatIfTable {
  /** Each gain column's heading. */
  columns: string[];
  /** The groups of lines; no groups shows no section. */
  groups: WhatIfGroup[];
}

/** A line as the table holds it: with its group's name ('' for lines under no heading). */
export interface WhatIfRow extends WhatIfLine {
  /** The group's name, '' for lines under no heading. */
  group: string;
}

/** The table's grouping: the rows under their group's name. */
const BY_KIND: TableGrouping<WhatIfRow> = { label: 'Kind of change', key: (row) => row.group };
/** The groupings the table's Group by menu offers. */
const GROUPINGS: readonly TableGrouping<WhatIfRow>[] = [BY_KIND];

/**
 * A report's "What would raise your ..." section: the heading, the note (the content) and the lines
 * by group (its Group by menu turns the groups off), in the app's data table: the change, its gains
 * (sortable) and why, a prose column wide enough to read; cards where the page is narrower than
 * 720 px. Nothing when there are no lines.
 */
@Component({
  selector: 'app-what-if-section',
  imports: [DataTable],
  templateUrl: './what-if-section.html',
  styleUrl: './what-if-section.scss',
})
export class WhatIfSection {
  /** The section's heading ("What would raise your score"), also the table's label. */
  readonly heading = input.required<string>();
  /** The lines to show, by group. */
  readonly table = input.required<WhatIfTable>();
  /** The grouping the table starts with: by kind of change. */
  protected readonly grouping = BY_KIND;
  /** The groupings the table offers. */
  protected readonly groupings = GROUPINGS;
  /** Every line, each with its group's name. */
  protected readonly rows = computed(() =>
    this.table().groups.flatMap((group) =>
      group.lines.map((line) => ({ ...line, group: group.name ?? '' })),
    ),
  );
  /** The table's columns: the change, one per gain column, and why. */
  protected readonly columns = computed((): DataColumn<WhatIfRow>[] => [
    { id: 'what', header: 'Change', text: (row) => row.what, rowHeader: true, wrap: true },
    ...this.table().columns.map((header, index): DataColumn<WhatIfRow> => ({
      id: `gain-${index}`,
      header,
      text: (row) => row.gains[index] ?? '',
      tone: () => 'value',
    })),
    {
      id: 'how',
      header: 'Why',
      text: (row) => row.how,
      prose: true,
      sortable: false,
      tone: () => 'muted',
    },
  ]);
  /** A row's id in the table: its group and its change. */
  protected readonly rowId = (row: WhatIfRow): string => `${row.group}/${row.what}`;
}
