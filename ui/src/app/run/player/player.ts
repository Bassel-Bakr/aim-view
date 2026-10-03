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
import { isClickReport, Report, Tracks } from '../../api';
import { Button } from '../../controls/button';
import { Playback, RATES } from '../playback';
import { FlickFocus } from '../flick-focus';
import { clock } from '../track';
import { PathCost } from '../fastest-path/path-cost';
import { FaintCutoff } from '../../services/faint-cutoff';
import { drawFaint, FaintStyle, pointedTrack, readFaintStyle } from '../faint-cutoff/faint-overlay';
import { drawClick, drawPaths, drawTrack, OverlayStyle, readOverlayStyle } from './overlay';

const OVERLAY_KEY = 'aimview-overlay';
const FASTEST_KEY = 'aimview-fastest';
const MINE_KEY = 'aimview-mine';
const RATE_LABELS: Record<number, string> = { 1: '1×', 0.5: '½×', 0.25: '¼×', 0.125: '⅛×' };
/** Keys typed into these are theirs. */
const FIELDS = 'input, textarea, select, dialog';
/** These also use Space and the arrows. */
const KEY_OWNERS = `${FIELDS}, [role=listbox], [role=slider]`;

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
 * The video with the review drawn over it, the seek bar and the playback controls, and the panels that work on the
 * video above it (projected with the `panel` attribute). The overlay, the clock and the seek bar follow every frame
 * through the video's frame callback, outside change detection. Full screen (the button, or F) fills the screen with
 * the video, its timeline and the controls, and puts the panels below them.
 */
