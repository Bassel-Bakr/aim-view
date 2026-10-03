import { computed, inject, Injectable } from '@angular/core';
import { isClickReport } from '../../api';
import { FaintCutoff } from '../../services/faint-cutoff';
import { Review } from '../../services/review';
import { analysePaths, PathAnalysis } from './path-analysis';

/**
 * A clicking run's picks against the fastest order, worked out once its report and tracks are in: the tracks without
 * those the faint-target cut-off leaves out.
 */
@Injectable({ providedIn: 'root' })
export class PathCost {
  private readonly review = inject(Review);
  private readonly faint = inject(FaintCutoff);

  readonly analysis = computed<PathAnalysis | null>(() => {
    const r = this.review.report.hasValue() ? this.review.report.value() : null;
    const t = this.faint.tracks();
    return isClickReport(r) && t ? analysePaths(r, t) : null;
  });
}
