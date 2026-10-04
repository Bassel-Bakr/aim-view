import {
  afterRenderEffect,
  Component,
  computed,
  ElementRef,
  inject,
  input,
  viewChild,
} from '@angular/core';
import { ClickReport, Flick } from '../../api';
import {
  arrow,
  formatCount,
  formatDegrees,
  formatEnded,
  formatMs,
  formatSpeed,
} from '../../format';
import { PathAnalysis, pickText } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { micro, microSplit } from '../report/budget';

/** A flick as the list shows it. */
export interface FlickRow {
  flick: Flick;
  n: number;
  /** The distance and the way it was: "12.3° ↑". */
  distance: string;
  ttk: string;
  landed: string;
  confirmation: string;
  flickSpeed: string;
  onTheMove: string;
  shots: string;
  missed: boolean;
  /** What picking this target cost against the fastest pick. */
  pathing: string;
  reaction: string;
  flickTime: string;
  micro: string;
  /** The micro's two parts, on hover: "120 ms onto the target, 80 ms settling". */
  microSplit: string;
  offCenter: string;
  micros: string;
  spawn: string;
}

export function flickRows(r: ClickReport, paths: PathAnalysis | null): FlickRow[] {
  return r.flicks.map((m) => ({
    flick: m,
    n: m.n,
    distance: `${m.D0.toFixed(1)}° ${arrow(m.dir)}`,
    ttk: formatMs(m.total),
    landed: formatEnded(m.end_left, r.summary.radius),
    confirmation: formatMs(m.still),
    flickSpeed: formatSpeed(m.peak),
    onTheMove: formatSpeed(m.click_speed),
    shots: formatCount(m.shots),
    missed: (m.shots ?? 0) > 1,
    pathing: pickText(paths, m.n),
    reaction: formatMs(m.react),
    flickTime: formatMs(m.flick),
    micro: formatMs(micro(m)),
    microSplit: m.parts ? microSplit(m.parts) : '',
    offCenter: formatDegrees(m.click_off),
    micros: formatCount(m.corr),
    spawn: m.spawned ? 'yes' : 'no',
  }));
}

/**
 * Every flick of a clicking run, beside the video: click one to replay it slowed down. The flick in focus is marked
 * and kept in view inside the list (never scrolling the page); "Follow the video" moves the focus with the video.
 */
@Component({
  selector: 'app-flick-list',
  templateUrl: './flick-list.html',
  styleUrl: './flick-list.scss',
})
export class FlickList {
  readonly report = input.required<ClickReport>();
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);
  private readonly scroll = viewChild<ElementRef<HTMLElement>>('scroll');
  protected readonly rows = computed(() => flickRows(this.report(), this.paths.analysis()));

  constructor() {
    afterRenderEffect(() => {
      const m = this.focus.selected();
      if (m) this.keepInView(m.n);
    });
  }

  /** Centers the row in the list when it is out of view, scrolling the list only. */
  private keepInView(n: number): void {
    const box = this.scroll()?.nativeElement;
    const row = box?.querySelector<HTMLElement>(`tr[data-n="${n}"]`);
    if (!box || !row) return;
    const top = row.getBoundingClientRect().top - box.getBoundingClientRect().top;
    if (top >= 0 && top + row.offsetHeight <= box.clientHeight) return;
    box.scrollTop += top - box.clientHeight / 2 + row.offsetHeight / 2;
  }
}
