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
  CellAlign,
  compareSortValues,
  DataColumn,
  groupText,
  SortValue,
  sortValueOf,
  TableGrouping,
} from './data-column';

/**
 * Every table of the app: rows and typed columns in, TanStack Table's sorting, grouping and column hiding (headless:
 * the markup and styles are this component's), after the Goldman Sachs design system's data grid guidance. A
 * column's header sorts it (again: the other way, a third time: back to the rows' order); a grouping (fixed, or
 * picked in the Group by menu) puts the rows under group rows that fold them away, each with its count and summary;
 * the Columns menu shows and hides columns, kept in this browser. Headers and cells stay on one line, except prose
 * columns, which wrap at a readable width; numbers sit to the right in even digits. The header row stays in view when
 * the table scrolls in its own box (as tall as --table-max-height), the first column too when it scrolls sideways
 * (stickyFirst), and on a phone each row is a card. A picked row (pickLabel: its first cell becomes a button; a click
 * anywhere on the row picks it) is sent out (pick); the selected one is marked and kept in view inside the table's
 * box, never scrolling the page. A column's cells and header can be templates of the table's user (data-cell.ts). Its
 * styles are global (themes/data-table.scss), as the other shared controls' are.
 */

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
const SORT_MARKS = { asc: '▲', desc: '▼' } as const;
/** Where the Columns menu opens: this far under its button. */
const MENU_GAP_PX = 6;
const STORAGE_PREFIX = 'aimview-columns-';
let nextMenu = 0;

@Component({
  selector: 'app-data-table',
  imports: [NgTemplateOutlet],
  templateUrl: './data-table.html',
})
export class DataTable<Row extends RowData> {
  readonly rows = input.required<readonly Row[]>();
  readonly columns = input.required<readonly DataColumn<Row>[]>();
  /** A row's id, which keeps its place (selectedId, the groups' open state); by default its place in rows. */
  readonly rowId = input<((row: Row) => string) | null>(null);
  /** What the table holds, for a screen reader. */
  readonly label = input('');
  /** A fixed grouping, with no menu. */
  readonly grouping = input<TableGrouping<Row> | null>(null);
  /** The groupings the Group by menu offers, after "Nothing". */
  readonly groupings = input<readonly TableGrouping<Row>[]>([]);
  readonly sortable = input(true);
  /** The Columns menu, which shows and hides columns; kept in this browser under storageKey when there is one. */
  readonly columnMenu = input(false);
  readonly storageKey = input<string | null>(null);
  /** The first column stays in view when the table scrolls sideways. */
  readonly stickyFirst = input(false);
  readonly selectedId = input<string | null>(null);
  /** The words for picking a row ("Play kill 3"); none: rows are not picked. */
  readonly pickLabel = input<((row: Row) => string) | null>(null);
  readonly pick = output<Row>();

  private readonly cells = contentChildren(DataCell);
  private readonly headers = contentChildren(DataHeader);
  private readonly scroll = viewChild.required<ElementRef<HTMLElement>>('scroll');
  protected readonly menuId = `table-columns-${nextMenu++}`;

  /** The grouping in use: the Group by menu's choice, or the fixed one. */
  protected readonly chosen = linkedSignal<TableGrouping<Row> | null>(() => this.grouping());
  private readonly sorting = signal<SortingState>([]);
  /** Which groups are open: every one at first, and again whenever the grouping changes. */
  private readonly expanded = linkedSignal<ExpandedState>(() => {
    this.chosen();
    return true;
  });
  private readonly visibility = linkedSignal<ColumnVisibilityState>(() =>
    startingVisibility(this.columns(), this.storageKey()),
  );
  protected readonly shown = computed(() =>
    this.columns().filter((column) => this.visibility()[column.id] !== false),
  );
  private readonly aligns = computed(
    () => new Map(this.columns().map((column) => [column.id, alignOf(column, this.rows())])),
  );
  private readonly cellTemplates = computed(
    () => new Map(this.cells().map((cell) => [cell.column(), cell.template])),
  );
  private readonly headerTemplates = computed(
    () => new Map(this.headers().map((header) => [header.column(), header.template])),
  );
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

  constructor() {
    afterRenderEffect(() => {
      const id = this.selectedId();
      if (id !== null) this.keepInView(id);
    });
  }

  protected canSort(column: DataColumn<Row>): boolean {
    return this.sortable() && column.sortable !== false;
  }

  protected alignOf(id: string): CellAlign {
    return this.aligns().get(id) ?? 'start';
  }

  protected cellTemplate(id: string): TemplateRef<DataCellContext<unknown>> | null {
    const templates = this.cellTemplates();
    return templates.get(id) ?? templates.get('*') ?? null;
  }

  protected headerTemplate(id: string): TemplateRef<DataHeaderContext> | null {
    const templates = this.headerTemplates();
    return templates.get(id) ?? templates.get('*') ?? null;
  }

  protected sortOf(id: string): string | null {
    const sorted = this.table.getColumn(id)?.getIsSorted();
    return sorted ? ARIA_SORT[sorted] : null;
  }

  protected sortMark(id: string): string {
    const sorted = this.table.getColumn(id)?.getIsSorted();
    return sorted ? SORT_MARKS[sorted] : '↕';
  }

  protected sortBy(id: string): void {
    this.table.getColumn(id)?.toggleSorting(undefined, false);
  }

  protected groupBy(label: string): void {
    this.chosen.set(this.groupings().find((grouping) => grouping.label === label) ?? null);
  }

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

  protected isShown(id: string): boolean {
    return this.visibility()[id] !== false;
  }

  /** Shows or hides a column; the last one shown stays. */
  protected toggleColumn(id: string): void {
    this.table.getColumn(id)?.toggleVisibility(!this.isShown(id));
  }

  protected pickRow(row: Row): void {
    if (this.pickLabel()) this.pick.emit(row);
  }

  /** Opens the Columns menu under its button, its right edge on the button's. */
  protected placeMenu(button: HTMLElement, menu: HTMLElement): void {
    const box = button.getBoundingClientRect();
    menu.style.top = `${box.bottom + MENU_GAP_PX}px`;
    menu.style.right = `${Math.max(0, document.documentElement.clientWidth - box.right)}px`;
  }

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
