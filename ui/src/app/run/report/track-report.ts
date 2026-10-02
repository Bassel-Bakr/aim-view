import { Component, computed, input } from '@angular/core';
import { TrackReport as TrackReportData } from '../../api';
import { motionView, trackNote, trackStats, whatIfRows } from './track-stats';
import { card, note } from '@themes/controls.styles';
import { slotClasses } from '@themes/slot-classes';
import { trackReportStyles } from '@themes/track-report.styles';

/**
 * A tracking run's report: the time on the bot and the drops off it, how the crosshair followed the bot's motion (by
 * direction too), and what would raise the accuracy.
 */
@Component({
  selector: 'app-track-report',
  templateUrl: './track-report.html',
})
export class TrackReport {
  readonly report = input.required<TrackReportData>();
  protected readonly ui = slotClasses(trackReportStyles());
  protected readonly card = slotClasses(card());
  protected readonly note = note();

  protected readonly stats = computed(() => trackStats(this.report().summary));
  protected readonly about = computed(() => trackNote(this.report().summary));
  protected readonly motion = computed(() => motionView(this.report().summary.motion));
  protected readonly whatIf = computed(() => whatIfRows(this.report().summary.what_if));
}
