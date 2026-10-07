/**
 * The column beside a clicking run's video.
 *
 * In: the clicking report, the kill picked in the flick list (FlickFocus) and the fastest-path
 * analysis once the tracks are in (PathCost).
 * Out: the time budget, the TTK by distance bars and the checks, those to work on first.
 */

import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { pathing } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { averageReload, budget } from '../report/budget';
import { distanceBars, sortedIssues } from '../report/click-stats';

/**
 * Beside a clicking run's video: where a kill's time goes (the picked kill's against the run's),
 * the TTK by distance, and the checks (Pathing among them once the tracks are in), each opening to
 * say why.
 */
@Component({
  selector: 'app-click-side',
  templateUrl: './click-side.html',
  styleUrl: './click-side.scss',
})
export class ClickSide {
  /** The clicking run's report. */
  readonly report = input.required<ClickReport>();
  /** The kill picked in the flick list, whose budget shows beside the run's. */
  protected readonly focus = inject(FlickFocus);
  /** The fastest-path analysis, for the Pathing check. */
  private readonly paths = inject(PathCost);

  /**
   * Where the time goes: the average kill's steps, or the picked kill's above the average's; null
   * when the report has no budget.
   */
  protected readonly budget = computed(() =>
    budget(
      this.report().summary.budget,
      this.focus.selected(),
      averageReload(this.report().summary, this.report().flicks),
    ),
  );
  /** The TTK by distance bars, one per distance band. */
  protected readonly distance = computed(() => distanceBars(this.report().summary.by_distance));
  /**
   * The Pathing check and its costliest picks; null while the tracks load or no pick had a choice.
   */
  protected readonly pathing = computed(() => pathing(this.paths.analysis(), this.report()));
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
