import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { FlickFocus } from '../flick-focus';
import { killCurve, profileChart } from './flick-profile-model';

/**
 * The flick speed profile: the camera's speed through the run's flicks, each as a share of its own top speed against
 * its time as a share of the flick, averaged, with the middle half of the flicks as a band and the top speed marked.
 * The kill in focus is drawn over it. It changes only when the report or the focus does, so it is drawn through the
 * template.
 */
@Component({
  selector: 'app-flick-profile',
  templateUrl: './flick-profile.html',
  styleUrl: './flick-profile.scss',
})
export class FlickProfileChart {
  readonly report = input.required<ClickReport>();
  private readonly focus = inject(FlickFocus);
  protected readonly chart = computed(() => profileChart(this.report().summary.flick_profile));
  protected readonly flicks = computed(() => this.report().summary.flick_profile?.flicks ?? 0);
  protected readonly picked = computed(() => {
    const c = this.chart();
    return c ? killCurve(this.focus.selected(), c) : null;
  });
}
