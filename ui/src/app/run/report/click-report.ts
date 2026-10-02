import { Component, computed, inject, input } from '@angular/core';
import { Button } from '../../controls/button';
import { ClickReport as ClickReportData } from '../../api';
import { extraShots, pathing, pickText } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { budget } from './budget';
import {
  directionRows,
  distanceRows,
  killStats,
  PathSummary,
  runStats,
  sortedIssues,
  sourceNote,
} from './click-stats';

/**
 * A clicking run's report: the whole run's cards (or the picked kill's, with the run's medians), where a kill's time
 * goes, the checks (Pathing among them once the tracks are in), and the kills by distance and by direction.
 */
@Component({
  imports: [Button],
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
      ? killStats(m, s, pickText(this.paths.analysis(), m.n))
      : runStats(s, this.pathSummary());
  });
  protected readonly source = computed(() => sourceNote(this.report().summary));
  protected readonly budget = computed(() => budget(this.report().summary.budget, this.picked()));
  protected readonly pathing = computed(() => pathing(this.paths.analysis(), this.report()));
  protected readonly issues = computed(() => {
    const p = this.pathing();
    return sortedIssues(p ? [...this.report().issues, p.issue] : this.report().issues);
  });
  protected readonly byDistance = computed(() => distanceRows(this.report().summary.by_distance));
  protected readonly byDirection = computed(() =>
    directionRows(this.report().summary.by_direction),
  );
}
