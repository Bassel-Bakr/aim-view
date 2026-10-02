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
import { timelineStyles } from '@themes/timeline.styles';
import { slotClasses } from '@themes/slot-classes';

/** Seconds the arrow keys move, and Page Up or Down. */
const STEP = 1;
const PAGE = 10;

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

  protected readonly ui = slotClasses(timelineStyles());
  protected readonly data = computed(() => timeline(this.report(), this.tracks()));
  protected readonly runSeconds = computed(() => Math.round(this.data().n / this.data().fps));
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
    const stop = this.playback.onFrame((t) => this.moveHead(t));
    const resize = new ResizeObserver(() => this.draw());
    resize.observe(box);
    let dragging = false;
    const down = (e: PointerEvent) => {
      dragging = true;
      box.setPointerCapture(e.pointerId);
      this.seekToPointer(e);
    };
    const move = (e: PointerEvent) => {
      if (dragging) this.seekToPointer(e);
      this.showTip(e);
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
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (!w) return;
    const dpr = devicePixelRatio || 1;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const c = canvas.getContext('2d');
    if (!c) return;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.style ??= readTimelineStyle(canvas);
    drawTimeline(c, this.data(), w, h, this.style);
    this.moveHead(this.playback.time);
  }

  /** The frame (counted from the run's start) under the pointer. */
  private frameAt(e: PointerEvent): number {
    const r = this.box().nativeElement.getBoundingClientRect();
    const tl = this.data();
    return Math.min(tl.n - 1, Math.max(0, Math.floor(((e.clientX - r.left) / r.width) * tl.n)));
  }

  private seekToPointer(e: PointerEvent): void {
    const tl = this.data();
    this.playback.seek((tl.start + this.frameAt(e) + 0.5) / tl.fps);
  }

  private showTip(e: PointerEvent): void {
    const tip = this.tip().nativeElement;
    const r = this.box().nativeElement.getBoundingClientRect();
    tip.textContent = describe(this.data(), this.frameAt(e));
    tip.hidden = false;
    tip.style.left = `${Math.min(r.width - tip.offsetWidth, Math.max(0, e.clientX - r.left))}px`;
  }

  /** The playhead's place, and the slider's value for screen readers while the video is paused. */
  private moveHead(t: number): void {
    const tl = this.data();
    const k = t * tl.fps - tl.start;
    const head = this.head().nativeElement;
    head.hidden = k < 0 || k > tl.n;
    head.style.left = `${(100 * k) / tl.n}%`;
    if (this.playback.paused()) {
      const box = this.box().nativeElement;
      const at = Math.min(tl.n - 1, Math.max(0, Math.round(k)));
      box.setAttribute('aria-valuenow', String(Math.round(at / tl.fps)));
      box.setAttribute('aria-valuetext', describe(tl, at));
    }
  }

  /** The arrow keys move a second, Page Up and Down ten, Home and End to the run's start and end. */
  protected moveWithKeys(e: KeyboardEvent): void {
    const tl = this.data();
    const now = this.playback.time;
    const start = tl.start / tl.fps;
    const end = (tl.start + tl.n) / tl.fps;
    const to: Record<string, number> = {
      ArrowRight: now + STEP,
      ArrowUp: now + STEP,
      ArrowLeft: now - STEP,
      ArrowDown: now - STEP,
      PageUp: now + PAGE,
      PageDown: now - PAGE,
      Home: start,
      End: end,
    };
    if (!(e.key in to)) return;
    e.preventDefault();
    this.playback.seek(Math.min(end, Math.max(start, to[e.key])));
  }
}
