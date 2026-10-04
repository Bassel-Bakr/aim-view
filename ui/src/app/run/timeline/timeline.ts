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
import { TrackReport, Tracks } from '../../api';
import { Playback } from '../playback';
import { describe, timeline } from '../track';
import { drawTimeline, readTimelineStyle, TimelineStyle } from './timeline-drawing';

/** Seconds the arrow keys move, and Page Up or Down. */
const STEP_SECONDS = 1;
const PAGE_SECONDS = 10;

/**
 * A tracking run, moment by moment: how far outside the bot's edge the crosshair was, and whether it was on the bot.
 * A slider: click or drag to go there in the video, or use the arrow keys. The playhead follows every frame outside
 * change detection.
 */
@Component({
  selector: 'app-timeline',
  templateUrl: './timeline.html',
  styleUrl: './timeline.scss',
})
export class Timeline {
  readonly report = input.required<TrackReport>();
  readonly tracks = input.required<Tracks>();
  private readonly playback = inject(Playback);
  private readonly destroyRef = inject(DestroyRef);
  private readonly box = viewChild.required<ElementRef<HTMLElement>>('box');
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('chart');
  private readonly head = viewChild.required<ElementRef<HTMLElement>>('head');
  private readonly tip = viewChild.required<ElementRef<HTMLElement>>('tip');

  protected readonly data = computed(() => timeline(this.report(), this.tracks()));
  protected readonly runSeconds = computed(() =>
    Math.round(this.data().frameCount / this.data().fps),
  );
  private style: TimelineStyle | null = null;

  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.data();
      untracked(() => this.draw());
    });
  }

  /** The playhead and the slider's value follow the video; the mouse seeks and shows each moment. */
  private follow(): void {
    const box = this.box().nativeElement;
    const stop = this.playback.onFrame((seconds) => this.moveHead(seconds));
    const resize = new ResizeObserver(() => this.draw());
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

  private draw(): void {
    const canvas = this.canvas().nativeElement;
    const widthPx = canvas.clientWidth;
    const heightPx = canvas.clientHeight;
    if (!widthPx) return;
    const pixelRatio = devicePixelRatio || 1;
    canvas.width = Math.round(widthPx * pixelRatio);
    canvas.height = Math.round(heightPx * pixelRatio);
    const context = canvas.getContext('2d');
    if (!context) return;
    context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    this.style ??= readTimelineStyle(canvas);
    drawTimeline(context, this.data(), widthPx, heightPx, this.style);
    this.moveHead(this.playback.time);
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

  private seekToPointer(event: PointerEvent): void {
    const run = this.data();
    this.playback.seek((run.start + this.frameAt(event) + 0.5) / run.fps);
  }

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

  /** The arrow keys move a second, Page Up and Down ten, Home and End to the run's start and end. */
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
