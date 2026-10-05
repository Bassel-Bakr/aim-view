import { Component, computed, inject, input } from '@angular/core';
import { ClickReport } from '../../api';
import { PathCost } from '../fastest-path/path-cost';
import { FlickFocus } from '../flick-focus';
import { flickRows } from '../kills-table/kill-rows';
import { KillsTable } from '../kills-table/kills-table';

/**
 * Every flick of a clicking run, beside the video: the kills table (sorted by a click on a header, grouped and its
 * columns hidden from its menus), where a click on a kill replays it slowed down. The flick in focus is marked and
 * kept in view inside the table's box (never scrolling the page); "Follow the video" moves the focus with the video.
 */
@Component({
  selector: 'app-flick-list',
  imports: [KillsTable],
  templateUrl: './flick-list.html',
  styleUrl: './flick-list.scss',
})
export class FlickList {
  readonly report = input.required<ClickReport>();
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);
  protected readonly rows = computed(() => flickRows(this.report(), this.paths.analysis()));
}
