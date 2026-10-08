/**
 * The run page's title line. In: the open recording (scenario, score, time, size, whether it has a
 * stats file) and its report's kill source. Out: the scenario's name with a badge for where the
 * kills (a tracking run's score and its bots' deaths) came from, and a line with the score, time
 * and size.
 */

import { DecimalPipe } from '@angular/common';
import { Component, computed, input } from '@angular/core';
import { Recording, Report, Source } from '../../api';
import { formatSize } from '../../format';
import { StampPipe } from '../../stamp-pipe';

/** The source badge's tone (themes/controls.scss `.badge`, data-tone): neutral, or good. */
type SourceTone = 'neutral' | 'good';

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

/**
 * Where a tracking run's score and its bots' deaths came from, as its label says it: a tracking run
 * has no kills to count, and the video alone still gives its time on the target.
 */
const TRACK_SOURCES: Record<Source, string> = {
  stats: 'Score from the stats file',
  hud: "Bots' deaths read from KovaaK's HUD",
  aimlab: "Read from Aim Lab's HUD",
  video: 'From the video alone (no score or accuracy)',
};

/** Each tracking run's source in more words, for the badge's tooltip. */
const TRACK_SOURCE_DETAILS: Record<Source, string> = {
  stats: "Score, accuracy and the bots' deaths from KovaaK's stats file",
  hud: "The bots' deaths read from KovaaK's session HUD in the video",
  aimlab: "Read from Aim Lab's POINTS in the video",
  video: 'The time on the target measured in the video alone: no score or accuracy',
};

/** The open recording's title and data source, and its score, time and size. */
@Component({
  selector: 'app-run-header',
  imports: [DecimalPipe, StampPipe],
  templateUrl: './run-header.html',
  styleUrl: './run-header.scss',
})
export class RunHeader {
  /** The open recording. */
  readonly recording = input.required<Recording>();
  /** The recording's report, or null before a review. */
  readonly report = input<Report | null>(null);
  /** Whether the report is a tracking run's, whose source gives a score and deaths, not kills. */
  private readonly tracking = computed(() => this.report()?.mode === 'track');
  /** The video file's size in whole megabytes ("1234 MB"). */
  protected readonly size = computed(() => formatSize(this.recording().size));

  /**
   * Where the review's kills came from, once there is a review; before that, whether a stats file
   * was found.
   */
  protected readonly source = computed(() => {
    const source = this.report()?.summary.info.source;
    if (source) return (this.tracking() ? TRACK_SOURCES : SOURCES)[source];
    return this.recording().stats ? 'Stats file' : 'No stats file';
  });
  /** The badge's tooltip: what the review got from its kill source; null before a review. */
  protected readonly sourceDetail = computed(() => {
    const source = this.report()?.summary.info.source;
    return source ? (this.tracking() ? TRACK_SOURCE_DETAILS : SOURCE_DETAILS)[source] : null;
  });
  /** Good when the kills are the stats file's, as exact as the review gets. */
  protected readonly sourceTone = computed<SourceTone>(() =>
    this.report()?.summary.info.source === 'stats' ? 'good' : 'neutral',
  );
}
