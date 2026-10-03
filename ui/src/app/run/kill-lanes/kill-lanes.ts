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

/** A kill time over this is drawn in the attention color, in seconds. */
const LONG_KILL = 1;
/** The kill time that fills the bars' lane, in seconds: longer ones are cut at the top. */
const TALLEST_KILL = 2;
/** How near a kill the pointer must be to pick it, in pixels. */
const PICK_DISTANCE = 6;
/** Seconds the arrow keys move. */
const STEP = 1;

/** The lanes' drawing: the kills' marks above, each kill's time as a bar below, both at the kill's moment. */
interface LaneStyle {
  kill: string;
  picked: string;
  quiet: string;
  long: string;
  grid: string;
  markHeight: number;
  barWidth: number;
  split: number;
}

function readStyle(el: Element): LaneStyle {
  const css = getComputedStyle(el);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    kill: v('--accent'),
    picked: v('--text-primary'),
    quiet: v('--series-quiet'),
    long: v('--attention'),
    grid: v('--grid'),
    markHeight: Number(v('--kill-lanes-mark')),
    barWidth: Number(v('--kill-lanes-bar')),
    split: Number(v('--kill-lanes-split')),
  };
}

/**
 * A clicking run's kills under the video, moment by moment: a mark at each kill, and under it the kill's time as a
 * bar (coral past a second). Click near a kill to play it, anywhere else to go there; the playhead follows every frame
 * outside change detection.
 */
@Component({
  selector: 'app-kill-lanes',
  templateUrl: './kill-lanes.html',
  styleUrl: './kill-lanes.scss',
})
export class KillLanes {
  readonly report = input.required<ClickReport>();
  private readonly playback = inject(Playback);
  private readonly focus = inject(FlickFocus);
  private readonly destroyRef = inject(DestroyRef);
  private readonly box = viewChild.required<ElementRef<HTMLElement>>('box');
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('lanes');
  private readonly head = viewChild.required<ElementRef<HTMLElement>>('head');
  private readonly tip = viewChild.required<ElementRef<HTMLElement>>('tip');

  protected readonly kills = computed(() => formatCount(this.report().flicks.length));
  /** The run's length in seconds: the video's, else to just after the last kill. */
  protected readonly seconds = computed(() => {
    const r = this.report();
    const last = r.flicks.at(-1);
    return this.playback.duration() || (last ? last.kill_frame / r.fps + STEP : STEP);
  });
  private style: LaneStyle | null = null;

  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.report();
      this.seconds();
      this.focus.selected();
      untracked(() => this.draw());
    });
  }

  /** The playhead follows the video; the pointer plays a kill near it, or seeks. */
  private follow(): void {
    const box = this.box().nativeElement;
    const stop = this.playback.onFrame((t) => this.moveHead(t));
    const resize = new ResizeObserver(() => this.draw());
    resize.observe(box);
    let dragging = false;
    const down = (e: PointerEvent) => {
      const near = this.flickNear(e);
      if (near) {
        this.focus.play(near);
        return;
      }
      dragging = true;
      box.setPointerCapture(e.pointerId);
      this.playback.seek(this.secondsAt(e));
    };
    const move = (e: PointerEvent) => {
      if (dragging) this.playback.seek(this.secondsAt(e));
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
    const s = (this.style ??= readStyle(canvas));
    const r = this.report();
    const picked = this.focus.selected();
    c.fillStyle = s.grid;
    c.fillRect(0, s.split, w, 1);
    const markTop = (s.split - s.markHeight) / 2;
    const barsHeight = h - s.split - 1;
    for (const f of r.flicks) {
      const at = Math.round((f.kill_frame / r.fps / this.seconds()) * w);
      c.fillStyle = f === picked ? s.picked : s.kill;
      c.fillRect(at - 1, markTop, 2, s.markHeight);
      const bar = Math.max(1, (Math.min(f.total, TALLEST_KILL) / TALLEST_KILL) * barsHeight);
      c.fillStyle = f === picked ? s.picked : f.total > LONG_KILL ? s.long : s.quiet;
      c.fillRect(at - s.barWidth / 2, h - bar, s.barWidth, bar);
    }
    this.moveHead(this.playback.time);
  }

  private moveHead(seconds: number): void {
    const head = this.head().nativeElement;
    head.hidden = false;
    head.style.left = `${Math.min(100, (100 * seconds) / this.seconds())}%`;
    this.box().nativeElement.setAttribute('aria-valuenow', String(Math.round(seconds)));
  }

  /** The seconds under the pointer. */
  private secondsAt(e: PointerEvent): number {
    const r = this.box().nativeElement.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * this.seconds();
  }

  /** The kill nearest the pointer, when it is near enough to pick. */
  private flickNear(e: PointerEvent): Flick | null {
    const box = this.box().nativeElement.getBoundingClientRect();
    const r = this.report();
    let best: Flick | null = null;
    let gap = PICK_DISTANCE;
    for (const f of r.flicks) {
      const d = Math.abs(
        box.left + (f.kill_frame / r.fps / this.seconds()) * box.width - e.clientX,
      );
      if (d <= gap) {
        best = f;
        gap = d;
      }
    }
    return best;
  }

  private showTip(e: PointerEvent): void {
    const tip = this.tip().nativeElement;
    const near = this.flickNear(e);
    tip.hidden = !near;
    if (!near) return;
    tip.textContent = `Kill ${near.n} · ${formatMs(near.total)} · ${formatDegrees(near.D0, 1)} · click to play it`;
    const r = this.box().nativeElement.getBoundingClientRect();
    tip.style.left = `${Math.min(r.width - tip.offsetWidth, Math.max(0, e.clientX - r.left))}px`;
  }

  protected moveWithKeys(e: KeyboardEvent): void {
    const by = e.key === 'ArrowLeft' ? -STEP : e.key === 'ArrowRight' ? STEP : 0;
    if (!by) return;
    e.preventDefault();
    this.playback.seek(this.playback.time + by);
  }
}
