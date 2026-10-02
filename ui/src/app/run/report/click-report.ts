import { Component, computed, inject, input } from '@angular/core';
import { ClickReport as ClickReportData } from '../../api';
import { FlickFocus } from '../flick-focus';
import { budget } from './budget';
import {
  directionRows,
  distanceRows,
  killStats,
  runStats,
  sortedIssues,
  sourceNote,
} from './click-stats';
import { clickReportStyles } from '@themes/click-report.styles';
import { button, card, note, swatch } from '@themes/controls.styles';
import { slotClasses } from '@themes/slot-classes';

/**
 * A clicking run's report: the whole run's cards (or the picked kill's, with the run's medians), where a kill's time
 * goes, the checks, and the kills by distance and by direction.
 */
@Component({
  selector: 'app-click-report',
  templateUrl: './click-report.html',
})
export class ClickReport {
  readonly report = input.required<ClickReportData>();
  protected readonly focus = inject(FlickFocus);
  protected readonly ui = slotClasses(clickReportStyles());
  protected readonly card = slotClasses(card());
  protected readonly note = note();
  protected readonly swatch = swatch();
  protected readonly button = button();

  protected readonly picked = this.focus.selected;
  protected readonly stats = computed(() => {
    const m = this.picked();
    const s = this.report().summary;
    return m ? killStats(m, s) : runStats(s);
  });
  protected readonly source = computed(() => sourceNote(this.report().summary));
  protected readonly budget = computed(() => budget(this.report().summary.budget, this.picked()));
  protected readonly issues = computed(() => sortedIssues(this.report().issues));
  protected readonly byDistance = computed(() => distanceRows(this.report().summary.by_distance));
  protected readonly byDirection = computed(() =>
    directionRows(this.report().summary.by_direction),
  );
}
