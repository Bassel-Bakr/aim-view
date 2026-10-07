/**
 * The headline tiles above a run's video.
 *
 * In: the tiles run.ts works out from the report (report/click-stats.ts `runHeadline` for a
 * clicking run, the first cards of report/track-stats.ts `trackStats` for a tracking run).
 * Out: one tile each, its label, value and note, tinted when it needs attention or is good news.
 */

import { Component, input } from '@angular/core';
import { HeadlineTile } from '../report/click-stats';

/** The run in a few big numbers, above the video. A flagged tile is the review's "work on this". */
@Component({
  selector: 'app-headline',
  templateUrl: './headline.html',
  styleUrl: './headline.scss',
})
export class Headline {
  /** The tiles to show, in order; the template tracks them by label. */
  readonly tiles = input.required<HeadlineTile[]>();
}
