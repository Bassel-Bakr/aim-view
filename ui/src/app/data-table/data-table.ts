/**
 * The app's one table component (`<app-data-table>`, `DataTable`). In: a feature's rows, its typed
 * columns (data-column.ts) and its cell and header templates (data-cell.ts). Out: the table's
 * markup (data-table.html), and the row the user picks.
 */

import { NgTemplateOutlet } from '@angular/common';
import {
  afterRenderEffect,
  Component,
  computed,
  contentChildren,
  ElementRef,
  input,
  linkedSignal,
  output,
  signal,
  TemplateRef,
  viewChild,
} from '@angular/core';
import {
  columnGroupingFeature,
  columnVisibilityFeature,
  createColumnHelper,
  createExpandedRowModel,
  createGroupedRowModel,
  createSortedRowModel,
  ExpandedState,
  injectTable,
  Row as TableRow,
  rowExpandingFeature,
  rowSortingFeature,
  SortingState,
  tableFeatures,
  Updater,
  ColumnVisibilityState,
  RowData,
} from '@tanstack/angular-table';
import { DataCell, DataCellContext, DataHeader, DataHeaderContext } from './data-cell';
import {
  alignOf,
  CardWidth,
  CellAlign,
  compareSortValues,
  DataColumn,
  groupText,
  SortValue,
  sortValueOf,
  TableGrouping,
} from './data-column';

/** The TanStack Table features every table uses: sorting, grouping, folding, hiding columns. */
const features = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
  columnGroupingFeature,
  groupedRowModel: createGroupedRowModel(),
  rowExpandingFeature,
  expandedRowModel: createExpandedRowModel(),
  columnVisibilityFeature,
});

/** The id of the column the rows are grouped on, which the table never shows. */
const GROUP_COLUMN = '__group';
/** How a sorted column says so to a screen reader. */
const ARIA_SORT = { asc: 'ascending', desc: 'descending' } as const;
/** The mark a sorted column's header shows for each direction. */
const SORT_MARKS = { asc: '▲', desc: '▼' } as const;
/** Where the Columns menu opens: this far under its button. */
const MENU_GAP_PX = 6;
/** The start of the localStorage key that keeps a table's shown columns (then its storageKey). */
const STORAGE_PREFIX = 'aimview-columns-';
/** The number the next table's Columns menu takes, for its element id. */
let nextMenu = 0;

/**
 * Every table of the app: rows and typed columns in, TanStack Table's sorting, grouping and column
 * hiding (headless: the markup and styles are this component's), after the Goldman Sachs design
 * system's data grid guidance. A column's header sorts it (again: the other way, a third time:
 * back to the rows' order); a grouping (fixed, or picked in the Group by menu) puts the rows under
 * group rows that fold them away, each with its count and summary; the Columns menu shows and
 * hides columns, kept in this browser. Headers and cells stay on one line, except prose columns,
 * which wrap at a readable width, and words that may wrap; numbers sit to the right in even
 * digits. The header row stays in view when the table scrolls in its own box (as tall as
 * --table-max-height), the first column too when it scrolls sideways (stickyFirst), and where the
 * page's main area is narrower than the table's cards width each row is a card. A picked row
 * (pickLabel: its first cell becomes a button; a click anywhere on the row picks it) is sent out
 * (pick); the selected one is marked and kept in view inside the table's box, never scrolling the
 * page. A column's cells and header can be templates of the table's user (data-cell.ts). Its
 * styles are global (themes/data-table.scss), as the other shared controls' are.
 */
@Component({
  selector: 'app-data-table',
  imports: [NgTemplateOutlet],
  host: { '[attr.data-cards]': 'cards()' },
  templateUrl: './data-table.html',
})
export class DataTable<Row extends RowData> {
  /** The rows, in the order the table shows them unsorted. */
  readonly rows = input.required<readonly Row[]>();
  /** The columns, in order. */
  readonly columns = input.required<readonly DataColumn<Row>[]>();
  /** A row's id, which keeps its place (selectedId, the groups' open state); by default its place in rows. */
  readonly rowId = input<((row: Row) => string) | null>(null);
  /** What the table holds, for a screen reader. */
  readonly label = input('');
  /** A fixed grouping, with no menu. */
  readonly grouping = input<TableGrouping<Row> | null>(null);
  /** The groupings the Group by menu offers, after "Nothing". */
  readonly groupings = input<readonly TableGrouping<Row>[]>([]);
  /** Whether a header click sorts its column (a column can still refuse: `sortable: false`). */
  readonly sortable = input(true);
  /** The Columns menu, which shows and hides columns; kept in this browser under storageKey when there is one. */
  readonly columnMenu = input(false);
  /** The name the shown columns are kept under in this browser; null: not kept. */
  readonly storageKey = input<string | null>(null);
  /** From which width of the page's main area the rows become cards; null: never (the table scrolls sideways). */
  readonly cards = input<CardWidth | null>('narrow');
  /** The first column stays in view when the table scrolls sideways. */
  readonly stickyFirst = input(false);
  /** The id of the row to mark as selected and keep in view; null: none. */
  readonly selectedId = input<string | null>(null);
  /** The words for picking a row ("Play kill 3"); none: rows are not picked. */
  readonly pickLabel = input<((row: Row) => string) | null>(null);
  /** A row the user picked. */
  readonly pick = output<Row>();

