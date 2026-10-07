/**
 * The flick in focus on a clicking run, shared by the parts of the run page that show flicks. In:
 * the review's click report (its flicks and fps), the video's frames (Playback) and the user's
 * picks. Out: the click report's parts (the flick list, the flick profile, the speed chart, the
 * kill lanes, the run charts, the click side panel) and the player, which show it and step through
 * the flicks.
 */

import { computed, effect, inject, Service, signal, untracked } from '@angular/core';
import { ClickReport, Flick, isClickReport } from '../api';
import { Library } from '../services/library';
import { Playback } from './playback';
import { Review } from '../services/review';
import { flickAt } from './track';

/** The local storage key that keeps the "follow the video" switch across visits ('0' is off). */
const FOLLOW_KEY = 'aimview-follow';
/** A replayed flick starts this long before the flick, in seconds. */
const BEFORE_FLICK = 0.15;
/** A replayed flick stops this long after its kill, in seconds. */
const AFTER_KILL = 1;

/**
 * The flick in focus on a clicking run: the one picked in the list, or, with "follow the video" on,
 * the one on screen. Following runs on every frame but sets the signal only when the flick changes.
 * A replay keeps its flick.
 */
@Service()
export class FlickFocus {
  /** The video, whose frames the focus follows and which replays a flick. */
  private readonly playback = inject(Playback);
  /** The selected recording's review, whose report gives the flicks. */
  private readonly review = inject(Review);
  /** The flick in focus, or null for none; cleared when another recording is picked. */
  readonly selected = signal<Flick | null>(null);
  /** Whether the focus follows the flick on screen as the video plays (on unless turned off). */
  readonly follow = signal(localStorage.getItem(FOLLOW_KEY) !== '0');

  /** The review's report when it is a clicking run's, else null (a tracking run has no flicks). */
  private readonly report = computed<ClickReport | null>(() => {
    const report = this.review.report.hasValue() ? this.review.report.value() : null;
    return isClickReport(report) ? report : null;
  });

  /** Clears the focus when the selected recording changes, and follows the video's frames. */
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

  /**
   * Turns "follow the video" on or off and keeps the choice; turning it on focuses the flick on
   * screen at once.
   */
  setFollow(on: boolean): void {
    this.follow.set(on);
    localStorage.setItem(FOLLOW_KEY, on ? '1' : '0');
    if (on) this.followVideo(this.playback.time);
  }

  /**
   * Focuses the flick at the frame on screen (time in seconds), when following is on and no replay
   * runs; a frame before the first flick keeps the focus where it was.
   */
  private followVideo(seconds: number): void {
    const report = this.report();
    if (!report || !this.follow() || this.playback.replaying) return;
    const at = flickAt(report.flicks, Math.round(seconds * report.fps));
    if (at && at !== this.selected()) this.selected.set(at);
  }
}
