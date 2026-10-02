import { DecimalPipe } from '@angular/common';
import { Component, computed, input } from '@angular/core';
import { Recording, Report, Source } from '../../api';
import { formatSize, KIND_LABELS } from '../../format';
import { StampPipe } from '../../stamp-pipe';

const SOURCES: Record<Source, string> = {
  stats: 'Stats file',
  hud: 'Session HUD',
  aimlab: 'Aim Lab HUD',
  video: 'Video only',
};

const SOURCE_DETAILS: Record<Source, string> = {
  stats: "Kills, shots and score from KovaaK's stats file",
  hud: "Kills and shots read from KovaaK's session HUD in the video",
  aimlab: "Hits, misses and score read from Aim Lab's POINTS in the video",
  video: 'Kills found in the video alone: no shots, misses or score',
};

/** The open recording's title, kind and data source, and its score, time and size. */
@Component({
  selector: 'app-run-header',
  imports: [DecimalPipe, StampPipe],
  templateUrl: './run-header.html',
  styleUrl: './run-header.scss',
})
export class RunHeader {
  readonly recording = input.required<Recording>();
  readonly report = input<Report | null>(null);
  protected readonly kindLabels = KIND_LABELS;
  protected readonly size = computed(() => formatSize(this.recording().size));

  /** Where the review's kills came from, once there is a review; before that, whether a stats file was found. */
  protected readonly source = computed(() => {
    const s = this.report()?.summary.info.source;
    if (s) return SOURCES[s];
    return this.recording().stats ? 'Stats file' : 'No stats file';
  });
  protected readonly sourceDetail = computed(() => {
    const s = this.report()?.summary.info.source;
    return s ? SOURCE_DETAILS[s] : null;
  });
}
