import { DecimalPipe } from '@angular/common';
import { afterRenderEffect, Component, computed, ElementRef, inject, signal } from '@angular/core';
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

/**
 * The recordings: a text filter, a chip per kind of run, and a list. The arrow keys, Home, End, Page Up and Page Down
 * move the selection; the list keeps the selected recording in view.
 */
@Component({
  selector: 'app-recordings',
  imports: [DecimalPipe, StampPipe],
  templateUrl: './recordings.html',
  styleUrl: './recordings.scss',
})
export class Recordings {
  protected readonly library = inject(Library);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  protected readonly kindLabels = KIND_LABELS;
  protected readonly query = signal('');
  protected readonly kind = signal<KindFilter>('all');

  private readonly all = computed<Recording[]>(() =>
    this.library.recordings.hasValue() ? this.library.recordings.value() : [],
  );

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

  constructor() {
    afterRenderEffect(() => {
      const id = this.activeId();
      if (id) this.host.nativeElement.querySelector(`#${id}`)?.scrollIntoView({ block: 'nearest' });
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
}
