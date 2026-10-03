import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { FlickFocus } from '../flick-focus';
import { clickGroup, fittsChart, flickSpeeds, killTimes } from './run-charts-model';

/**
 * A clicking run at a glance: every kill's time through the run, its time against its distance, where each click
 * landed on its target, and every flick's speed. Each kill can be clicked to play it; the kill in focus is marked.
 * The charts change only when the report or the focus does, so they are drawn through the template.
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
    () => this.speeds().lines.find((l) => l.flick === this.focus.selected()) ?? null,
  );
}