@Component({
  selector: 'app-player',
  imports: [Button],
  templateUrl: './player.html',
  styleUrl: './player.scss',
  host: {
    '(document:keydown)': 'handleKeydown($event)',
    '(document:fullscreenchange)': 'followFullScreen()',
  },
})
export class Player {
  readonly src = input.required<string>();
  readonly report = input<Report | null>(null);
  readonly tracks = input<Tracks | null>(null);
  /** Something is edited on the video (the excluded areas, projected with the `screen` attribute): no overlay. */
  readonly editing = input(false);
  protected readonly playback = inject(Playback);
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);
  private readonly faint = inject(FaintCutoff);
  private readonly destroyRef = inject(DestroyRef);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly video = viewChild.required<ElementRef<HTMLVideoElement>>('video');
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('overlay');
  private readonly clockText = viewChild.required<ElementRef<HTMLElement>>('clock');
  private readonly seekBar = viewChild.required<ElementRef<HTMLInputElement>>('seek');

  protected readonly rates = RATES;
  protected readonly rateLabels = RATE_LABELS;
  protected readonly aspect = signal<string | null>(null);
  protected readonly showOverlay = signal(localStorage.getItem(OVERLAY_KEY) !== '0');
  protected readonly showFastest = signal(localStorage.getItem(FASTEST_KEY) === '1');
  protected readonly showMine = signal(localStorage.getItem(MINE_KEY) !== '0');
  /** The player fills the screen. */
  protected readonly full = signal(false);
  protected readonly clicking = computed(() => isClickReport(this.report()));
  protected readonly marks = computed(() => markPositions(this.report(), this.playback.duration()));
  /** The video's length, in whole seconds: the seek bar's end. */
  protected readonly end = computed(() =>
    clock(this.playback.duration() || 0).replace(/\.\d$/, ''),
  );
  private style: OverlayStyle | null = null;
  private faintStyle: FaintStyle | null = null;
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
      this.editing();
      this.faint.dropped();
      this.faint.highlight();
      this.faint.showScores();
      this.faint.hover();
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
    this.clockText().nativeElement.textContent = clock(t);
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
    if (!r || this.editing()) return;
    this.style ??= readOverlayStyle(canvas);
    const frame = Math.round(t * r.fps);
    const scale = w / r.geometry.W;
    const tracks = this.tracks();
    if (isClickReport(r)) {
      const paths = this.paths.analysis();
      const show = { fastest: this.showFastest(), mine: this.showMine() };
      if (tracks && paths && (show.fastest || show.mine)) {
        drawPaths(c, r, tracks, frame, scale, this.style, paths, show);
      }
      if (this.showOverlay()) drawClick(c, r, frame, scale, this.style);
    } else if (tracks && this.showOverlay()) {
      drawTrack(c, r, tracks, frame, scale, this.style);
    }
    this.drawCutoff(c, frame, scale);
  }

  /** What the faint-target cut-off leaves out, dimmed, and the scores when asked for (faint-overlay.ts). */
  private drawCutoff(c: CanvasRenderingContext2D, frame: number, scale: number): void {
    const r = this.report();
    const all = this.faint.allTracks();
    const sc = this.faint.scores();
    if (!r || !all || !sc || !this.faint.has()) return;
    this.faintStyle ??= readFaintStyle(this.canvas().nativeElement);
    drawFaint(c, r, all, frame, scale, this.faintStyle, {
      scores: sc.scores,
      dropped: this.faint.dropped(),
      highlight: this.faint.highlight(),
      showScores: this.faint.showScores(),
      hover: this.faint.hover(),
    });
  }

  /** The track under the mouse, with its scores, for the cut-off. */
  protected pointAt(e: MouseEvent): void {
    const r = this.report();
    const all = this.faint.allTracks();
    const sc = this.faint.scores();
    if (!r || !all || !sc || !this.faint.has() || this.editing()) return;
    const box = this.canvas().nativeElement.getBoundingClientRect();
    const frame = Math.round(this.playback.time * r.fps);
    const scale = box.width / r.geometry.W;
    const hover = pointedTrack(
      r,
      all,
      frame,
      scale,
      e.clientX - box.left,
      e.clientY - box.top,
      sc.scores,
    );
    if ((hover?.text ?? null) !== (this.faint.hover()?.text ?? null)) this.faint.hover.set(hover);
  }

  protected stopPointing(): void {
    if (this.faint.hover()) this.faint.hover.set(null);
  }

  protected loadedMetadata(): void {
    const v = this.video().nativeElement;
    this.playback.duration.set(v.duration);
    if (v.videoWidth && v.videoHeight) this.aspect.set(`${v.videoWidth} / ${v.videoHeight}`);
    if (this.playback.startAt !== null) {
      this.playback.seek(this.playback.startAt);
      this.playback.startAt = null;
    }
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

  /** Fills the screen with the player, or leaves full screen. Where the browser refuses, nothing changes. */
  protected toggleFullScreen(): void {
    if (this.full()) {
      void document.exitFullscreen().catch(() => undefined);
      return;
    }
    const host: Partial<HTMLElement> = this.host.nativeElement;
    void host.requestFullscreen?.().catch(() => undefined);
  }

  /**
   * Full screen started or ended (the browser's own Escape ends it too). It starts on the video, not where the panels
   * below it were scrolled to last time.
   */
  protected followFullScreen(): void {
    const host = this.host.nativeElement;
    this.full.set(document.fullscreenElement === host);
    if (this.full()) host.scrollTop = 0;
  }

  /**
   * Space plays or pauses; Left and Right step one frame; Shift with Left or Right replays the previous or next flick
   * (a tracking run: goes to the previous or next bot's death); F goes full screen or leaves it, and so does Escape
   * where the browser leaves it to the page. Keys typed into a field, and Space and the arrows used by a list or a
   * slider, are theirs.
   */
  protected handleKeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const target = e.target instanceof Element ? e.target : null;
    if (target?.closest(FIELDS)) return;
    if (e.key.toLowerCase() === 'f') {
      this.toggleFullScreen();
      return;
    }
    if (e.key === 'Escape') {
      // handled here: the excluded areas editor stays open
      if (this.full()) {
        e.preventDefault();
        this.toggleFullScreen();
      }
      return;
    }
    if (target?.closest(KEY_OWNERS)) return;
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
