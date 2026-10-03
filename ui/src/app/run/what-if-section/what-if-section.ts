import { Component, computed, input } from '@angular/core';

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

/**
 * A report's "What would raise your ..." section: the heading, the note (the content) and the lines by group. Nothing
 * when there are no lines.
 */
@Component({
  selector: 'app-what-if-section',
  templateUrl: './what-if-section.html',
  styleUrl: './what-if-section.scss',
})
export class WhatIfSection {
  readonly heading = input.required<string>();
  readonly table = input.required<WhatIfTable>();

  /** A group heading spans the change, the gains and the why. */
  protected readonly span = computed(() => this.table().columns.length + 2);
}
