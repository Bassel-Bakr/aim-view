import { Component, computed, input } from '@angular/core';
import { DataColumn, TableGrouping } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';

/** A what-if line as shown: the change, its gains (one per gain column) and why it would help. */
export interface WhatIfLine {
  what: string;
  gains: string[];
  how: string;
}

/** The lines under one heading (name null: no heading). */
export interface WhatIfGroup {
  name: string | null;
  lines: WhatIfLine[];
}

/** A what-if table: the gain columns' headings and the groups of lines. */
export interface WhatIfTable {
  columns: string[];
  groups: WhatIfGroup[];
}

/** A line as the table holds it: with its group's name ('' for lines under no heading). */
export interface WhatIfRow extends WhatIfLine {
  group: string;
}

const BY_KIND: TableGrouping<WhatIfRow> = { label: 'Kind of change', key: (row) => row.group };
const GROUPINGS: readonly TableGrouping<WhatIfRow>[] = [BY_KIND];

/**
 * A report's "What would raise your ..." section: the heading, the note (the content) and the lines by group (its Group
 * by menu turns the groups off), in the app's data table: the change, its gains (sortable) and why, a prose column wide
 * enough to read; cards where the page is narrower than 720 px. Nothing when there are no lines.
 */
@Component({
  selector: 'app-what-if-section',
  imports: [DataTable],
  templateUrl: './what-if-section.html',
  styleUrl: './what-if-section.scss',
})
export class WhatIfSection {
  readonly heading = input.required<string>();
  readonly table = input.required<WhatIfTable>();
  protected readonly grouping = BY_KIND;
  protected readonly groupings = GROUPINGS;
  protected readonly rows = computed(() =>
    this.table().groups.flatMap((group) =>
      group.lines.map((line) => ({ ...line, group: group.name ?? '' })),
    ),
  );
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
  protected readonly rowId = (row: WhatIfRow): string => `${row.group}/${row.what}`;
}
