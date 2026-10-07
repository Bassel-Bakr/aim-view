/**
 * The open run's fastest-path analysis, shared by the parts that show it. In: the review's clicking
 * report and the tracks without those the faint-target cut-off leaves out. Out: the flick list's
 * Pathing costs and the click side panel's Pathing check.
 */

import { computed, inject, Service } from '@angular/core';
import { isClickReport } from '../../api';
import { FaintCutoff } from '../../services/faint-cutoff';
import { Review } from '../../services/review';
import { analysePaths, PathAnalysis } from './path-analysis';

/**
 * A clicking run's picks against the fastest order, worked out once its report and tracks are in:
 * the tracks without those the faint-target cut-off leaves out.
 */
@Service()
export class PathCost {
  /** The open recording's review, for its report. */
  private readonly review = inject(Review);
  /** The faint-target cut-off, for the tracks it keeps. */
  private readonly faint = inject(FaintCutoff);

  /** The analysis; null for a tracking run or until the report and tracks are in. */
  readonly analysis = computed<PathAnalysis | null>(() => {
    const report = this.review.report.hasValue() ? this.review.report.value() : null;
    const tracks = this.faint.tracks();
    return isClickReport(report) && tracks ? analysePaths(report, tracks) : null;
  });
}
