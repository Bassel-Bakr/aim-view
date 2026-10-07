/**
 * The flick speed profile chart in a clicking run's report.
 *
 * In: the clicking report's flick profile and the kill in focus (FlickFocus).
 * Out: an SVG chart (flick-profile.html) with the run's average and the kill's own curve.
 */

import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { FlickFocus } from '../flick-focus';
import { killCurve, profileChart } from './flick-profile-model';

/**
 * The flick speed profile: the camera's speed through the run's flicks, each as a share of its own
 * top speed against its time as a share of the flick, averaged, with the middle half of the flicks
 * as a band and the top speed marked. The kill in focus is drawn over it. It changes only when the
 * report or the focus does, so it is drawn through the template.
 */
@Component({
  selector: 'app-flick-profile',
  templateUrl: './flick-profile.html',
  styleUrl: './flick-profile.scss',
})
export class FlickProfileChart {
  /** The clicking run's report. */
  readonly report = input.required<ClickReport>();
  /** The kill in focus, whose own curve is drawn over the average. */
  private readonly focus = inject(FlickFocus);
  /** The chart's shapes; null when the report has no flick profile. */
  protected readonly chart = computed(() => profileChart(this.report().summary.flick_profile));
  /** How many flicks the profile averages, for the chart's note. */
  protected readonly flicks = computed(() => this.report().summary.flick_profile?.flicks ?? 0);
  /** The kill in focus's curve, as an SVG path; null with no kill in focus or no curve. */
  protected readonly picked = computed(() => {
    const model = this.chart();
    return model ? killCurve(this.focus.selected(), model) : null;
  });
}
