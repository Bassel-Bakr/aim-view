/**
 * The run page's title line. In: the open recording (scenario, score, time, size, whether it has a
 * stats file) and its report's kill source. Out: the scenario's name with a badge for where the
 * kills came from, and a line with the score, time and size.
 */

import { DecimalPipe } from '@angular/common';
import { Component, computed, input } from '@angular/core';
import { Recording, Report, Source } from '../../api';
import { Badge, BadgeTone } from '../../controls/badge';
import { formatSize } from '../../format';
import { StampPipe } from '../../stamp-pipe';

/** Where a review's kills came from, as its label says it. */
const SOURCES: Record<Source, string> = {
  stats: 'Kills from the stats file',
  hud: "Kills read from KovaaK's HUD",
  aimlab: "Kills read from Aim Lab's HUD",
  video: 'Kills from the video alone (no score, shots or accuracy)',
};

/** Each kill source in more words, for the badge's tooltip: what the review got from it. */
const SOURCE_DETAILS: Record<Source, string> = {
  stats: "Kills, shots and score from KovaaK's stats file",
  hud: "Kills and shots read from KovaaK's session HUD in the video",
  aimlab: "Hits, misses and score read from Aim Lab's POINTS in the video",
  video: 'Kills found in the video alone: no shots, misses or score',
};

/** The open recording's title and data source, and its score, time and size. */
@Component({
  selector: 'app-run-header',
  imports: [Badge, DecimalPipe, StampPipe],
  templateUrl: './run-header.html',
  styleUrl: './run-header.scss',
})
export class RunHeader {
  /** The open recording. */
  readonly recording = input.required<Recording>();
  /** The recording's report, or null before a review. */
  readonly report = input<Report | null>(null);
  /** The video file's size in whole megabytes ("1234 MB"). */
  protected readonly size = computed(() => formatSize(this.recording().size));

  /**
   * Where the review's kills came from, once there is a review; before that, whether a stats file
   * was found.
   */
  protected readonly source = computed(() => {
    const source = this.report()?.summary.info.source;
    if (source) return SOURCES[source];
    return this.recording().stats ? 'Stats file' : 'No stats file';
  });
  /** The badge's tooltip: what the review got from its kill source; null before a review. */
  protected readonly sourceDetail = computed(() => {
    const source = this.report()?.summary.info.source;
    return source ? SOURCE_DETAILS[source] : null;
  });
  /** Good when the kills are the stats file's, as exact as the review gets. */
  protected readonly sourceTone = computed<BadgeTone>(() =>
    this.report()?.summary.info.source === 'stats' ? 'good' : 'neutral',
  );
}