  /** The cell templates the table's user gave (appCell), by column. */
  private readonly cells = contentChildren(DataCell);
  /** The header templates the table's user gave (appHeader), by column. */
  private readonly headers = contentChildren(DataHeader);
  /** The box the table scrolls in. */
  private readonly scroll = viewChild.required<ElementRef<HTMLElement>>('scroll');
  /** The Columns menu's element id, for its button's popovertarget. */
  protected readonly menuId = `table-columns-${nextMenu++}`;

  /** The grouping in use: the Group by menu's choice, or the fixed one. */
  protected readonly chosen = linkedSignal<TableGrouping<Row> | null>(() => this.grouping());
  /** The column sorted on and its direction; empty: the rows' own order. */
  private readonly sorting = signal<SortingState>([]);
  /** Which groups are open: every one at first, and again whenever the grouping changes. */
  private readonly expanded = linkedSignal<ExpandedState>(() => {
    this.chosen();
    return true;
  });
  /** Which columns show, by id (false: hidden); from the columns and this browser at first. */
  private readonly visibility = linkedSignal<ColumnVisibilityState>(() =>
    startingVisibility(this.columns(), this.storageKey()),
  );
  /** The columns that show, in order. */
  protected readonly shown = computed(() =>
    this.columns().filter((column) => this.visibility()[column.id] !== false),
  );
  /** Each column's alignment, by id. */
  private readonly aligns = computed(
    () => new Map(this.columns().map((column) => [column.id, alignOf(column, this.rows())])),
  );
  /** The user's cell templates, by column id ('*': every column without its own). */
  private readonly cellTemplates = computed(
    () => new Map(this.cells().map((cell) => [cell.column(), cell.template])),
  );
  /** The user's header templates, by column id ('*': every column without its own). */
  private readonly headerTemplates = computed(
    () => new Map(this.headers().map((header) => [header.column(), header.template])),
  );
  /** The columns as TanStack Table takes them, with a hidden one for the grouping's key. */
  private readonly tableColumns = computed(() => {
    const helper = createColumnHelper<typeof features, Row>();
    const grouping = this.chosen();
    return helper.columns([
      ...this.columns().map((column) =>
        helper.accessor((row: Row) => sortValueOf(column, row), {
          id: column.id,
          enableSorting: this.canSort(column),
          sortUndefined: 'last',
          sortFn: (a, b, id) =>
            compareSortValues(a.getValue<SortValue>(id), b.getValue<SortValue>(id)),
        }),
      ),
      helper.accessor((row: Row) => grouping?.key(row) ?? '', {
        id: GROUP_COLUMN,
        enableSorting: false,
        enableHiding: false,
      }),
    ]);
  });
  /** The TanStack table: its state is this component's signals, which its changes update. */
  protected readonly table = injectTable(() => {
    const rowId = this.rowId();
    return {
      features,
      columns: this.tableColumns(),
      data: this.rows() as Row[],
      state: {
        sorting: this.sorting(),
        expanded: this.expanded(),
        grouping: this.chosen() ? [GROUP_COLUMN] : [],
        columnVisibility: this.visibility(),
      },
      onSortingChange: (next: Updater<SortingState>) =>
        this.sorting.update((now) => apply(next, now)),
      onExpandedChange: (next: Updater<ExpandedState>) =>
        this.expanded.update((now) => apply(next, now)),
      onColumnVisibilityChange: (next: Updater<ColumnVisibilityState>) =>
        this.showColumns(apply(next, this.visibility())),
      getRowId: (row: Row, index: number) => (rowId ? rowId(row) : String(index)),
      sortDescFirst: false,
      // the groups' open state is this component's: a new row model must not reset it
      autoResetExpanded: false,
    };
  });

  /** Keeps the selected row in view after each render. */
  constructor() {
    afterRenderEffect(() => {
      const id = this.selectedId();
      if (id !== null) this.keepInView(id);
    });
  }

  /** Whether a column's header sorts it: the table sorts and the column does not refuse. */
  protected canSort(column: DataColumn<Row>): boolean {
    return this.sortable() && column.sortable !== false;
  }

  /** A column's alignment, by its id. */
  protected alignOf(id: string): CellAlign {
    return this.aligns().get(id) ?? 'start';
  }

