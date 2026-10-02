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
import { Report, Tracks } from '../../api';
import { Playback, RATES } from '../playback';
import { FlickFocus } from '../flick-focus';
import { clock } from '../track';
import { PathCost } from '../fastest-path/path-cost';
import { drawClick, drawPaths, drawTrack, OverlayStyle, readOverlayStyle } from './overlay';
import { button, segmented, toggleSwitch } from '@themes/controls.styles';
import { playerStyles } from '@themes/player.styles';
import { slotClasses } from '@themes/slot-classes';

const OVERLAY_KEY = 'aimview-overlay';
const FASTEST_KEY = 'aimview-fastest';
const MINE_KEY = 'aimview-mine';
const RATE_LABELS: Record<number, string> = { 1: '1×', 0.5: '½×', 0.25: '¼×', 0.125: '⅛×' };

/**
 * Calls back with the time of each frame the video shows, through the video's own frame callback; where the browser
 * has none, once per screen refresh. Returns the function that stops it.
 */
export function everyFrame(video: HTMLVideoElement, shown: (seconds: number) => void): () => void {
  let handle = 0;
  const optional: Partial<HTMLVideoElement> = video;
  if (optional.requestVideoFrameCallback) {
    const tick: VideoFrameRequestCallback = (_now, meta) => {
      shown(meta.mediaTime);
      handle = video.requestVideoFrameCallback(tick);
    };
    handle = video.requestVideoFrameCallback(tick);
    return () => video.cancelVideoFrameCallback(handle);
  }
  const loop = () => {
    if (!video.paused) shown(video.currentTime);
    handle = requestAnimationFrame(loop);
  };
  handle = requestAnimationFrame(loop);
  return () => cancelAnimationFrame(handle);
}

/** Where the seek bar marks the run's moments, as shares of the video (0 to 100): kills, or a tracking run's deaths. */
export function markPositions(report: Report | null, duration: number): number[] {
  if (!report || !(duration > 0)) return [];
  const frames =
    report.mode === 'track'
      ? report.summary.switches.map(([start]) => start)
      : report.flicks.map((f) => f.kill_frame);
  return frames.map((f) => (100 * f) / report.fps / duration);
}

/**
 * The video with the review drawn over it, the seek bar and the playback controls. The overlay, the clock and the
 * seek bar follow every frame through the video's frame callback, outside change detection.
 */
