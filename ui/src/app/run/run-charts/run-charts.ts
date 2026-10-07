/**
 * "The run at a glance" for a clicking run, in its report.
 *
 * In: the clicking report (report/click-report.html) and the kill in focus (FlickFocus).
 * Out: nine SVG charts (run-charts.html, shapes from run-charts-model.ts); a click on a kill plays
 * it.
 */

import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { FlickFocus } from '../flick-focus';
import {
  CHART_WORDS,
  clickGroup,
  directionWheel,
  fittsChart,
  flickSpeeds,
  flickTimes,
  killShares,
  killTimes,
  landings,
  pace,
  sector,
} from './run-charts-model';

/**
 * A clicking run at a glance: every kill's time through the run, its time against its distance,
 * where each click landed on its target, and every flick's speed; then where each kill's time went,
 * where flicks land, flick time against distance, flick speed by direction, and the pace through
 * the run. Each kill can be clicked to play it; the kill in focus is marked. The charts change only
 * when the report or the focus does, so they are drawn through the template.
 */
@Component({
  selector: 'app-run-charts',
  templateUrl: './run-charts.html',
  styleUrl: './run-charts.scss',
})
export class RunCharts {
  /** The clicking run's report. */
  readonly report = input.required<ClickReport>();
  /** The kill in focus, marked on every chart; a click on a kill plays it. */
  protected readonly focus = inject(FlickFocus);
  /** Every kill's TTK through the run, in its parts. */
  protected readonly killTimes = computed(() => killTimes(this.report()));
  /** Each kill's TTK against its distance, with the run's Fitts' law curve. */
  protected readonly fitts = computed(() => fittsChart(this.report()));
  /** Where each click landed on its target. */
  protected readonly clicks = computed(() => clickGroup(this.report()));
  /** Every flick's speed, lined up at the end of its main flick. */
  protected readonly speeds = computed(() => flickSpeeds(this.report()));
  /** The speed curve of the kill in focus, drawn over the others. */
  protected readonly focusedSpeed = computed(
    () => this.speeds().lines.find((line) => line.flick === this.focus.selected()) ?? null,
  );
  /** The charts' words for a kill's parts and a flick's landing. */
  protected readonly words = CHART_WORDS;
  /** Each kill's time as shares of its four parts. */
  protected readonly shares = computed(() => killShares(this.report()));
  /** Where each flick's main movement ended against the target's center. */
  protected readonly landing = computed(() => landings(this.report()));
  /** Each flick's time against its distance, with Fitts' law fitted to the flicks. */
  protected readonly flickTime = computed(() => flickTimes(this.report()));
  /** Flick speed by direction, as a wheel of wedges. */
  protected readonly wheel = computed(() => directionWheel(this.report()));
  /** The kills in each 10 s of the run. */
  protected readonly pace = computed(() => pace(this.report()));
  /** The direction of the kill in focus, as the wheel's wedge index. */
  protected readonly selectedSector = computed(() => {
    const kill = this.focus.selected();
    return kill ? sector(kill.direction_deg) : null;
  });
}
