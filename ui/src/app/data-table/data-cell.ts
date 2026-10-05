import { Directive, inject, input, TemplateRef } from '@angular/core';

/**
 * Templates a data table (data-table.ts) draws in place of a column's text: `<ng-template appCell="id" [appCellRows]=
 * "rows" let-row let-column="column">` for a column's cells, `<ng-template appHeader="id" let-column>` for its
 * header ("*" for either: every column without its own). The rows input is there only to give `let-row` the table's
 * row type.
 */

/** A cell template's context: the row, and the id of the cell's column. */
export interface DataCellContext<Row> {
  $implicit: Row;
  column: string;
}

/** A header template's context: the id of its column. */
export interface DataHeaderContext {
  $implicit: string;
}

@Directive({ selector: 'ng-template[appCell]' })
export class DataCell<Row> {
  readonly column = input.required<string>({ alias: 'appCell' });
  readonly appCellRows = input<readonly Row[]>([]);
  readonly template = inject<TemplateRef<DataCellContext<Row>>>(TemplateRef);

  static ngTemplateContextGuard<Row>(
    _cell: DataCell<Row>,
    context: unknown,
  ): context is DataCellContext<Row> {
    return typeof context === 'object' && context !== null;
  }
}

@Directive({ selector: 'ng-template[appHeader]' })
export class DataHeader {
  readonly column = input.required<string>({ alias: 'appHeader' });
  readonly template = inject<TemplateRef<DataHeaderContext>>(TemplateRef);

  static ngTemplateContextGuard(
    _header: DataHeader,
    context: unknown,
  ): context is DataHeaderContext {
    return typeof context === 'object' && context !== null;
  }
}
