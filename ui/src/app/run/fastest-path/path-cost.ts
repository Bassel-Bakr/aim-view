import { computed, inject, Injectable } from '@angular/core';
import { Review } from '../review';
import { analysePaths, PathAnalysis } from './path-analysis';

/** A clicking run's picks against the fastest order, worked out once its report and tracks are in. */
@Injectable({ providedIn: 'root' })
export class PathCost {
  private readonly review = inject(Review);

  readonly analysis = computed<PathAnalysis | null>(() => {
    const r = this.review.report.hasValue() ? this.review.report.value() : null;
    const t = this.review.tracks.hasValue() ? this.review.tracks.value() : null;
    return r?.mode === 'click' && t ? analysePaths(r, t) : null;
  });
}
