import { Component, computed, inject, input } from '@angular/core';
import { TrackReport, Tracks } from '../../api';
import { Playback } from '../playback';
import { timeline } from '../track';
import { aroundMap, distanceSpread, onTargetWindows, WINDOW } from './track-charts-model';

/**
 * A tracking run at a glance: the time on the bot 10 s at a time, how far the crosshair was from the bot's center
 * line, and where it sat around the moving bot. A stretch can be clicked to go there in the video.
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

  protected goTo(seconds: number): void {
    this.playback.seek(seconds);
  }
}
