import { computed, effect, inject, Injectable, signal, untracked } from '@angular/core';
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
@Injectable({ providedIn: 'root' })
export class FlickFocus {
  private readonly playback = inject(Playback);
  private readonly review = inject(Review);
  readonly selected = signal<Flick | null>(null);
  readonly follow = signal(localStorage.getItem(FOLLOW_KEY) !== '0');

  private readonly report = computed<ClickReport | null>(() => {
    const r = this.review.report.hasValue() ? this.review.report.value() : null;
    return isClickReport(r) ? r : null;
  });

  constructor() {
    const library = inject(Library);
    effect(() => {
      library.selectedId();
      untracked(() => this.selected.set(null));
    });
    this.playback.onFrame((t) => this.followVideo(t));
  }

  /** Picks a flick and plays it, from just before it starts until just after its kill. */
  play(flick: Flick): void {
    const r = this.report();
    if (!r) return;
    this.selected.set(flick);
    this.playback.playRange(
      Math.max(0, flick.start_frame / r.fps - BEFORE_FLICK),
      flick.kill_frame / r.fps + AFTER_KILL,
    );
  }

  /** The previous or next flick from the one in focus (or on screen), played. */
  step(forward: boolean): void {
    const r = this.report();
    if (!r?.flicks.length) return;
    const at = this.selected() ?? flickAt(r.flicks, Math.round(this.playback.time * r.fps));
    const i = at ? r.flicks.indexOf(at) : -1;
    this.play(r.flicks[Math.max(0, Math.min(r.flicks.length - 1, forward ? i + 1 : i - 1))]);
  }

  setFollow(on: boolean): void {
    this.follow.set(on);
    localStorage.setItem(FOLLOW_KEY, on ? '1' : '0');
    if (on) this.followVideo(this.playback.time);
  }

  private followVideo(t: number): void {
    const r = this.report();
    if (!r || !this.follow() || this.playback.replaying) return;
    const at = flickAt(r.flicks, Math.round(t * r.fps));
    if (at && at !== this.selected()) this.selected.set(at);
  }
}