@Component({
  selector: 'app-player',
  templateUrl: './player.html',
  styleUrl: './player.scss',
  host: { '(document:keydown)': 'handleKeydown($event)' },
})
export class Player {
  readonly src = input.required<string>();
  readonly report = input<Report | null>(null);
  readonly tracks = input<Tracks | null>(null);
  protected readonly playback = inject(Playback);
  private readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);
  private readonly destroyRef = inject(DestroyRef);
  private readonly video = viewChild.required<ElementRef<HTMLVideoElement>>('video');
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('overlay');
  private readonly clockText = viewChild.required<ElementRef<HTMLElement>>('clock');
  private readonly seekBar = viewChild.required<ElementRef<HTMLInputElement>>('seek');

  protected readonly ui = slotClasses(playerStyles());
  protected readonly button = button();
  protected readonly speed = slotClasses(segmented());
  protected readonly toggle = slotClasses(toggleSwitch());
  protected readonly rates = RATES;
  protected readonly rateLabels = RATE_LABELS;
  protected readonly aspect = signal<string | null>(null);
  protected readonly showOverlay = signal(localStorage.getItem(OVERLAY_KEY) !== '0');
  protected readonly showFastest = signal(localStorage.getItem(FASTEST_KEY) === '1');
  protected readonly showMine = signal(localStorage.getItem(MINE_KEY) !== '0');
  protected readonly clicking = computed(() => this.report()?.mode === 'click');
  protected readonly marks = computed(() => markPositions(this.report(), this.playback.duration()));
  private style: OverlayStyle | null = null;
  private seeking = false;

  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.report();
      this.tracks();
      this.showOverlay();
      this.showFastest();
      this.showMine();
      this.paths.analysis();
      untracked(() => this.draw(this.playback.time));
    });
  }

  /** Starts following the video's frames: the overlay, the clock and the seek bar redraw on each one. */
  private follow(): void {
    const video = this.video().nativeElement;
    this.playback.attach(video);
    const stop = this.playback.onFrame((t) => this.draw(t));
    const cancel = everyFrame(video, (t) => this.playback.frame(t));
    const resize = new ResizeObserver(() => this.draw(this.playback.time));
    resize.observe(this.canvas().nativeElement);
    this.destroyRef.onDestroy(() => {
      cancel();
      resize.disconnect();
      stop();
      this.playback.attach(null);
    });
  }

  private draw(t: number): void {
    const video = this.video().nativeElement;
    this.clockText().nativeElement.textContent = `${clock(t)} / ${clock(video.duration || 0)}`;
    if (!this.seeking) this.seekBar().nativeElement.value = String(t);
    const canvas = this.canvas().nativeElement;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    const dpr = devicePixelRatio || 1;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const c = canvas.getContext('2d');
    if (!c) return;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.clearRect(0, 0, w, h);
    const r = this.report();
    if (!r) return;
    this.style ??= readOverlayStyle(canvas);
    const frame = Math.round(t * r.fps);
    const scale = w / r.geometry.W;
    const tracks = this.tracks();
    if (r.mode === 'click') {
      const paths = this.paths.analysis();
      const show = { fastest: this.showFastest(), mine: this.showMine() };
      if (tracks && paths && (show.fastest || show.mine)) {
        drawPaths(c, r, tracks, frame, scale, this.style, paths, show);
      }
      if (this.showOverlay()) drawClick(c, r, frame, scale, this.style);
    } else if (tracks && this.showOverlay()) {
      drawTrack(c, r, tracks, frame, scale, this.style);
    }
  }

  protected loadedMetadata(): void {
    const v = this.video().nativeElement;
    this.playback.duration.set(v.duration);
    if (v.videoWidth && v.videoHeight) this.aspect.set(`${v.videoWidth} / ${v.videoHeight}`);
    this.playback.frame(v.currentTime);
  }

  protected showSeekedFrame(): void {
    this.playback.frame(this.video().nativeElement.currentTime);
  }

  protected startSeeking(): void {
    this.seeking = true;
  }

  protected seekTo(seconds: number): void {
    this.playback.seek(seconds);
  }

  protected stopSeeking(): void {
    this.seeking = false;
  }

  protected toggleOverlay(): void {
    this.showOverlay.update((on) => !on);
    localStorage.setItem(OVERLAY_KEY, this.showOverlay() ? '1' : '0');
  }

  protected toggleFastest(): void {
    this.showFastest.update((on) => !on);
    localStorage.setItem(FASTEST_KEY, this.showFastest() ? '1' : '0');
  }

  protected toggleMine(): void {
    this.showMine.update((on) => !on);
    localStorage.setItem(MINE_KEY, this.showMine() ? '1' : '0');
  }

  /**
   * Space plays or pauses; Left and Right step one frame; Shift with Left or Right replays the previous or next flick
   * (a tracking run: goes to the previous or next bot's death). Keys typed into a field or used by a list or slider
   * are theirs.
   */
  protected handleKeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const owners = 'input, textarea, select, dialog, [role=listbox], [role=slider]';
    if (e.target instanceof Element && e.target.closest(owners)) return;
    const fps = this.report()?.fps ?? 60;
    if (e.key === ' ') {
      e.preventDefault();
      this.playback.toggle();
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      const forward = e.key === 'ArrowRight';
      if (e.shiftKey) this.jump(forward);
      else this.playback.step(forward ? 1 : -1, fps);
    }
  }

  /** The previous or next flick, replayed; a tracking run: the previous or next bot's death. */
  private jump(forward: boolean): void {
    const r = this.report();
    if (!r) return;
    const frame = Math.round(this.playback.time * r.fps);
    if (r.mode === 'track') {
      const deaths = r.summary.switches.map(([start]) => start);
      const to = forward
        ? deaths.find((d) => d > frame)
        : [...deaths].reverse().find((d) => d < frame);
      if (to !== undefined) this.playback.seek(to / r.fps);
      return;
    }
    this.focus.step(forward);
  }
}
