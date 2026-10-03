import { Component, computed, inject, input } from '@angular/core';
import { ClickReport as ClickReportData } from '../../api';
import { extraShots, pickText } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { RunCharts } from '../run-charts/run-charts';
import { WhatIfSection } from '../what-if-section/what-if-section';
import {
  clickWhatIf,
  directionRows,
  distanceRows,
  killStats,
  PathSummary,
  runStats,
  sourceNote,
} from './click-stats';

/**
 * A clicking run's report under the video: the whole run's cards (or the picked kill's, with the run's medians), the
 * run at a glance, the kills by distance and by direction, and what would raise the score. Where the time goes and the
 * checks are beside the video (click-side).
 */
@Component({
  imports: [RunCharts, WhatIfSection],
  selector: 'app-click-report',
  templateUrl: './click-report.html',
  styleUrl: './click-report.scss',
})
export class ClickReport {
  readonly report = input.required<ClickReportData>();
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);

  protected readonly picked = this.focus.selected;
  private readonly pathSummary = computed<PathSummary | null>(() => {
    const a = this.paths.analysis();
    return a
      ? { share: a.share, total: a.total, extra: extraShots(a, this.report(), a.total) }
      : null;
  });
  protected readonly stats = computed(() => {
    const m = this.picked();
    const s = this.report().summary;
    return m
      ? killStats(m, s, pickText(this.paths.analysis(), m.n), this.report().flicks)
      : runStats(s, this.pathSummary(), this.report().flicks, this.report().fps);
  });
  protected readonly source = computed(() => sourceNote(this.report().summary));
  protected readonly byDistance = computed(() => distanceRows(this.report().summary.by_distance));
  protected readonly byDirection = computed(() =>
    directionRows(this.report().summary.by_direction),
  );
  protected readonly whatIf = computed(() => clickWhatIf(this.report().summary.what_if));
}
