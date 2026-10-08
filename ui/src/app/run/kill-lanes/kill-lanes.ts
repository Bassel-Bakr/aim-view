/**
 * The kill lanes under a clicking run's video.
 *
 * In: the clicking report, the video's time (Playback), the picked kill (FlickFocus) and the user's
 * pointer and arrow keys.
 * Out: the lanes as SVG (their shapes from kill-lanes-model.ts, their colors from the stylesheet) with the
 * playhead over them; a click plays a kill or seeks the video.
 */

import {
  afterNextRender,
  afterRenderEffect,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  input,
  untracked,
  viewChild,
} from '@angular/core';
import { ClickReport, Flick } from '../../api';
import { formatCount, formatDegrees, formatMs } from '../../format';
import { FlickFocus } from '../flick-focus';
import { Playback } from '../playback';
import { killLanes } from './kill-lanes-model';

/** How near a kill the pointer must be to pick it, in pixels. */
const PICK_DISTANCE = 6;
/** Seconds the arrow keys move. */
const STEP_SECONDS = 1;

/**
 * A clicking run's kills under the video, moment by moment: a mark at each kill, and under it the
 * kill's time as a bar (coral past a second). Click near a kill to play it, anywhere else to go
 * there; the playhead follows every frame outside change detection.
 */
@Component({
  selector: 'app-kill-lanes',
  templateUrl: './kill-lanes.html',
  styleUrl: './kill-lanes.scss',
})
export class KillLanes {
  /** The clicking run's report. */
  readonly report = input.required<ClickReport>();
  /** The video: its time moves the playhead, and a click seeks it. */
  private readonly playback = inject(Playback);
  /** The picked kill, drawn apart; a click near a kill plays it. */
  private readonly focus = inject(FlickFocus);
  /** Stops the frame callback and the listeners when the lanes go. */
  private readonly destroyRef = inject(DestroyRef);
  /** The lanes' box: the slider that takes the pointer and the keys. */
  private readonly box = viewChild.required<ElementRef<HTMLElement>>('box');
  /** The playhead, moved on every frame outside the template. */
  private readonly head = viewChild.required<ElementRef<HTMLElement>>('head');
  /** The tip that names the kill under the pointer. */
  private readonly tip = viewChild.required<ElementRef<HTMLElement>>('tip');

  /** How many kills the review measured, for the label. */
  protected readonly kills = computed(() => formatCount(this.report().flicks.length));
  /** The run's length in seconds: the video's, else to just after the last kill. */
  protected readonly seconds = computed(() => {
    const report = this.report();
    const last = report.flicks.at(-1);
    return (
      this.playback.duration() ||
      (last ? last.kill_frame / report.fps + STEP_SECONDS : STEP_SECONDS)
    );
  });
  /** The lanes' shapes: each kill's mark and bar, the picked one apart. */
  protected readonly lanes = computed(() =>
    killLanes(this.report(), this.seconds(), this.focus.selected()),
  );

  /** Starts following the video after the first render, and places the playhead when the run's length changes. */
  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.seconds();
      untracked(() => this.moveHead(this.playback.time));
    });
  }

  /** The playhead follows the video; the pointer plays a kill near it, or seeks. */
  private follow(): void {
    const box = this.box().nativeElement;
    const stop = this.playback.onFrame((seconds) => this.moveHead(seconds));
    let dragging = false;
    const down = (event: PointerEvent) => {
      const near = this.flickNear(event);
      if (near) {
        this.focus.play(near);
        return;
      }
      dragging = true;
      box.setPointerCapture(event.pointerId);
      this.playback.seek(this.secondsAt(event));
    };
    const move = (event: PointerEvent) => {
      if (dragging) this.playback.seek(this.secondsAt(event));
      this.showTip(event);
    };
    const up = () => (dragging = false);
    const leave = () => (this.tip().nativeElement.hidden = true);
    box.addEventListener('pointerdown', down);
    box.addEventListener('pointermove', move);
    box.addEventListener('pointerup', up);
    box.addEventListener('pointerleave', leave);
    this.destroyRef.onDestroy(() => {
      stop();
      box.removeEventListener('pointerdown', down);
      box.removeEventListener('pointermove', move);
      box.removeEventListener('pointerup', up);
      box.removeEventListener('pointerleave', leave);
    });
  }

  /** Puts the playhead at `seconds` into the run, and tells assistive tech the slider's value. */
  private moveHead(seconds: number): void {
    const head = this.head().nativeElement;
    head.hidden = false;
    head.style.left = `${Math.min(100, (100 * seconds) / this.seconds())}%`;
    this.box().nativeElement.setAttribute('aria-valuenow', String(Math.round(seconds)));
  }

  /** The seconds under the pointer. */
  private secondsAt(event: PointerEvent): number {
    const bounds = this.box().nativeElement.getBoundingClientRect();
    return Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)) * this.seconds();
  }

  /** The kill nearest the pointer, when it is near enough to pick. */
  private flickNear(event: PointerEvent): Flick | null {
    const box = this.box().nativeElement.getBoundingClientRect();
    const report = this.report();
    let best: Flick | null = null;
    let nearestPx = PICK_DISTANCE;
    for (const kill of report.flicks) {
      const gapPx = Math.abs(
        box.left + (kill.kill_frame / report.fps / this.seconds()) * box.width - event.clientX,
      );
      if (gapPx <= nearestPx) {
        best = kill;
        nearestPx = gapPx;
      }
    }
    return best;
  }

  /** Names the kill under the pointer (its TTK and distance) in the tip, or hides the tip. */
  private showTip(event: PointerEvent): void {
    const tip = this.tip().nativeElement;
    const near = this.flickNear(event);
    tip.hidden = !near;
    if (!near) return;
    tip.textContent = `Kill ${near.kill_number} · ${formatMs(near.total)} · ${formatDegrees(near.D0, 1)} · click to play it`;
    const bounds = this.box().nativeElement.getBoundingClientRect();
    tip.style.left = `${Math.min(bounds.width - tip.offsetWidth, Math.max(0, event.clientX - bounds.left))}px`;
  }

  /** Left and Right seek the video a second back or on. */
  protected moveWithKeys(event: KeyboardEvent): void {
    const by =
      event.key === 'ArrowLeft' ? -STEP_SECONDS : event.key === 'ArrowRight' ? STEP_SECONDS : 0;
    if (!by) return;
    event.preventDefault();
    this.playback.seek(this.playback.time + by);
  }
}
