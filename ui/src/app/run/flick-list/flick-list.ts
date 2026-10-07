/**
 * The flick list beside a clicking run's video.
 *
 * In: the clicking report, the fastest-path analysis (PathCost) for each kill's Pathing cost, and
 * the flick in focus (FlickFocus).
 * Out: the kills table; a click on a kill picks it and plays it (FlickFocus `play`).
 */

import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { flickRows } from '../kills-table/kill-rows';
import { KillsTable } from '../kills-table/kills-table';

/**
 * Every flick of a clicking run, beside the video: the kills table (sorted by a click on a header,
 * grouped and its columns hidden from its menus), where a click on a kill replays it slowed down.
 * The flick in focus is marked and kept in view inside the table's box (never scrolling the page);
 * "Follow the video" moves the focus with the video.
 */
@Component({
  selector: 'app-flick-list',
  imports: [KillsTable],
  templateUrl: './flick-list.html',
  styleUrl: './flick-list.scss',
})
export class FlickList {
  /** The clicking run's report. */
  readonly report = input.required<ClickReport>();
  /** The flick in focus: the one picked, or the one the video is at while following it. */
  protected readonly focus = inject(FlickFocus);
  /** The fastest-path analysis, for each kill's Pathing cost. */
  private readonly paths = inject(PathCost);
  /** The table's rows, one per flick, with the Pathing costs once the tracks are in. */
  protected readonly rows = computed(() => flickRows(this.report(), this.paths.analysis()));
}
