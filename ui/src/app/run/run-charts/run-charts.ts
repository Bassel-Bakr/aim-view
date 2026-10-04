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
 * A clicking run at a glance: every kill's time through the run, its time against its distance, where each click
 * landed on its target, and every flick's speed; then where each kill's time went, where flicks land, flick time
 * against distance, flick speed by direction, and the pace through the run. Each kill can be clicked to play it; the
 * kill in focus is marked. The charts change only when the report or the focus does, so they are drawn through the
 * template.
 */
@Component({
  selector: 'app-run-charts',
  templateUrl: './run-charts.html',
  styleUrl: './run-charts.scss',
})
export class RunCharts {
  readonly report = input.required<ClickReport>();
  protected readonly focus = inject(FlickFocus);
  protected readonly killTimes = computed(() => killTimes(this.report()));
  protected readonly fitts = computed(() => fittsChart(this.report()));
  protected readonly clicks = computed(() => clickGroup(this.report()));
  protected readonly speeds = computed(() => flickSpeeds(this.report()));
  /** The speed curve of the kill in focus, drawn over the others. */
  protected readonly focusedSpeed = computed(
    () => this.speeds().lines.find((line) => line.flick === this.focus.selected()) ?? null,
  );
  protected readonly words = CHART_WORDS;
  protected readonly shares = computed(() => killShares(this.report()));
  protected readonly landing = computed(() => landings(this.report()));
  protected readonly flickTime = computed(() => flickTimes(this.report()));
  protected readonly wheel = computed(() => directionWheel(this.report()));
  protected readonly pace = computed(() => pace(this.report()));
  /** The direction of the kill in focus, as the wheel's wedge index. */
  protected readonly selectedSector = computed(() => {
    const kill = this.focus.selected();
    return kill ? sector(kill.direction_deg) : null;
  });
}
