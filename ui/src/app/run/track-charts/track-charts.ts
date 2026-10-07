/**
 * "The run at a glance" for a tracking run, in its report.
 *
 * In: the tracking report and the review's tracks (report/track-report.html).
 * Out: four SVG charts (track-charts.html, shapes from track-charts-model.ts); a click on a stretch
 * or a turn seeks the video (Playback).
 */

import { Component, computed, inject, input } from '@angular/core';
import { TrackReport, Tracks } from '../../api';
import { Playback } from '../playback';
import { timeline } from '../track';
import {
  aroundMap,
  distanceSpread,
  onTargetWindows,
  turnsBack,
  WINDOW,
} from './track-charts-model';

/**
 * A tracking run at a glance: the time on the bot 10 s at a time, how far the crosshair was from
 * the bot's center line, where it sat around the moving bot, and how long it took to get back on
 * the bot after each of its turns. A stretch or a turn can be clicked to go there in the video.
 */
@Component({
  selector: 'app-track-charts',
  templateUrl: './track-charts.html',
  styleUrl: './track-charts.scss',
})
export class TrackCharts {
  /** The tracking run's report. */
  readonly report = input.required<TrackReport>();
  /** The review's tracks, for the distance spread and the run moment by moment. */
  readonly tracks = input.required<Tracks>();
  /** The video, which a click on a stretch or a turn seeks. */
  private readonly playback = inject(Playback);
  /** A stretch's length in seconds, for the first chart's title. */
  protected readonly window = WINDOW;
  /** The run moment by moment: each frame's state and distance off the bot. */
  private readonly timeline = computed(() => timeline(this.report(), this.tracks()));
  /** The time on the bot, a stretch at a time. */
  protected readonly onTarget = computed(() => onTargetWindows(this.report(), this.timeline()));
  /** The spread of the distance from the bot's center line. */
  protected readonly spread = computed(() => distanceSpread(this.tracks(), this.timeline()));
  /** Where the crosshair sat around the moving bot. */
  protected readonly around = computed(() => aroundMap(this.report()));
  /** How long the crosshair took to get back on the bot after each turn. */
  protected readonly turns = computed(() => turnsBack(this.report(), this.timeline()));

  /** Seeks the video to `seconds`, where a clicked stretch or turn starts. */
  protected goTo(seconds: number): void {
    this.playback.seek(seconds);
  }
}