  /** The user's template for a column's cells; null: the cell shows the column's text. */
  protected cellTemplate(id: string): TemplateRef<DataCellContext<unknown>> | null {
    const templates = this.cellTemplates();
    return templates.get(id) ?? templates.get('*') ?? null;
  }

  /** The user's template for a column's header; null: the header shows the column's name. */
  protected headerTemplate(id: string): TemplateRef<DataHeaderContext> | null {
    const templates = this.headerTemplates();
    return templates.get(id) ?? templates.get('*') ?? null;
  }

  /** A column's aria-sort value ("ascending", "descending"); null when it is not sorted. */
  protected sortOf(id: string): string | null {
    const sorted = this.table.getColumn(id)?.getIsSorted();
    return sorted ? ARIA_SORT[sorted] : null;
  }

  /** The mark a column's header shows: its direction when sorted, else both ways. */
  protected sortMark(id: string): string {
    const sorted = this.table.getColumn(id)?.getIsSorted();
    return sorted ? SORT_MARKS[sorted] : '↕';
  }

  /** A header click: ascending, then descending, then the rows' own order. */
  protected sortBy(id: string): void {
    this.table.getColumn(id)?.toggleSorting(undefined, false);
  }

  /** Groups the rows by the Group by menu's choice, by its label ("Nothing": none). */
  protected groupBy(label: string): void {
    this.chosen.set(this.groupings().find((grouping) => grouping.label === label) ?? null);
  }

  /** The value a group row's rows share, as text. */
  protected groupName(row: TableRow<typeof features, Row>): string {
    return String(row.groupingValue ?? '');
  }

  /** A group's row: what its rows share, how many there are, and the grouping's summary of them. */
  protected groupLabel(row: TableRow<typeof features, Row>): string {
    const grouping = this.chosen();
    const name = this.groupName(row);
    if (!grouping) return name;
    return groupText(
      grouping,
      name,
      row.getLeafRows().map((leaf) => leaf.original),
    );
  }

  /** Whether a column shows. */
  protected isShown(id: string): boolean {
    return this.visibility()[id] !== false;
  }

  /** Shows or hides a column; the last one shown stays. */
  protected toggleColumn(id: string): void {
    this.table.getColumn(id)?.toggleVisibility(!this.isShown(id));
  }

  /** Sends a row out as picked, when the table's rows can be picked. */
  protected pickRow(row: Row): void {
    if (this.pickLabel()) this.pick.emit(row);
  }

  /** Opens the Columns menu under its button, its right edge on the button's. */
  protected placeMenu(button: HTMLElement, menu: HTMLElement): void {
    const box = button.getBoundingClientRect();
    menu.style.top = `${box.bottom + MENU_GAP_PX}px`;
    menu.style.right = `${Math.max(0, document.documentElement.clientWidth - box.right)}px`;
  }

  /** Shows the columns `next` says, unless it hides them all; keeps the choice in this browser. */
  private showColumns(next: ColumnVisibilityState): void {
    const shown = this.columns().filter((column) => next[column.id] !== false);
    if (shown.length === 0) return;
    this.visibility.set(next);
    const key = this.storageKey();
    if (!key) return;
    try {
      localStorage.setItem(STORAGE_PREFIX + key, JSON.stringify(next));
    } catch {
      // the choice lasts this visit only
    }
  }

  /** Centers the row in the table's box when it is out of view, scrolling the box only. */
  private keepInView(id: string): void {
    const box = this.scroll().nativeElement;
    const row = [...box.querySelectorAll<HTMLElement>('tr[data-id]')].find(
      (tr) => tr.dataset['id'] === id,
    );
    if (!row) return;
    const header = box.querySelector('thead')?.getBoundingClientRect().height ?? 0;
    const top = row.getBoundingClientRect().top - box.getBoundingClientRect().top;
    if (top >= header && top + row.offsetHeight <= box.clientHeight) return;
    box.scrollTop += top - box.clientHeight / 2 + row.offsetHeight / 2;
  }
}

/** A TanStack update applied: a new state, or a function of the state now. */
function apply<State>(next: Updater<State>, now: State): State {
  return typeof next === 'function' ? (next as (old: State) => State)(now) : next;
}

/** The columns hidden at first, then the ones this browser kept for the table. */
function startingVisibility<Row>(
  columns: readonly DataColumn<Row>[],
  key: string | null,
): ColumnVisibilityState {
  const state: ColumnVisibilityState = {};
  for (const column of columns) if (column.hidden) state[column.id] = false;
  if (!key) return state;
  try {
    const kept: unknown = JSON.parse(localStorage.getItem(STORAGE_PREFIX + key) ?? '{}');
    if (kept && typeof kept === 'object') {
      for (const [id, shown] of Object.entries(kept))
        if (typeof shown === 'boolean') state[id] = shown;
    }
  } catch {
    // nothing kept, or storage closed: the columns as the table gives them
  }
  return state;
}
