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

const DISTANCE_COLUMNS: readonly DataColumn<DistanceRow>[] = [
  fieldColumn('band', 'Distance', { rowHeader: true }),
  fieldColumn('flicks', 'Flicks'),
  fieldColumn('kill', 'Median TTK'),
  fieldColumn('reaction', 'Reaction'),
  fieldColumn('short', 'Underflicks'),
  fieldColumn('past', 'Overflicks'),
  fieldColumn('still', 'Confirmation'),
];

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
 * A clicking run's report under the video: the whole run's cards (or the picked kill's, with the run's medians), the
 * run at a glance, the kills by distance and by direction, and what would raise the score. Where the time goes and the
 * checks are beside the video (click-side).
 */
@Component({
  imports: [DataTable, FlickProfileChart, RunCharts, WhatIfSection],
  selector: 'app-click-report',
  templateUrl: './click-report.html',
  styleUrl: './click-report.scss',
})
export class ClickReport {
  readonly report = input.required<ClickReportData>();
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);

  protected readonly picked = this.focus.selected;
  private readonly pathSummary = computed<PathSummary | null>(() => {
    const a = this.paths.analysis();
    return a
      ? { share: a.share, total: a.total, extra: extraShots(a, this.report(), a.total) }
      : null;
  });
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
  protected readonly source = computed(() => sourceNote(this.report().summary));
  protected readonly byDistance = computed(() => distanceRows(this.report().summary.by_distance));
  protected readonly byDirection = computed(() =>
    directionRows(this.report().summary.by_direction),
  );
  protected readonly distanceColumns = DISTANCE_COLUMNS;
  protected readonly directionColumns = DIRECTION_COLUMNS;
  protected readonly whatIf = computed(() => clickWhatIf(this.report().summary.what_if));
}
