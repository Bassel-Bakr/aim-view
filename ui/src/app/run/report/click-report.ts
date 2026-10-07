/**
 * A clicking run's report under the video.
 *
 * In: the clicking report (run.ts), the kill in focus (FlickFocus) and the fastest-path analysis
 * (PathCost).
 * Out: the number cards, "The run at a glance" (run-charts, flick-profile), the by-distance and
 * by-direction tables, and the what-if table.
 */

import { Component, computed, inject, input } from '@angular/core';
import { ClickReport as ClickReportData } from '../../api';
import { DataColumn, fieldColumn } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';
import { extraShots, pickText } from '../fastest-path/path-analysis';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { FlickProfileChart } from '../flick-profile/flick-profile';
import { RunCharts } from '../run-charts/run-charts';
import { WhatIfSection } from '../what-if-section/what-if-section';
import {
  clickWhatIf,
  DirectionRow,
  directionRows,
  DistanceRow,
  distanceRows,
  killStats,
  PathSummary,
  runStats,
  sourceNote,
} from './click-stats';

/** The by-distance table's columns: a band of distance a row. */
const DISTANCE_COLUMNS: readonly DataColumn<DistanceRow>[] = [
  fieldColumn('band', 'Distance', { rowHeader: true }),
  fieldColumn('flicks', 'Flicks'),
  fieldColumn('kill', 'Median TTK'),
  fieldColumn('reaction', 'Reaction'),
  fieldColumn('short', 'Underflicks'),
  fieldColumn('past', 'Overflicks'),
  fieldColumn('still', 'Confirmation'),
];

/** The by-direction table's columns: a direction of flick (a 45-degree sector) a row. */
const DIRECTION_COLUMNS: readonly DataColumn<DirectionRow>[] = [
  fieldColumn('toward', 'Toward', { rowHeader: true }),
  fieldColumn('flicks', 'Flicks'),
  fieldColumn('kill', 'Median TTK'),
  fieldColumn('distance', 'Distance'),
  fieldColumn('beyond', 'For its distance', {
    title: "Median time beyond what the distance predicts (Fitts' law fitted to the run)",
  }),
  fieldColumn('short', 'Underflicks'),
  fieldColumn('past', 'Overflicks'),
];

/**
 * A clicking run's report under the video: the whole run's cards (or the picked kill's, with the
 * run's medians), the run at a glance, the kills by distance and by direction, and what would raise
 * the score. Where the time goes and the checks are beside the video (click-side).
 */
@Component({
  imports: [DataTable, FlickProfileChart, RunCharts, WhatIfSection],
  selector: 'app-click-report',
  templateUrl: './click-report.html',
  styleUrl: './click-report.scss',
})
export class ClickReport {
  /** The clicking run's report. */
  readonly report = input.required<ClickReportData>();
  /** The kill in focus; "Whole run" clears it. */
  protected readonly focus = inject(FlickFocus);
  /** The fastest-path analysis, for the path cards. */
  private readonly paths = inject(PathCost);

  /** The picked kill, whose cards show in place of the whole run's; null for the whole run. */
  protected readonly picked = this.focus.selected;
  /** The run's picks against the fastest order, for its cards; null while the tracks load. */
  private readonly pathSummary = computed<PathSummary | null>(() => {
    const a = this.paths.analysis();
    return a
      ? { share: a.share, total: a.total, extra: extraShots(a, this.report(), a.total) }
      : null;
  });
  /** The number cards: the picked kill's, else the whole run's. */
  protected readonly stats = computed(() => {
    const flick = this.picked();
    const summary = this.report().summary;
    return flick
      ? killStats(
          flick,
          summary,
          pickText(this.paths.analysis(), flick.kill_number),
          this.report().flicks,
        )
      : runStats(summary, this.pathSummary(), this.report().flicks, this.report().fps);
  });
  /** The note under the cards: where the kills came from and what was measured. */
  protected readonly source = computed(() => sourceNote(this.report().summary));
  /** The by-distance table's rows. */
  protected readonly byDistance = computed(() => distanceRows(this.report().summary.by_distance));
  /** The by-direction table's rows. */
  protected readonly byDirection = computed(() =>
    directionRows(this.report().summary.by_direction),
  );
  /** The by-distance table's columns. */
  protected readonly distanceColumns = DISTANCE_COLUMNS;
  /** The by-direction table's columns. */
  protected readonly directionColumns = DIRECTION_COLUMNS;
  /** What would raise the score: the extra kills and score each change would give. */
  protected readonly whatIf = computed(() => clickWhatIf(this.report().summary.what_if));
}
