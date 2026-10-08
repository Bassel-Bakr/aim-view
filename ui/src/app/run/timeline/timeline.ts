/**
 * The timeline under a tracking run's video.
 *
 * In: the tracking report and its tracks (run.ts), the video's time (Playback), and the user's
 * pointer and keys.
 * Out: the timeline as SVG (its shapes from timeline-model.ts, its colors from the stylesheet) with the playhead
 * over it; the pointer and the keys seek the video.
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
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { TrackReport, Tracks } from '../../api';
import { Playback } from '../playback';
import { describe, timeline } from '../track';
import { timelineChart } from './timeline-model';

/** Seconds the arrow keys move. */
const STEP_SECONDS = 1;
/** Seconds Page Up and Page Down move. */
const PAGE_SECONDS = 10;

/**
 * A tracking run, moment by moment: how far outside the bot's edge the crosshair was, and whether
 * it was on the bot. A slider: click or drag to go there in the video, or use the arrow keys. The
 * playhead follows every frame outside change detection.
 */
@Component({
  selector: 'app-timeline',
  templateUrl: './timeline.html',
  styleUrl: './timeline.scss',
})
export class Timeline {
  /** The tracking run's report: its run window, switches and hitbox. */
  readonly report = input.required<TrackReport>();
  /** The review's tracks, each frame's targets. */
  readonly tracks = input.required<Tracks>();
  /** The video: its time moves the playhead, and the pointer and keys seek it. */
  private readonly playback = inject(Playback);
  /** Stops the frame callback and the listeners when the timeline goes. */
  private readonly destroyRef = inject(DestroyRef);
  /** The timeline's box: the slider that takes the pointer and the keys. */
  private readonly box = viewChild.required<ElementRef<HTMLElement>>('box');
  /** The playhead, moved on every frame outside the template. */
  private readonly head = viewChild.required<ElementRef<HTMLElement>>('head');
  /** The tip that describes the moment under the pointer. */
  private readonly tip = viewChild.required<ElementRef<HTMLElement>>('tip');

  /** The run moment by moment: each frame's state and distance off the bot, and the deaths. */
  protected readonly data = computed(() => timeline(this.report(), this.tracks()));
  /** The run's length in whole seconds, the slider's largest value. */
  protected readonly runSeconds = computed(() =>
    Math.round(this.data().frameCount / this.data().fps),
  );
  /** The chart's width on screen, in CSS pixels: the timeline is laid out per pixel column. */
  private readonly widthPx = signal(0);
  /** The timeline's shapes, once the chart has a width. */
  protected readonly chart = computed(() => {
    const widthPx = this.widthPx();
    return widthPx ? timelineChart(this.data(), widthPx) : null;
  });

  /** Starts following the video after the first render, and places the playhead when the chart changes. */
  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.chart();
      untracked(() => this.moveHead(this.playback.time));
    });
  }

  /**
   * The playhead and the slider's value follow the video; the mouse seeks and shows each moment.
   */
  private follow(): void {
    const box = this.box().nativeElement;
    const stop = this.playback.onFrame((seconds) => this.moveHead(seconds));
    const resize = new ResizeObserver(([entry]) =>
      this.widthPx.set(Math.round(entry.contentRect.width)),
    );
    resize.observe(box);
    let dragging = false;
    const down = (event: PointerEvent) => {
      dragging = true;
      box.setPointerCapture(event.pointerId);
      this.seekToPointer(event);
    };
    const move = (event: PointerEvent) => {
      if (dragging) this.seekToPointer(event);
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
      resize.disconnect();
      box.removeEventListener('pointerdown', down);
      box.removeEventListener('pointermove', move);
      box.removeEventListener('pointerup', up);
      box.removeEventListener('pointerleave', leave);
    });
  }

  /** The frame (counted from the run's start) under the pointer. */
  private frameAt(event: PointerEvent): number {
    const bounds = this.box().nativeElement.getBoundingClientRect();
    const run = this.data();
    return Math.min(
      run.frameCount - 1,
      Math.max(0, Math.floor(((event.clientX - bounds.left) / bounds.width) * run.frameCount)),
    );
  }

  /** Seeks the video to the middle of the frame under the pointer. */
  private seekToPointer(event: PointerEvent): void {
    const run = this.data();
    this.playback.seek((run.start + this.frameAt(event) + 0.5) / run.fps);
  }

  /**
   * Describes the moment under the pointer in the tip: its time, state and distance off the bot.
   */
  private showTip(event: PointerEvent): void {
    const tip = this.tip().nativeElement;
    const bounds = this.box().nativeElement.getBoundingClientRect();
    tip.textContent = describe(this.data(), this.frameAt(event));
    tip.hidden = false;
    tip.style.left = `${Math.min(bounds.width - tip.offsetWidth, Math.max(0, event.clientX - bounds.left))}px`;
  }

  /** The playhead's place, and the slider's value for screen readers while the video is paused. */
  private moveHead(seconds: number): void {
    const run = this.data();
    const frame = seconds * run.fps - run.start;
    const head = this.head().nativeElement;
    head.hidden = frame < 0 || frame > run.frameCount;
    head.style.left = `${(100 * frame) / run.frameCount}%`;
    if (this.playback.paused()) {
      const box = this.box().nativeElement;
      const at = Math.min(run.frameCount - 1, Math.max(0, Math.round(frame)));
      box.setAttribute('aria-valuenow', String(Math.round(at / run.fps)));
      box.setAttribute('aria-valuetext', describe(run, at));
    }
  }

  /**
   * The arrow keys move a second, Page Up and Down ten, Home and End to the run's start and end; a
   * key never seeks outside the run.
   */
  protected moveWithKeys(event: KeyboardEvent): void {
    const run = this.data();
    const now = this.playback.time;
    const start = run.start / run.fps;
    const end = (run.start + run.frameCount) / run.fps;
    const to: Record<string, number> = {
      ArrowRight: now + STEP_SECONDS,
      ArrowUp: now + STEP_SECONDS,
      ArrowLeft: now - STEP_SECONDS,
      ArrowDown: now - STEP_SECONDS,
      PageUp: now + PAGE_SECONDS,
      PageDown: now - PAGE_SECONDS,
      Home: start,
      End: end,
    };
    if (!(event.key in to)) return;
    event.preventDefault();
    this.playback.seek(Math.min(end, Math.max(start, to[event.key])));
  }
}
