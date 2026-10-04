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
import { arrow, formatCount, formatDegrees, formatEnded } from '../../format';
import { PathAnalysis, pickText } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { micro, microSplit } from '../report/budget';

/** A flick as the list shows it. */
export interface FlickRow {
  flick: Flick;
  killNumber: number;
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
  /** The kill's steps in one cell, to keep the table narrow: reaction, flick and micro, "67 · 183 · 125 ms". */
  steps: string;
  /** The micro's two parts, on hover: "120 ms onto the target, 80 ms settling". */
  microSplit: string;
  offCenter: string;
  micros: string;
  spawn: string;
}

/**
 * A number rounded after scaling (seconds by 1000: milliseconds), without its unit, which the column's header names
 * (the table stays narrow enough to fit beside the player); a dash when not measured.
 */
function whole(value: number | null | undefined, scale = 1): string {
  return value == null ? '–' : String(Math.round(scale * value));
}

export function flickRows(report: ClickReport, paths: PathAnalysis | null): FlickRow[] {
  return report.flicks.map((flick) => ({
    flick: flick,
    killNumber: flick.kill_number,
    distance: `${flick.D0.toFixed(1)}° ${arrow(flick.direction_deg)}`,
    ttk: whole(flick.total, 1000),
    landed: formatEnded(flick.end_left, report.summary.radius),
    confirmation: whole(flick.still, 1000),
    flickSpeed: whole(flick.peak),
    onTheMove: whole(flick.click_speed),
    shots: formatCount(flick.shots),
    missed: (flick.shots ?? 0) > 1,
    pathing: pickText(paths, flick.kill_number),
    steps: [flick.react, flick.flick, micro(flick)]
      .map((seconds) => whole(seconds, 1000))
      .join(' · '),
    microSplit: flick.parts ? microSplit(flick.parts) : '',
    offCenter: formatDegrees(flick.click_off),
    micros: formatCount(flick.corrections),
    spawn: flick.spawned ? 'yes' : 'no',
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
      const flick = this.focus.selected();
      if (flick) this.keepInView(flick.kill_number);
    });
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
