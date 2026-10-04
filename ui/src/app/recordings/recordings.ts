import {
  CdkFixedSizeVirtualScroll,
  CdkVirtualForOf,
  CdkVirtualScrollViewport,
} from '@angular/cdk/scrolling';
import { DecimalPipe } from '@angular/common';
import {
  afterRenderEffect,
  Component,
  computed,
  ElementRef,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { Badge } from '../controls/badge';
import { Button } from '../controls/button';
import { Kind, Recording } from '../api';
import { KIND_LABELS } from '../format';
import { Library } from '../services/library';
import { StampPipe } from '../stamp-pipe';

export type KindFilter = Kind | 'all';

/** A filter chip: a kind of run, with how many recordings it holds. */
export interface KindChip {
  key: KindFilter;
  label: string;
  count: number;
}

const KIND_ORDER: Kind[] = ['static', 'dynamic', 'switching', 'tracking'];
const PAGE = 10;
/** A row's height when the token cannot be read (tests): the token's value (themes/recordings.scss). */
const ROW_HEIGHT = 54;

/** A row's height in pixels, from its token (--recordings-row-height), which the virtual scroll needs as a number. */
function rowHeight(): number {
  if (typeof document === 'undefined') return ROW_HEIGHT;
  const value = getComputedStyle(document.documentElement).getPropertyValue(
    '--recordings-row-height',
  );
  return parseFloat(value) || ROW_HEIGHT;
}

/**
 * The recordings: a text filter (Ctrl K from anywhere), a chip per kind of run, and a list, of which only the rows in
 * view are made (a virtual scroll: a VODs folder holds thousands). The arrow keys, Home, End, Page Up and Page Down
 * move the selection; the list keeps the selected recording in view.
 */
@Component({
  selector: 'app-recordings',
  imports: [
    CdkVirtualScrollViewport,
    CdkFixedSizeVirtualScroll,
    CdkVirtualForOf,
    DecimalPipe,
    StampPipe,
    Badge,
    Button,
  ],
  host: { '(document:keydown)': 'focusSearch($event)' },
  templateUrl: './recordings.html',
  styleUrl: './recordings.scss',
})
export class Recordings {
  protected readonly library = inject(Library);
  private readonly viewport = viewChild(CdkVirtualScrollViewport);
  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('q');
  protected readonly kindLabels = KIND_LABELS;
  /** Every row's height (the virtual scroll lays the rows out by it). */
  protected readonly rowHeight = rowHeight();
  protected readonly byId = (_: number, r: Recording): string => r.id;
  protected readonly query = signal('');
  protected readonly kind = signal<KindFilter>('all');

  private readonly all = this.library.all;

  protected readonly chips = computed<KindChip[]>(() => {
    const all = this.all();
    const kinds = KIND_ORDER.map((k): KindChip => ({
      key: k,
      label: KIND_LABELS[k],
      count: all.filter((r) => r.kind === k).length,
    })).filter((c) => c.count > 0);
    return [{ key: 'all', label: 'All', count: all.length }, ...kinds];
  });

  protected readonly shown = computed<Recording[]>(() => {
    const q = this.query().trim().toLowerCase();
    const kind = this.kind();
    return this.all().filter(
      (r) => (kind === 'all' || r.kind === kind) && (!q || r.scenario.toLowerCase().includes(q)),
    );
  });

  protected readonly activeId = computed<string | null>(() => {
    const i = this.shown().findIndex((r) => r.id === this.library.selectedId());
    return i < 0 ? null : `rec-${i}`;
  });

  /** Empties the list (recordings opened in this browser): the files stay where they are. */
  protected clearList(): void {
    this.library.selectedId.set(null);
    void this.library.source.clear();
  }

  constructor() {
    // the selected row in view, scrolled the least: a row out of view is not in the page to scroll into view
    afterRenderEffect(() => {
      const i = this.shown().findIndex((r) => r.id === this.library.selectedId());
      const v = this.viewport();
      if (i < 0 || !v) return;
      const top = i * this.rowHeight;
      const from = v.measureScrollOffset('top');
      const height = v.getViewportSize();
      if (top < from) v.scrollToOffset(top);
      else if (top + this.rowHeight > from + height)
        v.scrollToOffset(top + this.rowHeight - height);
    });
  }

  protected selectRow(e: MouseEvent): void {
    const row = (e.target as HTMLElement).closest<HTMLElement>('[role=option]');
    if (row) this.library.selectedId.set(this.shown()[Number(row.dataset['i'])].id);
  }

  protected moveSelection(e: KeyboardEvent): void {
    const list = this.shown();
    const at = list.findIndex((r) => r.id === this.library.selectedId());
    let to: number;
    switch (e.key) {
      case 'ArrowDown':
        to = at + 1;
        break;
      case 'ArrowUp':
        to = at - 1;
        break;
      case 'PageDown':
        to = at + PAGE;
        break;
      case 'PageUp':
        to = at - PAGE;
        break;
      case 'Home':
        to = 0;
        break;
      case 'End':
        to = list.length - 1;
        break;
      default:
        return;
    }
    e.preventDefault();
    if (list.length)
      this.library.selectedId.set(list[Math.max(0, Math.min(list.length - 1, to))].id);
  }

  /** Ctrl K (Cmd K on a Mac) from anywhere: the filter, its text selected. */
  protected focusSearch(e: KeyboardEvent): void {
    if (e.key.toLowerCase() !== 'k' || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
    e.preventDefault();
    const input = this.search().nativeElement;
    input.focus();
    input.select();
  }
}
