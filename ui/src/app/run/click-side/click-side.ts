import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { pathing } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { budget } from '../report/budget';
import { distanceBars, sortedIssues } from '../report/click-stats';

/**
 * Beside a clicking run's video: where a kill's time goes (the picked kill's against the run's), the kill time by
 * distance, and the checks (Pathing among them once the tracks are in), each opening to say why.
 */
@Component({
  selector: 'app-click-side',
  templateUrl: './click-side.html',
  styleUrl: './click-side.scss',
})
export class ClickSide {
  readonly report = input.required<ClickReport>();
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);

  protected readonly budget = computed(() =>
    budget(this.report().summary.budget, this.focus.selected()),
  );
  protected readonly distance = computed(() => distanceBars(this.report().summary.by_distance));
  protected readonly pathing = computed(() => pathing(this.paths.analysis(), this.report()));
  protected readonly issues = computed(() => {
    const p = this.pathing();
    return sortedIssues(p ? [...this.report().issues, p.issue] : this.report().issues);
  });
  protected readonly toWorkOn = computed(
    () => this.issues().filter((i) => i.flag === 'attention').length,
  );
}
