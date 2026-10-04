import { computed, effect, inject, Service, signal, untracked } from '@angular/core';
import { ClickReport, Flick, isClickReport } from '../api';
import { Library } from '../services/library';
import { Playback } from './playback';
import { Review } from '../services/review';
import { flickAt } from './track';

const FOLLOW_KEY = 'aimview-follow';
/** A replayed flick starts this long before the flick and stops this long after its kill, in seconds. */
const BEFORE_FLICK = 0.15;
const AFTER_KILL = 1;

/**
 * The flick in focus on a clicking run: the one picked in the list, or, with "follow the video" on, the one on screen.
 * Following runs on every frame but sets the signal only when the flick changes. A replay keeps its flick.
 */
@Service()
export class FlickFocus {
  private readonly playback = inject(Playback);
  private readonly review = inject(Review);
  readonly selected = signal<Flick | null>(null);
  readonly follow = signal(localStorage.getItem(FOLLOW_KEY) !== '0');

  private readonly report = computed<ClickReport | null>(() => {
    const report = this.review.report.hasValue() ? this.review.report.value() : null;
    return isClickReport(report) ? report : null;
  });

  constructor() {
    const library = inject(Library);
    effect(() => {
      library.selectedId();
      untracked(() => this.selected.set(null));
    });
    this.playback.onFrame((seconds) => this.followVideo(seconds));
  }

  /** Picks a flick and plays it, from just before it starts until just after its kill. */
  play(flick: Flick): void {
    const report = this.report();
    if (!report) return;
    this.selected.set(flick);
    this.playback.playRange(
      Math.max(0, flick.start_frame / report.fps - BEFORE_FLICK),
      flick.kill_frame / report.fps + AFTER_KILL,
    );
  }

  /** The previous or next flick from the one in focus (or on screen), played. */
  step(forward: boolean): void {
    const report = this.report();
    if (!report?.flicks.length) return;
    const at =
      this.selected() ?? flickAt(report.flicks, Math.round(this.playback.time * report.fps));
    const i = at ? report.flicks.indexOf(at) : -1;
    this.play(
      report.flicks[Math.max(0, Math.min(report.flicks.length - 1, forward ? i + 1 : i - 1))],
    );
  }

  setFollow(on: boolean): void {
    this.follow.set(on);
    localStorage.setItem(FOLLOW_KEY, on ? '1' : '0');
    if (on) this.followVideo(this.playback.time);
  }

  private followVideo(seconds: number): void {
    const report = this.report();
    if (!report || !this.follow() || this.playback.replaying) return;
    const at = flickAt(report.flicks, Math.round(seconds * report.fps));
    if (at && at !== this.selected()) this.selected.set(at);
  }
}
