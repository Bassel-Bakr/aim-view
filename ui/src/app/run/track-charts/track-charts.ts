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
 * A tracking run at a glance: the time on the bot 10 s at a time, how far the crosshair was from the bot's center
 * line, where it sat around the moving bot, and how long it took to get back on the bot after each of its turns. A
 * stretch or a turn can be clicked to go there in the video.
 */
@Component({
  selector: 'app-track-charts',
  templateUrl: './track-charts.html',
  styleUrl: './track-charts.scss',
})
export class TrackCharts {
  readonly report = input.required<TrackReport>();
  readonly tracks = input.required<Tracks>();
  private readonly playback = inject(Playback);
  protected readonly window = WINDOW;
  private readonly timeline = computed(() => timeline(this.report(), this.tracks()));
  protected readonly onTarget = computed(() => onTargetWindows(this.report(), this.timeline()));
  protected readonly spread = computed(() => distanceSpread(this.tracks(), this.timeline()));
  protected readonly around = computed(() => aroundMap(this.report()));
  protected readonly turns = computed(() => turnsBack(this.report(), this.timeline()));

  protected goTo(seconds: number): void {
    this.playback.seek(seconds);
  }
}
