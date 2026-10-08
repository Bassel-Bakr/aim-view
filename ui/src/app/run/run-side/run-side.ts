/**
 * The column beside a run's video.
 *
 * In: the run's report, the kill picked in the flick list (FlickFocus) and the fastest-path
 * analysis once the tracks are in (PathCost).
 * Out: a clicking run's time budget and TTK by distance bars, and any run's checks, those to work
 * on first.
 */

import { Component, computed, inject, input } from '@angular/core';
import { isClickReport, Report } from '../../api';
import { pathing } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { averageReload, budget } from '../report/budget';
import { distanceBars, sortedIssues } from '../report/click-stats';

/**
 * Beside a run's video: on a clicking run, where a kill's time goes (the picked kill's against the
 * run's) and the TTK by distance; on any run, the checks (a clicking run's Pathing among them once
 * the tracks are in), each opening to say why.
 */
@Component({
  selector: 'app-run-side',
  templateUrl: './run-side.html',
  styleUrl: './run-side.scss',
})
export class RunSide {
  /** The run's report: a clicking run's or a tracking run's. */
  readonly report = input.required<Report>();
  /** The report when the run is a clicking run, else null: only it has a budget, distances and paths. */
  private readonly clickReport = computed(() => {
    const report = this.report();
    return isClickReport(report) ? report : null;
  });
  /** The kill picked in the flick list, whose budget shows beside the run's. */
  protected readonly focus = inject(FlickFocus);
  /** The fastest-path analysis, for the Pathing check. */
  private readonly paths = inject(PathCost);

  /**
   * Where the time goes: the average kill's steps, or the picked kill's above the average's; null
   * when the report has no budget.
   */
  protected readonly budget = computed(() => {
    const report = this.clickReport();
    if (!report) return null;
    return budget(
      report.summary.budget,
      this.focus.selected(),
      averageReload(report.summary, report.flicks),
    );
  });
  /** The TTK by distance bars, one per distance band; none on a tracking run. */
  protected readonly distance = computed(() => {
    const report = this.clickReport();
    return report ? distanceBars(report.summary.by_distance) : [];
  });
  /**
   * The Pathing check and its costliest picks; null on a tracking run, while the tracks load or when
   * no pick had a choice.
   */
  protected readonly pathing = computed(() => {
    const report = this.clickReport();
    return report ? pathing(this.paths.analysis(), report) : null;
  });
  /** The core's checks and the Pathing check, those that need attention first. */
  protected readonly issues = computed(() => {
    const check = this.pathing();
    return sortedIssues(check ? [...this.report().issues, check.issue] : this.report().issues);
  });
  /** How many checks need attention, for the checks' heading. */
  protected readonly toWorkOn = computed(
    () => this.issues().filter((i) => i.flag === 'attention').length,
  );
}
