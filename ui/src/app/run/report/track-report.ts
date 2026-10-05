import { Component, computed, input } from '@angular/core';
import { TrackReport as TrackReportData, Tracks } from '../../api';
import { DataColumn, fieldColumn } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';
import { TrackCharts } from '../track-charts/track-charts';
import { WhatIfSection } from '../what-if-section/what-if-section';
import {
  HEADLINE_TILES,
  MotionRow,
  motionView,
  trackNote,
  trackStats,
  whatIfTable,
} from './track-stats';

const MOTION_COLUMNS: readonly DataColumn<MotionRow>[] = [
  fieldColumn('moving', 'Target moving', { rowHeader: true }),
  fieldColumn('time', 'Time'),
  fieldColumn('on', 'On target'),
  fieldColumn('distance', 'Distance from the line'),
  fieldColumn('lag', 'Behind or ahead'),
];

/**
 * A tracking run's report: the time on the bot and the drops off it, how the crosshair followed the bot's motion (by
 * direction too), and what would raise the accuracy.
 */
@Component({
  imports: [DataTable, TrackCharts, WhatIfSection],
  selector: 'app-track-report',
  templateUrl: './track-report.html',
  styleUrl: './track-report.scss',
})
export class TrackReport {
  readonly report = input.required<TrackReportData>();
  /** The tracks, for the charts; null while they load. */
  readonly tracks = input<Tracks | null>(null);
  protected readonly motionColumns = MOTION_COLUMNS;

  /** The cards after the headline's, which are above the video. */
  protected readonly stats = computed(() =>
    trackStats(this.report().summary).slice(HEADLINE_TILES),
  );
  protected readonly about = computed(() => trackNote(this.report().summary));
  protected readonly motion = computed(() => motionView(this.report().summary.motion ?? null));
  protected readonly whatIf = computed(() => whatIfTable(this.report().summary.what_if ?? []));
}
