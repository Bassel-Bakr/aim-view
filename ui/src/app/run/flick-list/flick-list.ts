import {
  afterRenderEffect,
  Component,
  computed,
  ElementRef,
  inject,
  input,
  signal,
  viewChild,
} from '@angular/core';
import { ClickReport } from '../../api';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { flickRows, GROUPINGS, KillGrouping } from '../kills-table/kill-rows';
import { KillsTable } from '../kills-table/kills-table';

/**
 * Every flick of a clicking run, beside the video: the kills table (sorted by a click on a header, grouped from the
 * choice above it), where a click on a kill replays it slowed down. The flick in focus is marked and kept in view
 * inside the list (never scrolling the page); "Follow the video" moves the focus with the video.
 */
@Component({
  selector: 'app-flick-list',
  imports: [KillsTable],
  templateUrl: './flick-list.html',
  styleUrl: './flick-list.scss',
})
export class FlickList {
  readonly report = input.required<ClickReport>();
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);
  private readonly scroll = viewChild<ElementRef<HTMLElement>>('scroll');
  protected readonly rows = computed(() => flickRows(this.report(), this.paths.analysis()));
  protected readonly groupings = GROUPINGS;
  protected readonly grouped = signal<KillGrouping | null>(null);

  constructor() {
    afterRenderEffect(() => {
      const flick = this.focus.selected();
      if (flick) this.keepInView(flick.kill_number);
    });
  }

  protected groupBy(value: string): void {
    this.grouped.set(GROUPINGS.find((choice) => choice.key === value)?.key ?? null);
  }

  /** Centers the row in the list when it is out of view, scrolling the list only. */
  private keepInView(killNumber: number): void {
    const box = this.scroll()?.nativeElement;
    const row = box?.querySelector<HTMLElement>(`tr[data-n="${killNumber}"]`);
    if (!box || !row) return;
    const top = row.getBoundingClientRect().top - box.getBoundingClientRect().top;
    if (top >= 0 && top + row.offsetHeight <= box.clientHeight) return;
    box.scrollTop += top - box.clientHeight / 2 + row.offsetHeight / 2;
  }
}
