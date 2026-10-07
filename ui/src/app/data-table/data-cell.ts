/**
 * Templates a data table (data-table.ts) draws in place of a column's text: `<ng-template appCell="id" [appCellRows]=
 * "rows" let-row let-column="column">` for a column's cells, `<ng-template appHeader="id" let-column>` for its
 * header ("*" for either: every column without its own). The rows input is there only to give `let-row` the table's
 * row type.
 */

import { Directive, inject, input, TemplateRef } from '@angular/core';

/** A cell template's context: the row, and the id of the cell's column. */
export interface DataCellContext<Row> {
  /** The row (let-row). */
  $implicit: Row;
  /** The id of the cell's column (let-column="column"). */
  column: string;
}

/** A header template's context: the id of its column. */
export interface DataHeaderContext {
  /** The id of the header's column (let-column). */
  $implicit: string;
}

/** A template for a column's cells (`appCell`), which the data table finds among its content. */
@Directive({ selector: 'ng-template[appCell]' })
export class DataCell<Row> {
  /** The id of the column whose cells it draws; "*" for every column without its own. */
  readonly column = input.required<string>({ alias: 'appCell' });
  /** The table's rows, only to give `let-row` their type; the directive never reads them. */
  readonly appCellRows = input<readonly Row[]>([]);
  /** The template itself. */
  readonly template = inject<TemplateRef<DataCellContext<Row>>>(TemplateRef);

  /** Tells the template type checker the context is a DataCellContext of the table's rows. */
  static ngTemplateContextGuard<Row>(
    _cell: DataCell<Row>,
    context: unknown,
  ): context is DataCellContext<Row> {
    return typeof context === 'object' && context !== null;
  }
}

/** A template for a column's header (`appHeader`), which the data table finds among its content. */
@Directive({ selector: 'ng-template[appHeader]' })
export class DataHeader {
  /** The id of the column whose header it draws; "*" for every column without its own. */
  readonly column = input.required<string>({ alias: 'appHeader' });
  /** The template itself. */
  readonly template = inject<TemplateRef<DataHeaderContext>>(TemplateRef);

  /** Tells the template type checker the context is a DataHeaderContext. */
  static ngTemplateContextGuard(
    _header: DataHeader,
    context: unknown,
  ): context is DataHeaderContext {
    return typeof context === 'object' && context !== null;
  }
}
